use tauri::{ipc::Channel, State};

use crate::core::app_state::AppState;
use crate::core::tool_gateway::ToolGatewayResult;
use crate::ipc::frontend_channels::ThreadStreamEvent;
use crate::model::errors::AppError;

#[tauri::command]
pub async fn thread_start_run(
    state: State<'_, AppState>,
    thread_id: String,
    prompt: String,
    run_mode: Option<String>,
    on_event: Channel<ThreadStreamEvent>,
) -> Result<String, AppError> {
    let run_mode = run_mode.unwrap_or_else(|| "default".to_string());

    // TODO: resolve effective model plan from active profile
    let model_plan = serde_json::json!({});

    let (run_id, mut event_rx) = state
        .agent_run_manager
        .start_run(&thread_id, &prompt, &run_mode, model_plan)
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
    let result = state
        .tool_gateway
        .resolve_approval(&tool_call_id, approved)
        .await?;

    match result {
        Some(ToolGatewayResult::Executed { tool_call_id, output }) => {
            // Tool executed successfully — send result back to sidecar
            state
                .agent_run_manager
                .send_tool_result(&tool_call_id, &run_id, output.result, output.success)
                .await?;
        }
        Some(ToolGatewayResult::Denied { tool_call_id, reason }) => {
            // Denied — send denial back to sidecar
            state
                .agent_run_manager
                .send_tool_result(
                    &tool_call_id,
                    &run_id,
                    serde_json::json!({"denied": true, "reason": reason}),
                    false,
                )
                .await?;
        }
        Some(ToolGatewayResult::ApprovalRequired { .. }) => {
            // Should not happen after resolve_approval
        }
        None => {
            // Approval not found — send error back
            state
                .agent_run_manager
                .send_tool_result(
                    &tool_call_id,
                    &run_id,
                    serde_json::json!({"error": "approval not found"}),
                    false,
                )
                .await?;
        }
    }

    Ok(())
}

#[tauri::command]
pub async fn sidecar_status(
    state: State<'_, AppState>,
) -> Result<serde_json::Value, AppError> {
    let running = state.sidecar_manager.is_running().await;
    Ok(serde_json::json!({
        "running": running,
    }))
}
