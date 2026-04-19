use serde::{Deserialize, Serialize};

pub const IMPLEMENTATION_PLAN_MESSAGE_KIND: &str = "implementation_plan";
pub const IMPLEMENTATION_PLAN_APPROVAL_KIND: &str = "implementation_plan_approval";
pub const IMPLEMENTATION_PLAN_PENDING_STATE: &str = "pending";
pub const IMPLEMENTATION_PLAN_APPROVED_STATE: &str = "approved";
pub const IMPLEMENTATION_PLAN_SUPERSEDED_STATE: &str = "superseded";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PlanApprovalAction {
    ApplyPlan,
    ApplyPlanWithContextReset,
}

impl PlanApprovalAction {
    pub fn label(&self) -> &'static str {
        match self {
            Self::ApplyPlan => "按计划实施",
            Self::ApplyPlanWithContextReset => "清理上下文后按计划实施",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PlanStep {
    pub id: String,
    pub title: String,
    pub description: String,
    pub status: String,
    pub files: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PlanArtifact {
    pub kind: String,
    pub title: String,
    pub summary: String,
    #[serde(default)]
    pub context: String,
    #[serde(default)]
    pub design: String,
    #[serde(default)]
    pub key_implementation: String,
    pub steps: Vec<PlanStep>,
    #[serde(default)]
    pub verification: String,
    pub risks: Vec<String>,
    #[serde(default)]
    pub assumptions: Vec<String>,
    pub plan_revision: u32,
    pub needs_context_reset_option: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PlanMessageMetadata {
    #[serde(flatten)]
    pub artifact: PlanArtifact,
    pub approval_state: String,
    pub generated_from_run_id: String,
    pub run_mode_at_creation: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PlanApprovalOption {
    pub action: PlanApprovalAction,
    pub label: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ApprovalPromptMetadata {
    pub kind: String,
    pub plan_revision: u32,
    pub plan_message_id: String,
    pub state: String,
    pub options: Vec<PlanApprovalOption>,
    pub expires_on_new_user_message: bool,
    pub approved_action: Option<PlanApprovalAction>,
}

pub fn parse_plan_message_metadata(value: &serde_json::Value) -> Option<PlanMessageMetadata> {
    serde_json::from_value::<PlanMessageMetadata>(value.clone()).ok()
}

pub fn parse_approval_prompt_metadata(value: &serde_json::Value) -> Option<ApprovalPromptMetadata> {
    serde_json::from_value::<ApprovalPromptMetadata>(value.clone()).ok()
}

pub fn build_plan_message_metadata(
    artifact: PlanArtifact,
    run_id: &str,
    run_mode: &str,
) -> PlanMessageMetadata {
    PlanMessageMetadata {
        artifact,
        approval_state: IMPLEMENTATION_PLAN_PENDING_STATE.to_string(),
        generated_from_run_id: run_id.to_string(),
        run_mode_at_creation: run_mode.to_string(),
    }
}

pub fn build_approval_prompt_metadata(
    plan_revision: u32,
    plan_message_id: &str,
) -> ApprovalPromptMetadata {
    ApprovalPromptMetadata {
        kind: IMPLEMENTATION_PLAN_APPROVAL_KIND.to_string(),
        plan_revision,
        plan_message_id: plan_message_id.to_string(),
        state: IMPLEMENTATION_PLAN_PENDING_STATE.to_string(),
        options: vec![
            PlanApprovalOption {
                action: PlanApprovalAction::ApplyPlan,
                label: PlanApprovalAction::ApplyPlan.label().to_string(),
            },
            PlanApprovalOption {
                action: PlanApprovalAction::ApplyPlanWithContextReset,
                label: PlanApprovalAction::ApplyPlanWithContextReset
                    .label()
                    .to_string(),
            },
        ],
        expires_on_new_user_message: true,
        approved_action: None,
    }
}

pub fn plan_markdown(metadata: &PlanMessageMetadata) -> String {
    let artifact = &metadata.artifact;
    let mut lines = vec![format!("# {}", artifact.title)];

    if !artifact.summary.trim().is_empty() {
        lines.push(String::new());
        lines.push(artifact.summary.trim().to_string());
    }

    append_prose_section(&mut lines, "Context", &artifact.context);
    append_prose_section(&mut lines, "Design", &artifact.design);
    append_prose_section(
        &mut lines,
        "Key Implementation",
        &artifact.key_implementation,
    );

    if !artifact.steps.is_empty() {
        lines.push(String::new());
        lines.push("## Steps".to_string());
        for (index, step) in artifact.steps.iter().enumerate() {
            let mut line = format!("{}. {}", index + 1, step.title);
            if !step.description.trim().is_empty() {
                line.push_str(&format!(" — {}", step.description.trim()));
            }
            if !step.files.is_empty() {
                line.push_str(&format!(" ({})", step.files.join(", ")));
            }
            lines.push(line);
        }
    }

    append_prose_section(&mut lines, "Verification", &artifact.verification);
    append_string_section(&mut lines, "Risks", &artifact.risks);
    append_string_section(&mut lines, "Assumptions", &artifact.assumptions);

    lines.join("\n")
}

pub fn approval_prompt_markdown(artifact: &PlanArtifact) -> String {
    format!(
        "Review **{}** and choose how to start implementation.",
        artifact.title
    )
}

pub fn build_plan_artifact_from_tool_input(
    tool_input: &serde_json::Value,
    plan_revision: u32,
) -> PlanArtifact {
    let root = tool_input
        .get("plan")
        .and_then(serde_json::Value::as_object)
        .or_else(|| tool_input.as_object());

    let title = root
        .and_then(|value| value.get("title"))
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("Implementation Plan")
        .to_string();

    let summary = root
        .and_then(|value| {
            value
                .get("summary")
                .or_else(|| value.get("description"))
                .or_else(|| value.get("overview"))
        })
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("Review the proposed implementation plan before coding.")
        .to_string();

    let steps = root
        .and_then(|value| value.get("steps"))
        .and_then(serde_json::Value::as_array)
        .map(|steps| {
            steps
                .iter()
                .enumerate()
                .filter_map(|(index, step)| parse_step(step, index))
                .collect::<Vec<_>>()
        })
        .filter(|steps| !steps.is_empty())
        .unwrap_or_else(|| {
            vec![PlanStep {
                id: "step-1".to_string(),
                title: "Review the approved plan details".to_string(),
                description: summary.clone(),
                status: "pending".to_string(),
                files: Vec::new(),
            }]
        });

    let context = read_prose_field(root.and_then(|value| value.get("context")));
    let design = read_prose_field(root.and_then(|value| value.get("design")));
    let key_implementation =
        read_prose_field(root.and_then(|value| value.get("keyImplementation")));
    let verification = read_prose_field(root.and_then(|value| value.get("verification")));
    let risks = read_string_list(root.and_then(|value| value.get("risks")));
    let assumptions = read_string_list(root.and_then(|value| value.get("assumptions")));
    let needs_context_reset_option = root
        .and_then(|value| value.get("needsContextResetOption"))
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(true);

    PlanArtifact {
        kind: IMPLEMENTATION_PLAN_MESSAGE_KIND.to_string(),
        title,
        summary,
        context,
        design,
        key_implementation,
        steps,
        verification,
        risks,
        assumptions,
        plan_revision,
        needs_context_reset_option,
    }
}

fn append_prose_section(lines: &mut Vec<String>, title: &str, content: &str) {
    let trimmed = content.trim();
    if trimmed.is_empty() {
        return;
    }

    lines.push(String::new());
    lines.push(format!("## {}", title));
    lines.push(trimmed.to_string());
}

fn append_string_section(lines: &mut Vec<String>, title: &str, items: &[String]) {
    if items.is_empty() {
        return;
    }

    lines.push(String::new());
    lines.push(format!("## {}", title));
    for item in items {
        lines.push(format!("- {}", item));
    }
}

fn parse_step(step: &serde_json::Value, index: usize) -> Option<PlanStep> {
    if let Some(value) = step
        .as_str()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        return Some(PlanStep {
            id: format!("step-{}", index + 1),
            title: value.to_string(),
            description: String::new(),
            status: "pending".to_string(),
            files: Vec::new(),
        });
    }

    let object = step.as_object()?;
    let title = object
        .get("title")
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .or_else(|| {
            object
                .get("description")
                .and_then(serde_json::Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
        })?
        .to_string();

    let description = object
        .get("description")
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .unwrap_or_default()
        .to_string();

    let status = object
        .get("status")
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("pending")
        .to_string();

    let files = read_string_list(object.get("files"));
    let id = object
        .get("id")
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
        .unwrap_or_else(|| format!("step-{}", index + 1));

    Some(PlanStep {
        id,
        title,
        description,
        status,
        files,
    })
}

fn read_string_list(value: Option<&serde_json::Value>) -> Vec<String> {
    match value {
        Some(serde_json::Value::String(single)) if !single.trim().is_empty() => {
            vec![single.trim().to_string()]
        }
        Some(serde_json::Value::Array(entries)) => entries
            .iter()
            .filter_map(serde_json::Value::as_str)
            .map(str::trim)
            .filter(|entry| !entry.is_empty())
            .map(ToString::to_string)
            .collect::<Vec<_>>(),
        _ => Vec::new(),
    }
}

fn read_prose_field(value: Option<&serde_json::Value>) -> String {
    match value {
        Some(serde_json::Value::String(text)) => text.trim().to_string(),
        Some(serde_json::Value::Array(entries)) => entries
            .iter()
            .filter_map(serde_json::Value::as_str)
            .map(str::trim)
            .filter(|entry| !entry.is_empty())
            .collect::<Vec<_>>()
            .join("\n\n"),
        _ => String::new(),
    }
}

// ---------------------------------------------------------------------------
// Plan file persistence — write plan markdown to ~/.tiy/plans/{thread_id}.md
// ---------------------------------------------------------------------------

use std::path::{Path, PathBuf};

/// Return the directory used for plan file persistence.
pub fn plan_file_dir() -> Option<PathBuf> {
    dirs::home_dir().map(|home| home.join(".tiy").join("plans"))
}

/// Return the plan file path for a given thread.
pub fn plan_file_path(thread_id: &str) -> Option<PathBuf> {
    plan_file_dir().map(|dir| dir.join(format!("{thread_id}.md")))
}

/// Write plan markdown to disk. Creates the directory if needed.
/// Returns the written path on success.
pub fn write_plan_file(thread_id: &str, markdown: &str) -> Result<PathBuf, std::io::Error> {
    let dir = plan_file_dir().ok_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::NotFound, "home directory not available")
    })?;
    write_plan_file_to(&dir, thread_id, markdown)
}

/// Write plan markdown to an explicit directory. Creates it if needed.
/// Returns the written path on success.
fn write_plan_file_to(
    dir: &Path,
    thread_id: &str,
    markdown: &str,
) -> Result<PathBuf, std::io::Error> {
    std::fs::create_dir_all(dir)?;
    let path = dir.join(format!("{thread_id}.md"));
    std::fs::write(&path, markdown)?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::{
        approval_prompt_markdown, build_approval_prompt_metadata,
        build_plan_artifact_from_tool_input, build_plan_message_metadata,
        parse_approval_prompt_metadata, parse_plan_message_metadata, plan_markdown,
        write_plan_file_to, PlanApprovalAction,
    };

    #[test]
    fn plan_artifact_builder_accepts_string_and_object_steps() {
        let artifact = build_plan_artifact_from_tool_input(
            &serde_json::json!({
                "title": "Runtime refactor",
                "summary": "Refactor plan mode behavior.",
                "context": "Plan artifact only stores a subset of planning data today.",
                "design": "Expand the artifact and keep older plans compatible.",
                "keyImplementation": "Update plan checkpoint parsing and rendering.",
                "steps": [
                    "Remove helper plan tool",
                    {
                        "title": "Persist approval prompt",
                        "description": "Store approval prompt as a message",
                        "files": ["src-tauri/src/core/agent_run_manager.rs"]
                    }
                ],
                "verification": "Run Rust tests covering plan artifact parsing.",
                "risks": ["State drift"],
                "assumptions": ["Plan mode continues to pause before implementation."]
            }),
            3,
        );

        assert_eq!(artifact.plan_revision, 3);
        assert_eq!(
            artifact.context,
            "Plan artifact only stores a subset of planning data today."
        );
        assert_eq!(
            artifact.key_implementation,
            "Update plan checkpoint parsing and rendering."
        );
        assert_eq!(artifact.steps.len(), 2);
        assert_eq!(artifact.steps[0].title, "Remove helper plan tool");
        assert_eq!(
            artifact.steps[1].files,
            vec!["src-tauri/src/core/agent_run_manager.rs"]
        );
        assert_eq!(
            artifact.verification,
            "Run Rust tests covering plan artifact parsing."
        );
    }

    #[test]
    fn metadata_round_trip_is_stable() {
        let artifact = build_plan_artifact_from_tool_input(
            &serde_json::json!({
                "title": "Implement checkpoint",
                "summary": "Pause before execution."
            }),
            1,
        );
        let metadata = build_plan_message_metadata(artifact.clone(), "run-1", "plan");
        let parsed =
            parse_plan_message_metadata(&serde_json::to_value(&metadata).unwrap()).unwrap();
        assert_eq!(parsed.artifact, artifact);

        let approval = build_approval_prompt_metadata(artifact.plan_revision, "msg-plan");
        let parsed_approval =
            parse_approval_prompt_metadata(&serde_json::to_value(&approval).unwrap()).unwrap();
        assert_eq!(parsed_approval.plan_revision, 1);
        assert_eq!(parsed_approval.options.len(), 2);
        assert_eq!(
            parsed_approval.options[0].action,
            PlanApprovalAction::ApplyPlan
        );
        assert!(approval_prompt_markdown(&artifact).contains("Implement checkpoint"));
    }

    #[test]
    fn plan_markdown_renders_new_structured_sections() {
        let artifact = build_plan_artifact_from_tool_input(
            &serde_json::json!({
                "title": "Structured plan",
                "summary": "Publish a richer planning artifact.",
                "context": "The current artifact only stores summary, steps, and risks.",
                "design": "Add new sections as structured prose fields.",
                "keyImplementation": "Extend Rust schema and frontend parsing.",
                "steps": ["Update the plan artifact builder."],
                "verification": "Run plan-related Rust and TypeScript verification.",
                "risks": ["Older plans must remain readable."],
                "assumptions": ["Historical messages may omit new fields."]
            }),
            2,
        );
        let metadata = build_plan_message_metadata(artifact, "run-2", "plan");
        let markdown = plan_markdown(&metadata);

        assert!(markdown.contains("## Context"));
        assert!(markdown.contains("## Design"));
        assert!(markdown.contains("## Key Implementation"));
        assert!(markdown.contains("## Verification"));
        assert!(markdown.contains("## Assumptions"));
    }

    #[test]
    fn write_plan_file_creates_and_overwrites() {
        // Use a dedicated tempdir so this test never races with parallel tests
        // that mutate $HOME.
        let temp_dir = tempfile::tempdir().expect("tempdir");
        let plans_dir = temp_dir.path().join("plans");
        let thread_id = format!("test-plan-file-{}", std::process::id());

        // First write creates the file.
        let path = write_plan_file_to(&plans_dir, &thread_id, "# Revision 1\n\nFirst draft.")
            .expect("first write should succeed");
        assert!(path.exists());
        let content = std::fs::read_to_string(&path).expect("read first revision");
        assert!(content.contains("# Revision 1"));

        // Second write overwrites the same file.
        let path2 = write_plan_file_to(&plans_dir, &thread_id, "# Revision 2\n\nRefined plan.")
            .expect("second write should succeed");
        assert_eq!(path, path2);
        let content = std::fs::read_to_string(&path2).expect("read second revision");
        assert!(content.contains("# Revision 2"));
        assert!(!content.contains("# Revision 1"));
    }
}
