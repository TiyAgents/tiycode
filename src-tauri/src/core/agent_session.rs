use std::collections::HashMap;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex as StdMutex, Weak};

use sqlx::SqlitePool;
use tiy_core::agent::{Agent, AgentMessage, AgentTool, AgentToolResult, ToolExecutionMode};
use tiy_core::thinking::ThinkingLevel;
use tiy_core::types::{
    AssistantMessage, AssistantMessageEvent, ContentBlock, Cost, InputType, Model, Provider,
    StopReason, TextContent, Transport, Usage, UserMessage,
};
use tokio::sync::mpsc;

use crate::core::subagent::{
    runtime_orchestration_tools, HelperAgentOrchestrator, HelperRunRequest,
    RuntimeOrchestrationTool, SubagentProfile,
};
use crate::core::tool_gateway::{
    ApprovalRequest, ToolExecutionOptions, ToolExecutionRequest, ToolGateway, ToolGatewayResult,
};
use crate::ipc::frontend_channels::ThreadStreamEvent;
use crate::model::errors::{AppError, ErrorSource};
use crate::model::provider::AgentProfileRecord;
use crate::model::thread::{MessageRecord, RunUsageDto};
use crate::persistence::repo::{
    message_repo, profile_repo, provider_repo, settings_repo, tool_call_repo,
};

const MESSAGE_HISTORY_LIMIT: i64 = 200;
const DEFAULT_CONTEXT_WINDOW: u32 = 128_000;
const DEFAULT_MAX_OUTPUT_TOKENS: u32 = 32_000;
const DEFAULT_FULL_TOOL_PROFILE: &str = "default_full";
const PLAN_READ_ONLY_TOOL_PROFILE: &str = "plan_read_only";
const STANDARD_TOOL_TIMEOUT_SECS: u64 = 120;
const SUBAGENT_TOOL_TIMEOUT_SECS: u64 = 600;
const WORKSPACE_INSTRUCTION_FILE_NAMES: &[&str] = &["AGENTS.md", "CLAUDE.md", "AGENT.MD"];
const WORKSPACE_INSTRUCTION_MAX_CHARS: usize = 12_800;
const SHELL_GUIDE_TOOL_NAMES: &[&str] = &["python3", "python", "node", "npm", "uv", "git", "rg"];

#[derive(Debug, Clone)]
struct WorkspaceInstructionSnippet {
    file_name: &'static str,
    content: String,
    truncated: bool,
}

#[derive(Debug, Clone)]
struct ToolAvailability {
    name: &'static str,
    path: Option<PathBuf>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProfileResponseStyle {
    Balanced,
    Concise,
    Guide,
}

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeModelRole {
    pub provider_id: String,
    pub model_record_id: String,
    pub provider: Option<String>,
    pub provider_key: Option<String>,
    pub provider_type: String,
    pub provider_name: Option<String>,
    pub model: String,
    pub model_id: String,
    pub model_display_name: Option<String>,
    pub base_url: String,
    pub context_window: Option<String>,
    pub max_output_tokens: Option<String>,
    pub custom_headers: Option<HashMap<String, String>>,
    pub provider_options: Option<serde_json::Value>,
}

#[derive(Debug, Clone, serde::Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeModelPlan {
    pub profile_id: Option<String>,
    pub profile_name: Option<String>,
    pub custom_instructions: Option<String>,
    pub response_style: Option<String>,
    pub response_language: Option<String>,
    pub primary: Option<RuntimeModelRole>,
    pub auxiliary: Option<RuntimeModelRole>,
    pub lightweight: Option<RuntimeModelRole>,
    pub thinking_level: Option<String>,
    pub transport: Option<String>,
    pub tool_profile_by_mode: Option<serde_json::Value>,
}

#[derive(Debug, Clone)]
pub struct ResolvedModelRole {
    pub provider_id: String,
    pub model_record_id: String,
    pub model_id: String,
    pub model_name: String,
    pub provider_type: String,
    pub provider_name: String,
    pub api_key: Option<String>,
    pub provider_options: Option<serde_json::Value>,
    pub model: Model,
}

#[derive(Debug, Clone)]
pub struct ResolvedRuntimeModelPlan {
    pub raw: RuntimeModelPlan,
    pub primary: ResolvedModelRole,
    pub auxiliary: Option<ResolvedModelRole>,
    pub lightweight: Option<ResolvedModelRole>,
    pub thinking_level: ThinkingLevel,
    pub transport: Transport,
}

#[derive(Debug, Clone)]
pub struct AgentSessionSpec {
    pub run_id: String,
    pub thread_id: String,
    pub workspace_path: String,
    pub run_mode: String,
    pub tool_profile_name: String,
    pub system_prompt: String,
    pub history_messages: Vec<MessageRecord>,
    pub model_plan: ResolvedRuntimeModelPlan,
}

pub async fn build_session_spec(
    pool: &SqlitePool,
    run_id: &str,
    thread_id: &str,
    workspace_path: &str,
    run_mode: &str,
    model_plan_value: &serde_json::Value,
) -> Result<AgentSessionSpec, AppError> {
    let raw_plan: RuntimeModelPlan =
        serde_json::from_value(model_plan_value.clone()).unwrap_or_default();
    let resolved_plan = resolve_model_plan(pool, raw_plan.clone()).await?;
    let history_messages =
        message_repo::list_recent(pool, thread_id, None, MESSAGE_HISTORY_LIMIT).await?;
    let system_prompt = build_system_prompt(pool, &raw_plan, workspace_path, run_mode).await?;

    Ok(AgentSessionSpec {
        run_id: run_id.to_string(),
        thread_id: thread_id.to_string(),
        workspace_path: workspace_path.to_string(),
        run_mode: run_mode.to_string(),
        tool_profile_name: resolve_tool_profile_name(&raw_plan, run_mode),
        system_prompt,
        history_messages,
        model_plan: resolved_plan,
    })
}

pub struct AgentSession {
    spec: AgentSessionSpec,
    pool: SqlitePool,
    tool_gateway: Arc<ToolGateway>,
    helper_orchestrator: Arc<HelperAgentOrchestrator>,
    event_tx: mpsc::UnboundedSender<ThreadStreamEvent>,
    agent: Arc<Agent>,
    cancel_requested: Arc<AtomicBool>,
    abort_signal: tiy_core::agent::AbortSignal,
}

impl AgentSession {
    pub fn new(
        pool: SqlitePool,
        tool_gateway: Arc<ToolGateway>,
        helper_orchestrator: Arc<HelperAgentOrchestrator>,
        event_tx: mpsc::UnboundedSender<ThreadStreamEvent>,
        spec: AgentSessionSpec,
    ) -> Arc<Self> {
        Arc::new_cyclic(|weak_self| {
            let agent = Arc::new(Agent::with_model(spec.model_plan.primary.model.clone()));
            configure_agent(&agent, &spec, weak_self.clone());

            Self {
                spec,
                pool,
                tool_gateway,
                helper_orchestrator,
                event_tx,
                agent,
                cancel_requested: Arc::new(AtomicBool::new(false)),
                abort_signal: tiy_core::agent::AbortSignal::new(),
            }
        })
    }

    pub fn start(self: Arc<Self>) {
        tokio::spawn(async move {
            self.run().await;
        });
    }

    pub async fn cancel(&self) {
        self.cancel_requested.store(true, Ordering::SeqCst);
        self.abort_signal.cancel();
        self.helper_orchestrator.cancel_run(&self.spec.run_id).await;
        self.agent.abort();
    }

