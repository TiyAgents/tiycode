use tauri::State;

use crate::core::app_state::AppState;
use crate::core::index_manager::{FileTreeNode, SearchResponse};
use crate::model::errors::{AppError, ErrorSource};
use crate::persistence::repo::workspace_repo;

#[tauri::command]
pub async fn index_get_tree(
    state: State<'_, AppState>,
    workspace_id: String,
) -> Result<FileTreeNode, AppError> {
    let workspace = workspace_repo::find_by_id(&state.pool, &workspace_id)
        .await?
        .ok_or_else(|| AppError::not_found(ErrorSource::Workspace, "workspace"))?;

    state
        .index_manager
        .get_tree(&workspace.canonical_path)
        .await
}

#[tauri::command]
pub async fn index_search(
    state: State<'_, AppState>,
    workspace_id: String,
    query: String,
    file_pattern: Option<String>,
    max_results: Option<usize>,
) -> Result<SearchResponse, AppError> {
    let workspace = workspace_repo::find_by_id(&state.pool, &workspace_id)
        .await?
        .ok_or_else(|| AppError::not_found(ErrorSource::Workspace, "workspace"))?;

    state
        .index_manager
        .search(
            &workspace.canonical_path,
            &query,
            file_pattern.as_deref(),
            max_results,
        )
        .await
}
