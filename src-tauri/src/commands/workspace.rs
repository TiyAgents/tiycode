use tauri::State;

use crate::core::app_state::AppState;
use crate::model::errors::AppError;
use crate::model::workspace::{WorkspaceAddInput, WorkspaceDto};

#[tauri::command]
pub async fn workspace_list(state: State<'_, AppState>) -> Result<Vec<WorkspaceDto>, AppError> {
    let records = state.workspace_manager.list().await?;
    Ok(records.into_iter().map(WorkspaceDto::from).collect())
}

#[tauri::command]
pub async fn workspace_add(
    state: State<'_, AppState>,
    path: String,
    name: Option<String>,
) -> Result<WorkspaceDto, AppError> {
    let input = WorkspaceAddInput { path, name };
    let record = state.workspace_manager.add(input).await?;
    Ok(WorkspaceDto::from(record))
}

#[tauri::command]
pub async fn workspace_remove(
    state: State<'_, AppState>,
    id: String,
) -> Result<(), AppError> {
    state.workspace_manager.remove(&id).await
}

#[tauri::command]
pub async fn workspace_set_default(
    state: State<'_, AppState>,
    id: String,
) -> Result<(), AppError> {
    state.workspace_manager.set_default(&id).await
}

#[tauri::command]
pub async fn workspace_validate(
    state: State<'_, AppState>,
    id: String,
) -> Result<WorkspaceDto, AppError> {
    let record = state.workspace_manager.validate(&id).await?;
    Ok(WorkspaceDto::from(record))
}
