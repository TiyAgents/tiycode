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
pub struct SandboxPermissionsSource {
    spec_version: u32,
}

impl SandboxPermissionsSource {
    pub fn new(spec_version: u32) -> Self {
        Self { spec_version }
    }
}

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
        let (tmpl, body) = parse_front_matter(&raw).map_err(|e| {
            FatalError::new(
                codes::TEMPLATE_NOT_FOUND,
                format!("{}: {}", TEMPLATE_REL_PATH, e),
            )
        })?;

        if tmpl.version != self.spec_version {
            return Err(FatalError::new(
                "template.version_mismatch",
                format!(
                    "{}: template front-matter version {} != spec version {}",
                    TEMPLATE_REL_PATH, tmpl.version, self.spec_version
                ),
            ));
        }

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

pub struct SystemEnvironmentSource {
    spec_version: u32,
}

impl SystemEnvironmentSource {
    pub fn new(spec_version: u32) -> Self {
        Self { spec_version }
    }
}

#[async_trait]
impl SectionSource for SystemEnvironmentSource {
    fn source_kind(&self) -> &'static str {
        "template:system_environment.tpl.md"
    }

    async fn build(&self, _cx: &BuildCx<'_>) -> Result<SectionOutcome, FatalError> {
        let raw = load_template(SYSENV_TEMPLATE_REL_PATH, SYSENV_TEMPLATE_EMBEDDED);
        let (tmpl, body) = parse_front_matter(&raw).map_err(|e| {
            FatalError::new(
                codes::TEMPLATE_NOT_FOUND,
                format!("{}: {}", SYSENV_TEMPLATE_REL_PATH, e),
            )
        })?;

        if tmpl.version != self.spec_version {
            return Err(FatalError::new(
                "template.version_mismatch",
                format!(
                    "{}: template front-matter version {} != spec version {}",
                    SYSENV_TEMPLATE_REL_PATH, tmpl.version, self.spec_version
                ),
            ));
        }
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

pub struct WorkspaceLocationSource {
    spec_version: u32,
}

impl WorkspaceLocationSource {
    pub fn new(spec_version: u32) -> Self {
        Self { spec_version }
    }
}

#[async_trait]
impl SectionSource for WorkspaceLocationSource {
    fn source_kind(&self) -> &'static str {
        "template:workspace_location.tpl.md"
    }

    async fn build(&self, cx: &BuildCx<'_>) -> Result<SectionOutcome, FatalError> {
        let raw = load_template(WSLOC_TEMPLATE_REL_PATH, WSLOC_TEMPLATE_EMBEDDED);
        let (tmpl, body) = parse_front_matter(&raw).map_err(|e| {
            FatalError::new(
                codes::TEMPLATE_NOT_FOUND,
                format!("{}: {}", WSLOC_TEMPLATE_REL_PATH, e),
            )
        })?;

        if tmpl.version != self.spec_version {
            return Err(FatalError::new(
                "template.version_mismatch",
                format!(
                    "{}: template front-matter version {} != spec version {}",
                    WSLOC_TEMPLATE_REL_PATH, tmpl.version, self.spec_version
                ),
            ));
        }
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

pub struct ProjectContextSource {
    spec_version: u32,
}

impl ProjectContextSource {
    pub fn new(spec_version: u32) -> Self {
        Self { spec_version }
    }
}

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
        let (tmpl, body) = parse_front_matter(&raw).map_err(|e| {
            FatalError::new(
                codes::TEMPLATE_NOT_FOUND,
                format!("{}: {}", PROJCTX_TEMPLATE_REL_PATH, e),
            )
        })?;

        if tmpl.version != self.spec_version {
            return Err(FatalError::new(
                "template.version_mismatch",
                format!(
                    "{}: template front-matter version {} != spec version {}",
                    PROJCTX_TEMPLATE_REL_PATH, tmpl.version, self.spec_version
                ),
            ));
        }

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

pub struct RunModeSource {
    spec_version: u32,
}

impl RunModeSource {
    pub fn new(spec_version: u32) -> Self {
        Self { spec_version }
    }
}

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
        let (tmpl, body) = parse_front_matter(&raw).map_err(|e| {
            FatalError::new(codes::TEMPLATE_NOT_FOUND, format!("{}: {}", rel_path, e))
        })?;

        if tmpl.version != self.spec_version {
            return Err(FatalError::new(
                "template.version_mismatch",
                format!(
                    "{}: template front-matter version {} != spec version {}",
                    rel_path, tmpl.version, self.spec_version
                ),
            ));
        }

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

#[cfg(test)]
mod tests {
    use super::super::build_context::{BuildCx, ModelTarget};
    use super::super::clock::FixedClock;
    use super::super::renderer::MarkdownRenderer;
    use super::super::run_mode::RunMode;
    use super::super::section_source::SectionSource;
    use super::super::signals::SignalCache;
    use super::{RunModeSource, SystemEnvironmentSource};
    use std::sync::Arc;

    /// Construct a minimal BuildCx for testing sources that don't need DB access.
    async fn test_cx() -> BuildCx<'static> {
        let pool = sqlx::SqlitePool::connect_lazy("sqlite::memory:").unwrap();
        let pool_ref = Box::leak(Box::new(pool));
        BuildCx {
            pool: pool_ref,
            workspace_path: "/test/workspace",
            thread_id: None,
            run_id: None,
            raw_plan: None,
            run_mode: RunMode::Default,
            helper_profile: None,
            custom_subagent_slug: None,
            target_model: ModelTarget::AnthropicClaude {
                context_window: 200_000,
                supports_cache_control: true,
            },
            clock: Arc::new(FixedClock::new(chrono::Utc::now())),
            signals: Arc::new(SignalCache::new()),
            renderer: Arc::new(MarkdownRenderer),
            response_language: None,
        }
    }

    /// § 3.18: source_idempotency — same BuildCx must produce the same output.
    /// SystemEnvironmentSource is fully deterministic (env::consts only).
    #[tokio::test]
    async fn source_idempotency_system_environment() {
        let cx = test_cx().await;
        let source = SystemEnvironmentSource::new(1);

        let out1 = source.build(&cx).await.unwrap();
        let out2 = source.build(&cx).await.unwrap();

        match (&out1, &out2) {
            (
                super::super::section_source::SectionOutcome::Produced(b1),
                super::super::section_source::SectionOutcome::Produced(b2),
            ) => {
                assert_eq!(
                    b1.markdown, b2.markdown,
                    "SystemEnvironmentSource is not idempotent"
                );
            }
            _ => panic!("expected Produced outcomes"),
        }
    }

    /// § 3.18: source_determinism — output must be stable under deterministic inputs.
    /// SystemEnvironment produces the same OS/arch/shell for a given machine.
    #[tokio::test]
    async fn source_determinism_system_environment() {
        let cx = test_cx().await;
        let source = SystemEnvironmentSource::new(1);

        // Build 3 times; all should be identical
        let mut prev: Option<String> = None;
        for i in 0..3 {
            let out = source.build(&cx).await.unwrap();
            if let super::super::section_source::SectionOutcome::Produced(b) = out {
                if let Some(ref p) = prev {
                    assert_eq!(
                        &b.markdown, p,
                        "SystemEnvironmentSource output diverged on build {}",
                        i
                    );
                }
                prev = Some(b.markdown);
            } else {
                panic!("expected Produced on build {}", i);
            }
        }
    }

    /// RunModeSource idempotency across both plan and default modes.
    #[tokio::test]
    async fn source_idempotency_run_mode() {
        let source = RunModeSource::new(1);

        for mode in &[RunMode::Default, RunMode::Plan] {
            let cx = test_cx().await;
            // Create a separate cx for each mode to avoid mutability issues.
            let cx_mode = BuildCx {
                run_mode: *mode,
                ..cx
            };

            let out1 = source.build(&cx_mode).await.unwrap();
            let out2 = source.build(&cx_mode).await.unwrap();

            match (&out1, &out2) {
                (
                    super::super::section_source::SectionOutcome::Produced(b1),
                    super::super::section_source::SectionOutcome::Produced(b2),
                ) => {
                    assert_eq!(
                        b1.markdown, b2.markdown,
                        "RunModeSource is not idempotent for {:?}",
                        mode
                    );
                }
                _ => panic!("expected Produced for {:?}", mode),
            }
        }
    }

    /// RunModeSource: plan mode and default mode must produce different outputs.
    #[tokio::test]
    async fn run_mode_plan_vs_default_differ() {
        let source = RunModeSource::new(1);
        let base_cx = test_cx().await;

        let cx_plan = BuildCx {
            run_mode: RunMode::Plan,
            ..base_cx.clone()
        };
        let cx_default = BuildCx {
            run_mode: RunMode::Default,
            ..base_cx
        };

        let out_plan = source.build(&cx_plan).await.unwrap();
        let out_default = source.build(&cx_default).await.unwrap();

        match (&out_plan, &out_default) {
            (
                super::super::section_source::SectionOutcome::Produced(bp),
                super::super::section_source::SectionOutcome::Produced(bd),
            ) => {
                assert_ne!(
                    bp.markdown, bd.markdown,
                    "plan and default run_mode must produce different output"
                );
            }
            _ => panic!("expected Produced outcomes"),
        }
    }
}
