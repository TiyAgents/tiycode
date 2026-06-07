use async_trait::async_trait;

use crate::core::subagent::SubagentProfile;

use super::super::build_context::BuildCx;
use super::super::section_source::{
    FatalError, SectionBody, SectionMeta, SectionOutcome, SectionSource,
};

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
                let template = include_str!("../templates/subagent/explore.md");
                let (_tmpl, body) =
                    super::super::templates::parse_front_matter(template).map_err(|e| {
                        FatalError::new("template.parse", format!("subagent/explore.md: {e}"))
                    })?;
                // No template vars needed for static persona prompts
                let vars = super::super::templates::TemplateVars::new();
                let rendered = super::super::templates::render_template_strict(&body, &[], &vars)
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
                let template = include_str!("../templates/subagent/review.md");
                let (_tmpl, body) =
                    super::super::templates::parse_front_matter(template).map_err(|e| {
                        FatalError::new("template.parse", format!("subagent/review.md: {e}"))
                    })?;
                let vars = super::super::templates::TemplateVars::new();
                let rendered = super::super::templates::render_template_strict(&body, &[], &vars)
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
            Some(SubagentProfile::Judge) => {
                let template = include_str!("../templates/subagent/judge.md");
                let (_tmpl, body) =
                    super::super::templates::parse_front_matter(template).map_err(|e| {
                        FatalError::new("template.parse", format!("subagent/judge.md: {e}"))
                    })?;
                let vars = super::super::templates::TemplateVars::new();
                let rendered = super::super::templates::render_template_strict(&body, &[], &vars)
                    .map_err(|e| {
                    FatalError::new("template.render", format!("subagent/judge.md: {e}"))
                })?;
                Ok(SectionOutcome::Produced(SectionBody {
                    markdown: rendered,
                    meta: SectionMeta {
                        template_path: Some("templates/subagent/judge.md"),
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
