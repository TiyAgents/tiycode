use async_trait::async_trait;

use crate::core::plan_checkpoint::parse_plan_message_metadata;
use crate::persistence::repo::message_repo;

use super::build_context::BuildCx;
use super::section_source::{FatalError, SectionBody, SectionMeta, SectionOutcome, SectionSource};
use super::templates::{load_template, parse_front_matter, render_template_strict, TemplateVars};

const TEMPLATE_REL_PATH: &str = "active_plan.tpl.md";
const TEMPLATE_EMBEDDED: &str = include_str!("templates/active_plan.tpl.md");
const DECLARED_KEYS: &[&'static str] = &[];

/// Produces the "Active Plan" section when an approved (non-superseded) plan exists
/// for the current thread. Placed in the Ephemeral layer so it does not break
/// LLM prefix-cache stability.
pub struct ActivePlanSource;

#[async_trait]
impl SectionSource for ActivePlanSource {
    fn source_kind(&self) -> &'static str {
        "active_plan_source"
    }

    async fn build(&self, cx: &BuildCx<'_>) -> Result<SectionOutcome, FatalError> {
        let thread_id = match cx.thread_id {
            Some(tid) => tid,
            None => return Ok(SectionOutcome::Skip),
        };

        let messages = message_repo::list_recent(cx.pool, thread_id, None, 256)
            .await
            .map_err(|e| {
                FatalError::new(super::error_codes::codes::PLAN_LOAD_FAILED, e.to_string())
            })?;

        // Find the latest non-superseded plan message
        let active_plan = messages.iter().rev().find_map(|m| {
            if m.message_type != "plan" {
                return None;
            }
            let raw: serde_json::Value = serde_json::from_str(m.metadata_json.as_deref()?).ok()?;
            let meta = parse_plan_message_metadata(&raw)?;
            if meta.approval_state == "superseded" {
                return None;
            }
            Some(meta)
        });

        let _plan = match active_plan {
            Some(p) => p,
            None => return Ok(SectionOutcome::Skip),
        };

        let raw = load_template(TEMPLATE_REL_PATH, TEMPLATE_EMBEDDED);
        let (_tmpl, body) = parse_front_matter(&raw).map_err(|e| {
            FatalError::new(
                super::error_codes::codes::TEMPLATE_NOT_FOUND,
                format!("{}: {}", TEMPLATE_REL_PATH, e),
            )
        })?;

        let vars = TemplateVars::new();

        let rendered = render_template_strict(&body, DECLARED_KEYS, &vars).map_err(|e| {
            FatalError::new(
                super::error_codes::codes::TEMPLATE_MISSING_KEY,
                format!("{}: {}", TEMPLATE_REL_PATH, e),
            )
        })?;

        Ok(SectionOutcome::Produced(SectionBody {
            markdown: rendered,
            meta: SectionMeta {
                template_path: Some(TEMPLATE_REL_PATH),
                ..Default::default()
            },
        }))
    }
}
