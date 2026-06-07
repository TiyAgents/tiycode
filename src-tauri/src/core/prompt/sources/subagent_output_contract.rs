use async_trait::async_trait;

use crate::core::subagent::SubagentProfile;

use super::super::build_context::BuildCx;
use super::super::section_source::{
    FatalError, SectionBody, SectionMeta, SectionOutcome, SectionSource,
};
use super::super::templates::{
    load_template, parse_front_matter, render_template_strict, TemplateVars,
};

const EXPLORE_TEMPLATE_REL_PATH: &str = "subagent/output_contract.explore.md";
const EXPLORE_TEMPLATE_EMBEDDED: &str =
    include_str!("../templates/subagent/output_contract.explore.md");
const REVIEW_TEMPLATE_REL_PATH: &str = "subagent/output_contract.review.md";
const REVIEW_TEMPLATE_EMBEDDED: &str =
    include_str!("../templates/subagent/output_contract.review.md");
const JUDGE_TEMPLATE_REL_PATH: &str = "subagent/output_contract.judge.md";
const JUDGE_TEMPLATE_EMBEDDED: &str =
    include_str!("../templates/subagent/output_contract.judge.md");
const DECLARED_KEYS: &[&'static str] = &[];

/// Template-backed SectionSource for the SubagentOutputContract section.
/// Replaces LegacySubagentOutputContractSource's hardcoded strings.
pub struct SubagentOutputContractSource {
    spec_version: u32,
}

impl SubagentOutputContractSource {
    pub fn new(spec_version: u32) -> Self {
        Self { spec_version }
    }
}

#[async_trait]
impl SectionSource for SubagentOutputContractSource {
    fn source_kind(&self) -> &'static str {
        "template:subagent/output_contract"
    }

    async fn build(&self, cx: &BuildCx<'_>) -> Result<SectionOutcome, FatalError> {
        let (rel_path, embedded) = match cx.helper_profile {
            Some(SubagentProfile::Explore) => {
                (EXPLORE_TEMPLATE_REL_PATH, EXPLORE_TEMPLATE_EMBEDDED)
            }
            Some(SubagentProfile::Review) => (REVIEW_TEMPLATE_REL_PATH, REVIEW_TEMPLATE_EMBEDDED),
            Some(SubagentProfile::Judge) => (JUDGE_TEMPLATE_REL_PATH, JUDGE_TEMPLATE_EMBEDDED),
            Some(SubagentProfile::Custom { .. }) => {
                // Custom subagents get a generic output contract
                return Ok(SectionOutcome::Produced(SectionBody::markdown(
                    "Your output will be consumed by the parent agent, not the user. Produce a concise, structured summary. Lead with the key conclusion, then supporting details. Reference specific file paths and code locations where relevant. Skip preamble.",
                )));
            }
            None => return Ok(SectionOutcome::Skip),
        };

        let raw = load_template(rel_path, embedded);
        let (tmpl, body) = parse_front_matter(&raw).map_err(|e| {
            FatalError::new(
                super::super::error_codes::codes::TEMPLATE_NOT_FOUND,
                format!("{}: {}", rel_path, e),
            )
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

        let vars = TemplateVars::new();
        let rendered = render_template_strict(&body, DECLARED_KEYS, &vars).map_err(|e| {
            FatalError::new(
                super::super::error_codes::codes::TEMPLATE_MISSING_KEY,
                format!("{}: {}", rel_path, e),
            )
        })?;

        Ok(SectionOutcome::Produced(SectionBody {
            markdown: rendered.trim_end().to_string(),
            meta: SectionMeta {
                template_path: Some(rel_path),
                ..Default::default()
            },
        }))
    }
}
