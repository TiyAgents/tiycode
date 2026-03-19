use std::collections::HashMap;
use std::sync::Mutex as StdMutex;
use std::sync::Arc;

use sqlx::SqlitePool;
use tokio::sync::Mutex;
use tiy_core::agent::{
    Agent, AgentMessage, AgentTool, AgentToolResult, ToolExecutionMode,
};
use tiy_core::types::{ContentBlock, TextContent};

use crate::core::agent_session::ResolvedModelRole;
use crate::core::executors::ToolOutput;
use crate::core::tool_gateway::{
    ToolExecutionOptions, ToolExecutionRequest, ToolGateway, ToolGatewayResult,
};
use crate::ipc::frontend_channels::ThreadStreamEvent;
use crate::model::errors::{AppError, ErrorSource};
use crate::persistence::repo::{run_helper_repo, tool_call_repo};

pub struct HelperRunRequest {
    pub run_id: String,
    pub thread_id: String,
    pub helper_kind: String,
    pub parent_tool_call_id: Option<String>,
    pub task: String,
    pub model_role: ResolvedModelRole,
    pub system_prompt: String,
    pub workspace_path: String,
    pub run_mode: String,
    pub event_tx: tokio::sync::mpsc::UnboundedSender<ThreadStreamEvent>,
}

pub struct HelperRunResult {
    pub summary: String,
}

pub struct HelperAgentOrchestrator {
    pool: SqlitePool,
    tool_gateway: Arc<ToolGateway>,
    active_helpers: Arc<Mutex<HashMap<String, Vec<Arc<Agent>>>>>,
}

