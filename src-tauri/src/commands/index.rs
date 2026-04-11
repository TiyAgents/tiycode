use tauri::{ipc::Channel, State};

use crate::core::app_state::AppState;
use crate::core::index_manager::{
    DirectoryChildrenResponse, FileFilterResponse, FileTreeNode, FileTreeResponse,
    RevealPathResponse, SearchBatchResponse, SearchOptions, SearchResponse,
};
use crate::core::local_search::{SearchOutputMode, SearchQueryMode};
use crate::ipc::frontend_channels::SearchStreamEvent;
use crate::model::errors::{AppError, ErrorSource};
use crate::persistence::repo::workspace_repo;
use std::time::Duration;

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
        state
            .git_manager
            .get_workspace_overlay(&workspace.canonical_path)
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
        state
            .git_manager
            .get_workspace_overlay(&workspace.canonical_path)
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
    file_type: Option<String>,
    max_results: Option<usize>,
    query_mode: Option<String>,
    output_mode: Option<String>,
    case_insensitive: Option<bool>,
    multiline: Option<bool>,
    timeout_ms: Option<u64>,
) -> Result<SearchResponse, AppError> {
    let workspace = workspace_repo::find_by_id(&state.pool, &workspace_id)
        .await?
        .ok_or_else(|| AppError::not_found(ErrorSource::Workspace, "workspace"))?;

    state
        .index_manager
        .search(
            &workspace.canonical_path,
            &query,
            SearchOptions {
                file_pattern,
                file_type,
                max_results,
                query_mode: SearchQueryMode::from_str(query_mode.as_deref()),
                output_mode: SearchOutputMode::from_str(output_mode.as_deref()),
                case_insensitive: case_insensitive.unwrap_or(false),
                multiline: multiline.unwrap_or(false),
                timeout: timeout_ms.map(Duration::from_millis),
                cancellation: None,
            },
        )
        .await
}

#[tauri::command]
pub async fn index_search_stream(
    state: State<'_, AppState>,
    workspace_id: String,
    query: String,
    file_pattern: Option<String>,
    file_type: Option<String>,
    max_results: Option<usize>,
    query_mode: Option<String>,
    output_mode: Option<String>,
    case_insensitive: Option<bool>,
    multiline: Option<bool>,
    timeout_ms: Option<u64>,
    on_event: Channel<SearchStreamEvent>,
) -> Result<(), AppError> {
    let workspace = workspace_repo::find_by_id(&state.pool, &workspace_id)
        .await?
        .ok_or_else(|| AppError::not_found(ErrorSource::Workspace, "workspace"))?;

    let search_id = on_event.id();
    let cancellation = state.index_manager.register_stream_search(search_id).await;
    let workspace_id_for_events = workspace_id.clone();
    let query_for_events = query.clone();
    let index_manager = state.index_manager.clone();
    let workspace_path = workspace.canonical_path.clone();

    tokio::spawn(async move {
        if on_event
            .send(SearchStreamEvent::Started {
                workspace_id: workspace_id_for_events.clone(),
                query: query_for_events.clone(),
            })
            .is_err()
        {
            index_manager.finish_stream_search(search_id).await;
            return;
        }

        let result = index_manager
            .search_stream(
                &workspace_path,
                &query_for_events,
                SearchOptions {
                    file_pattern,
                    file_type,
                    max_results,
                    query_mode: SearchQueryMode::from_str(query_mode.as_deref()),
                    output_mode: SearchOutputMode::from_str(output_mode.as_deref()),
                    case_insensitive: case_insensitive.unwrap_or(false),
                    multiline: multiline.unwrap_or(false),
                    timeout: timeout_ms.map(Duration::from_millis),
                    cancellation: Some(cancellation.clone()),
                },
                |batch: SearchBatchResponse| {
                    on_event
                        .send(SearchStreamEvent::Batch {
                            workspace_id: workspace_id_for_events.clone(),
                            batch,
                        })
                        .map_err(|error| {
                            AppError::internal(
                                ErrorSource::Index,
                                format!("search stream channel closed: {error}"),
                            )
                        })
                },
            )
            .await;

        index_manager.finish_stream_search(search_id).await;

        match result {
            Ok(response) => {
                if response.cancelled {
                    return;
                }
                let _ = on_event.send(SearchStreamEvent::Completed {
                    workspace_id: workspace_id_for_events,
                    response,
                });
            }
            Err(error) => {
                let _ = on_event.send(SearchStreamEvent::Failed {
                    workspace_id: workspace_id_for_events,
                    query: query_for_events,
                    error: error.to_string(),
                });
            }
        }
    });

    Ok(())
}

#[tauri::command]
pub async fn index_cancel_search_stream(
    state: State<'_, AppState>,
    search_id: u32,
) -> Result<(), AppError> {
    state.index_manager.cancel_stream_search(search_id).await;
    Ok(())
}
