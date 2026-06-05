use async_trait::async_trait;
use std::borrow::Cow;

use crate::model::errors::AppError;
use crate::persistence::repo::settings_repo;

use super::build_context::BuildCx;
use super::error_codes::codes;
use super::section_source::{FatalError, SectionBody, SectionMeta, SectionOutcome, SectionSource};
use super::templates::{load_template, parse_front_matter, render_template_strict, TemplateVars};

const TEMPLATE_REL_PATH: &str = "sandbox_permissions.tpl.md";
const TEMPLATE_EMBEDDED: &str = include_str!("templates/sandbox_permissions.tpl.md");
const DECLARED_KEYS: &[&'static str] = &[
    "workspace_path",
    "approval_policy",
    "run_mode_line",
    "writable_roots_line",
];

/// SectionSource for SandboxPermissions, backed by a template file.
/// Reads approval_policy + writable_roots from settings, and run_mode from BuildCx.
pub struct SandboxPermissionsSource;

#[async_trait]
impl SectionSource for SandboxPermissionsSource {
    fn source_kind(&self) -> &'static str {
        "template:sandbox_permissions.tpl.md"
    }

    async fn build(&self, cx: &BuildCx<'_>) -> Result<SectionOutcome, FatalError> {
        let approval_policy = match load_approval_policy(cx).await {
            Ok(v) => v,
            Err(e) => {
                return Ok(SectionOutcome::SoftFailed {
                    code: "settings.approval_policy.load_failed",
                    error: Box::new(std::io::Error::new(
                        std::io::ErrorKind::Other,
                        e.to_string(),
                    )),
                });
            }
        };

        let writable_roots = match load_writable_roots(cx).await {
            Ok(v) => v,
            Err(e) => {
                return Ok(SectionOutcome::SoftFailed {
                    code: "settings.writable_roots.load_failed",
                    error: Box::new(std::io::Error::new(
                        std::io::ErrorKind::Other,
                        e.to_string(),
                    )),
                });
            }
        };

        let run_mode_line = if cx.run_mode.as_str() == "plan" {
            "Plan mode is active, so mutating tools are blocked; shell follows the configured approval policy and must be used only for read-only commands."
        } else {
            "Default mode is active, so tool use follows the configured approval policy."
        };

        let writable_roots_line = if writable_roots.is_empty() {
            String::new()
        } else {
            let roots_display: Vec<String> = writable_roots
                .iter()
                .map(|root| format!("`{root}`"))
                .collect();
            format!(
                "\n- Additional writable roots: {}. File tools (read, write, edit, list, find, search) can operate on files under these paths in addition to the workspace.",
                roots_display.join(", ")
            )
        };

        let raw = load_template(TEMPLATE_REL_PATH, TEMPLATE_EMBEDDED);
        let (_tmpl, body) = parse_front_matter(&raw).map_err(|e| {
            FatalError::new(
                codes::TEMPLATE_NOT_FOUND,
                format!("{}: {}", TEMPLATE_REL_PATH, e),
            )
        })?;

        let vars = TemplateVars::new()
            .insert_user_text("workspace_path", cx.workspace_path)
            .insert("approval_policy", approval_policy)
            .insert("run_mode_line", run_mode_line)
            .insert("writable_roots_line", writable_roots_line);

        let rendered = render_template_strict(&body, DECLARED_KEYS, &vars).map_err(|e| {
            FatalError::new(
                codes::TEMPLATE_MISSING_KEY,
                format!("{}: {}", TEMPLATE_REL_PATH, e),
            )
        })?;

        Ok(SectionOutcome::Produced(SectionBody {
            markdown: rendered.trim_end().to_string(),
            meta: SectionMeta {
                template_path: Some(TEMPLATE_REL_PATH),
                ..Default::default()
            },
        }))
    }
}

async fn load_approval_policy(cx: &BuildCx<'_>) -> Result<String, AppError> {
    Ok(settings_repo::policy_get(cx.pool, "approval_policy")
        .await?
        .map(|record| parse_approval_policy_mode(&record.value_json))
        .unwrap_or_else(|| "require_for_mutations".to_string()))
}

async fn load_writable_roots(cx: &BuildCx<'_>) -> Result<Vec<String>, AppError> {
    use crate::core::workspace_paths::{merge_writable_roots, parse_writable_roots};
    Ok(settings_repo::policy_get(cx.pool, "writable_roots")
        .await?
        .map(|record| parse_writable_roots(&record.value_json))
        .map(|roots| merge_writable_roots(&roots))
        .unwrap_or_else(|| merge_writable_roots(&[])))
}

fn parse_approval_policy_mode(value_json: &str) -> String {
    let parsed: serde_json::Value = serde_json::from_str(value_json).unwrap_or_default();
    if let Some(value) = parsed.as_str() {
        return value.to_string();
    }
    parsed
        .get("mode")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("require_for_mutations")
        .to_string()
}

