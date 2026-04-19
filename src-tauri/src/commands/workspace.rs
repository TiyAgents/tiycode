use tauri::State;

use crate::core::app_state::AppState;
use crate::core::worktree_manager::WorktreeInfoDto;
use crate::model::errors::{AppError, ErrorSource};
use crate::model::workspace::{WorkspaceAddInput, WorkspaceDto, WorktreeCreateInput};
use crate::persistence::repo::workspace_repo;

#[tauri::command]
pub async fn workspace_list(state: State<'_, AppState>) -> Result<Vec<WorkspaceDto>, AppError> {
    tracing::debug!("⏱ [ipc] workspace_list command entered");
    let t0 = std::time::Instant::now();
    let records = state.workspace_manager.list().await?;
    let result: Vec<WorkspaceDto> = records.into_iter().map(WorkspaceDto::from).collect();
    tracing::debug!(
        elapsed_ms = t0.elapsed().as_millis(),
        count = result.len(),
        "⏱ [ipc] workspace_list command done"
    );
    Ok(result)
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
pub async fn workspace_ensure_default(
    state: State<'_, AppState>,
) -> Result<WorkspaceDto, AppError> {
    let record = state
        .workspace_manager
        .ensure_default_thread_workspace()
        .await?;
    Ok(WorkspaceDto::from(record))
}

#[tauri::command]
pub async fn workspace_remove(
    state: State<'_, AppState>,
    id: String,
    force: Option<bool>,
) -> Result<(), AppError> {
    state
        .workspace_manager
        .remove(&id, force.unwrap_or(false))
        .await
}

#[tauri::command]
pub async fn workspace_set_default(state: State<'_, AppState>, id: String) -> Result<(), AppError> {
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

// ---------------------------------------------------------------------------
// Worktree commands
// ---------------------------------------------------------------------------

async fn load_workspace(
    state: &State<'_, AppState>,
    id: &str,
) -> Result<crate::model::workspace::WorkspaceRecord, AppError> {
    workspace_repo::find_by_id(&state.pool, id)
        .await?
        .ok_or_else(|| AppError::not_found(ErrorSource::Workspace, "workspace"))
}

#[tauri::command]
pub async fn workspace_list_worktrees(
    state: State<'_, AppState>,
    workspace_id: String,
) -> Result<Vec<WorktreeInfoDto>, AppError> {
    let parent = load_workspace(&state, &workspace_id).await?;
    state.worktree_manager.list(&parent).await
}

#[tauri::command]
pub async fn workspace_create_worktree(
    state: State<'_, AppState>,
    workspace_id: String,
    input: WorktreeCreateInput,
) -> Result<WorkspaceDto, AppError> {
    let parent = load_workspace(&state, &workspace_id).await?;
    let record = state.worktree_manager.create(&parent, input).await?;
    Ok(WorkspaceDto::from(record))
}

#[tauri::command]
pub async fn workspace_remove_worktree(
    state: State<'_, AppState>,
    id: String,
    force: Option<bool>,
) -> Result<(), AppError> {
    // Physical removal + DB cascade are both handled inside
    // `workspace_manager.remove`: it detects the `kind=worktree` row, calls
    // the injected `WorktreeManager::remove_physical`, and then deletes the
    // DB rows (threads / terminals / runs cascade included).
    state
        .workspace_manager
        .remove(&id, force.unwrap_or(false))
        .await
}

#[tauri::command]
pub async fn workspace_prune_worktrees(
    state: State<'_, AppState>,
    workspace_id: String,
) -> Result<(), AppError> {
    let parent = load_workspace(&state, &workspace_id).await?;
    state.worktree_manager.prune(&parent).await
}
