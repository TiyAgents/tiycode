use std::cmp::Ordering;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::ffi::OsStr;
use std::fs;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use chrono::Utc;
use reqwest::header::{HeaderMap, HeaderName, HeaderValue, ACCEPT, CONTENT_TYPE};
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;
use tiycore::agent::AgentTool;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::process::Command;

use crate::core::executors::ToolOutput;
use crate::core::shell_runtime::resolve_command_path;
use crate::core::windows_process::configure_background_tokio_command;
use crate::model::errors::{AppError, ErrorSource};
use crate::model::extensions::{
    ConfigDiagnosticDto, ConfigDiagnosticKind, ConfigDiagnosticSeverity, ExtensionActivityEventDto,
    ExtensionCommandDto, ExtensionDetailDto, ExtensionHealth, ExtensionInstallState, ExtensionKind,
    ExtensionSourceDto, ExtensionSummaryDto, MarketplaceItemDto, MarketplaceRemoveSourcePlanDto,
    MarketplaceSourceDto, MarketplaceSourceInputDto, MarketplaceSourcePluginRefDto,
    McpResourceSummaryDto, McpServerConfigDto, McpServerConfigInput, McpServerStateDto,
    McpToolSummaryDto, PluginCommandDto, PluginDetailDto, PluginHookGroupDto, PluginToolDto,
    SkillPreviewDto, SkillRecordDto,
};
use crate::persistence::repo::{audit_repo, settings_repo};

const EXTENSIONS_PLUGINS_FILE_NAME: &str = "plugins.json";
const LEGACY_EXTENSIONS_INSTALLED_PLUGINS_KEY: &str = "extensions.plugins.installed";
const EXTENSIONS_PLUGIN_CONFIG_KEY: &str = "extensions.plugins.config";
const EXTENSIONS_MCP_RUNTIME_KEY: &str = "extensions.mcp.runtime";
const EXTENSIONS_SKILLS_MAX_PROMPT_CHARS_KEY: &str = "extensions.skills.max_prompt_chars";
const EXTENSIONS_SKILLS_MAX_SELECTED_COUNT_KEY: &str = "extensions.skills.max_selected_count";
const DEFAULT_PLUGIN_TIMEOUT_MS: u64 = 30_000;
const DEFAULT_HOOK_TIMEOUT_MS: u64 = 5_000;
const DEFAULT_MCP_TIMEOUT_MS: u64 = 15_000;
const MAX_MCP_TIMEOUT_MS: u64 = 120_000;
const MCP_PROTOCOL_VERSION: &str = "2025-06-18";
const MCP_HEADER_SESSION_ID: &str = "Mcp-Session-Id";
const DEFAULT_MARKETPLACE_SOURCE_KIND: &str = "git";
const BUILTIN_MARKETPLACE_ANTHROPIC_NAME: &str = "Anthropic Official Plugins";
const BUILTIN_MARKETPLACE_ANTHROPIC_URL: &str =
    "https://github.com/anthropics/claude-plugins-official.git";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfigScope {
    Global,
    Workspace,
}

impl ConfigScope {
    pub fn from_option(scope: Option<&str>) -> Self {
        match scope.unwrap_or("global") {
            "workspace" => Self::Workspace,
            _ => Self::Global,
        }
    }

    pub fn from_str(scope: &str) -> Self {
        match scope {
            "workspace" => Self::Workspace,
            _ => Self::Global,
        }
    }

    fn as_str(&self) -> &'static str {
        match self {
            Self::Global => "global",
            Self::Workspace => "workspace",
        }
    }
}

#[derive(Debug, Clone)]
pub struct ExtensionsManager {
    pool: SqlitePool,
    diagnostics: Arc<Mutex<Vec<ConfigDiagnosticDto>>>,
}

#[derive(Debug, Clone)]
struct ConfigLoadOutcome<T> {
    value: T,
}

#[derive(Debug, Clone)]
pub struct ResolvedTool {
    pub tool_name: String,
    pub provider_type: String,
    pub provider_id: String,
    pub required_permission: String,
    route: ToolRoute,
}

#[derive(Debug, Clone)]
enum ToolRoute {
    Plugin {
        plugin: InstalledPluginRuntime,
        tool: PluginManifestTool,
    },
    Mcp {
        server_id: String,
        tool: McpToolSummaryDto,
    },
}

