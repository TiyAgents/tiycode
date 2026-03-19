use tauri::{ipc::Channel, State};

use crate::core::app_state::AppState;
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

#[tauri::command]
pub async fn thread_start_run(
    state: State<'_, AppState>,
    thread_id: String,
    prompt: String,
    run_mode: Option<String>,
    model_plan: Option<serde_json::Value>,
    on_event: Channel<ThreadStreamEvent>,
) -> Result<String, AppError> {
    let run_mode = run_mode.unwrap_or_else(|| "default".to_string());
    let model_plan = model_plan.unwrap_or_default();
    let (profile_id, provider_id, model_id) = extract_run_model_refs(&model_plan);

    let (run_id, mut event_rx) = state
        .agent_run_manager
        .clone()
        .start_run(
            &thread_id,
            &prompt,
            &run_mode,
            profile_id,
            provider_id,
            model_id,
            model_plan,
        )
        .await?;

    // Forward events from the internal channel to the Tauri Channel
    tokio::spawn(async move {
        while let Some(event) = event_rx.recv().await {
            if on_event.send(event).is_err() {
                break;
            }
        }
    });

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