    async fn run(self: Arc<Self>) {
        let current_message_id = Arc::new(StdMutex::new(None::<String>));
        let current_reasoning_message_id = Arc::new(StdMutex::new(None::<String>));
        let last_usage = Arc::new(StdMutex::new(None::<Usage>));
        let reasoning_buffer = Arc::new(StdMutex::new(String::new()));
        let run_id = self.spec.run_id.clone();
        let event_tx = self.event_tx.clone();
        let context_window = self
            .spec
            .model_plan
            .primary
            .model
            .context_window
            .to_string();
        let model_display_name = self.spec.model_plan.primary.model_name.clone();

        let message_id_ref = Arc::clone(&current_message_id);
        let reasoning_message_id_ref = Arc::clone(&current_reasoning_message_id);
        let last_usage_ref = Arc::clone(&last_usage);
        let reasoning_ref = Arc::clone(&reasoning_buffer);
        let unsubscribe = self.agent.subscribe(move |event| {
            handle_agent_event(
                &run_id,
                &event_tx,
                &message_id_ref,
                &reasoning_message_id_ref,
                &last_usage_ref,
                &reasoning_ref,
                &context_window,
                &model_display_name,
                event,
            );
        });

        let _ = self.event_tx.send(ThreadStreamEvent::RunStarted {
            run_id: self.spec.run_id.clone(),
            run_mode: self.spec.run_mode.clone(),
        });

        let result = self.agent.continue_().await;
        unsubscribe();

        match result {
            Ok(_) => {
                if self.cancel_requested.load(Ordering::SeqCst) {
                    let _ = self.event_tx.send(ThreadStreamEvent::RunCancelled {
                        run_id: self.spec.run_id.clone(),
                    });
                } else {
                    let _ = self.event_tx.send(ThreadStreamEvent::RunCompleted {
                        run_id: self.spec.run_id.clone(),
                    });
                }
            }
            Err(error) => {
                if self.cancel_requested.load(Ordering::SeqCst) {
                    let _ = self.event_tx.send(ThreadStreamEvent::RunCancelled {
                        run_id: self.spec.run_id.clone(),
                    });
                } else {
                    let _ = self.event_tx.send(ThreadStreamEvent::RunFailed {
                        run_id: self.spec.run_id.clone(),
                        error: error.to_string(),
                    });
                }
            }
        }
    }

    async fn execute_tool_call(
        &self,
        tool_name: &str,
        tool_call_id: &str,
        tool_input: &serde_json::Value,
    ) -> AgentToolResult {
        let insert_result = tool_call_repo::insert(
            &self.pool,
            &tool_call_repo::ToolCallInsert {
                id: tool_call_id.to_string(),
                run_id: self.spec.run_id.clone(),
                thread_id: self.spec.thread_id.clone(),
                tool_name: tool_name.to_string(),
                tool_input_json: tool_input.to_string(),
                status: "requested".to_string(),
            },
        )
        .await;

        if let Err(error) = insert_result {
            return agent_error_result(format!("failed to persist tool call: {error}"));
        }

        if let Some(tool) = RuntimeOrchestrationTool::parse(tool_name) {
            return self
                .execute_helper_tool(tool, tool_call_id, tool_input)
                .await;
        }

        let _ = self.event_tx.send(ThreadStreamEvent::ToolRequested {
            run_id: self.spec.run_id.clone(),
            tool_call_id: tool_call_id.to_string(),
            tool_name: tool_name.to_string(),
            tool_input: tool_input.clone(),
        });

        let request = ToolExecutionRequest {
            run_id: self.spec.run_id.clone(),
            thread_id: self.spec.thread_id.clone(),
            tool_call_id: tool_call_id.to_string(),
            tool_name: tool_name.to_string(),
            tool_input: tool_input.clone(),
            workspace_path: self.spec.workspace_path.clone(),
            run_mode: self.spec.run_mode.clone(),
        };

        let event_tx = self.event_tx.clone();
        let run_id = self.spec.run_id.clone();
        let tool_call_id_owned = tool_call_id.to_string();
        let tool_timeout = standard_tool_timeout();
        let outcome = tokio::time::timeout(
            tool_timeout,
            self.tool_gateway.execute_tool_call(
                request,
                self.abort_signal.clone(),
                ToolExecutionOptions::default(),
                move |approval: ApprovalRequest| {
                    let _ = event_tx.send(ThreadStreamEvent::ApprovalRequired {
                        run_id: approval.run_id,
                        tool_call_id: approval.tool_call_id,
                        tool_name: approval.tool_name,
                        tool_input: approval.tool_input,
                        reason: approval.reason,
                    });
                },
                {
                    let event_tx = self.event_tx.clone();
                    let run_id = run_id.clone();
                    let tool_call_id = tool_call_id_owned.clone();
                    move || {
                        let _ = event_tx.send(ThreadStreamEvent::ToolRunning {
                            run_id: run_id.clone(),
                            tool_call_id: tool_call_id.clone(),
                        });
                    }
                },
            ),
        )
        .await;

        let outcome = match outcome {
            Ok(outcome) => outcome,
            Err(_) => {
                let message = format!(
                    "Tool '{}' timed out after {}s",
                    tool_name, STANDARD_TOOL_TIMEOUT_SECS
                );
                let result = serde_json::json!({ "error": message.clone() });
                tool_call_repo::update_result(
                    &self.pool,
                    tool_call_id,
                    &result.to_string(),
                    "failed",
                )
                .await
                .ok();

                let _ = self.event_tx.send(ThreadStreamEvent::ToolFailed {
                    run_id: self.spec.run_id.clone(),
                    tool_call_id: tool_call_id.to_string(),
                    error: message.clone(),
                });

                return agent_error_result(message);
            }
        };

        match outcome {
            Ok(outcome) => {
                if outcome.approval_required {
                    let approved = matches!(outcome.result, ToolGatewayResult::Executed { .. });
                    let _ = self.event_tx.send(ThreadStreamEvent::ApprovalResolved {
                        run_id: self.spec.run_id.clone(),
                        tool_call_id: tool_call_id.to_string(),
                        approved,
                    });
                }

                match outcome.result {
                    ToolGatewayResult::Executed { output, .. } => {
                        let _ = self.event_tx.send(ThreadStreamEvent::ToolCompleted {
                            run_id: self.spec.run_id.clone(),
                            tool_call_id: tool_call_id.to_string(),
                            result: output.result.clone(),
                        });
                        agent_tool_result_from_output(output)
                    }
                    ToolGatewayResult::Denied { reason, .. } => {
                        let _ = self.event_tx.send(ThreadStreamEvent::ToolFailed {
                            run_id: self.spec.run_id.clone(),
                            tool_call_id: tool_call_id.to_string(),
                            error: reason.clone(),
                        });
                        agent_error_result(reason)
                    }
                    ToolGatewayResult::EscalationRequired { reason, .. } => {
                        let _ = self.event_tx.send(ThreadStreamEvent::ToolFailed {
                            run_id: self.spec.run_id.clone(),
                            tool_call_id: tool_call_id.to_string(),
                            error: reason.clone(),
                        });
                        agent_error_result(reason)
                    }
                    ToolGatewayResult::Cancelled { .. } => {
                        let message = "Tool execution cancelled".to_string();
                        let _ = self.event_tx.send(ThreadStreamEvent::ToolFailed {
                            run_id: self.spec.run_id.clone(),
                            tool_call_id: tool_call_id.to_string(),
                            error: message.clone(),
                        });
                        agent_error_result(message)
                    }
                }
            }
            Err(error) => {
                let _ = self.event_tx.send(ThreadStreamEvent::ToolFailed {
                    run_id: self.spec.run_id.clone(),
                    tool_call_id: tool_call_id.to_string(),
                    error: error.to_string(),
                });
                agent_error_result(error.to_string())
            }
        }
    }