#[derive(Debug, Clone)]
pub struct ToolProviderContext {
    pub provider_type: String,
    pub provider_id: String,
    pub required_permission: String,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
struct InstalledPluginRecord {
    id: String,
    path: String,
    enabled: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
struct PluginConfigStore {
    #[serde(default)]
    items: HashMap<String, serde_json::Value>,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
struct SkillStateStore {
    #[serde(default)]
    enabled: Vec<String>,
    #[serde(default)]
    disabled: Vec<String>,
    #[serde(default, alias = "pinned", skip_serializing)]
    #[allow(dead_code)]
    legacy_pinned: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
struct McpConfigFile {
    #[serde(default)]
    servers: Vec<McpServerConfigInput>,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
struct MarketplaceSourceStore {
    #[serde(default)]
    sources: Vec<MarketplaceSourceRecord>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
struct MarketplaceSourceRecord {
    id: String,
    name: String,
    url: String,
    kind: String,
    last_synced_at: Option<String>,
    last_error: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
struct MarketplaceSourceManifest {
    name: Option<String>,
    description: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
struct McpRuntimeStore {
    #[serde(default)]
    items: HashMap<String, McpRuntimeRecord>,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
struct McpRuntimeRecord {
    #[serde(default)]
    tools: Vec<McpToolSummaryDto>,
    #[serde(default)]
    resources: Vec<McpResourceSummaryDto>,
    stale_snapshot: bool,
    last_error: Option<String>,
    status: Option<String>,
    phase: Option<String>,
    updated_at: Option<String>,
}

#[derive(Debug, Clone)]
struct StreamableHttpSession {
    protocol_version: String,
    session_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PluginManifest {
    id: String,
    name: String,
    version: String,
    description: Option<String>,
    author: Option<String>,
    homepage: Option<String>,
    default_enabled: Option<bool>,
    #[serde(default)]
    capabilities: Vec<String>,
    #[serde(default)]
    permissions: Vec<String>,
    hooks: Option<PluginManifestHooks>,
    #[serde(default)]
    tools: Vec<PluginManifestTool>,
    #[serde(default)]
    commands: Vec<PluginManifestCommand>,
    timeout_ms: Option<u64>,
    skills_dir: Option<String>,
    config_schema: Option<PluginManifestSchema>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PluginManifestHooks {
    pre_tool_use: Option<Vec<String>>,
    post_tool_use: Option<Vec<String>>,
    on_run_start: Option<Vec<String>>,
    on_run_complete: Option<Vec<String>>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PluginManifestTool {
    name: String,
    description: String,
    command: String,
    #[serde(default)]
    args: Vec<String>,
    env: Option<HashMap<String, String>>,
    cwd: Option<String>,
    timeout_ms: Option<u64>,
    required_permission: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PluginManifestCommand {
    name: String,
    description: String,
    prompt_template: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PluginManifestSchema {
    #[allow(dead_code)]
    r#type: String,
    path: String,
}

#[derive(Debug, Clone)]
struct InstalledPluginRuntime {
    manifest: PluginManifest,
    path: PathBuf,
    enabled: bool,
}

#[derive(Debug, Clone)]
struct PluginCommandRegistration {
    plugin_id: String,
    command: PluginManifestCommand,
}

#[derive(Debug, Clone)]
struct PluginHookRegistration {
    plugin: InstalledPluginRuntime,
    handler: String,
}

#[derive(Debug, Clone)]
struct SkillRuntime {
    record: SkillRecordDto,
    content: String,
}

#[derive(Debug, Clone, Serialize)]
struct PluginToolInput<'a> {
    args: &'a serde_json::Value,
    workspace: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    thread_id: Option<&'a str>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PluginToolOutput {
    success: bool,
    result: Option<serde_json::Value>,
    error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
struct HookInput<'a> {
    event: &'a str,
    payload: serde_json::Value,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
struct HookOutput {
    action: Option<String>,
    message: Option<String>,
    metadata: Option<serde_json::Value>,
}

impl ExtensionsManager {
    pub fn new(pool: SqlitePool) -> Self {
        Self {
            pool,
            diagnostics: Arc::new(Mutex::new(Vec::new())),
        }
    }

    pub fn list_config_diagnostics(&self) -> Vec<ConfigDiagnosticDto> {
        self.diagnostics
            .lock()
            .map(|items| items.clone())
            .unwrap_or_default()
    }

    pub async fn list_extensions(
        &self,
        workspace_path: Option<&str>,
        scope: ConfigScope,
    ) -> Result<Vec<ExtensionSummaryDto>, AppError> {
        let plugins = self.collect_plugin_summaries().await?;
        let mcps = self.collect_mcp_summaries(workspace_path, scope).await?;
        let skills = self.collect_skill_summaries(workspace_path, scope).await?;

        let mut items = Vec::with_capacity(plugins.len() + mcps.len() + skills.len());
        items.extend(plugins);
        items.extend(mcps);
        items.extend(skills);
        items.sort_by(compare_extension_summaries);
        Ok(items)
    }

    pub async fn get_extension_detail(
        &self,
        id: &str,
        workspace_path: Option<&str>,
        scope: ConfigScope,
    ) -> Result<ExtensionDetailDto, AppError> {
        let plugin_runtimes = self.load_plugin_runtimes().await?;
        if let Some(plugin) = plugin_runtimes
            .iter()
            .find(|plugin| plugin.manifest.id == id)
        {
            let summary = self.build_plugin_summary(
                plugin,
                ExtensionInstallState::from(plugin.enabled),
                None,
            );
            return Ok(ExtensionDetailDto {
                summary,
                plugin: Some(self.build_plugin_detail(plugin, None)),
                mcp: None,
                skill: None,
            });
        }

        let mcp_states = self.list_mcp_servers(workspace_path, scope).await?;
        if let Some(server) = mcp_states.into_iter().find(|server| server.id == id) {
            let summary = self.build_mcp_summary(&server);
            return Ok(ExtensionDetailDto {
                summary,
                plugin: None,
                mcp: Some(server),
                skill: None,
            });
        }

        let skills = self.load_skills(workspace_path, scope).await?;
        if let Some(skill) = skills.into_iter().find(|skill| skill.record.id == id) {
            let summary = self.build_skill_summary(&skill.record);
            return Ok(ExtensionDetailDto {
                summary,
                plugin: None,
                mcp: None,
                skill: Some(skill.record),
            });
        }

        Err(AppError::not_found(
            ErrorSource::Settings,
            format!("extension '{id}'"),
        ))
    }

    /// Resolve the effective scope for an MCP server: global-first, then workspace.
    async fn resolve_mcp_scope(&self, id: &str, workspace_path: Option<&str>) -> ConfigScope {
        if let Ok(global) = self
            .load_mcp_configs_for_scope(None, ConfigScope::Global)
            .await
        {
            if global.iter().any(|c| c.id == id) {
                return ConfigScope::Global;
            }
        }
        if workspace_path.is_some() {
            if let Ok(ws) = self
                .load_mcp_configs_for_scope(workspace_path, ConfigScope::Workspace)
                .await
            {
                if ws.iter().any(|c| c.id == id) {
                    return ConfigScope::Workspace;
                }
            }
        }
        ConfigScope::Global
    }

    /// Resolve the effective scope for a skill: global-first, then workspace.
    async fn resolve_skill_scope(&self, id: &str, workspace_path: Option<&str>) -> ConfigScope {
        if self
            .skill_exists(id, None, ConfigScope::Global)
            .await
            .unwrap_or(false)
        {
            return ConfigScope::Global;
        }
        if workspace_path.is_some()
            && self
                .skill_exists(id, workspace_path, ConfigScope::Workspace)
                .await
                .unwrap_or(false)
        {
            return ConfigScope::Workspace;
        }
        ConfigScope::Global
    }

    pub async fn enable_extension(
        &self,
        id: &str,
        workspace_path: Option<&str>,
        scope: Option<ConfigScope>,
    ) -> Result<(), AppError> {
        if self.update_plugin_enabled(id, true).await? {
            self.write_extension_audit(
                "extension_enabled",
                "plugin",
                id,
                serde_json::json!({ "enabled": true }),
            )
            .await?;
            return Ok(());
        }

        let mcp_scope = match scope {
            Some(s) => s,
            None => self.resolve_mcp_scope(id, workspace_path).await,
        };
        if self
            .update_mcp_enabled(id, true, workspace_path, mcp_scope)
            .await?
        {
            self.write_extension_audit(
                "extension_enabled",
                "mcp",
                id,
                serde_json::json!({ "enabled": true }),
            )
            .await?;
            return Ok(());
        }

        let skill_scope = match scope {
            Some(s) => s,
            None => self.resolve_skill_scope(id, workspace_path).await,
        };
        if self
            .update_skill_enabled(id, true, workspace_path, skill_scope)
            .await?
        {
            self.write_extension_audit(
                "extension_enabled",
                "skill",
                id,
                serde_json::json!({ "enabled": true }),
            )
            .await?;
            return Ok(());
        }

        Err(AppError::not_found(
            ErrorSource::Settings,
            format!("extension '{id}'"),
        ))
    }

    pub async fn disable_extension(
        &self,
        id: &str,
        workspace_path: Option<&str>,
        scope: Option<ConfigScope>,
    ) -> Result<(), AppError> {
        if self.update_plugin_enabled(id, false).await? {
            self.write_extension_audit(
                "extension_disabled",
                "plugin",
                id,
                serde_json::json!({ "enabled": false }),
            )
            .await?;
            return Ok(());
        }

        let mcp_scope = match scope {
            Some(s) => s,
            None => self.resolve_mcp_scope(id, workspace_path).await,
        };
        if self
            .update_mcp_enabled(id, false, workspace_path, mcp_scope)
            .await?
        {
            self.write_extension_audit(
                "extension_disabled",
                "mcp",
                id,
                serde_json::json!({ "enabled": false }),
            )
            .await?;
            return Ok(());
        }

        let skill_scope = match scope {
            Some(s) => s,
            None => self.resolve_skill_scope(id, workspace_path).await,
        };
        if self
            .update_skill_enabled(id, false, workspace_path, skill_scope)
            .await?
        {
            self.write_extension_audit(
                "extension_disabled",
                "skill",
                id,
                serde_json::json!({ "enabled": false }),
            )
            .await?;
            return Ok(());
        }

        Err(AppError::not_found(
            ErrorSource::Settings,
            format!("extension '{id}'"),
        ))
    }

    pub async fn uninstall_extension(
        &self,
        id: &str,
        workspace_path: Option<&str>,
        scope: ConfigScope,
    ) -> Result<(), AppError> {
        if self.uninstall_plugin(id).await? {
            self.write_extension_audit(
                "extension_uninstalled",
                "plugin",
                id,
                serde_json::json!({}),
            )
            .await?;
            return Ok(());
        }

        if self.remove_mcp_server(id, workspace_path, scope).await? {
            self.write_extension_audit("extension_uninstalled", "mcp", id, serde_json::json!({}))
                .await?;
            return Ok(());
        }

        Err(AppError::not_found(
            ErrorSource::Settings,
            format!("extension '{id}'"),
        ))
    }

    pub async fn validate_plugin_dir(&self, path: &str) -> Result<PluginDetailDto, AppError> {
        let runtime = self.load_plugin_from_dir(Path::new(path), false)?;
        Ok(self.build_plugin_detail(&runtime, None))
    }

    pub async fn install_plugin_from_dir(&self, path: &str) -> Result<PluginDetailDto, AppError> {
        let runtime = self.load_plugin_from_dir(Path::new(path), false)?;
        let enabled = runtime.manifest.default_enabled.unwrap_or(true);
        let mut installed = self.load_installed_plugin_records().await?;
        installed.retain(|record| record.id != runtime.manifest.id);
        installed.push(InstalledPluginRecord {
            id: runtime.manifest.id.clone(),
            path: runtime.path.to_string_lossy().to_string(),
            enabled,
        });
        self.save_installed_plugin_records(&installed).await?;
        let installed_runtime = InstalledPluginRuntime { enabled, ..runtime };
        self.sync_plugin_managed_mcp_configs(&installed_runtime)
            .await?;
        self.write_extension_audit(
            "plugin_installed",
            "plugin",
            &installed_runtime.manifest.id,
            serde_json::json!({ "path": installed_runtime.path.to_string_lossy() }),
        )
        .await?;
        Ok(self.build_plugin_detail(&installed_runtime, None))
    }

    pub async fn update_plugin_config(
        &self,
        id: &str,
        config: serde_json::Value,
    ) -> Result<(), AppError> {
        let mut store = self.load_plugin_config_store().await?;
        store.items.insert(id.to_string(), config.clone());
        self.save_plugin_config_store(&store).await?;
        self.write_extension_audit("plugin_config_updated", "plugin", id, config)
            .await
    }

    pub async fn list_mcp_servers(
        &self,
        workspace_path: Option<&str>,
        scope: ConfigScope,
    ) -> Result<Vec<McpServerStateDto>, AppError> {
        let configs = self
            .load_mcp_configs_with_scope(workspace_path, scope)
            .await?;
        let runtime = self.load_mcp_runtime_store().await?;
        let mut results = Vec::with_capacity(configs.len());

        for (config, config_scope) in configs {
            let runtime_record = runtime.items.get(&config.id);
            let state = if config.enabled
                && (runtime_record.is_none()
                    || runtime_record
                        .map(|record| mcp_runtime_record_needs_refresh(&config.id, record))
                        .unwrap_or(false)
                    || runtime_record
                        .map(mcp_runtime_record_is_disabled)
                        .unwrap_or(false))
            {
                self.refresh_mcp_runtime(&config, None, config_scope.as_str())
                    .await?
            } else {
                self.build_mcp_state(&config, runtime_record, config_scope.as_str())
            };
            results.push(state);
        }

        results.sort_by(compare_mcp_server_states);
        Ok(results)
    }

    pub async fn add_mcp_server(
        &self,
        input: McpServerConfigInput,
        workspace_path: Option<&str>,
        scope: ConfigScope,
    ) -> Result<McpServerStateDto, AppError> {
        let input = canonicalize_mcp_config(input);
        self.validate_mcp_input(&input)?;
        let mut configs = self
            .load_mcp_configs_for_scope(workspace_path, scope)
            .await?;
        configs.retain(|server| server.id != input.id);
        configs.push(input.clone());
        self.save_mcp_configs_for_scope(&configs, workspace_path, scope)
            .await?;
        let state = self
            .refresh_mcp_runtime(&input, None, scope.as_str())
            .await?;
        self.write_extension_audit(
            "mcp_added",
            "mcp",
            &input.id,
            serde_json::to_value(self.mask_mcp_config(&input)).unwrap_or_default(),
        )
        .await?;
        Ok(state)
    }

    pub async fn update_mcp_server(
        &self,
        id: &str,
        input: McpServerConfigInput,
        workspace_path: Option<&str>,
        scope: ConfigScope,
    ) -> Result<McpServerStateDto, AppError> {
        let input = canonicalize_mcp_config(input);
        if id != input.id {
            return Err(AppError::validation(
                ErrorSource::Settings,
                "MCP server id cannot be changed",
            ));
        }

        self.validate_mcp_input(&input)?;
        let mut configs = self
            .load_mcp_configs_for_scope(workspace_path, scope)
            .await?;
        let mut found = false;
        for server in &mut configs {
            if server.id == id {
                *server = merge_mcp_sensitive_fields(server, input.clone());
                found = true;
                break;
            }
        }
        if !found && scope == ConfigScope::Workspace && workspace_path.is_some() {
            let exists_globally = self
                .load_mcp_configs_for_scope(None, ConfigScope::Global)
                .await?
                .into_iter()
                .any(|server| server.id == id);
            if exists_globally {
                configs.push(input.clone());
                found = true;
            }
        }
        if !found {
            return Err(AppError::not_found(
                ErrorSource::Settings,
                format!("MCP server '{id}'"),
            ));
        }
        self.save_mcp_configs_for_scope(&configs, workspace_path, scope)
            .await?;
        let saved = configs
            .iter()
            .find(|server| server.id == id)
            .cloned()
            .ok_or_else(|| {
                AppError::not_found(ErrorSource::Settings, format!("MCP server '{id}'"))
            })?;
        let state = self
            .refresh_mcp_runtime(&saved, None, scope.as_str())
            .await?;
        self.write_extension_audit(
            "mcp_updated",
            "mcp",
            id,
            serde_json::to_value(self.mask_mcp_config(&saved)).unwrap_or_default(),
        )
        .await?;
        Ok(state)
    }

    pub async fn remove_mcp_server(
        &self,
        id: &str,
        workspace_path: Option<&str>,
        scope: ConfigScope,
    ) -> Result<bool, AppError> {
        let mut configs = self
            .load_mcp_configs_for_scope(workspace_path, scope)
            .await?;
        let before = configs.len();
        configs.retain(|server| server.id != id);
        if before == configs.len() {
            return Ok(false);
        }
        self.save_mcp_configs_for_scope(&configs, workspace_path, scope)
            .await?;
        let mut runtime = self.load_mcp_runtime_store().await?;
        runtime.items.remove(id);
        self.save_mcp_runtime_store(&runtime).await?;
        Ok(true)
    }

    pub async fn restart_mcp_server(
        &self,
        id: &str,
        workspace_path: Option<&str>,
        scope: ConfigScope,
    ) -> Result<McpServerStateDto, AppError> {
        let configs = self
            .load_mcp_configs_with_scope(workspace_path, scope)
            .await?;
        let (config, config_scope) = configs
            .into_iter()
            .find(|(server, _)| server.id == id)
            .ok_or_else(|| {
                AppError::not_found(ErrorSource::Settings, format!("MCP server '{id}'"))
            })?;
        let state = self
            .refresh_mcp_runtime(&config, Some("manual_restart"), config_scope.as_str())
            .await?;
        self.write_extension_audit(
            "mcp_restarted",
            "mcp",
            id,
            serde_json::json!({ "status": state.status }),
        )
        .await?;
        Ok(state)
    }

    pub async fn get_mcp_server_state(
        &self,
        id: &str,
        workspace_path: Option<&str>,
        scope: ConfigScope,
    ) -> Result<McpServerStateDto, AppError> {
        let states = self.list_mcp_servers(workspace_path, scope).await?;
        states
            .into_iter()
            .find(|server| server.id == id)
            .ok_or_else(|| AppError::not_found(ErrorSource::Settings, format!("MCP server '{id}'")))
    }

    pub async fn list_skills(
        &self,
        workspace_path: Option<&str>,
        scope: ConfigScope,
    ) -> Result<Vec<SkillRecordDto>, AppError> {
        Ok(self
            .load_skills(workspace_path, scope)
            .await?
            .into_iter()
            .map(|skill| skill.record)
            .collect())
    }

    pub async fn rescan_skills(
        &self,
        workspace_path: Option<&str>,
        scope: ConfigScope,
    ) -> Result<Vec<SkillRecordDto>, AppError> {
        self.list_skills(workspace_path, scope).await
    }

    pub async fn set_skill_enabled(
        &self,
        id: &str,
        enabled: bool,
        workspace_path: Option<&str>,
        scope: Option<ConfigScope>,
    ) -> Result<(), AppError> {
        let resolved_scope = match scope {
            Some(s) => s,
            None => self.resolve_skill_scope(id, workspace_path).await,
        };
        if !self
            .skill_exists(id, workspace_path, resolved_scope)
            .await?
        {
            return Err(AppError::not_found(
                ErrorSource::Settings,
                format!("skill '{id}'"),
            ));
        }
        let mut store = self
            .load_skill_state_store(workspace_path, resolved_scope)
            .await?;
        update_named_membership(&mut store.enabled, id, enabled);
        update_named_membership(&mut store.disabled, id, !enabled);
        self.save_skill_state_store(&store, workspace_path, resolved_scope)
            .await?;
        self.write_extension_audit(
            if enabled {
                "skill_enabled"
            } else {
                "skill_disabled"
            },
            "skill",
            id,
            serde_json::json!({ "enabled": enabled }),
        )
        .await
    }

    pub async fn preview_skill(
        &self,
        id: &str,
        workspace_path: Option<&str>,
        scope: ConfigScope,
    ) -> Result<SkillPreviewDto, AppError> {
        let skill = self
            .load_skills(workspace_path, scope)
            .await?
            .into_iter()
            .find(|skill| skill.record.id == id)
            .ok_or_else(|| AppError::not_found(ErrorSource::Settings, format!("skill '{id}'")))?;
        Ok(SkillPreviewDto {
            record: skill.record,
            content: skill.content,
        })
    }

    pub async fn list_extension_commands(&self) -> Result<Vec<ExtensionCommandDto>, AppError> {
        let mut commands = self.load_registered_plugin_commands().await?;
        commands.sort_by(|left, right| left.name.cmp(&right.name));
        Ok(commands)
    }

    pub async fn marketplace_list_sources(&self) -> Result<Vec<MarketplaceSourceDto>, AppError> {
        let store = self.load_marketplace_sources()?;
        Ok(store
            .sources
            .into_iter()
            .map(|source| self.build_marketplace_source_dto(&source, None))
            .collect())
    }

    pub async fn marketplace_add_source(
        &self,
        input: MarketplaceSourceInputDto,
    ) -> Result<MarketplaceSourceDto, AppError> {
        let mut store = self.load_marketplace_sources()?;
        let id = marketplace_source_id(&input.url);
        store.sources.retain(|source| source.id != id);
        store.sources.push(MarketplaceSourceRecord {
            id: id.clone(),
            name: input.name.clone(),
            url: input.url.clone(),
            kind: DEFAULT_MARKETPLACE_SOURCE_KIND.to_string(),
            last_synced_at: None,
            last_error: None,
        });
        self.save_marketplace_sources(&store)?;
        self.marketplace_refresh_source(&id).await
    }

    pub async fn marketplace_get_remove_source_plan(
        &self,
        id: &str,
    ) -> Result<MarketplaceRemoveSourcePlanDto, AppError> {
        self.build_marketplace_remove_source_plan(id).await
    }

    pub async fn marketplace_remove_source(&self, id: &str) -> Result<(), AppError> {
        let plan = self.build_marketplace_remove_source_plan(id).await?;
        if !plan.can_remove {
            return Err(AppError::validation(
                ErrorSource::Settings,
                plan.summary.clone(),
            ));
        }

        for plugin in &plan.removable_installed_plugins {
            self.uninstall_plugin(&plugin.id).await?;
        }

        let mut store = self.load_marketplace_sources()?;
        let before = store.sources.len();
        store.sources.retain(|source| source.id != id);
        if before == store.sources.len() {
            return Err(AppError::not_found(
                ErrorSource::Settings,
                format!("marketplace source '{id}'"),
            ));
        }
        self.save_marketplace_sources(&store)?;
        let cache_dir = marketplace_cache_root().join(id);
        if cache_dir.exists() {
            if let Err(error) = fs::remove_dir_all(&cache_dir) {
                tracing::warn!(
                    source_id = %id,
                    path = %cache_dir.display(),
                    error = %error,
                    "failed to remove marketplace source cache"
                );
            }
        }
        self.write_extension_audit(
            "marketplace_source_removed",
            "marketplace_source",
            id,
            serde_json::json!({
                "removedPluginIds": plan
                    .removable_installed_plugins
                    .iter()
                    .map(|plugin| plugin.id.clone())
                    .collect::<Vec<_>>(),
            }),
        )
        .await?;
        Ok(())
    }

    pub async fn marketplace_refresh_source(
        &self,
        id: &str,
    ) -> Result<MarketplaceSourceDto, AppError> {
        let mut store = self.load_marketplace_sources()?;
        let source_index = store
            .sources
            .iter()
            .position(|source| source.id == id)
            .ok_or_else(|| {
                AppError::not_found(ErrorSource::Settings, format!("marketplace source '{id}'"))
            })?;
        let source_snapshot = store.sources[source_index].clone();

        match self.sync_marketplace_source_repo(&source_snapshot).await {
            Ok(_) => {
                store.sources[source_index].last_synced_at = Some(Utc::now().to_rfc3339());
                store.sources[source_index].last_error = None;
            }
            Err(error) => {
                store.sources[source_index].last_error = Some(error.user_message.clone());
            }
        }

        self.save_marketplace_sources(&store)?;
        let source = store.sources[source_index].clone();
        let items = self.marketplace_items_for_source(&source).await?;
        Ok(self.build_marketplace_source_dto(&source, Some(items.len())))
    }

    pub async fn marketplace_list_items(&self) -> Result<Vec<MarketplaceItemDto>, AppError> {
        let mut store = self.load_marketplace_sources()?;
        let mut did_update_sources = false;
        for index in 0..store.sources.len() {
            if !is_builtin_marketplace_source_id(&store.sources[index].id) {
                continue;
            }
            let cache_dir = marketplace_cache_root().join(&store.sources[index].id);
            if cache_dir.exists() {
                continue;
            }
            let source_snapshot = store.sources[index].clone();
            match self.sync_marketplace_source_repo(&source_snapshot).await {
                Ok(_) => {
                    store.sources[index].last_synced_at = Some(Utc::now().to_rfc3339());
                    store.sources[index].last_error = None;
                }
                Err(error) => {
                    store.sources[index].last_error = Some(error.user_message.clone());
                }
            }
            did_update_sources = true;
        }
        if did_update_sources {
            self.save_marketplace_sources(&store)?;
        }
        let installed = self.load_installed_plugin_records().await?;
        let installed_by_path = installed
            .iter()
            .map(|record| (record.path.clone(), record.enabled))
            .collect::<HashMap<_, _>>();
        let mut items = Vec::new();

        for source in &store.sources {
            match self.marketplace_items_for_source(source).await {
                Ok(source_items) => {
                    items.extend(source_items.into_iter().map(|mut item| {
                        if let Some(enabled) = installed_by_path.get(&item.path) {
                            item.installed = true;
                            item.enabled = *enabled;
                        }
                        item
                    }));
                }
                Err(error) => {
                    tracing::warn!(
                        source_id = %source.id,
                        source_name = %source.name,
                        error = %error.user_message,
                        "failed to load marketplace source items"
                    );
                }
            }
        }

        items.sort_by(compare_marketplace_items);
        Ok(items)
    }

    pub async fn marketplace_install_item(&self, id: &str) -> Result<PluginDetailDto, AppError> {
        let item = self
            .marketplace_list_items()
            .await?
            .into_iter()
            .find(|item| item.id == id)
            .ok_or_else(|| {
                AppError::not_found(ErrorSource::Settings, format!("marketplace item '{id}'"))
            })?;
        self.install_plugin_from_dir(&item.path).await
    }

    pub async fn list_activity(
        &self,
        limit: usize,
    ) -> Result<Vec<ExtensionActivityEventDto>, AppError> {
        audit_repo::list_extension_activity(&self.pool, limit).await
    }

    pub async fn list_runtime_agent_tools(
        &self,
        workspace_path: Option<&str>,
    ) -> Result<Vec<AgentTool>, AppError> {
        let scope = if workspace_path
            .map(|path| !path.trim().is_empty())
            .unwrap_or(false)
        {
            ConfigScope::Workspace
        } else {
            ConfigScope::Global
        };
        let mut tools = Vec::new();

        for server in self.list_mcp_servers(workspace_path, scope).await? {
            if server.status != "connected" && server.status != "degraded" {
                continue;
            }
            tools.extend(server.tools.iter().map(|tool| {
                AgentTool::new(
                    tool.qualified_name.clone(),
                    tool.name.clone(),
                    tool.description.clone().unwrap_or_else(|| {
                        format!("MCP tool '{}' from {}", tool.name, server.label)
                    }),
                    tool.input_schema.clone().unwrap_or_else(|| {
                        serde_json::json!({
                            "type": "object",
                            "additionalProperties": true
                        })
                    }),
                )
            }));
        }

        Ok(tools)
    }

    pub async fn resolve_tool(&self, tool_name: &str) -> Result<Option<ResolvedTool>, AppError> {
        for plugin in self.load_enabled_plugin_runtimes().await? {
            if let Some(tool) = plugin
                .manifest
                .tools
                .iter()
                .find(|tool| tool.name == tool_name)
                .cloned()
            {
                return Ok(Some(ResolvedTool {
                    tool_name: tool_name.to_string(),
                    provider_type: "plugin".to_string(),
                    provider_id: plugin.manifest.id.clone(),
                    required_permission: tool.required_permission.clone(),
                    route: ToolRoute::Plugin { plugin, tool },
                }));
            }
        }

        for server in self.list_mcp_servers(None, ConfigScope::Global).await? {
            if server.status == "connected" || server.status == "degraded" {
                if let Some(tool) = server
                    .tools
                    .iter()
                    .find(|tool| tool.qualified_name == tool_name)
                {
                    return Ok(Some(ResolvedTool {
                        tool_name: tool_name.to_string(),
                        provider_type: "mcp".to_string(),
                        provider_id: server.id.clone(),
                        required_permission: "read".to_string(),
                        route: ToolRoute::Mcp {
                            server_id: server.id.clone(),
                            tool: tool.clone(),
                        },
                    }));
                }
            }
        }

        Ok(None)
    }

    pub async fn execute_resolved_tool(
        &self,
        resolved: &ResolvedTool,
        tool_input: &serde_json::Value,
        workspace_path: &str,
        thread_id: &str,
    ) -> Result<ToolOutput, AppError> {
        match &resolved.route {
            ToolRoute::Plugin { plugin, tool } => {
                self.execute_plugin_tool(plugin, tool, tool_input, workspace_path, Some(thread_id))
                    .await
            }
            ToolRoute::Mcp { server_id, tool } => {
                self.execute_mcp_tool(server_id, tool, tool_input, workspace_path)
                    .await
            }
        }
    }

    pub async fn run_pre_tool_hooks(
        &self,
        tool_name: &str,
        tool_input: &serde_json::Value,
        workspace_path: &str,
        thread_id: &str,
        run_id: &str,
        tool_call_id: &str,
    ) -> Result<Option<String>, AppError> {
        let payload = serde_json::json!({
            "toolName": tool_name,
            "toolArgs": tool_input,
            "workspace": workspace_path,
            "threadId": thread_id,
            "runId": run_id,
        });

        for registration in self.load_plugin_hook_registrations("pre_tool_use").await? {
            let plugin = registration.plugin;
            let handler = registration.handler;
            let output = self
                .execute_hook(&plugin, &handler, "pre_tool_use", payload.clone())
                .await?;
            self.write_tool_hook_audit(
                &plugin.manifest.id,
                "pre_tool_use",
                tool_call_id,
                run_id,
                thread_id,
                &output,
            )
            .await?;
            if matches!(output.action.as_deref(), Some("block")) {
                return Ok(Some(
                    output
                        .message
                        .unwrap_or_else(|| "Blocked by extension hook".to_string()),
                ));
            }
        }

        Ok(None)
    }

    pub async fn run_post_tool_hooks(
        &self,
        tool_name: &str,
        tool_input: &serde_json::Value,
        tool_result: &serde_json::Value,
        workspace_path: &str,
        thread_id: &str,
        run_id: &str,
        tool_call_id: &str,
    ) -> Result<(), AppError> {
        let payload = serde_json::json!({
            "toolName": tool_name,
            "toolArgs": tool_input,
            "toolResult": tool_result,
            "workspace": workspace_path,
            "threadId": thread_id,
            "runId": run_id,
        });

        for registration in self.load_plugin_hook_registrations("post_tool_use").await? {
            let plugin = registration.plugin;
            let handler = registration.handler;
            let output = self
                .execute_hook(&plugin, &handler, "post_tool_use", payload.clone())
                .await?;
            self.write_tool_hook_audit(
                &plugin.manifest.id,
                "post_tool_use",
                tool_call_id,
                run_id,
                thread_id,
                &output,
            )
            .await?;
        }

        Ok(())
    }

    pub fn provider_context_from_resolved(resolved: &ResolvedTool) -> ToolProviderContext {
        ToolProviderContext {
            provider_type: resolved.provider_type.clone(),
            provider_id: resolved.provider_id.clone(),
            required_permission: resolved.required_permission.clone(),
        }
    }

    async fn collect_plugin_summaries(&self) -> Result<Vec<ExtensionSummaryDto>, AppError> {
        let installed = self.load_installed_plugin_records().await?;
        let installed_ids = installed
            .iter()
            .map(|record| record.id.clone())
            .collect::<HashSet<_>>();
        let mut items = Vec::new();

        for plugin in self.load_plugin_runtimes().await? {
            let install_state = if plugin.enabled {
                ExtensionInstallState::Enabled
            } else {
                ExtensionInstallState::Disabled
            };
            items.push(self.build_plugin_summary(&plugin, install_state, None));
        }

        for discovered_dir in self.discover_plugin_dirs()? {
            let runtime = match self.load_plugin_from_dir(&discovered_dir, true) {
                Ok(runtime) => runtime,
                Err(error) => {
                    let name = discovered_dir
                        .file_name()
                        .and_then(OsStr::to_str)
                        .unwrap_or("Unknown plugin")
                        .to_string();
                    items.push(ExtensionSummaryDto {
                        id: format!("discovered:{}", discovered_dir.display()),
                        kind: ExtensionKind::Plugin,
                        name,
                        version: "0.0.0".to_string(),
                        description: Some(error.user_message),
                        source: ExtensionSourceDto::LocalDir {
                            path: discovered_dir.to_string_lossy().to_string(),
                        },
                        install_state: ExtensionInstallState::Error,
                        health: ExtensionHealth::Error,
                        permissions: Vec::new(),
                        tags: vec!["local".to_string()],
                    });
                    continue;
                }
            };

            if installed_ids.contains(&runtime.manifest.id) {
                continue;
            }

            items.push(self.build_plugin_summary(
                &runtime,
                ExtensionInstallState::Discovered,
                None,
            ));
        }

        Ok(items)
    }

    async fn collect_mcp_summaries(
        &self,
        workspace_path: Option<&str>,
        scope: ConfigScope,
    ) -> Result<Vec<ExtensionSummaryDto>, AppError> {
        Ok(self
            .list_mcp_servers(workspace_path, scope)
            .await?
            .into_iter()
            .map(|server| self.build_mcp_summary(&server))
            .collect())
    }

    async fn collect_skill_summaries(
        &self,
        workspace_path: Option<&str>,
        scope: ConfigScope,
    ) -> Result<Vec<ExtensionSummaryDto>, AppError> {
        Ok(self
            .load_skills(workspace_path, scope)
            .await?
            .into_iter()
            .map(|skill| self.build_skill_summary(&skill.record))
            .collect())
    }

    async fn load_plugin_runtimes(&self) -> Result<Vec<InstalledPluginRuntime>, AppError> {
        let installed = self.load_installed_plugin_records().await?;
        let mut items = Vec::with_capacity(installed.len());
        for record in installed {
            let mut runtime = self.load_plugin_from_dir(Path::new(&record.path), false)?;
            runtime.enabled = record.enabled;
            items.push(runtime);
        }
        items.sort_by(|left, right| {
            left.manifest
                .name
                .to_lowercase()
                .cmp(&right.manifest.name.to_lowercase())
        });
        Ok(items)
    }

    async fn load_enabled_plugin_runtimes(&self) -> Result<Vec<InstalledPluginRuntime>, AppError> {
        Ok(self
            .load_plugin_runtimes()
            .await?
            .into_iter()
            .filter(|plugin| plugin.enabled)
            .collect())
    }

    async fn load_registered_plugin_commands(&self) -> Result<Vec<ExtensionCommandDto>, AppError> {
        Ok(self
            .load_enabled_plugin_runtimes()
            .await?
            .into_iter()
            .flat_map(|plugin| {
                self.load_plugin_command_definitions(&plugin)
                    .into_iter()
                    .filter_map(move |command| {
                        command.prompt_template.clone().map(|prompt_template| {
                            PluginCommandRegistration {
                                plugin_id: plugin.manifest.id.clone(),
                                command: PluginManifestCommand {
                                    prompt_template: Some(prompt_template),
                                    ..command
                                },
                            }
                        })
                    })
            })
            .map(|registration| ExtensionCommandDto {
                plugin_id: registration.plugin_id,
                name: registration.command.name,
                description: registration.command.description,
                prompt_template: registration.command.prompt_template.unwrap_or_default(),
            })
            .collect())
    }

    fn load_plugin_command_definitions(
        &self,
        plugin: &InstalledPluginRuntime,
    ) -> Vec<PluginManifestCommand> {
        let mut commands = plugin.manifest.commands.clone();
        let commands_root = plugin.path.join("commands");
        if !commands_root.is_dir() {
            return commands;
        }

        for entry in fs::read_dir(&commands_root).ok().into_iter().flatten() {
            let Ok(entry) = entry else {
                continue;
            };
            let path = entry.path();
            if !path.is_file() {
                continue;
            }
            let Some(name) = path.file_stem().and_then(OsStr::to_str) else {
                continue;
            };
            if commands.iter().any(|command| command.name == name) {
                continue;
            }
            let Ok(raw) = fs::read_to_string(&path) else {
                tracing::warn!(path = %path.display(), "failed to read plugin command file");
                continue;
            };
            let Some(command) = parse_plugin_command_markdown(&raw, name) else {
                tracing::warn!(path = %path.display(), "failed to parse plugin command file");
                continue;
            };
            commands.push(command);
        }

        commands.sort_by(|left, right| left.name.cmp(&right.name));
        commands
    }

    async fn load_plugin_hook_registrations(
        &self,
        event: &str,
    ) -> Result<Vec<PluginHookRegistration>, AppError> {
        Ok(self
            .load_enabled_plugin_runtimes()
            .await?
            .into_iter()
            .flat_map(|plugin| {
                self.handlers_for_event(&plugin.manifest, event)
                    .into_iter()
                    .map(move |handler| PluginHookRegistration {
                        plugin: plugin.clone(),
                        handler,
                    })
            })
            .collect())
    }

    fn load_plugin_from_dir(
        &self,
        dir: &Path,
        discovered: bool,
    ) -> Result<InstalledPluginRuntime, AppError> {
        let plugin_dir = fs::canonicalize(dir).map_err(|error| {
            AppError::recoverable(
                ErrorSource::Settings,
                "extensions.plugin.invalid_path",
                format!(
                    "Unable to access plugin directory '{}': {error}",
                    dir.display()
                ),
            )
        })?;
        let manifest_path = if plugin_dir.join("plugin.json").is_file() {
            plugin_dir.join("plugin.json")
        } else {
            plugin_dir.join(".claude-plugin/plugin.json")
        };
        let manifest_raw = fs::read_to_string(&manifest_path).map_err(|error| {
            AppError::recoverable(
                ErrorSource::Settings,
                "extensions.plugin.missing_manifest",
                format!(
                    "Unable to read '{}' for plugin '{}': {error}",
                    manifest_path.display(),
                    dir.display()
                ),
            )
        })?;
        let manifest = parse_plugin_manifest(&manifest_raw, &plugin_dir).map_err(|error| {
            AppError::recoverable(
                ErrorSource::Settings,
                "extensions.plugin.invalid_manifest",
                format!(
                    "Invalid plugin manifest in '{}': {error}",
                    manifest_path.display()
                ),
            )
        })?;

        if !discovered && !plugin_dir.exists() {
            return Err(AppError::not_found(
                ErrorSource::Settings,
                format!("plugin directory '{}'", plugin_dir.display()),
            ));
        }

        Ok(InstalledPluginRuntime {
            enabled: manifest.default_enabled.unwrap_or(true),
            manifest,
            path: plugin_dir,
        })
    }

    fn build_plugin_summary(
        &self,
        plugin: &InstalledPluginRuntime,
        install_state: ExtensionInstallState,
        last_error: Option<String>,
    ) -> ExtensionSummaryDto {
        ExtensionSummaryDto {
            id: plugin.manifest.id.clone(),
            kind: ExtensionKind::Plugin,
            name: plugin.manifest.name.clone(),
            version: plugin.manifest.version.clone(),
            description: plugin.manifest.description.clone().or(last_error.clone()),
            source: ExtensionSourceDto::LocalDir {
                path: plugin.path.to_string_lossy().to_string(),
            },
            install_state: install_state.clone(),
            health: match install_state {
                ExtensionInstallState::Enabled => ExtensionHealth::Healthy,
                ExtensionInstallState::Error => ExtensionHealth::Error,
                _ => ExtensionHealth::Unknown,
            },
            permissions: plugin.manifest.permissions.clone(),
            tags: plugin.manifest.capabilities.clone(),
        }
    }

    fn build_plugin_detail(
        &self,
        plugin: &InstalledPluginRuntime,
        last_error: Option<String>,
    ) -> PluginDetailDto {
        let command_names = self.collect_command_bundle_names(&plugin.path, &plugin.manifest);
        let commands = command_names
            .into_iter()
            .map(|name| {
                plugin
                    .manifest
                    .commands
                    .iter()
                    .find(|command| command.name == name)
                    .cloned()
                    .unwrap_or(PluginManifestCommand {
                        name,
                        description: "Bundled command".to_string(),
                        prompt_template: None,
                    })
            })
            .map(|command| PluginCommandDto {
                name: command.name,
                description: command.description,
                prompt_template: command.prompt_template,
            })
            .collect();

        PluginDetailDto {
            id: plugin.manifest.id.clone(),
            path: plugin.path.to_string_lossy().to_string(),
            author: plugin.manifest.author.clone(),
            homepage: plugin.manifest.homepage.clone(),
            default_enabled: plugin.manifest.default_enabled.unwrap_or(true),
            enabled: plugin.enabled,
            capabilities: plugin.manifest.capabilities.clone(),
            permissions: plugin.manifest.permissions.clone(),
            hooks: self.build_plugin_hook_groups(&plugin.manifest),
            tools: plugin
                .manifest
                .tools
                .iter()
                .map(|tool| PluginToolDto {
                    name: tool.name.clone(),
                    description: tool.description.clone(),
                    command: tool.command.clone(),
                    args: tool.args.clone(),
                    cwd: tool.cwd.clone(),
                    timeout_ms: tool.timeout_ms,
                    required_permission: tool.required_permission.clone(),
                })
                .collect(),
            commands,
            bundled_skills: self
                .collect_skill_bundle_names(&plugin.path, plugin.manifest.skills_dir.as_deref()),
            bundled_mcp_servers: self.collect_mcp_bundle_names(&plugin.path),
            timeout_ms: plugin.manifest.timeout_ms,
            skills_dir: plugin.manifest.skills_dir.clone(),
            config_schema_path: plugin
                .manifest
                .config_schema
                .as_ref()
                .map(|schema| schema.path.clone()),
            last_error,
        }
    }

    fn discover_plugin_dirs(&self) -> Result<Vec<PathBuf>, AppError> {
        let base = tiy_home().join("plugins");
        let mut dirs = Vec::new();
        if !base.exists() {
            return Ok(dirs);
        }
        for entry in fs::read_dir(base)? {
            let entry = entry?;
            let path = entry.path();
            if path.is_dir() {
                dirs.push(path);
            }
        }
        Ok(dirs)
    }

    async fn load_installed_plugin_records(&self) -> Result<Vec<InstalledPluginRecord>, AppError> {
        let file = global_plugins_config_path();
        let records = self
            .read_json_file_with_diagnostics::<Vec<InstalledPluginRecord>>(
                &file,
                "plugins",
                ConfigScope::Global,
            )?
            .value;
        if !records.is_empty() {
            return Ok(records);
        }

        let legacy_records = self
            .read_json_setting::<Vec<InstalledPluginRecord>>(
                LEGACY_EXTENSIONS_INSTALLED_PLUGINS_KEY,
            )
            .await?;
        if !legacy_records.is_empty() {
            self.save_installed_plugin_records(&legacy_records).await?;
            let _ =
                settings_repo::delete(&self.pool, LEGACY_EXTENSIONS_INSTALLED_PLUGINS_KEY).await;
            return Ok(legacy_records);
        }

        Ok(records)
    }

    async fn save_installed_plugin_records(
        &self,
        records: &[InstalledPluginRecord],
    ) -> Result<(), AppError> {
        self.write_json_file(&global_plugins_config_path(), records)
    }

    async fn load_plugin_config_store(&self) -> Result<PluginConfigStore, AppError> {
        self.read_json_setting(EXTENSIONS_PLUGIN_CONFIG_KEY).await
    }

    async fn save_plugin_config_store(&self, store: &PluginConfigStore) -> Result<(), AppError> {
        self.write_json_setting(EXTENSIONS_PLUGIN_CONFIG_KEY, store)
            .await
    }

    async fn update_plugin_enabled(&self, id: &str, enabled: bool) -> Result<bool, AppError> {
        let mut records = self.load_installed_plugin_records().await?;
        let mut found = false;
        let mut target_path = None;
        for record in &mut records {
            if record.id == id {
                record.enabled = enabled;
                target_path = Some(record.path.clone());
                found = true;
                break;
            }
        }
        if found {
            self.save_installed_plugin_records(&records).await?;
            if let Some(path) = target_path {
                let mut plugin = self.load_plugin_from_dir(Path::new(&path), false)?;
                plugin.enabled = enabled;
                self.sync_plugin_managed_mcp_configs(&plugin).await?;
            }
        }
        Ok(found)
    }

    async fn uninstall_plugin(&self, id: &str) -> Result<bool, AppError> {
        let mut records = self.load_installed_plugin_records().await?;
        let before = records.len();
        records.retain(|record| record.id != id);
        if before == records.len() {
            return Ok(false);
        }
        self.save_installed_plugin_records(&records).await?;
        self.remove_plugin_managed_mcp_configs(id).await?;
        Ok(true)
    }

    async fn load_mcp_configs_for_scope(
        &self,
        workspace_path: Option<&str>,
        scope: ConfigScope,
    ) -> Result<Vec<McpServerConfigInput>, AppError> {
        let file = match scope {
            ConfigScope::Global => global_mcp_path(),
            ConfigScope::Workspace => workspace_mcp_read_path(workspace_path)?,
        };
        Ok(self
            .read_json_file_with_diagnostics::<McpConfigFile>(&file, "mcp", scope)?
            .value
            .servers
            .into_iter()
            .map(canonicalize_mcp_config)
            .collect())
    }

    async fn load_mcp_configs_with_scope(
        &self,
        workspace_path: Option<&str>,
        scope: ConfigScope,
    ) -> Result<Vec<(McpServerConfigInput, ConfigScope)>, AppError> {
        if scope == ConfigScope::Global || workspace_path.is_none() {
            return Ok(self
                .load_mcp_configs_for_scope(None, ConfigScope::Global)
                .await?
                .into_iter()
                .map(|config| (config, ConfigScope::Global))
                .collect());
        }

        let global = self
            .load_mcp_configs_for_scope(None, ConfigScope::Global)
            .await?;
        let workspace = self
            .load_mcp_configs_for_scope(workspace_path, ConfigScope::Workspace)
            .await?;
        let workspace_ids = workspace
            .iter()
            .map(|config| config.id.clone())
            .collect::<HashSet<_>>();

        let mut items = global
            .into_iter()
            .filter(|config| !workspace_ids.contains(&config.id))
            .map(|config| (config, ConfigScope::Global))
            .collect::<Vec<_>>();
        items.extend(
            workspace
                .into_iter()
                .map(|config| (config, ConfigScope::Workspace)),
        );
        items.sort_by(|left, right| {
            left.0
                .label
                .to_lowercase()
                .cmp(&right.0.label.to_lowercase())
        });
        Ok(items)
    }

    async fn save_mcp_configs_for_scope(
        &self,
        configs: &[McpServerConfigInput],
        workspace_path: Option<&str>,
        scope: ConfigScope,
    ) -> Result<(), AppError> {
        let file = match scope {
            ConfigScope::Global => global_mcp_path(),
            ConfigScope::Workspace => workspace_mcp_path(workspace_path)?,
        };
        self.write_json_file(
            &file,
            &McpConfigFile {
                servers: configs.to_vec(),
            },
        )
    }

    async fn load_mcp_runtime_store(&self) -> Result<McpRuntimeStore, AppError> {
        self.read_json_setting(EXTENSIONS_MCP_RUNTIME_KEY).await
    }

    async fn save_mcp_runtime_store(&self, store: &McpRuntimeStore) -> Result<(), AppError> {
        self.write_json_setting(EXTENSIONS_MCP_RUNTIME_KEY, store)
            .await
    }

    fn validate_mcp_input(&self, input: &McpServerConfigInput) -> Result<(), AppError> {
        if input.id.trim().is_empty() || input.label.trim().is_empty() {
            return Err(AppError::validation(
                ErrorSource::Settings,
                "MCP id and label are required",
            ));
        }

        match canonicalize_mcp_transport(&input.transport) {
            "stdio" => {
                if input.command.as_deref().unwrap_or("").trim().is_empty() {
                    return Err(AppError::validation(
                        ErrorSource::Settings,
                        "stdio MCP servers require a command",
                    ));
                }
            }
            "streamable-http" => {
                if input.url.as_deref().unwrap_or("").trim().is_empty() {
                    return Err(AppError::validation(
                        ErrorSource::Settings,
                        "streamable-http MCP servers require a URL",
                    ));
                }
            }
            _ => {
                return Err(AppError::validation(
                    ErrorSource::Settings,
                    "Unsupported MCP transport",
                ))
            }
        }

        Ok(())
    }

    async fn refresh_mcp_runtime(
        &self,
        config: &McpServerConfigInput,
        phase_override: Option<&str>,
        scope: &str,
    ) -> Result<McpServerStateDto, AppError> {
        tracing::info!(server = %config.label, id = %config.id, scope, transport = ?config.transport, "MCP runtime refresh starting");
        let mut store = self.load_mcp_runtime_store().await?;
        let previous_runtime = store.items.remove(&config.id).unwrap_or_default();
        let mut runtime: McpRuntimeRecord;
        let updated_at = Utc::now().to_rfc3339();

        let (status, phase, last_error) = if !config.enabled {
            runtime = McpRuntimeRecord::default();
            ("disconnected".to_string(), "shutdown".to_string(), None)
        } else if let Err(error) = self.validate_mcp_input(config) {
            runtime = McpRuntimeRecord::default();
            (
                "config_error".to_string(),
                "config_load".to_string(),
                Some(error.user_message),
            )
        } else {
            match self.probe_mcp_runtime(config).await {
                Ok(probed_runtime) => {
                    runtime = probed_runtime;
                    (
                        "connected".to_string(),
                        phase_override.unwrap_or("ready").to_string(),
                        None,
                    )
                }
                Err(error) => {
                    let has_snapshot = !previous_runtime.tools.is_empty()
                        || !previous_runtime.resources.is_empty();
                    runtime = if has_snapshot {
                        McpRuntimeRecord {
                            stale_snapshot: true,
                            last_error: Some(error.user_message.clone()),
                            status: Some("degraded".to_string()),
                            phase: Some(phase_override.unwrap_or("runtime_probe").to_string()),
                            updated_at: previous_runtime.updated_at.clone(),
                            ..previous_runtime
                        }
                    } else {
                        McpRuntimeRecord {
                            tools: Vec::new(),
                            resources: Vec::new(),
                            stale_snapshot: false,
                            last_error: Some(error.user_message.clone()),
                            status: Some("error".to_string()),
                            phase: Some(phase_override.unwrap_or("runtime_probe").to_string()),
                            updated_at: None,
                        }
                    };

                    (
                        runtime
                            .status
                            .clone()
                            .unwrap_or_else(|| "error".to_string()),
                        runtime.phase.clone().unwrap_or_else(|| {
                            phase_override.unwrap_or("runtime_probe").to_string()
                        }),
                        runtime.last_error.clone(),
                    )
                }
            }
        };

        runtime.status = Some(status.clone());
        runtime.phase = Some(phase.clone());
        runtime.last_error = last_error.clone();
        runtime.updated_at = Some(updated_at.clone());
        store.items.insert(config.id.clone(), runtime.clone());
        self.save_mcp_runtime_store(&store).await?;

        tracing::info!(server = %config.label, %status, %phase, last_error = ?last_error, "MCP runtime refresh completed");
        Ok(McpServerStateDto {
            id: config.id.clone(),
            label: config.label.clone(),
            scope: scope.to_string(),
            status,
            phase,
            tools: runtime.tools,
            resources: runtime.resources,
            stale_snapshot: runtime.stale_snapshot,
            last_error,
            updated_at,
            config: self.mask_mcp_config(config),
        })
    }

    fn build_mcp_state(
        &self,
        config: &McpServerConfigInput,
        runtime: Option<&McpRuntimeRecord>,
        scope: &str,
    ) -> McpServerStateDto {
        let validation_error = self.validate_mcp_input(&config).err();
        let now = Utc::now().to_rfc3339();
        let runtime = runtime.cloned().unwrap_or_default();

        let (status, phase, last_error) = if !config.enabled {
            ("disconnected".to_string(), "shutdown".to_string(), None)
        } else if let Some(error) = validation_error {
            (
                "config_error".to_string(),
                "config_load".to_string(),
                Some(error.user_message),
            )
        } else if runtime.status.is_none() && runtime.phase.is_none() {
            (
                "disconnected".to_string(),
                "not_started".to_string(),
                runtime.last_error.clone(),
            )
        } else if runtime.stale_snapshot {
            (
                "degraded".to_string(),
                runtime.phase.unwrap_or_else(|| "runtime_probe".to_string()),
                runtime.last_error.clone(),
            )
        } else {
            (
                runtime.status.unwrap_or_else(|| "disconnected".to_string()),
                runtime.phase.unwrap_or_else(|| "not_started".to_string()),
                runtime.last_error.clone(),
            )
        };

        McpServerStateDto {
            id: config.id.clone(),
            label: config.label.clone(),
            scope: scope.to_string(),
            status,
            phase,
            tools: runtime.tools,
            resources: runtime.resources,
            stale_snapshot: runtime.stale_snapshot,
            last_error,
            updated_at: runtime.updated_at.unwrap_or(now),
            config: self.mask_mcp_config(config),
        }
    }

    fn build_mcp_summary(&self, server: &McpServerStateDto) -> ExtensionSummaryDto {
        let install_state = if !server.config.enabled {
            ExtensionInstallState::Disabled
        } else if server.status == "config_error" || server.status == "error" {
            ExtensionInstallState::Error
        } else if server.status == "connected" || server.status == "degraded" {
            ExtensionInstallState::Enabled
        } else {
            ExtensionInstallState::Installed
        };

        let mut tags = vec![server.config.transport.clone()];
        if server.stale_snapshot {
            tags.push("stale-snapshot".to_string());
        }

        ExtensionSummaryDto {
            id: server.id.clone(),
            kind: ExtensionKind::Mcp,
            name: server.label.clone(),
            version: "config".to_string(),
            description: server.last_error.clone(),
            source: ExtensionSourceDto::Builtin,
            install_state: install_state.clone(),
            health: match server.status.as_str() {
                "connected" => ExtensionHealth::Healthy,
                "degraded" => ExtensionHealth::Degraded,
                "error" | "config_error" => ExtensionHealth::Error,
                _ => ExtensionHealth::Unknown,
            },
            permissions: if canonicalize_mcp_transport(&server.config.transport)
                == "streamable-http"
            {
                vec!["network-access".to_string()]
            } else {
                vec!["shell-exec".to_string()]
            },
            tags,
        }
    }

    fn mask_mcp_config(&self, input: &McpServerConfigInput) -> McpServerConfigDto {
        McpServerConfigDto {
            id: input.id.clone(),
            label: input.label.clone(),
            transport: input.transport.clone(),
            enabled: input.enabled,
            auto_start: input.auto_start,
            command: input.command.clone(),
            args: input.args.clone().unwrap_or_default(),
            env: input
                .env
                .clone()
                .unwrap_or_default()
                .into_iter()
                .map(|(key, value)| (key, mask_sensitive_value(&value)))
                .collect(),
            cwd: input.cwd.clone(),
            url: input.url.clone().map(mask_url),
            headers: input
                .headers
                .clone()
                .unwrap_or_default()
                .into_iter()
                .map(|(key, value)| (key, mask_sensitive_value(&value)))
                .collect(),
            timeout_ms: input.timeout_ms,
        }
    }

    async fn load_skills(
        &self,
        workspace_path: Option<&str>,
        scope: ConfigScope,
    ) -> Result<Vec<SkillRuntime>, AppError> {
        let global_state = self
            .load_skill_state_store(None, ConfigScope::Global)
            .await?;
        let workspace_state = if scope == ConfigScope::Workspace && workspace_path.is_some() {
            Some(
                self.load_skill_state_store(workspace_path, ConfigScope::Workspace)
                    .await?,
            )
        } else {
            None
        };
        let max_prompt_chars = self.load_skill_prompt_budget().await?;
        let mut results = Vec::new();
        let mut visited = HashSet::new();

        for (source_label, path) in self.skill_source_roots(workspace_path, scope).await? {
            if !path.exists() {
                continue;
            }

            for skill_dir in read_child_dirs(&path)? {
                let skill_file = skill_dir.join("SKILL.md");
                if !skill_file.is_file() {
                    continue;
                }
                let raw = match fs::read_to_string(&skill_file) {
                    Ok(raw) => raw,
                    Err(error) => {
                        tracing::warn!(path = %skill_file.display(), error = %error, "failed to read skill file");
                        continue;
                    }
                };
                let parsed = parse_skill_markdown(&raw, &skill_dir, &source_label);
                let Some((mut record, content)) = parsed else {
                    continue;
                };
                if visited.contains(&record.id) {
                    continue;
                }
                visited.insert(record.id.clone());

                apply_skill_state(&mut record, &global_state);
                if let Some(workspace_state) = workspace_state.as_ref() {
                    apply_skill_state(&mut record, workspace_state);
                }
                record.scope = scope.as_str().to_string();

                record.prompt_budget_chars = max_prompt_chars.min(record.content_preview.len());
                results.push(SkillRuntime { record, content });
            }
        }

        results.sort_by(|left, right| compare_skill_records(&left.record, &right.record));
        Ok(results)
    }

    async fn load_skill_state_store(
        &self,
        workspace_path: Option<&str>,
        scope: ConfigScope,
    ) -> Result<SkillStateStore, AppError> {
        let file = match scope {
            ConfigScope::Global => global_skills_config_path(),
            ConfigScope::Workspace => workspace_skills_read_path(workspace_path)?,
        };
        Ok(self
            .read_json_file_with_diagnostics(&file, "skills", scope)?
            .value)
    }

    async fn save_skill_state_store(
        &self,
        store: &SkillStateStore,
        workspace_path: Option<&str>,
        scope: ConfigScope,
    ) -> Result<(), AppError> {
        let file = match scope {
            ConfigScope::Global => global_skills_config_path(),
            ConfigScope::Workspace => workspace_skills_config_path(workspace_path)?,
        };
        self.write_json_file(&file, store)
    }

    async fn load_skill_prompt_budget(&self) -> Result<usize, AppError> {
        let max_chars_record =
            settings_repo::get(&self.pool, EXTENSIONS_SKILLS_MAX_PROMPT_CHARS_KEY).await?;
        let max_count_record =
            settings_repo::get(&self.pool, EXTENSIONS_SKILLS_MAX_SELECTED_COUNT_KEY).await?;
        let max_chars = max_chars_record
            .and_then(|record| serde_json::from_str::<usize>(&record.value_json).ok())
            .unwrap_or(4_000);
        let max_count = max_count_record
            .and_then(|record| serde_json::from_str::<usize>(&record.value_json).ok())
            .unwrap_or(4);
        Ok(max_chars.saturating_mul(max_count.max(1)))
    }

    async fn skill_source_roots(
        &self,
        workspace_path: Option<&str>,
        scope: ConfigScope,
    ) -> Result<Vec<(String, PathBuf)>, AppError> {
        let mut roots = vec![
            ("builtin".to_string(), agents_home().join("skills")),
            ("builtin".to_string(), tiy_home().join("skills")),
        ];
        if scope == ConfigScope::Workspace {
            if let Some(workspace_path) = workspace_path {
                roots.push((
                    "workspace".to_string(),
                    PathBuf::from(workspace_path).join(".tiy/skills"),
                ));
            }
        }
        for plugin in self.load_enabled_plugin_runtimes().await? {
            let skills_dir = plugin
                .manifest
                .skills_dir
                .clone()
                .unwrap_or_else(|| "skills".to_string());
            roots.push(("plugin".to_string(), plugin.path.join(skills_dir)));
        }
        Ok(roots)
    }

    fn build_skill_summary(&self, record: &SkillRecordDto) -> ExtensionSummaryDto {
        ExtensionSummaryDto {
            id: record.id.clone(),
            kind: ExtensionKind::Skill,
            name: record.name.clone(),
            version: "content".to_string(),
            description: record.description.clone(),
            source: match record.source.as_str() {
                "builtin" => ExtensionSourceDto::Builtin,
                _ => ExtensionSourceDto::LocalDir {
                    path: record.path.clone(),
                },
            },
            install_state: if record.enabled {
                ExtensionInstallState::Enabled
            } else {
                ExtensionInstallState::Disabled
            },
            health: ExtensionHealth::Healthy,
            permissions: Vec::new(),
            tags: record.tags.clone(),
        }
    }

    async fn skill_exists(
        &self,
        id: &str,
        workspace_path: Option<&str>,
        scope: ConfigScope,
    ) -> Result<bool, AppError> {
        Ok(self
            .load_skills(workspace_path, scope)
            .await?
            .iter()
            .any(|skill| skill.record.id == id))
    }

    fn load_marketplace_sources(&self) -> Result<MarketplaceSourceStore, AppError> {
        let mut store = self
            .read_json_file_with_diagnostics::<MarketplaceSourceStore>(
                &global_marketplace_sources_path(),
                "marketplaces",
                ConfigScope::Global,
            )?
            .value;
        let mut by_id = store
            .sources
            .into_iter()
            .map(|source| (source.id.clone(), source))
            .collect::<HashMap<_, _>>();
        for source in builtin_marketplace_sources() {
            by_id.entry(source.id.clone()).or_insert(source);
        }
        let mut sources = by_id.into_values().collect::<Vec<_>>();
        sources.sort_by(|left, right| left.name.to_lowercase().cmp(&right.name.to_lowercase()));
        store.sources = sources;
        Ok(store)
    }

    fn save_marketplace_sources(&self, store: &MarketplaceSourceStore) -> Result<(), AppError> {
        let persisted = MarketplaceSourceStore {
            sources: store
                .sources
                .iter()
                .filter(|source| !is_builtin_marketplace_source_id(&source.id))
                .cloned()
                .collect(),
        };
        self.write_json_file(&global_marketplace_sources_path(), &persisted)
    }

    async fn sync_marketplace_source_repo(
        &self,
        source: &MarketplaceSourceRecord,
    ) -> Result<(), AppError> {
        let cache_dir = marketplace_cache_root().join(&source.id);
        if let Some(parent) = cache_dir.parent() {
            fs::create_dir_all(parent)?;
        }

        let mut command = Command::new("git");
        configure_background_tokio_command(&mut command);
        if cache_dir.exists() {
            command
                .arg("-C")
                .arg(&cache_dir)
                .arg("pull")
                .arg("--ff-only");
        } else {
            command
                .arg("clone")
                .arg("--depth")
                .arg("1")
                .arg(&source.url)
                .arg(&cache_dir);
        }
        let output = command.output().await.map_err(|error| {
            AppError::recoverable(
                ErrorSource::Settings,
                "extensions.marketplace.git_failed",
                format!(
                    "Failed to sync marketplace source '{}': {error}",
                    source.name
                ),
            )
        })?;
        if !output.status.success() {
            return Err(AppError::recoverable(
                ErrorSource::Settings,
                "extensions.marketplace.git_failed",
                String::from_utf8_lossy(&output.stderr).trim().to_string(),
            ));
        }
        Ok(())
    }

    async fn marketplace_items_for_source(
        &self,
        source: &MarketplaceSourceRecord,
    ) -> Result<Vec<MarketplaceItemDto>, AppError> {
        let cache_dir = marketplace_cache_root().join(&source.id);
        if !cache_dir.exists() {
            return Ok(Vec::new());
        }

        let source_manifest = self.read_json_file::<MarketplaceSourceManifest>(
            &cache_dir.join(".claude-plugin/marketplace.json"),
        )?;
        let source_name = source_manifest.name.unwrap_or_else(|| source.name.clone());
        let installed = self
            .load_installed_plugin_records()
            .await?
            .into_iter()
            .map(|record| (record.path, record.enabled))
            .collect::<HashMap<_, _>>();

        let mut items = Vec::new();
        for root_name in ["plugins", "external_plugins"] {
            let root = cache_dir.join(root_name);
            if !root.is_dir() {
                continue;
            }
            for plugin_dir in read_child_dirs(&root)? {
                let plugin_manifest_path = plugin_dir.join(".claude-plugin/plugin.json");
                if !plugin_manifest_path.is_file() {
                    continue;
                }
                let raw = fs::read_to_string(&plugin_manifest_path)?;
                let manifest = parse_plugin_manifest(&raw, &plugin_dir).map_err(|error| {
                    AppError::recoverable(
                        ErrorSource::Settings,
                        "extensions.marketplace.invalid_plugin_manifest",
                        format!(
                            "Invalid marketplace plugin manifest '{}': {error}",
                            plugin_manifest_path.display()
                        ),
                    )
                })?;
                let plugin_path = plugin_dir.to_string_lossy().to_string();
                let plugin_id = format!(
                    "{}:{}",
                    source.id,
                    plugin_dir
                        .file_name()
                        .and_then(OsStr::to_str)
                        .unwrap_or("plugin")
                );
                let mut tags = Vec::new();
                let skill_names =
                    self.collect_skill_bundle_names(&plugin_dir, manifest.skills_dir.as_deref());
                if !skill_names.is_empty() {
                    tags.push("skill-pack".to_string());
                }
                let command_names = self.collect_command_bundle_names(&plugin_dir, &manifest);
                if !command_names.is_empty() || plugin_dir.join("commands").is_dir() {
                    tags.push("command-provider".to_string());
                }
                let mcp_servers = self.collect_mcp_bundle_names(&plugin_dir);
                if !mcp_servers.is_empty() {
                    tags.push("mcp-bundle".to_string());
                }
                let (installed_flag, enabled_flag) = installed
                    .get(&plugin_path)
                    .map(|enabled| (true, *enabled))
                    .unwrap_or((false, false));
                items.push(MarketplaceItemDto {
                    id: plugin_id,
                    source_id: source.id.clone(),
                    source_name: source_name.clone(),
                    kind: "plugin".to_string(),
                    name: manifest.name.clone(),
                    version: manifest.version.clone(),
                    summary: manifest
                        .description
                        .clone()
                        .unwrap_or_else(|| "Marketplace plugin".to_string()),
                    description: manifest
                        .description
                        .clone()
                        .unwrap_or_else(|| "Marketplace plugin".to_string()),
                    publisher: manifest
                        .author
                        .clone()
                        .unwrap_or_else(|| source_name.clone()),
                    tags,
                    hooks: self.build_plugin_hook_groups(&manifest),
                    command_names,
                    mcp_servers,
                    skill_names,
                    path: plugin_path,
                    installable: true,
                    installed: installed_flag,
                    enabled: enabled_flag,
                });
            }
        }

        Ok(items)
    }

    fn build_marketplace_source_dto(
        &self,
        source: &MarketplaceSourceRecord,
        plugin_count: Option<usize>,
    ) -> MarketplaceSourceDto {
        let plugin_count = plugin_count.unwrap_or_else(|| {
            if source.last_error.is_some() {
                0
            } else {
                marketplace_cache_plugin_count(source)
            }
        });

        MarketplaceSourceDto {
            id: source.id.clone(),
            name: source.name.clone(),
            url: source.url.clone(),
            builtin: is_builtin_marketplace_source_id(&source.id),
            kind: source.kind.clone(),
            status: if source.last_error.is_some() {
                "error".to_string()
            } else if source.last_synced_at.is_some() {
                "ready".to_string()
            } else {
                "idle".to_string()
            },
            last_synced_at: source.last_synced_at.clone(),
            last_error: source.last_error.clone(),
            plugin_count,
        }
    }

    async fn build_marketplace_remove_source_plan(
        &self,
        id: &str,
    ) -> Result<MarketplaceRemoveSourcePlanDto, AppError> {
        if is_builtin_marketplace_source_id(id) {
            return Err(AppError::validation(
                ErrorSource::Settings,
                "Builtin marketplace sources cannot be removed",
            ));
        }

        let store = self.load_marketplace_sources()?;
        let source = store
            .sources
            .into_iter()
            .find(|source| source.id == id)
            .ok_or_else(|| {
                AppError::not_found(ErrorSource::Settings, format!("marketplace source '{id}'"))
            })?;
        let source_root = marketplace_cache_root().join(&source.id);

        let mut installed_plugins = self
            .load_installed_plugin_records()
            .await?
            .into_iter()
            .filter(|record| Path::new(&record.path).starts_with(&source_root))
            .map(|record| {
                let runtime = self
                    .load_plugin_from_dir(Path::new(&record.path), false)
                    .ok();
                MarketplaceSourcePluginRefDto {
                    id: record.id,
                    name: runtime
                        .as_ref()
                        .map(|plugin| plugin.manifest.name.clone())
                        .unwrap_or_else(|| "Unknown plugin".to_string()),
                    version: runtime
                        .as_ref()
                        .map(|plugin| plugin.manifest.version.clone())
                        .unwrap_or_else(|| "unknown".to_string()),
                    enabled: record.enabled,
                    path: record.path,
                }
            })
            .collect::<Vec<_>>();

        installed_plugins.sort_by(|left, right| {
            left.name
                .to_lowercase()
                .cmp(&right.name.to_lowercase())
                .then_with(|| left.id.cmp(&right.id))
        });

        let (blocking_plugins, removable_installed_plugins): (Vec<_>, Vec<_>) = installed_plugins
            .into_iter()
            .partition(|plugin| plugin.enabled);
        let can_remove = blocking_plugins.is_empty();
        let summary = if can_remove {
            let removable_count = removable_installed_plugins.len();
            if removable_count == 0 {
                format!("Remove '{}' from Extensions Center.", source.name)
            } else {
                format!(
                    "Remove '{}' and {} installed plugin{} from this source.",
                    source.name,
                    removable_count,
                    if removable_count == 1 { "" } else { "s" }
                )
            }
        } else {
            format!(
                "Disable {} enabled plugin{} before removing '{}'.",
                blocking_plugins.len(),
                if blocking_plugins.len() == 1 { "" } else { "s" },
                source.name
            )
        };

        Ok(MarketplaceRemoveSourcePlanDto {
            source: self.build_marketplace_source_dto(&source, None),
            can_remove,
            blocking_plugins,
            removable_installed_plugins,
            summary,
        })
    }

    fn build_plugin_hook_groups(&self, manifest: &PluginManifest) -> Vec<PluginHookGroupDto> {
        let mut hook_groups = Vec::new();
        for (event, handlers) in [
            (
                "pre_tool_use",
                manifest
                    .hooks
                    .as_ref()
                    .and_then(|hooks| hooks.pre_tool_use.clone()),
            ),
            (
                "post_tool_use",
                manifest
                    .hooks
                    .as_ref()
                    .and_then(|hooks| hooks.post_tool_use.clone()),
            ),
            (
                "run_started",
                manifest
                    .hooks
                    .as_ref()
                    .and_then(|hooks| hooks.on_run_start.clone()),
            ),
            (
                "run_finished",
                manifest
                    .hooks
                    .as_ref()
                    .and_then(|hooks| hooks.on_run_complete.clone()),
            ),
        ] {
            if let Some(handlers) = handlers {
                hook_groups.push(PluginHookGroupDto {
                    event: event.to_string(),
                    handlers,
                });
            }
        }
        hook_groups
    }

    fn collect_skill_bundle_names(
        &self,
        plugin_dir: &Path,
        skills_dir: Option<&str>,
    ) -> Vec<String> {
        let skills_root = plugin_dir.join(skills_dir.unwrap_or("skills"));
        if !skills_root.is_dir() {
            return Vec::new();
        }

        let mut items = read_child_dirs(&skills_root)
            .unwrap_or_default()
            .into_iter()
            .filter_map(|skill_dir| {
                let skill_file = skill_dir.join("SKILL.md");
                if !skill_file.is_file() {
                    return None;
                }
                let raw = fs::read_to_string(&skill_file).ok()?;
                let (record, _) = parse_skill_markdown(&raw, &skill_dir, "plugin")?;
                Some(record.name)
            })
            .collect::<Vec<_>>();
        items.sort();
        items
    }

    fn collect_command_bundle_names(
        &self,
        plugin_dir: &Path,
        manifest: &PluginManifest,
    ) -> Vec<String> {
        let mut items = manifest
            .commands
            .iter()
            .map(|command| command.name.clone())
            .collect::<Vec<_>>();
        let commands_root = plugin_dir.join("commands");
        if commands_root.is_dir() {
            for entry in fs::read_dir(&commands_root).ok().into_iter().flatten() {
                let Ok(entry) = entry else {
                    continue;
                };
                let path = entry.path();
                let candidate = if path.is_file() {
                    path.file_stem()
                } else if path.is_dir() {
                    path.file_name()
                } else {
                    None
                };
                let Some(name) = candidate.and_then(OsStr::to_str) else {
                    continue;
                };
                if !name.is_empty() {
                    items.push(name.to_string());
                }
            }
        }
        items.sort();
        items.dedup();
        items
    }

    fn collect_mcp_bundle_names(&self, plugin_dir: &Path) -> Vec<String> {
        let mcp_path = plugin_dir.join(".mcp.json");
        if !mcp_path.is_file() {
            return Vec::new();
        }
        let raw = match fs::read_to_string(&mcp_path) {
            Ok(raw) => raw,
            Err(_) => return Vec::new(),
        };
        let value = match serde_json::from_str::<serde_json::Value>(&raw) {
            Ok(value) => value,
            Err(_) => return Vec::new(),
        };

        let mut items = match value.get("servers") {
            Some(serde_json::Value::Object(map)) => map.keys().cloned().collect::<Vec<_>>(),
            Some(serde_json::Value::Array(servers)) => servers
                .iter()
                .filter_map(read_named_value)
                .collect::<Vec<_>>(),
            _ => match value {
                serde_json::Value::Object(map) => map
                    .iter()
                    .filter_map(|(key, value)| {
                        if value.is_object() {
                            Some(read_named_value(value).unwrap_or_else(|| key.to_string()))
                        } else {
                            None
                        }
                    })
                    .collect::<Vec<_>>(),
                _ => Vec::new(),
            },
        };
        items.sort();
        items.dedup();
        items
    }

    fn load_plugin_managed_mcp_configs(
        &self,
        plugin: &InstalledPluginRuntime,
    ) -> Result<Vec<McpServerConfigInput>, AppError> {
        let mcp_path = plugin.path.join(".mcp.json");
        if !mcp_path.is_file() {
            return Ok(Vec::new());
        }
        let raw = fs::read_to_string(&mcp_path).map_err(|error| {
            AppError::recoverable(
                ErrorSource::Settings,
                "extensions.plugin.invalid_mcp_bundle",
                format!("Unable to read '{}': {error}", mcp_path.display()),
            )
        })?;
        let value = serde_json::from_str::<serde_json::Value>(&raw).map_err(|error| {
            AppError::recoverable(
                ErrorSource::Settings,
                "extensions.plugin.invalid_mcp_bundle",
                format!("Invalid MCP bundle '{}': {error}", mcp_path.display()),
            )
        })?;

        Ok(read_plugin_mcp_server_entries(&value)
            .into_iter()
            .filter_map(|(server_name, spec)| {
                build_plugin_managed_mcp_config(&plugin.manifest, server_name, spec)
            })
            .collect())
    }

    async fn sync_plugin_managed_mcp_configs(
        &self,
        plugin: &InstalledPluginRuntime,
    ) -> Result<(), AppError> {
        let mut configs = self
            .load_mcp_configs_for_scope(None, ConfigScope::Global)
            .await?;
        let prefix = plugin_managed_mcp_prefix(&plugin.manifest.id);
        let existing_managed_configs = configs
            .iter()
            .filter(|config| config.id.starts_with(&prefix))
            .map(|config| (config.id.clone(), config.clone()))
            .collect::<HashMap<_, _>>();
        configs.retain(|config| !config.id.starts_with(&prefix));
        let managed_configs = if plugin.enabled {
            self.load_plugin_managed_mcp_configs(plugin)?
                .into_iter()
                .map(|config| {
                    merge_plugin_managed_mcp_config(
                        existing_managed_configs.get(&config.id),
                        config,
                    )
                })
                .collect()
        } else {
            Vec::new()
        };
        if plugin.enabled {
            configs.extend(managed_configs.clone());
            configs
                .sort_by(|left, right| left.label.to_lowercase().cmp(&right.label.to_lowercase()));
        }
        self.save_mcp_configs_for_scope(&configs, None, ConfigScope::Global)
            .await?;
        self.remove_mcp_runtime_records_with_prefix(&prefix).await?;
        for config in managed_configs {
            self.refresh_mcp_runtime(&config, None, ConfigScope::Global.as_str())
                .await?;
        }
        Ok(())
    }

    async fn remove_plugin_managed_mcp_configs(&self, plugin_id: &str) -> Result<(), AppError> {
        let mut configs = self
            .load_mcp_configs_for_scope(None, ConfigScope::Global)
            .await?;
        let prefix = plugin_managed_mcp_prefix(plugin_id);
        let before = configs.len();
        configs.retain(|config| !config.id.starts_with(&prefix));
        if before == configs.len() {
            return Ok(());
        }
        self.save_mcp_configs_for_scope(&configs, None, ConfigScope::Global)
            .await?;
        self.remove_mcp_runtime_records_with_prefix(&prefix).await
    }

    async fn read_json_setting<T>(&self, key: &str) -> Result<T, AppError>
    where
        T: for<'de> Deserialize<'de> + Default,
    {
        let value = settings_repo::get(&self.pool, key).await?;
        match value {
            Some(record) => serde_json::from_str(&record.value_json).map_err(|error| {
                AppError::recoverable(
                    ErrorSource::Settings,
                    "extensions.settings.invalid_json",
                    format!("Invalid extension setting payload for '{key}': {error}"),
                )
            }),
            None => Ok(T::default()),
        }
    }

    async fn write_json_setting<T>(&self, key: &str, value: &T) -> Result<(), AppError>
    where
        T: Serialize + ?Sized,
    {
        let encoded = serde_json::to_string(value).map_err(|error| {
            AppError::internal(
                ErrorSource::Settings,
                format!("Failed to serialize extension setting '{key}': {error}"),
            )
        })?;
        settings_repo::set(&self.pool, key, &encoded).await
    }

    fn read_json_file<T>(&self, path: &Path) -> Result<T, AppError>
    where
        T: for<'de> Deserialize<'de> + Default,
    {
        Ok(self
            .read_json_file_with_diagnostics(path, "config", ConfigScope::Global)?
            .value)
    }

    fn read_json_file_with_diagnostics<T>(
        &self,
        path: &Path,
        area: &str,
        scope: ConfigScope,
    ) -> Result<ConfigLoadOutcome<T>, AppError>
    where
        T: for<'de> Deserialize<'de> + Default,
    {
        if !path.exists() {
            self.clear_diagnostic(path, area, scope);
            return Ok(ConfigLoadOutcome {
                value: T::default(),
            });
        }
        let raw = match fs::read_to_string(path) {
            Ok(raw) => raw,
            Err(error) => {
                let diagnostic = self.make_config_diagnostic(
                    path,
                    area,
                    scope,
                    ConfigDiagnosticKind::ReadFailed,
                    format!("Unable to read {area} config"),
                    format!("Failed to read '{}': {error}", path.display()),
                    format!(
                        "Check that '{}' is readable and not locked by another process.",
                        path.display()
                    ),
                );
                self.record_diagnostic(diagnostic.clone());
                return Ok(ConfigLoadOutcome {
                    value: T::default(),
                });
            }
        };

        match serde_json::from_str(&raw) {
            Ok(value) => {
                self.clear_diagnostic(path, area, scope);
                Ok(ConfigLoadOutcome { value })
            }
            Err(error) => {
                let diagnostic = self.make_config_diagnostic(
                    path,
                    area,
                    scope,
                    ConfigDiagnosticKind::InvalidJson,
                    format!("{area} config is not valid JSON"),
                    format!("Invalid JSON in '{}': {error}", path.display()),
                    format!(
                        "Fix the JSON syntax in '{}' or replace it with a valid backup.",
                        path.display()
                    ),
                );
                self.record_diagnostic(diagnostic.clone());
                Ok(ConfigLoadOutcome {
                    value: T::default(),
                })
            }
        }
    }

    fn write_json_file<T>(&self, path: &Path, value: &T) -> Result<(), AppError>
    where
        T: Serialize + ?Sized,
    {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let encoded = serde_json::to_string_pretty(value).map_err(|error| {
            AppError::internal(
                ErrorSource::Settings,
                format!(
                    "Failed to serialize extension config '{}': {error}",
                    path.display()
                ),
            )
        })?;
        fs::write(path, encoded)?;
        Ok(())
    }

    fn record_diagnostic(&self, diagnostic: ConfigDiagnosticDto) {
        if let Ok(mut items) = self.diagnostics.lock() {
            items.retain(|item| item.id != diagnostic.id);
            items.push(diagnostic);
            items.sort_by(|left, right| left.file_path.cmp(&right.file_path));
        }
    }

    fn clear_diagnostic(&self, path: &Path, area: &str, scope: ConfigScope) {
        let id = config_diagnostic_id(path, area, scope);
        if let Ok(mut items) = self.diagnostics.lock() {
            items.retain(|item| item.id != id);
        }
    }

    fn make_config_diagnostic(
        &self,
        path: &Path,
        area: &str,
        scope: ConfigScope,
        kind: ConfigDiagnosticKind,
        summary: String,
        detail: String,
        suggestion: String,
    ) -> ConfigDiagnosticDto {
        ConfigDiagnosticDto {
            id: config_diagnostic_id(path, area, scope),
            scope: scope.as_str().to_string(),
            area: area.to_string(),
            file_path: display_config_path(path),
            severity: ConfigDiagnosticSeverity::Error,
            kind,
            summary,
            detail,
            suggestion,
        }
    }

    async fn write_extension_audit(
        &self,
        action: &str,
        target_type: &str,
        target_id: &str,
        result: serde_json::Value,
    ) -> Result<(), AppError> {
        audit_repo::insert(
            &self.pool,
            &audit_repo::AuditInsert {
                actor_type: "user".to_string(),
                actor_id: None,
                source: "extensions".to_string(),
                workspace_id: None,
                thread_id: None,
                run_id: None,
                tool_call_id: None,
                action: action.to_string(),
                target_type: Some(target_type.to_string()),
                target_id: Some(target_id.to_string()),
                policy_check_json: None,
                result_json: Some(result.to_string()),
            },
        )
        .await
    }

    async fn write_tool_hook_audit(
        &self,
        plugin_id: &str,
        event: &str,
        tool_call_id: &str,
        run_id: &str,
        thread_id: &str,
        output: &HookOutput,
    ) -> Result<(), AppError> {
        audit_repo::insert(
            &self.pool,
            &audit_repo::AuditInsert {
                actor_type: "agent".to_string(),
                actor_id: Some(run_id.to_string()),
                source: format!("plugin:{plugin_id}"),
                workspace_id: None,
                thread_id: Some(thread_id.to_string()),
                run_id: Some(run_id.to_string()),
                tool_call_id: Some(tool_call_id.to_string()),
                action: format!("hook_{event}"),
                target_type: Some("plugin_hook".to_string()),
                target_id: Some(plugin_id.to_string()),
                policy_check_json: None,
                result_json: Some(serde_json::to_string(output).unwrap_or_default()),
            },
        )
        .await
    }

    fn handlers_for_event(&self, manifest: &PluginManifest, event: &str) -> Vec<String> {
        let hooks = match manifest.hooks.as_ref() {
            Some(hooks) => hooks,
            None => return Vec::new(),
        };

        match event {
            "pre_tool_use" => hooks.pre_tool_use.clone().unwrap_or_default(),
            "post_tool_use" => hooks.post_tool_use.clone().unwrap_or_default(),
            "run_started" => hooks.on_run_start.clone().unwrap_or_default(),
            "run_finished" => hooks.on_run_complete.clone().unwrap_or_default(),
            _ => Vec::new(),
        }
    }

    async fn execute_hook(
        &self,
        plugin: &InstalledPluginRuntime,
        handler: &str,
        event: &str,
        payload: serde_json::Value,
    ) -> Result<HookOutput, AppError> {
        let command_path = plugin.path.join(handler);
        let output = self
            .execute_command_json(
                command_path.as_os_str(),
                &[],
                plugin.path.as_path(),
                DEFAULT_HOOK_TIMEOUT_MS,
                &HookInput { event, payload },
                None,
            )
            .await?;
        serde_json::from_slice::<HookOutput>(&output.stdout).map_err(|error| {
            AppError::recoverable(
                ErrorSource::Tool,
                "extensions.plugin.invalid_hook_output",
                format!(
                    "Hook '{}' from plugin '{}' returned invalid JSON: {error}",
                    handler, plugin.manifest.id
                ),
            )
        })
    }

    async fn execute_plugin_tool(
        &self,
        plugin: &InstalledPluginRuntime,
        tool: &PluginManifestTool,
        tool_input: &serde_json::Value,
        workspace_path: &str,
        thread_id: Option<&str>,
    ) -> Result<ToolOutput, AppError> {
        let timeout_ms = tool
            .timeout_ms
            .or(plugin.manifest.timeout_ms)
            .unwrap_or(DEFAULT_PLUGIN_TIMEOUT_MS)
            .min(300_000);

        let variables = build_plugin_variables(workspace_path, &plugin.path, thread_id);
        let args = tool
            .args
            .iter()
            .map(|arg| substitute_variables(arg, &variables))
            .collect::<Vec<_>>();
        let cwd = tool
            .cwd
            .as_deref()
            .map(|cwd| PathBuf::from(substitute_variables(cwd, &variables)))
            .unwrap_or_else(|| plugin.path.clone());
        let env = tool.env.as_ref().map(|env| {
            env.iter()
                .map(|(key, value)| (key.clone(), substitute_variables(value, &variables)))
                .collect::<Vec<_>>()
        });

        let output = self
            .execute_command_json(
                OsStr::new(&tool.command),
                &args,
                &cwd,
                timeout_ms,
                &PluginToolInput {
                    args: tool_input,
                    workspace: workspace_path,
                    thread_id,
                },
                env.as_deref(),
            )
            .await?;

        let parsed =
            serde_json::from_slice::<PluginToolOutput>(&output.stdout).map_err(|error| {
                AppError::recoverable(
                    ErrorSource::Tool,
                    "extensions.plugin.invalid_tool_output",
                    format!(
                        "Plugin tool '{}' from '{}' returned invalid JSON: {error}",
                        tool.name, plugin.manifest.id
                    ),
                )
            })?;

        Ok(ToolOutput {
            success: parsed.success,
            result: match (parsed.result, parsed.error) {
                (Some(result), _) => result,
                (None, Some(error)) => serde_json::json!({ "error": error }),
                (None, None) => serde_json::json!({ "ok": parsed.success }),
            },
        })
    }

    async fn execute_command_json<T: Serialize>(
        &self,
        program: &OsStr,
        args: &[String],
        cwd: &Path,
        timeout_ms: u64,
        stdin_payload: &T,
        env: Option<&[(String, String)]>,
    ) -> Result<std::process::Output, AppError> {
        let mut command = Command::new(program);
        command.args(args);
        command.current_dir(cwd);
        command.kill_on_drop(true);
        command.stdin(std::process::Stdio::piped());
        command.stdout(std::process::Stdio::piped());
        command.stderr(std::process::Stdio::piped());
        if let Some(env_pairs) = env {
            for (key, value) in env_pairs {
                command.env(key, value);
            }
        }

        let mut child = command.spawn().map_err(|error| {
            AppError::recoverable(
                ErrorSource::Tool,
                "extensions.command.spawn_failed",
                format!(
                    "Failed to start '{}': {error}",
                    Path::new(program).display()
                ),
            )
        })?;

        let payload = serde_json::to_vec(stdin_payload).map_err(|error| {
            AppError::internal(
                ErrorSource::Tool,
                format!("Failed to serialize command payload: {error}"),
            )
        })?;

        if let Some(mut stdin) = child.stdin.take() {
            tokio::spawn(async move {
                let _ = stdin.write_all(&payload).await;
            });
        }

        let wait = child.wait_with_output();
        let output = tokio::time::timeout(std::time::Duration::from_millis(timeout_ms), wait)
            .await
            .map_err(|_| {
                AppError::recoverable(
                    ErrorSource::Tool,
                    "extensions.command.timeout",
                    format!("Extension command timed out after {timeout_ms}ms"),
                )
            })??;

        if !output.status.success() {
            return Err(AppError::recoverable(
                ErrorSource::Tool,
                "extensions.command.non_zero_exit",
                String::from_utf8_lossy(&output.stderr).trim().to_string(),
            ));
        }

        Ok(output)
    }

    async fn execute_mcp_tool(
        &self,
        server_id: &str,
        tool: &McpToolSummaryDto,
        tool_input: &serde_json::Value,
        workspace_path: &str,
    ) -> Result<ToolOutput, AppError> {
        tracing::info!(server_id, tool = %tool.name, "MCP tool execution starting");
        tracing::debug!(server_id, tool = %tool.name, %tool_input, "MCP tool execution input");
        let config = self
            .load_mcp_configs_with_scope(Some(workspace_path), ConfigScope::Global)
            .await?
            .into_iter()
            .find(|(config, _)| config.id == server_id)
            .map(|(config, _)| config)
            .ok_or_else(|| {
                AppError::not_found(ErrorSource::Settings, format!("MCP server '{server_id}'"))
            })?;

        if !config.enabled {
            return Err(AppError::recoverable(
                ErrorSource::Tool,
                "extensions.mcp.disabled",
                format!("MCP server '{}' is disabled", config.label),
            ));
        }

        let result = self
            .call_mcp_tool_once(&config, &tool.name, tool_input, Some(workspace_path))
            .await?;

        let success = !result
            .get("isError")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false);
        tracing::info!(server_id, tool = %tool.name, success, "MCP tool execution completed");
        tracing::debug!(server_id, tool = %tool.name, %result, "MCP tool execution result");

        Ok(ToolOutput { success, result })
    }

    async fn probe_mcp_runtime(
        &self,
        config: &McpServerConfigInput,
    ) -> Result<McpRuntimeRecord, AppError> {
        match canonicalize_mcp_transport(&config.transport) {
            "stdio" => self.probe_stdio_mcp_runtime(config).await,
            "streamable-http" => self.probe_streamable_http_mcp_runtime(config).await,
            _ => Err(AppError::validation(
                ErrorSource::Settings,
                "Unsupported MCP transport",
            )),
        }
    }

    async fn probe_stdio_mcp_runtime(
        &self,
        config: &McpServerConfigInput,
    ) -> Result<McpRuntimeRecord, AppError> {
        let server_id = config.id.clone();
        let (tools, resources) = self
            .with_stdio_mcp_client(config, None, |stdin, stdout| {
                Box::pin(async move {
                    let init_result = initialize_mcp_session(stdin, stdout).await?;
                    let capabilities = init_result
                        .get("capabilities")
                        .cloned()
                        .unwrap_or(serde_json::Value::Null);
                    let tools = if mcp_capability_enabled(&capabilities, "tools") {
                        parse_mcp_tools(
                            &call_stdio_mcp_method(
                                stdin,
                                stdout,
                                2,
                                "tools/list",
                                serde_json::json!({}),
                            )
                            .await?,
                            &server_id,
                        )
                    } else {
                        Vec::new()
                    };
                    let resources = if mcp_capability_enabled(&capabilities, "resources")
                        || mcp_capability_enabled(&capabilities, "resourceTemplates")
                    {
                        parse_mcp_resources(
                            &call_stdio_mcp_method(
                                stdin,
                                stdout,
                                3,
                                "resources/list",
                                serde_json::json!({}),
                            )
                            .await?,
                        )
                    } else {
                        Vec::new()
                    };
                    Ok((tools, resources))
                })
            })
            .await?;

        Ok(McpRuntimeRecord {
            tools,
            resources,
            stale_snapshot: false,
            last_error: None,
            status: Some("connected".to_string()),
            phase: Some("ready".to_string()),
            updated_at: None,
        })
    }

    async fn probe_streamable_http_mcp_runtime(
        &self,
        config: &McpServerConfigInput,
    ) -> Result<McpRuntimeRecord, AppError> {
        let server_id = config.id.clone();
        let (session, init_result) = initialize_streamable_http_session(config).await?;
        let capabilities = init_result
            .get("capabilities")
            .cloned()
            .unwrap_or(serde_json::Value::Null);

        let tools = if mcp_capability_enabled(&capabilities, "tools") {
            call_streamable_http_mcp_method(
                config,
                &session,
                2,
                "tools/list",
                serde_json::json!({}),
            )
            .await
            .map(|result| parse_mcp_tools(&result, &server_id))
        } else {
            Ok(Vec::new())
        };
        let resources = if mcp_capability_enabled(&capabilities, "resources")
            || mcp_capability_enabled(&capabilities, "resourceTemplates")
        {
            call_streamable_http_mcp_method(
                config,
                &session,
                3,
                "resources/list",
                serde_json::json!({}),
            )
            .await
            .map(|result| parse_mcp_resources(&result))
        } else {
            Ok(Vec::new())
        };

        let result = match (tools, resources) {
            (Ok(tools), Ok(resources)) => Ok(McpRuntimeRecord {
                tools,
                resources,
                stale_snapshot: false,
                last_error: None,
                status: Some("connected".to_string()),
                phase: Some("ready".to_string()),
                updated_at: None,
            }),
            (Err(error), _) | (_, Err(error)) => Err(error),
        };

        close_streamable_http_session(config, &session).await;
        result
    }

    async fn call_streamable_http_mcp_tool_once(
        &self,
        config: &McpServerConfigInput,
        tool_name: &str,
        arguments: &serde_json::Value,
    ) -> Result<serde_json::Value, AppError> {
        let (session, _) = initialize_streamable_http_session(config).await?;
        let result = call_streamable_http_mcp_method(
            config,
            &session,
            4,
            "tools/call",
            serde_json::json!({
                "name": tool_name,
                "arguments": arguments,
            }),
        )
        .await;
        close_streamable_http_session(config, &session).await;
        result
    }

    async fn call_mcp_tool_once(
        &self,
        config: &McpServerConfigInput,
        tool_name: &str,
        arguments: &serde_json::Value,
        workspace_path: Option<&str>,
    ) -> Result<serde_json::Value, AppError> {
        match canonicalize_mcp_transport(&config.transport) {
            "stdio" => {
                let tool_name = tool_name.to_string();
                let arguments = arguments.clone();
                self.with_stdio_mcp_client(config, workspace_path, |stdin, stdout| {
                    Box::pin(async move {
                        initialize_mcp_session(stdin, stdout).await?;
                        call_stdio_mcp_method(
                            stdin,
                            stdout,
                            4,
                            "tools/call",
                            serde_json::json!({
                                "name": tool_name,
                                "arguments": arguments,
                            }),
                        )
                        .await
                    })
                })
                .await
            }
            "streamable-http" => {
                self.call_streamable_http_mcp_tool_once(config, tool_name, arguments)
                    .await
            }
            _ => Err(AppError::validation(
                ErrorSource::Settings,
                "Unsupported MCP transport",
            )),
        }
    }

    async fn with_stdio_mcp_client<T, F>(
        &self,
        config: &McpServerConfigInput,
        workspace_path: Option<&str>,
        session: F,
    ) -> Result<T, AppError>
    where
        F: for<'a> FnOnce(
            &'a mut tokio::process::ChildStdin,
            &'a mut BufReader<tokio::process::ChildStdout>,
        ) -> std::pin::Pin<
            Box<dyn std::future::Future<Output = Result<T, AppError>> + Send + 'a>,
        >,
    {
        tracing::info!(server = %config.label, command = ?config.command, "MCP stdio process spawning");
        let mut child = spawn_stdio_mcp_process(config, workspace_path).await?;
        tracing::info!(server = %config.label, pid = ?child.id(), "MCP stdio process spawned");
        let mut stdin = child.stdin.take().ok_or_else(|| {
            AppError::internal(
                ErrorSource::Tool,
                format!("MCP server '{}' did not expose stdin", config.label),
            )
        })?;
        let stdout = child.stdout.take().ok_or_else(|| {
            AppError::internal(
                ErrorSource::Tool,
                format!("MCP server '{}' did not expose stdout", config.label),
            )
        })?;
        let stderr = child.stderr.take().ok_or_else(|| {
            AppError::internal(
                ErrorSource::Tool,
                format!("MCP server '{}' did not expose stderr", config.label),
            )
        })?;
        let stderr_task = tokio::spawn(async move {
            let mut reader = BufReader::new(stderr);
            let mut output = String::new();
            let _ = reader.read_to_string(&mut output).await;
            output.trim().to_string()
        });

        let mut stdout = BufReader::new(stdout);
        let timeout_ms = config
            .timeout_ms
            .unwrap_or(DEFAULT_MCP_TIMEOUT_MS)
            .min(120_000);
        let result = tokio::time::timeout(
            std::time::Duration::from_millis(timeout_ms),
            session(&mut stdin, &mut stdout),
        )
        .await
        .map_err(|_| {
            tracing::warn!(server = %config.label, timeout_ms, "MCP stdio session timed out");
            AppError::recoverable(
                ErrorSource::Tool,
                "extensions.mcp.timeout",
                format!(
                    "MCP server '{}' timed out after {timeout_ms}ms",
                    config.label
                ),
            )
        })?;

        drop(stdin);
        let _ = child.kill().await;
        let _ = child.wait().await;
        let stderr_output = stderr_task.await.unwrap_or_default();
        if !stderr_output.is_empty() {
            tracing::debug!(server = %config.label, stderr = %stderr_output, "MCP stdio process stderr");
        }

        result.map_err(|error| append_mcp_stderr(error, &stderr_output))
    }

    async fn remove_mcp_runtime_records_with_prefix(&self, prefix: &str) -> Result<(), AppError> {
        let mut store = self.load_mcp_runtime_store().await?;
        let before = store.items.len();
        store.items.retain(|id, _| !id.starts_with(prefix));
        if before == store.items.len() {
            return Ok(());
        }
        self.save_mcp_runtime_store(&store).await
    }

    async fn update_mcp_enabled(
        &self,
        id: &str,
        enabled: bool,
        workspace_path: Option<&str>,
        scope: ConfigScope,
    ) -> Result<bool, AppError> {
        let mut configs = self
            .load_mcp_configs_for_scope(workspace_path, scope)
            .await?;
        if let Some(config) = configs.iter_mut().find(|config| config.id == id) {
            config.enabled = enabled;
            let target = config.clone();
            self.save_mcp_configs_for_scope(&configs, workspace_path, scope)
                .await?;
            let _ = self
                .refresh_mcp_runtime(&target, None, scope.as_str())
                .await?;
            return Ok(true);
        }

        if scope == ConfigScope::Workspace && workspace_path.is_some() {
            if let Some(mut config) = self
                .load_mcp_configs_for_scope(None, ConfigScope::Global)
                .await?
                .into_iter()
                .find(|config| config.id == id)
            {
                config.enabled = enabled;
                configs.push(config.clone());
                self.save_mcp_configs_for_scope(&configs, workspace_path, scope)
                    .await?;
                let _ = self
                    .refresh_mcp_runtime(&config, None, scope.as_str())
                    .await?;
                return Ok(true);
            }
        }

        Ok(false)
    }

    async fn update_skill_enabled(
        &self,
        id: &str,
        enabled: bool,
        workspace_path: Option<&str>,
        scope: ConfigScope,
    ) -> Result<bool, AppError> {
        if !self.skill_exists(id, workspace_path, scope).await? {
            return Ok(false);
        }
        self.set_skill_enabled(id, enabled, workspace_path, Some(scope))
            .await?;
        Ok(true)
    }
}

impl From<bool> for ExtensionInstallState {
    fn from(enabled: bool) -> Self {
        if enabled {
            Self::Enabled
        } else {
            Self::Disabled
        }
    }
}

fn display_config_path(path: &Path) -> String {
    if let Some(home) = dirs::home_dir() {
        if let Ok(relative) = path.strip_prefix(&home) {
            return format!("~/{}", relative.display());
        }

        if let Ok(canonical_home) = dunce::canonicalize(&home) {
            if let Ok(relative) = path.strip_prefix(&canonical_home) {
                return format!("~/{}", relative.display());
            }
        }
    }

    path.display().to_string()
}

fn tiy_home() -> PathBuf {
    dirs::home_dir()
        .expect("cannot resolve HOME directory")
        .join(".tiy")
}

fn agents_home() -> PathBuf {
    dirs::home_dir()
        .expect("cannot resolve HOME directory")
        .join(".agents")
}

fn global_mcp_path() -> PathBuf {
    tiy_home().join("mcp.json")
}

fn workspace_mcp_path(workspace_path: Option<&str>) -> Result<PathBuf, AppError> {
    let workspace_path = workspace_path.ok_or_else(|| {
        AppError::validation(
            ErrorSource::Settings,
            "workspace path is required for workspace-scoped MCP config",
        )
    })?;
    Ok(PathBuf::from(workspace_path).join(".tiy/mcp.local.json"))
}

fn legacy_workspace_mcp_path(workspace_path: Option<&str>) -> Result<PathBuf, AppError> {
    let workspace_path = workspace_path.ok_or_else(|| {
        AppError::validation(
            ErrorSource::Settings,
            "workspace path is required for workspace-scoped MCP config",
        )
    })?;
    Ok(PathBuf::from(workspace_path).join(".tiy/mcp.json"))
}

fn workspace_mcp_read_path(workspace_path: Option<&str>) -> Result<PathBuf, AppError> {
    let path = workspace_mcp_path(workspace_path)?;
    if path.exists() {
        return Ok(path);
    }
    let legacy_path = legacy_workspace_mcp_path(workspace_path)?;
    if legacy_path.exists() {
        return Ok(legacy_path);
    }
    Ok(path)
}

fn global_skills_config_path() -> PathBuf {
    tiy_home().join("skills.json")
}

fn workspace_skills_config_path(workspace_path: Option<&str>) -> Result<PathBuf, AppError> {
    let workspace_path = workspace_path.ok_or_else(|| {
        AppError::validation(
            ErrorSource::Settings,
            "workspace path is required for workspace-scoped skill config",
        )
    })?;
    Ok(PathBuf::from(workspace_path).join(".tiy/skills.local.json"))
}

fn legacy_workspace_skills_config_path(workspace_path: Option<&str>) -> Result<PathBuf, AppError> {
    let workspace_path = workspace_path.ok_or_else(|| {
        AppError::validation(
            ErrorSource::Settings,
            "workspace path is required for workspace-scoped skill config",
        )
    })?;
    Ok(PathBuf::from(workspace_path).join(".tiy/skills.json"))
}

fn workspace_skills_read_path(workspace_path: Option<&str>) -> Result<PathBuf, AppError> {
    let path = workspace_skills_config_path(workspace_path)?;
    if path.exists() {
        return Ok(path);
    }
    let legacy_path = legacy_workspace_skills_config_path(workspace_path)?;
    if legacy_path.exists() {
        return Ok(legacy_path);
    }
    Ok(path)
}

fn global_marketplace_sources_path() -> PathBuf {
    tiy_home().join("marketplaces.json")
}

fn global_plugins_config_path() -> PathBuf {
    tiy_home().join(EXTENSIONS_PLUGINS_FILE_NAME)
}

fn marketplace_cache_root() -> PathBuf {
    tiy_home().join("catalog/marketplaces")
}

fn config_diagnostic_id(path: &Path, area: &str, scope: ConfigScope) -> String {
    format!("{}:{}:{}", scope.as_str(), area, path.display())
}

fn builtin_marketplace_sources() -> Vec<MarketplaceSourceRecord> {
    vec![MarketplaceSourceRecord {
        id: marketplace_source_id(BUILTIN_MARKETPLACE_ANTHROPIC_URL),
        name: BUILTIN_MARKETPLACE_ANTHROPIC_NAME.to_string(),
        url: BUILTIN_MARKETPLACE_ANTHROPIC_URL.to_string(),
        kind: DEFAULT_MARKETPLACE_SOURCE_KIND.to_string(),
        last_synced_at: None,
        last_error: None,
    }]
}

fn is_builtin_marketplace_source_id(id: &str) -> bool {
    builtin_marketplace_sources()
        .iter()
        .any(|source| source.id == id)
}

fn marketplace_source_id(url: &str) -> String {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    url.hash(&mut hasher);
    format!("{:x}", hasher.finish())
}

fn marketplace_cache_plugin_count(source: &MarketplaceSourceRecord) -> usize {
    let cache_dir = marketplace_cache_root().join(&source.id);
    ["plugins", "external_plugins"]
        .into_iter()
        .map(|root_name| cache_dir.join(root_name))
        .filter(|root| root.is_dir())
        .map(|root| {
            read_child_dirs(&root)
                .unwrap_or_default()
                .into_iter()
                .filter(|plugin_dir| plugin_dir.join(".claude-plugin/plugin.json").is_file())
                .count()
        })
        .sum()
}

fn read_child_dirs(path: &Path) -> Result<Vec<PathBuf>, AppError> {
    let mut dirs = Vec::new();
    for entry in fs::read_dir(path)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            dirs.push(path);
        }
    }
    Ok(dirs)
}

fn parse_plugin_manifest(raw: &str, plugin_dir: &Path) -> Result<PluginManifest, String> {
    let value = serde_json::from_str::<serde_json::Value>(raw)
        .map_err(|error| format!("manifest is not valid JSON: {error}"))?;
    let object = value
        .as_object()
        .ok_or_else(|| "manifest root must be a JSON object".to_string())?;
    let fallback_name = plugin_dir
        .file_name()
        .and_then(OsStr::to_str)
        .filter(|name| !name.is_empty())
        .unwrap_or("plugin")
        .to_string();

    let commands = object
        .get("commands")
        .and_then(serde_json::Value::as_array)
        .map(|entries| {
            entries
                .iter()
                .enumerate()
                .filter_map(|(index, entry)| {
                    let command = entry.as_object()?;
                    let name = read_string_keys(command, &["name", "id", "label"])
                        .unwrap_or_else(|| format!("command-{}", index + 1));
                    Some(PluginManifestCommand {
                        description: read_string_keys(command, &["description"])
                            .unwrap_or_else(|| name.clone()),
                        name,
                        prompt_template: read_string_keys(
                            command,
                            &["promptTemplate", "prompt_template"],
                        ),
                    })
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    let tools = object
        .get("tools")
        .and_then(serde_json::Value::as_array)
        .map(|entries| {
            entries
                .iter()
                .enumerate()
                .filter_map(|(index, entry)| {
                    let tool = entry.as_object()?;
                    let command = read_string_keys(tool, &["command", "cmd"])?;
                    let name = read_string_keys(tool, &["name", "id", "label"])
                        .unwrap_or_else(|| format!("tool-{}", index + 1));
                    Some(PluginManifestTool {
                        name: name.clone(),
                        description: read_string_keys(tool, &["description"])
                            .unwrap_or_else(|| name.clone()),
                        command,
                        args: read_string_array_keys(tool, &["args"]),
                        env: read_string_map_keys(tool, &["env"]),
                        cwd: read_string_keys(tool, &["cwd"]),
                        timeout_ms: read_u64_keys(tool, &["timeoutMs", "timeout_ms"]),
                        required_permission: read_string_keys(
                            tool,
                            &["requiredPermission", "required_permission"],
                        )
                        .unwrap_or_else(|| "read".to_string()),
                    })
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    Ok(PluginManifest {
        id: read_string_keys(object, &["id"]).unwrap_or_else(|| fallback_name.clone()),
        name: read_string_keys(object, &["name", "title"]).unwrap_or_else(|| fallback_name.clone()),
        version: read_string_keys(object, &["version"]).unwrap_or_else(|| "0.0.0".to_string()),
        description: read_string_keys(object, &["description", "summary"]),
        author: read_author_name(object.get("author")),
        homepage: read_string_keys(object, &["homepage", "repository", "url"]),
        default_enabled: read_bool_keys(object, &["defaultEnabled", "default_enabled"]),
        capabilities: read_string_array_keys(object, &["capabilities"]),
        permissions: read_string_array_keys(object, &["permissions"]),
        hooks: read_plugin_hooks(object.get("hooks")),
        tools,
        commands,
        timeout_ms: read_u64_keys(object, &["timeoutMs", "timeout_ms"]),
        skills_dir: read_string_keys(object, &["skillsDir", "skills_dir"]),
        config_schema: read_config_schema(
            object
                .get("configSchema")
                .or_else(|| object.get("config_schema")),
        ),
    })
}

fn parse_skill_markdown(
    raw: &str,
    skill_dir: &Path,
    source: &str,
) -> Option<(SkillRecordDto, String)> {
    let (frontmatter, body) = split_frontmatter(raw)?;
    let meta = parse_frontmatter_map(frontmatter);
    let base_id = meta.get("id").cloned().or_else(|| {
        skill_dir
            .file_name()
            .and_then(OsStr::to_str)
            .map(str::to_string)
    })?;
    let name = meta.get("name").cloned().unwrap_or_else(|| base_id.clone());
    let description = meta.get("description").cloned();
    let tags = parse_array_field(meta.get("tags"));
    let triggers = parse_array_field(meta.get("triggers"));
    let tools = parse_array_field(meta.get("tools"));
    let priority = meta.get("priority").cloned();
    let trimmed_body = body.trim();
    let preview = trimmed_body.chars().take(320).collect::<String>();

    let namespaced_id = if source == "builtin" {
        base_id
    } else {
        format!("{source}:{base_id}")
    };

    Some((
        SkillRecordDto {
            id: namespaced_id,
            name,
            description,
            tags,
            triggers,
            tools,
            priority,
            source: source.to_string(),
            path: skill_dir.to_string_lossy().to_string(),
            enabled: true,
            scope: "global".to_string(),
            content_preview: preview.clone(),
            prompt_budget_chars: preview.len(),
        },
        raw.to_string(),
    ))
}

fn parse_plugin_command_markdown(raw: &str, fallback_name: &str) -> Option<PluginManifestCommand> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }

    let (description, prompt_template) = if let Some((frontmatter, body)) = split_frontmatter(raw) {
        let meta = parse_frontmatter_map(frontmatter);
        let description = meta
            .get("description")
            .cloned()
            .unwrap_or_else(|| format!("Plugin command /{fallback_name}"));
        let prompt = body.trim().to_string();
        (description, prompt)
    } else {
        (
            format!("Plugin command /{fallback_name}"),
            trimmed.to_string(),
        )
    };

    if prompt_template.is_empty() {
        return None;
    }

    Some(PluginManifestCommand {
        name: fallback_name.to_string(),
        description,
        prompt_template: Some(prompt_template),
    })
}

fn update_named_membership(values: &mut Vec<String>, id: &str, enabled: bool) {
    values.retain(|value| value != id);
    if enabled {
        values.push(id.to_string());
        values.sort();
    }
}

fn compare_extension_summaries(
    left: &ExtensionSummaryDto,
    right: &ExtensionSummaryDto,
) -> Ordering {
    let left_enabled = matches!(left.install_state, ExtensionInstallState::Enabled);
    let right_enabled = matches!(right.install_state, ExtensionInstallState::Enabled);
    right_enabled
        .cmp(&left_enabled)
        .then_with(|| {
            let left_installed = !matches!(left.install_state, ExtensionInstallState::Discovered);
            let right_installed = !matches!(right.install_state, ExtensionInstallState::Discovered);
            right_installed.cmp(&left_installed)
        })
        .then_with(|| left.name.to_lowercase().cmp(&right.name.to_lowercase()))
        .then_with(|| left.id.cmp(&right.id))
}

fn compare_mcp_server_states(left: &McpServerStateDto, right: &McpServerStateDto) -> Ordering {
    right
        .config
        .enabled
        .cmp(&left.config.enabled)
        .then_with(|| left.label.to_lowercase().cmp(&right.label.to_lowercase()))
        .then_with(|| left.id.cmp(&right.id))
}

fn compare_skill_records(left: &SkillRecordDto, right: &SkillRecordDto) -> Ordering {
    right
        .enabled
        .cmp(&left.enabled)
        .then_with(|| left.name.to_lowercase().cmp(&right.name.to_lowercase()))
        .then_with(|| left.id.cmp(&right.id))
}

fn compare_marketplace_items(left: &MarketplaceItemDto, right: &MarketplaceItemDto) -> Ordering {
    right
        .enabled
        .cmp(&left.enabled)
        .then_with(|| right.installed.cmp(&left.installed))
        .then_with(|| left.name.to_lowercase().cmp(&right.name.to_lowercase()))
        .then_with(|| left.id.cmp(&right.id))
}

fn apply_skill_state(record: &mut SkillRecordDto, state: &SkillStateStore) {
    if state.disabled.iter().any(|value| value == &record.id) {
        record.enabled = false;
    }
    if state.enabled.iter().any(|value| value == &record.id) {
        record.enabled = true;
    }
}

fn split_frontmatter(raw: &str) -> Option<(&str, &str)> {
    let trimmed = raw.trim_start();
    let rest = trimmed.strip_prefix("---")?;
    let rest = rest
        .strip_prefix('\n')
        .or_else(|| rest.strip_prefix("\r\n"))?;
    let end = rest.find("\n---").or_else(|| rest.find("\r\n---"))?;
    let (frontmatter, body_with_sep) = rest.split_at(end);
    let body = body_with_sep
        .strip_prefix("\n---\n")
        .or_else(|| body_with_sep.strip_prefix("\r\n---\r\n"))
        .or_else(|| body_with_sep.strip_prefix("\n---"))
        .unwrap_or_default();
    Some((frontmatter, body))
}

fn parse_frontmatter_map(frontmatter: &str) -> BTreeMap<String, String> {
    let mut values = BTreeMap::new();
    let lines = frontmatter.lines().collect::<Vec<_>>();
    let mut index = 0usize;

    while index < lines.len() {
        let raw_line = lines[index];
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') {
            index += 1;
            continue;
        }
        if raw_line.starts_with(' ') || raw_line.starts_with('\t') {
            index += 1;
            continue;
        }

        let Some((key, value)) = line.split_once(':') else {
            index += 1;
            continue;
        };
        let key = key.trim().to_string();
        let value = value.trim();

        if matches!(value, ">" | ">-" | ">+" | "|" | "|-" | "|+") {
            let folded = value.starts_with('>');
            index += 1;
            let mut block_lines = Vec::new();
            while index < lines.len() {
                let next_line = lines[index];
                if next_line.starts_with(' ') || next_line.starts_with('\t') {
                    block_lines.push(next_line.trim().to_string());
                    index += 1;
                    continue;
                }
                if next_line.trim().is_empty() {
                    block_lines.push(String::new());
                    index += 1;
                    continue;
                }
                break;
            }

            let parsed = if folded {
                fold_yaml_block_scalar(&block_lines)
            } else {
                block_lines.join("\n").trim().to_string()
            };
            values.insert(key, parsed);
            continue;
        }

        if value.is_empty() {
            index += 1;
            let mut list_items = Vec::new();
            while index < lines.len() {
                let next_line = lines[index];
                let trimmed = next_line.trim();
                if trimmed.is_empty() {
                    index += 1;
                    continue;
                }
                if next_line.starts_with(' ') || next_line.starts_with('\t') {
                    if let Some(item) = trimmed.strip_prefix("- ") {
                        list_items.push(trim_yaml_scalar(item));
                        index += 1;
                        continue;
                    }
                }
                break;
            }

            values.insert(key, list_items.join("\n"));
            continue;
        }

        values.insert(key, trim_yaml_scalar(value));
        index += 1;
    }
    values
}

fn parse_array_field(value: Option<&String>) -> Vec<String> {
    let Some(value) = value else {
        return Vec::new();
    };
    let trimmed = value.trim();
    if !(trimmed.starts_with('[') && trimmed.ends_with(']')) {
        if trimmed.contains('\n') {
            return trimmed
                .lines()
                .map(str::trim)
                .filter(|item| !item.is_empty())
                .map(trim_yaml_scalar)
                .collect();
        }
        return if trimmed.is_empty() {
            Vec::new()
        } else {
            vec![trim_yaml_scalar(trimmed)]
        };
    }
    trimmed[1..trimmed.len() - 1]
        .split(',')
        .map(str::trim)
        .filter(|item| !item.is_empty())
        .map(trim_yaml_scalar)
        .collect()
}

fn trim_yaml_scalar(value: &str) -> String {
    value
        .trim()
        .trim_matches('"')
        .trim_matches('\'')
        .to_string()
}

fn fold_yaml_block_scalar(lines: &[String]) -> String {
    let mut result = String::new();

    for line in lines {
        if line.is_empty() {
            if !result.ends_with("\n\n") {
                if result.ends_with(' ') {
                    result.pop();
                }
                result.push_str("\n\n");
            }
            continue;
        }

        if result.is_empty() || result.ends_with("\n\n") {
            result.push_str(line);
        } else {
            result.push(' ');
            result.push_str(line);
        }
    }

    result.trim().to_string()
}

fn read_named_value(value: &serde_json::Value) -> Option<String> {
    value
        .get("label")
        .and_then(serde_json::Value::as_str)
        .or_else(|| value.get("name").and_then(serde_json::Value::as_str))
        .or_else(|| value.get("id").and_then(serde_json::Value::as_str))
        .map(str::to_string)
}

fn plugin_managed_mcp_prefix(plugin_id: &str) -> String {
    format!("plugin::{plugin_id}::")
}

fn plugin_managed_mcp_id(plugin_id: &str, server_name: &str) -> String {
    format!("{}{}", plugin_managed_mcp_prefix(plugin_id), server_name)
}

fn read_plugin_mcp_server_entries<'a>(
    value: &'a serde_json::Value,
) -> Vec<(String, &'a serde_json::Map<String, serde_json::Value>)> {
    let mut entries = Vec::new();
    match value.get("servers") {
        Some(serde_json::Value::Object(map)) => {
            for (server_name, spec) in map {
                if let Some(spec) = spec.as_object() {
                    entries.push((server_name.clone(), spec));
                }
            }
        }
        Some(serde_json::Value::Array(items)) => {
            for (index, item) in items.iter().enumerate() {
                let Some(spec) = item.as_object() else {
                    continue;
                };
                let server_name = read_string_keys(spec, &["id", "name", "label"])
                    .unwrap_or_else(|| format!("server-{}", index + 1));
                entries.push((server_name, spec));
            }
        }
        _ => {
            if let Some(map) = value.as_object() {
                for (server_name, spec) in map {
                    if let Some(spec) = spec.as_object() {
                        entries.push((server_name.clone(), spec));
                    }
                }
            }
        }
    }
    entries
}

fn build_plugin_managed_mcp_config(
    manifest: &PluginManifest,
    server_name: String,
    spec: &serde_json::Map<String, serde_json::Value>,
) -> Option<McpServerConfigInput> {
    let raw_transport = read_string_keys(spec, &["transport", "type"]);
    let has_url = read_string_keys(spec, &["url"]).is_some();
    let has_command = read_string_keys(spec, &["command", "cmd"]).is_some();
    let transport = match raw_transport.as_deref() {
        Some("streamable-http") | Some("http") | Some("https") => "streamable-http".to_string(),
        Some("stdio") => "stdio".to_string(),
        Some(other) if other.contains("http") => "streamable-http".to_string(),
        _ if has_url => "streamable-http".to_string(),
        _ if has_command => "stdio".to_string(),
        _ => return None,
    };
    let label = read_string_keys(spec, &["label", "name"])
        .unwrap_or_else(|| build_plugin_managed_mcp_label(&manifest.name, &server_name));
    Some(McpServerConfigInput {
        id: plugin_managed_mcp_id(&manifest.id, &server_name),
        label,
        transport,
        enabled: true,
        auto_start: true,
        command: read_string_keys(spec, &["command", "cmd"]),
        args: Some(read_string_array_keys(spec, &["args"])),
        env: read_string_map_keys(spec, &["env"]),
        cwd: read_string_keys(spec, &["cwd"]),
        url: read_string_keys(spec, &["url"]),
        headers: read_string_map_keys(spec, &["headers"]),
        timeout_ms: read_u64_keys(spec, &["timeoutMs", "timeout_ms"]),
    })
}

fn build_plugin_managed_mcp_label(plugin_name: &str, server_name: &str) -> String {
    let plugin_name = plugin_name.trim();
    let server_name = server_name.trim();

    if plugin_name.is_empty() {
        return server_name.to_string();
    }
    if server_name.is_empty() {
        return plugin_name.to_string();
    }
    if plugin_managed_mcp_names_match(plugin_name, server_name) {
        return plugin_name.to_string();
    }

    format!("{plugin_name} / {server_name}")
}

fn plugin_managed_mcp_names_match(left: &str, right: &str) -> bool {
    normalize_plugin_managed_mcp_name(left) == normalize_plugin_managed_mcp_name(right)
}

fn normalize_plugin_managed_mcp_name(value: &str) -> String {
    value
        .chars()
        .filter(|char| char.is_ascii_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

fn merge_plugin_managed_mcp_config(
    existing: Option<&McpServerConfigInput>,
    managed: McpServerConfigInput,
) -> McpServerConfigInput {
    let Some(existing) = existing else {
        return managed;
    };

    McpServerConfigInput {
        enabled: existing.enabled,
        auto_start: existing.auto_start,
        env: existing.env.clone().or(managed.env.clone()),
        cwd: existing.cwd.clone().or(managed.cwd.clone()),
        headers: existing.headers.clone().or(managed.headers.clone()),
        timeout_ms: existing.timeout_ms.or(managed.timeout_ms),
        ..managed
    }
}

fn read_string_keys(
    object: &serde_json::Map<String, serde_json::Value>,
    keys: &[&str],
) -> Option<String> {
    keys.iter()
        .filter_map(|key| object.get(*key))
        .find_map(read_string_value)
}

fn read_string_value(value: &serde_json::Value) -> Option<String> {
    value.as_str().map(str::trim).and_then(|value| {
        if value.is_empty() {
            None
        } else {
            Some(value.to_string())
        }
    })
}

fn read_string_array_keys(
    object: &serde_json::Map<String, serde_json::Value>,
    keys: &[&str],
) -> Vec<String> {
    let mut items = keys
        .iter()
        .filter_map(|key| object.get(*key))
        .find_map(|value| match value {
            serde_json::Value::Array(entries) => Some(
                entries
                    .iter()
                    .filter_map(read_string_value)
                    .collect::<Vec<_>>(),
            ),
            _ => None,
        })
        .unwrap_or_default();
    items.sort();
    items.dedup();
    items
}

fn read_string_map_keys(
    object: &serde_json::Map<String, serde_json::Value>,
    keys: &[&str],
) -> Option<HashMap<String, String>> {
    let map = keys
        .iter()
        .filter_map(|key| object.get(*key))
        .find_map(|value| match value {
            serde_json::Value::Object(map) => Some(
                map.iter()
                    .filter_map(|(key, value)| {
                        read_string_value(value).map(|value| (key.clone(), value))
                    })
                    .collect::<HashMap<_, _>>(),
            ),
            _ => None,
        })?;
    if map.is_empty() {
        None
    } else {
        Some(map)
    }
}

fn read_bool_keys(
    object: &serde_json::Map<String, serde_json::Value>,
    keys: &[&str],
) -> Option<bool> {
    keys.iter()
        .filter_map(|key| object.get(*key))
        .find_map(serde_json::Value::as_bool)
}

fn read_u64_keys(
    object: &serde_json::Map<String, serde_json::Value>,
    keys: &[&str],
) -> Option<u64> {
    keys.iter()
        .filter_map(|key| object.get(*key))
        .find_map(serde_json::Value::as_u64)
}

fn read_author_name(value: Option<&serde_json::Value>) -> Option<String> {
    let value = value?;
    read_string_value(value).or_else(|| {
        value
            .as_object()
            .and_then(|object| read_string_keys(object, &["name", "label", "id"]))
    })
}

fn read_plugin_hooks(value: Option<&serde_json::Value>) -> Option<PluginManifestHooks> {
    let object = value?.as_object()?;
    let pre_tool_use = read_optional_string_array_keys(object, &["preToolUse", "pre_tool_use"]);
    let post_tool_use = read_optional_string_array_keys(object, &["postToolUse", "post_tool_use"]);
    let on_run_start = read_optional_string_array_keys(object, &["onRunStart", "on_run_start"]);
    let on_run_complete =
        read_optional_string_array_keys(object, &["onRunComplete", "on_run_complete"]);
    let hooks = PluginManifestHooks {
        pre_tool_use,
        post_tool_use,
        on_run_start,
        on_run_complete,
    };
    if hooks.pre_tool_use.is_none()
        && hooks.post_tool_use.is_none()
        && hooks.on_run_start.is_none()
        && hooks.on_run_complete.is_none()
    {
        None
    } else {
        Some(hooks)
    }
}

fn read_optional_string_array_keys(
    object: &serde_json::Map<String, serde_json::Value>,
    keys: &[&str],
) -> Option<Vec<String>> {
    let items = read_string_array_keys(object, keys);
    if items.is_empty() {
        None
    } else {
        Some(items)
    }
}

fn read_config_schema(value: Option<&serde_json::Value>) -> Option<PluginManifestSchema> {
    let object = value?.as_object()?;
    let path = read_string_keys(object, &["path"])?;
    Some(PluginManifestSchema {
        r#type: read_string_keys(object, &["type"]).unwrap_or_else(|| "json-schema".to_string()),
        path,
    })
}

fn canonicalize_mcp_transport(transport: &str) -> &'static str {
    match transport.trim().to_ascii_lowercase().as_str() {
        "http-streamable" | "streamable-http" => "streamable-http",
        "stdio" => "stdio",
        _ => "",
    }
}

fn canonicalize_mcp_config(mut config: McpServerConfigInput) -> McpServerConfigInput {
    let transport = canonicalize_mcp_transport(&config.transport);
    if !transport.is_empty() {
        config.transport = transport.to_string();
    }
    config
}

fn merge_masked_string_map(
    existing: Option<&HashMap<String, String>>,
    incoming: Option<HashMap<String, String>>,
) -> Option<HashMap<String, String>> {
    let Some(mut merged) = incoming else {
        return None;
    };
    if let Some(existing) = existing {
        for (key, existing_value) in existing {
            let Some(candidate) = merged.get_mut(key) else {
                continue;
            };
            if candidate == &mask_sensitive_value(existing_value) {
                *candidate = existing_value.clone();
            }
        }
    }
    Some(merged)
}

fn merge_mcp_sensitive_fields(
    existing: &McpServerConfigInput,
    mut incoming: McpServerConfigInput,
) -> McpServerConfigInput {
    if let (Some(existing_url), Some(candidate_url)) = (&existing.url, &incoming.url) {
        if candidate_url == &mask_url(existing_url.clone()) {
            incoming.url = Some(existing_url.clone());
        }
    }
    incoming.env = merge_masked_string_map(existing.env.as_ref(), incoming.env);
    incoming.headers = merge_masked_string_map(existing.headers.as_ref(), incoming.headers);
    incoming
}

fn build_streamable_http_client(
    config: &McpServerConfigInput,
) -> Result<reqwest::Client, AppError> {
    let timeout_ms = config
        .timeout_ms
        .unwrap_or(DEFAULT_MCP_TIMEOUT_MS)
        .min(MAX_MCP_TIMEOUT_MS);
    reqwest::Client::builder()
        .timeout(Duration::from_millis(timeout_ms))
        .build()
        .map_err(|error| {
            AppError::internal(
                ErrorSource::Tool,
                format!(
                    "Failed to build HTTP client for MCP server '{}': {error}",
                    config.label
                ),
            )
        })
}

/// Returns the cached environment from the user's login shell.
///
/// On macOS (and Linux), GUI apps do not inherit environment variables set in
/// `.zshrc`, `.bashrc`, or tools like nvm/fnm/pyenv. This function runs the
/// user's login shell once to capture the full environment and caches the result
/// for the lifetime of the process.
fn login_shell_env() -> &'static std::collections::HashMap<String, String> {
    use std::collections::HashMap;
    use std::sync::OnceLock;

    static CACHE: OnceLock<HashMap<String, String>> = OnceLock::new();
    CACHE.get_or_init(|| {
        #[cfg(target_os = "windows")]
        {
            HashMap::new()
        }
        #[cfg(not(target_os = "windows"))]
        {
            use crate::core::shell_runtime::current_shell;
            use std::time::Duration;

            const TIMEOUT: Duration = Duration::from_millis(3000);

            let shell = current_shell();
            let mut cmd = std::process::Command::new(&shell);
            cmd.args(["-l", "-c", "env"]);
            cmd.stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::null())
                .stdin(std::process::Stdio::null());

            // Use a blocking spawn + wait with timeout.  This runs once during
            // the first MCP connection attempt, so a short block is acceptable.
            let result: Option<HashMap<String, String>> = (|| {
                let mut child = cmd.spawn().ok()?;
                let mut stdout = child.stdout.take()?;
                let (tx, rx) = std::sync::mpsc::channel();
                let reader_handle = std::thread::spawn(move || {
                    use std::io::Read;
                    let mut buf = Vec::new();
                    let read_result = stdout.read_to_end(&mut buf);
                    let _ = tx.send(read_result.map(|_| buf));
                });
                let stdout_bytes = match rx.recv_timeout(TIMEOUT) {
                    Ok(Ok(bytes)) => bytes,
                    _ => {
                        // Timeout or read error — kill the child to prevent resource leaks
                        let _ = child.kill();
                        let _ = child.wait();
                        // Join the reader thread so it doesn't outlive the child process
                        let _ = reader_handle.join();
                        return None;
                    }
                };
                let status = child.wait().ok()?;
                if !status.success() {
                    return None;
                }
                let stdout = String::from_utf8_lossy(&stdout_bytes);
                let mut map = HashMap::new();
                for line in stdout.lines() {
                    if let Some((key, value)) = line.split_once('=') {
                        if !key.is_empty()
                            && key.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
                        {
                            map.insert(key.to_string(), value.to_string());
                        }
                    }
                }
                Some(map)
            })();

            let env = result.unwrap_or_default();
            if env.is_empty() {
                tracing::warn!("login_shell_env: captured 0 env vars from login shell");
            } else {
                let mut keys: Vec<&str> = env.keys().map(|k| k.as_str()).collect();
                keys.sort_unstable();
                tracing::info!(
                    count = env.len(),
                    keys = %keys.join(", "),
                    "login_shell_env: captured env vars from login shell"
                );
                // Log PATH separately since it is the most important for tool resolution
                if let Some(path) = env.get("PATH") {
                    tracing::debug!(PATH = %path, "login_shell_env: captured PATH");
                }
            }
            env
        }
    })
}

/// Resolves an environment variable by name, first checking the process
/// environment, then falling back to the cached login shell environment.
fn resolve_env_var(name: &str) -> Option<String> {
    let from_process = std::env::var(name).ok();
    if let Some(ref val) = from_process {
        tracing::debug!(var = %name, source = "process", "resolve_env_var: resolved");
        return Some(val.clone());
    }
    let from_login = login_shell_env().get(name).cloned();
    if from_login.is_some() {
        tracing::debug!(var = %name, source = "login_shell", "resolve_env_var: resolved");
    } else {
        tracing::debug!(var = %name, "resolve_env_var: not found in process or login shell");
    }
    from_login
}

/// Expands `${VAR}` and `$VAR` patterns in a string using the current process
/// environment, falling back to the user's login shell environment for variables
/// not present in the process env (common on macOS GUI apps).
/// Unresolved variables are left as-is so the user sees what failed.
fn expand_env_vars(input: &str) -> String {
    let mut result = String::with_capacity(input.len());
    let mut chars = input.chars().peekable();

    while let Some(ch) = chars.next() {
        if ch == '$' {
            let braced = chars.peek() == Some(&'{');
            if braced {
                chars.next(); // consume '{'
            }
            let mut var_name = String::new();
            while let Some(&c) = chars.peek() {
                if braced {
                    if c == '}' {
                        chars.next(); // consume '}'
                        break;
                    }
                    var_name.push(c);
                    chars.next();
                } else if c.is_ascii_alphanumeric() || c == '_' {
                    var_name.push(c);
                    chars.next();
                } else {
                    break;
                }
            }
            if var_name.is_empty() {
                result.push('$');
                if braced {
                    result.push('{');
                    result.push('}');
                }
            } else if let Some(val) = resolve_env_var(&var_name) {
                result.push_str(&val);
            } else {
                // Leave unresolved variable as-is for debuggability
                if braced {
                    result.push_str(&format!("${{{}}}", var_name));
                } else {
                    result.push('$');
                    result.push_str(&var_name);
                }
            }
        } else {
            result.push(ch);
        }
    }

    result
}

fn build_streamable_http_headers(
    config: &McpServerConfigInput,
    session: Option<&StreamableHttpSession>,
) -> Result<HeaderMap, AppError> {
    let mut headers = HeaderMap::new();
    headers.insert(
        ACCEPT,
        HeaderValue::from_static("application/json, text/event-stream"),
    );
    headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));

    let protocol_version = session
        .map(|session| session.protocol_version.as_str())
        .unwrap_or(MCP_PROTOCOL_VERSION);
    headers.insert(
        HeaderName::from_static("mcp-protocol-version"),
        HeaderValue::from_str(protocol_version).map_err(|error| {
            AppError::validation(
                ErrorSource::Settings,
                format!("Invalid MCP protocol version header: {error}"),
            )
        })?,
    );

    if let Some(session_id) = session.and_then(|session| session.session_id.as_deref()) {
        headers.insert(
            HeaderName::from_static("mcp-session-id"),
            HeaderValue::from_str(session_id).map_err(|error| {
                AppError::validation(
                    ErrorSource::Settings,
                    format!("Invalid MCP session header: {error}"),
                )
            })?,
        );
    }

    if let Some(custom_headers) = &config.headers {
        let header_keys: Vec<&str> = custom_headers.keys().map(|k| k.as_str()).collect();
        tracing::info!(
            server = %config.label,
            count = custom_headers.len(),
            keys = %header_keys.join(", "),
            "build_streamable_http_headers: injecting custom headers"
        );
        for (key, value) in custom_headers {
            let name = HeaderName::from_bytes(key.trim().as_bytes()).map_err(|error| {
                AppError::validation(
                    ErrorSource::Settings,
                    format!("Invalid MCP header name '{key}': {error}"),
                )
            })?;
            let expanded = expand_env_vars(value);
            let value = HeaderValue::from_str(&expanded).map_err(|error| {
                AppError::validation(
                    ErrorSource::Settings,
                    format!("Invalid MCP header value for '{key}': {error}"),
                )
            })?;
            headers.insert(name, value);
        }
    }

    Ok(headers)
}

async fn initialize_streamable_http_session(
    config: &McpServerConfigInput,
) -> Result<(StreamableHttpSession, serde_json::Value), AppError> {
    tracing::info!(server = %config.label, url = ?config.url, "MCP HTTP session initializing");
    let response = send_streamable_http_jsonrpc_request(
        config,
        None,
        &serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {
                "protocolVersion": MCP_PROTOCOL_VERSION,
                "capabilities": {},
                "clientInfo": {
                    "name": "tiycode",
                    "version": env!("CARGO_PKG_VERSION"),
                }
            }
        }),
    )
    .await?;
    let init_result = extract_streamable_http_jsonrpc_result(&response.body, 1, "initialize")?;
    let session = StreamableHttpSession {
        protocol_version: init_result
            .get("protocolVersion")
            .and_then(serde_json::Value::as_str)
            .filter(|value| !value.trim().is_empty())
            .unwrap_or(MCP_PROTOCOL_VERSION)
            .to_string(),
        session_id: response.session_id,
    };

