use tauri::State;

use crate::core::app_state::AppState;
use crate::core::index_manager::{
    DirectoryChildrenResponse, FileFilterResponse, FileTreeNode, FileTreeResponse,
    RevealPathResponse, SearchResponse,
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

    let (tree_result, overlay_result) = tokio::join!(
        state.index_manager.get_tree(&workspace.canonical_path),
        state.git_manager.get_workspace_overlay(&workspace.canonical_path)
    );

    let mut tree = tree_result?;
    let overlay = overlay_result?;

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

    let (children_result, overlay_result) = tokio::join!(
        state.index_manager.get_children(
            &workspace.canonical_path,
            &directory_path,
            offset,
            max_results,
        ),
        state.git_manager.get_workspace_overlay(&workspace.canonical_path)
    );

    let response = children_result?;
    let overlay = overlay_result?;

    let mut overlay_root = FileTreeNode {
        name: workspace.name.clone(),
        path: String::new(),
        is_dir: true,
        is_expandable: true,
        children_has_more: false,
        children_next_offset: None,
        git_state: None,
        children: Some(response.children),
    };

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
pub async fn index_reveal_path(
    state: State<'_, AppState>,
    workspace_id: String,
    target_path: String,
) -> Result<RevealPathResponse, AppError> {
    let workspace = workspace_repo::find_by_id(&state.pool, &workspace_id)
        .await?
        .ok_or_else(|| AppError::not_found(ErrorSource::Workspace, "workspace"))?;

    let (response_result, overlay_result) = tokio::join!(
        state
            .index_manager
            .reveal_path(&workspace.canonical_path, &target_path),
        state
            .git_manager
            .get_workspace_overlay(&workspace.canonical_path)
    );

    let mut response = response_result?;
    let overlay = overlay_result?;

    for segment in &mut response.segments {
        let mut overlay_root = FileTreeNode {
            name: if segment.directory_path.is_empty() {
                workspace.name.clone()
            } else {
                segment
                    .directory_path
                    .split('/')
                    .next_back()
                    .unwrap_or(&workspace.name)
                    .to_string()
            },
            path: segment.directory_path.clone(),
            is_dir: true,
            is_expandable: true,
            children_has_more: segment.has_more,
            children_next_offset: segment.next_offset,
            git_state: None,
            children: Some(segment.children.clone()),
        };

        overlay_root.apply_git_overlay(&overlay.states);
        segment.children = overlay_root.children.unwrap_or_default();
    }

    Ok(response)
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
