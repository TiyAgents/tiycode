use super::*;

// --- MCP types ---

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub(super) struct McpConfigFile {
    #[serde(default)]
    pub(super) servers: Vec<McpServerConfigInput>,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub(super) struct McpRuntimeStore {
    #[serde(default)]
    pub(super) items: HashMap<String, McpRuntimeRecord>,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub(super) struct McpRuntimeRecord {
    #[serde(default)]
    pub(super) tools: Vec<McpToolSummaryDto>,
    #[serde(default)]
    pub(super) resources: Vec<McpResourceSummaryDto>,
    pub(super) stale_snapshot: bool,
    pub(super) last_error: Option<String>,
    pub(super) status: Option<String>,
    pub(super) phase: Option<String>,
    pub(super) updated_at: Option<String>,
}

#[derive(Debug, Clone)]
pub(super) struct StreamableHttpSession {
    pub(super) protocol_version: String,
    pub(super) session_id: Option<String>,
}

// --- MCP impl methods ---

impl ExtensionsManager {
    pub(super) async fn resolve_mcp_scope(
        &self,
        id: &str,
        workspace_path: Option<&str>,
    ) -> ConfigScope {
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
        // Intentionally no workspace→global fallback: if the MCP does not exist
        // at the requested scope, editing it should not silently materialize a
        // copy in a different config file. Callers that actually intend to add
        // a workspace-level override should go through `add_mcp_server`.
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

    pub(super) async fn collect_mcp_summaries(
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

    pub(super) async fn load_mcp_configs_for_scope(
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

    pub(super) async fn load_mcp_configs_with_scope(
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

    pub(super) async fn save_mcp_configs_for_scope(
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

    pub(super) async fn load_mcp_runtime_store(&self) -> Result<McpRuntimeStore, AppError> {
        self.read_json_setting(EXTENSIONS_MCP_RUNTIME_KEY).await
    }

    pub(super) async fn save_mcp_runtime_store(
        &self,
        store: &McpRuntimeStore,
    ) -> Result<(), AppError> {
        self.write_json_setting(EXTENSIONS_MCP_RUNTIME_KEY, store)
            .await
    }

    pub(super) fn validate_mcp_input(&self, input: &McpServerConfigInput) -> Result<(), AppError> {
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

    pub(super) async fn refresh_mcp_runtime(
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

    pub(super) fn build_mcp_state(
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

    pub(super) fn build_mcp_summary(&self, server: &McpServerStateDto) -> ExtensionSummaryDto {
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

    pub(super) fn mask_mcp_config(&self, input: &McpServerConfigInput) -> McpServerConfigDto {
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

    pub(super) async fn execute_mcp_tool(
        &self,
        server_id: &str,
        tool: &McpToolSummaryDto,
        tool_input: &serde_json::Value,
        workspace_path: &str,
    ) -> Result<ToolOutput, AppError> {
        tracing::info!(server_id, tool = %tool.name, "MCP tool execution starting");
        tracing::debug!(server_id, tool = %tool.name, %tool_input, "MCP tool execution input");
        let config = self
            .load_mcp_configs_with_scope(Some(workspace_path), ConfigScope::Workspace)
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

    pub(super) async fn probe_mcp_runtime(
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

    pub(super) async fn probe_stdio_mcp_runtime(
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

    pub(super) async fn probe_streamable_http_mcp_runtime(
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

    pub(super) async fn call_streamable_http_mcp_tool_once(
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

    pub(super) async fn call_mcp_tool_once(
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

    pub(super) async fn with_stdio_mcp_client<T, F>(
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

    pub(super) async fn remove_mcp_runtime_records_with_prefix(
        &self,
        prefix: &str,
    ) -> Result<(), AppError> {
        let mut store = self.load_mcp_runtime_store().await?;
        let before = store.items.len();
        store.items.retain(|id, _| !id.starts_with(prefix));
        if before == store.items.len() {
            return Ok(());
        }
        self.save_mcp_runtime_store(&store).await
    }

    pub(super) async fn update_mcp_enabled(
        &self,
        id: &str,
        enabled: bool,
        workspace_path: Option<&str>,
        scope: ConfigScope,
    ) -> Result<bool, AppError> {
        // Only update the config at the caller-supplied scope. We intentionally
        // do not fall back to cloning a globally-defined MCP entry into the
        // workspace config file: the install-location rule says toggling a
        // user-level MCP must stay at the user level. If the id isn't found in
        // the requested scope, return `Ok(false)` so `enable_extension` /
        // `disable_extension` can continue trying other scopes or extension
        // kinds without silently shadowing the entry.
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

        Ok(false)
    }
}

// --- MCP free functions ---

pub(super) fn global_mcp_path() -> PathBuf {
    tiy_home().join("mcp.json")
}

pub(super) fn workspace_mcp_path(workspace_path: Option<&str>) -> Result<PathBuf, AppError> {
    let workspace_path = workspace_path.ok_or_else(|| {
        AppError::validation(
            ErrorSource::Settings,
            "workspace path is required for workspace-scoped MCP config",
        )
    })?;
    Ok(PathBuf::from(workspace_path).join(".tiy/mcp.local.json"))
}

pub(super) fn legacy_workspace_mcp_path(workspace_path: Option<&str>) -> Result<PathBuf, AppError> {
    let workspace_path = workspace_path.ok_or_else(|| {
        AppError::validation(
            ErrorSource::Settings,
            "workspace path is required for workspace-scoped MCP config",
        )
    })?;
    Ok(PathBuf::from(workspace_path).join(".tiy/mcp.json"))
}

pub(super) fn workspace_mcp_read_path(workspace_path: Option<&str>) -> Result<PathBuf, AppError> {
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

pub(super) fn compare_mcp_server_states(
    left: &McpServerStateDto,
    right: &McpServerStateDto,
) -> Ordering {
    right
        .config
        .enabled
        .cmp(&left.config.enabled)
        .then_with(|| left.label.to_lowercase().cmp(&right.label.to_lowercase()))
        .then_with(|| left.id.cmp(&right.id))
}

pub(super) fn canonicalize_mcp_transport(transport: &str) -> &'static str {
    match transport.trim().to_ascii_lowercase().as_str() {
        "http-streamable" | "streamable-http" => "streamable-http",
        "stdio" => "stdio",
        _ => "",
    }
}

pub(super) fn canonicalize_mcp_config(mut config: McpServerConfigInput) -> McpServerConfigInput {
    let transport = canonicalize_mcp_transport(&config.transport);
    if !transport.is_empty() {
        config.transport = transport.to_string();
    }
    config
}

pub(super) fn merge_masked_string_map(
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

pub(super) fn merge_mcp_sensitive_fields(
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

pub(super) fn build_streamable_http_client(
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
pub(super) fn login_shell_env() -> &'static std::collections::HashMap<String, String> {
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
            cmd.args(["-l", "-i", "-c", "env"]);
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
                    tracing::info!(PATH = %path, "login_shell_env: captured PATH");
                }
            }
            env
        }
    })
}

/// Resolves an environment variable by name, first checking the process
/// environment, then falling back to the cached login shell environment.
pub(super) fn resolve_env_var(name: &str) -> Option<String> {
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
pub(super) fn expand_env_vars(input: &str) -> String {
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

pub(super) fn build_streamable_http_headers(
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

pub(super) async fn initialize_streamable_http_session(
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

pub(super) async fn call_streamable_http_mcp_method(
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

pub(super) async fn send_streamable_http_notification(
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

pub(super) async fn close_streamable_http_session(
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
pub(super) struct StreamableHttpJsonRpcResponse {
    pub(super) session_id: Option<String>,
    pub(super) body: serde_json::Value,
}

pub(super) async fn send_streamable_http_jsonrpc_request(
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

pub(super) fn extract_streamable_http_jsonrpc_result(
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

pub(super) fn parse_streamable_http_sse_payload(
    payload: &str,
) -> Result<serde_json::Value, AppError> {
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

pub(super) async fn spawn_stdio_mcp_process(
    config: &McpServerConfigInput,
    workspace_path: Option<&str>,
) -> Result<tokio::process::Child, AppError> {
    let configured_program = config.command.as_deref().unwrap_or_default().trim();
    let program = resolve_command_path(configured_program)
        .await
        .unwrap_or_else(|| PathBuf::from(configured_program));
    tracing::info!(
        server = %config.label,
        configured = %configured_program,
        resolved = %program.display(),
        "spawn_stdio_mcp_process: resolved command path"
    );
    let mut command = Command::new(&program);
    configure_background_tokio_command(&mut command);
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

pub(super) async fn initialize_mcp_session(
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

pub(super) async fn call_stdio_mcp_method(
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

pub(super) async fn write_stdio_mcp_message(
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

pub(super) async fn read_stdio_mcp_message(
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

pub(super) fn parse_content_length_header(line: &str) -> Option<usize> {
    let (name, value) = line.split_once(':')?;
    if !name.trim().eq_ignore_ascii_case("content-length") {
        return None;
    }
    value.trim().parse::<usize>().ok()
}

pub(super) fn message_id_matches(message: &serde_json::Value, id: u64) -> bool {
    message
        .get("id")
        .and_then(serde_json::Value::as_u64)
        .map(|message_id| message_id == id)
        .unwrap_or(false)
}

pub(super) fn mcp_capability_enabled(capabilities: &serde_json::Value, key: &str) -> bool {
    capabilities
        .as_object()
        .and_then(|map| map.get(key))
        .is_some()
}

pub(super) fn mcp_runtime_record_needs_refresh(
    server_id: &str,
    runtime: &McpRuntimeRecord,
) -> bool {
    runtime.tools.iter().any(|tool| {
        tool.qualified_name != build_mcp_runtime_tool_name(server_id, &tool.name)
            || !mcp_tool_name_is_provider_safe(&tool.qualified_name)
    })
}

pub(super) fn mcp_runtime_record_is_disabled(runtime: &McpRuntimeRecord) -> bool {
    matches!(
        runtime.status.as_deref(),
        Some("disconnected") | Some("error")
    ) || matches!(runtime.phase.as_deref(), Some("shutdown"))
}

pub(super) fn mcp_tool_name_is_provider_safe(name: &str) -> bool {
    !name.is_empty()
        && name
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '_' || ch == '-')
}

pub(super) fn parse_mcp_tools(
    result: &serde_json::Value,
    server_id: &str,
) -> Vec<McpToolSummaryDto> {
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

pub(super) fn build_mcp_runtime_tool_name(server_id: &str, tool_name: &str) -> String {
    let server_segment = server_id.rsplit("::").next().unwrap_or(server_id);
    let server = sanitize_mcp_runtime_name_segment(server_segment);
    let tool = sanitize_mcp_runtime_name_segment(tool_name);
    format!("__mcp_{}_{}", server, tool)
}

pub(super) fn sanitize_mcp_runtime_name_segment(value: &str) -> String {
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

pub(super) fn parse_mcp_resources(result: &serde_json::Value) -> Vec<McpResourceSummaryDto> {
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

pub(super) fn append_mcp_stderr(mut error: AppError, stderr_output: &str) -> AppError {
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