    send_streamable_http_notification(
        config,
        &session,
        "notifications/initialized",
        serde_json::json!({}),
    )
    .await?;

    tracing::info!(server = %config.label, session_id = ?session.session_id, "MCP HTTP session initialized successfully");
    Ok((session, init_result))
}

async fn call_streamable_http_mcp_method(
    config: &McpServerConfigInput,
    session: &StreamableHttpSession,
    id: u64,
    method: &str,
    params: serde_json::Value,
) -> Result<serde_json::Value, AppError> {
    tracing::info!(id, method, server = %config.label, "MCP HTTP request sending");
    tracing::debug!(id, method, %params, "MCP HTTP request params");
    let response = send_streamable_http_jsonrpc_request(
        config,
        Some(session),
        &serde_json::json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params,
        }),
    )
    .await?;
    let result = extract_streamable_http_jsonrpc_result(&response.body, id, method);
    match &result {
        Ok(value) => {
            tracing::info!(id, method, server = %config.label, "MCP HTTP response received");
            tracing::debug!(id, method, %value, "MCP HTTP response body");
        }
        Err(error) => {
            tracing::warn!(id, method, server = %config.label, error = %error.user_message, "MCP HTTP response error");
        }
    }
    result
}

async fn send_streamable_http_notification(
    config: &McpServerConfigInput,
    session: &StreamableHttpSession,
    method: &str,
    params: serde_json::Value,
) -> Result<(), AppError> {
    send_streamable_http_jsonrpc_request(
        config,
        Some(session),
        &serde_json::json!({
            "jsonrpc": "2.0",
            "method": method,
            "params": params,
        }),
    )
    .await?;
    Ok(())
}

async fn close_streamable_http_session(
    config: &McpServerConfigInput,
    session: &StreamableHttpSession,
) {
    let Some(_session_id) = session.session_id.as_deref() else {
        return;
    };
    let Ok(client) = build_streamable_http_client(config) else {
        return;
    };
    let Ok(headers) = build_streamable_http_headers(config, Some(session)) else {
        return;
    };
    let Some(url) = config.url.as_deref() else {
        return;
    };
    let _ = client
        .delete(url)
        .headers(headers)
        .send()
        .await
        .map(|response| {
            let _ = response.error_for_status();
        });
}

#[derive(Debug)]
struct StreamableHttpJsonRpcResponse {
    session_id: Option<String>,
    body: serde_json::Value,
}

async fn send_streamable_http_jsonrpc_request(
    config: &McpServerConfigInput,
    session: Option<&StreamableHttpSession>,
    message: &serde_json::Value,
) -> Result<StreamableHttpJsonRpcResponse, AppError> {
    let method = message
        .get("method")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("unknown");
    tracing::info!(server = %config.label, url = ?config.url, method, "MCP HTTP sending request");
    let client = build_streamable_http_client(config)?;
    let url = config.url.as_deref().ok_or_else(|| {
        AppError::validation(
            ErrorSource::Settings,
            "streamable-http MCP servers require a URL",
        )
    })?;
    let headers = build_streamable_http_headers(config, session)?;
    let payload = serde_json::to_vec(message).map_err(|error| {
        AppError::internal(
            ErrorSource::Tool,
            format!("Failed to serialize MCP HTTP request: {error}"),
        )
    })?;
    let response = client
        .post(url)
        .headers(headers)
        .body(payload)
        .send()
        .await
        .map_err(|error| {
            AppError::recoverable(
                ErrorSource::Tool,
                "extensions.mcp.http_request_failed",
                format!("Failed to call MCP server '{}': {error}", config.label),
            )
        })?;

    let status = response.status();
    tracing::info!(server = %config.label, %status, "MCP HTTP response status");
    let session_id = response
        .headers()
        .get(MCP_HEADER_SESSION_ID)
        .and_then(|value| value.to_str().ok())
        .map(str::to_string)
        .or_else(|| session.and_then(|current| current.session_id.clone()));
    let content_type = response
        .headers()
        .get(CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("")
        .to_ascii_lowercase();
    let body = response.text().await.map_err(|error| {
        AppError::recoverable(
            ErrorSource::Tool,
            "extensions.mcp.read_failed",
            format!("Failed to read MCP HTTP response: {error}"),
        )
    })?;

    if !status.is_success() {
        let detail = if body.trim().is_empty() {
            status.to_string()
        } else {
            format!("{status}: {}", body.trim())
        };
        tracing::warn!(server = %config.label, %status, %detail, "MCP HTTP request failed");
        return Err(AppError::recoverable(
            ErrorSource::Tool,
            "extensions.mcp.http_request_failed",
            format!("MCP server '{}' returned {detail}", config.label),
        ));
    }

    if body.trim().is_empty() {
        return Ok(StreamableHttpJsonRpcResponse {
            session_id,
            body: serde_json::Value::Null,
        });
    }

    let parsed = if content_type.starts_with("text/event-stream") {
        parse_streamable_http_sse_payload(&body)?
    } else {
        serde_json::from_str::<serde_json::Value>(&body).map_err(|error| {
            AppError::recoverable(
                ErrorSource::Tool,
                "extensions.mcp.invalid_json",
                format!("MCP HTTP response was not valid JSON: {error}"),
            )
        })?
    };

    Ok(StreamableHttpJsonRpcResponse {
        session_id,
        body: parsed,
    })
}

fn extract_streamable_http_jsonrpc_result(
    payload: &serde_json::Value,
    id: u64,
    method: &str,
) -> Result<serde_json::Value, AppError> {
    let message = match payload {
        serde_json::Value::Array(items) => items
            .iter()
            .find(|item| message_id_matches(item, id))
            .cloned()
            .ok_or_else(|| {
                AppError::recoverable(
                    ErrorSource::Tool,
                    "extensions.mcp.read_failed",
                    format!("MCP method '{method}' did not return a response payload"),
                )
            })?,
        serde_json::Value::Object(_) if message_id_matches(payload, id) => payload.clone(),
        serde_json::Value::Object(_) => {
            return Err(AppError::recoverable(
                ErrorSource::Tool,
                "extensions.mcp.read_failed",
                format!("MCP method '{method}' did not return the expected response id"),
            ))
        }
        serde_json::Value::Null => {
            return Err(AppError::recoverable(
                ErrorSource::Tool,
                "extensions.mcp.read_failed",
                format!("MCP method '{method}' returned an empty response"),
            ))
        }
        _ => {
            return Err(AppError::recoverable(
                ErrorSource::Tool,
                "extensions.mcp.invalid_json",
                format!("MCP method '{method}' returned an unsupported response payload"),
            ))
        }
    };

    if let Some(error) = message.get("error") {
        let code = error
            .get("code")
            .and_then(serde_json::Value::as_i64)
            .map(|code| code.to_string())
            .unwrap_or_else(|| "unknown".to_string());
        let detail = error
            .get("message")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("unknown error");
        return Err(AppError::recoverable(
            ErrorSource::Tool,
            "extensions.mcp.rpc_error",
            format!("MCP method '{method}' failed ({code}): {detail}"),
        ));
    }

    Ok(message
        .get("result")
        .cloned()
        .unwrap_or(serde_json::Value::Null))
}

fn parse_streamable_http_sse_payload(payload: &str) -> Result<serde_json::Value, AppError> {
    let normalized = payload.replace("\r\n", "\n");
    let mut messages = Vec::new();

    for block in normalized.split("\n\n") {
        let mut data_lines = Vec::new();
        for line in block.lines() {
            if let Some(rest) = line.strip_prefix("data:") {
                data_lines.push(rest.trim_start().to_string());
            }
        }
        if data_lines.is_empty() {
            continue;
        }
        let data = data_lines.join("\n");
        messages.push(
            serde_json::from_str::<serde_json::Value>(&data).map_err(|error| {
                AppError::recoverable(
                    ErrorSource::Tool,
                    "extensions.mcp.invalid_json",
                    format!("MCP SSE event was not valid JSON: {error}"),
                )
            })?,
        );
    }

    if messages.is_empty() {
        return Err(AppError::recoverable(
            ErrorSource::Tool,
            "extensions.mcp.read_failed",
            "MCP SSE response did not include any JSON-RPC payloads",
        ));
    }

    Ok(serde_json::Value::Array(messages))
}

async fn spawn_stdio_mcp_process(
    config: &McpServerConfigInput,
    workspace_path: Option<&str>,
) -> Result<tokio::process::Child, AppError> {
    let configured_program = config.command.as_deref().unwrap_or_default().trim();
    let program = resolve_command_path(configured_program)
        .await
        .unwrap_or_else(|| PathBuf::from(configured_program));
    let mut command = Command::new(&program);
    command.args(config.args.clone().unwrap_or_default());
    if let Some(cwd) = config.cwd.as_deref().filter(|cwd| !cwd.trim().is_empty()) {
        command.current_dir(cwd);
    } else if let Some(workspace_path) = workspace_path.filter(|path| !path.trim().is_empty()) {
        command.current_dir(workspace_path);
    }
    command.kill_on_drop(true);
    command.stdin(std::process::Stdio::piped());
    command.stdout(std::process::Stdio::piped());
    command.stderr(std::process::Stdio::piped());

    // Inject login shell environment so child processes (and their shebangs
    // like `#!/usr/bin/env node`) can find tools that live outside the minimal
    // GUI-app PATH (e.g. nvm-managed node).  User-configured `config.env`
    // entries are applied afterwards so they can override any login-shell value.
    let login_env = login_shell_env();
    {
        let mut login_keys: Vec<&str> = login_env.keys().map(|k| k.as_str()).collect();
        login_keys.sort_unstable();
        tracing::info!(
            server = %config.label,
            count = login_env.len(),
            keys = %login_keys.join(", "),
            "spawn_stdio_mcp_process: injecting login shell env"
        );
    }
    for (key, value) in login_env {
        command.env(key, value);
    }
    if let Some(env) = &config.env {
        let config_keys: Vec<&str> = env.keys().map(|k| k.as_str()).collect();
        tracing::info!(
            server = %config.label,
            count = env.len(),
            keys = %config_keys.join(", "),
            "spawn_stdio_mcp_process: injecting user-configured env (overrides login shell)"
        );
        for (key, value) in env {
            let expanded = expand_env_vars(value);
            tracing::debug!(
                server = %config.label,
                key = %key,
                "spawn_stdio_mcp_process: config env key applied"
            );
            command.env(key, expanded);
        }
    }

    command.spawn().map_err(|error| {
        AppError::recoverable(
            ErrorSource::Tool,
            "extensions.mcp.spawn_failed",
            format!(
                "Failed to start MCP server '{}' with command '{}': {error}",
                config.label,
                program.display()
            ),
        )
    })
}

async fn initialize_mcp_session(
    stdin: &mut tokio::process::ChildStdin,
    stdout: &mut BufReader<tokio::process::ChildStdout>,
) -> Result<serde_json::Value, AppError> {
    tracing::info!("MCP stdio session initializing");
    let init_result = call_stdio_mcp_method(
        stdin,
        stdout,
        1,
        "initialize",
        serde_json::json!({
            "protocolVersion": MCP_PROTOCOL_VERSION,
            "capabilities": {},
            "clientInfo": {
                "name": "tiycode",
                "version": env!("CARGO_PKG_VERSION"),
            }
        }),
    )
    .await?;
    write_stdio_mcp_message(
        stdin,
        &serde_json::json!({
            "jsonrpc": "2.0",
            "method": "notifications/initialized",
            "params": {},
        }),
    )
    .await?;
    tracing::info!("MCP stdio session initialized successfully");
    Ok(init_result)
}

async fn call_stdio_mcp_method(
    stdin: &mut tokio::process::ChildStdin,
    stdout: &mut BufReader<tokio::process::ChildStdout>,
    id: u64,
    method: &str,
    params: serde_json::Value,
) -> Result<serde_json::Value, AppError> {
    tracing::info!(id, method, "MCP stdio request sending");
    tracing::debug!(id, method, %params, "MCP stdio request params");
    write_stdio_mcp_message(
        stdin,
        &serde_json::json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params,
        }),
    )
    .await?;

