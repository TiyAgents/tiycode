use tauri::State;

use crate::core::app_state::AppState;
use crate::core::sleep_manager::PREVENT_SLEEP_WHILE_RUNNING_SETTING_KEY;
use crate::model::errors::AppError;
use crate::model::provider::{
    AgentProfileDto, AgentProfileInput, CustomProviderCreateInput, ProviderCatalogEntryDto,
    ProviderSettingsDto, ProviderSettingsUpdateInput,
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
pub async fn provider_catalog_list(
    state: State<'_, AppState>,
) -> Result<Vec<ProviderCatalogEntryDto>, AppError> {
    state.settings_manager.list_provider_catalog().await
}

#[tauri::command]
pub async fn provider_settings_get_all(
    state: State<'_, AppState>,
) -> Result<Vec<ProviderSettingsDto>, AppError> {
    state.settings_manager.get_all_provider_settings().await
}

#[tauri::command]
pub async fn provider_settings_upsert_builtin(
    state: State<'_, AppState>,
    provider_key: String,
    input: ProviderSettingsUpdateInput,
) -> Result<ProviderSettingsDto, AppError> {
    state
        .settings_manager
        .upsert_builtin_provider_settings(&provider_key, input)
        .await
}

#[tauri::command]
pub async fn provider_settings_create_custom(
    state: State<'_, AppState>,
    input: CustomProviderCreateInput,
) -> Result<ProviderSettingsDto, AppError> {
    state.settings_manager.create_custom_provider(input).await
}

#[tauri::command]
pub async fn provider_settings_update_custom(
    state: State<'_, AppState>,
    id: String,
    input: ProviderSettingsUpdateInput,
) -> Result<ProviderSettingsDto, AppError> {
    state
        .settings_manager
        .update_custom_provider(&id, input)
        .await
}

#[tauri::command]
pub async fn provider_settings_delete_custom(
    state: State<'_, AppState>,
    id: String,
) -> Result<(), AppError> {
    state.settings_manager.delete_custom_provider(&id).await
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