impl HelperAgentOrchestrator {
    pub fn new(pool: SqlitePool, tool_gateway: Arc<ToolGateway>) -> Self {
        Self {
            pool,
            tool_gateway,
            active_helpers: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub async fn run_helper(
        &self,
        request: HelperRunRequest,
    ) -> Result<HelperRunResult, AppError> {
        let helper_profile = resolve_helper_profile(&request.helper_kind)?;
        let helper_id = uuid::Uuid::now_v7().to_string();
        let resolved_helper_kind = helper_profile.kind.to_string();
        let escalation_summary = Arc::new(StdMutex::new(None::<String>));

        run_helper_repo::insert(
            &self.pool,
            &run_helper_repo::RunHelperInsert {
                id: helper_id.clone(),
                run_id: request.run_id.clone(),
                thread_id: request.thread_id.clone(),
                helper_kind: resolved_helper_kind.clone(),
                parent_tool_call_id: request.parent_tool_call_id.clone(),
                status: "running".to_string(),
                model_role: "assistant".to_string(),
                provider_id: Some(request.model_role.provider_id.clone()),
                model_id: Some(request.model_role.model_id.clone()),
                input_summary: Some(request.task.clone()),
            },
        )
        .await?;

        let _ = request.event_tx.send(ThreadStreamEvent::SubagentStarted {
            run_id: request.run_id.clone(),
            subtask_id: helper_id.clone(),
            helper_kind: resolved_helper_kind.clone(),
        });

        let agent = Arc::new(Agent::with_model(request.model_role.model.clone()));
        agent.set_system_prompt(format!(
            "{}\n\n{}\n\nProduce a concise summary for the parent agent.",
            request.system_prompt, helper_profile.system_prompt
        ));
        agent.set_tools(helper_tools_for_profile(helper_profile.kind));
        agent.set_tool_execution(ToolExecutionMode::Sequential);

        if let Some(api_key) = request.model_role.api_key.clone() {
            agent.set_api_key(api_key);
        }

        if let Some(provider_options) = request.model_role.provider_options.clone() {
            agent.set_on_payload(move |payload, _model| {
                let provider_options = provider_options.clone();
                Box::pin(async move { Some(merge_payload(payload, &provider_options)) })
            });
        }

        let helper_pool = self.pool.clone();
        let helper_gateway = Arc::clone(&self.tool_gateway);
        let helper_run_id = request.run_id.clone();
        let helper_thread_id = request.thread_id.clone();
        let helper_workspace_path = request.workspace_path.clone();
        let helper_run_mode = request.run_mode.clone();
        let helper_profile_kind = resolved_helper_kind.clone();
        let helper_id_for_tools = helper_id.clone();
        let helper_agent = Arc::clone(&agent);
        let escalation_summary_ref = Arc::clone(&escalation_summary);
        agent.set_tool_executor(move |tool_name, tool_call_id, tool_input, _update_cb| {
            let tool_name = tool_name.to_string();
            let tool_input = tool_input.clone();
            let persisted_tool_call_id = format!("{helper_id_for_tools}:{tool_call_id}");
            let helper_pool = helper_pool.clone();
            let helper_gateway = Arc::clone(&helper_gateway);
            let helper_run_id = helper_run_id.clone();
            let helper_thread_id = helper_thread_id.clone();
            let helper_workspace_path = helper_workspace_path.clone();
            let helper_run_mode = helper_run_mode.clone();
            let helper_profile_kind = helper_profile_kind.clone();
            let helper_agent = Arc::clone(&helper_agent);
            let escalation_summary_ref = Arc::clone(&escalation_summary_ref);

            async move {
                if let Err(error) = tool_call_repo::insert(
                    &helper_pool,
                    &tool_call_repo::ToolCallInsert {
                        id: persisted_tool_call_id.clone(),
                        run_id: helper_run_id.clone(),
                        thread_id: helper_thread_id.clone(),
                        tool_name: tool_name.clone(),
                        tool_input_json: tool_input.to_string(),
                        status: "requested".to_string(),
                    },
                )
                .await
                {
                    return helper_agent_error_result(format!(
                        "failed to persist helper tool call: {error}"
                    ));
                }

                let request = ToolExecutionRequest {
                    run_id: helper_run_id.clone(),
                    thread_id: helper_thread_id.clone(),
                    tool_call_id: persisted_tool_call_id.clone(),
                    tool_name: tool_name.clone(),
                    tool_input: tool_input.clone(),
                    workspace_path: helper_workspace_path.clone(),
                    run_mode: helper_run_mode.clone(),
                };

                match helper_gateway
                    .execute_tool_call(
                        request,
                        tiy_core::agent::AbortSignal::new(),
                        ToolExecutionOptions {
                            allow_user_approval: false,
                        },
                        |_| {},
                        || {},
                    )
                    .await
                {
                    Ok(outcome) => match outcome.result {
                        ToolGatewayResult::Executed { output, .. } => {
                            helper_agent_tool_result_from_output(output)
                        }
                        ToolGatewayResult::Denied { reason, .. } => helper_agent_error_result(reason),
                        ToolGatewayResult::EscalationRequired { reason, .. } => {
                            let summary = format!(
                                "Escalation required from the parent run: `{}` cannot complete this step because {}. Ask the parent run to perform or approve it directly.",
                                helper_profile_kind, reason
                            );
                            if let Ok(mut slot) = escalation_summary_ref.lock() {
                                *slot = Some(summary.clone());
                            }
                            helper_agent.abort();
                            helper_agent_error_result(summary)
                        }
                        ToolGatewayResult::Cancelled { .. } => {
                            helper_agent_error_result("Helper tool execution cancelled")
                        }
                    },
                    Err(error) => helper_agent_error_result(error.to_string()),
                }
            }
        });

        {
            let mut helpers = self.active_helpers.lock().await;
            helpers
                .entry(request.run_id.clone())
                .or_default()
                .push(Arc::clone(&agent));
        }

        let result = agent.prompt(request.task.clone()).await;
        self.remove_helper(&request.run_id, &agent).await;

        if let Some(summary) = take_escalation_summary(&escalation_summary) {
            run_helper_repo::mark_completed(&self.pool, &helper_id, &summary).await?;

            let _ = request.event_tx.send(ThreadStreamEvent::SubagentCompleted {
                run_id: request.run_id,
                subtask_id: helper_id,
                helper_kind: resolved_helper_kind.clone(),
                summary: Some(summary.clone()),
            });

            return Ok(HelperRunResult { summary });
        }

        match result {
            Ok(messages) => {
                let summary = extract_summary(&messages)
                    .unwrap_or_else(|| "Helper completed without a textual summary.".to_string());

                run_helper_repo::mark_completed(&self.pool, &helper_id, &summary).await?;

                let _ = request.event_tx.send(ThreadStreamEvent::SubagentCompleted {
                    run_id: request.run_id,
                    subtask_id: helper_id,
                    helper_kind: resolved_helper_kind,
                    summary: Some(summary.clone()),
                });

                Ok(HelperRunResult { summary })
            }
            Err(error) => {
                let interrupted = error.to_string().to_lowercase().contains("aborted");
                run_helper_repo::mark_failed(&self.pool, &helper_id, &error.to_string(), interrupted)
                    .await?;

                let _ = request.event_tx.send(ThreadStreamEvent::SubagentFailed {
                    run_id: request.run_id,
                    subtask_id: helper_id,
                    helper_kind: resolved_helper_kind,
                    error: error.to_string(),
                });

                Err(AppError::internal(
                    ErrorSource::Thread,
                    format!("helper execution failed: {error}"),
                ))
            }
        }
    }

    pub async fn cancel_run(&self, run_id: &str) {
        let helpers = {
            let mut active = self.active_helpers.lock().await;
            active.remove(run_id).unwrap_or_default()
        };

        for helper in helpers {
            helper.abort();
        }
    }

    async fn remove_helper(&self, run_id: &str, helper: &Arc<Agent>) {
        let mut active = self.active_helpers.lock().await;
        if let Some(helpers) = active.get_mut(run_id) {
            helpers.retain(|candidate| !Arc::ptr_eq(candidate, helper));
            if helpers.is_empty() {
                active.remove(run_id);
            }
        }
    }
}

fn extract_summary(messages: &[AgentMessage]) -> Option<String> {
    messages.iter().rev().find_map(|message| match message {
        AgentMessage::Assistant(message) => {
            let text = message.text_content();
            if text.trim().is_empty() {
                None
            } else {
                Some(text)
            }
        }
        _ => None,
    })
}

fn merge_payload(mut base: serde_json::Value, patch: &serde_json::Value) -> serde_json::Value {
    merge_json_value(&mut base, patch);
    base
}

fn merge_json_value(base: &mut serde_json::Value, patch: &serde_json::Value) {
    match (base, patch) {
        (serde_json::Value::Object(base_map), serde_json::Value::Object(patch_map)) => {
            for (key, patch_value) in patch_map {
                if let Some(base_value) = base_map.get_mut(key) {
                    merge_json_value(base_value, patch_value);
                } else {
                    base_map.insert(key.clone(), patch_value.clone());
                }
            }
        }
        (base_value, patch_value) => {
            *base_value = patch_value.clone();
        }
    }
}

struct HelperProfile {
    kind: &'static str,
    system_prompt: &'static str,
}

fn resolve_helper_profile(helper_kind: &str) -> Result<HelperProfile, AppError> {
    let profile = match helper_kind {
        "delegate_research" => HelperProfile {
            kind: "helper_scout",
            system_prompt: "You are an internal scout helper. Stay read-only, inspect the workspace with allowed tools, and summarize only the findings that matter to the parent run.",
        },
        "delegate_plan_review" => HelperProfile {
            kind: "helper_planner",
            system_prompt: "You are an internal planning helper. Stay read-only, inspect relevant files, and return concise risks, gaps, and next-step suggestions for the parent run.",
        },
        "delegate_code_review" => HelperProfile {
            kind: "helper_reviewer",
            system_prompt: "You are an internal review helper. Stay read-only, use allowed repository inspection tools, and optionally inspect read-only terminal state when it directly supports the review.",
        },
        other => {
            return Err(AppError::recoverable(
                ErrorSource::Thread,
                "thread.helper.unsupported_kind",
                format!("Unsupported helper kind: {other}"),
            ));
        }
    };

    Ok(profile)
}

fn helper_tools_for_profile(profile_kind: &str) -> Vec<AgentTool> {
    let mut tools = vec![
        AgentTool::new(
            "read_file",
            "Read File",
            "Read a file inside the current workspace.",
            serde_json::json!({
                "type": "object",
                "properties": { "path": { "type": "string" } },
                "required": ["path"]
            }),
        ),
        AgentTool::new(
            "list_dir",
            "List Directory",
            "List files and folders inside the current workspace.",
            serde_json::json!({
                "type": "object",
                "properties": { "path": { "type": "string" } }
            }),
        ),
        AgentTool::new(
            "search_repo",
            "Search Repo",
            "Search the current workspace with ripgrep.",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "query": { "type": "string" },
                    "directory": { "type": "string" },
                    "filePattern": { "type": "string" }
                },
                "required": ["query"]
            }),
        ),
    ];

    if profile_kind == "helper_reviewer" {
        tools.extend([
            AgentTool::new(
                "terminal_get_status",
                "Terminal Status",
                "Inspect the current thread terminal status without mutating it.",
                serde_json::json!({
                    "type": "object",
                    "properties": {}
                }),
            ),
            AgentTool::new(
                "terminal_get_recent_output",
                "Terminal Output",
                "Read the recent terminal output for the current thread.",
                serde_json::json!({
                    "type": "object",
                    "properties": {}
                }),
            ),
        ]);
    }

    tools
}

fn take_escalation_summary(summary: &Arc<StdMutex<Option<String>>>) -> Option<String> {
    summary.lock().ok().and_then(|mut slot| slot.take())
}

fn helper_agent_tool_result_from_output(output: ToolOutput) -> AgentToolResult {
    let rendered =
        serde_json::to_string_pretty(&output.result).unwrap_or_else(|_| output.result.to_string());

    if output.success {
        AgentToolResult {
            content: vec![ContentBlock::Text(TextContent::new(rendered))],
            details: Some(output.result),
        }
    } else {
        AgentToolResult {
            content: vec![ContentBlock::Text(TextContent::new(format!("Error: {rendered}")))],
            details: Some(output.result),
        }
    }
}

fn helper_agent_error_result(message: impl Into<String>) -> AgentToolResult {
    AgentToolResult {
        content: vec![ContentBlock::Text(TextContent::new(format!(
            "Error: {}",
            message.into()
        )))],
        details: None,
    }
}