    loop {
        let message = read_stdio_mcp_message(stdout).await?;
        if !message_id_matches(&message, id) {
            continue;
        }
        if let Some(error) = message.get("error") {
            let code = error
                .get("code")
                .and_then(serde_json::Value::as_i64)
                .map(|code| code.to_string())
                .unwrap_or_else(|| "unknown".to_string());
            let detail = error
                .get("message")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("unknown error");
            tracing::warn!(id, method, %code, detail, "MCP stdio response error");
            return Err(AppError::recoverable(
                ErrorSource::Tool,
                "extensions.mcp.rpc_error",
                format!("MCP method '{method}' failed ({code}): {detail}"),
            ));
        }
        let result = message
            .get("result")
            .cloned()
            .unwrap_or(serde_json::Value::Null);
        tracing::info!(id, method, "MCP stdio response received");
        tracing::debug!(id, method, %result, "MCP stdio response body");
        return Ok(result);
    }
}

async fn write_stdio_mcp_message(
    stdin: &mut tokio::process::ChildStdin,
    message: &serde_json::Value,
) -> Result<(), AppError> {
    let mut payload = serde_json::to_vec(message).map_err(|error| {
        AppError::internal(
            ErrorSource::Tool,
            format!("Failed to serialize MCP message: {error}"),
        )
    })?;
    payload.push(b'\n');
    stdin.write_all(&payload).await.map_err(|error| {
        AppError::recoverable(
            ErrorSource::Tool,
            "extensions.mcp.write_failed",
            format!("Failed to write MCP request: {error}"),
        )
    })?;
    stdin.flush().await.map_err(|error| {
        AppError::recoverable(
            ErrorSource::Tool,
            "extensions.mcp.write_failed",
            format!("Failed to flush MCP request: {error}"),
        )
    })
}

async fn read_stdio_mcp_message(
    stdout: &mut BufReader<tokio::process::ChildStdout>,
) -> Result<serde_json::Value, AppError> {
    loop {
        let mut line = String::new();
        let read = stdout.read_line(&mut line).await.map_err(|error| {
            AppError::recoverable(
                ErrorSource::Tool,
                "extensions.mcp.read_failed",
                format!("Failed to read MCP response: {error}"),
            )
        })?;
        if read == 0 {
            return Err(AppError::recoverable(
                ErrorSource::Tool,
                "extensions.mcp.unexpected_eof",
                "MCP server closed the connection before responding",
            ));
        }

        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        if let Some(length) = parse_content_length_header(trimmed) {
            loop {
                let mut header = String::new();
                let header_read = stdout.read_line(&mut header).await.map_err(|error| {
                    AppError::recoverable(
                        ErrorSource::Tool,
                        "extensions.mcp.read_failed",
                        format!("Failed to read MCP headers: {error}"),
                    )
                })?;
                if header_read == 0 || header == "\n" || header == "\r\n" {
                    break;
                }
            }

            let mut body = vec![0; length];
            stdout.read_exact(&mut body).await.map_err(|error| {
                AppError::recoverable(
                    ErrorSource::Tool,
                    "extensions.mcp.read_failed",
                    format!("Failed to read MCP response body: {error}"),
                )
            })?;
            let body = String::from_utf8(body).map_err(|error| {
                AppError::recoverable(
                    ErrorSource::Tool,
                    "extensions.mcp.invalid_utf8",
                    format!("MCP response was not valid UTF-8: {error}"),
                )
            })?;
            return serde_json::from_str::<serde_json::Value>(&body).map_err(|error| {
                AppError::recoverable(
                    ErrorSource::Tool,
                    "extensions.mcp.invalid_json",
                    format!("MCP response was not valid JSON: {error}"),
                )
            });
        }

        if !trimmed.starts_with('{') {
            continue;
        }

        return serde_json::from_str::<serde_json::Value>(trimmed).map_err(|error| {
            AppError::recoverable(
                ErrorSource::Tool,
                "extensions.mcp.invalid_json",
                format!("MCP response was not valid JSON: {error}"),
            )
        });
    }
}

fn parse_content_length_header(line: &str) -> Option<usize> {
    let (name, value) = line.split_once(':')?;
    if !name.trim().eq_ignore_ascii_case("content-length") {
        return None;
    }
    value.trim().parse::<usize>().ok()
}

fn message_id_matches(message: &serde_json::Value, id: u64) -> bool {
    message
        .get("id")
        .and_then(serde_json::Value::as_u64)
        .map(|message_id| message_id == id)
        .unwrap_or(false)
}

fn mcp_capability_enabled(capabilities: &serde_json::Value, key: &str) -> bool {
    capabilities
        .as_object()
        .and_then(|map| map.get(key))
        .is_some()
}

fn mcp_runtime_record_needs_refresh(server_id: &str, runtime: &McpRuntimeRecord) -> bool {
    runtime.tools.iter().any(|tool| {
        tool.qualified_name != build_mcp_runtime_tool_name(server_id, &tool.name)
            || !mcp_tool_name_is_provider_safe(&tool.qualified_name)
    })
}

fn mcp_runtime_record_is_disabled(runtime: &McpRuntimeRecord) -> bool {
    matches!(runtime.status.as_deref(), Some("disconnected"))
        || matches!(runtime.phase.as_deref(), Some("shutdown"))
}

fn mcp_tool_name_is_provider_safe(name: &str) -> bool {
    !name.is_empty()
        && name
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '_' || ch == '-')
}

fn parse_mcp_tools(result: &serde_json::Value, server_id: &str) -> Vec<McpToolSummaryDto> {
    let mut tools = result
        .get("tools")
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|tool| {
            let object = tool.as_object()?;
            let name = object.get("name")?.as_str()?.trim().to_string();
            if name.is_empty() {
                return None;
            }
            Some(McpToolSummaryDto {
                qualified_name: build_mcp_runtime_tool_name(server_id, &name),
                description: object
                    .get("description")
                    .and_then(serde_json::Value::as_str)
                    .map(str::to_string),
                input_schema: object.get("inputSchema").cloned(),
                name,
            })
        })
        .collect::<Vec<_>>();
    tools.sort_by(|left, right| left.name.to_lowercase().cmp(&right.name.to_lowercase()));
    tools
}

fn build_mcp_runtime_tool_name(server_id: &str, tool_name: &str) -> String {
    let server_segment = server_id.rsplit("::").next().unwrap_or(server_id);
    let server = sanitize_mcp_runtime_name_segment(server_segment);
    let tool = sanitize_mcp_runtime_name_segment(tool_name);
    format!("__mcp_{}_{}", server, tool)
}

fn sanitize_mcp_runtime_name_segment(value: &str) -> String {
    let sanitized = value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '_' || ch == '-' {
                ch
            } else {
                '_'
            }
        })
        .collect::<String>()
        .trim_matches('_')
        .to_string();

