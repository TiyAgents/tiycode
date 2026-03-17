use tauri::State;

use crate::core::app_state::AppState;
use crate::core::index_manager::{
    DirectoryChildrenResponse, FileFilterResponse, FileTreeNode, FileTreeResponse, SearchResponse,
};
use crate::model::errors::{AppError, ErrorSource};
use crate::persistence::repo::workspace_repo;

#[tauri::command]
pub async fn index_get_tree(
    state: State<'_, AppState>,
    workspace_id: String,
) -> Result<FileTreeResponse, AppError> {
    let workspace = workspace_repo::find_by_id(&state.pool, &workspace_id)
        .await?
        .ok_or_else(|| AppError::not_found(ErrorSource::Workspace, "workspace"))?;

    let mut tree = state
        .index_manager
        .get_tree(&workspace.canonical_path)
        .await?;

    let overlay = state
        .git_manager
        .get_workspace_overlay(&workspace.canonical_path)
        .await?;

    tree.apply_git_overlay(&overlay.states);

    Ok(FileTreeResponse {
        repo_available: overlay.repo_available,
        tree,
    })
}

#[tauri::command]
pub async fn index_get_children(
    state: State<'_, AppState>,
    workspace_id: String,
    directory_path: String,
    offset: Option<usize>,
    max_results: Option<usize>,
) -> Result<DirectoryChildrenResponse, AppError> {
    let workspace = workspace_repo::find_by_id(&state.pool, &workspace_id)
        .await?
        .ok_or_else(|| AppError::not_found(ErrorSource::Workspace, "workspace"))?;

    let mut overlay_root = FileTreeNode {
        name: workspace.name.clone(),
        path: String::new(),
        is_dir: true,
        is_expandable: true,
        children_has_more: false,
        children_next_offset: None,
        git_state: None,
        children: None,
    };
    let response = state
        .index_manager
        .get_children(
            &workspace.canonical_path,
            &directory_path,
            offset,
            max_results,
        )
        .await?;
    overlay_root.children = Some(response.children);

    let overlay = state
        .git_manager
        .get_workspace_overlay(&workspace.canonical_path)
        .await?;

    overlay_root.apply_git_overlay(&overlay.states);

    Ok(DirectoryChildrenResponse {
        children: overlay_root.children.unwrap_or_default(),
        has_more: response.has_more,
        next_offset: response.next_offset,
    })
}

#[tauri::command]
pub async fn index_filter_files(
    state: State<'_, AppState>,
    workspace_id: String,
    query: String,
    max_results: Option<usize>,
) -> Result<FileFilterResponse, AppError> {
    let workspace = workspace_repo::find_by_id(&state.pool, &workspace_id)
        .await?
        .ok_or_else(|| AppError::not_found(ErrorSource::Workspace, "workspace"))?;

    state
        .index_manager
        .filter_files(&workspace.canonical_path, &query, max_results)
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
