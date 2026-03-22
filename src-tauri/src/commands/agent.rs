use tauri::{ipc::Channel, State};
use tokio::sync::broadcast;

use crate::core::app_state::AppState;
use crate::core::plan_checkpoint::PlanApprovalAction;
use crate::ipc::frontend_channels::ThreadStreamEvent;
use crate::model::errors::AppError;

fn extract_run_string(model_plan: &serde_json::Value, path: &[&str]) -> Option<String> {
    let mut current = model_plan;

    for segment in path {
        current = current.get(*segment)?;
    }

    current.as_str().map(ToString::to_string)
}

fn extract_run_model_refs(
    model_plan: &serde_json::Value,
) -> (Option<String>, Option<String>, Option<String>) {
    (
        extract_run_string(model_plan, &["profileId"]),
        extract_run_string(model_plan, &["primary", "providerId"]),
        extract_run_string(model_plan, &["primary", "modelRecordId"])
            .or_else(|| extract_run_string(model_plan, &["primary", "modelId"])),
    )
}

fn forward_thread_stream_events(
    run_id: String,
    mut event_rx: broadcast::Receiver<ThreadStreamEvent>,
    on_event: Channel<ThreadStreamEvent>,
) {
    tokio::spawn(async move {
        loop {
            match event_rx.recv().await {
                Ok(event) => {
                    if on_event.send(event).is_err() {
                        break;
                    }
                }
                Err(broadcast::error::RecvError::Closed) => break,
                Err(broadcast::error::RecvError::Lagged(dropped_events)) => {
                    if on_event
                        .send(ThreadStreamEvent::StreamResyncRequired {
                            run_id: run_id.clone(),
                            dropped_events: dropped_events as u64,
                        })
                        .is_err()
                    {
                        break;
                    }
                }
            }
        }
    });
}

#[tauri::command]
pub async fn thread_start_run(
    state: State<'_, AppState>,
    thread_id: String,
    prompt: String,
    display_prompt: Option<String>,
    prompt_metadata: Option<serde_json::Value>,
    run_mode: Option<String>,
    model_plan: Option<serde_json::Value>,
    on_event: Channel<ThreadStreamEvent>,
) -> Result<String, AppError> {
    let run_mode = run_mode.unwrap_or_else(|| "default".to_string());
    let model_plan = model_plan.unwrap_or_default();
    let (profile_id, provider_id, model_id) = extract_run_model_refs(&model_plan);

    let (run_id, event_rx) = state
        .agent_run_manager
        .clone()
        .start_run(
            &thread_id,
            &prompt,
            display_prompt,
            prompt_metadata,
            &run_mode,
            profile_id,
            provider_id,
            model_id,
            model_plan,
        )
        .await?;

    // Forward events from the internal channel to the Tauri Channel
    forward_thread_stream_events(run_id.clone(), event_rx, on_event);

    Ok(run_id)
}

#[tauri::command]
pub async fn thread_subscribe_run(
    state: State<'_, AppState>,
    thread_id: String,
    on_event: Channel<ThreadStreamEvent>,
) -> Result<Option<String>, AppError> {
    let Some((run_id, event_rx)) = state.agent_run_manager.subscribe_run(&thread_id).await? else {
        return Ok(None);
    };

    forward_thread_stream_events(run_id.clone(), event_rx, on_event);
    Ok(Some(run_id))
}

#[tauri::command]
pub async fn thread_execute_approved_plan(
    state: State<'_, AppState>,
    thread_id: String,
    approval_message_id: String,
    action: String,
    on_event: Channel<ThreadStreamEvent>,
) -> Result<String, AppError> {
    let action = match action.as_str() {
        "apply_plan" => PlanApprovalAction::ApplyPlan,
        "apply_plan_with_context_reset" => PlanApprovalAction::ApplyPlanWithContextReset,
        other => {
            return Err(AppError::recoverable(
                crate::model::errors::ErrorSource::Thread,
                "thread.plan_approval.invalid_action",
                format!("Unsupported plan approval action '{other}'"),
            ));
        }
    };

    let (run_id, event_rx) = state
        .agent_run_manager
        .clone()
        .execute_approved_plan(&thread_id, &approval_message_id, action)
        .await?;

    forward_thread_stream_events(run_id.clone(), event_rx, on_event);
    Ok(run_id)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::extract_run_model_refs;

    #[test]
    fn extracts_profile_and_primary_refs_from_model_plan() {
        let model_plan = json!({
            "profileId": "profile-1",
            "primary": {
                "providerId": "provider-1",
                "modelRecordId": "model-record-1",
                "modelId": "gpt-5"
            }
        });

        let (profile_id, provider_id, model_id) = extract_run_model_refs(&model_plan);

        assert_eq!(profile_id.as_deref(), Some("profile-1"));
        assert_eq!(provider_id.as_deref(), Some("provider-1"));
        assert_eq!(model_id.as_deref(), Some("model-record-1"));
    }

    #[test]
    fn falls_back_to_primary_model_id_when_record_id_is_missing() {
        let model_plan = json!({
            "primary": {
                "providerId": "provider-1",
                "modelId": "gpt-5"
            }
        });

        let (_, provider_id, model_id) = extract_run_model_refs(&model_plan);

        assert_eq!(provider_id.as_deref(), Some("provider-1"));
        assert_eq!(model_id.as_deref(), Some("gpt-5"));
    }
}

#[tauri::command]
pub async fn thread_clear_context(
    state: State<'_, AppState>,
    thread_id: String,
) -> Result<(), AppError> {
    state.agent_run_manager.clear_thread_context(&thread_id).await
}

#[tauri::command]
pub async fn thread_compact_context(
    state: State<'_, AppState>,
    thread_id: String,
    instructions: Option<String>,
) -> Result<(), AppError> {
    state
        .agent_run_manager
        .compact_thread_context(&thread_id, instructions)
        .await
}

#[tauri::command]
pub async fn thread_cancel_run(
    state: State<'_, AppState>,
    thread_id: String,
) -> Result<(), AppError> {
    state.agent_run_manager.cancel_run(&thread_id).await
}

#[tauri::command]
pub async fn tool_approval_respond(
    state: State<'_, AppState>,
    tool_call_id: String,
    run_id: String,
    approved: bool,
) -> Result<(), AppError> {
    let found = state
        .tool_gateway
        .resolve_approval(&tool_call_id, approved)
        .await?;

    if !found {
        return Err(AppError::recoverable(
            crate::model::errors::ErrorSource::Tool,
            "tool.approval.not_found",
            format!(
                "No pending approval was found for tool call '{tool_call_id}' in run '{run_id}'"
            ),
        ));
    }

    Ok(())
}

#[tauri::command]
pub async fn tool_clarify_respond(
    state: State<'_, AppState>,
    tool_call_id: String,
    response: serde_json::Value,
) -> Result<(), AppError> {
    let found = state
        .tool_gateway
        .resolve_clarification(&tool_call_id, response)
        .await?;

    if !found {
        return Err(AppError::recoverable(
            crate::model::errors::ErrorSource::Tool,
            "tool.clarify.not_found",
            format!("No pending clarification was found for tool call '{tool_call_id}'"),
        ));
    }

    Ok(())
}