// ─── SystemEnvironment ────────────────────────────────────────────

const SYSENV_TEMPLATE_REL_PATH: &str = "system_environment.tpl.md";
const SYSENV_TEMPLATE_EMBEDDED: &str = include_str!("templates/system_environment.tpl.md");
const SYSENV_DECLARED_KEYS: &[&'static str] = &["os", "arch", "shell"];

pub struct SystemEnvironmentSource;

#[async_trait]
impl SectionSource for SystemEnvironmentSource {
    fn source_kind(&self) -> &'static str {
        "template:system_environment.tpl.md"
    }

    async fn build(&self, _cx: &BuildCx<'_>) -> Result<SectionOutcome, FatalError> {
        let raw = load_template(SYSENV_TEMPLATE_REL_PATH, SYSENV_TEMPLATE_EMBEDDED);
        let (_tmpl, body) = parse_front_matter(&raw).map_err(|e| {
            FatalError::new(
                codes::TEMPLATE_NOT_FOUND,
                format!("{}: {}", SYSENV_TEMPLATE_REL_PATH, e),
            )
        })?;
        let vars = TemplateVars::new()
            .insert("os", std::env::consts::OS)
            .insert("arch", std::env::consts::ARCH)
            .insert("shell", crate::core::shell_runtime::current_shell());
        let rendered = render_template_strict(&body, SYSENV_DECLARED_KEYS, &vars).map_err(|e| {
            FatalError::new(
                codes::TEMPLATE_MISSING_KEY,
                format!("{}: {}", SYSENV_TEMPLATE_REL_PATH, e),
            )
        })?;
        Ok(SectionOutcome::Produced(SectionBody {
            markdown: rendered.trim_end().to_string(),
            meta: SectionMeta {
                template_path: Some(SYSENV_TEMPLATE_REL_PATH),
                ..Default::default()
            },
        }))
    }
}

// ─── WorkspaceLocation ────────────────────────────────────────────

const WSLOC_TEMPLATE_REL_PATH: &str = "workspace_location.tpl.md";
const WSLOC_TEMPLATE_EMBEDDED: &str = include_str!("templates/workspace_location.tpl.md");
const WSLOC_DECLARED_KEYS: &[&'static str] = &["workspace_path"];

pub struct WorkspaceLocationSource;

#[async_trait]
impl SectionSource for WorkspaceLocationSource {
    fn source_kind(&self) -> &'static str {
        "template:workspace_location.tpl.md"
    }

    async fn build(&self, cx: &BuildCx<'_>) -> Result<SectionOutcome, FatalError> {
        let raw = load_template(WSLOC_TEMPLATE_REL_PATH, WSLOC_TEMPLATE_EMBEDDED);
        let (_tmpl, body) = parse_front_matter(&raw).map_err(|e| {
            FatalError::new(
                codes::TEMPLATE_NOT_FOUND,
                format!("{}: {}", WSLOC_TEMPLATE_REL_PATH, e),
            )
        })?;
        let vars = TemplateVars::new().insert_user_text("workspace_path", cx.workspace_path);
        let rendered = render_template_strict(&body, WSLOC_DECLARED_KEYS, &vars).map_err(|e| {
            FatalError::new(
                codes::TEMPLATE_MISSING_KEY,
                format!("{}: {}", WSLOC_TEMPLATE_REL_PATH, e),
            )
        })?;
        Ok(SectionOutcome::Produced(SectionBody {
            markdown: rendered.trim_end().to_string(),
            meta: SectionMeta {
                template_path: Some(WSLOC_TEMPLATE_REL_PATH),
                ..Default::default()
            },
        }))
    }
}

// ─── ProjectContext ───────────────────────────────────────────────

