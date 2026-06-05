use async_trait::async_trait;
use std::borrow::Cow;

use super::super::build_context::BuildCx;
use super::super::error_codes::codes;
use super::super::section_source::{
    FatalError, SectionBody, SectionMeta, SectionOutcome, SectionSource,
};
use super::super::templates::{
    load_template, parse_front_matter, render_template_strict, TemplateVars,
};
const RUN_MODE_PLAN_TEMPLATE: &str = "run_mode.plan.md";
const RUN_MODE_PLAN_EMBEDDED: &str = include_str!("../templates/run_mode.plan.md");
const RUN_MODE_DEFAULT_TEMPLATE: &str = "run_mode.default.md";
const RUN_MODE_DEFAULT_EMBEDDED: &str = include_str!("../templates/run_mode.default.md");
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
