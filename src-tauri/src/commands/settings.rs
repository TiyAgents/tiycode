use tauri::State;

use crate::core::app_state::AppState;
use crate::core::sleep_manager::PREVENT_SLEEP_WHILE_RUNNING_SETTING_KEY;
use crate::model::errors::AppError;
use crate::model::provider::{
    AgentProfileDto, AgentProfileInput, ProviderDto, ProviderInput, ProviderModelDto,
    ProviderModelInput,
};
use crate::model::settings::SettingDto;

// ---------------------------------------------------------------------------
// Settings KV
// ---------------------------------------------------------------------------

#[tauri::command]
pub async fn settings_get(
    state: State<'_, AppState>,
    key: String,
) -> Result<Option<SettingDto>, AppError> {
    Ok(state
        .settings_manager
        .get_setting(&key)
        .await?
        .map(|r| r.into_dto()))
}

#[tauri::command]
pub async fn settings_get_all(state: State<'_, AppState>) -> Result<Vec<SettingDto>, AppError> {
    Ok(state
        .settings_manager
        .get_all_settings()
        .await?
        .into_iter()
        .map(|r| r.into_dto())
        .collect())
}

#[tauri::command]
pub async fn settings_set(
    state: State<'_, AppState>,
    key: String,
    value: String,
) -> Result<(), AppError> {
    state.settings_manager.set_setting(&key, &value).await?;

    if key == PREVENT_SLEEP_WHILE_RUNNING_SETTING_KEY {
        match serde_json::from_str::<bool>(&value) {
            Ok(enabled) => state.sleep_manager.set_user_preference(enabled).await,
            Err(error) => tracing::warn!(error = %error, "invalid prevent sleep setting payload"),
        }
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Policies
// ---------------------------------------------------------------------------

#[tauri::command]
pub async fn policy_get(
    state: State<'_, AppState>,
    key: String,
) -> Result<Option<SettingDto>, AppError> {
    Ok(state
        .settings_manager
        .get_policy(&key)
        .await?
        .map(|r| r.into_dto()))
}

#[tauri::command]
pub async fn policy_get_all(state: State<'_, AppState>) -> Result<Vec<SettingDto>, AppError> {
    Ok(state
        .settings_manager
        .get_all_policies()
        .await?
        .into_iter()
        .map(|r| r.into_dto())
        .collect())
}

#[tauri::command]
pub async fn policy_set(
    state: State<'_, AppState>,
    key: String,
    value: String,
) -> Result<(), AppError> {
    state.settings_manager.set_policy(&key, &value).await
}

// ---------------------------------------------------------------------------
// Providers
// ---------------------------------------------------------------------------

#[tauri::command]
pub async fn provider_list(state: State<'_, AppState>) -> Result<Vec<ProviderDto>, AppError> {
    Ok(state
        .settings_manager
        .list_providers()
        .await?
        .into_iter()
        .map(ProviderDto::from)
        .collect())
}

#[tauri::command]
pub async fn provider_create(
    state: State<'_, AppState>,
    input: ProviderInput,
) -> Result<ProviderDto, AppError> {
    let record = state.settings_manager.create_provider(input).await?;
    Ok(ProviderDto::from(record))
}

#[tauri::command]
pub async fn provider_update(
    state: State<'_, AppState>,
    id: String,
    input: ProviderInput,
) -> Result<ProviderDto, AppError> {
    let record = state.settings_manager.update_provider(&id, input).await?;
    Ok(ProviderDto::from(record))
}

#[tauri::command]
pub async fn provider_delete(state: State<'_, AppState>, id: String) -> Result<(), AppError> {
    state.settings_manager.delete_provider(&id).await
}

// ---------------------------------------------------------------------------
// Provider Models
// ---------------------------------------------------------------------------

#[tauri::command]
pub async fn provider_model_list(
    state: State<'_, AppState>,
    provider_id: String,
) -> Result<Vec<ProviderModelDto>, AppError> {
    Ok(state
        .settings_manager
        .list_models(&provider_id)
        .await?
        .into_iter()
        .map(ProviderModelDto::from)
        .collect())
}

#[tauri::command]
pub async fn provider_model_add(
    state: State<'_, AppState>,
    provider_id: String,
    input: ProviderModelInput,
) -> Result<ProviderModelDto, AppError> {
    let record = state
        .settings_manager
        .add_model(&provider_id, input)
        .await?;
    Ok(ProviderModelDto::from(record))
}

#[tauri::command]
pub async fn provider_model_remove(state: State<'_, AppState>, id: String) -> Result<(), AppError> {
    state.settings_manager.remove_model(&id).await
}

// ---------------------------------------------------------------------------
// Agent Profiles
// ---------------------------------------------------------------------------

#[tauri::command]
pub async fn profile_list(state: State<'_, AppState>) -> Result<Vec<AgentProfileDto>, AppError> {
    Ok(state
        .settings_manager
        .list_profiles()
        .await?
        .into_iter()
        .map(AgentProfileDto::from)
        .collect())
}

#[tauri::command]
pub async fn profile_create(
    state: State<'_, AppState>,
    input: AgentProfileInput,
) -> Result<AgentProfileDto, AppError> {
    let record = state.settings_manager.create_profile(input).await?;
    Ok(AgentProfileDto::from(record))
}

#[tauri::command]
pub async fn profile_update(
    state: State<'_, AppState>,
    id: String,
    input: AgentProfileInput,
) -> Result<AgentProfileDto, AppError> {
    let record = state.settings_manager.update_profile(&id, input).await?;
    Ok(AgentProfileDto::from(record))
}

#[tauri::command]
pub async fn profile_delete(state: State<'_, AppState>, id: String) -> Result<(), AppError> {
    state.settings_manager.delete_profile(&id).await
}
