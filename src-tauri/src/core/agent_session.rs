use std::collections::HashMap;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex as StdMutex, Weak};

use sqlx::SqlitePool;
use tiy_core::agent::{
    Agent, AgentError, AgentMessage, AgentTool, AgentToolResult, ToolExecutionMode,
};
use tiy_core::thinking::ThinkingLevel;
use tiy_core::types::{
    AssistantMessage, AssistantMessageEvent, ContentBlock, Cost, InputType, Model, Provider,
    StopReason, TextContent, Transport, Usage, UserMessage,
};
use tokio::sync::mpsc;

use crate::core::plan_checkpoint::{
    approval_prompt_markdown, build_approval_prompt_metadata, build_plan_artifact_from_tool_input,
    build_plan_message_metadata, parse_plan_message_metadata, plan_markdown,
};
use crate::core::subagent::{
    runtime_orchestration_tools, HelperAgentOrchestrator, HelperRunRequest,
    RuntimeOrchestrationTool, SubagentProfile, TERM_CLOSE_TOOL_DESCRIPTION,
    TERM_OUTPUT_TOOL_DESCRIPTION, TERM_PANEL_USAGE_NOTE, TERM_RESTART_TOOL_DESCRIPTION,
    TERM_STATUS_TOOL_DESCRIPTION, TERM_WRITE_TOOL_DESCRIPTION,
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
    pub initial_prompt: Option<String>,
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
    let history_messages = message_repo::list_recent(pool, thread_id, None, MESSAGE_HISTORY_LIMIT)
        .await?
        .into_iter()
        .filter(|message| message.status != "discarded")
        .collect();
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
        initial_prompt: None,
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
    checkpoint_requested: AtomicBool,
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
            agent.set_max_turns(crate::desktop_agent_max_turns!());
            configure_agent(&agent, &spec, weak_self.clone());

            Self {
                spec,
                pool,
                tool_gateway,
                helper_orchestrator,
                event_tx,
                agent,
                cancel_requested: Arc::new(AtomicBool::new(false)),
                checkpoint_requested: AtomicBool::new(false),
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
        let last_completed_message_id = Arc::new(StdMutex::new(None::<String>));
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
        let last_completed_message_id_ref = Arc::clone(&last_completed_message_id);
        let reasoning_message_id_ref = Arc::clone(&current_reasoning_message_id);
        let last_usage_ref = Arc::clone(&last_usage);
        let reasoning_ref = Arc::clone(&reasoning_buffer);
        let unsubscribe = self.agent.subscribe(move |event| {
            handle_agent_event(
                &run_id,
                &event_tx,
                &message_id_ref,
                &last_completed_message_id_ref,
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

        let result = if let Some(prompt) = self.spec.initial_prompt.clone() {
            self.agent.prompt(prompt).await
        } else {
            self.agent.continue_().await
        };
        unsubscribe();

        match result {
            Ok(_) => {
                if self.cancel_requested.load(Ordering::SeqCst) {
                    let _ = self.event_tx.send(ThreadStreamEvent::RunCancelled {
                        run_id: self.spec.run_id.clone(),
                    });
                } else if self.checkpoint_requested.load(Ordering::SeqCst) {
                    let _ = self.event_tx.send(ThreadStreamEvent::RunCheckpointed {
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
                } else if self.checkpoint_requested.load(Ordering::SeqCst) {
                    let _ = self.event_tx.send(ThreadStreamEvent::RunCheckpointed {
                        run_id: self.spec.run_id.clone(),
                    });
                } else if let AgentError::MaxTurnsReached(max_turns) = &error {
                    let _ = self.event_tx.send(ThreadStreamEvent::RunLimitReached {
                        run_id: self.spec.run_id.clone(),
                        error: error.to_string(),
                        max_turns: *max_turns,
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
        if tool_name == "update_plan" {
            return self.execute_plan_checkpoint(tool_input).await;
        }

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

        let helper_role = resolve_helper_model_role(&self.spec.model_plan, tool);
        let helper_profile = resolve_helper_profile(tool);

        let result = self
            .helper_orchestrator
            .run_helper(HelperRunRequest {
                run_id: self.spec.run_id.clone(),
                thread_id: self.spec.thread_id.clone(),
                tool,
                helper_profile: Some(helper_profile),
                parent_tool_call_id: Some(tool_call_id.to_string()),
                task: task.clone(),
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

    async fn execute_plan_checkpoint(&self, tool_input: &serde_json::Value) -> AgentToolResult {
        let plan_revision = match self.next_plan_revision().await {
            Ok(revision) => revision,
            Err(error) => return agent_error_result(error.to_string()),
        };
        let artifact = build_plan_artifact_from_tool_input(tool_input, plan_revision);
        let plan_message_id = uuid::Uuid::now_v7().to_string();
        let approval_message_id = uuid::Uuid::now_v7().to_string();
        let plan_metadata =
            build_plan_message_metadata(artifact.clone(), &self.spec.run_id, &self.spec.run_mode);
        let approval_metadata =
            build_approval_prompt_metadata(artifact.plan_revision, &plan_message_id);

        let plan_message = MessageRecord {
            id: plan_message_id.clone(),
            thread_id: self.spec.thread_id.clone(),
            run_id: Some(self.spec.run_id.clone()),
            role: "assistant".to_string(),
            content_markdown: plan_markdown(&plan_metadata),
            message_type: "plan".to_string(),
            status: "completed".to_string(),
            metadata_json: serde_json::to_string(&plan_metadata).ok(),
            created_at: String::new(),
        };
        let approval_message = MessageRecord {
            id: approval_message_id.clone(),
            thread_id: self.spec.thread_id.clone(),
            run_id: Some(self.spec.run_id.clone()),
            role: "assistant".to_string(),
            content_markdown: approval_prompt_markdown(&artifact),
            message_type: "approval_prompt".to_string(),
            status: "completed".to_string(),
            metadata_json: serde_json::to_string(&approval_metadata).ok(),
            created_at: String::new(),
        };

        if let Err(error) = message_repo::insert(&self.pool, &plan_message).await {
            return agent_error_result(format!("failed to persist plan message: {error}"));
        }
        if let Err(error) = message_repo::insert(&self.pool, &approval_message).await {
            return agent_error_result(format!("failed to persist approval prompt: {error}"));
        }

        let _ = self.event_tx.send(ThreadStreamEvent::PlanUpdated {
            run_id: self.spec.run_id.clone(),
            plan: serde_json::to_value(&artifact).unwrap_or_else(|_| serde_json::json!({})),
        });

        self.checkpoint_requested.store(true, Ordering::SeqCst);
        self.abort_signal.cancel();
        self.agent.abort();

        AgentToolResult {
            content: vec![ContentBlock::Text(TextContent::new(
                "Implementation plan published. Waiting for approval before execution.",
            ))],
            details: Some(serde_json::json!({
                "planMessageId": plan_message_id,
                "approvalMessageId": approval_message_id,
                "plan": artifact,
            })),
        }
    }

    async fn next_plan_revision(&self) -> Result<u32, AppError> {
        let messages =
            message_repo::list_recent(&self.pool, &self.spec.thread_id, None, 256).await?;
        let next = messages
            .iter()
            .filter(|message| message.message_type == "plan")
            .filter_map(|message| {
                message
                    .metadata_json
                    .as_deref()
                    .and_then(|raw| serde_json::from_str::<serde_json::Value>(raw).ok())
                    .and_then(|value| {
                        value
                            .get("planRevision")
                            .and_then(serde_json::Value::as_u64)
                            .and_then(|value| u32::try_from(value).ok())
                    })
            })
            .max()
            .unwrap_or(0);
        Ok(next.saturating_add(1))
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
    last_completed_message_id: &StdMutex<Option<String>>,
    current_reasoning_message_id: &StdMutex<Option<String>>,
    last_usage: &StdMutex<Option<Usage>>,
    reasoning_buffer: &StdMutex<String>,
    context_window: &str,
    model_display_name: &str,
    event: &tiy_core::agent::AgentEvent,
) {
    match event {
        tiy_core::agent::AgentEvent::TurnRetrying {
            attempt,
            max_attempts,
            delay_ms,
            reason,
        } => {
            let _ = event_tx.send(ThreadStreamEvent::RunRetrying {
                run_id: run_id.to_string(),
                attempt: *attempt,
                max_attempts: *max_attempts,
                delay_ms: *delay_ms,
                reason: reason.clone(),
            });
        }
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
                set_last_completed_message_id(last_completed_message_id, Some(message_id.clone()));
                let _ = event_tx.send(ThreadStreamEvent::MessageCompleted {
                    run_id: run_id.to_string(),
                    message_id,
                    content,
                });
            }

            reset_reasoning_state(current_reasoning_message_id, reasoning_buffer);
        }
        tiy_core::agent::AgentEvent::MessageDiscarded { reason, .. } => {
            if let Some(message_id) = read_last_completed_message_id(last_completed_message_id) {
                let _ = event_tx.send(ThreadStreamEvent::MessageDiscarded {
                    run_id: run_id.to_string(),
                    message_id,
                    reason: reason.clone(),
                });
            }
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

fn set_last_completed_message_id(
    last_completed_message_id: &StdMutex<Option<String>>,
    value: Option<String>,
) {
    if let Ok(mut guard) = last_completed_message_id.lock() {
        *guard = value;
    }
}

fn read_last_completed_message_id(
    last_completed_message_id: &StdMutex<Option<String>>,
) -> Option<String> {
    last_completed_message_id
        .lock()
        .ok()
        .and_then(|guard| guard.clone())
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
            "Read a file inside the current workspace. Supports optional offset/limit windowing for large files and returns a truncated preview when the selected range exceeds safety limits.",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string" },
                    "offset": {
                        "type": "integer",
                        "description": "Optional 1-indexed line number to start reading from."
                    },
                    "limit": {
                        "type": "integer",
                        "description": "Optional maximum number of lines to read from the offset."
                    }
                },
                "required": ["path"]
            }),
        ),
        AgentTool::new(
            "list",
            "List Directory",
            "List files and folders inside the current workspace. Supports an optional preview limit for large directories.",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string" },
                    "limit": {
                        "type": "integer",
                        "description": "Optional maximum number of entries to return. Defaults to 500 and is capped for safety."
                    }
                }
            }),
        ),
        AgentTool::new(
            "search",
            "Search Repo",
            "Search the current workspace with ripgrep. Results are preview-limited for safety; omit wildcard-only filePattern values like '*' or '**/*'.",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "query": {
                        "type": "string",
                        "description": "Search term or regex."
                    },
                    "directory": {
                        "type": "string",
                        "description": "Directory to search in (default: workspace root)."
                    },
                    "filePattern": {
                        "type": "string",
                        "description": "Optional glob filter such as '*.rs' or 'src/**/*.ts'. Omit it to search all files; do not pass '*' or '**/*'."
                    },
                    "maxResults": {
                        "type": "integer",
                        "description": "Optional preview limit for returned matches. Defaults to 100 and is capped for context safety."
                    }
                },
                "required": ["query"]
            }),
        ),
        AgentTool::new(
            "find",
            "Find Files",
            "Search for files by glob pattern. Returns matching file paths relative to the workspace. Respects common ignore patterns (.git, node_modules, target). Supports an optional preview limit and truncates output to 1000 results or 100KB.",
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
                    },
                    "limit": {
                        "type": "integer",
                        "description": "Optional maximum number of matches to preview. Defaults to 1000 and is capped for safety."
                    }
                },
                "required": ["pattern"]
            }),
        ),
        AgentTool::new(
            "term_status",
            "Terminal Status",
            TERM_STATUS_TOOL_DESCRIPTION,
            serde_json::json!({
                "type": "object",
                "properties": {}
            }),
        ),
        AgentTool::new(
            "term_output",
            "Terminal Output",
            TERM_OUTPUT_TOOL_DESCRIPTION,
            serde_json::json!({
                "type": "object",
                "properties": {}
            }),
        ),
        AgentTool::new(
            "update_plan",
            "Update Plan",
            "Publish the current implementation plan and pause before execution. Use this when the main agent has enough context to present a concrete pre-implementation plan for user approval.",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "title": { "type": "string" },
                    "summary": { "type": "string" },
                    "steps": {
                        "type": "array",
                        "items": {
                            "oneOf": [
                                { "type": "string" },
                                {
                                    "type": "object",
                                    "properties": {
                                        "id": { "type": "string" },
                                        "title": { "type": "string" },
                                        "description": { "type": "string" },
                                        "status": { "type": "string" },
                                        "files": {
                                            "type": "array",
                                            "items": { "type": "string" }
                                        }
                                    }
                                }
                            ]
                        }
                    },
                    "risks": {
                        "type": "array",
                        "items": { "type": "string" }
                    },
                    "openQuestions": {
                        "type": "array",
                        "items": { "type": "string" }
                    },
                    "needsContextResetOption": { "type": "boolean" },
                    "plan": {
                        "type": "object",
                        "description": "Optional nested plan payload. If provided, the runtime reads planning fields from this object."
                    }
                }
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
        tools.push(AgentTool::new(
            "term_write",
            "Terminal Write",
            TERM_WRITE_TOOL_DESCRIPTION,
            serde_json::json!({
                "type": "object",
                "properties": {
                    "data": {
                        "type": "string",
                        "description": "Input to send to the current thread's Terminal panel session."
                    }
                },
                "required": ["data"]
            }),
        ));
        tools.push(AgentTool::new(
            "term_restart",
            "Terminal Restart",
            TERM_RESTART_TOOL_DESCRIPTION,
            serde_json::json!({
                "type": "object",
                "properties": {
                    "cols": {
                        "type": "integer",
                        "description": "Optional terminal width in columns for the restarted Terminal panel session."
                    },
                    "rows": {
                        "type": "integer",
                        "description": "Optional terminal height in rows for the restarted Terminal panel session."
                    }
                }
            }),
        ));
        tools.push(AgentTool::new(
            "term_close",
            "Terminal Close",
            TERM_CLOSE_TOOL_DESCRIPTION,
            serde_json::json!({
                "type": "object",
                "properties": {}
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

fn resolve_helper_profile(tool: RuntimeOrchestrationTool) -> SubagentProfile {
    match tool {
        RuntimeOrchestrationTool::Explore => SubagentProfile::Explore,
        RuntimeOrchestrationTool::Review => SubagentProfile::Review,
    }
}

fn resolve_helper_model_role(
    model_plan: &ResolvedRuntimeModelPlan,
    tool: RuntimeOrchestrationTool,
) -> ResolvedModelRole {
    match tool {
        RuntimeOrchestrationTool::Explore | RuntimeOrchestrationTool::Review => model_plan
            .auxiliary
            .clone()
            .unwrap_or_else(|| model_plan.primary.clone()),
    }
}

pub(crate) fn convert_history_messages(
    messages: &[MessageRecord],
    model: &Model,
) -> Vec<AgentMessage> {
    messages
        .iter()
        .filter_map(|message| match message.message_type.as_str() {
            "plain_message" => match message.role.as_str() {
                "user" => Some(AgentMessage::User(UserMessage::text(
                    message.content_markdown.clone(),
                ))),
                "assistant" => Some(AgentMessage::Assistant(assistant_message_from_text(
                    &message.content_markdown,
                    model,
                ))),
                _ => None,
            },
            "plan" if message.role == "assistant" => Some(AgentMessage::Assistant(
                assistant_message_from_text(&format_plan_history_message(message), model),
            )),
            _ => None,
        })
        .collect()
}

fn format_plan_history_message(message: &MessageRecord) -> String {
    let metadata = message
        .metadata_json
        .as_deref()
        .and_then(|raw| serde_json::from_str::<serde_json::Value>(raw).ok())
        .and_then(|value| parse_plan_message_metadata(&value));

    let Some(metadata) = metadata else {
        return format!(
            "Implementation plan checkpoint:\n{}",
            message.content_markdown.trim()
        );
    };

    format!(
        "Implementation plan checkpoint (revision {}, approval state: {}):\n{}",
        metadata.artifact.plan_revision,
        metadata.approval_state,
        plan_markdown(&metadata)
    )
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
- Before taking tool actions or making substantive changes, send a brief, friendly reply that acknowledges the request and states the next step you are about to take.\n\
- Read files before editing. Understand existing code before making changes.\n\
- Use edit for precise, surgical changes. Use write only for new files or complete rewrites.\n\
- Prefer search and find over shell for file exploration — they are faster and respect ignore patterns.\n\
- For search, omit wildcard-only filePattern values such as `*` or `**/*`; leaving filePattern unset already searches the full selected directory.\n\
- Delegate proactively on substantial work. When the task is cross-file, unfamiliar, risky, or likely to benefit from a second pass, use a helper instead of doing all exploration and review yourself.\n\
- Use agent_explore to investigate unfamiliar areas, collect evidence, map dependencies, explain the current state, or gather the right files before choosing an implementation.\n\
- For complex tasks, briefly confirm your understanding of the goal, scope, or constraints before publishing an implementation plan.\n\
- Use update_plan to publish the current implementation plan once the intended change is clear.\n\
- Do not use update_plan for pure analysis, architecture explanation, current-state summaries, or information gathering with no concrete implementation to plan.\n\
- In default mode, if the task is complex or risky enough to benefit from explicit pre-implementation approval, publish a plan with update_plan before making changes.\n\
- Use agent_review after implementation with target='code' or target='diff' to check regressions, edge cases, and consistency. The review helper is responsible for running the necessary type-check and test commands and returning the verification results alongside the code review findings.\n\
- After agent_review completes, treat its verification output as the default source of truth for post-implementation type-check and test status. Do not rerun the same verification commands yourself unless the helper explicitly could not run them, reported inconclusive results, or the user asked you to double-check.\n\
- Recommended flow for non-trivial tasks: agent_explore -> confirm goal -> update_plan -> wait for approval -> implement -> agent_review(target='code' or 'diff').\n\
- Skip delegation only when the task is small, obvious, and isolated enough that extra helper work would not pay off.\n\
- Match the active response style when deciding answer length and explanation depth. Show file paths clearly when working with files.\n\
- When summarizing your actions, describe what you did in plain text — do not re-read or re-cat files to prove your work.\n\
- Flag risks, destructive operations, or ambiguity before acting. Ask when intent is unclear.",
        ),
        build_prompt_section(
            "Final Response Structure",
            final_response_structure_system_instruction(),
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
            run_mode_prompt_body(run_mode),
        ));
    } else {
        parts.push(build_prompt_section(
            "Run Mode",
            run_mode_prompt_body(run_mode),
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

fn final_response_structure_system_instruction() -> &'static str {
    "For conclusion-oriented replies, choose a structure that matches the task instead of forcing one template for every situation.\n\
- Keep the outer Markdown layout disciplined: use at most two heading levels in one reply, avoid turning every sub-point into its own heading, and prefer short sections with lists underneath over a long chain of peer headers.\n\
- When the reply is more than a very small update, prefer a clearly structured Markdown presentation instead of one dense block of prose.\n\
- Use short Markdown section headers for the main sections only. Put supporting detail inside numbered lists or flat bullet lists rather than promoting each detail to a new heading.\n\
- Use numbered lists for ordered reasons, changes, or options. Use flat bullet lists for evidence, verification items, or supporting facts.\n\
- Use emphasis or inline code sparingly to highlight the key conclusion, the recommended option, commands, file paths, settings, or identifiers that the user should notice quickly. Do not overload the reply with inline code formatting.\n\
- For simple tasks, you may compress the structure into a short paragraph or a short flat list, but keep a clear top-down order.\n\
- Use one of these default patterns:\n\
  - Debug or problem analysis: conclusion -> causes 1, 2, and 3 if relevant -> evidence tied to each cause -> recommendation options 1, 2, and 3 with a recommended option.\n\
  - Code change or result report: outcome -> key changes 1, 2, and 3 if relevant -> verification or evidence -> next steps, risks, or follow-up recommendation.\n\
  - Comparison or decision support: recommendation -> options 1, 2, and 3 -> tradeoffs and evidence -> clearly state the recommended option and why.\n\
  - Direct explanation or question answering: direct answer -> key points 1, 2, and 3 if relevant -> examples or evidence when helpful -> next step only if it adds value.\n\
- Do not force explicit headings on every reply unless the task benefits from a more structured presentation."
}

fn run_mode_prompt_body(run_mode: &str) -> String {
    match run_mode {
        "plan" => format!(
            "Plan mode is active.\n\
- Use only read-only tools plus update_plan: read, list, search, find, term_status, term_output, update_plan.\n\
- {TERM_PANEL_USAGE_NOTE}\n\
- Use agent_explore for read-only investigation and current-state analysis.\n\
- Use update_plan only for the formal pre-implementation plan, not for general analysis or explanation.\n\
- Once you publish a plan with update_plan, the run will pause for user approval before any implementation can begin.\n\
- Do NOT use edit, write, or shell unless the user explicitly requests execution.\n\
- Focus on analysis, explanation, and actionable planning. Identify risks, gaps, and concrete next steps."
        ),
        _ => format!(
            "Default execution mode is active.\n\
- Use the configured tool profile, subject to policy, approvals, and workspace boundaries.\n\
- {TERM_PANEL_USAGE_NOTE}\n\
- If the task is complex enough that implementation should pause for review first, publish an implementation plan with update_plan before making changes.\n\
- Prefer the smallest sufficient action that moves the task forward."
        ),
    }
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
    use crate::core::workspace_paths::parse_writable_roots;

    let approval_policy = settings_repo::policy_get(pool, "approval_policy")
        .await?
        .map(|record| parse_approval_policy_mode(&record.value_json))
        .unwrap_or_else(|| "require_for_mutations".to_string());

    let writable_roots: Vec<String> = settings_repo::policy_get(pool, "writable_roots")
        .await?
        .map(|record| parse_writable_roots(&record.value_json))
        .unwrap_or_default();

    let run_mode_line = if run_mode == "plan" {
        "Plan mode is active, so mutating tools are blocked."
    } else {
        "Default mode is active, so tool use follows the configured approval policy."
    };

    let mut lines = vec![
        "- Effective runtime sandbox: workspace-scoped tool execution with policy checks.".to_string(),
        format!("- Workspace boundary: file and path-aware tools are restricted to the current workspace (`{workspace_path}`)."),
        format!("- Approval policy: {approval_policy}."),
        "- Read-only tools are generally auto-allowed; mutating tools may require approval.".to_string(),
        format!("- {run_mode_line}"),
    ];

    if !writable_roots.is_empty() {
        let roots_display: Vec<String> = writable_roots
            .iter()
            .map(|root| format!("`{root}`"))
            .collect();
        lines.push(format!(
            "- Additional writable roots: {}. File tools (read, write, edit, list, find, search) can operate on files under these paths in addition to the workspace.",
            roots_display.join(", ")
        ));
    }

    lines.push("- Outer host sandbox metadata is not exposed here; rely on these effective runtime constraints.".to_string());

    Ok(build_prompt_section(
        "Sandbox & Permissions",
        lines.join("\n"),
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
            "Response style: balanced. Default to a compact but complete answer. Lead with the answer or outcome first. Use a short paragraph or a short flat list when that makes the reply clearer. Add explanation when it materially helps understanding, but avoid over-explaining routine details."
        }
        ProfileResponseStyle::Concise => {
            "Response style: concise. Treat brevity as a hard default. Lead with the answer, result, or next action immediately. Keep the final response to 1-3 short sentences or a very short flat list unless the user explicitly asks for more detail. Do not include background, reasoning, summaries, or pleasantries unless they are required for correctness. Prefer code, commands, and direct facts over prose."
        }
        ProfileResponseStyle::Guide => {
            "Response style: guided. Lead with the answer, then explain the reasoning, tradeoffs, and recommended next steps clearly. Be intentionally explanatory when that helps the user learn or make a decision. Surface relevant alternatives, caveats, or examples when useful."
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
        build_profile_response_prompt_parts, build_prompt_section, build_system_prompt,
        collect_workspace_instruction_snippet, convert_history_messages,
        final_response_structure_system_instruction, handle_agent_event,
        normalize_profile_response_language, normalize_profile_response_style,
        resolve_helper_model_role, resolve_helper_profile, response_style_system_instruction,
        run_mode_prompt_body, runtime_security_config, runtime_tools_for_profile,
        standard_tool_timeout, ProfileResponseStyle, ResolvedModelRole, ResolvedRuntimeModelPlan,
        RuntimeModelPlan, DEFAULT_FULL_TOOL_PROFILE, PLAN_READ_ONLY_TOOL_PROFILE,
        STANDARD_TOOL_TIMEOUT_SECS, SUBAGENT_TOOL_TIMEOUT_SECS,
    };
    use std::fs;
    use std::sync::Mutex as StdMutex;

    use tempfile::tempdir;
    use tiy_core::agent::{AgentEvent, AgentMessage};
    use tiy_core::thinking::ThinkingLevel;
    use tiy_core::types::{Api, AssistantMessage, AssistantMessageEvent, Provider};
    use tokio::sync::mpsc;

    use crate::core::plan_checkpoint::{
        build_plan_artifact_from_tool_input, build_plan_message_metadata,
    };
    use crate::core::subagent::{RuntimeOrchestrationTool, SubagentProfile};
    use crate::ipc::frontend_channels::ThreadStreamEvent;
    use crate::model::provider::AgentProfileRecord;
    use crate::model::thread::MessageRecord;
    use crate::persistence::init_database;

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

    fn sample_resolved_model_role(model_id: &str) -> ResolvedModelRole {
        let model = tiy_core::types::Model::builder()
            .id(model_id)
            .name(model_id)
            .provider(Provider::OpenAI)
            .base_url("https://api.openai.com/v1")
            .context_window(128_000)
            .max_tokens(32_000)
            .input(vec![tiy_core::types::InputType::Text])
            .cost(tiy_core::types::Cost::default())
            .build()
            .expect("sample resolved model");

        ResolvedModelRole {
            provider_id: format!("provider-{model_id}"),
            model_record_id: format!("record-{model_id}"),
            model_id: model_id.to_string(),
            model_name: model_id.to_string(),
            provider_type: "openai".to_string(),
            provider_name: "OpenAI".to_string(),
            api_key: Some("sk-test".to_string()),
            provider_options: None,
            model,
        }
    }

    fn sample_resolved_runtime_model_plan(
        auxiliary: Option<ResolvedModelRole>,
    ) -> ResolvedRuntimeModelPlan {
        ResolvedRuntimeModelPlan {
            raw: RuntimeModelPlan::default(),
            primary: sample_resolved_model_role("primary-model"),
            auxiliary,
            lightweight: None,
            thinking_level: ThinkingLevel::Off,
            transport: tiy_core::types::Transport::Sse,
        }
    }

    fn message_text(message: &AgentMessage) -> String {
        match message {
            AgentMessage::User(user) => match &user.content {
                tiy_core::types::UserContent::Text(text) => text.clone(),
                tiy_core::types::UserContent::Blocks(blocks) => blocks
                    .iter()
                    .filter_map(|block| match block {
                        tiy_core::types::ContentBlock::Text(text) => Some(text.text.as_str()),
                        _ => None,
                    })
                    .collect::<Vec<_>>()
                    .join("\n"),
            },
            AgentMessage::Assistant(assistant) => assistant.text_content(),
            _ => String::new(),
        }
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
        let last_completed_message_id = StdMutex::new(None::<String>);
        handle_agent_event(
            run_id,
            event_tx,
            current_message_id,
            &last_completed_message_id,
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
    fn response_style_instructions_have_stronger_behavioral_separation() {
        let balanced = response_style_system_instruction(ProfileResponseStyle::Balanced);
        let concise = response_style_system_instruction(ProfileResponseStyle::Concise);
        let guide = response_style_system_instruction(ProfileResponseStyle::Guide);

        assert!(balanced.contains("compact but complete answer"));
        assert!(concise.contains("1-3 short sentences"));
        assert!(concise.contains("hard default"));
        assert!(guide.contains("tradeoffs"));
        assert!(guide.contains("recommended next steps"));
    }

    #[test]
    fn final_response_structure_instruction_matches_task_types_and_markdown_hierarchy() {
        let instruction = final_response_structure_system_instruction();

        assert!(instruction.contains("at most two heading levels"));
        assert!(instruction.contains("avoid turning every sub-point into its own heading"));
        assert!(instruction.contains("Debug or problem analysis"));
        assert!(instruction.contains("Code change or result report"));
        assert!(instruction.contains("Comparison or decision support"));
        assert!(instruction.contains("Direct explanation or question answering"));
        assert!(instruction.contains("structured Markdown presentation"));
        assert!(instruction.contains("Do not overload the reply with inline code formatting"));
    }

    #[test]
    fn final_response_structure_section_is_distinct_from_response_style_rules() {
        let section = build_prompt_section(
            "Final Response Structure",
            final_response_structure_system_instruction(),
        );
        let balanced = response_style_system_instruction(ProfileResponseStyle::Balanced);

        assert!(section.starts_with("## Final Response Structure"));
        assert!(section.contains("For simple tasks, you may compress the structure"));
        assert!(balanced.contains("compact but complete answer"));
        assert!(!balanced.contains("reason 1, 2, and 3"));
    }

    #[test]
    fn run_mode_prompt_clarifies_terminal_panel_scope() {
        let plan_prompt = run_mode_prompt_body("plan");
        let default_prompt = run_mode_prompt_body("default");

        assert!(plan_prompt.contains("embedded Terminal panel"));
        assert!(plan_prompt.contains("update_plan"));
        assert!(plan_prompt.contains("pause for user approval"));
        assert!(plan_prompt.contains("do not inspect your own runtime"));
        assert!(default_prompt.contains("embedded Terminal panel"));
        assert!(default_prompt.contains("do not inspect your own runtime"));
    }

    #[test]
    fn default_full_profile_exposes_mutating_terminal_tools() {
        let tools = runtime_tools_for_profile(DEFAULT_FULL_TOOL_PROFILE);
        let tool_names: Vec<&str> = tools.iter().map(|tool| tool.name.as_str()).collect();

        assert!(tool_names.contains(&"term_write"));
        assert!(tool_names.contains(&"term_restart"));
        assert!(tool_names.contains(&"term_close"));
    }

    #[test]
    fn plan_read_only_profile_does_not_expose_mutating_terminal_tools() {
        let tools = runtime_tools_for_profile(PLAN_READ_ONLY_TOOL_PROFILE);
        let tool_names: Vec<&str> = tools.iter().map(|tool| tool.name.as_str()).collect();

        assert!(!tool_names.contains(&"term_write"));
        assert!(!tool_names.contains(&"term_restart"));
        assert!(!tool_names.contains(&"term_close"));
    }

    #[test]
    fn runtime_file_tools_expose_window_and_limit_parameters() {
        let tools = runtime_tools_for_profile(DEFAULT_FULL_TOOL_PROFILE);

        let read_tool = tools
            .iter()
            .find(|tool| tool.name == "read")
            .expect("read tool should exist");
        let list_tool = tools
            .iter()
            .find(|tool| tool.name == "list")
            .expect("list tool should exist");
        let find_tool = tools
            .iter()
            .find(|tool| tool.name == "find")
            .expect("find tool should exist");

        let read_properties = read_tool.parameters["properties"]
            .as_object()
            .expect("read properties should be object");
        let list_properties = list_tool.parameters["properties"]
            .as_object()
            .expect("list properties should be object");
        let find_properties = find_tool.parameters["properties"]
            .as_object()
            .expect("find properties should be object");

        assert!(read_properties.contains_key("offset"));
        assert!(read_properties.contains_key("limit"));
        assert!(list_properties.contains_key("limit"));
        assert!(find_properties.contains_key("limit"));
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

    #[tokio::test]
    async fn system_prompt_delegates_post_implementation_verification_to_review_helper() {
        let temp_dir = tempdir().expect("temp dir");
        let workspace_root = temp_dir.path().join("workspace");
        fs::create_dir(&workspace_root).expect("workspace dir");

        let db_path = temp_dir.path().join("test.db");
        let pool = init_database(&db_path).await.expect("database");

        let prompt = build_system_prompt(
            &pool,
            &RuntimeModelPlan::default(),
            workspace_root.to_string_lossy().as_ref(),
            "default",
        )
        .await
        .expect("system prompt");

        assert!(prompt.contains(
            "review helper is responsible for running the necessary type-check and test commands"
        ));
        assert!(prompt.contains(
            "Do not rerun the same verification commands yourself unless the helper explicitly could not run them"
        ));
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

    #[test]
    fn turn_retrying_event_emits_runtime_retry_notice() {
        let (event_tx, mut event_rx) = mpsc::unbounded_channel();
        let current_message_id = StdMutex::new(None::<String>);
        let last_completed_message_id = StdMutex::new(None::<String>);
        let current_reasoning_message_id = StdMutex::new(None::<String>);
        let last_usage = StdMutex::new(None::<tiy_core::types::Usage>);
        let reasoning_buffer = StdMutex::new(String::new());

        handle_agent_event(
            "run-retry",
            &event_tx,
            &current_message_id,
            &last_completed_message_id,
            &current_reasoning_message_id,
            &last_usage,
            &reasoning_buffer,
            TEST_CONTEXT_WINDOW,
            TEST_MODEL_DISPLAY_NAME,
            &AgentEvent::TurnRetrying {
                attempt: 1,
                max_attempts: 3,
                delay_ms: 1_000,
                reason: "Incomplete anthropic stream: missing message_stop".to_string(),
            },
        );

        let events = std::iter::from_fn(|| event_rx.try_recv().ok()).collect::<Vec<_>>();
        assert!(matches!(
            events.as_slice(),
            [ThreadStreamEvent::RunRetrying {
                run_id,
                attempt: 1,
                max_attempts: 3,
                delay_ms: 1_000,
                reason,
            }] if run_id == "run-retry" && reason.contains("Incomplete anthropic stream")
        ));
    }

    #[test]
    fn message_discarded_reuses_last_completed_assistant_message_id() {
        let (event_tx, mut event_rx) = mpsc::unbounded_channel();
        let current_message_id = StdMutex::new(None::<String>);
        let last_completed_message_id = StdMutex::new(None::<String>);
        let current_reasoning_message_id = StdMutex::new(None::<String>);
        let last_usage = StdMutex::new(None::<tiy_core::types::Usage>);
        let reasoning_buffer = StdMutex::new(String::new());
        let assistant = sample_partial_assistant_message();

        handle_agent_event(
            "run-discard",
            &event_tx,
            &current_message_id,
            &last_completed_message_id,
            &current_reasoning_message_id,
            &last_usage,
            &reasoning_buffer,
            TEST_CONTEXT_WINDOW,
            TEST_MODEL_DISPLAY_NAME,
            &AgentEvent::MessageEnd {
                message: AgentMessage::Assistant(assistant.clone()),
            },
        );
        handle_agent_event(
            "run-discard",
            &event_tx,
            &current_message_id,
            &last_completed_message_id,
            &current_reasoning_message_id,
            &last_usage,
            &reasoning_buffer,
            TEST_CONTEXT_WINDOW,
            TEST_MODEL_DISPLAY_NAME,
            &AgentEvent::MessageDiscarded {
                message: AgentMessage::Assistant(assistant),
                reason: "Incomplete anthropic stream: missing message_stop".to_string(),
            },
        );

        let events = std::iter::from_fn(|| event_rx.try_recv().ok()).collect::<Vec<_>>();
        let completed_message_id = events.iter().find_map(|event| match event {
            ThreadStreamEvent::MessageCompleted { message_id, .. } => Some(message_id.clone()),
            _ => None,
        });
        let discarded_message_id = events.iter().find_map(|event| match event {
            ThreadStreamEvent::MessageDiscarded { message_id, .. } => Some(message_id.clone()),
            _ => None,
        });

        assert!(completed_message_id.is_some());
        assert_eq!(completed_message_id, discarded_message_id);
    }

    #[test]
    fn helper_profiles_match_explore_and_review_tools() {
        assert_eq!(
            resolve_helper_profile(RuntimeOrchestrationTool::Explore),
            SubagentProfile::Explore,
        );
        assert_eq!(
            resolve_helper_profile(RuntimeOrchestrationTool::Review),
            SubagentProfile::Review,
        );
    }

    #[test]
    fn update_plan_tool_is_available_in_both_runtime_profiles() {
        for profile in [DEFAULT_FULL_TOOL_PROFILE, PLAN_READ_ONLY_TOOL_PROFILE] {
            let tools = runtime_tools_for_profile(profile);
            let tool_names: Vec<&str> = tools.iter().map(|tool| tool.name.as_str()).collect();

            assert!(tool_names.contains(&"update_plan"));
        }
    }

    #[test]
    fn explore_and_review_use_auxiliary_model_when_available() {
        let model_plan =
            sample_resolved_runtime_model_plan(Some(sample_resolved_model_role("assistant-model")));

        let explore_role =
            resolve_helper_model_role(&model_plan, RuntimeOrchestrationTool::Explore);
        let review_role = resolve_helper_model_role(&model_plan, RuntimeOrchestrationTool::Review);

        assert_eq!(explore_role.model_id, "assistant-model");
        assert_eq!(review_role.model_id, "assistant-model");
    }

    #[test]
    fn explore_and_review_fallback_to_primary_without_auxiliary_model() {
        let model_plan = sample_resolved_runtime_model_plan(None);

        let explore_role =
            resolve_helper_model_role(&model_plan, RuntimeOrchestrationTool::Explore);
        let review_role = resolve_helper_model_role(&model_plan, RuntimeOrchestrationTool::Review);

        assert_eq!(explore_role.model_id, "primary-model");
        assert_eq!(review_role.model_id, "primary-model");
    }

    #[test]
    fn plan_mode_prompt_mentions_waiting_for_approval_after_update_plan() {
        let prompt = run_mode_prompt_body("plan");

        assert!(prompt.contains("Once you publish a plan with update_plan"));
        assert!(prompt.contains("pause for user approval"));
    }

    #[test]
    fn build_plan_artifact_extracts_numbered_steps() {
        let artifact = build_plan_artifact_from_tool_input(
            &serde_json::json!({
                "title": "Implementation Plan",
                "summary": "Produce the implementation plan.",
                "steps": [
                    "Update runtime-thread-surface.",
                    { "title": "Validate typecheck." }
                ]
            }),
            3,
        );

        assert_eq!(artifact.title, "Implementation Plan");
        assert_eq!(artifact.plan_revision, 3);
        assert_eq!(artifact.steps[0].title, "Update runtime-thread-surface.");
        assert_eq!(artifact.steps[1].title, "Validate typecheck.");
    }

    #[test]
    fn convert_history_messages_keeps_plan_checkpoints_but_skips_approval_prompts() {
        let artifact = build_plan_artifact_from_tool_input(
            &serde_json::json!({
                "title": "Plan title",
                "summary": "Carry the previous plan forward.",
                "steps": ["Keep the plan in follow-up context."]
            }),
            2,
        );
        let plan_metadata = build_plan_message_metadata(artifact, "run-plan", "plan");
        let messages = vec![
            MessageRecord {
                id: "msg-user".to_string(),
                thread_id: "thread-1".to_string(),
                run_id: None,
                role: "user".to_string(),
                content_markdown: "Please refine the plan.".to_string(),
                message_type: "plain_message".to_string(),
                status: "completed".to_string(),
                metadata_json: None,
                created_at: String::new(),
            },
            MessageRecord {
                id: "msg-plan".to_string(),
                thread_id: "thread-1".to_string(),
                run_id: Some("run-plan".to_string()),
                role: "assistant".to_string(),
                content_markdown: "stale plan body".to_string(),
                message_type: "plan".to_string(),
                status: "completed".to_string(),
                metadata_json: Some(
                    serde_json::to_string(&plan_metadata).expect("serialize plan metadata"),
                ),
                created_at: String::new(),
            },
            MessageRecord {
                id: "msg-approval".to_string(),
                thread_id: "thread-1".to_string(),
                run_id: Some("run-plan".to_string()),
                role: "assistant".to_string(),
                content_markdown: "Review and approve the plan.".to_string(),
                message_type: "approval_prompt".to_string(),
                status: "completed".to_string(),
                metadata_json: None,
                created_at: String::new(),
            },
        ];

        let history =
            convert_history_messages(&messages, &sample_resolved_model_role("primary").model);

        assert_eq!(history.len(), 2);
        assert_eq!(message_text(&history[0]), "Please refine the plan.");
        assert!(message_text(&history[1])
            .contains("Implementation plan checkpoint (revision 2, approval state: pending):"));
        assert!(message_text(&history[1]).contains("# Plan title"));
        assert!(message_text(&history[1]).contains("Keep the plan in follow-up context."));
    }
}
