use async_trait::async_trait;

use super::super::build_context::BuildCx;
use super::super::section_source::{
    FatalError, SectionBody, SectionMeta, SectionOutcome, SectionSource,
};
use super::super::templates::{
    load_template, parse_front_matter, render_template_strict, TemplateVars,
};

const TEMPLATE_REL_PATH: &str = "title/contract.md";
const TEMPLATE_EMBEDDED: &str = include_str!("../templates/title/contract.md");
const DECLARED_KEYS: &[&'static str] = &[];

/// Template-backed SectionSource for the TitleContract section.
/// Replaces LegacyTitleContractSource's hardcoded string.
pub struct TitleContractSource {
    spec_version: u32,
}

impl TitleContractSource {
    pub fn new(spec_version: u32) -> Self {
        Self { spec_version }
    }
}

#[async_trait]
impl SectionSource for TitleContractSource {
    fn source_kind(&self) -> &'static str {
        "template:title/contract.md"
    }

    async fn build(&self, _cx: &BuildCx<'_>) -> Result<SectionOutcome, FatalError> {
        let raw = load_template(TEMPLATE_REL_PATH, TEMPLATE_EMBEDDED);
        let (tmpl, body) = parse_front_matter(&raw).map_err(|e| {
            FatalError::new(
                super::super::error_codes::codes::TEMPLATE_NOT_FOUND,
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

        let vars = TemplateVars::new();
        let rendered = render_template_strict(&body, DECLARED_KEYS, &vars).map_err(|e| {
            FatalError::new(
                super::super::error_codes::codes::TEMPLATE_MISSING_KEY,
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
