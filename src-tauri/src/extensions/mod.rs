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
