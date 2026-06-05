use async_trait::async_trait;

use crate::core::subagent::SubagentProfile;

use super::build_context::BuildCx;
use super::context::PromptBuildContext;
use super::providers::ProfileProvider;
use super::section::PromptSectionProvider;
use super::section_source::{FatalError, SectionBody, SectionMeta, SectionOutcome, SectionSource};

// ---------------------------------------------------------------------------
// Legacy adapter for ProfileInstructions, the only section that still depends
// on dynamic PromptSectionProvider logic (ProfileProvider).
// ---------------------------------------------------------------------------

pub struct LegacyProfileInstructionsSource(pub ProfileProvider);

#[async_trait]
impl SectionSource for LegacyProfileInstructionsSource {
    async fn build(&self, cx: &BuildCx<'_>) -> Result<SectionOutcome, FatalError> {
        let raw_plan = match cx.raw_plan {
            Some(plan) => plan,
            None => return Ok(SectionOutcome::Skip),
        };
        let old_ctx = PromptBuildContext::new(
            cx.pool,
            raw_plan,
            cx.workspace_path,
            cx.run_mode.as_str(),
        );

        let sections = self
            .0
            .collect(&old_ctx)
            .await
            .map_err(|e| FatalError::new("legacy.provider", e.to_string()))?;

        match sections
            .into_iter()
            .find(|s| s.key == "profile_instructions")
        {
            Some(section) if !section.body.trim().is_empty() => {
                Ok(SectionOutcome::Produced(SectionBody::markdown(section.body)))
            }
            _ => Ok(SectionOutcome::Skip),
        }
    }
}

// ---------------------------------------------------------------------------
// SubagentBodySource: loads template-backed subagent body for Explore/Review;
// returns user-provided system_prompt for Custom subagents.
// ---------------------------------------------------------------------------

pub struct SubagentBodySource;

#[async_trait]
impl SectionSource for SubagentBodySource {
    fn source_kind(&self) -> &'static str {
        "subagent_body"
    }

    async fn build(&self, cx: &BuildCx<'_>) -> Result<SectionOutcome, FatalError> {
        match cx.helper_profile {
            Some(SubagentProfile::Explore) => {
                let template = include_str!("templates/subagent/explore.md");
                let (_tmpl, body) =
                    super::templates::parse_front_matter(template).map_err(|e| {
                        FatalError::new("template.parse", format!("subagent/explore.md: {e}"))
                    })?;
                // No template vars needed for static persona prompts
                let vars = super::templates::TemplateVars::new();
                let rendered = super::templates::render_template_strict(&body, &[], &vars)
                    .map_err(|e| {
                        FatalError::new("template.render", format!("subagent/explore.md: {e}"))
                    })?;
                Ok(SectionOutcome::Produced(SectionBody {
                    markdown: rendered,
                    meta: SectionMeta {
                        template_path: Some("templates/subagent/explore.md"),
                        ..Default::default()
                    },
                }))
            }
            Some(SubagentProfile::Review) => {
                let template = include_str!("templates/subagent/review.md");
                let (_tmpl, body) =
                    super::templates::parse_front_matter(template).map_err(|e| {
                        FatalError::new("template.parse", format!("subagent/review.md: {e}"))
                    })?;
                let vars = super::templates::TemplateVars::new();
                let rendered = super::templates::render_template_strict(&body, &[], &vars)
                    .map_err(|e| {
                        FatalError::new("template.render", format!("subagent/review.md: {e}"))
                    })?;
                Ok(SectionOutcome::Produced(SectionBody {
                    markdown: rendered,
                    meta: SectionMeta {
                        template_path: Some("templates/subagent/review.md"),
                        ..Default::default()
                    },
                }))
            }
            Some(SubagentProfile::Custom { system_prompt, .. }) => {
                if system_prompt.trim().is_empty() {
                    return Ok(SectionOutcome::Skip);
                }
                Ok(SectionOutcome::Produced(SectionBody::markdown(
                    system_prompt,
                )))
            }
            None => Ok(SectionOutcome::Skip),
        }
    }
}