const PROJCTX_TEMPLATE_REL_PATH: &str = "project_context.tpl.md";
const PROJCTX_TEMPLATE_EMBEDDED: &str = include_str!("templates/project_context.tpl.md");
const PROJCTX_DECLARED_KEYS: &[&'static str] = &["file_name", "content", "truncated_marker"];

const WORKSPACE_INSTRUCTION_FILE_NAMES: &[&str] = &["AGENTS.md", "CLAUDE.md", "AGENT.MD"];
const WORKSPACE_INSTRUCTION_MAX_CHARS: usize = 12_800;

pub struct ProjectContextSource;

#[async_trait]
impl SectionSource for ProjectContextSource {
    fn source_kind(&self) -> &'static str {
        "template:project_context.tpl.md"
    }

    async fn build(&self, cx: &BuildCx<'_>) -> Result<SectionOutcome, FatalError> {
        let snippet = match collect_workspace_instruction_snippet(cx.workspace_path) {
            Some(s) => s,
            None => return Ok(SectionOutcome::Skip),
        };

        let raw = load_template(PROJCTX_TEMPLATE_REL_PATH, PROJCTX_TEMPLATE_EMBEDDED);
        let (_tmpl, body) = parse_front_matter(&raw).map_err(|e| {
            FatalError::new(
                codes::TEMPLATE_NOT_FOUND,
                format!("{}: {}", PROJCTX_TEMPLATE_REL_PATH, e),
            )
        })?;

        let truncated_marker = if snippet.truncated {
            "\n[Truncated for prompt size.]"
        } else {
            ""
        };
        let vars = TemplateVars::new()
            .insert("file_name", snippet.file_name)
            .insert_user_text("content", snippet.content)
            .insert("truncated_marker", truncated_marker);

        let rendered =
            render_template_strict(&body, PROJCTX_DECLARED_KEYS, &vars).map_err(|e| {
                FatalError::new(
                    codes::TEMPLATE_MISSING_KEY,
                    format!("{}: {}", PROJCTX_TEMPLATE_REL_PATH, e),
                )
            })?;
        Ok(SectionOutcome::Produced(SectionBody {
            markdown: rendered.trim_end().to_string(),
            meta: SectionMeta {
                template_path: Some(PROJCTX_TEMPLATE_REL_PATH),
                ..Default::default()
            },
        }))
    }
}

#[derive(Debug, Clone)]
struct WorkspaceInstructionSnippet {
    file_name: &'static str,
    content: String,
    truncated: bool,
}

fn collect_workspace_instruction_snippet(
    workspace_path: &str,
) -> Option<WorkspaceInstructionSnippet> {
    use std::path::Path;
    let workspace_root = Path::new(workspace_path);
    if !workspace_root.is_dir() {
        return None;
    }

    WORKSPACE_INSTRUCTION_FILE_NAMES
        .iter()
        .find_map(|file_name| {
            let path = workspace_root.join(file_name);
            if !path.is_file() {
                return None;
            }
            let raw = std::fs::read(&path).ok()?;
            let content = normalize_prompt_doc_content(&String::from_utf8_lossy(&raw));
            if content.is_empty() {
                return None;
            }
            let (content, truncated) = truncate_chars(&content, WORKSPACE_INSTRUCTION_MAX_CHARS);
            Some(WorkspaceInstructionSnippet {
                file_name,
                content,
                truncated,
            })
        })
}

fn normalize_prompt_doc_content(value: &str) -> String {
    value
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

fn truncate_chars(value: &str, max_chars: usize) -> (String, bool) {
    let char_count = value.chars().count();
    if char_count <= max_chars {
        return (value.to_string(), false);
    }
    let truncated = value.chars().take(max_chars).collect::<String>();
    (truncated.trim_end().to_string(), true)
}

// ─── RunMode (plan/default branch) ────────────────────────────────

const RUN_MODE_PLAN_TEMPLATE: &str = "run_mode.plan.md";
const RUN_MODE_PLAN_EMBEDDED: &str = include_str!("templates/run_mode.plan.md");
const RUN_MODE_DEFAULT_TEMPLATE: &str = "run_mode.default.md";
const RUN_MODE_DEFAULT_EMBEDDED: &str = include_str!("templates/run_mode.default.md");
const RUN_MODE_DECLARED_KEYS: &[&'static str] = &["term_panel_usage_note"];

pub struct RunModeSource;

#[async_trait]
impl SectionSource for RunModeSource {
    fn source_kind(&self) -> &'static str {
        "template:run_mode.*.md"
    }

    async fn build(&self, cx: &BuildCx<'_>) -> Result<SectionOutcome, FatalError> {
        let (rel_path, embedded) = if cx.run_mode.as_str() == "plan" {
            (RUN_MODE_PLAN_TEMPLATE, RUN_MODE_PLAN_EMBEDDED)
        } else {
            (RUN_MODE_DEFAULT_TEMPLATE, RUN_MODE_DEFAULT_EMBEDDED)
        };

        let raw = load_template(rel_path, embedded);
        let (_tmpl, body) = parse_front_matter(&raw).map_err(|e| {
            FatalError::new(codes::TEMPLATE_NOT_FOUND, format!("{}: {}", rel_path, e))
        })?;

        let vars = TemplateVars::new().insert(
            "term_panel_usage_note",
            crate::core::subagent::TERM_PANEL_USAGE_NOTE,
        );

        let rendered =
            render_template_strict(&body, RUN_MODE_DECLARED_KEYS, &vars).map_err(|e| {
                FatalError::new(codes::TEMPLATE_MISSING_KEY, format!("{}: {}", rel_path, e))
            })?;

        // Cow wraps the const &'static str — clone if borrowed
        let _ = Cow::Borrowed(rel_path);

        Ok(SectionOutcome::Produced(SectionBody {
            markdown: rendered,
            meta: SectionMeta {
                template_path: Some(rel_path),
                ..Default::default()
            },
        }))
    }
}
