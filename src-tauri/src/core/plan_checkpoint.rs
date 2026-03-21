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
    pub steps: Vec<PlanStep>,
    pub risks: Vec<String>,
    pub open_questions: Vec<String>,
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
                label: PlanApprovalAction::ApplyPlanWithContextReset.label().to_string(),
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

    if !artifact.risks.is_empty() {
        lines.push(String::new());
        lines.push("## Risks".to_string());
        for risk in &artifact.risks {
            lines.push(format!("- {}", risk));
        }
    }

    if !artifact.open_questions.is_empty() {
        lines.push(String::new());
        lines.push("## Open Questions".to_string());
        for question in &artifact.open_questions {
            lines.push(format!("- {}", question));
        }
    }

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
        .and_then(|value| value.get("summary").or_else(|| value.get("description")).or_else(|| value.get("overview")))
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

    let risks = read_string_list(root.and_then(|value| value.get("risks")));
    let open_questions = read_string_list(
        root.and_then(|value| value.get("openQuestions").or_else(|| value.get("open_questions"))),
    );
    let needs_context_reset_option = root
        .and_then(|value| value.get("needsContextResetOption"))
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(true);

    PlanArtifact {
        kind: IMPLEMENTATION_PLAN_MESSAGE_KIND.to_string(),
        title,
        summary,
        steps,
        risks,
        open_questions,
        plan_revision,
        needs_context_reset_option,
    }
}

fn parse_step(step: &serde_json::Value, index: usize) -> Option<PlanStep> {
    if let Some(value) = step.as_str().map(str::trim).filter(|value| !value.is_empty()) {
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
    value
        .and_then(serde_json::Value::as_array)
        .map(|entries| {
            entries
                .iter()
                .filter_map(serde_json::Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(ToString::to_string)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::{
        approval_prompt_markdown, build_approval_prompt_metadata,
        build_plan_artifact_from_tool_input, build_plan_message_metadata,
        parse_approval_prompt_metadata, parse_plan_message_metadata, PlanApprovalAction,
    };

    #[test]
    fn plan_artifact_builder_accepts_string_and_object_steps() {
        let artifact = build_plan_artifact_from_tool_input(
            &serde_json::json!({
                "title": "Runtime refactor",
                "summary": "Refactor plan mode behavior.",
                "steps": [
                    "Remove helper plan tool",
                    {
                        "title": "Persist approval prompt",
                        "description": "Store approval prompt as a message",
                        "files": ["src-tauri/src/core/agent_run_manager.rs"]
                    }
                ],
                "risks": ["State drift"],
                "openQuestions": ["Need a new event?"]
            }),
            3,
        );

        assert_eq!(artifact.plan_revision, 3);
        assert_eq!(artifact.steps.len(), 2);
        assert_eq!(artifact.steps[0].title, "Remove helper plan tool");
        assert_eq!(artifact.steps[1].files, vec!["src-tauri/src/core/agent_run_manager.rs"]);
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
        let parsed = parse_plan_message_metadata(&serde_json::to_value(&metadata).unwrap()).unwrap();
        assert_eq!(parsed.artifact, artifact);

        let approval = build_approval_prompt_metadata(artifact.plan_revision, "msg-plan");
        let parsed_approval =
            parse_approval_prompt_metadata(&serde_json::to_value(&approval).unwrap()).unwrap();
        assert_eq!(parsed_approval.plan_revision, 1);
        assert_eq!(parsed_approval.options.len(), 2);
        assert_eq!(parsed_approval.options[0].action, PlanApprovalAction::ApplyPlan);
        assert!(approval_prompt_markdown(&artifact).contains("Implement checkpoint"));
    }
}
