use tauri::State;

use crate::core::app_state::AppState;
use crate::core::index_manager::{FileTreeNode, SearchResponse};
use crate::model::errors::{AppError, ErrorSource};

#[tauri::command]
pub async fn index_get_tree(
    state: State<'_, AppState>,
    workspace_id: String,
) -> Result<FileTreeNode, AppError> {
    let workspace = state
        .workspace_manager
        .list()
        .await?
        .into_iter()
        .find(|w| w.id == workspace_id)
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
    let workspace = state
        .workspace_manager
        .list()
        .await?
        .into_iter()
        .find(|w| w.id == workspace_id)
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
