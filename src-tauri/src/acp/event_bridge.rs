use agent_client_protocol::schema::{
    ContentBlock, ContentChunk, Plan, PlanEntry, PlanEntryPriority, PlanEntryStatus, SessionUpdate,
    StopReason, TextContent, ToolCall, ToolCallContent, ToolCallStatus, ToolCallUpdate,
    ToolCallUpdateFields, ToolKind,
};

use crate::ipc::frontend_channels::{ArtifactStatus, ThreadStreamEvent};
use crate::model::task_item::TaskStage;

#[derive(Debug, Clone, PartialEq)]
pub struct AcpEventMapping {
    pub updates: Vec<SessionUpdate>,
    pub stop_reason: Option<StopReason>,
    pub terminal_error: Option<String>,
}

impl AcpEventMapping {
    fn updates(updates: Vec<SessionUpdate>) -> Self {
        Self {
            updates,
            stop_reason: None,
            terminal_error: None,
        }
    }

    fn stop(stop_reason: StopReason, terminal_error: Option<String>) -> Self {
        Self {
            updates: Vec::new(),
            stop_reason: Some(stop_reason),
            terminal_error,
        }
    }
}

pub fn map_thread_event_to_acp(event: &ThreadStreamEvent) -> AcpEventMapping {
    match event {
        ThreadStreamEvent::MessageDelta {
            message_id, delta, ..
        } => AcpEventMapping::updates(vec![SessionUpdate::AgentMessageChunk(text_chunk(
            delta.clone(),
            Some(message_id.clone()),
        ))]),
        ThreadStreamEvent::ReasoningUpdated {
            message_id,
            reasoning,
            ..
        } if !reasoning.trim().is_empty() => {
            AcpEventMapping::updates(vec![SessionUpdate::AgentThoughtChunk(text_chunk(
                reasoning.clone(),
                Some(message_id.clone()),
            ))])
        }
        ThreadStreamEvent::ToolRequested {
            tool_call_id,
            tool_name,
            tool_input,
            ..
        } => AcpEventMapping::updates(vec![SessionUpdate::ToolCall(
            ToolCall::new(tool_call_id.clone(), tool_title(tool_name))
                .kind(tool_kind(tool_name))
                .status(ToolCallStatus::Pending)
                .raw_input(Some(tool_input.clone())),
        )]),
        ThreadStreamEvent::ApprovalRequired {
            tool_call_id,
            tool_name,
            tool_input,
            reason,
            ..
        } => AcpEventMapping::updates(vec![SessionUpdate::ToolCallUpdate(ToolCallUpdate::new(
            tool_call_id.clone(),
            ToolCallUpdateFields::new()
                .title(Some(format!(
                    "Awaiting approval: {}",
                    tool_title(tool_name)
                )))
                .kind(Some(tool_kind(tool_name)))
                .status(Some(ToolCallStatus::Pending))
                .raw_input(Some(tool_input.clone()))
                .content(Some(vec![text_tool_content(reason.clone())])),
        ))]),
        ThreadStreamEvent::ToolRunning { tool_call_id, .. } => {
            AcpEventMapping::updates(vec![SessionUpdate::ToolCallUpdate(ToolCallUpdate::new(
                tool_call_id.clone(),
                ToolCallUpdateFields::new().status(Some(ToolCallStatus::InProgress)),
            ))])
        }
        ThreadStreamEvent::ToolCompleted {
            tool_call_id,
            tool_name,
            result,
            ..
        } => AcpEventMapping::updates(vec![SessionUpdate::ToolCallUpdate(ToolCallUpdate::new(
            tool_call_id.clone(),
            ToolCallUpdateFields::new()
                .title(Some(format!("Completed: {}", tool_title(tool_name))))
                .kind(Some(tool_kind(tool_name)))
                .status(Some(ToolCallStatus::Completed))
                .raw_output(Some(result.clone()))
                .content(Some(vec![text_tool_content(json_summary(result))])),
        ))]),
        ThreadStreamEvent::ToolFailed {
            tool_call_id,
            error,
            ..
        } => AcpEventMapping::updates(vec![SessionUpdate::ToolCallUpdate(ToolCallUpdate::new(
            tool_call_id.clone(),
            ToolCallUpdateFields::new()
                .status(Some(ToolCallStatus::Failed))
                .content(Some(vec![text_tool_content(error.clone())])),
        ))]),
        ThreadStreamEvent::PlanUpdated { plan, .. } => {
            AcpEventMapping::updates(vec![SessionUpdate::Plan(plan_from_value(plan))])
        }
        ThreadStreamEvent::TaskBoardUpdated { task_board, .. } => {
            let entries = task_board
                .tasks
                .iter()
                .map(|task| {
                    let (content, status) = match task.stage {
                        TaskStage::Pending => (task.description.clone(), PlanEntryStatus::Pending),
                        TaskStage::InProgress => {
                            (task.description.clone(), PlanEntryStatus::InProgress)
                        }
                        TaskStage::Completed => {
                            (task.description.clone(), PlanEntryStatus::Completed)
                        }
                        // ACP v0.11 plan entries only support pending/in_progress/completed.
                        // Preserve failed task visibility by marking the entry text while using
                        // the closest terminal status the schema can represent.
                        TaskStage::Failed => (
                            format!("[failed] {}", task.description),
                            PlanEntryStatus::Completed,
                        ),
                    };
                    PlanEntry::new(content, PlanEntryPriority::Medium, status)
                })
                .collect();
            AcpEventMapping::updates(vec![SessionUpdate::Plan(Plan::new(entries))])
        }
        ThreadStreamEvent::ArtifactUpdated {
            artifact_type,
            status,
            error,
            ..
        } => {
            let label = match status {
                ArtifactStatus::Started => "Artifact started",
                ArtifactStatus::Delta => "Artifact updated",
                ArtifactStatus::Completed => "Artifact completed",
                ArtifactStatus::Failed => "Artifact failed",
            };
            let mut message = format!("{label}: {artifact_type}");
            if let Some(error) = error {
                message.push_str(&format!("\n{error}"));
            }
            AcpEventMapping::updates(vec![SessionUpdate::AgentThoughtChunk(text_chunk(
                message, None,
            ))])
        }
        ThreadStreamEvent::ThreadTitleUpdated { title, .. } => {
            AcpEventMapping::updates(vec![SessionUpdate::SessionInfoUpdate(
                agent_client_protocol::schema::SessionInfoUpdate::new().title(Some(title.clone())),
            )])
        }
        ThreadStreamEvent::StreamResyncRequired { dropped_events, .. } => {
            AcpEventMapping::updates(vec![SessionUpdate::AgentThoughtChunk(text_chunk(
                format!("Stream resync required; {dropped_events} events were dropped."),
                None,
            ))])
        }
        ThreadStreamEvent::RunCompleted { .. } => AcpEventMapping::stop(StopReason::EndTurn, None),
        ThreadStreamEvent::RunLimitReached { error, .. } => {
            AcpEventMapping::stop(StopReason::MaxTurnRequests, Some(error.clone()))
        }
        ThreadStreamEvent::RunFailed { error, .. } => {
            AcpEventMapping::stop(StopReason::Refusal, Some(error.clone()))
        }
        ThreadStreamEvent::RunCancelled { .. } => {
            AcpEventMapping::stop(StopReason::Cancelled, None)
        }
        ThreadStreamEvent::RunInterrupted { .. } => {
            AcpEventMapping::stop(StopReason::Refusal, Some("Run interrupted".to_string()))
        }
        _ => AcpEventMapping::updates(Vec::new()),
    }
}

