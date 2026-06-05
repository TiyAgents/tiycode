use async_trait::async_trait;

use crate::model::goal::GoalStatus;
use crate::persistence::repo::goal_repo;

use super::build_context::BuildCx;
use super::section_source::{FatalError, SectionBody, SectionMeta, SectionOutcome, SectionSource};
use super::templates::{load_template, parse_front_matter, render_template_strict, TemplateVars};

const TEMPLATE_REL_PATH: &str = "active_goal.tpl.md";
const TEMPLATE_EMBEDDED: &str = include_str!("templates/active_goal.tpl.md");
const DECLARED_KEYS: &[&'static str] = &["objective", "turns_used", "max_turns"];

/// Produces the "Active Goal" section when an active goal exists for the current thread.
/// Placed in the Ephemeral layer so it does not break LLM prefix-cache stability.
pub struct ActiveGoalSource;

#[async_trait]
impl SectionSource for ActiveGoalSource {
    fn source_kind(&self) -> &'static str {
        "active_goal_source"
    }

    async fn build(&self, cx: &BuildCx<'_>) -> Result<SectionOutcome, FatalError> {
        let thread_id = match cx.thread_id {
            Some(tid) => tid,
            None => return Ok(SectionOutcome::Skip),
        };

        let goal = goal_repo::find_by_thread_id(cx.pool, thread_id)
            .await
            .map_err(|e| {
                FatalError::new(super::error_codes::codes::GOAL_LOAD_FAILED, e.to_string())
            })?;

        let goal = match goal {
            Some(g) if g.status == GoalStatus::Active => g,
            _ => return Ok(SectionOutcome::Skip),
        };

        let raw = load_template(TEMPLATE_REL_PATH, TEMPLATE_EMBEDDED);
        let (_tmpl, body) = parse_front_matter(&raw).map_err(|e| {
            FatalError::new(
                super::error_codes::codes::TEMPLATE_NOT_FOUND,
                format!("{}: {}", TEMPLATE_REL_PATH, e),
            )
        })?;

        let vars = TemplateVars::new()
            .insert_user_text("objective", goal.objective)
            .insert("turns_used", goal.turns_used.to_string())
            .insert("max_turns", goal.max_turns.to_string());

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
