use std::time::Duration;

use tauri::State;

use crate::core::app_state::AppState;
use crate::model::errors::AppError;
use crate::model::thread::{AddMessageInput, MessageDto, ThreadSnapshotDto, ThreadSummaryDto};

#[tauri::command]
pub async fn thread_list(
    state: State<'_, AppState>,
    workspace_id: String,
    limit: Option<i64>,
    offset: Option<i64>,
) -> Result<Vec<ThreadSummaryDto>, AppError> {
    tracing::info!(workspace_id = %workspace_id, "⏱ [ipc] thread_list command entered");
    let t0 = std::time::Instant::now();
    let result = state
        .thread_manager
        .list(&workspace_id, limit, offset)
        .await?;
    tracing::info!(
        elapsed_ms = t0.elapsed().as_millis(),
        count = result.len(),
        "⏱ [ipc] thread_list command done"
    );
    Ok(result)
}

#[tauri::command]
pub async fn thread_create(
    state: State<'_, AppState>,
    workspace_id: String,
    title: Option<String>,
) -> Result<ThreadSummaryDto, AppError> {
    state.thread_manager.create(&workspace_id, title).await
}

#[tauri::command]
pub async fn thread_load(
    state: State<'_, AppState>,
    id: String,
    message_cursor: Option<String>,
    message_limit: Option<i64>,
) -> Result<ThreadSnapshotDto, AppError> {
    state
        .thread_manager
        .load(&id, message_cursor, message_limit)
        .await
}

#[tauri::command]
pub async fn thread_update_title(
    state: State<'_, AppState>,
    id: String,
    title: String,
) -> Result<(), AppError> {
    state.thread_manager.update_title(&id, &title).await
}

#[tauri::command]
pub async fn thread_delete(state: State<'_, AppState>, id: String) -> Result<(), AppError> {
    state.agent_run_manager.cancel_run_if_active(&id).await?;
    state
        .agent_run_manager
        .wait_until_thread_inactive(&id, Duration::from_secs(5))
        .await?;
    state.terminal_manager.close_for_thread(&id).await?;
    state.thread_manager.delete(&id).await
}

#[tauri::command]
pub async fn thread_add_message(
    state: State<'_, AppState>,
    thread_id: String,
    input: AddMessageInput,
) -> Result<MessageDto, AppError> {
    state.thread_manager.add_message(&thread_id, input).await
}