pub fn content_blocks_to_markdown(blocks: &[ContentBlock]) -> String {
    let mut parts = Vec::new();
    for block in blocks {
        match block {
            ContentBlock::Text(text) => parts.push(text.text.clone()),
            ContentBlock::ResourceLink(link) => {
                parts.push(format!("[resource]({})", link.uri));
            }
            ContentBlock::Resource(resource) => {
                parts.push(format!("[embedded resource: {:?}]", resource.resource));
            }
            ContentBlock::Image(image) => {
                let label = image.uri.as_deref().unwrap_or("embedded image");
                parts.push(format!("[image: {label} ({})]", image.mime_type));
            }
            ContentBlock::Audio(audio) => {
                parts.push(format!("[audio: {}]", audio.mime_type));
            }
            _ => parts.push("[unsupported content block]".to_string()),
        }
    }
    parts.join("\n\n")
}

fn text_chunk(text: String, message_id: Option<String>) -> ContentChunk {
    ContentChunk::new(ContentBlock::Text(TextContent::new(text))).message_id(message_id)
}

fn text_tool_content(text: String) -> ToolCallContent {
    ToolCallContent::from(ContentBlock::Text(TextContent::new(text)))
}

fn tool_title(tool_name: &str) -> String {
    tool_name.replace('_', " ")
}

fn tool_kind(tool_name: &str) -> ToolKind {
    match tool_name {
        "read" | "list" | "find" | "git_status" | "git_diff" | "git_log" => ToolKind::Read,
        "write" | "edit" | "patch" | "create_dir" => ToolKind::Edit,
        "delete" => ToolKind::Delete,
        "rename" => ToolKind::Move,
        "search" | "web_search" => ToolKind::Search,
        "shell" | "term_write" | "term_restart" | "term_close" | "term_status" | "term_output" => {
            ToolKind::Execute
        }
        "update_plan" | "create_task" | "update_task" | "query_task" => ToolKind::Think,
        _ => ToolKind::Other,
    }
}

