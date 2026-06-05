use async_trait::async_trait;
use std::borrow::Cow;

use crate::model::errors::AppError;
use crate::persistence::repo::settings_repo;

use super::super::build_context::BuildCx;
use super::super::error_codes::codes;
use super::super::section_source::{
    FatalError, SectionBody, SectionMeta, SectionOutcome, SectionSource,
};
use super::super::templates::{
    load_template, parse_front_matter, render_template_strict, TemplateVars,
};

const TEMPLATE_REL_PATH: &str = "sandbox_permissions.tpl.md";
const TEMPLATE_EMBEDDED: &str = include_str!("../templates/sandbox_permissions.tpl.md");
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