    async fn execute_helper_tool(
        &self,
        tool: RuntimeOrchestrationTool,
        tool_call_id: &str,
        tool_input: &serde_json::Value,
    ) -> AgentToolResult {
        let task = tool_input
            .get("task")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .trim()
            .to_string();

        if task.is_empty() {
            tool_call_repo::update_result(
                &self.pool,
                tool_call_id,
                &serde_json::json!({ "error": "missing helper task" }).to_string(),
                "failed",
            )
            .await
            .ok();
            return agent_error_result("missing helper task");
        }

        let helper_role = self
            .spec
            .model_plan
            .auxiliary
            .clone()
            .unwrap_or_else(|| self.spec.model_plan.primary.clone());
        let helper_profile = resolve_helper_profile(tool, tool_input);

        let result = self
            .helper_orchestrator
            .run_helper(HelperRunRequest {
                run_id: self.spec.run_id.clone(),
                thread_id: self.spec.thread_id.clone(),
                tool,
                helper_profile: Some(helper_profile),
                parent_tool_call_id: Some(tool_call_id.to_string()),
                task,
                model_role: helper_role,
                system_prompt: self.spec.system_prompt.clone(),
                workspace_path: self.spec.workspace_path.clone(),
                run_mode: self.spec.run_mode.clone(),
                event_tx: self.event_tx.clone(),
            })
            .await;

        match result {
            Ok(summary) => {
                let result = serde_json::json!({
                    "summary": summary.summary.clone(),
                    "snapshot": summary.snapshot,
                });
                tool_call_repo::update_result(
                    &self.pool,
                    tool_call_id,
                    &result.to_string(),
                    "completed",
                )
                .await
                .ok();

                AgentToolResult {
                    content: vec![ContentBlock::Text(TextContent::new(summary.summary))],
                    details: Some(result),
                }
            }
            Err(error) => {
                tool_call_repo::update_result(
                    &self.pool,
                    tool_call_id,
                    &serde_json::json!({ "error": error.to_string() }).to_string(),
                    "failed",
                )
                .await
                .ok();

                agent_error_result(error.to_string())
            }
        }
    }
}

fn configure_agent(agent: &Arc<Agent>, spec: &AgentSessionSpec, weak_self: Weak<AgentSession>) {
    agent.set_system_prompt(spec.system_prompt.clone());
    agent.replace_messages(convert_history_messages(
        &spec.history_messages,
        &spec.model_plan.primary.model,
    ));
    agent.set_tools(runtime_tools_for_profile(&spec.tool_profile_name));
    agent.set_tool_execution(ToolExecutionMode::Sequential);
    agent.set_thinking_level(spec.model_plan.thinking_level);
    agent.set_transport(spec.model_plan.transport);
    agent.set_security_config(runtime_security_config());

    // Context compression: automatically trim messages to fit the context window.
    let compression_settings = crate::core::context_compression::CompressionSettings::new(
        spec.model_plan.primary.model.context_window,
    );
    agent.set_transform_context(move |messages| {
        let settings = compression_settings.clone();
        async move { crate::core::context_compression::compress_context(messages, &settings) }
    });

    if let Some(api_key) = spec.model_plan.primary.api_key.clone() {
        agent.set_api_key(api_key);
    }

    if let Some(provider_options) = spec.model_plan.primary.provider_options.clone() {
        agent.set_on_payload(move |payload, _model| {
            let provider_options = provider_options.clone();
            Box::pin(async move { Some(merge_payload(payload, &provider_options)) })
        });
    }

    agent.set_tool_executor(move |tool_name, tool_call_id, tool_input, _update_cb| {
        let weak_self = weak_self.clone();
        let tool_name = tool_name.to_string();
        let tool_call_id = tool_call_id.to_string();
        let tool_input = tool_input.clone();

        async move {
            match weak_self.upgrade() {
                Some(session) => {
                    session
                        .execute_tool_call(&tool_name, &tool_call_id, &tool_input)
                        .await
                }
                None => agent_error_result("agent session unavailable"),
            }
        }
    });
}

fn handle_agent_event(
    run_id: &str,
    event_tx: &mpsc::UnboundedSender<ThreadStreamEvent>,
    current_message_id: &StdMutex<Option<String>>,
    current_reasoning_message_id: &StdMutex<Option<String>>,
    last_usage: &StdMutex<Option<Usage>>,
    reasoning_buffer: &StdMutex<String>,
    context_window: &str,
    model_display_name: &str,
    event: &tiy_core::agent::AgentEvent,
) {
    match event {
        tiy_core::agent::AgentEvent::MessageUpdate {
            assistant_event, ..
        } => {
            match assistant_event.as_ref() {
                AssistantMessageEvent::TextDelta { delta, .. } => {
                    let message_id = ensure_message_id(current_message_id);
                    let _ = event_tx.send(ThreadStreamEvent::MessageDelta {
                        run_id: run_id.to_string(),
                        message_id,
                        delta: delta.clone(),
                    });
                }
                AssistantMessageEvent::ThinkingStart { .. } => {
                    reset_reasoning_state(current_reasoning_message_id, reasoning_buffer);
                }
                AssistantMessageEvent::ThinkingDelta { delta, .. } => {
                    if let Ok(mut buffer) = reasoning_buffer.lock() {
                        buffer.push_str(delta);
                        let message_id = ensure_message_id(current_reasoning_message_id);
                        let _ = event_tx.send(ThreadStreamEvent::ReasoningUpdated {
                            run_id: run_id.to_string(),
                            message_id,
                            reasoning: buffer.clone(),
                        });
                    }
                }
                AssistantMessageEvent::ThinkingEnd { content, .. } => {
                    let reasoning = if let Ok(mut buffer) = reasoning_buffer.lock() {
                        buffer.clear();
                        buffer.push_str(content);
                        buffer.clone()
                    } else {
                        content.clone()
                    };

                    if reasoning.trim().is_empty() {
                        reset_reasoning_state(current_reasoning_message_id, reasoning_buffer);
                        return;
                    }

                    let message_id = ensure_message_id(current_reasoning_message_id);
                    let _ = event_tx.send(ThreadStreamEvent::ReasoningUpdated {
                        run_id: run_id.to_string(),
                        message_id,
                        reasoning,
                    });
                    reset_reasoning_state(current_reasoning_message_id, reasoning_buffer);
                }
                _ => {}
            }

            if let Some(partial) = assistant_event.partial_message() {
                emit_usage_update_if_changed(
                    run_id,
                    event_tx,
                    last_usage,
                    &partial.usage,
                    context_window,
                    model_display_name,
                );
            }
        }
        tiy_core::agent::AgentEvent::MessageEnd { message } => {
            if let AgentMessage::Assistant(assistant) = message {
                let content = assistant.text_content();
                if content.is_empty() && assistant.has_tool_calls() {
                    emit_usage_update_if_changed(
                        run_id,
                        event_tx,
                        last_usage,
                        &assistant.usage,
                        context_window,
                        model_display_name,
                    );
                    reset_message_id(current_message_id);
                    reset_reasoning_state(current_reasoning_message_id, reasoning_buffer);
                    return;
                }

                emit_usage_update_if_changed(
                    run_id,
                    event_tx,
                    last_usage,
                    &assistant.usage,
                    context_window,
                    model_display_name,
                );
                let message_id = take_or_create_message_id(current_message_id);
                let _ = event_tx.send(ThreadStreamEvent::MessageCompleted {
                    run_id: run_id.to_string(),
                    message_id,
                    content,
                });
            }

            reset_reasoning_state(current_reasoning_message_id, reasoning_buffer);
        }
        _ => {}
    }
}

fn emit_usage_update_if_changed(
    run_id: &str,
    event_tx: &mpsc::UnboundedSender<ThreadStreamEvent>,
    last_usage: &StdMutex<Option<Usage>>,
    usage: &Usage,
    context_window: &str,
    model_display_name: &str,
) {
    let should_emit = if let Ok(mut previous_usage) = last_usage.lock() {
        if previous_usage.as_ref() == Some(usage) {
            return;
        }

        if usage.total_tokens == 0
            && usage.input == 0
            && usage.output == 0
            && usage.cache_read == 0
            && usage.cache_write == 0
        {
            return;
        }

        *previous_usage = Some(*usage);
        true
    } else {
        usage.total_tokens > 0
            || usage.input > 0
            || usage.output > 0
            || usage.cache_read > 0
            || usage.cache_write > 0
    };

    if !should_emit {
        return;
    }

    let _ = event_tx.send(ThreadStreamEvent::ThreadUsageUpdated {
        run_id: run_id.to_string(),
        model_display_name: Some(model_display_name.to_string()),
        context_window: Some(context_window.to_string()),
        usage: RunUsageDto::from(*usage),
    });
}

