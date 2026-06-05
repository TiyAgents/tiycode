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
const PROJCTX_TEMPLATE_REL_PATH: &str = "project_context.tpl.md";
const PROJCTX_TEMPLATE_EMBEDDED: &str = include_str!("../templates/project_context.tpl.md");
const PROJCTX_DECLARED_KEYS: &[&'static str] = &["file_name", "content", "truncated_marker"];

const WORKSPACE_INSTRUCTION_FILE_NAMES: &[&str] = &["AGENTS.md", "CLAUDE.md", "AGENT.MD"];
const WORKSPACE_INSTRUCTION_MAX_CHARS: usize = 12_800;

pub struct ProjectContextSource {
    spec_version: u32,
}

impl ProjectContextSource {
    pub fn new(spec_version: u32) -> Self {
        Self { spec_version }
    }
}

#[async_trait]
impl SectionSource for ProjectContextSource {
    fn source_kind(&self) -> &'static str {
        "template:project_context.tpl.md"
    }

    async fn build(&self, cx: &BuildCx<'_>) -> Result<SectionOutcome, FatalError> {
        let snippet = match collect_workspace_instruction_snippet(cx.workspace_path) {
            Some(s) => s,
            None => return Ok(SectionOutcome::Skip),
        };

        let raw = load_template(PROJCTX_TEMPLATE_REL_PATH, PROJCTX_TEMPLATE_EMBEDDED);
        let (tmpl, body) = parse_front_matter(&raw).map_err(|e| {
            FatalError::new(
                codes::TEMPLATE_NOT_FOUND,
                format!("{}: {}", PROJCTX_TEMPLATE_REL_PATH, e),
            )
        })?;

        if tmpl.version != self.spec_version {
            return Err(FatalError::new(
                "template.version_mismatch",
                format!(
                    "{}: template front-matter version {} != spec version {}",
                    PROJCTX_TEMPLATE_REL_PATH, tmpl.version, self.spec_version
                ),
            ));
        }

        let truncated_marker = if snippet.truncated {
            "\n[Truncated for prompt size.]"
        } else {
            ""
        };
        let vars = TemplateVars::new()
            .insert("file_name", snippet.file_name)
            .insert_user_text("content", snippet.content)
            .insert("truncated_marker", truncated_marker);

        let rendered =
            render_template_strict(&body, PROJCTX_DECLARED_KEYS, &vars).map_err(|e| {
                FatalError::new(
                    codes::TEMPLATE_MISSING_KEY,
                    format!("{}: {}", PROJCTX_TEMPLATE_REL_PATH, e),
                )
            })?;
        Ok(SectionOutcome::Produced(SectionBody {
            markdown: rendered.trim_end().to_string(),
            meta: SectionMeta {
                template_path: Some(PROJCTX_TEMPLATE_REL_PATH),
                ..Default::default()
            },
        }))
    }
}

#[derive(Debug, Clone)]
struct WorkspaceInstructionSnippet {
    file_name: &'static str,
    content: String,
    truncated: bool,
}

fn collect_workspace_instruction_snippet(
    workspace_path: &str,
) -> Option<WorkspaceInstructionSnippet> {
    use std::path::Path;
    let workspace_root = Path::new(workspace_path);
    if !workspace_root.is_dir() {
        return None;
    }

    WORKSPACE_INSTRUCTION_FILE_NAMES
        .iter()
        .find_map(|file_name| {
            let path = workspace_root.join(file_name);
            if !path.is_file() {
                return None;
            }
            let raw = std::fs::read(&path).ok()?;
            let content = normalize_prompt_doc_content(&String::from_utf8_lossy(&raw));
            if content.is_empty() {
                return None;
            }
            let (content, truncated) = truncate_chars(&content, WORKSPACE_INSTRUCTION_MAX_CHARS);
            Some(WorkspaceInstructionSnippet {
                file_name,
                content,
                truncated,
            })
        })
}

fn normalize_prompt_doc_content(value: &str) -> String {
    value
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

fn truncate_chars(value: &str, max_chars: usize) -> (String, bool) {
    let char_count = value.chars().count();
    if char_count <= max_chars {
        return (value.to_string(), false);
    }
    let truncated = value.chars().take(max_chars).collect::<String>();
    (truncated.trim_end().to_string(), true)
}
