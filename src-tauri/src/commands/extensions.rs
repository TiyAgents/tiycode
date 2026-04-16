use tauri::State;

use crate::core::app_state::AppState;
use crate::extensions::ConfigScope;
use crate::model::errors::AppError;
use crate::model::extensions::{
    ConfigDiagnosticDto, ExtensionActivityEventDto, ExtensionCommandDto, ExtensionDetailDto,
    ExtensionSummaryDto, MarketplaceItemDto, MarketplaceRemoveSourcePlanDto, MarketplaceSourceDto,
    MarketplaceSourceInputDto, McpServerConfigInput, McpServerStateDto, PluginDetailDto,
    SkillPreviewDto, SkillRecordDto,
};

#[tauri::command]
pub async fn extensions_list(
    state: State<'_, AppState>,
    workspace_path: Option<String>,
    scope: Option<String>,
) -> Result<Vec<ExtensionSummaryDto>, AppError> {
    state
        .extensions_manager
        .list_extensions(
            workspace_path.as_deref(),
            ConfigScope::from_option(scope.as_deref()),
        )
        .await
}

#[tauri::command]
pub async fn config_list_diagnostics(
    state: State<'_, AppState>,
) -> Result<Vec<ConfigDiagnosticDto>, AppError> {
    Ok(state.extensions_manager.list_config_diagnostics())
}

#[tauri::command]
pub async fn extension_get_detail(
    state: State<'_, AppState>,
    id: String,
    workspace_path: Option<String>,
    scope: Option<String>,
) -> Result<ExtensionDetailDto, AppError> {
    state
        .extensions_manager
        .get_extension_detail(
            &id,
            workspace_path.as_deref(),
            ConfigScope::from_option(scope.as_deref()),
        )
        .await
}

#[tauri::command]
pub async fn extension_enable(
    state: State<'_, AppState>,
    id: String,
    workspace_path: Option<String>,
    scope: Option<String>,
) -> Result<(), AppError> {
    state
        .extensions_manager
        .enable_extension(
            &id,
            workspace_path.as_deref(),
            scope.as_deref().map(ConfigScope::from_str),
        )
        .await
}

#[tauri::command]
pub async fn extension_disable(
    state: State<'_, AppState>,
    id: String,
    workspace_path: Option<String>,
    scope: Option<String>,
) -> Result<(), AppError> {
    state
        .extensions_manager
        .disable_extension(
            &id,
            workspace_path.as_deref(),
            scope.as_deref().map(ConfigScope::from_str),
        )
        .await
}

#[tauri::command]
pub async fn extension_uninstall(
    state: State<'_, AppState>,
    id: String,
    workspace_path: Option<String>,
    scope: Option<String>,
) -> Result<(), AppError> {
    state
        .extensions_manager
        .uninstall_extension(
            &id,
            workspace_path.as_deref(),
            ConfigScope::from_option(scope.as_deref()),
        )
        .await
}

#[tauri::command]
pub async fn extensions_list_commands(
    state: State<'_, AppState>,
) -> Result<Vec<ExtensionCommandDto>, AppError> {
    state.extensions_manager.list_extension_commands().await
}

#[tauri::command]
pub async fn extensions_list_activity(
    state: State<'_, AppState>,
    limit: Option<usize>,
) -> Result<Vec<ExtensionActivityEventDto>, AppError> {
    state
        .extensions_manager
        .list_activity(limit.unwrap_or(50))
        .await
}

#[tauri::command]
pub async fn plugin_validate_dir(
    state: State<'_, AppState>,
    path: String,
) -> Result<PluginDetailDto, AppError> {
    state.extensions_manager.validate_plugin_dir(&path).await
}

#[tauri::command]
pub async fn plugin_install_from_dir(
    state: State<'_, AppState>,
    path: String,
) -> Result<PluginDetailDto, AppError> {
    state
        .extensions_manager
        .install_plugin_from_dir(&path)
        .await
}

#[tauri::command]
pub async fn plugin_update_config(
    state: State<'_, AppState>,
    id: String,
    config: serde_json::Value,
) -> Result<(), AppError> {
    state
        .extensions_manager
        .update_plugin_config(&id, config)
        .await
}

#[tauri::command]
pub async fn mcp_list_servers(
    state: State<'_, AppState>,
    workspace_path: Option<String>,
    scope: Option<String>,
) -> Result<Vec<McpServerStateDto>, AppError> {
    state
        .extensions_manager
        .list_mcp_servers(
            workspace_path.as_deref(),
            ConfigScope::from_option(scope.as_deref()),
        )
        .await
}

#[tauri::command]
pub async fn mcp_add_server(
    state: State<'_, AppState>,
    input: McpServerConfigInput,
    workspace_path: Option<String>,
    scope: Option<String>,
) -> Result<McpServerStateDto, AppError> {
    state
        .extensions_manager
        .add_mcp_server(
            input,
            workspace_path.as_deref(),
            ConfigScope::from_option(scope.as_deref()),
        )
        .await
}

