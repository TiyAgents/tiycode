use tauri::{ipc::Channel, State};

use crate::core::app_state::AppState;
use crate::core::terminal_manager::ShellOption;
use crate::ipc::frontend_channels::TerminalStreamEvent;
use crate::model::errors::AppError;
use crate::model::terminal::{TerminalAttachDto, TerminalSessionDto};

#[tauri::command]
pub async fn terminal_create_or_attach(
    state: State<'_, AppState>,
    thread_id: String,
    cols: Option<u16>,
    rows: Option<u16>,
    shell_path: Option<String>,
    shell_args: Option<String>,
    term_env: Option<String>,
    on_event: Channel<TerminalStreamEvent>,
) -> Result<TerminalAttachDto, AppError> {
    let attachment = state
        .terminal_manager
        .create_or_attach(&thread_id, cols, rows, shell_path.as_deref(), shell_args.as_deref(), term_env.as_deref())
        .await?;

    let mut event_rx = attachment.receiver;
    tokio::spawn(async move {
        while let Ok(event) = event_rx.recv().await {
            if on_event.send(event).is_err() {
                break;
            }
        }
    });

    Ok(attachment.attach)
}

#[tauri::command]
pub async fn terminal_write_input(
    state: State<'_, AppState>,
    thread_id: String,
    data: String,
) -> Result<(), AppError> {
    state.terminal_manager.write_input(&thread_id, &data).await
}

#[tauri::command]
pub async fn terminal_resize(
    state: State<'_, AppState>,
    thread_id: String,
    cols: u16,
    rows: u16,
) -> Result<(), AppError> {
    state.terminal_manager.resize(&thread_id, cols, rows).await
}

#[tauri::command]
pub async fn terminal_restart(
    state: State<'_, AppState>,
    thread_id: String,
    cols: Option<u16>,
    rows: Option<u16>,
    shell_path: Option<String>,
    shell_args: Option<String>,
    term_env: Option<String>,
    on_event: Channel<TerminalStreamEvent>,
) -> Result<TerminalAttachDto, AppError> {
    let attachment = state
        .terminal_manager
        .restart(&thread_id, cols, rows, shell_path.as_deref(), shell_args.as_deref(), term_env.as_deref())
        .await?;

    let mut event_rx = attachment.receiver;
    tokio::spawn(async move {
        while let Ok(event) = event_rx.recv().await {
            if on_event.send(event).is_err() {
                break;
            }
        }
    });

    Ok(attachment.attach)
}

#[tauri::command]
pub async fn terminal_close(state: State<'_, AppState>, thread_id: String) -> Result<(), AppError> {
    state.terminal_manager.close(&thread_id).await
}

#[tauri::command]
pub async fn terminal_list(
    state: State<'_, AppState>,
) -> Result<Vec<TerminalSessionDto>, AppError> {
    Ok(state.terminal_manager.list().await)
}

#[tauri::command]
pub async fn terminal_list_available_shells() -> Result<Vec<ShellOption>, AppError> {
    Ok(crate::core::terminal_manager::list_available_shells())
}
