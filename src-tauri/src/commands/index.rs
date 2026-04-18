use tauri::{ipc::Channel, AppHandle, Emitter, State};

use crate::core::app_state::AppState;
use crate::core::index_manager::{
    DirectoryChildrenResponse, FileFilterResponse, FileTreeResponse, RevealPathResponse,
    SearchBatchResponse, SearchOptions, SearchResponse,
};
use crate::core::local_search::{SearchOutputMode, SearchQueryMode};
use crate::ipc::app_events;
use crate::ipc::frontend_channels::SearchStreamEvent;
use crate::model::errors::{AppError, ErrorSource};
use crate::persistence::repo::workspace_repo;
use std::time::Duration;

#[tauri::command]
pub async fn index_get_tree(
    app: AppHandle,
    state: State<'_, AppState>,
    workspace_id: String,
) -> Result<FileTreeResponse, AppError> {
    let workspace = workspace_repo::find_by_id(&state.pool, &workspace_id)
        .await?
        .ok_or_else(|| AppError::not_found(ErrorSource::Workspace, "workspace"))?;

    let tree = state
        .index_manager
        .get_tree(&workspace.canonical_path)
        .await?;

    // Spawn async overlay fetch — pushes the result to the frontend via event
    // so the tree can render immediately without waiting for `git status`.
    let canonical_path = workspace.canonical_path.clone();
    let ws_id = workspace_id.clone();
    let git_manager = state.git_manager.clone();
    tauri::async_runtime::spawn(async move {
        match git_manager.get_workspace_overlay(&canonical_path).await {
            Ok(overlay) => {
                let payload = app_events::IndexGitOverlayReadyPayload {
                    workspace_id: ws_id,
                    repo_available: overlay.repo_available,
                    states: overlay.states.clone(),
                };
                let _ = app.emit(app_events::INDEX_GIT_OVERLAY_READY, payload);
            }
            Err(error) => {
                tracing::warn!(error = %error, "git overlay fetch failed for tree");
            }
        }
    });

    Ok(FileTreeResponse {
        repo_available: false,
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

    state
        .index_manager
        .get_children(
            &workspace.canonical_path,
            &directory_path,
            offset,
            max_results,
        )
        .await
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

    state
        .index_manager
        .reveal_path(&workspace.canonical_path, &target_path)
        .await
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