    if sanitized.is_empty() {
        "tool".to_string()
    } else {
        sanitized
    }
}

fn parse_mcp_resources(result: &serde_json::Value) -> Vec<McpResourceSummaryDto> {
    let mut resources = result
        .get("resources")
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|resource| {
            let object = resource.as_object()?;
            let uri = object.get("uri")?.as_str()?.trim().to_string();
            if uri.is_empty() {
                return None;
            }
            Some(McpResourceSummaryDto {
                name: object
                    .get("name")
                    .and_then(serde_json::Value::as_str)
                    .filter(|name| !name.trim().is_empty())
                    .map(str::to_string)
                    .unwrap_or_else(|| uri.clone()),
                uri,
                description: object
                    .get("description")
                    .and_then(serde_json::Value::as_str)
                    .map(str::to_string),
                mime_type: object
                    .get("mimeType")
                    .and_then(serde_json::Value::as_str)
                    .map(str::to_string),
            })
        })
        .collect::<Vec<_>>();
    resources.sort_by(|left, right| left.name.to_lowercase().cmp(&right.name.to_lowercase()));
    resources
}

fn append_mcp_stderr(mut error: AppError, stderr_output: &str) -> AppError {
    let trimmed = stderr_output.trim();
    if trimmed.is_empty() {
        return error;
    }
    error.user_message = format!("{} ({trimmed})", error.user_message);
    error
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};
    use tempfile::tempdir;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::{TcpListener, TcpStream};

