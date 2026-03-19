//! Unified policy evaluation engine.
//!
//! Single security boundary for all tool execution. Evaluates:
//! 1. Built-in dangerous pattern hard deny
//! 2. User deny list
//! 3. Workspace boundary check
//! 4. Run mode restrictions (plan mode blocks mutations)
//! 5. User allow list
//! 6. Approval policy fallback

use serde::Serialize;
use sqlx::SqlitePool;

use crate::core::workspace_paths::{canonicalize_workspace_root, resolve_path_within_workspace};
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
    "write_file",
    "apply_patch",
    "run_command",
    "git_add",
    "git_stage",
    "git_unstage",
    "git_commit",
    "git_push",
    "git_pull",
    "git_fetch",
    "terminal_write",
    "terminal_write_input",
    "terminal_restart",
    "terminal_close",
    "marketplace_install",
];

/// Tools that are read-only and generally safe.
const READ_ONLY_TOOLS: &[&str] = &[
    "read_file",
    "list_dir",
    "search_repo",
    "git_status",
    "git_diff",
    "git_log",
    "terminal_get_status",
    "terminal_get_recent_output",
];

// ---------------------------------------------------------------------------
// Built-in dangerous patterns for run_command
// ---------------------------------------------------------------------------

const DANGEROUS_COMMAND_PATTERNS: &[&str] = &[
    "rm -rf /",
    "rm -rf /*",
    "rm -rf ~",
    "sudo ",
    "mkfs",
    "dd if=",
    "curl|sh",
    "curl |sh",
    "curl | sh",
    "wget|sh",
    "wget |sh",
    "wget | sh",
    "curl|bash",
    "curl |bash",
    "curl | bash",
    "wget|bash",
    "wget |bash",
    "wget | bash",
    "> /dev/sd",
    "chmod 777 /",
    ":(){ :|:& };:",
];

// ---------------------------------------------------------------------------
// PolicyEngine
// ---------------------------------------------------------------------------

pub struct PolicyEngine {
    pool: SqlitePool,
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
        run_mode: &str,
    ) -> Result<PolicyCheck, AppError> {
        let mut checked = Vec::new();

        // 1. Built-in dangerous pattern check (run_command only)
        if tool_name == "run_command" {
            checked.push("builtin_dangerous_patterns".to_string());
            if let Some(cmd) = tool_input["command"].as_str() {
                let cmd_lower = cmd.to_lowercase();
                for pattern in DANGEROUS_COMMAND_PATTERNS {
                    if cmd_lower.contains(pattern) {
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
                if let Some(tool) = rule["tool"].as_str() {
                    if tool == tool_name || tool == "*" {
                        let pattern = rule["pattern"].as_str().unwrap_or("");
                        if pattern.is_empty() || input_matches_pattern(tool_input, pattern) {
                            return Ok(PolicyCheck {
                                tool_name: tool_name.to_string(),
                                verdict: PolicyVerdict::Deny {
                                    reason: format!(
                                        "Denied by user deny list rule: {tool}/{pattern}"
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
            let target_path = extract_target_path(tool_name, tool_input);
            if let Some(target) = target_path {
                if let Err(error) = resolve_path_within_workspace(
                    &workspace_root,
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

        // 4. Run mode restriction (plan mode blocks mutations)
        if run_mode == "plan" && MUTATING_TOOLS.contains(&tool_name) {
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
                if let Some(tool) = rule["tool"].as_str() {
                    if tool == tool_name || tool == "*" {
                        return Ok(PolicyCheck {
                            tool_name: tool_name.to_string(),
                            verdict: PolicyVerdict::AutoAllow,
                            checked_rules: checked,
                        });
                    }
                }
            }
        }

        // 6. Read-only tools default to auto-allow
        if READ_ONLY_TOOLS.contains(&tool_name) {
            checked.push("read_only_default".to_string());
            return Ok(PolicyCheck {
                tool_name: tool_name.to_string(),
                verdict: PolicyVerdict::AutoAllow,
                checked_rules: checked,
            });
        }

        // 7. Approval policy fallback
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
                Ok(val["mode"]
                    .as_str()
                    .unwrap_or("require_for_mutations")
                    .to_string())
            }
            None => Ok("require_for_mutations".to_string()),
        }
    }
}

/// Extract the target file path from tool input for boundary checking.
fn extract_target_path(tool_name: &str, input: &serde_json::Value) -> Option<String> {
    match tool_name {
        "read_file" | "write_file" | "list_dir" => input["path"].as_str().map(|s| s.to_string()),
        "apply_patch" => input["path"].as_str().map(|s| s.to_string()),
        "search_repo" => input["directory"].as_str().map(|s| s.to_string()),
        _ => None,
    }
}

/// Simple pattern matching for deny/allow list rules.
fn input_matches_pattern(input: &serde_json::Value, pattern: &str) -> bool {
    if pattern == "*" || pattern.is_empty() {
        return true;
    }
    // Check if any string value in the input contains the pattern
    let input_str = input.to_string();
    input_str.contains(pattern)
}