fn ensure_message_id(current_message_id: &StdMutex<Option<String>>) -> String {
    if let Ok(mut guard) = current_message_id.lock() {
        if let Some(existing) = guard.clone() {
            return existing;
        }

        let message_id = uuid::Uuid::now_v7().to_string();
        *guard = Some(message_id.clone());
        return message_id;
    }

    uuid::Uuid::now_v7().to_string()
}

fn take_or_create_message_id(current_message_id: &StdMutex<Option<String>>) -> String {
    if let Ok(mut guard) = current_message_id.lock() {
        if let Some(existing) = guard.take() {
            return existing;
        }
    }

    uuid::Uuid::now_v7().to_string()
}

fn reset_message_id(current_message_id: &StdMutex<Option<String>>) {
    if let Ok(mut guard) = current_message_id.lock() {
        *guard = None;
    }
}

fn reset_reasoning_state(
    current_reasoning_message_id: &StdMutex<Option<String>>,
    reasoning_buffer: &StdMutex<String>,
) {
    reset_message_id(current_reasoning_message_id);
    if let Ok(mut buffer) = reasoning_buffer.lock() {
        buffer.clear();
    }
}

fn runtime_tools_for_profile(profile_name: &str) -> Vec<AgentTool> {
    let mut tools = vec![
        AgentTool::new(
            "read",
            "Read File",
            "Read a file inside the current workspace.",
            serde_json::json!({
                "type": "object",
                "properties": { "path": { "type": "string" } },
                "required": ["path"]
            }),
        ),
        AgentTool::new(
            "list",
            "List Directory",
            "List files and folders inside the current workspace.",
            serde_json::json!({
                "type": "object",
                "properties": { "path": { "type": "string" } }
            }),
        ),
        AgentTool::new(
            "search",
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
        AgentTool::new(
            "find",
            "Find Files",
            "Search for files by glob pattern. Returns matching file paths relative to the workspace. Respects common ignore patterns (.git, node_modules, target). Output is truncated to 1000 results or 100KB.",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "pattern": {
                        "type": "string",
                        "description": "Glob pattern to match files, e.g. '*.ts', '*.json', '*.spec.ts'"
                    },
                    "path": {
                        "type": "string",
                        "description": "Directory to search in (default: workspace root)"
                    }
                },
                "required": ["pattern"]
            }),
        ),
        AgentTool::new(
            "term_status",
            "Terminal Status",
            "Inspect the current thread terminal status without mutating it.",
            serde_json::json!({
                "type": "object",
                "properties": {}
            }),
        ),
        AgentTool::new(
            "term_output",
            "Terminal Output",
            "Read the recent terminal output for the current thread.",
            serde_json::json!({
                "type": "object",
                "properties": {}
            }),
        ),
    ];
    tools.extend(runtime_orchestration_tools());

    if profile_name == DEFAULT_FULL_TOOL_PROFILE {
        tools.push(AgentTool::new(
            "edit",
            "Edit File",
            "Make a targeted edit to a file by specifying the exact text to find and its replacement. \
             The old_string must uniquely identify the text to replace (appear exactly once in the file). \
             Include enough surrounding context in old_string to make it unique. \
             If old_string is empty, a new file will be created with new_string as content. \
             Supports fuzzy matching for trailing whitespace and Unicode quote/dash differences.",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "Absolute path to the file to edit"
                    },
                    "old_string": {
                        "type": "string",
                        "description": "The exact text to find and replace. Must match exactly once in the file. Use empty string to create a new file."
                    },
                    "new_string": {
                        "type": "string",
                        "description": "The replacement text"
                    }
                },
                "required": ["path", "old_string", "new_string"]
            }),
        ));
        tools.push(AgentTool::new(
            "write",
            "Write File",
            "Write or overwrite a file inside the current workspace.",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string" },
                    "content": { "type": "string" }
                },
                "required": ["path", "content"]
            }),
        ));
        tools.push(AgentTool::new(
            "shell",
            "Run Command",
            "Run a non-interactive shell command inside the current workspace.",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "command": { "type": "string" },
                    "cwd": { "type": "string" },
                    "timeout": { "type": "number" }
                },
                "required": ["command"]
            }),
        ));
    }

    tools
}

fn resolve_tool_profile_name(raw_plan: &RuntimeModelPlan, run_mode: &str) -> String {
    if let Some(profile_name) = raw_plan
        .tool_profile_by_mode
        .as_ref()
        .and_then(|value| value.get(run_mode))
        .and_then(serde_json::Value::as_str)
        .filter(|value| !value.trim().is_empty())
    {
        return profile_name.to_string();
    }

    match run_mode {
        "plan" => PLAN_READ_ONLY_TOOL_PROFILE.to_string(),
        _ => DEFAULT_FULL_TOOL_PROFILE.to_string(),
    }
}

fn resolve_helper_profile(
    tool: RuntimeOrchestrationTool,
    tool_input: &serde_json::Value,
) -> SubagentProfile {
    match tool {
        RuntimeOrchestrationTool::DelegateResearch => SubagentProfile::Scout,
        RuntimeOrchestrationTool::DelegatePlanReview => SubagentProfile::Planner,
        RuntimeOrchestrationTool::DelegateCodeReview => match tool_input
            .get("target")
            .and_then(serde_json::Value::as_str)
            .map(|value| value.trim().to_ascii_lowercase())
            .as_deref()
        {
            Some("plan") => SubagentProfile::Planner,
            _ => SubagentProfile::Reviewer,
        },
    }
}

fn convert_history_messages(messages: &[MessageRecord], model: &Model) -> Vec<AgentMessage> {
    messages
        .iter()
        .filter(|message| message.message_type == "plain_message")
        .filter_map(|message| match message.role.as_str() {
            "user" => Some(AgentMessage::User(UserMessage::text(
                message.content_markdown.clone(),
            ))),
            "assistant" => Some(AgentMessage::Assistant(assistant_message_from_text(
                &message.content_markdown,
                model,
            ))),
            _ => None,
        })
        .collect()
}

fn assistant_message_from_text(content: &str, model: &Model) -> AssistantMessage {
    AssistantMessage::builder()
        .content(vec![ContentBlock::Text(TextContent::new(content))])
        .api(effective_api_for_model(model))
        .provider(model.provider.clone())
        .model(model.id.clone())
        .usage(Usage::default())
        .stop_reason(StopReason::Stop)
        .build()
        .expect("assistant history message should always build")
}

fn effective_api_for_model(model: &Model) -> tiy_core::types::Api {
    if let Some(api) = model.api.clone() {
        return api;
    }

    match &model.provider {
        Provider::OpenAI | Provider::OpenAIResponses | Provider::AzureOpenAIResponses => {
            tiy_core::types::Api::OpenAIResponses
        }
        Provider::Anthropic | Provider::MiniMax | Provider::MiniMaxCN | Provider::KimiCoding => {
            tiy_core::types::Api::AnthropicMessages
        }
        Provider::Google | Provider::GoogleGeminiCli | Provider::GoogleAntigravity => {
            tiy_core::types::Api::GoogleGenerativeAi
        }
        Provider::GoogleVertex => tiy_core::types::Api::GoogleVertex,
        Provider::Ollama => tiy_core::types::Api::Ollama,
        Provider::XAI
        | Provider::Groq
        | Provider::OpenRouter
        | Provider::OpenAICompatible
        | Provider::OpenAICodex
        | Provider::GitHubCopilot
        | Provider::Cerebras
        | Provider::VercelAiGateway
        | Provider::ZAI
        | Provider::Mistral
        | Provider::HuggingFace
        | Provider::OpenCode
        | Provider::OpenCodeGo
        | Provider::DeepSeek
        | Provider::Zenmux => tiy_core::types::Api::OpenAICompletions,
        Provider::AmazonBedrock => tiy_core::types::Api::BedrockConverseStream,
        Provider::Custom(name) => tiy_core::types::Api::Custom(name.clone()),
    }
}