    #[derive(Debug, Clone)]
    struct ObservedHttpRequest {
        method: String,
        headers: HashMap<String, String>,
        body: String,
    }

    async fn read_http_request(
        stream: &mut TcpStream,
    ) -> (String, HashMap<String, String>, serde_json::Value) {
        let mut buffer = Vec::new();
        let mut chunk = [0_u8; 2048];
        let header_end = loop {
            let read = stream.read(&mut chunk).await.expect("read request");
            assert!(read > 0, "request closed before headers");
            buffer.extend_from_slice(&chunk[..read]);
            if let Some(index) = buffer.windows(4).position(|window| window == b"\r\n\r\n") {
                break index + 4;
            }
        };

        let header_text = String::from_utf8(buffer[..header_end].to_vec()).expect("headers utf8");
        let mut lines = header_text.split("\r\n");
        let request_line = lines.next().expect("request line");
        let method = request_line
            .split_whitespace()
            .next()
            .expect("request method")
            .to_string();
        let headers = lines
            .filter(|line| !line.trim().is_empty())
            .filter_map(|line| {
                let (name, value) = line.split_once(':')?;
                Some((name.trim().to_ascii_lowercase(), value.trim().to_string()))
            })
            .collect::<HashMap<_, _>>();
        let content_length = headers
            .get("content-length")
            .and_then(|value| value.parse::<usize>().ok())
            .unwrap_or(0);
        let mut body = buffer[header_end..].to_vec();
        while body.len() < content_length {
            let read = stream.read(&mut chunk).await.expect("read body");
            assert!(read > 0, "request closed before body");
            body.extend_from_slice(&chunk[..read]);
        }
        body.truncate(content_length);
        let json = if body.is_empty() {
            serde_json::Value::Null
        } else {
            serde_json::from_slice(&body).expect("json body")
        };
        (method, headers, json)
    }

