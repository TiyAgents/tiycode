use tauri::{ipc::Channel, State};

use crate::core::app_state::AppState;
use crate::ipc::frontend_channels::GitStreamEvent;
use crate::model::errors::{AppError, ErrorSource};
use crate::model::git::{GitCommitSummaryDto, GitDiffDto, GitFileStatusDto, GitSnapshotDto};
use crate::persistence::repo::workspace_repo;

#[tauri::command]
pub async fn git_get_snapshot(
    state: State<'_, AppState>,
    workspace_id: String,
) -> Result<GitSnapshotDto, AppError> {
    let workspace = workspace_repo::find_by_id(&state.pool, &workspace_id)
        .await?
        .ok_or_else(|| AppError::not_found(ErrorSource::Workspace, "workspace"))?;

    state
        .git_manager
        .get_snapshot(&workspace_id, &workspace.canonical_path)
        .await
}

#[tauri::command]
pub async fn git_get_history(
    state: State<'_, AppState>,
    workspace_id: String,
    limit: Option<usize>,
) -> Result<Vec<GitCommitSummaryDto>, AppError> {
    let workspace = workspace_repo::find_by_id(&state.pool, &workspace_id)
        .await?
        .ok_or_else(|| AppError::not_found(ErrorSource::Workspace, "workspace"))?;

    state
        .git_manager
        .get_history(&workspace.canonical_path, limit)
        .await
}

#[tauri::command]
pub async fn git_get_diff(
    state: State<'_, AppState>,
    workspace_id: String,
    path: String,
    staged: Option<bool>,
) -> Result<GitDiffDto, AppError> {
    let workspace = workspace_repo::find_by_id(&state.pool, &workspace_id)
        .await?
        .ok_or_else(|| AppError::not_found(ErrorSource::Workspace, "workspace"))?;

    state
        .git_manager
        .get_diff(&workspace.canonical_path, &path, staged.unwrap_or(false))
        .await
}

#[tauri::command]
pub async fn git_get_file_status(
    state: State<'_, AppState>,
    workspace_id: String,
    path: String,
) -> Result<GitFileStatusDto, AppError> {
    let workspace = workspace_repo::find_by_id(&state.pool, &workspace_id)
        .await?
        .ok_or_else(|| AppError::not_found(ErrorSource::Workspace, "workspace"))?;

    state
        .git_manager
        .get_file_status(&workspace.canonical_path, &path)
        .await
}

#[tauri::command]
pub async fn git_subscribe(
    state: State<'_, AppState>,
    workspace_id: String,
    on_event: Channel<GitStreamEvent>,
) -> Result<(), AppError> {
    let mut receiver = state.git_manager.subscribe(&workspace_id).await;

    tokio::spawn(async move {
        while let Ok(event) = receiver.recv().await {
            if on_event.send(event).is_err() {
                break;
            }
        }
    });

    Ok(())
}

#[tauri::command]
pub async fn git_refresh(
    state: State<'_, AppState>,
    workspace_id: String,
) -> Result<GitSnapshotDto, AppError> {
    let workspace = workspace_repo::find_by_id(&state.pool, &workspace_id)
        .await?
        .ok_or_else(|| AppError::not_found(ErrorSource::Workspace, "workspace"))?;

    state
        .git_manager
        .refresh(&workspace_id, &workspace.canonical_path)
        .await
}

#[tauri::command]
pub async fn git_stage(
    state: State<'_, AppState>,
    workspace_id: String,
    paths: Vec<String>,
) -> Result<GitSnapshotDto, AppError> {
    let workspace = workspace_repo::find_by_id(&state.pool, &workspace_id)
        .await?
        .ok_or_else(|| AppError::not_found(ErrorSource::Workspace, "workspace"))?;

    state
        .git_manager
        .stage(&workspace_id, &workspace.canonical_path, &paths)
        .await
}

#[tauri::command]
pub async fn git_unstage(
    state: State<'_, AppState>,
    workspace_id: String,
    paths: Vec<String>,
) -> Result<GitSnapshotDto, AppError> {
    let workspace = workspace_repo::find_by_id(&state.pool, &workspace_id)
        .await?
        .ok_or_else(|| AppError::not_found(ErrorSource::Workspace, "workspace"))?;

    state
        .git_manager
        .unstage(&workspace_id, &workspace.canonical_path, &paths)
        .await
}
