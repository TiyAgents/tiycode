//! Unified policy evaluation engine.
//!
//! Single security boundary for all tool execution. Evaluates:
//! 1. Built-in dangerous pattern hard deny
//! 2. User deny list
//! 3. Workspace boundary check
//! 4. Run mode restrictions (plan mode blocks hard-deny mutations; shell follows normal approval)
//! 5. User allow list
//! 6. Approval policy fallback

use serde::Serialize;
use sqlx::SqlitePool;

use crate::core::workspace_paths::{
    canonicalize_workspace_root, normalize_additional_roots, resolve_path_within_roots,
};
use crate::extensions::ToolProviderContext;
use crate::model::errors::AppError;
use crate::persistence::repo::settings_repo;

// ---------------------------------------------------------------------------
// Policy verdict
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PolicyVerdict {
    AutoAllow,
    RequireApproval { reason: String },
    Deny { reason: String },
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PolicyCheck {
    pub tool_name: String,
    pub verdict: PolicyVerdict,
    pub checked_rules: Vec<String>,
}

// ---------------------------------------------------------------------------
// Tool classification
// ---------------------------------------------------------------------------

/// Tools that mutate the workspace or system.
const MUTATING_TOOLS: &[&str] = &[
    "write",
    "edit",
    "patch",
    "shell",
    "git_add",
    "git_stage",
    "git_unstage",
    "git_commit",
    "git_push",
    "git_pull",
    "git_fetch",
    "term_write",
    "term_restart",
    "term_close",
    "market_install",
];

/// Tools that are hard-denied in plan mode.
/// Shell is intentionally excluded — it follows the normal approval policy so that
/// read-only commands (git log, npm ls, command -v, skill CLIs, etc.) remain
/// available for information gathering in plan mode.
const PLAN_HARD_DENY_TOOLS: &[&str] = &[
    "write",
    "edit",
    "patch",
    "git_add",
    "git_stage",
    "git_unstage",
    "git_commit",
    "git_push",
    "git_pull",
    "git_fetch",
    "term_write",
    "term_restart",
    "term_close",
    "market_install",
];

/// Tools that are read-only and generally safe.
const READ_ONLY_TOOLS: &[&str] = &[
    "read",
    "list",
    "find",
    "search",
    "git_status",
    "git_diff",
    "git_log",
    "term_status",
    "term_output",
];

// ---------------------------------------------------------------------------
// Built-in dangerous patterns for shell
// ---------------------------------------------------------------------------

const DANGEROUS_COMMAND_PATTERNS: &[&str] = &[
    "rm -rf /",
    "rm -rf /\\**",
    "rm -rf ~",
    "rm -rf ~/\\**",
    "sudo *",
    "mkfs*",
    "dd if=*",
    "curl*|*sh",
    "wget*|*sh",
    "curl*|*bash",
    "wget*|*bash",
    "* > /dev/sd*",
    "*> /dev/sd*",
    "*>/dev/sd*",
    "chmod 777 /",
    "chmod 777 /\\**",
    ":(){ :|:& };:",
];

// ---------------------------------------------------------------------------
// PolicyEngine
// ---------------------------------------------------------------------------

pub struct PolicyEngine {
    pool: SqlitePool,
}

#[derive(Debug, Clone)]
struct EffectivePolicyRule {
    tool: String,
    pattern: String,
}

impl PolicyEngine {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    /// Evaluate whether a tool call should be allowed, require approval, or be denied.
    pub async fn evaluate(
        &self,
        tool_name: &str,
        tool_input: &serde_json::Value,
        workspace_canonical_path: Option<&str>,
        writable_roots: &[String],
        run_mode: &str,
        provider_context: Option<&ToolProviderContext>,
    ) -> Result<PolicyCheck, AppError> {
        let mut checked = Vec::new();

        // 1. Built-in dangerous pattern check (shell only)
        if tool_name == "shell" {
            checked.push("builtin_dangerous_patterns".to_string());
            if let Some(cmd) = tool_input["command"].as_str() {
                let cmd_lower = cmd.to_lowercase();
                for pattern in DANGEROUS_COMMAND_PATTERNS {
                    if shell_command_matches_pattern(&cmd_lower, pattern) {
                        return Ok(PolicyCheck {
                            tool_name: tool_name.to_string(),
                            verdict: PolicyVerdict::Deny {
                                reason: format!("Command matches dangerous pattern: {pattern}"),
                            },
                            checked_rules: checked,
                        });
                    }
                }
            }
        }

        // 2. User deny list
        checked.push("user_deny_list".to_string());
        if let Some(deny_list) = self.load_policy_list("deny_list").await? {
            for rule in &deny_list {
                if let Some(parsed_rule) = effective_policy_rule(rule) {
                    if parsed_rule.tool == tool_name || parsed_rule.tool == "*" {
                        if parsed_rule.pattern.is_empty()
                            || input_matches_pattern(tool_name, tool_input, &parsed_rule.pattern)
                        {
                            return Ok(PolicyCheck {
                                tool_name: tool_name.to_string(),
                                verdict: PolicyVerdict::Deny {
                                    reason: format!(
                                        "Denied by user deny list rule: {}/{}",
                                        parsed_rule.tool, parsed_rule.pattern
                                    ),
                                },
                                checked_rules: checked,
                            });
                        }
                    }
                }
            }
        }

        // 3. Workspace boundary check for file-related tools
        if let Some(ws_path) = workspace_canonical_path {
            checked.push("workspace_boundary".to_string());
            let workspace_root = canonicalize_workspace_root(
                ws_path,
                crate::model::errors::ErrorSource::Tool,
                "tool.workspace.not_directory",
            )?;
            let additional_roots = allowed_additional_roots(tool_name, writable_roots);
            let target_path = extract_target_path(tool_name, tool_input);
            if let Some(target) = target_path {
                if let Err(error) = resolve_path_within_roots(
                    &workspace_root,
                    &additional_roots,
                    &target,
                    crate::model::errors::ErrorSource::Tool,
                    "tool.path.outside_workspace",
                    format!("Path '{target}' is outside workspace boundary '{ws_path}'"),
                ) {
                    if error.error_code == "tool.path.outside_workspace" {
                        return Ok(PolicyCheck {
                            tool_name: tool_name.to_string(),
                            verdict: PolicyVerdict::Deny {
                                reason: error.user_message,
                            },
                            checked_rules: checked,
                        });
                    }

                    return Err(error);
                }
            }
        }

        // 4. Run mode restriction (plan mode blocks hard-deny mutations;
        //    shell is excluded so it falls through to the normal approval policy)
        if run_mode == "plan" && PLAN_HARD_DENY_TOOLS.contains(&tool_name) {
            checked.push("plan_mode_restriction".to_string());
            return Ok(PolicyCheck {
                tool_name: tool_name.to_string(),
                verdict: PolicyVerdict::Deny {
                    reason: "Mutating tools are blocked in plan mode".to_string(),
                },
                checked_rules: checked,
            });
        }

        // 5. User allow list (auto-allow matching rules)
        checked.push("user_allow_list".to_string());
        if let Some(allow_list) = self.load_policy_list("allow_list").await? {
            for rule in &allow_list {
                if let Some(parsed_rule) = effective_policy_rule(rule) {
                    if parsed_rule.tool == tool_name || parsed_rule.tool == "*" {
                        if !parsed_rule.pattern.is_empty()
                            && !input_matches_pattern(tool_name, tool_input, &parsed_rule.pattern)
                        {
                            continue;
                        }

                        return Ok(PolicyCheck {
                            tool_name: tool_name.to_string(),
                            verdict: PolicyVerdict::AutoAllow,
                            checked_rules: checked,
                        });
                    }
                }
            }
        }

        // 6. Extension provider heuristics
        if let Some(provider_context) = provider_context {
            checked.push("extension_provider".to_string());
            if provider_context.provider_type == "plugin" {
                return Ok(PolicyCheck {
                    tool_name: tool_name.to_string(),
                    verdict: match provider_context.required_permission.as_str() {
                        "write" | "exec" => PolicyVerdict::RequireApproval {
                            reason: format!(
                                "Plugin '{}' requires approval for '{}' access",
                                provider_context.provider_id, provider_context.required_permission
                            ),
                        },
                        _ => PolicyVerdict::AutoAllow,
                    },
                    checked_rules: checked,
                });
            }

            if provider_context.provider_type == "mcp" {
                return Ok(PolicyCheck {
                    tool_name: tool_name.to_string(),
                    verdict: PolicyVerdict::AutoAllow,
                    checked_rules: checked,
                });
            }
        }

        // 7. Read-only tools default to auto-allow
        if READ_ONLY_TOOLS.contains(&tool_name) {
            checked.push("read_only_default".to_string());
            return Ok(PolicyCheck {
                tool_name: tool_name.to_string(),
                verdict: PolicyVerdict::AutoAllow,
                checked_rules: checked,
            });
        }

        // 8. Approval policy fallback
        checked.push("approval_policy".to_string());
        let approval_mode = self.load_approval_mode().await?;

        let verdict = match approval_mode.as_str() {
            "auto" => PolicyVerdict::AutoAllow,
            "require_all" => PolicyVerdict::RequireApproval {
                reason: "Approval required by policy (require_all)".to_string(),
            },
            "require_for_mutations" | _ => {
                if MUTATING_TOOLS.contains(&tool_name) {
                    PolicyVerdict::RequireApproval {
                        reason: format!("'{tool_name}' is a mutating tool requiring approval"),
                    }
                } else {
                    PolicyVerdict::AutoAllow
                }
            }
        };

        Ok(PolicyCheck {
            tool_name: tool_name.to_string(),
            verdict,
            checked_rules: checked,
        })
    }

    // -----------------------------------------------------------------------
    // Internal helpers
    // -----------------------------------------------------------------------

    async fn load_policy_list(
        &self,
        key: &str,
    ) -> Result<Option<Vec<serde_json::Value>>, AppError> {
        let record = settings_repo::policy_get(&self.pool, key).await?;
        match record {
            Some(r) => {
                let list: Vec<serde_json::Value> =
                    serde_json::from_str(&r.value_json).unwrap_or_default();
                if list.is_empty() {
                    Ok(None)
                } else {
                    Ok(Some(list))
                }
            }
            None => Ok(None),
        }
    }

    async fn load_approval_mode(&self) -> Result<String, AppError> {
        let record = settings_repo::policy_get(&self.pool, "approval_policy").await?;
        match record {
            Some(r) => {
                let val: serde_json::Value =
                    serde_json::from_str(&r.value_json).unwrap_or_default();
                if let Some(mode) = val.as_str() {
                    Ok(mode.to_string())
                } else {
                    Ok(val["mode"]
                        .as_str()
                        .unwrap_or("require_for_mutations")
                        .to_string())
                }
            }
            None => Ok("require_for_mutations".to_string()),
        }
    }
}

fn allowed_additional_roots(tool_name: &str, writable_roots: &[String]) -> Vec<std::path::PathBuf> {
    if tool_uses_writable_roots(tool_name) {
        normalize_additional_roots(writable_roots)
    } else {
        Vec::new()
    }
}

fn tool_uses_writable_roots(tool_name: &str) -> bool {
    matches!(
        tool_name,
        "write" | "edit" | "patch" | "read" | "list" | "find" | "search"
    )
}

/// Extract the target file path from tool input for boundary checking.
fn extract_target_path(tool_name: &str, input: &serde_json::Value) -> Option<String> {
    match tool_name {
        "read" | "write" | "edit" | "list" => input["path"].as_str().map(|s| s.to_string()),
        "find" => input["path"].as_str().map(|s| s.to_string()),
        "patch" => input["path"].as_str().map(|s| s.to_string()),
        "search" => input["directory"].as_str().map(|s| s.to_string()),
        _ => None,
    }
}

/// Simple pattern matching for deny/allow list rules.
fn input_matches_pattern(tool_name: &str, input: &serde_json::Value, pattern: &str) -> bool {
    if pattern == "*" || pattern.is_empty() {
        return true;
    }

    if tool_name == "shell" {
        return input["command"]
            .as_str()
            .is_some_and(|command| shell_command_matches_pattern(command, pattern));
    }

    json_value_matches_pattern(input, pattern)
}

fn shell_command_matches_pattern(command: &str, pattern: &str) -> bool {
    let normalized_pattern = normalize_policy_text(pattern);
    if normalized_pattern.is_empty() {
        return true;
    }

    let normalized_command = normalize_policy_text(command);
    if simple_glob_match(&normalized_pattern, &normalized_command) {
        return true;
    }

    split_shell_command_segments(command)
        .into_iter()
        .map(|segment| normalize_policy_text(&segment))
        .any(|segment| simple_glob_match(&normalized_pattern, &segment))
}

fn json_value_matches_pattern(input: &serde_json::Value, pattern: &str) -> bool {
    match input {
        serde_json::Value::String(value) => simple_glob_match(
            &normalize_policy_text(pattern),
            &normalize_policy_text(value),
        ),
        serde_json::Value::Array(values) => values
            .iter()
            .any(|value| json_value_matches_pattern(value, pattern)),
        serde_json::Value::Object(map) => map
            .values()
            .any(|value| json_value_matches_pattern(value, pattern)),
        serde_json::Value::Null => false,
        other => simple_glob_match(
            &normalize_policy_text(pattern),
            &normalize_policy_text(&other.to_string()),
        ),
    }
}

fn normalize_policy_text(raw: &str) -> String {
    raw.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn split_shell_command_segments(command: &str) -> Vec<String> {
    let chars: Vec<char> = command.chars().collect();
    let mut segments = Vec::new();
    let mut current = String::new();
    let mut index = 0;
    let mut in_single_quote = false;
    let mut in_double_quote = false;

    while index < chars.len() {
        let ch = chars[index];

        if in_single_quote {
            current.push(ch);
            if ch == '\'' {
                in_single_quote = false;
            }
            index += 1;
            continue;
        }

        if in_double_quote {
            current.push(ch);
            if ch == '\\' && index + 1 < chars.len() {
                index += 1;
                current.push(chars[index]);
            } else if ch == '"' {
                in_double_quote = false;
            }
            index += 1;
            continue;
        }

        match ch {
            '\'' => {
                in_single_quote = true;
                current.push(ch);
                index += 1;
            }
            '"' => {
                in_double_quote = true;
                current.push(ch);
                index += 1;
            }
            '&' if chars.get(index + 1) == Some(&'&') => {
                push_shell_segment(&mut segments, &mut current);
                index += 2;
            }
            '|' if chars.get(index + 1) == Some(&'|') => {
                push_shell_segment(&mut segments, &mut current);
                index += 2;
            }
            ';' | '&' => {
                push_shell_segment(&mut segments, &mut current);
                index += 1;
            }
            _ => {
                current.push(ch);
                index += 1;
            }
        }
    }

    push_shell_segment(&mut segments, &mut current);
    segments
}

fn push_shell_segment(segments: &mut Vec<String>, current: &mut String) {
    let trimmed = current.trim();
    if !trimmed.is_empty() {
        segments.push(trimmed.to_string());
    }
    current.clear();
}

fn simple_glob_match(pattern: &str, text: &str) -> bool {
    let pattern_chars: Vec<char> = pattern.chars().collect();
    let text_chars: Vec<char> = text.chars().collect();
    let mut memo = vec![vec![None; text_chars.len() + 1]; pattern_chars.len() + 1];

    fn matches_from(
        pattern: &[char],
        text: &[char],
        pi: usize,
        ti: usize,
        memo: &mut [Vec<Option<bool>>],
    ) -> bool {
        if let Some(cached) = memo[pi][ti] {
            return cached;
        }

        let result = if pi == pattern.len() {
            ti == text.len()
        } else if pattern[pi] == '\\' {
            if pi + 1 >= pattern.len() {
                ti < text.len()
                    && text[ti] == '\\'
                    && matches_from(pattern, text, pi + 1, ti + 1, memo)
            } else {
                ti < text.len()
                    && text[ti] == pattern[pi + 1]
                    && matches_from(pattern, text, pi + 2, ti + 1, memo)
            }
        } else if pattern[pi] == '*' {
            matches_from(pattern, text, pi + 1, ti, memo)
                || (ti < text.len() && matches_from(pattern, text, pi, ti + 1, memo))
        } else {
            ti < text.len()
                && pattern[pi] == text[ti]
                && matches_from(pattern, text, pi + 1, ti + 1, memo)
        };

        memo[pi][ti] = Some(result);
        result
    }

    matches_from(&pattern_chars, &text_chars, 0, 0, &mut memo)
}

fn effective_policy_rule(rule: &serde_json::Value) -> Option<EffectivePolicyRule> {
    let base_tool = rule["tool"].as_str().unwrap_or("*").trim();
    let base_pattern = rule["pattern"].as_str().unwrap_or("").trim();

    let mut effective = EffectivePolicyRule {
        tool: normalize_rule_tool(base_tool),
        pattern: base_pattern.to_string(),
    };

    if let Some(prefixed) = parse_prefixed_policy_pattern(base_pattern) {
        effective = prefixed;
    }

    if effective.tool.is_empty() {
        effective.tool = "*".to_string();
    }

    Some(effective)
}

fn parse_prefixed_policy_pattern(raw: &str) -> Option<EffectivePolicyRule> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }

    let colon_index = trimmed.find(':')?;
    let prefix = trimmed[..colon_index].trim().to_ascii_lowercase();
    let remainder = trimmed[colon_index + 1..].trim_start();

    match prefix.as_str() {
        "shell" => {
            if remainder.is_empty() {
                None
            } else {
                Some(EffectivePolicyRule {
                    tool: "shell".to_string(),
                    pattern: remainder.to_string(),
                })
            }
        }
        "any" => {
            if remainder.is_empty() {
                None
            } else {
                Some(EffectivePolicyRule {
                    tool: "*".to_string(),
                    pattern: remainder.to_string(),
                })
            }
        }
        "tool" => {
            let mut parts = remainder.splitn(2, char::is_whitespace);
            let tool_name = parts.next().unwrap_or("").trim();
            let pattern = parts.next().unwrap_or("").trim();
            if tool_name.is_empty() || pattern.is_empty() {
                return None;
            }

            Some(EffectivePolicyRule {
                tool: normalize_rule_tool(tool_name),
                pattern: pattern.to_string(),
            })
        }
        _ => None,
    }
}

fn normalize_rule_tool(tool: &str) -> String {
    let trimmed = tool.trim();
    if trimmed.is_empty() {
        "*".to_string()
    } else {
        trimmed.to_ascii_lowercase()
    }
}