#[tauri::command]
pub async fn mcp_update_server(
    state: State<'_, AppState>,
    id: String,
    input: McpServerConfigInput,
    workspace_path: Option<String>,
    scope: Option<String>,
) -> Result<McpServerStateDto, AppError> {
    state
        .extensions_manager
        .update_mcp_server(
            &id,
            input,
            workspace_path.as_deref(),
            ConfigScope::from_option(scope.as_deref()),
        )
        .await
}

#[tauri::command]
pub async fn mcp_remove_server(
    state: State<'_, AppState>,
    id: String,
    workspace_path: Option<String>,
    scope: Option<String>,
) -> Result<(), AppError> {
    state
        .extensions_manager
        .remove_mcp_server(
            &id,
            workspace_path.as_deref(),
            ConfigScope::from_option(scope.as_deref()),
        )
        .await?;
    Ok(())
}

#[tauri::command]
pub async fn mcp_restart_server(
    state: State<'_, AppState>,
    id: String,
    workspace_path: Option<String>,
    scope: Option<String>,
) -> Result<McpServerStateDto, AppError> {
    state
        .extensions_manager
        .restart_mcp_server(
            &id,
            workspace_path.as_deref(),
            ConfigScope::from_option(scope.as_deref()),
        )
        .await
}

#[tauri::command]
pub async fn mcp_get_server_state(
    state: State<'_, AppState>,
    id: String,
    workspace_path: Option<String>,
    scope: Option<String>,
) -> Result<McpServerStateDto, AppError> {
    state
        .extensions_manager
        .get_mcp_server_state(
            &id,
            workspace_path.as_deref(),
            ConfigScope::from_option(scope.as_deref()),
        )
        .await
}

#[tauri::command]
pub async fn skill_list(
    state: State<'_, AppState>,
    workspace_path: Option<String>,
    scope: Option<String>,
) -> Result<Vec<SkillRecordDto>, AppError> {
    state
        .extensions_manager
        .list_skills(
            workspace_path.as_deref(),
            ConfigScope::from_option(scope.as_deref()),
        )
        .await
}

#[tauri::command]
pub async fn skill_rescan(
    state: State<'_, AppState>,
    workspace_path: Option<String>,
    scope: Option<String>,
) -> Result<Vec<SkillRecordDto>, AppError> {
    state
        .extensions_manager
        .rescan_skills(
            workspace_path.as_deref(),
            ConfigScope::from_option(scope.as_deref()),
        )
        .await
}

#[tauri::command]
pub async fn skill_enable(
    state: State<'_, AppState>,
    id: String,
    workspace_path: Option<String>,
    scope: Option<String>,
) -> Result<(), AppError> {
    state
        .extensions_manager
        .set_skill_enabled(
            &id,
            true,
            workspace_path.as_deref(),
            ConfigScope::from_option(scope.as_deref()),
        )
        .await
}

#[tauri::command]
pub async fn skill_disable(
    state: State<'_, AppState>,
    id: String,
    workspace_path: Option<String>,
    scope: Option<String>,
) -> Result<(), AppError> {
    state
        .extensions_manager
        .set_skill_enabled(
            &id,
            false,
            workspace_path.as_deref(),
            ConfigScope::from_option(scope.as_deref()),
        )
        .await
}

#[tauri::command]
pub async fn skill_preview(
    state: State<'_, AppState>,
    id: String,
    workspace_path: Option<String>,
    scope: Option<String>,
) -> Result<SkillPreviewDto, AppError> {
    state
        .extensions_manager
        .preview_skill(
            &id,
            workspace_path.as_deref(),
            ConfigScope::from_option(scope.as_deref()),
        )
        .await
}

#[tauri::command]
pub async fn marketplace_list_sources(
    state: State<'_, AppState>,
) -> Result<Vec<MarketplaceSourceDto>, AppError> {
    state.extensions_manager.marketplace_list_sources().await
}

#[tauri::command]
pub async fn marketplace_add_source(
    state: State<'_, AppState>,
    input: MarketplaceSourceInputDto,
) -> Result<MarketplaceSourceDto, AppError> {
    state.extensions_manager.marketplace_add_source(input).await
}

#[tauri::command]
pub async fn marketplace_remove_source(
    state: State<'_, AppState>,
    id: String,
) -> Result<(), AppError> {
    state
        .extensions_manager
        .marketplace_remove_source(&id)
        .await
}

#[tauri::command]
pub async fn marketplace_get_remove_source_plan(
    state: State<'_, AppState>,
    id: String,
) -> Result<MarketplaceRemoveSourcePlanDto, AppError> {
    state
        .extensions_manager
        .marketplace_get_remove_source_plan(&id)
        .await
}

#[tauri::command]
pub async fn marketplace_refresh_source(
    state: State<'_, AppState>,
    id: String,
) -> Result<MarketplaceSourceDto, AppError> {
    state
        .extensions_manager
        .marketplace_refresh_source(&id)
        .await
}

#[tauri::command]
pub async fn marketplace_list_items(
    state: State<'_, AppState>,
) -> Result<Vec<MarketplaceItemDto>, AppError> {
    state.extensions_manager.marketplace_list_items().await
}

#[tauri::command]
pub async fn marketplace_install_item(
    state: State<'_, AppState>,
    id: String,
) -> Result<PluginDetailDto, AppError> {
    state.extensions_manager.marketplace_install_item(&id).await
}
