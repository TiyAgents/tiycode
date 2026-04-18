use tauri::AppHandle;
use tauri::State;
#[cfg(not(target_os = "macos"))]
use tauri_plugin_autostart::ManagerExt as AutoStartManagerExt;

use crate::core::app_state::AppState;
use crate::core::desktop_runtime::{
    DesktopRuntimeState, LAUNCH_AT_LOGIN_SETTING_KEY, MINIMIZE_TO_TRAY_SETTING_KEY,
};
use crate::core::sleep_manager::PREVENT_SLEEP_WHILE_RUNNING_SETTING_KEY;
#[cfg(target_os = "macos")]
use crate::core::startup_manager;
use crate::model::errors::AppError;
use crate::model::provider::{
    AgentProfileDto, AgentProfileInput, CustomProviderCreateInput, ProviderCatalogEntryDto,
    ProviderModelConnectionTestResultDto, ProviderSettingsDto, ProviderSettingsUpdateInput,
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
    tracing::info!(key = %key, "⏱ [ipc] settings_get command entered");
    let t0 = std::time::Instant::now();
    let result = state
        .settings_manager
        .get_setting(&key)
        .await?
        .map(|r| r.into_dto());
    tracing::info!(elapsed_ms = t0.elapsed().as_millis(), key = %key, "⏱ [ipc] settings_get command done");
    Ok(result)
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
    app: AppHandle,
    state: State<'_, AppState>,
    desktop_runtime: State<'_, DesktopRuntimeState>,
    key: String,
    value: String,
) -> Result<(), AppError> {
    state.settings_manager.set_setting(&key, &value).await?;

    match key.as_str() {
        PREVENT_SLEEP_WHILE_RUNNING_SETTING_KEY => match serde_json::from_str::<bool>(&value) {
            Ok(enabled) => state.sleep_manager.set_user_preference(enabled).await,
            Err(error) => tracing::warn!(error = %error, "invalid prevent sleep setting payload"),
        },
        LAUNCH_AT_LOGIN_SETTING_KEY => match serde_json::from_str::<bool>(&value) {
            Ok(enabled) => {
                #[cfg(target_os = "macos")]
                {
                    startup_manager::set_launch_at_login(enabled)?;
                }

                #[cfg(not(target_os = "macos"))]
                {
                    let autolaunch = app.autolaunch();
                    let result = if enabled {
                        autolaunch.enable()
                    } else {
                        autolaunch.disable()
                    };

                    if let Err(error) = result {
                        return Err(AppError::internal(
                            crate::model::errors::ErrorSource::Settings,
                            error.to_string(),
                        ));
                    }
                }
            }
            Err(error) => tracing::warn!(error = %error, "invalid launch at login payload"),
        },
        MINIMIZE_TO_TRAY_SETTING_KEY => match serde_json::from_str::<bool>(&value) {
            Ok(enabled) => {
                desktop_runtime.set_minimize_to_tray(enabled);

                if let Some(tray) = app.tray_by_id("main-tray") {
                    if let Err(error) = tray.set_visible(enabled) {
                        tracing::warn!(error = %error, "failed to update tray visibility");
                    }
                }
            }
            Err(error) => tracing::warn!(error = %error, "invalid minimize to tray payload"),
        },
        _ => {}
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
    tracing::info!("⏱ [ipc] provider_settings_get_all command entered");
    let t0 = std::time::Instant::now();
    let result = state.settings_manager.get_all_provider_settings().await;
    tracing::info!(
        elapsed_ms = t0.elapsed().as_millis(),
        "⏱ [ipc] provider_settings_get_all command done"
    );
    result
}

#[tauri::command]
pub async fn provider_settings_fetch_models(
    state: State<'_, AppState>,
    id: String,
) -> Result<ProviderSettingsDto, AppError> {
    state.settings_manager.fetch_provider_models(&id).await
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

#[tauri::command]
pub async fn provider_model_test_connection(
    state: State<'_, AppState>,
    provider_id: String,
    model_id: String,
) -> Result<ProviderModelConnectionTestResultDto, AppError> {
    state
        .settings_manager
        .test_provider_model_connection(&provider_id, &model_id)
        .await
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
