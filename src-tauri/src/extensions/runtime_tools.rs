use super::*;

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