/// Maximum size for a single tool result sent to the LLM (8 MB).
/// OpenAI Responses API enforces a 10 MB limit per `input[n].output` field;
/// this leaves headroom for protocol overhead and JSON escaping.
const MAX_TOOL_RESULT_SIZE: usize = 8_000_000;

fn agent_tool_result_from_output(output: crate::core::executors::ToolOutput) -> AgentToolResult {
    // Use compact JSON (no pretty-print) to reduce whitespace overhead.
    let mut rendered =
        serde_json::to_string(&output.result).unwrap_or_else(|_| output.result.to_string());

    // Hard safety cap — truncate if the serialized result is still too large.
    if rendered.len() > MAX_TOOL_RESULT_SIZE {
        rendered.truncate(MAX_TOOL_RESULT_SIZE);
        // Ensure we don't cut in the middle of a multi-byte UTF-8 char
        while !rendered.is_char_boundary(rendered.len()) {
            rendered.pop();
        }
        rendered.push_str("\n\n[Tool output truncated: exceeded 8MB limit]");
    }

    if output.success {
        AgentToolResult {
            content: vec![ContentBlock::Text(TextContent::new(rendered))],
            details: Some(output.result),
        }
    } else {
        AgentToolResult {
            content: vec![ContentBlock::Text(TextContent::new(format!(
                "Error: {rendered}"
            )))],
            details: Some(output.result),
        }
    }
}

fn agent_error_result(message: impl Into<String>) -> AgentToolResult {
    AgentToolResult {
        content: vec![ContentBlock::Text(TextContent::new(format!(
            "Error: {}",
            message.into()
        )))],
        details: None,
    }
}

async fn resolve_model_plan(
    pool: &SqlitePool,
    raw_plan: RuntimeModelPlan,
) -> Result<ResolvedRuntimeModelPlan, AppError> {
    let primary = raw_plan.primary.clone().ok_or_else(|| {
        AppError::recoverable(
            ErrorSource::Settings,
            "settings.model_plan.primary_missing",
            "Run model plan is missing the primary model",
        )
    })?;

    let primary = resolve_runtime_model_role(pool, primary).await?;
    let auxiliary = match raw_plan.auxiliary.clone() {
        Some(role) => Some(resolve_runtime_model_role(pool, role).await?),
        None => None,
    };
    let lightweight = match raw_plan.lightweight.clone() {
        Some(role) => Some(resolve_runtime_model_role(pool, role).await?),
        None => None,
    };

    Ok(ResolvedRuntimeModelPlan {
        thinking_level: raw_plan
            .thinking_level
            .as_deref()
            .map(ThinkingLevel::from)
            .unwrap_or(ThinkingLevel::Off),
        transport: parse_transport(raw_plan.transport.as_deref()),
        raw: raw_plan,
        primary,
        auxiliary,
        lightweight,
    })
}

pub async fn resolve_runtime_model_role(
    pool: &SqlitePool,
    role: RuntimeModelRole,
) -> Result<ResolvedModelRole, AppError> {
    let provider = provider_repo::find_by_id(pool, &role.provider_id)
        .await?
        .ok_or_else(|| AppError::not_found(ErrorSource::Settings, "provider"))?;

    let provider_name = role
        .provider_name
        .clone()
        .unwrap_or_else(|| provider.display_name.clone());
    let model_name = role
        .model_display_name
        .clone()
        .unwrap_or_else(|| role.model.clone());
    let context_window = parse_positive_u32(role.context_window.as_deref(), DEFAULT_CONTEXT_WINDOW);
    let max_output_tokens =
        parse_positive_u32(role.max_output_tokens.as_deref(), DEFAULT_MAX_OUTPUT_TOKENS);

    let mut builder = Model::builder()
        .id(&role.model)
        .name(&model_name)
        .provider(Provider::from(role.provider_type.clone()))
        .base_url(&role.base_url)
        .context_window(context_window)
        .max_tokens(max_output_tokens)
        .input(vec![InputType::Text])
        .cost(Cost::default());

    if let Some(headers) = role.custom_headers.clone() {
        if !headers.is_empty() {
            builder = builder.headers(headers);
        }
    }

    let model = builder.build().map_err(|error| {
        AppError::internal(
            ErrorSource::Settings,
            format!("failed to build runtime model: {error}"),
        )
    })?;

    Ok(ResolvedModelRole {
        provider_id: role.provider_id,
        model_record_id: role.model_record_id,
        model_id: role.model_id,
        model_name,
        provider_type: role.provider_type,
        provider_name,
        api_key: provider.api_key_encrypted,
        provider_options: normalize_provider_options(role.provider_options),
        model,
    })
}

async fn build_system_prompt(
    pool: &SqlitePool,
    raw_plan: &RuntimeModelPlan,
    workspace_path: &str,
    run_mode: &str,
) -> Result<String, AppError> {
    let mut parts = vec![
        build_prompt_section(
            "Role",
            "You are Tiy Agent, an expert working assistant embedded in the user's desktop workspace.\n\
You help users by reading files, searching code, editing files, executing commands, and writing new files.",
        ),
        build_prompt_section(
            "Behavioral Guidelines",
            "Guidelines:\n\
- Read files before editing. Understand existing code before making changes.\n\
- Use edit for precise, surgical changes. Use write only for new files or complete rewrites.\n\
- Prefer search and find over shell for file exploration — they are faster and respect ignore patterns.\n\
- Delegate proactively on substantial work. When the task is cross-file, unfamiliar, risky, or likely to benefit from a second pass, use a helper instead of doing all exploration and review yourself.\n\
- Use agent_research to investigate unfamiliar areas, collect evidence, map dependencies, or gather the right files before choosing an implementation.\n\
- Use agent_review with target='plan' to stress-test an implementation approach before coding, and with target='code' or target='diff' to review completed work for regressions, edge cases, and consistency.\n\
- Recommended flow for non-trivial tasks: agent_research -> form a plan -> agent_review(target='plan') -> implement -> agent_review(target='code' or 'diff').\n\
- Skip delegation only when the task is small, obvious, and isolated enough that extra helper work would not pay off.\n\
- Be concise in your responses. Show file paths clearly when working with files.\n\
- When summarizing your actions, describe what you did in plain text — do not re-read or re-cat files to prove your work.\n\
- Flag risks, destructive operations, or ambiguity before acting. Ask when intent is unclear.",
        ),
    ];

    if let Some(section) = build_project_context_section(workspace_path) {
        parts.push(section);
    }

    parts.push(build_system_environment_section());
    parts.push(build_sandbox_permissions_section(pool, run_mode, workspace_path).await?);
    parts.push(build_shell_tooling_guide_section());

    let mut profile_lines = Vec::new();
    if let Some(custom_instructions) = raw_plan.custom_instructions.as_deref() {
        let trimmed = custom_instructions.trim();
        if !trimmed.is_empty() {
            profile_lines.push(trimmed.to_string());
        }
    }
    let mut profile_response_parts = build_profile_response_prompt_parts_from_runtime(
        raw_plan.response_language.as_deref(),
        raw_plan.response_style.as_deref(),
    );
    let runtime_has_response_language =
        normalize_profile_response_language(raw_plan.response_language.as_deref()).is_some();
    let runtime_has_explicit_response_style = raw_plan
        .response_style
        .as_deref()
        .map(str::trim)
        .is_some_and(|value| !value.is_empty());

    if let Some(profile_id) = raw_plan.profile_id.as_deref() {
        if let Some(profile) = profile_repo::find_by_id(pool, profile_id).await? {
            if profile_lines.is_empty() {
                if let Some(custom_instructions) = profile.custom_instructions.as_deref() {
                    let trimmed = custom_instructions.trim();
                    if !trimmed.is_empty() {
                        profile_lines.push(trimmed.to_string());
                    }
                }
            }

            if !runtime_has_response_language {
                if let Some(language) =
                    normalize_profile_response_language(profile.response_language.as_deref())
                {
                    profile_response_parts.insert(
                        0,
                        format!(
                            "Respond in {language} unless the user explicitly asks for a different language."
                        ),
                    );
                }
            }

            if !runtime_has_explicit_response_style {
                profile_response_parts = build_profile_response_prompt_parts_from_runtime(
                    if runtime_has_response_language {
                        raw_plan.response_language.as_deref()
                    } else {
                        profile.response_language.as_deref()
                    },
                    profile.response_style.as_deref(),
                );
            }
        }
    }

    profile_lines.extend(profile_response_parts);

    if !profile_lines.is_empty() {
        parts.push(build_prompt_section(
            "Profile Instructions",
            profile_lines.join("\n"),
        ));
    }

    if run_mode == "plan" {
        parts.push(build_prompt_section(
            "Run Mode",
            "Plan mode is active.\n\
- Use only read-only tools: read, list, search, find, term_status, term_output.\n\
- Do NOT use edit, write, or shell unless the user explicitly requests execution.\n\
- Focus on analysis, explanation, and actionable planning. Identify risks, gaps, and concrete next steps.",
        ));
    } else {
        parts.push(build_prompt_section(
            "Run Mode",
            "Default execution mode is active.\n\
- Use the configured tool profile, subject to policy, approvals, and workspace boundaries.\n\
- Prefer the smallest sufficient action that moves the task forward.",
        ));
    }

    // Append runtime context last so the model always sees current date and working directory.
    let date = chrono::Local::now().format("%Y-%m-%d").to_string();
    parts.push(build_prompt_section(
        "Runtime Context",
        format!("Current date: {date}\nWorkspace path: {workspace_path}"),
    ));

    Ok(parts.join("\n\n"))
}