    async fn write_http_response(
        stream: &mut TcpStream,
        status: &str,
        headers: &[(&str, String)],
        body: &str,
    ) {
        let mut response = format!("HTTP/1.1 {status}\r\nContent-Length: {}\r\n", body.len());
        for (name, value) in headers {
            response.push_str(&format!("{name}: {value}\r\n"));
        }
        response.push_str("\r\n");
        response.push_str(body);
        stream
            .write_all(response.as_bytes())
            .await
            .expect("write response");
    }

    async fn spawn_fake_streamable_http_server() -> (
        String,
        Arc<Mutex<Vec<ObservedHttpRequest>>>,
        tokio::task::JoinHandle<()>,
    ) {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind fake server");
        let address = listener.local_addr().expect("local addr");
        let requests = Arc::new(Mutex::new(Vec::new()));
        let requests_clone = Arc::clone(&requests);
        let handle = tokio::spawn(async move {
            for _ in 0..9 {
                let (mut stream, _) = listener.accept().await.expect("accept connection");
                let (method, headers, body) = read_http_request(&mut stream).await;
                requests_clone
                    .lock()
                    .expect("requests lock")
                    .push(ObservedHttpRequest {
                        method: method.clone(),
                        headers: headers.clone(),
                        body: body.to_string(),
                    });
                match method.as_str() {
                    "POST" => {
                        let rpc_method = body
                            .get("method")
                            .and_then(serde_json::Value::as_str)
                            .expect("rpc method");
                        match rpc_method {
                            "initialize" => {
                                write_http_response(
                                    &mut stream,
                                    "200 OK",
                                    &[
                                        ("Content-Type", "application/json".to_string()),
                                        (MCP_HEADER_SESSION_ID, "session-123".to_string()),
                                    ],
                                    &serde_json::json!({
                                        "jsonrpc": "2.0",
                                        "id": body.get("id").and_then(serde_json::Value::as_u64).expect("init id"),
                                        "result": {
                                            "protocolVersion": MCP_PROTOCOL_VERSION,
                                            "capabilities": { "tools": {}, "resources": {} },
                                            "serverInfo": { "name": "Fake HTTP MCP", "version": "1.0.0" }
                                        }
                                    })
                                    .to_string(),
                                )
                                .await;
                            }
                            "notifications/initialized" => {
                                assert_eq!(
                                    headers.get("mcp-session-id").map(String::as_str),
                                    Some("session-123")
                                );
                                write_http_response(&mut stream, "202 Accepted", &[], "").await;
                            }
                            "tools/list" => {
                                assert_eq!(
                                    headers.get("authorization").map(String::as_str),
                                    Some("Bearer test-token")
                                );
                                let body = concat!(
                                    "event: message\r\n",
                                    "data: {\"jsonrpc\":\"2.0\",\"method\":\"notifications/message\",\"params\":{\"level\":\"info\"}}\r\n\r\n",
                                    "data: {\"jsonrpc\":\"2.0\",\"id\":2,\"result\":{\"tools\":[{\"name\":\"lookup\",\"description\":\"Look things up\",\"inputSchema\":{\"type\":\"object\"}}]}}\r\n\r\n"
                                );
                                write_http_response(
                                    &mut stream,
                                    "200 OK",
                                    &[("Content-Type", "text/event-stream".to_string())],
                                    body,
                                )
                                .await;
                            }
                            "resources/list" => {
                                write_http_response(
                                    &mut stream,
                                    "200 OK",
                                    &[("Content-Type", "application/json".to_string())],
                                    &serde_json::json!({
                                        "jsonrpc": "2.0",
                                        "id": body.get("id").and_then(serde_json::Value::as_u64).expect("resources id"),
                                        "result": {
                                            "resources": [
                                                {
                                                    "uri": "file:///docs/readme.md",
                                                    "name": "README",
                                                    "description": "Repo readme",
                                                    "mimeType": "text/markdown"
                                                }
                                            ]
                                        }
                                    })
                                    .to_string(),
                                )
                                .await;
                            }
                            "tools/call" => {
                                let body = concat!(
                                    "data: {\"jsonrpc\":\"2.0\",\"id\":4,\"result\":{\"content\":[{\"type\":\"text\",\"text\":\"ok\"}],\"isError\":false}}\r\n\r\n"
                                );
                                write_http_response(
                                    &mut stream,
                                    "200 OK",
                                    &[("Content-Type", "text/event-stream".to_string())],
                                    body,
                                )
                                .await;
                            }
                            other => panic!("unexpected rpc method: {other}"),
                        }
                    }
                    "DELETE" => {
                        assert_eq!(
                            headers.get("mcp-session-id").map(String::as_str),
                            Some("session-123")
                        );
                        write_http_response(&mut stream, "204 No Content", &[], "").await;
                    }
                    other => panic!("unexpected HTTP method: {other}"),
                }
            }
        });

        (format!("http://{address}/mcp"), requests, handle)
    }

    #[test]
    fn compare_extension_summaries_sorts_enabled_then_installed_then_name() {
        let mut items = vec![
            ExtensionSummaryDto {
                id: "skill-zeta".to_string(),
                kind: ExtensionKind::Skill,
                name: "Zeta".to_string(),
                version: "1.0.0".to_string(),
                description: None,
                source: ExtensionSourceDto::Builtin,
                install_state: ExtensionInstallState::Discovered,
                health: ExtensionHealth::Unknown,
                permissions: Vec::new(),
                tags: Vec::new(),
            },
            ExtensionSummaryDto {
                id: "skill-bravo".to_string(),
                kind: ExtensionKind::Skill,
                name: "bravo".to_string(),
                version: "1.0.0".to_string(),
                description: None,
                source: ExtensionSourceDto::Builtin,
                install_state: ExtensionInstallState::Installed,
                health: ExtensionHealth::Unknown,
                permissions: Vec::new(),
                tags: Vec::new(),
            },
            ExtensionSummaryDto {
                id: "skill-alpha".to_string(),
                kind: ExtensionKind::Skill,
                name: "Alpha".to_string(),
                version: "1.0.0".to_string(),
                description: None,
                source: ExtensionSourceDto::Builtin,
                install_state: ExtensionInstallState::Enabled,
                health: ExtensionHealth::Unknown,
                permissions: Vec::new(),
                tags: Vec::new(),
            },
        ];

        items.sort_by(compare_extension_summaries);

        assert_eq!(
            items
                .iter()
                .map(|item| item.id.as_str())
                .collect::<Vec<_>>(),
            vec!["skill-alpha", "skill-bravo", "skill-zeta"]
        );
    }

    #[test]
    fn compare_mcp_server_states_sorts_enabled_then_name() {
        let mut items = vec![
            McpServerStateDto {
                id: "server-zeta".to_string(),
                label: "Zeta".to_string(),
                scope: "global".to_string(),
                status: "ready".to_string(),
                phase: "idle".to_string(),
                tools: Vec::new(),
                resources: Vec::new(),
                stale_snapshot: false,
                last_error: None,
                updated_at: "2026-04-15T00:00:00Z".to_string(),
                config: McpServerConfigDto {
                    id: "server-zeta".to_string(),
                    label: "Zeta".to_string(),
                    transport: "stdio".to_string(),
                    enabled: false,
                    auto_start: false,
                    command: None,
                    args: Vec::new(),
                    env: HashMap::new(),
                    cwd: None,
                    url: None,
                    headers: HashMap::new(),
                    timeout_ms: None,
                },
            },
            McpServerStateDto {
                id: "server-alpha".to_string(),
                label: "Alpha".to_string(),
                scope: "global".to_string(),
                status: "ready".to_string(),
                phase: "idle".to_string(),
                tools: Vec::new(),
                resources: Vec::new(),
                stale_snapshot: false,
                last_error: None,
                updated_at: "2026-04-15T00:00:00Z".to_string(),
                config: McpServerConfigDto {
                    id: "server-alpha".to_string(),
                    label: "Alpha".to_string(),
                    transport: "stdio".to_string(),
                    enabled: true,
                    auto_start: true,
                    command: None,
                    args: Vec::new(),
                    env: HashMap::new(),
                    cwd: None,
                    url: None,
                    headers: HashMap::new(),
                    timeout_ms: None,
                },
            },
            McpServerStateDto {
                id: "server-bravo".to_string(),
                label: "bravo".to_string(),
                scope: "global".to_string(),
                status: "ready".to_string(),
                phase: "idle".to_string(),
                tools: Vec::new(),
                resources: Vec::new(),
                stale_snapshot: false,
                last_error: None,
                updated_at: "2026-04-15T00:00:00Z".to_string(),
                config: McpServerConfigDto {
                    id: "server-bravo".to_string(),
                    label: "bravo".to_string(),
                    transport: "stdio".to_string(),
                    enabled: true,
                    auto_start: true,
                    command: None,
                    args: Vec::new(),
                    env: HashMap::new(),
                    cwd: None,
                    url: None,
                    headers: HashMap::new(),
                    timeout_ms: None,
                },
            },
        ];

        items.sort_by(compare_mcp_server_states);

        assert_eq!(
            items
                .iter()
                .map(|item| item.id.as_str())
                .collect::<Vec<_>>(),
            vec!["server-alpha", "server-bravo", "server-zeta"]
        );
    }

    #[test]
    fn compare_skill_records_sorts_enabled_then_name() {
        let mut items = vec![
            SkillRecordDto {
                id: "skill-zeta".to_string(),
                name: "Zeta".to_string(),
                description: None,
                tags: Vec::new(),
                triggers: Vec::new(),
                tools: Vec::new(),
                priority: None,
                source: "builtin".to_string(),
                path: "/tmp/zeta".to_string(),
                enabled: false,
                scope: "global".to_string(),
                content_preview: String::new(),
                prompt_budget_chars: 100,
            },
            SkillRecordDto {
                id: "skill-alpha".to_string(),
                name: "Alpha".to_string(),
                description: None,
                tags: Vec::new(),
                triggers: Vec::new(),
                tools: Vec::new(),
                priority: None,
                source: "builtin".to_string(),
                path: "/tmp/alpha".to_string(),
                enabled: true,
                scope: "global".to_string(),
                content_preview: String::new(),
                prompt_budget_chars: 100,
            },
            SkillRecordDto {
                id: "skill-bravo".to_string(),
                name: "bravo".to_string(),
                description: None,
                tags: Vec::new(),
                triggers: Vec::new(),
                tools: Vec::new(),
                priority: None,
                source: "builtin".to_string(),
                path: "/tmp/bravo".to_string(),
                enabled: true,
                scope: "global".to_string(),
                content_preview: String::new(),
                prompt_budget_chars: 100,
            },
        ];

        items.sort_by(compare_skill_records);

        assert_eq!(
            items
                .iter()
                .map(|item| item.id.as_str())
                .collect::<Vec<_>>(),
            vec!["skill-alpha", "skill-bravo", "skill-zeta"]
        );
    }

