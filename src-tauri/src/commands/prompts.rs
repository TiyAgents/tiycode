use tauri::State;

use crate::core::app_state::AppState;
use crate::model::errors::AppError;
use crate::model::settings::{PromptCommandDto, PromptCommandInput};

#[tauri::command]
pub async fn prompt_command_list(
    state: State<'_, AppState>,
) -> Result<Vec<PromptCommandDto>, AppError> {
    state.prompt_command_manager.list_commands()
}

#[tauri::command]
pub async fn prompt_command_create(
    state: State<'_, AppState>,
    input: PromptCommandInput,
) -> Result<PromptCommandDto, AppError> {
    state.prompt_command_manager.create_command(input)
}

#[tauri::command]
pub async fn prompt_command_update(
    state: State<'_, AppState>,
    id: String,
    input: PromptCommandInput,
) -> Result<PromptCommandDto, AppError> {
    state.prompt_command_manager.update_command(&id, input)
}

#[tauri::command]
pub async fn prompt_command_delete(state: State<'_, AppState>, id: String) -> Result<(), AppError> {
    state.prompt_command_manager.delete_command(&id)
}
