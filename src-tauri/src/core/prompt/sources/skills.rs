use async_trait::async_trait;

use crate::extensions::{ConfigScope, ExtensionsManager};

use super::super::build_context::BuildCx;
use super::super::section_source::{
    FatalError, SectionBody, SectionMeta, SectionOutcome, SectionSource,
};
use super::super::templates::{
    load_template, parse_front_matter, render_template_strict, TemplateVars,
};

const TEMPLATE_REL_PATH: &str = "skills_usage.md";
const TEMPLATE_EMBEDDED: &str = include_str!("../templates/skills_usage.md");
const DECLARED_KEYS: &[&'static str] = &["skills_list"];

/// Template-backed SectionSource for the Skills section.
/// Loads skills from the ExtensionsManager and renders via the skills_usage.md template.
/// Replaces LegacySkillsSource (which delegates to the old SkillsProvider).
pub struct SkillsSource;

#[async_trait]
impl SectionSource for SkillsSource {
    fn source_kind(&self) -> &'static str {
        "template:skills_usage.md"
    }

    async fn build(&self, cx: &BuildCx<'_>) -> Result<SectionOutcome, FatalError> {
        let skills = match ExtensionsManager::new(cx.pool.clone())
            .list_skills(Some(cx.workspace_path), ConfigScope::Workspace)
            .await
        {
            Ok(skills) => skills,
            Err(e) => {
                return Ok(SectionOutcome::SoftFailed {
                    code: super::super::error_codes::codes::SKILLS_LOAD_FAILED,
                    error: Box::new(std::io::Error::new(
                        std::io::ErrorKind::Other,
                        e.to_string(),
                    )),
                });
            }
        };

        let enabled_skills: Vec<_> = skills.into_iter().filter(|skill| skill.enabled).collect();

        if enabled_skills.is_empty() {
            return Ok(SectionOutcome::Skip);
        }

        let skills_list = enabled_skills
            .iter()
            .map(|s| {
                let description = s
                    .description
                    .as_deref()
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .unwrap_or("No description provided.");
                let skill_file = std::path::Path::new(&s.path).join("SKILL.md");
                format!(
                    "- {}: {} (file: {})",
                    s.name,
                    description,
                    skill_file.display()
                )
            })
            .collect::<Vec<_>>()
            .join("\n");

        let raw = load_template(TEMPLATE_REL_PATH, TEMPLATE_EMBEDDED);
        let (_tmpl, body) = parse_front_matter(&raw).map_err(|e| {
            FatalError::new(
                super::super::error_codes::codes::TEMPLATE_NOT_FOUND,
                format!("{}: {}", TEMPLATE_REL_PATH, e),
            )
        })?;

        let vars = TemplateVars::new().insert_user_text("skills_list", skills_list);
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