fn build_prompt_section(title: &str, body: impl AsRef<str>) -> String {
    format!("## {title}\n{}", body.as_ref())
}

fn build_project_context_section(workspace_path: &str) -> Option<String> {
    let snippet = collect_workspace_instruction_snippet(workspace_path)?;
    let mut body =
        "Workspace instruction file found at the workspace root. Follow it when relevant."
            .to_string();
    body.push_str("\n\n");
    body.push_str(&format!("### {}\n", snippet.file_name));
    body.push_str("```md\n");
    body.push_str(&snippet.content);
    if snippet.truncated {
        body.push_str("\n[Truncated for prompt size.]");
    }
    body.push_str("\n```");

    Some(build_prompt_section(
        "Project Context (workspace instructions)",
        body,
    ))
}

async fn build_sandbox_permissions_section(
    pool: &SqlitePool,
    run_mode: &str,
    workspace_path: &str,
) -> Result<String, AppError> {
    let approval_policy = settings_repo::policy_get(pool, "approval_policy")
        .await?
        .map(|record| parse_approval_policy_mode(&record.value_json))
        .unwrap_or_else(|| "require_for_mutations".to_string());

    let run_mode_line = if run_mode == "plan" {
        "Plan mode is active, so mutating tools are blocked."
    } else {
        "Default mode is active, so tool use follows the configured approval policy."
    };

    Ok(build_prompt_section(
        "Sandbox & Permissions",
        format!(
            "- Effective runtime sandbox: workspace-scoped tool execution with policy checks.\n- Workspace boundary: file and path-aware tools are restricted to the current workspace (`{workspace_path}`).\n- Approval policy: {approval_policy}.\n- Read-only tools are generally auto-allowed; mutating tools may require approval.\n- {run_mode_line}\n- Outer host sandbox metadata is not exposed here; rely on these effective runtime constraints."
        ),
    ))
}

fn parse_approval_policy_mode(value_json: &str) -> String {
    let parsed: serde_json::Value = serde_json::from_str(value_json).unwrap_or_default();

    if let Some(value) = parsed.as_str() {
        return value.to_string();
    }

    parsed
        .get("mode")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("require_for_mutations")
        .to_string()
}

fn collect_workspace_instruction_snippet(
    workspace_path: &str,
) -> Option<WorkspaceInstructionSnippet> {
    let workspace_root = Path::new(workspace_path);
    if !workspace_root.is_dir() {
        return None;
    }

    WORKSPACE_INSTRUCTION_FILE_NAMES
        .iter()
        .find_map(|file_name| {
            let path = workspace_root.join(file_name);
            if !path.is_file() {
                return None;
            }

            let raw = std::fs::read(&path).ok()?;
            let content = normalize_prompt_doc_content(&String::from_utf8_lossy(&raw));
            if content.is_empty() {
                return None;
            }

            let (content, truncated) = truncate_chars(&content, WORKSPACE_INSTRUCTION_MAX_CHARS);
            Some(WorkspaceInstructionSnippet {
                file_name,
                content,
                truncated,
            })
        })
}

fn normalize_prompt_doc_content(value: &str) -> String {
    value
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

fn truncate_chars(value: &str, max_chars: usize) -> (String, bool) {
    let char_count = value.chars().count();
    if char_count <= max_chars {
        return (value.to_string(), false);
    }

    let truncated = value.chars().take(max_chars).collect::<String>();
    (truncated.trim_end().to_string(), true)
}

fn build_system_environment_section() -> String {
    let shell = current_shell();
    let tool_lines = detect_shell_tools()
        .into_iter()
        .map(|tool| match tool.path {
            Some(path) => format!("- {}: available at {}", tool.name, path.display()),
            None => format!("- {}: not found on PATH", tool.name),
        })
        .collect::<Vec<_>>()
        .join("\n");

    build_prompt_section(
        "System Environment",
        format!(
            "- Operating system: {}\n- Architecture: {}\n- Default shell: {}\n- Common CLI tools:\n{}",
            std::env::consts::OS,
            std::env::consts::ARCH,
            shell,
            tool_lines
        ),
    )
}

fn build_shell_tooling_guide_section() -> String {
    let tool_lookup = detect_shell_tools()
        .into_iter()
        .map(|tool| (tool.name, tool.path.is_some()))
        .collect::<HashMap<_, _>>();

    let python_hint = if tool_lookup.get("python3").copied().unwrap_or(false) {
        "Prefer `python3` for Python commands in shell examples."
    } else if tool_lookup.get("python").copied().unwrap_or(false) {
        "Use `python` for Python commands in shell examples."
    } else {
        "Do not assume Python is available; verify before proposing Python shell commands."
    };

    let node_hint = if tool_lookup.get("node").copied().unwrap_or(false)
        || tool_lookup.get("npm").copied().unwrap_or(false)
    {
        "Node tooling is available. Prefer `npm` scripts when the workspace defines them."
    } else {
        "Do not assume Node tooling is available; verify before proposing Node shell commands."
    };

    let uv_hint = if tool_lookup.get("uv").copied().unwrap_or(false) {
        "Use `uv` for lightweight Python environment and script execution when that fits the task."
    } else {
        "Do not assume `uv` is available."
    };

    let rg_hint = if tool_lookup.get("rg").copied().unwrap_or(false) {
        "Prefer `rg` for text search and file discovery before broader shell commands."
    } else {
        "If `rg` is unavailable, fall back to the built-in search and find tools before broad shell scans."
    };

    let git_hint = if tool_lookup.get("git").copied().unwrap_or(false) {
        "Use `git` for repo status, diff, and history checks when repository context matters."
    } else {
        "Do not assume `git` is available in shell commands."
    };

    build_prompt_section(
        "Shell Tooling Guide",
        format!(
            "- Shell commands run through the user's default shell (`{}`).\n- Prefer workspace-aware tools (`read`, `list`, `search`, `find`, `edit`) before shell when they fit.\n- {}\n- {}\n- {}\n- {}\n- {}",
            current_shell(),
            rg_hint,
            python_hint,
            node_hint,
            uv_hint,
            git_hint
        ),
    )
}

fn current_shell() -> String {
    std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_string())
}

fn detect_shell_tools() -> Vec<ToolAvailability> {
    SHELL_GUIDE_TOOL_NAMES
        .iter()
        .map(|name| ToolAvailability {
            name,
            path: find_command_on_path(name),
        })
        .collect()
}

