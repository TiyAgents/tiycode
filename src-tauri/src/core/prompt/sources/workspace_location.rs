use async_trait::async_trait;

use super::super::build_context::BuildCx;
use super::super::error_codes::codes;
use super::super::section_source::{
    FatalError, SectionBody, SectionMeta, SectionOutcome, SectionSource,
};
use super::super::templates::{
    load_template, parse_front_matter, render_template_strict, TemplateVars,
};
const WSLOC_TEMPLATE_REL_PATH: &str = "workspace_location.tpl.md";
const WSLOC_TEMPLATE_EMBEDDED: &str = include_str!("../templates/workspace_location.tpl.md");
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