    #[test]
    fn compare_marketplace_items_sorts_enabled_then_installed_then_name() {
        let mut items = vec![
            MarketplaceItemDto {
                id: "market-zeta".to_string(),
                source_id: "source".to_string(),
                source_name: "Source".to_string(),
                kind: "plugin".to_string(),
                name: "Zeta".to_string(),
                version: "1.0.0".to_string(),
                summary: "summary".to_string(),
                description: "description".to_string(),
                publisher: "publisher".to_string(),
                tags: Vec::new(),
                hooks: Vec::new(),
                command_names: Vec::new(),
                mcp_servers: Vec::new(),
                skill_names: Vec::new(),
                path: "/tmp/zeta".to_string(),
                installable: true,
                installed: false,
                enabled: false,
            },
            MarketplaceItemDto {
                id: "market-bravo".to_string(),
                source_id: "source".to_string(),
                source_name: "Source".to_string(),
                kind: "plugin".to_string(),
                name: "bravo".to_string(),
                version: "1.0.0".to_string(),
                summary: "summary".to_string(),
                description: "description".to_string(),
                publisher: "publisher".to_string(),
                tags: Vec::new(),
                hooks: Vec::new(),
                command_names: Vec::new(),
                mcp_servers: Vec::new(),
                skill_names: Vec::new(),
                path: "/tmp/bravo".to_string(),
                installable: true,
                installed: true,
                enabled: false,
            },
            MarketplaceItemDto {
                id: "market-alpha".to_string(),
                source_id: "source".to_string(),
                source_name: "Source".to_string(),
                kind: "plugin".to_string(),
                name: "Alpha".to_string(),
                version: "1.0.0".to_string(),
                summary: "summary".to_string(),
                description: "description".to_string(),
                publisher: "publisher".to_string(),
                tags: Vec::new(),
                hooks: Vec::new(),
                command_names: Vec::new(),
                mcp_servers: Vec::new(),
                skill_names: Vec::new(),
                path: "/tmp/alpha".to_string(),
                installable: true,
                installed: true,
                enabled: true,
            },
        ];

        items.sort_by(compare_marketplace_items);

        assert_eq!(
            items
                .iter()
                .map(|item| item.id.as_str())
                .collect::<Vec<_>>(),
            vec!["market-alpha", "market-bravo", "market-zeta"]
        );
    }

    #[test]
    fn skill_state_store_deserializes_legacy_pinned_alias_without_serializing_it() {
        let store: SkillStateStore = serde_json::from_value(serde_json::json!({
            "enabled": ["skill-a"],
            "disabled": ["skill-b"],
            "pinned": ["skill-legacy"]
        }))
        .expect("deserialize skill state");

        assert_eq!(store.enabled, vec!["skill-a"]);
        assert_eq!(store.disabled, vec!["skill-b"]);
        assert_eq!(store.legacy_pinned, vec!["skill-legacy"]);

        let serialized = serde_json::to_value(&store).expect("serialize skill state");
        assert_eq!(
            serialized.get("enabled"),
            Some(&serde_json::json!(["skill-a"]))
        );
        assert_eq!(
            serialized.get("disabled"),
            Some(&serde_json::json!(["skill-b"]))
        );
        assert!(serialized.get("pinned").is_none());
        assert!(serialized.get("legacyPinned").is_none());
    }

    #[test]
    fn parse_skill_markdown_namespaces_non_builtin_sources() {
        let skill_dir = tempdir().expect("tempdir");
        let raw = r#"---
name: Skill Alpha
description: Example skill
---

Body text
"#;

        let (builtin_record, _) =
            parse_skill_markdown(raw, skill_dir.path(), "builtin").expect("builtin skill");
        let (workspace_record, _) =
            parse_skill_markdown(raw, skill_dir.path(), "workspace").expect("workspace skill");

        assert!(!builtin_record.id.is_empty());
        assert_eq!(
            workspace_record.id,
            format!("workspace:{}", builtin_record.id)
        );
    }

    #[test]
    fn parse_skill_markdown_supports_folded_descriptions_and_yaml_lists() {
        let skill_dir = tempdir().expect("tempdir");
        let raw = r#"---
name: project-docs-sync
description: >-
  Proactively detect when project documentation files need updating.
  Automatically applies targeted edits to keep docs in sync.
tags:
  - documentation
  - automation
triggers:
  - README.md
  - AGENTS.md
tools:
  - git
  - rg
---

Body text
"#;

        let (record, _) = parse_skill_markdown(raw, skill_dir.path(), "builtin")
            .expect("folded description skill");

        assert_eq!(
            record.description.as_deref(),
            Some(
                "Proactively detect when project documentation files need updating. Automatically applies targeted edits to keep docs in sync."
            )
        );
        assert_eq!(record.tags, vec!["documentation", "automation"]);
        assert_eq!(record.triggers, vec!["README.md", "AGENTS.md"]);
        assert_eq!(record.tools, vec!["git", "rg"]);
    }

    #[test]
    fn parse_plugin_manifest_supports_minimal_marketplace_shape() {
        let plugin_dir = tempdir().expect("tempdir");
        fs::create_dir_all(plugin_dir.path().join("commands")).expect("create commands dir");
        fs::write(
            plugin_dir.path().join("commands").join("feature-dev.md"),
            "# feature-dev",
        )
        .expect("write command file");

        let manifest = parse_plugin_manifest(
            r#"{
              "name": "feature-dev",
              "description": "Comprehensive feature development workflow",
              "author": {
                "name": "Anthropic",
                "email": "support@anthropic.com"
              }
            }"#,
            plugin_dir.path(),
        )
        .expect("parse manifest");

        assert_eq!(
            manifest.id,
            plugin_dir
                .path()
                .file_name()
                .and_then(OsStr::to_str)
                .unwrap_or("plugin")
        );
        assert_eq!(manifest.name, "feature-dev");
        assert_eq!(manifest.version, "0.0.0");
        assert_eq!(manifest.author.as_deref(), Some("Anthropic"));
        assert!(manifest.commands.is_empty());
    }

    #[test]
    fn build_plugin_managed_mcp_config_supports_bundle_specs() {
        let manifest = PluginManifest {
            id: "bundle.plugin".to_string(),
            name: "Bundle Plugin".to_string(),
            version: "1.0.0".to_string(),
            description: None,
            author: None,
            homepage: None,
            default_enabled: Some(true),
            capabilities: Vec::new(),
            permissions: Vec::new(),
            hooks: None,
            tools: Vec::new(),
            commands: Vec::new(),
            timeout_ms: None,
            skills_dir: None,
            config_schema: None,
        };

        let http_value = serde_json::json!({
            "type": "http",
            "url": "https://mcp.example.com/api"
        });
        let http_config = build_plugin_managed_mcp_config(
            &manifest,
            "example-server".to_string(),
            http_value.as_object().expect("http object"),
        )
        .expect("http config");
        assert_eq!(http_config.id, "plugin::bundle.plugin::example-server");
        assert_eq!(http_config.transport, "streamable-http");
        assert_eq!(
            http_config.url.as_deref(),
            Some("https://mcp.example.com/api")
        );

        let stdio_value = serde_json::json!({
            "command": "uvx",
            "args": ["context7-mcp"],
            "env": { "TOKEN": "secret" }
        });
        let stdio_config = build_plugin_managed_mcp_config(
            &manifest,
            "context7".to_string(),
            stdio_value.as_object().expect("stdio object"),
        )
        .expect("stdio config");
        assert_eq!(stdio_config.label, "Bundle Plugin / context7");
        assert_eq!(stdio_config.transport, "stdio");
        assert_eq!(stdio_config.command.as_deref(), Some("uvx"));
        assert_eq!(
            stdio_config.args.as_ref().expect("args"),
            &vec!["context7-mcp".to_string()]
        );
        assert_eq!(
            stdio_config
                .env
                .as_ref()
                .and_then(|env| env.get("TOKEN"))
                .map(String::as_str),
            Some("secret")
        );
    }

    #[test]
    fn merge_plugin_managed_mcp_config_preserves_user_enabled_state() {
        let existing = McpServerConfigInput {
            id: "plugin::bundle.plugin::context7".to_string(),
            label: "Bundle Plugin / context7".to_string(),
            transport: "stdio".to_string(),
            enabled: false,
            auto_start: false,
            command: Some("old-command".to_string()),
            args: Some(vec!["old-arg".to_string()]),
            env: Some(HashMap::from([("TOKEN".to_string(), "secret".to_string())])),
            cwd: Some("/tmp/context7".to_string()),
            url: None,
            headers: Some(HashMap::from([(
                "Authorization".to_string(),
                "Bearer test".to_string(),
            )])),
            timeout_ms: Some(45_000),
        };

        let managed = McpServerConfigInput {
            id: existing.id.clone(),
            label: existing.label.clone(),
            transport: "stdio".to_string(),
            enabled: true,
            auto_start: true,
            command: Some("uvx".to_string()),
            args: Some(vec!["context7-mcp".to_string()]),
            env: None,
            cwd: None,
            url: None,
            headers: None,
            timeout_ms: Some(15_000),
        };

        let merged = merge_plugin_managed_mcp_config(Some(&existing), managed);

        assert!(!merged.enabled);
        assert!(!merged.auto_start);
        assert_eq!(merged.command.as_deref(), Some("uvx"));
        assert_eq!(
            merged.args.as_ref().expect("args"),
            &vec!["context7-mcp".to_string()]
        );
        assert_eq!(
            merged
                .env
                .as_ref()
                .and_then(|env| env.get("TOKEN"))
                .map(String::as_str),
            Some("secret")
        );
        assert_eq!(merged.cwd.as_deref(), Some("/tmp/context7"));
        assert_eq!(
            merged
                .headers
                .as_ref()
                .and_then(|headers| headers.get("Authorization"))
                .map(String::as_str),
            Some("Bearer test")
        );
        assert_eq!(merged.timeout_ms, Some(45_000));
    }

    #[test]
    fn merge_mcp_sensitive_fields_preserves_masked_values_on_edit() {
        let existing = McpServerConfigInput {
            id: "server".to_string(),
            label: "Server".to_string(),
            transport: "streamable-http".to_string(),
            enabled: true,
            auto_start: true,
            command: None,
            args: None,
            env: Some(HashMap::from([(
                "TOKEN".to_string(),
                "super-secret".to_string(),
            )])),
            cwd: None,
            url: Some("https://example.com/mcp?token=secret".to_string()),
            headers: Some(HashMap::from([(
                "Authorization".to_string(),
                "Bearer test-token".to_string(),
            )])),
            timeout_ms: Some(30_000),
        };

        let incoming = McpServerConfigInput {
            id: existing.id.clone(),
            label: existing.label.clone(),
            transport: "http-streamable".to_string(),
            enabled: true,
            auto_start: true,
            command: None,
            args: None,
            env: Some(HashMap::from([(
                "TOKEN".to_string(),
                mask_sensitive_value("super-secret"),
            )])),
            cwd: None,
            url: Some(mask_url("https://example.com/mcp?token=secret".to_string())),
            headers: Some(HashMap::from([(
                "Authorization".to_string(),
                mask_sensitive_value("Bearer test-token"),
            )])),
            timeout_ms: Some(30_000),
        };

        let merged = merge_mcp_sensitive_fields(&existing, canonicalize_mcp_config(incoming));

        assert_eq!(merged.transport, "streamable-http");
        assert_eq!(
            merged.url.as_deref(),
            Some("https://example.com/mcp?token=secret")
        );
        assert_eq!(
            merged
                .headers
                .as_ref()
                .and_then(|headers| headers.get("Authorization"))
                .map(String::as_str),
            Some("Bearer test-token")
        );
        assert_eq!(
            merged
                .env
                .as_ref()
                .and_then(|env| env.get("TOKEN"))
                .map(String::as_str),
            Some("super-secret")
        );
    }

    #[test]
    fn build_plugin_managed_mcp_label_deduplicates_equivalent_names() {
        assert_eq!(
            build_plugin_managed_mcp_label("context7", "context7"),
            "context7"
        );
        assert_eq!(
            build_plugin_managed_mcp_label("Context7", "context-7"),
            "Context7"
        );
        assert_eq!(
            build_plugin_managed_mcp_label("Anthropic Tools", "filesystem"),
            "Anthropic Tools / filesystem"
        );
    }

    #[test]
    fn build_plugin_managed_mcp_config_deduplicates_matching_plugin_and_server_names() {
        let manifest = PluginManifest {
            id: "context7".to_string(),
            name: "context7".to_string(),
            version: "1.0.0".to_string(),
            description: None,
            author: None,
            homepage: None,
            default_enabled: Some(true),
            capabilities: Vec::new(),
            permissions: Vec::new(),
            hooks: None,
            tools: Vec::new(),
            commands: Vec::new(),
            timeout_ms: None,
            skills_dir: None,
            config_schema: None,
        };

        let value = serde_json::json!({
            "command": "uvx",
            "args": ["context7-mcp"]
        });
        let config = build_plugin_managed_mcp_config(
            &manifest,
            "context7".to_string(),
            value.as_object().expect("stdio object"),
        )
        .expect("stdio config");

        assert_eq!(config.label, "context7");
    }

    #[test]
    fn parse_plugin_command_markdown_supports_command_files() {
        let command = parse_plugin_command_markdown(
            r#"---
description: Code review a pull request
disable-model-invocation: false
---

Provide a code review for the given pull request."#,
            "code-review",
        )
        .expect("command");

        assert_eq!(command.name, "code-review");
        assert_eq!(command.description, "Code review a pull request");
        assert_eq!(
            command.prompt_template.as_deref(),
            Some("Provide a code review for the given pull request.")
        );
    }

    #[tokio::test]
    async fn load_plugin_from_dir_supports_claude_plugin_manifest_layout() {
        let plugin_dir = tempdir().expect("tempdir");
        let manifest_dir = plugin_dir.path().join(".claude-plugin");
        fs::create_dir_all(&manifest_dir).expect("create manifest dir");
        fs::write(
            manifest_dir.join("plugin.json"),
            r#"{
              "id": "market.plugin",
              "name": "Marketplace Plugin",
              "version": "1.0.0",
              "description": "plugin from marketplace",
              "tools": [],
              "commands": []
            }"#,
        )
        .expect("write manifest");

        let runtime = ExtensionsManager::new(
            SqlitePool::connect("sqlite::memory:")
                .await
                .expect("sqlite pool"),
        )
        .load_plugin_from_dir(plugin_dir.path(), false)
        .expect("load plugin");

        assert_eq!(runtime.manifest.id, "market.plugin");
        assert_eq!(runtime.manifest.name, "Marketplace Plugin");
        assert_eq!(
            runtime.path,
            fs::canonicalize(plugin_dir.path()).expect("canonical plugin path")
        );
    }

    #[tokio::test]
    async fn probe_stdio_mcp_runtime_discovers_tools_and_executes_calls() {
        let server_dir = tempdir().expect("tempdir");
        let server_path = server_dir.path().join("fake-mcp.js");
        fs::write(
            &server_path,
            r#"const readline = require("readline");
const rl = readline.createInterface({ input: process.stdin, crlfDelay: Infinity });
function send(message) {
  process.stdout.write(JSON.stringify(message) + "\n");
}
rl.on("line", (line) => {
  const message = JSON.parse(line);
  if (message.method === "initialize") {
    send({
      jsonrpc: "2.0",
      id: message.id,
      result: {
        protocolVersion: "2025-06-18",
        capabilities: { tools: {}, resources: {} },
        serverInfo: { name: "Fake MCP", version: "1.0.0" }
      }
    });
    return;
  }
  if (message.method === "notifications/initialized") {
    return;
  }
  if (message.method === "tools/list") {
    send({
      jsonrpc: "2.0",
      id: message.id,
      result: {
        tools: [
          {
            name: "lookup",
            description: "Look things up",
            inputSchema: { type: "object" }
          }
        ]
      }
    });
    return;
  }
  if (message.method === "resources/list") {
    send({
      jsonrpc: "2.0",
      id: message.id,
      result: {
        resources: [
          {
            uri: "file:///docs/readme.md",
            name: "README",
            description: "Repo readme",
            mimeType: "text/markdown"
          }
        ]
      }
    });
    return;
  }
  if (message.method === "tools/call") {
    send({
      jsonrpc: "2.0",
      id: message.id,
      result: {
        content: [{ type: "text", text: "ok" }],
        isError: false
      }
    });
  }
});"#,
        )
        .expect("write fake mcp server");

        let manager = ExtensionsManager::new(
            SqlitePool::connect("sqlite::memory:")
                .await
                .expect("sqlite pool"),
        );
        let config = McpServerConfigInput {
            id: "fake::server".to_string(),
            label: "Fake MCP".to_string(),
            transport: "stdio".to_string(),
            enabled: true,
            auto_start: true,
            command: Some("node".to_string()),
            args: Some(vec![server_path.to_string_lossy().to_string()]),
            env: None,
            cwd: None,
            url: None,
            headers: None,
            timeout_ms: Some(5_000),
        };

        let runtime = manager
            .probe_stdio_mcp_runtime(&config)
            .await
            .expect("probe runtime");
        assert_eq!(runtime.tools.len(), 1);
        assert_eq!(runtime.tools[0].name, "lookup");
        assert_eq!(runtime.tools[0].qualified_name, "__mcp_server_lookup");
        assert_eq!(runtime.resources.len(), 1);
        assert_eq!(runtime.resources[0].name, "README");

        let result = manager
            .call_mcp_tool_once(
                &config,
                "lookup",
                &serde_json::json!({ "query": "docs" }),
                None,
            )
            .await
            .expect("call mcp tool");
        assert_eq!(
            result
                .get("content")
                .and_then(serde_json::Value::as_array)
                .and_then(|content| content.first())
                .and_then(|item| item.get("text"))
                .and_then(serde_json::Value::as_str),
            Some("ok")
        );
    }

    #[tokio::test]
    async fn probe_streamable_http_runtime_discovers_tools_and_executes_calls() {
        let (url, requests, server_task) = spawn_fake_streamable_http_server().await;
        let manager = ExtensionsManager::new(
            SqlitePool::connect("sqlite::memory:")
                .await
                .expect("sqlite pool"),
        );
        let config = McpServerConfigInput {
            id: "fake::http".to_string(),
            label: "Fake HTTP MCP".to_string(),
            transport: "streamable-http".to_string(),
            enabled: true,
            auto_start: true,
            command: None,
            args: None,
            env: None,
            cwd: None,
            url: Some(url),
            headers: Some(HashMap::from([(
                "Authorization".to_string(),
                "Bearer test-token".to_string(),
            )])),
            timeout_ms: Some(5_000),
        };

        let runtime = manager
            .probe_streamable_http_mcp_runtime(&config)
            .await
            .expect("probe runtime");
        assert_eq!(runtime.tools.len(), 1);
        assert_eq!(runtime.tools[0].name, "lookup");
        assert_eq!(runtime.tools[0].qualified_name, "__mcp_http_lookup");
        assert_eq!(runtime.resources.len(), 1);
        assert_eq!(runtime.resources[0].name, "README");

        let result = manager
            .call_streamable_http_mcp_tool_once(
                &config,
                "lookup",
                &serde_json::json!({ "query": "docs" }),
            )
            .await
            .expect("call mcp tool");
        assert_eq!(
            result
                .get("content")
                .and_then(serde_json::Value::as_array)
                .and_then(|content| content.first())
                .and_then(|item| item.get("text"))
                .and_then(serde_json::Value::as_str),
            Some("ok")
        );

        server_task.await.expect("server task");
        let requests = requests.lock().expect("requests lock");
        assert_eq!(requests.len(), 9);
        assert!(requests.iter().any(|request| request.method == "DELETE"));
        assert!(requests.iter().any(|request| {
            request
                .headers
                .get("mcp-protocol-version")
                .map(String::as_str)
                == Some(MCP_PROTOCOL_VERSION)
        }));
        assert!(requests
            .iter()
            .any(|request| request.body.contains("\"tools/call\"")));
    }

    #[test]
    fn build_mcp_runtime_tool_name_uses_provider_safe_format() {
        assert_eq!(
            build_mcp_runtime_tool_name("plugin::context7::context7", "resolve-library-id"),
            "__mcp_context7_resolve-library-id"
        );
        assert_eq!(
            build_mcp_runtime_tool_name("workspace/http server", "query docs"),
            "__mcp_workspace_http_server_query_docs"
        );
    }

    #[test]
    fn legacy_mcp_runtime_records_trigger_refresh() {
        let legacy = McpRuntimeRecord {
            tools: vec![McpToolSummaryDto {
                name: "query-docs".to_string(),
                qualified_name: "plugin::context7::context7::query-docs".to_string(),
                description: None,
                input_schema: None,
            }],
            ..McpRuntimeRecord::default()
        };

        assert!(mcp_runtime_record_needs_refresh(
            "plugin::context7::context7",
            &legacy
        ));

        let current = McpRuntimeRecord {
            tools: vec![McpToolSummaryDto {
                name: "query-docs".to_string(),
                qualified_name: "__mcp_context7_query-docs".to_string(),
                description: None,
                input_schema: None,
            }],
            ..McpRuntimeRecord::default()
        };

        assert!(!mcp_runtime_record_needs_refresh(
            "plugin::context7::context7",
            &current
        ));
    }

    #[tokio::test]
    async fn invalid_config_json_returns_default_and_records_diagnostic() {
        let manager = ExtensionsManager::new(
            SqlitePool::connect("sqlite::memory:")
                .await
                .expect("sqlite pool"),
        );
        let temp = tempdir().expect("tempdir");
        let path = temp.path().join("plugins.json");
        fs::write(&path, "{ invalid json").expect("write invalid json");

        let records = manager
            .read_json_file_with_diagnostics::<Vec<InstalledPluginRecord>>(
                &path,
                "plugins",
                ConfigScope::Global,
            )
            .expect("read config")
            .value;

        assert!(records.is_empty());
        let diagnostics = manager.list_config_diagnostics();
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].area, "plugins");
        assert_eq!(diagnostics[0].kind, ConfigDiagnosticKind::InvalidJson);
        assert!(diagnostics[0].file_path.contains("plugins.json"));
    }

    #[test]
    fn expand_env_vars_braced_syntax() {
        // SAFETY: test-only, unique var names avoid races with other tests
        unsafe { std::env::set_var("_TEST_EXPAND_TOKEN", "my_secret_123") };
        let result = expand_env_vars("Bearer ${_TEST_EXPAND_TOKEN}");
        assert_eq!(result, "Bearer my_secret_123");
        unsafe { std::env::remove_var("_TEST_EXPAND_TOKEN") };
    }

    #[test]
    fn expand_env_vars_unbraced_syntax() {
        unsafe { std::env::set_var("_TEST_EXPAND_PLAIN", "value_abc") };
        let result = expand_env_vars("prefix-$_TEST_EXPAND_PLAIN-suffix");
        assert_eq!(result, "prefix-value_abc-suffix");
        unsafe { std::env::remove_var("_TEST_EXPAND_PLAIN") };
    }

    #[test]
    fn expand_env_vars_missing_variable_preserved() {
        let result = expand_env_vars("Bearer ${_NONEXISTENT_VAR_12345}");
        assert_eq!(result, "Bearer ${_NONEXISTENT_VAR_12345}");
    }

    #[test]
    fn expand_env_vars_no_variables() {
        assert_eq!(expand_env_vars("plain text"), "plain text");
    }

    #[test]
    fn expand_env_vars_dollar_sign_alone() {
        assert_eq!(expand_env_vars("price is $"), "price is $");
    }

    #[test]
    fn expand_env_vars_multiple_vars() {
        unsafe { std::env::set_var("_TEST_A", "hello") };
        unsafe { std::env::set_var("_TEST_B", "world") };
        let result = expand_env_vars("${_TEST_A} $_TEST_B!");
        assert_eq!(result, "hello world!");
        unsafe { std::env::remove_var("_TEST_A") };
        unsafe { std::env::remove_var("_TEST_B") };
    }

    #[test]
    fn expand_env_vars_empty_braced_preserved() {
        // `${}` should be preserved as-is, not lose the closing `}`
        assert_eq!(expand_env_vars("before ${}after"), "before ${}after");
    }
}

fn build_plugin_variables(
    workspace_path: &str,
    plugin_dir: &Path,
    thread_id: Option<&str>,
) -> HashMap<String, String> {
    let mut values = HashMap::new();
    values.insert("workspace".to_string(), workspace_path.to_string());
    values.insert(
        "plugin_dir".to_string(),
        plugin_dir.to_string_lossy().to_string(),
    );
    if let Some(thread_id) = thread_id {
        values.insert("thread_id".to_string(), thread_id.to_string());
    }
    values
}

fn substitute_variables(input: &str, variables: &HashMap<String, String>) -> String {
    variables
        .iter()
        .fold(input.to_string(), |current, (key, value)| {
            current.replace(&format!("${{{key}}}"), value)
        })
}

fn mask_sensitive_value(value: &str) -> String {
    let len = value.chars().count();
    if len <= 4 {
        return "****".to_string();
    }
    let prefix = value.chars().take(4).collect::<String>();
    format!("{prefix}****")
}

fn mask_url(value: String) -> String {
    let Some((base, _query)) = value.split_once('?') else {
        return value;
    };
    format!("{base}?****")
}
