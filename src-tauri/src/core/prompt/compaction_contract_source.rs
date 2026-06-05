use async_trait::async_trait;

use super::build_context::BuildCx;
use super::section_source::{FatalError, SectionBody, SectionMeta, SectionOutcome, SectionSource};
use super::surface::CompactionKind;
use super::templates::{load_template, parse_front_matter, render_template_strict, TemplateVars};

const COMPACT_TEMPLATE_REL_PATH: &str = "compaction/compact.md";
const COMPACT_TEMPLATE_EMBEDDED: &str = include_str!("templates/compaction/compact.md");
const MERGE_TEMPLATE_REL_PATH: &str = "compaction/merge.md";
const MERGE_TEMPLATE_EMBEDDED: &str = include_str!("templates/compaction/merge.md");
const DECLARED_KEYS: &[&'static str] = &["response_language_line"];

/// Template-backed SectionSource for the CompactionContract section.
/// Replaces LegacyCompactionContractSource's hardcoded strings.
pub struct CompactionContractSource {
    spec_version: u32,
}

impl CompactionContractSource {
    pub fn new(spec_version: u32) -> Self {
        Self { spec_version }
    }
}

#[async_trait]
impl SectionSource for CompactionContractSource {
    fn source_kind(&self) -> &'static str {
        "template:compaction"
    }

    async fn build(&self, cx: &BuildCx<'_>) -> Result<SectionOutcome, FatalError> {
        let kind = cx_compaction_kind(cx);
        let (rel_path, embedded) = match kind {
            Some(CompactionKind::Compact) => (COMPACT_TEMPLATE_REL_PATH, COMPACT_TEMPLATE_EMBEDDED),
            Some(CompactionKind::Merge) => (MERGE_TEMPLATE_REL_PATH, MERGE_TEMPLATE_EMBEDDED),
            None => return Ok(SectionOutcome::Skip),
        };

        let raw = load_template(rel_path, embedded);
        let (tmpl, body) = parse_front_matter(&raw).map_err(|e| {
            FatalError::new(
                super::error_codes::codes::TEMPLATE_NOT_FOUND,
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

        let response_language_line = build_response_language_line(cx.response_language);

        let vars = TemplateVars::new().insert("response_language_line", response_language_line);
        let rendered = render_template_strict(&body, DECLARED_KEYS, &vars).map_err(|e| {
            FatalError::new(
                super::error_codes::codes::TEMPLATE_MISSING_KEY,
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

/// Probe BuildCx to find the active compaction kind via custom_subagent_slug marker.
fn cx_compaction_kind(cx: &BuildCx<'_>) -> Option<CompactionKind> {
    match cx.custom_subagent_slug {
        Some("__compact__") => Some(CompactionKind::Compact),
        Some("__merge__") => Some(CompactionKind::Merge),
        _ => None,
    }
}

fn build_response_language_line(response_language: Option<&str>) -> String {
    match crate::core::agent_session::normalize_profile_response_language(response_language) {
        Some(language) => format!(
            "- Respond in {language} unless the user explicitly asks for a different language."
        ),
        None => String::new(),
    }
}