fn find_command_on_path(command: &str) -> Option<PathBuf> {
    let path_value = std::env::var_os("PATH")?;
    let candidates = executable_candidates(command);

    for directory in std::env::split_paths(&path_value) {
        for candidate in &candidates {
            let path = directory.join(candidate);
            if path.is_file() {
                return Some(path);
            }
        }
    }

    None
}

fn executable_candidates(command: &str) -> Vec<OsString> {
    #[cfg(target_os = "windows")]
    {
        if Path::new(command).extension().is_some() {
            return vec![OsString::from(command)];
        }

        let pathext =
            std::env::var_os("PATHEXT").unwrap_or_else(|| OsString::from(".COM;.EXE;.BAT;.CMD"));
        let mut candidates = vec![OsString::from(command)];

        for ext in pathext.to_string_lossy().split(';') {
            let trimmed = ext.trim();
            if trimmed.is_empty() {
                continue;
            }
            candidates.push(OsString::from(format!("{command}{trimmed}")));
        }

        candidates
    }

    #[cfg(not(target_os = "windows"))]
    {
        vec![OsString::from(command)]
    }
}

pub fn build_profile_response_prompt_parts(profile: &AgentProfileRecord) -> Vec<String> {
    build_profile_response_prompt_parts_from_runtime(
        profile.response_language.as_deref(),
        profile.response_style.as_deref(),
    )
}

fn build_profile_response_prompt_parts_from_runtime(
    response_language: Option<&str>,
    response_style: Option<&str>,
) -> Vec<String> {
    let mut parts = Vec::new();

    if let Some(language) = normalize_profile_response_language(response_language) {
        parts.push(format!(
            "Respond in {language} unless the user explicitly asks for a different language."
        ));
    }

    parts.push(
        response_style_system_instruction(normalize_profile_response_style(response_style))
            .to_string(),
    );

    parts
}

pub fn normalize_profile_response_language(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
}

pub fn normalize_profile_response_style(value: Option<&str>) -> ProfileResponseStyle {
    match value.unwrap_or("balanced").trim().to_lowercase().as_str() {
        "concise" => ProfileResponseStyle::Concise,
        "guide" | "guided" => ProfileResponseStyle::Guide,
        _ => ProfileResponseStyle::Balanced,
    }
}

pub fn response_style_system_instruction(style: ProfileResponseStyle) -> &'static str {
    match style {
        ProfileResponseStyle::Balanced => {
            "Response style: balanced. Be clear and direct by default. Expand with detail when the topic warrants it, but avoid unnecessary filler."
        }
        ProfileResponseStyle::Concise => {
            "Response style: concise. Keep answers short and direct. Minimize explanation unless asked. Prefer code and commands over prose. Skip pleasantries."
        }
        ProfileResponseStyle::Guide => {
            "Response style: guided. Explain tradeoffs, reasoning, and next steps clearly. Help the user understand why, not just what. Surface alternatives when relevant."
        }
    }
}

