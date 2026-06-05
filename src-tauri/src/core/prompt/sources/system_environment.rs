use async_trait::async_trait;

use super::super::build_context::BuildCx;
use super::super::error_codes::codes;
use super::super::section_source::{
    FatalError, SectionBody, SectionMeta, SectionOutcome, SectionSource,
};
use super::super::templates::{
    load_template, parse_front_matter, render_template_strict, TemplateVars,
};
const SYSENV_TEMPLATE_REL_PATH: &str = "system_environment.tpl.md";
const SYSENV_TEMPLATE_EMBEDDED: &str = include_str!("../templates/system_environment.tpl.md");
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
