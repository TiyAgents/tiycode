mod config_io;
mod marketplace;
mod mcp;
mod plugins;
mod runtime_tools;
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

pub use runtime_tools::{ResolvedTool, ToolProviderContext};

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