fn parse_positive_u32(value: Option<&str>, fallback: u32) -> u32 {
    value
        .and_then(|value| value.trim().parse::<u32>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(fallback)
}

fn parse_transport(value: Option<&str>) -> Transport {
    match value.unwrap_or("sse").trim().to_lowercase().as_str() {
        "websocket" | "ws" => Transport::WebSocket,
        "auto" => Transport::Auto,
        _ => Transport::Sse,
    }
}

fn normalize_provider_options(value: Option<serde_json::Value>) -> Option<serde_json::Value> {
    value.and_then(|value| match value {
        serde_json::Value::Object(map) if map.is_empty() => None,
        serde_json::Value::Object(_) => Some(value),
        _ => None,
    })
}

fn runtime_security_config() -> tiy_core::types::SecurityConfig {
    let mut security = tiy_core::types::SecurityConfig::default();
    security.agent.tool_execution_timeout_secs = SUBAGENT_TOOL_TIMEOUT_SECS;
    security
}

fn standard_tool_timeout() -> std::time::Duration {
    std::time::Duration::from_secs(STANDARD_TOOL_TIMEOUT_SECS)
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

#[cfg(test)]
mod tests {
    use super::{
        build_profile_response_prompt_parts, collect_workspace_instruction_snippet,
        handle_agent_event, normalize_profile_response_language, normalize_profile_response_style,
        response_style_system_instruction, runtime_security_config, standard_tool_timeout,
        ProfileResponseStyle, STANDARD_TOOL_TIMEOUT_SECS, SUBAGENT_TOOL_TIMEOUT_SECS,
    };
    use std::fs;
    use std::sync::Mutex as StdMutex;

    use tempfile::tempdir;
    use tiy_core::agent::{AgentEvent, AgentMessage};
    use tiy_core::types::{Api, AssistantMessage, AssistantMessageEvent, Provider};
    use tokio::sync::mpsc;

    use crate::ipc::frontend_channels::ThreadStreamEvent;
    use crate::model::provider::AgentProfileRecord;

    const TEST_CONTEXT_WINDOW: &str = "128000";
    const TEST_MODEL_DISPLAY_NAME: &str = "GPT Test";

    fn sample_profile() -> AgentProfileRecord {
        AgentProfileRecord {
            id: "profile-1".to_string(),
            name: "Default".to_string(),
            custom_instructions: None,
            commit_message_prompt: None,
            response_style: Some("balanced".to_string()),
            response_language: Some("English".to_string()),
            commit_message_language: Some("English".to_string()),
            primary_provider_id: None,
            primary_model_id: None,
            auxiliary_provider_id: None,
            auxiliary_model_id: None,
            lightweight_provider_id: None,
            lightweight_model_id: None,
            is_default: true,
            created_at: String::new(),
            updated_at: String::new(),
        }
    }

    fn sample_partial_assistant_message() -> AssistantMessage {
        AssistantMessage::builder()
            .api(Api::OpenAICompletions)
            .provider(Provider::OpenAI)
            .model("gpt-test")
            .build()
            .expect("partial assistant message")
    }

    fn handle_test_agent_event(
        run_id: &str,
        event_tx: &mpsc::UnboundedSender<ThreadStreamEvent>,
        current_message_id: &StdMutex<Option<String>>,
        current_reasoning_message_id: &StdMutex<Option<String>>,
        last_usage: &StdMutex<Option<tiy_core::types::Usage>>,
        reasoning_buffer: &StdMutex<String>,
        event: &AgentEvent,
    ) {
        handle_agent_event(
            run_id,
            event_tx,
            current_message_id,
            current_reasoning_message_id,
            last_usage,
            reasoning_buffer,
            TEST_CONTEXT_WINDOW,
            TEST_MODEL_DISPLAY_NAME,
            event,
        );
    }

    #[test]
    fn test_runtime_security_config_extends_helper_tool_timeout() {
        let security = runtime_security_config();

        assert_eq!(
            security.agent.tool_execution_timeout_secs,
            SUBAGENT_TOOL_TIMEOUT_SECS
        );
    }

    #[test]
    fn test_standard_tool_timeout_remains_120_seconds() {
        assert_eq!(
            standard_tool_timeout().as_secs(),
            STANDARD_TOOL_TIMEOUT_SECS
        );
    }

    #[test]
    fn profile_response_language_is_trimmed() {
        assert_eq!(
            normalize_profile_response_language(Some("  简体中文  ")).as_deref(),
            Some("简体中文")
        );
        assert_eq!(normalize_profile_response_language(Some("   ")), None);
    }

    #[test]
    fn profile_response_style_defaults_to_balanced() {
        assert_eq!(
            normalize_profile_response_style(Some("guide")),
            ProfileResponseStyle::Guide
        );
        assert_eq!(
            normalize_profile_response_style(Some("concise")),
            ProfileResponseStyle::Concise
        );
        assert_eq!(
            normalize_profile_response_style(Some("unknown")),
            ProfileResponseStyle::Balanced
        );
    }

    #[test]
    fn profile_prompt_parts_include_language_and_style() {
        let mut profile = sample_profile();
        profile.response_language = Some("Japanese".to_string());
        profile.response_style = Some("concise".to_string());

        let parts = build_profile_response_prompt_parts(&profile);

        assert_eq!(parts.len(), 2);
        assert!(parts[0].contains("Japanese"));
        assert_eq!(
            parts[1],
            response_style_system_instruction(ProfileResponseStyle::Concise)
        );
    }

    #[test]
    fn workspace_instruction_snippet_uses_priority_order() {
        let temp_dir = tempdir().expect("temp dir");
        let root = temp_dir.path();

        fs::write(root.join("CLAUDE.md"), "Claude instructions").expect("write claude");
        fs::write(root.join("AGENT.MD"), "Agent instructions").expect("write agent");
        fs::write(root.join("AGENTS.md"), "Agents instructions").expect("write agents");

        let snippet = collect_workspace_instruction_snippet(root.to_string_lossy().as_ref())
            .expect("workspace instruction snippet");

        assert_eq!(snippet.file_name, "AGENTS.md");
        assert_eq!(snippet.content, "Agents instructions");
    }

    #[test]
    fn reasoning_blocks_reset_message_id_between_thought_segments() {
        let (event_tx, mut event_rx) = mpsc::unbounded_channel();
        let current_message_id = StdMutex::new(None::<String>);
        let current_reasoning_message_id = StdMutex::new(None::<String>);
        let last_usage = StdMutex::new(None::<tiy_core::types::Usage>);
        let reasoning_buffer = StdMutex::new(String::new());
        let partial = sample_partial_assistant_message();

        handle_test_agent_event(
            "run-1",
            &event_tx,
            &current_message_id,
            &current_reasoning_message_id,
            &last_usage,
            &reasoning_buffer,
            &AgentEvent::MessageUpdate {
                message: AgentMessage::Assistant(partial.clone()),
                assistant_event: Box::new(AssistantMessageEvent::ThinkingStart {
                    content_index: 0,
                    partial: partial.clone(),
                }),
            },
        );
        handle_test_agent_event(
            "run-1",
            &event_tx,
            &current_message_id,
            &current_reasoning_message_id,
            &last_usage,
            &reasoning_buffer,
            &AgentEvent::MessageUpdate {
                message: AgentMessage::Assistant(partial.clone()),
                assistant_event: Box::new(AssistantMessageEvent::ThinkingDelta {
                    content_index: 0,
                    delta: "first thought".to_string(),
                    partial: partial.clone(),
                }),
            },
        );
        handle_test_agent_event(
            "run-1",
            &event_tx,
            &current_message_id,
            &current_reasoning_message_id,
            &last_usage,
            &reasoning_buffer,
            &AgentEvent::MessageUpdate {
                message: AgentMessage::Assistant(partial.clone()),
                assistant_event: Box::new(AssistantMessageEvent::ThinkingEnd {
                    content_index: 0,
                    content: "first thought".to_string(),
                    partial: partial.clone(),
                }),
            },
        );
        handle_test_agent_event(
            "run-1",
            &event_tx,
            &current_message_id,
            &current_reasoning_message_id,
            &last_usage,
            &reasoning_buffer,
            &AgentEvent::MessageUpdate {
                message: AgentMessage::Assistant(partial.clone()),
                assistant_event: Box::new(AssistantMessageEvent::ThinkingStart {
                    content_index: 1,
                    partial: partial.clone(),
                }),
            },
        );
        handle_test_agent_event(
            "run-1",
            &event_tx,
            &current_message_id,
            &current_reasoning_message_id,
            &last_usage,
            &reasoning_buffer,
            &AgentEvent::MessageUpdate {
                message: AgentMessage::Assistant(partial.clone()),
                assistant_event: Box::new(AssistantMessageEvent::ThinkingDelta {
                    content_index: 1,
                    delta: "second thought".to_string(),
                    partial,
                }),
            },
        );

        let events = std::iter::from_fn(|| event_rx.try_recv().ok()).collect::<Vec<_>>();
        let reasoning_events = events
            .into_iter()
            .filter_map(|event| match event {
                ThreadStreamEvent::ReasoningUpdated {
                    message_id,
                    reasoning,
                    ..
                } => Some((message_id, reasoning)),
                _ => None,
            })
            .collect::<Vec<_>>();

        assert_eq!(reasoning_events.len(), 3);
        assert_eq!(reasoning_events[0].1, "first thought");
        assert_eq!(reasoning_events[1].1, "first thought");
        assert_eq!(reasoning_events[2].1, "second thought");
        assert_eq!(reasoning_events[0].0, reasoning_events[1].0);
        assert_ne!(reasoning_events[0].0, reasoning_events[2].0);
    }

    #[test]
    fn empty_reasoning_blocks_do_not_emit_reasoning_events() {
        let (event_tx, mut event_rx) = mpsc::unbounded_channel();
        let current_message_id = StdMutex::new(None::<String>);
        let current_reasoning_message_id = StdMutex::new(None::<String>);
        let last_usage = StdMutex::new(None::<tiy_core::types::Usage>);
        let reasoning_buffer = StdMutex::new(String::new());
        let partial = sample_partial_assistant_message();

        handle_test_agent_event(
            "run-1",
            &event_tx,
            &current_message_id,
            &current_reasoning_message_id,
            &last_usage,
            &reasoning_buffer,
            &AgentEvent::MessageUpdate {
                message: AgentMessage::Assistant(partial.clone()),
                assistant_event: Box::new(AssistantMessageEvent::ThinkingStart {
                    content_index: 0,
                    partial: partial.clone(),
                }),
            },
        );
        handle_test_agent_event(
            "run-1",
            &event_tx,
            &current_message_id,
            &current_reasoning_message_id,
            &last_usage,
            &reasoning_buffer,
            &AgentEvent::MessageUpdate {
                message: AgentMessage::Assistant(partial),
                assistant_event: Box::new(AssistantMessageEvent::ThinkingEnd {
                    content_index: 0,
                    content: String::new(),
                    partial: sample_partial_assistant_message(),
                }),
            },
        );

        let reasoning_events = std::iter::from_fn(|| event_rx.try_recv().ok())
            .filter(|event| matches!(event, ThreadStreamEvent::ReasoningUpdated { .. }))
            .collect::<Vec<_>>();

        assert!(reasoning_events.is_empty());
    }

    #[test]
    fn message_end_emits_usage_updates_once_per_change() {
        let (event_tx, mut event_rx) = mpsc::unbounded_channel();
        let current_message_id = StdMutex::new(None::<String>);
        let current_reasoning_message_id = StdMutex::new(None::<String>);
        let last_usage = StdMutex::new(None::<tiy_core::types::Usage>);
        let reasoning_buffer = StdMutex::new(String::new());
        let assistant = AssistantMessage::builder()
            .api(Api::OpenAICompletions)
            .provider(Provider::OpenAI)
            .model("gpt-test")
            .usage(tiy_core::types::Usage::from_tokens(256, 32))
            .build()
            .expect("assistant message with usage");

        handle_test_agent_event(
            "run-usage",
            &event_tx,
            &current_message_id,
            &current_reasoning_message_id,
            &last_usage,
            &reasoning_buffer,
            &AgentEvent::MessageEnd {
                message: AgentMessage::Assistant(assistant.clone()),
            },
        );
        handle_test_agent_event(
            "run-usage",
            &event_tx,
            &current_message_id,
            &current_reasoning_message_id,
            &last_usage,
            &reasoning_buffer,
            &AgentEvent::MessageEnd {
                message: AgentMessage::Assistant(assistant),
            },
        );

        let usage_events = std::iter::from_fn(|| event_rx.try_recv().ok())
            .filter_map(|event| match event {
                ThreadStreamEvent::ThreadUsageUpdated { usage, .. } => Some(usage),
                _ => None,
            })
            .collect::<Vec<_>>();

        assert_eq!(usage_events.len(), 1);
        assert_eq!(usage_events[0].input_tokens, 256);
        assert_eq!(usage_events[0].output_tokens, 32);
        assert_eq!(usage_events[0].total_tokens, 288);
    }
}
