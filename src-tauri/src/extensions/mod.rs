mod config_io;
mod marketplace;
mod mcp;
mod plugins;
mod skills;

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

#[cfg(test)]
use config_io::config_diagnostic_id;
use marketplace::*;
use mcp::*;
use plugins::*;
use skills::*;

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

    pub async fn list_extension_commands(&self) -> Result<Vec<ExtensionCommandDto>, AppError> {
        let mut commands = self.load_registered_plugin_commands().await?;
        commands.sort_by(|left, right| left.name.cmp(&right.name));
        Ok(commands)
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

    pub async fn resolve_tool(
        &self,
        tool_name: &str,
        workspace_path: Option<&str>,
    ) -> Result<Option<ResolvedTool>, AppError> {
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

        let scope = if workspace_path
            .map(|path| !path.trim().is_empty())
            .unwrap_or(false)
        {
            ConfigScope::Workspace
        } else {
            ConfigScope::Global
        };
        for server in self.list_mcp_servers(workspace_path, scope).await? {
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

#[cfg(test)]
fn compare_marketplace_items(left: &MarketplaceItemDto, right: &MarketplaceItemDto) -> Ordering {
    right
        .enabled
        .cmp(&left.enabled)
        .then_with(|| right.installed.cmp(&left.installed))
        .then_with(|| left.name.to_lowercase().cmp(&right.name.to_lowercase()))
        .then_with(|| left.id.cmp(&right.id))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};
    use tempfile::tempdir;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::{TcpListener, TcpStream};

    #[test]
    fn config_scope_parses_workspace_and_defaults_to_global() {
        assert_eq!(ConfigScope::from_option(None), ConfigScope::Global);
        assert_eq!(
            ConfigScope::from_option(Some("workspace")),
            ConfigScope::Workspace
        );
        assert_eq!(
            ConfigScope::from_option(Some("unknown")),
            ConfigScope::Global
        );
        assert_eq!(ConfigScope::from_str("workspace"), ConfigScope::Workspace);
        assert_eq!(ConfigScope::from_str("global"), ConfigScope::Global);
        assert_eq!(ConfigScope::Global.as_str(), "global");
        assert_eq!(ConfigScope::Workspace.as_str(), "workspace");
    }

    fn mcp_config(transport: &str) -> McpServerConfigInput {
        McpServerConfigInput {
            id: "server".to_string(),
            label: "Server".to_string(),
            transport: transport.to_string(),
            enabled: true,
            auto_start: true,
            command: Some("node".to_string()),
            args: Some(vec!["server.js".to_string()]),
            env: Some(HashMap::from([(
                "TOKEN".to_string(),
                "super-secret".to_string(),
            )])),
            cwd: Some("/tmp/server".to_string()),
            url: Some("https://example.com/mcp?token=secret".to_string()),
            headers: Some(HashMap::from([(
                "Authorization".to_string(),
                "Bearer test-token".to_string(),
            )])),
            timeout_ms: Some(5_000),
        }
    }

    #[tokio::test]
    async fn validate_mcp_input_accepts_supported_transports_and_rejects_invalid_configs() {
        let manager = ExtensionsManager::new(
            SqlitePool::connect("sqlite::memory:")
                .await
                .expect("sqlite pool"),
        );

        assert!(manager.validate_mcp_input(&mcp_config("stdio")).is_ok());
        assert!(manager
            .validate_mcp_input(&mcp_config("http-streamable"))
            .is_ok());

        let mut missing_identity = mcp_config("stdio");
        missing_identity.id = "  ".to_string();
        assert_eq!(
            manager
                .validate_mcp_input(&missing_identity)
                .unwrap_err()
                .user_message,
            "MCP id and label are required"
        );

        let mut missing_command = mcp_config("stdio");
        missing_command.command = Some("  ".to_string());
        assert_eq!(
            manager
                .validate_mcp_input(&missing_command)
                .unwrap_err()
                .user_message,
            "stdio MCP servers require a command"
        );

        let mut missing_url = mcp_config("streamable-http");
        missing_url.url = None;
        assert_eq!(
            manager
                .validate_mcp_input(&missing_url)
                .unwrap_err()
                .user_message,
            "streamable-http MCP servers require a URL"
        );

        let unsupported = mcp_config("sse");
        assert_eq!(
            manager
                .validate_mcp_input(&unsupported)
                .unwrap_err()
                .user_message,
            "Unsupported MCP transport"
        );
    }

    #[tokio::test]
    async fn mask_mcp_config_redacts_sensitive_values_and_preserves_shape() {
        let manager = ExtensionsManager::new(
            SqlitePool::connect("sqlite::memory:")
                .await
                .expect("sqlite pool"),
        );
        let masked = manager.mask_mcp_config(&mcp_config("streamable-http"));

        assert_eq!(masked.id, "server");
        assert_eq!(masked.label, "Server");
        assert_eq!(masked.transport, "streamable-http");
        assert!(masked.enabled);
        assert_eq!(masked.command.as_deref(), Some("node"));
        assert_eq!(masked.args, vec!["server.js".to_string()]);
        assert_eq!(masked.cwd.as_deref(), Some("/tmp/server"));
        assert_eq!(masked.timeout_ms, Some(5_000));
        assert_ne!(
            masked.env.get("TOKEN").map(String::as_str),
            Some("super-secret")
        );
        assert_ne!(
            masked.headers.get("Authorization").map(String::as_str),
            Some("Bearer test-token")
        );
        assert_ne!(
            masked.url.as_deref(),
            Some("https://example.com/mcp?token=secret")
        );
    }

    #[tokio::test]
    async fn build_mcp_state_maps_disabled_invalid_not_started_connected_and_degraded() {
        let manager = ExtensionsManager::new(
            SqlitePool::connect("sqlite::memory:")
                .await
                .expect("sqlite pool"),
        );

        let mut disabled = mcp_config("stdio");
        disabled.enabled = false;
        let state = manager.build_mcp_state(&disabled, None, ConfigScope::Workspace.as_str());
        assert_eq!(state.status, "disconnected");
        assert_eq!(state.phase, "shutdown");
        assert_eq!(state.scope, "workspace");

        let mut invalid = mcp_config("stdio");
        invalid.command = None;
        let state = manager.build_mcp_state(&invalid, None, ConfigScope::Global.as_str());
        assert_eq!(state.status, "config_error");
        assert_eq!(state.phase, "config_load");
        assert_eq!(
            state.last_error.as_deref(),
            Some("stdio MCP servers require a command")
        );

        let state = manager.build_mcp_state(&mcp_config("stdio"), None, "global");
        assert_eq!(state.status, "disconnected");
        assert_eq!(state.phase, "not_started");

        let connected_runtime = McpRuntimeRecord {
            tools: vec![McpToolSummaryDto {
                name: "lookup".to_string(),
                qualified_name: "__mcp_server_lookup".to_string(),
                description: Some("Lookup docs".to_string()),
                input_schema: Some(serde_json::json!({ "type": "object" })),
            }],
            resources: vec![McpResourceSummaryDto {
                uri: "file:///docs/readme.md".to_string(),
                name: "README".to_string(),
                description: None,
                mime_type: Some("text/markdown".to_string()),
            }],
            stale_snapshot: false,
            last_error: None,
            status: Some("connected".to_string()),
            phase: Some("ready".to_string()),
            updated_at: Some("2026-04-25T00:00:00Z".to_string()),
        };
        let state =
            manager.build_mcp_state(&mcp_config("stdio"), Some(&connected_runtime), "global");
        assert_eq!(state.status, "connected");
        assert_eq!(state.phase, "ready");
        assert_eq!(state.tools.len(), 1);
        assert_eq!(state.resources.len(), 1);
        assert_eq!(state.updated_at, "2026-04-25T00:00:00Z");

        let degraded_runtime = McpRuntimeRecord {
            stale_snapshot: true,
            last_error: Some("probe failed".to_string()),
            status: Some("error".to_string()),
            phase: Some("runtime_probe".to_string()),
            ..connected_runtime
        };
        let state =
            manager.build_mcp_state(&mcp_config("stdio"), Some(&degraded_runtime), "global");
        assert_eq!(state.status, "degraded");
        assert_eq!(state.phase, "runtime_probe");
        assert!(state.stale_snapshot);
        assert_eq!(state.last_error.as_deref(), Some("probe failed"));
    }

    #[tokio::test]
    async fn extension_builders_render_plugin_mcp_skill_and_marketplace_dtos() {
        let manager = ExtensionsManager::new(
            SqlitePool::connect("sqlite::memory:")
                .await
                .expect("sqlite pool"),
        );
        let plugin_dir = tempdir().expect("plugin dir");
        let commands_dir = plugin_dir.path().join("commands");
        let skills_dir = plugin_dir.path().join("skills/alpha-skill");
        fs::create_dir_all(&commands_dir).expect("commands dir");
        fs::create_dir_all(&skills_dir).expect("skills dir");
        fs::write(commands_dir.join("fix.md"), "Fix it").expect("command file");
        fs::write(
            skills_dir.join("SKILL.md"),
            "---\nname: Alpha Skill\ntags:\n- docs\n---\nSkill body",
        )
        .expect("skill file");
        fs::write(
            plugin_dir.path().join(".mcp.json"),
            serde_json::json!({
                "servers": [
                    { "name": "Docs Server" },
                    { "id": "api-server", "label": "API Server" }
                ]
            })
            .to_string(),
        )
        .expect("mcp bundle");

        let manifest = PluginManifest {
            id: "demo.plugin".to_string(),
            name: "Demo Plugin".to_string(),
            version: "1.0.0".to_string(),
            description: Some("Demo description".to_string()),
            author: Some("Ada".to_string()),
            homepage: Some("https://example.com".to_string()),
            default_enabled: Some(false),
            capabilities: vec!["tools".to_string(), "commands".to_string()],
            permissions: vec!["shell".to_string()],
            hooks: Some(PluginManifestHooks {
                pre_tool_use: Some(vec!["pre".to_string()]),
                post_tool_use: Some(vec!["post".to_string()]),
                on_run_start: Some(vec!["start".to_string()]),
                on_run_complete: Some(vec!["done".to_string()]),
            }),
            tools: vec![PluginManifestTool {
                name: "run".to_string(),
                description: "Run a task".to_string(),
                command: "node".to_string(),
                args: vec!["tool.js".to_string()],
                env: None,
                cwd: Some("tools".to_string()),
                timeout_ms: Some(1234),
                required_permission: "shell".to_string(),
            }],
            commands: vec![PluginManifestCommand {
                name: "review".to_string(),
                description: "Review code".to_string(),
                prompt_template: Some("Review this".to_string()),
            }],
            timeout_ms: Some(30_000),
            skills_dir: Some("skills".to_string()),
            config_schema: Some(PluginManifestSchema {
                r#type: "json-schema".to_string(),
                path: "schema.json".to_string(),
            }),
        };
        let runtime = InstalledPluginRuntime {
            manifest: manifest.clone(),
            path: plugin_dir.path().to_path_buf(),
            enabled: true,
        };

        let summary = manager.build_plugin_summary(
            &runtime,
            ExtensionInstallState::Enabled,
            Some("ignored because description exists".to_string()),
        );
        assert_eq!(summary.kind, ExtensionKind::Plugin);
        assert_eq!(summary.health, ExtensionHealth::Healthy);
        assert_eq!(summary.description.as_deref(), Some("Demo description"));
        assert_eq!(summary.permissions, vec!["shell".to_string()]);
        assert_eq!(
            summary.tags,
            vec!["tools".to_string(), "commands".to_string()]
        );
        assert!(matches!(
            summary.source,
            ExtensionSourceDto::LocalDir { .. }
        ));

        let error_summary = manager.build_plugin_summary(
            &InstalledPluginRuntime {
                manifest: PluginManifest {
                    description: None,
                    ..manifest.clone()
                },
                ..runtime.clone()
            },
            ExtensionInstallState::Error,
            Some("load failed".to_string()),
        );
        assert_eq!(error_summary.health, ExtensionHealth::Error);
        assert_eq!(error_summary.description.as_deref(), Some("load failed"));

        let detail = manager.build_plugin_detail(&runtime, Some("previous error".to_string()));
        assert_eq!(detail.author.as_deref(), Some("Ada"));
        assert!(!detail.default_enabled);
        assert!(detail.enabled);
        assert_eq!(detail.hooks.len(), 4);
        assert_eq!(detail.tools[0].required_permission, "shell");
        assert_eq!(
            detail
                .commands
                .iter()
                .map(|command| command.name.as_str())
                .collect::<Vec<_>>(),
            vec!["fix", "review"]
        );
        assert_eq!(detail.bundled_skills, vec!["Alpha Skill".to_string()]);
        assert_eq!(
            detail.bundled_mcp_servers,
            vec!["API Server".to_string(), "Docs Server".to_string()]
        );
        assert_eq!(detail.config_schema_path.as_deref(), Some("schema.json"));
        assert_eq!(detail.last_error.as_deref(), Some("previous error"));

        let mcp_state = |status: &str, enabled: bool, stale_snapshot: bool, transport: &str| {
            McpServerStateDto {
                id: format!("docs-{status}"),
                label: "Docs".to_string(),
                scope: "global".to_string(),
                status: status.to_string(),
                phase: "ready".to_string(),
                tools: Vec::new(),
                resources: Vec::new(),
                stale_snapshot,
                last_error: Some("runtime note".to_string()),
                updated_at: "now".to_string(),
                config: McpServerConfigDto {
                    id: format!("docs-{status}"),
                    label: "Docs".to_string(),
                    transport: transport.to_string(),
                    enabled,
                    auto_start: true,
                    command: None,
                    args: Vec::new(),
                    env: HashMap::new(),
                    cwd: None,
                    url: Some("https://example.com/mcp".to_string()),
                    headers: HashMap::new(),
                    timeout_ms: None,
                },
            }
        };
        let connected =
            manager.build_mcp_summary(&mcp_state("connected", true, false, "streamable-http"));
        assert_eq!(connected.install_state, ExtensionInstallState::Enabled);
        assert_eq!(connected.health, ExtensionHealth::Healthy);
        assert_eq!(connected.permissions, vec!["network-access".to_string()]);
        let degraded = manager.build_mcp_summary(&mcp_state("degraded", true, true, "stdio"));
        assert_eq!(degraded.health, ExtensionHealth::Degraded);
        assert!(degraded.tags.contains(&"stale-snapshot".to_string()));
        assert_eq!(degraded.permissions, vec!["shell-exec".to_string()]);
        let config_error =
            manager.build_mcp_summary(&mcp_state("config_error", true, false, "stdio"));
        assert_eq!(config_error.install_state, ExtensionInstallState::Error);
        assert_eq!(config_error.health, ExtensionHealth::Error);
        let disabled = manager.build_mcp_summary(&mcp_state("disconnected", false, false, "stdio"));
        assert_eq!(disabled.install_state, ExtensionInstallState::Disabled);
        assert_eq!(disabled.health, ExtensionHealth::Unknown);

        let skill = SkillRecordDto {
            id: "skill-alpha".to_string(),
            name: "Alpha Skill".to_string(),
            description: Some("Helps with docs".to_string()),
            tags: vec!["docs".to_string()],
            triggers: Vec::new(),
            tools: Vec::new(),
            priority: None,
            source: "workspace".to_string(),
            path: "/workspace/.tiy/skills/alpha".to_string(),
            enabled: false,
            scope: "workspace".to_string(),
            content_preview: "preview".to_string(),
            prompt_budget_chars: 7,
        };
        let skill_summary = manager.build_skill_summary(&skill);
        assert_eq!(skill_summary.kind, ExtensionKind::Skill);
        assert_eq!(skill_summary.install_state, ExtensionInstallState::Disabled);
        assert_eq!(skill_summary.health, ExtensionHealth::Healthy);
        assert!(matches!(
            skill_summary.source,
            ExtensionSourceDto::LocalDir { .. }
        ));
        assert_eq!(skill_summary.tags, vec!["docs".to_string()]);

        let marketplace = manager.build_marketplace_source_dto(
            &MarketplaceSourceRecord {
                id: "custom-source".to_string(),
                name: "Custom Source".to_string(),
                url: "https://example.com/catalog.git".to_string(),
                kind: "git".to_string(),
                last_synced_at: Some("2026-04-25T00:00:00Z".to_string()),
                last_error: None,
            },
            Some(3),
        );
        assert_eq!(marketplace.status, "ready");
        assert_eq!(marketplace.plugin_count, 3);
        assert!(!marketplace.builtin);
        let marketplace_error = manager.build_marketplace_source_dto(
            &MarketplaceSourceRecord {
                id: "custom-source".to_string(),
                name: "Custom Source".to_string(),
                url: "https://example.com/catalog.git".to_string(),
                kind: "git".to_string(),
                last_synced_at: None,
                last_error: Some("git failed".to_string()),
            },
            None,
        );
        assert_eq!(marketplace_error.status, "error");
        assert_eq!(marketplace_error.plugin_count, 0);
    }

    #[test]
    fn plugin_manifest_helpers_parse_aliases_and_optional_sections() {
        let plugin_dir = Path::new("/plugins/demo");
        let manifest = parse_plugin_manifest(
            r#"{
              "id": "demo.plugin",
              "name": "Demo Plugin",
              "version": "1.2.3",
              "summary": "Demo summary",
              "author": { "name": "Ada" },
              "repository": "https://example.com/repo.git",
              "default_enabled": false,
              "capabilities": ["tools", "tools", "commands"],
              "permissions": ["shell", "network"],
              "hooks": { "pre_tool_use": ["pre"], "postToolUse": ["post"] },
              "tools": [
                { "name": "run", "description": "Run command", "command": "node", "args": ["tool.js"], "permission": "shell" }
              ],
              "commands": [
                { "name": "review", "description": "Review code", "prompt": "Review this" }
              ],
              "timeout_ms": 42,
              "skills_dir": "skills",
              "config_schema": { "path": "schema.json", "type": "json-schema" }
            }"#,
            plugin_dir,
        )
        .expect("manifest");

        assert_eq!(manifest.id, "demo.plugin");
        assert_eq!(manifest.description.as_deref(), Some("Demo summary"));
        assert_eq!(manifest.author.as_deref(), Some("Ada"));
        assert_eq!(
            manifest.homepage.as_deref(),
            Some("https://example.com/repo.git")
        );
        assert_eq!(manifest.default_enabled, Some(false));
        assert_eq!(
            manifest.capabilities,
            vec!["commands".to_string(), "tools".to_string()]
        );
        assert_eq!(
            manifest.permissions,
            vec!["network".to_string(), "shell".to_string()]
        );
        assert!(manifest
            .hooks
            .as_ref()
            .and_then(|hooks| hooks.pre_tool_use.as_ref())
            .is_some());
        assert_eq!(manifest.tools[0].required_permission, "read");
        assert_eq!(manifest.commands[0].description, "Review code");
        assert_eq!(manifest.timeout_ms, Some(42));
        assert_eq!(manifest.skills_dir.as_deref(), Some("skills"));
        assert_eq!(
            manifest
                .config_schema
                .as_ref()
                .map(|schema| schema.path.as_str()),
            Some("schema.json")
        );

        let invalid = parse_plugin_manifest("{not json", plugin_dir).unwrap_err();
        assert!(invalid.contains("manifest is not valid JSON"));
    }

    #[test]
    fn low_level_manifest_readers_trim_deduplicate_and_ignore_invalid_values() {
        let object = serde_json::json!({
            "name": "  Demo  ",
            "empty": "   ",
            "args": [" b ", 12, "a", "a", ""],
            "env": { "TOKEN": " secret ", "EMPTY": " ", "NUMBER": 12 },
            "enabled": true,
            "timeout_ms": 120,
            "author": { "label": "Grace" },
            "hooks": { "onRunStart": ["start", "start"], "on_run_complete": ["done"] },
            "schema": { "path": "schema.json" }
        })
        .as_object()
        .unwrap()
        .clone();

        assert_eq!(
            read_string_keys(&object, &["missing", "name"]).as_deref(),
            Some("Demo")
        );
        assert_eq!(read_string_keys(&object, &["empty"]), None);
        assert_eq!(
            read_string_array_keys(&object, &["args"]),
            vec!["a".to_string(), "b".to_string()]
        );
        assert_eq!(
            read_string_map_keys(&object, &["env"])
                .unwrap()
                .get("TOKEN")
                .map(String::as_str),
            Some("secret")
        );
        assert_eq!(read_bool_keys(&object, &["enabled"]), Some(true));
        assert_eq!(read_u64_keys(&object, &["timeout_ms"]), Some(120));
        assert_eq!(
            read_author_name(object.get("author")).as_deref(),
            Some("Grace")
        );
        assert_eq!(
            read_plugin_hooks(object.get("hooks"))
                .and_then(|hooks| hooks.on_run_complete)
                .unwrap(),
            vec!["done".to_string()]
        );
        assert_eq!(
            read_config_schema(object.get("schema")).map(|schema| schema.r#type),
            Some("json-schema".to_string())
        );
    }

    #[test]
    fn mcp_runtime_parsers_sort_and_sanitize_tool_and_resource_names() {
        let result = serde_json::json!({
            "tools": [
                { "name": "z tool", "description": "Zed", "inputSchema": { "type": "object" } },
                { "name": "alpha", "description": "Alpha" },
                { "name": "  " },
                { "description": "missing name" }
            ],
            "resources": [
                { "uri": "file:///z", "name": "Zed", "mimeType": "text/plain" },
                { "uri": "file:///a", "description": "No name" },
                { "uri": "  " }
            ]
        });

        let tools = parse_mcp_tools(&result, "plugin::server one");
        assert_eq!(
            tools
                .iter()
                .map(|tool| tool.name.as_str())
                .collect::<Vec<_>>(),
            vec!["alpha", "z tool"]
        );
        assert_eq!(tools[1].qualified_name, "__mcp_server_one_z_tool");
        assert!(mcp_tool_name_is_provider_safe(&tools[0].qualified_name));
        assert!(!mcp_tool_name_is_provider_safe("bad name"));
        assert_eq!(sanitize_mcp_runtime_name_segment(" !! "), "tool");

        let resources = parse_mcp_resources(&result);
        assert_eq!(
            resources
                .iter()
                .map(|resource| resource.name.as_str())
                .collect::<Vec<_>>(),
            vec!["file:///a", "Zed"]
        );
        assert_eq!(resources[0].description.as_deref(), Some("No name"));
        assert_eq!(resources[1].mime_type.as_deref(), Some("text/plain"));
    }

    #[test]
    fn streamable_http_response_helpers_parse_arrays_sse_and_errors() {
        let array_payload = serde_json::json!([
            { "jsonrpc": "2.0", "id": 1, "result": { "ok": false } },
            { "jsonrpc": "2.0", "id": 2, "result": { "ok": true } }
        ]);
        assert_eq!(
            extract_streamable_http_jsonrpc_result(&array_payload, 2, "tools/list").unwrap(),
            serde_json::json!({ "ok": true })
        );

        let wrong_id = serde_json::json!({ "jsonrpc": "2.0", "id": 3, "result": {} });
        assert_eq!(
            extract_streamable_http_jsonrpc_result(&wrong_id, 2, "tools/list")
                .unwrap_err()
                .error_code,
            "extensions.mcp.read_failed"
        );

        let rpc_error = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 2,
            "error": { "code": -32601, "message": "missing" }
        });
        let error =
            extract_streamable_http_jsonrpc_result(&rpc_error, 2, "tools/list").unwrap_err();
        assert_eq!(error.error_code, "extensions.mcp.rpc_error");
        assert!(error.user_message.contains("missing"));

        let sse = "event: message\ndata: {\"jsonrpc\":\"2.0\",\"id\":1,\"result\":{\"a\":1}}\n\ndata: {\"jsonrpc\":\"2.0\",\"id\":2,\"result\":{\"b\":2}}\n\n";
        let parsed = parse_streamable_http_sse_payload(sse).unwrap();
        assert_eq!(parsed.as_array().unwrap().len(), 2);
        assert_eq!(
            parse_streamable_http_sse_payload("event: ping\n\n")
                .unwrap_err()
                .error_code,
            "extensions.mcp.read_failed"
        );
    }

    #[test]
    fn config_path_and_marketplace_helpers_are_stable() {
        let workspace = tempdir().expect("workspace");
        assert!(workspace_mcp_path(Some(workspace.path().to_str().unwrap()))
            .unwrap()
            .ends_with(".tiy/mcp.local.json"));
        assert_eq!(
            workspace_mcp_path(None).unwrap_err().error_code,
            "settings.validation"
        );

        let diagnostic_id =
            config_diagnostic_id(Path::new("/tmp/config.json"), "mcp", ConfigScope::Workspace);
        assert!(diagnostic_id.starts_with("workspace:mcp:"));

        let builtins = builtin_marketplace_sources();
        assert_eq!(builtins.len(), 1);
        assert!(is_builtin_marketplace_source_id(&builtins[0].id));
        let id = marketplace_source_id("https://example.com/catalog.git");
        assert!(!id.is_empty());
        assert!(id.chars().all(|ch| ch.is_ascii_hexdigit()));
    }

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
            dunce::canonicalize(plugin_dir.path()).expect("canonical plugin path")
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