fn plan_from_value(value: &serde_json::Value) -> Plan {
    let entries = value
        .get("steps")
        .and_then(|steps| steps.as_array())
        .map(|steps| {
            steps
                .iter()
                .enumerate()
                .map(|(index, step)| {
                    let content = step
                        .get("description")
                        .or_else(|| step.get("title"))
                        .and_then(|value| value.as_str())
                        .map(ToString::to_string)
                        .unwrap_or_else(|| format!("Step {}", index + 1));
                    let status = match step.get("status").and_then(|value| value.as_str()) {
                        Some("in_progress") | Some("active") | Some("running") => {
                            PlanEntryStatus::InProgress
                        }
                        Some("completed") | Some("done") => PlanEntryStatus::Completed,
                        _ => PlanEntryStatus::Pending,
                    };
                    PlanEntry::new(content, PlanEntryPriority::Medium, status)
                })
                .collect::<Vec<_>>()
        })
        .filter(|entries| !entries.is_empty())
        .unwrap_or_else(|| {
            let summary = value
                .get("summary")
                .and_then(|value| value.as_str())
                .or_else(|| value.get("title").and_then(|value| value.as_str()))
                .unwrap_or("Implementation plan updated");
            vec![PlanEntry::new(
                summary.to_string(),
                PlanEntryPriority::Medium,
                PlanEntryStatus::InProgress,
            )]
        });
    Plan::new(entries)
}

fn json_summary(value: &serde_json::Value) -> String {
    let rendered = serde_json::to_string_pretty(value).unwrap_or_else(|_| value.to_string());
    let max_chars = 4_000;
    if rendered.chars().count() > max_chars {
        let truncated: String = rendered.chars().take(max_chars).collect();
        format!("{truncated}\n…")
    } else {
        rendered
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn message_delta_maps_to_agent_message_chunk() {
        let event = ThreadStreamEvent::MessageDelta {
            run_id: "run-1".to_string(),
            message_id: "message-1".to_string(),
            delta: "hello".to_string(),
        };

        let mapping = map_thread_event_to_acp(&event);
        assert_eq!(mapping.stop_reason, None);
        assert!(matches!(
            mapping.updates.as_slice(),
            [SessionUpdate::AgentMessageChunk(_)]
        ));
    }

    #[test]
    fn tool_events_map_to_tool_call_updates() {
        let event = ThreadStreamEvent::ToolCompleted {
            run_id: "run-1".to_string(),
            tool_call_id: "tool-1".to_string(),
            tool_name: "edit".to_string(),
            result: json!({"path":"/tmp/file.rs"}),
        };

        let mapping = map_thread_event_to_acp(&event);
        match mapping.updates.as_slice() {
            [SessionUpdate::ToolCallUpdate(update)] => {
                assert_eq!(update.tool_call_id.0.as_ref(), "tool-1");
                assert_eq!(update.fields.status, Some(ToolCallStatus::Completed));
                assert_eq!(update.fields.kind, Some(ToolKind::Edit));
            }
            other => panic!("unexpected updates: {other:?}"),
        }
    }

    #[test]
    fn terminal_events_map_to_stop_reasons() {
        assert_eq!(
            map_thread_event_to_acp(&ThreadStreamEvent::RunCompleted {
                run_id: "run-1".to_string()
            })
            .stop_reason,
            Some(StopReason::EndTurn)
        );
        assert_eq!(
            map_thread_event_to_acp(&ThreadStreamEvent::RunCancelled {
                run_id: "run-1".to_string()
            })
            .stop_reason,
            Some(StopReason::Cancelled)
        );
    }

    #[test]
    fn plan_json_maps_to_acp_plan_entries() {
        let plan = plan_from_value(&json!({
            "steps": [
                {"description": "Inspect code", "status": "completed"},
                {"description": "Implement bridge", "status": "in_progress"}
            ]
        }));

        assert_eq!(plan.entries.len(), 2);
        assert_eq!(plan.entries[0].status, PlanEntryStatus::Completed);
        assert_eq!(plan.entries[1].status, PlanEntryStatus::InProgress);
    }

    #[test]
    fn task_board_failed_steps_remain_visible_in_acp_plan() {
        let event = ThreadStreamEvent::TaskBoardUpdated {
            run_id: "run-1".to_string(),
            task_board: crate::model::task_board::TaskBoardDto {
                id: "board-1".to_string(),
                thread_id: "thread-1".to_string(),
                title: "Plan".to_string(),
                status: crate::model::task_board::TaskBoardStatus::Active,
                active_task_id: None,
                tasks: vec![crate::model::task_item::TaskItemDto {
                    id: "task-1".to_string(),
                    task_board_id: "board-1".to_string(),
                    description: "Run verification".to_string(),
                    stage: TaskStage::Failed,
                    sort_order: 0,
                    error_detail: Some("failed".to_string()),
                    created_at: "2026-05-27T00:00:00Z".to_string(),
                    updated_at: "2026-05-27T00:00:00Z".to_string(),
                }],
                created_at: "2026-05-27T00:00:00Z".to_string(),
                updated_at: "2026-05-27T00:00:00Z".to_string(),
            },
        };

        let mapping = map_thread_event_to_acp(&event);
        match mapping.updates.as_slice() {
            [SessionUpdate::Plan(plan)] => {
                assert_eq!(plan.entries[0].status, PlanEntryStatus::Completed);
                assert!(plan.entries[0].content.starts_with("[failed] "));
            }
            other => panic!("unexpected updates: {other:?}"),
        }
    }
}
