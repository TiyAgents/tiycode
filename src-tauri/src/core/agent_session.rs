use base64::{engine::general_purpose, Engine as _};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex as StdMutex, Weak};

use sqlx::SqlitePool;
use tiycore::agent::{
    Agent, AgentError, AgentMessage, AgentTool, AgentToolResult, ToolExecutionMode,
};
use tiycore::thinking::ThinkingLevel;
use tiycore::types::{
    AssistantMessage, AssistantMessageEvent, ContentBlock, Cost, ImageContent, InputType, Model,
    OpenAICompletionsCompat, Provider, StopReason, TextContent, ThinkingContent, ToolCall,
    ToolResultMessage, Transport, Usage, UserMessage,
};
use tokio::sync::mpsc;

use crate::core::context_compression::ContextTokenCalibration;
use crate::core::plan_checkpoint::{
    approval_prompt_markdown, build_approval_prompt_metadata, build_plan_artifact_from_tool_input,
    build_plan_message_metadata, parse_plan_message_metadata, plan_markdown, write_plan_file,
};
use crate::core::prompt;
use crate::core::subagent::{
    extract_review_report, runtime_orchestration_tools, HelperAgentOrchestrator, HelperRunRequest,
    ReviewRequest, RuntimeOrchestrationTool, SubagentProfile, TERM_CLOSE_TOOL_DESCRIPTION,
    TERM_OUTPUT_TOOL_DESCRIPTION, TERM_RESTART_TOOL_DESCRIPTION, TERM_STATUS_TOOL_DESCRIPTION,
    TERM_WRITE_TOOL_DESCRIPTION,
};
use crate::core::tool_gateway::{
    ApprovalRequest, ToolExecutionOptions, ToolExecutionRequest, ToolGateway, ToolGatewayResult,
};
use crate::extensions::ExtensionsManager;
use crate::ipc::frontend_channels::ThreadStreamEvent;
use crate::model::errors::{AppError, ErrorSource};
use crate::model::provider::AgentProfileRecord;
use crate::model::thread::{
    MessageAttachmentDto, MessageRecord, RunSummaryDto, RunUsageDto, ToolCallDto,
};
use crate::persistence::repo::{message_repo, provider_repo, run_repo, tool_call_repo};

/// Deprecated: previously used as the hard limit for `message_repo::list_recent` in
/// `build_session_spec`.  Replaced by `message_repo::list_since_last_reset()` which
/// queries the DB for the reset boundary directly.  Retained for reference only.
#[allow(dead_code)]
const MESSAGE_HISTORY_LIMIT: i64 = 200;
const DEFAULT_CONTEXT_WINDOW: u32 = 128_000;
const DEFAULT_MAX_OUTPUT_TOKENS: u32 = 32_000;
const DEFAULT_FULL_TOOL_PROFILE: &str = "default_full";
const PLAN_READ_ONLY_TOOL_PROFILE: &str = "plan_read_only";
const STANDARD_TOOL_TIMEOUT_SECS: u64 = 120;
const SUBAGENT_TOOL_TIMEOUT_SECS: u64 = 600;
/// Main agent timeout is effectively unlimited (24 h) because user-interactive
/// tools like `clarify` and approval prompts must wait for human input without
/// being killed by the outer tiycore timeout.  The per-tool execution timeout
/// inside `tool_gateway` (120 s) still guards against runaway non-interactive
/// tool calls.
const MAIN_AGENT_TOOL_TIMEOUT_SECS: u64 = 86_400;
const CLARIFY_TOOL_NAME: &str = "clarify";
const PLAN_MODE_MISSING_CHECKPOINT_ERROR: &str =
    "Plan mode requires publishing a plan with update_plan before the run can finish.";
const TEXT_ATTACHMENT_MAX_CHARS: usize = 12_000;

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
    pub supports_image_input: Option<bool>,
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
    pub runtime_tools: Vec<AgentTool>,
    pub system_prompt: String,
    pub history_messages: Vec<MessageRecord>,
    pub history_tool_calls: Vec<ToolCallDto>,
    pub model_plan: ResolvedRuntimeModelPlan,
    pub initial_prompt: Option<String>,
    pub initial_context_calibration: ContextTokenCalibration,
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
    let tool_profile_name = resolve_tool_profile_name(&raw_plan, run_mode);
    // Load all messages since the last context reset marker (or all messages
    // if no reset exists).  The new `list_since_last_reset` queries the DB
    // directly for the reset boundary, removing the former hard-coded 200-row
    // limit (`MESSAGE_HISTORY_LIMIT`) that could silently discard older
    // context_reset markers.
    let history_messages = message_repo::list_since_last_reset(pool, thread_id).await?;

    // Load tool calls from all runs referenced by the history messages so that
    // convert_history_messages can reconstruct assistant-tool-call / tool-result
    // pairs that are not stored in the messages table.
    let history_run_ids: Vec<String> = history_messages
        .iter()
        .filter_map(|m| m.run_id.clone())
        .collect::<std::collections::HashSet<_>>()
        .into_iter()
        .collect();
    let history_tool_calls =
        tool_call_repo::list_parent_visible_by_run_ids(pool, &history_run_ids).await?;
    let latest_historical_run =
        run_repo::find_latest_with_prompt_usage_by_thread_excluding_run(pool, thread_id, run_id)
            .await?;

    let system_prompt = build_system_prompt(pool, &raw_plan, workspace_path, run_mode).await?;
    let extension_tools = ExtensionsManager::new(pool.clone())
        .list_runtime_agent_tools(Some(workspace_path))
        .await?;
    let initial_context_calibration = build_initial_context_token_calibration(
        latest_historical_run.as_ref(),
        &history_messages,
        &history_tool_calls,
        &resolved_plan.primary,
        &system_prompt,
    );

    Ok(AgentSessionSpec {
        run_id: run_id.to_string(),
        thread_id: thread_id.to_string(),
        workspace_path: workspace_path.to_string(),
        run_mode: run_mode.to_string(),
        tool_profile_name: tool_profile_name.clone(),
        runtime_tools: runtime_tools_for_profile_with_extensions(
            &tool_profile_name,
            extension_tools,
        ),
        system_prompt,
        history_messages,
        history_tool_calls,
        model_plan: resolved_plan,
        initial_prompt: None,
        initial_context_calibration,
    })
}

pub(crate) fn trim_history_to_current_context(messages: &[MessageRecord]) -> Vec<MessageRecord> {
    let start_index = messages
        .iter()
        .rposition(is_context_reset_marker)
        .map(|index| index + 1)
        .unwrap_or(0);

    messages[start_index..]
        .iter()
        .filter(|message| message.status != "discarded")
        .cloned()
        .collect()
}

fn effective_prompt_tokens(input_tokens: u64, cache_read_tokens: u64) -> u64 {
    input_tokens.saturating_add(cache_read_tokens)
}

#[derive(Debug, Default)]
struct ContextCompressionRuntimeState {
    calibration: ContextTokenCalibration,
    pending_prompt_estimate: Option<u32>,
}

impl ContextCompressionRuntimeState {
    fn new(initial_calibration: ContextTokenCalibration) -> Self {
        Self {
            calibration: initial_calibration,
            pending_prompt_estimate: None,
        }
    }

    fn calibration(&self) -> ContextTokenCalibration {
        self.calibration
    }

    fn record_pending_prompt_estimate(&mut self, estimated_tokens: u32) {
        self.pending_prompt_estimate = Some(estimated_tokens);
    }

    fn observe_prompt_usage(&mut self, actual_prompt_tokens: u64) {
        let Some(estimated_tokens) = self.pending_prompt_estimate.take() else {
            return;
        };

        self.calibration = self
            .calibration
            .observe(estimated_tokens, actual_prompt_tokens);
    }
}

fn current_context_token_calibration(
    state: &StdMutex<ContextCompressionRuntimeState>,
) -> ContextTokenCalibration {
    state
        .lock()
        .map(|state| state.calibration())
        .unwrap_or_default()
}

fn record_pending_prompt_estimate(
    state: &StdMutex<ContextCompressionRuntimeState>,
    estimated_tokens: u32,
) {
    if let Ok(mut state) = state.lock() {
        state.record_pending_prompt_estimate(estimated_tokens);
    }
}

fn observe_context_usage_calibration(
    state: &StdMutex<ContextCompressionRuntimeState>,
    usage: &Usage,
) {
    let actual_prompt_tokens = effective_prompt_tokens(usage.input, usage.cache_read);
    if actual_prompt_tokens == 0 {
        return;
    }

    if let Ok(mut state) = state.lock() {
        state.observe_prompt_usage(actual_prompt_tokens);
    }
}

fn build_initial_context_token_calibration(
    latest_historical_run: Option<&RunSummaryDto>,
    history_messages: &[MessageRecord],
    history_tool_calls: &[ToolCallDto],
    primary_model: &ResolvedModelRole,
    system_prompt: &str,
) -> ContextTokenCalibration {
    let Some(latest_historical_run) = latest_historical_run else {
        return ContextTokenCalibration::default();
    };

    let historical_prompt_tokens = effective_prompt_tokens(
        latest_historical_run.usage.input_tokens,
        latest_historical_run.usage.cache_read_tokens,
    );
    if historical_prompt_tokens == 0
        || !run_summary_matches_primary_model(latest_historical_run, primary_model)
    {
        return ContextTokenCalibration::default();
    }

    let history =
        convert_history_messages(history_messages, history_tool_calls, &primary_model.model);
    let estimated_tokens = crate::core::context_compression::estimate_total_tokens(&history)
        .saturating_add(crate::core::context_compression::estimate_tokens(
            system_prompt,
        ));

    ContextTokenCalibration::from_observation(estimated_tokens, historical_prompt_tokens)
        .unwrap_or_default()
}

fn run_summary_matches_primary_model(
    run_summary: &RunSummaryDto,
    primary_model: &ResolvedModelRole,
) -> bool {
    run_summary.model_id.as_deref() == Some(primary_model.model_id.as_str())
        || run_summary.model_id.as_deref() == Some(primary_model.model.id.as_str())
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
    abort_signal: tiycore::agent::AbortSignal,
    context_compression_state: Arc<StdMutex<ContextCompressionRuntimeState>>,
}

impl AgentSession {
    pub fn new(
        pool: SqlitePool,
        tool_gateway: Arc<ToolGateway>,
        helper_orchestrator: Arc<HelperAgentOrchestrator>,
        event_tx: mpsc::UnboundedSender<ThreadStreamEvent>,
        spec: AgentSessionSpec,
        max_turns: usize,
    ) -> Arc<Self> {
        Arc::new_cyclic(|weak_self| {
            let agent = Arc::new(Agent::with_model(spec.model_plan.primary.model.clone()));
            let context_compression_state = Arc::new(StdMutex::new(
                ContextCompressionRuntimeState::new(spec.initial_context_calibration),
            ));
            agent.set_max_turns(max_turns);
            configure_agent(
                &agent,
                &spec,
                weak_self.clone(),
                Arc::clone(&context_compression_state),
            );

            Self {
                spec,
                pool,
                tool_gateway,
                helper_orchestrator,
                event_tx,
                agent,
                cancel_requested: Arc::new(AtomicBool::new(false)),
                checkpoint_requested: AtomicBool::new(false),
                abort_signal: tiycore::agent::AbortSignal::new(),
                context_compression_state,
            }
        })
    }

    pub fn start(self: Arc<Self>) -> tokio::task::JoinHandle<()> {
        tokio::spawn(async move {
            self.run().await;
        })
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
        let context_compression_state_ref = Arc::clone(&self.context_compression_state);
        let unsubscribe = self.agent.subscribe(move |event| {
            handle_agent_event(
                &run_id,
                &event_tx,
                &message_id_ref,
                &last_completed_message_id_ref,
                &reasoning_message_id_ref,
                &last_usage_ref,
                &context_compression_state_ref,
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
                } else if let Some(error) = plan_mode_missing_checkpoint_error(
                    &self.spec.run_mode,
                    self.checkpoint_requested.load(Ordering::SeqCst),
                ) {
                    let _ = self.event_tx.send(ThreadStreamEvent::RunFailed {
                        run_id: self.spec.run_id.clone(),
                        error: error.to_string(),
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

    /// Persist context_summary and context_reset markers to the DB after
    /// auto-compression.
    ///
    /// `boundary_message_id` is the id of the **first DB-backed message we
    /// intend to keep** in the next run's loaded history. It is stored inside
    /// the reset marker's metadata as `boundaryMessageId` so that
    /// `list_since_last_reset` can use it as the lower bound — this is the
    /// only way to keep the DB view aligned with the in-memory `cut_point`,
    /// because UUID v7 timing means the reset marker itself is written *after*
    /// some messages that the cut_point means to preserve.
    ///
    /// The reset marker is written first, then the summary, so both rows have
    /// `id > boundary_message_id` and the next `list_since_last_reset(…)` call
    /// will return `[boundary_message, …, reset_marker, summary_marker]` in
    /// chronological order.
    pub async fn persist_compression_markers(
        &self,
        thread_id: &str,
        summary: &str,
        source: &str,
        boundary_message_id: Option<&str>,
    ) -> Result<(), crate::model::errors::AppError> {
        persist_compression_markers_to_pool(
            &self.pool,
            thread_id,
            summary,
            source,
            boundary_message_id,
        )
        .await
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

        if tool_name == "create_task" || tool_name == "update_task" || tool_name == "query_task" {
            return self
                .execute_task_tool(tool_name, tool_call_id, tool_input)
                .await;
        }

        if tool_name == CLARIFY_TOOL_NAME {
            return self
                .execute_clarify_request(tool_name, tool_call_id, tool_input)
                .await;
        }

        let tool_call_storage_id = uuid::Uuid::now_v7().to_string();
        let insert_result = tool_call_repo::insert(
            &self.pool,
            &tool_call_repo::ToolCallInsert {
                id: tool_call_storage_id.clone(),
                tool_call_id: tool_call_id.to_string(),
                run_id: self.spec.run_id.clone(),
                thread_id: self.spec.thread_id.clone(),
                helper_id: None,
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
                .execute_helper_tool(tool, tool_call_id, &tool_call_storage_id, tool_input)
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
            tool_call_storage_id: tool_call_storage_id.clone(),
            tool_name: tool_name.to_string(),
            tool_input: tool_input.clone(),
            workspace_path: self.spec.workspace_path.clone(),
            run_mode: self.spec.run_mode.clone(),
        };

        let event_tx = self.event_tx.clone();
        let run_id = self.spec.run_id.clone();
        let tool_call_id_owned = tool_call_id.to_string();
        let outcome = self
            .tool_gateway
            .execute_tool_call(
                request,
                self.abort_signal.clone(),
                ToolExecutionOptions {
                    allow_user_approval: true,
                    execution_timeout: Some(standard_tool_timeout()),
                },
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
            )
            .await;

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
                    ToolGatewayResult::TimedOut { timeout_secs, .. } => {
                        let message =
                            format!("Tool '{}' timed out after {}s", tool_name, timeout_secs);
                        let result = serde_json::json!({ "error": message.clone() });
                        tool_call_repo::update_result(
                            &self.pool,
                            &tool_call_storage_id,
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

    async fn execute_clarify_request(
        &self,
        tool_name: &str,
        tool_call_id: &str,
        tool_input: &serde_json::Value,
    ) -> AgentToolResult {
        if let Err(error) = validate_clarify_input(tool_input) {
            let _ = self.event_tx.send(ThreadStreamEvent::ToolFailed {
                run_id: self.spec.run_id.clone(),
                tool_call_id: tool_call_id.to_string(),
                error: error.clone(),
            });
            return agent_error_result(error);
        }

        let tool_call_storage_id = uuid::Uuid::now_v7().to_string();
        if let Err(error) = tool_call_repo::insert(
            &self.pool,
            &tool_call_repo::ToolCallInsert {
                id: tool_call_storage_id.clone(),
                tool_call_id: tool_call_id.to_string(),
                run_id: self.spec.run_id.clone(),
                thread_id: self.spec.thread_id.clone(),
                helper_id: None,
                tool_name: tool_name.to_string(),
                tool_input_json: tool_input.to_string(),
                status: "requested".to_string(),
            },
        )
        .await
        {
            return agent_error_result(format!("failed to persist tool call: {error}"));
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
            tool_call_storage_id: tool_call_storage_id.clone(),
            tool_name: tool_name.to_string(),
            tool_input: tool_input.clone(),
            workspace_path: self.spec.workspace_path.clone(),
            run_mode: self.spec.run_mode.clone(),
        };

        match self
            .tool_gateway
            .request_clarification(request, self.abort_signal.clone(), {
                let event_tx = self.event_tx.clone();
                let run_id = self.spec.run_id.clone();
                let tool_call_id = tool_call_id.to_string();
                let tool_name = tool_name.to_string();
                let tool_input = tool_input.clone();
                move || {
                    let _ = event_tx.send(ThreadStreamEvent::ClarifyRequired {
                        run_id: run_id.clone(),
                        tool_call_id: tool_call_id.clone(),
                        tool_name: tool_name.clone(),
                        tool_input: tool_input.clone(),
                    });
                }
            })
            .await
        {
            Ok(output) => {
                let _ = self.event_tx.send(ThreadStreamEvent::ClarifyResolved {
                    run_id: self.spec.run_id.clone(),
                    tool_call_id: tool_call_id.to_string(),
                    response: output.result.clone(),
                });
                let _ = self.event_tx.send(ThreadStreamEvent::ToolCompleted {
                    run_id: self.spec.run_id.clone(),
                    tool_call_id: tool_call_id.to_string(),
                    result: output.result.clone(),
                });
                agent_tool_result_from_output(output)
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
        tool_call_storage_id: &str,
        tool_input: &serde_json::Value,
    ) -> AgentToolResult {
        let (task, review_request) = if tool == RuntimeOrchestrationTool::Review {
            match ReviewRequest::from_tool_input(tool_input) {
                Ok(request) => (request.to_helper_prompt(), Some(request)),
                Err(error) => {
                    tool_call_repo::update_result(
                        &self.pool,
                        tool_call_storage_id,
                        &serde_json::json!({ "error": error }).to_string(),
                        "failed",
                    )
                    .await
                    .ok();
                    return agent_error_result(error);
                }
            }
        } else {
            let task = tool_input
                .get("task")
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default()
                .trim()
                .to_string();
            (task, None)
        };

        if task.is_empty() {
            tool_call_repo::update_result(
                &self.pool,
                tool_call_storage_id,
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
                let review_report = if tool == RuntimeOrchestrationTool::Review {
                    summary
                        .raw_summary
                        .as_deref()
                        .and_then(extract_review_report)
                } else {
                    None
                };

                let result = serde_json::json!({
                    "summary": summary.summary.clone(),
                    "rawSummary": summary.raw_summary.clone(),
                    "snapshot": summary.snapshot,
                    "reviewRequest": review_request,
                    "reviewReport": review_report,
                });
                tool_call_repo::update_result(
                    &self.pool,
                    tool_call_storage_id,
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
                    tool_call_storage_id,
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
            attachments_json: None,
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
            attachments_json: None,
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

        // Persist plan markdown to ~/.tiy/plans/{thread_id}.md for incremental
        // plan refinement and downstream review verification.
        let plan_file_path = write_plan_file(&self.spec.thread_id, &plan_message.content_markdown)
            .ok()
            .map(|path| path.to_string_lossy().to_string());

        if let Some(ref path) = plan_file_path {
            tracing::info!(
                thread_id = %self.spec.thread_id,
                plan_revision = plan_revision,
                path = %path,
                "plan file written to disk"
            );
        }

        self.checkpoint_requested.store(true, Ordering::SeqCst);
        self.abort_signal.cancel();
        self.agent.abort();

        let result_message = match &plan_file_path {
            Some(path) => format!(
                "Implementation plan published and saved to {path}. Waiting for approval before execution."
            ),
            None => "Implementation plan published. Waiting for approval before execution.".to_string(),
        };

        AgentToolResult {
            content: vec![ContentBlock::Text(TextContent::new(result_message))],
            details: Some(serde_json::json!({
                "planMessageId": plan_message_id,
                "approvalMessageId": approval_message_id,
                "planFilePath": plan_file_path,
                "plan": artifact,
            })),
        }
    }

    async fn execute_task_tool(
        &self,
        tool_name: &str,
        tool_call_id: &str,
        tool_input: &serde_json::Value,
    ) -> AgentToolResult {
        use crate::core::task_board_manager;
        use crate::model::task_board::{CreateTaskInput, QueryTaskInput, UpdateTaskInput};

        // Persist the tool call record
        let tool_call_storage_id = uuid::Uuid::now_v7().to_string();
        if let Err(error) = tool_call_repo::insert(
            &self.pool,
            &tool_call_repo::ToolCallInsert {
                id: tool_call_storage_id.clone(),
                tool_call_id: tool_call_id.to_string(),
                run_id: self.spec.run_id.clone(),
                thread_id: self.spec.thread_id.clone(),
                helper_id: None,
                tool_name: tool_name.to_string(),
                tool_input_json: tool_input.to_string(),
                status: "running".to_string(),
            },
        )
        .await
        {
            return agent_error_result(format!("failed to persist tool call: {error}"));
        }

        let _ = self.event_tx.send(ThreadStreamEvent::ToolRunning {
            run_id: self.spec.run_id.clone(),
            tool_call_id: tool_call_id.to_string(),
        });

        let result: Result<serde_json::Value, String> = if tool_name == "create_task" {
            match serde_json::from_value::<CreateTaskInput>(tool_input.clone()) {
                Ok(input) => {
                    match task_board_manager::create_task_board(
                        &self.pool,
                        &self.spec.thread_id,
                        &input,
                    )
                    .await
                    {
                        Ok(dto) => {
                            let _ = self.event_tx.send(ThreadStreamEvent::TaskBoardUpdated {
                                run_id: self.spec.run_id.clone(),
                                task_board: dto.clone(),
                            });
                            serde_json::to_value(&dto).map_err(|error| {
                                format!("Failed to serialize create_task result: {error}")
                            })
                        }
                        Err(e) => Err(e.to_string()),
                    }
                }
                Err(e) => Err(format!("Invalid create_task input: {}", e)),
            }
        } else if tool_name == "update_task" {
            match serde_json::from_value::<UpdateTaskInput>(tool_input.clone()) {
                Ok(input) => {
                    match task_board_manager::update_task_board(
                        &self.pool,
                        &self.spec.thread_id,
                        &input,
                    )
                    .await
                    {
                        Ok(dto) => {
                            let _ = self.event_tx.send(ThreadStreamEvent::TaskBoardUpdated {
                                run_id: self.spec.run_id.clone(),
                                task_board: dto.clone(),
                            });
                            serde_json::to_value(&dto).map_err(|error| {
                                format!("Failed to serialize update_task result: {error}")
                            })
                        }
                        Err(e) => Err(e.to_string()),
                    }
                }
                Err(e) => Err(format!("Invalid update_task input: {}", e)),
            }
        } else if tool_name == "query_task" {
            match serde_json::from_value::<QueryTaskInput>(tool_input.clone()) {
                Ok(input) => task_board_manager::query_thread_task_boards(
                    &self.pool,
                    &self.spec.thread_id,
                    input.scope,
                )
                .await
                .map_err(|error| error.to_string())
                .and_then(|result| {
                    serde_json::to_value(&result)
                        .map_err(|error| format!("Failed to serialize query_task result: {error}"))
                }),
                Err(e) => Err(format!("Invalid query_task input: {}", e)),
            }
        } else {
            Err(format!("Unknown task tool: {}", tool_name))
        };

        match result {
            Ok(result_json) => {
                tool_call_repo::update_result(
                    &self.pool,
                    &tool_call_storage_id,
                    &result_json.to_string(),
                    "completed",
                )
                .await
                .ok();

                let _ = self.event_tx.send(ThreadStreamEvent::ToolCompleted {
                    run_id: self.spec.run_id.clone(),
                    tool_call_id: tool_call_id.to_string(),
                    result: result_json.clone(),
                });

                AgentToolResult {
                    content: vec![ContentBlock::Text(TextContent::new(
                        serde_json::to_string(&result_json)
                            .unwrap_or_else(|_| "Task updated successfully".to_string()),
                    ))],
                    details: Some(result_json),
                }
            }
            Err(error) => {
                let error_json = serde_json::json!({ "error": &error });
                tool_call_repo::update_result(
                    &self.pool,
                    &tool_call_storage_id,
                    &error_json.to_string(),
                    "failed",
                )
                .await
                .ok();

                let _ = self.event_tx.send(ThreadStreamEvent::ToolFailed {
                    run_id: self.spec.run_id.clone(),
                    tool_call_id: tool_call_id.to_string(),
                    error: error.clone(),
                });

                agent_error_result(error)
            }
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

fn configure_agent(
    agent: &Arc<Agent>,
    spec: &AgentSessionSpec,
    weak_self: Weak<AgentSession>,
    context_compression_state: Arc<StdMutex<ContextCompressionRuntimeState>>,
) {
    agent.set_system_prompt(spec.system_prompt.clone());
    agent.replace_messages(convert_history_messages(
        &spec.history_messages,
        &spec.history_tool_calls,
        &spec.model_plan.primary.model,
    ));
    agent.set_tools(spec.runtime_tools.clone());
    agent.set_tool_execution(ToolExecutionMode::Sequential);
    agent.set_thinking_level(spec.model_plan.thinking_level);
    agent.set_transport(spec.model_plan.transport);
    agent.set_security_config(main_agent_security_config());

    // Context compression: when messages exceed the token budget, generate
    // a summary with the primary model, persist markers to DB, and keep only
    // recent messages. Falls back to pure truncation if the LLM call fails.
    //
    // Two correctness hazards addressed here:
    //
    // 1. UUID v7 timing. The reset marker we write to DB uses `now_v7()`, but
    //    `cut_point` in-memory points at a slice that includes messages the
    //    current run persisted EARLIER than this call. A naive
    //    `list_since_last_reset WHERE id >= reset_id` would exclude those
    //    earlier messages and effectively "lose" the current user prompt on
    //    the next reload. We therefore resolve a DB-backed boundary id
    //    conservatively covering all recent messages and attach it to the
    //    reset marker's metadata.
    //
    // 2. Summary-of-summary decay. If a previous auto-compression already
    //    injected a `<context_summary>` as the head of `messages`, naively
    //    re-summarising `old_messages` would re-summarise an already-
    //    summarised prefix, losing detail each pass. Instead we detect the
    //    prior summary, treat it as a pinned prefix, and ask the model to
    //    **merge** the prior summary with the delta of messages since then.
    let compression_settings = crate::core::context_compression::CompressionSettings::new(
        spec.model_plan.primary.model.context_window,
    );
    let primary_model_role = spec.model_plan.primary.clone();
    let compression_weak_self = weak_self.clone();
    let compression_thread_id = spec.thread_id.clone();
    let compression_run_id = spec.run_id.clone();
    let compression_response_language = spec.model_plan.raw.response_language.clone();
    let compression_state = Arc::clone(&context_compression_state);
    // Pre-compute the system prompt's estimated token count so the
    // compression check includes fixed overhead that the provider counts
    // against the context window but `estimate_total_tokens(messages)`
    // does not see.  This narrows the gap the calibration ratio has to
    // cover, making the trigger point more predictable.
    let system_prompt_estimated_tokens =
        crate::core::context_compression::estimate_tokens(&spec.system_prompt);
    agent.set_transform_context(move |messages| {
        // Cheap pass-through check first: only clone the heavy captured state
        // (ResolvedModelRole, String ids, Weak) when compression will actually
        // run. For long sessions with many turns this avoids per-turn heap
        // allocations when the thread is still well under budget.
        let settings = compression_settings.clone();
        let raw_estimated_tokens =
            crate::core::context_compression::estimate_total_tokens(&messages)
                .saturating_add(system_prompt_estimated_tokens);
        let calibration = current_context_token_calibration(&compression_state);
        let calibrated_total_tokens = crate::core::context_compression::calibrate_total_tokens(
            raw_estimated_tokens,
            Some(calibration),
        );
        let needs_compression = !messages.is_empty()
            && crate::core::context_compression::should_compress_total_tokens(
                calibrated_total_tokens,
                &settings,
            );

        let model_role = if needs_compression {
            Some(primary_model_role.clone())
        } else {
            None
        };
        let weak = if needs_compression {
            Some(compression_weak_self.clone())
        } else {
            None
        };
        let thread_id = if needs_compression {
            Some(compression_thread_id.clone())
        } else {
            None
        };
        let run_id = if needs_compression {
            Some(compression_run_id.clone())
        } else {
            None
        };
        let response_language = if needs_compression {
            Some(compression_response_language.clone())
        } else {
            None
        };
        let compression_state = Arc::clone(&compression_state);

        async move {
            let transformed_messages = if !needs_compression {
                messages
            } else {
                // Unwraps are sound: all `Some(_)` are populated together under
                // `needs_compression`, so either all four are `Some` (compression
                // path) or we returned above.
                run_auto_compression(
                    messages,
                    settings,
                    model_role.expect("model_role populated when compressing"),
                    weak.expect("weak populated when compressing"),
                    thread_id.expect("thread_id populated when compressing"),
                    run_id.expect("run_id populated when compressing"),
                    response_language.expect("response_language populated when compressing"),
                )
                .await
            };
            let sent_estimated_tokens =
                crate::core::context_compression::estimate_total_tokens(&transformed_messages)
                    .saturating_add(system_prompt_estimated_tokens);
            record_pending_prompt_estimate(&compression_state, sent_estimated_tokens);
            transformed_messages
        }
    });

    if let Some(api_key) = spec.model_plan.primary.api_key.clone() {
        agent.set_api_key(api_key);
    }

    // Set session ID for prompt caching (used as prompt_cache_key in OpenAI Responses API).
    // Using thread_id ensures the same conversation thread shares a cache across runs.
    agent.set_session_id(&spec.thread_id);

    // Inject default TiyCode identification headers for all LLM API requests.
    agent.set_custom_headers(crate::core::tiycode_default_headers());

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
    context_compression_state: &StdMutex<ContextCompressionRuntimeState>,
    reasoning_buffer: &StdMutex<String>,
    context_window: &str,
    model_display_name: &str,
    event: &tiycore::agent::AgentEvent,
) {
    match event {
        tiycore::agent::AgentEvent::TurnRetrying {
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
        tiycore::agent::AgentEvent::MessageUpdate {
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
                            thinking_signature: None,
                        });
                    }
                }
                AssistantMessageEvent::ThinkingEnd {
                    content, partial, ..
                } => {
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

                    // Extract thinking_signature from the partial message's last
                    // Thinking content block.  The signature is populated by the
                    // protocol layer during streaming and is complete by the time
                    // ThinkingEnd fires.
                    let thinking_signature = partial
                        .content
                        .iter()
                        .rev()
                        .find_map(|b| b.as_thinking())
                        .and_then(|t| t.thinking_signature.clone());

                    let message_id = ensure_message_id(current_reasoning_message_id);
                    let _ = event_tx.send(ThreadStreamEvent::ReasoningUpdated {
                        run_id: run_id.to_string(),
                        message_id,
                        reasoning,
                        thinking_signature,
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
                    context_compression_state,
                    &partial.usage,
                    context_window,
                    model_display_name,
                );
            }
        }
        tiycore::agent::AgentEvent::MessageEnd { message } => {
            if let AgentMessage::Assistant(assistant) = message {
                let content = assistant.text_content();

                // Skip emitting MessageCompleted when the assistant produced
                // no usable text content.  Two sub-cases:
                //
                // a) Empty content WITH tool calls — the tool-call-only path.
                //    Tool calls are persisted separately; no plain_message needed.
                //
                // b) Empty content WITHOUT tool calls — typically a provider
                //    error (transport error, 500, 403, etc.) that interrupted
                //    the stream before any text was generated.  Persisting an
                //    empty plain_message would poison the history: on the next
                //    run, convert_history_messages creates an AssistantMessage
                //    with only a Text("") block; tiycore serialises it with
                //    `content: null` (the empty text is filtered) while
                //    reasoning_content may be present, causing DeepSeek to
                //    reject the request with 400.
                if content.is_empty() {
                    emit_usage_update_if_changed(
                        run_id,
                        event_tx,
                        last_usage,
                        context_compression_state,
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
                    context_compression_state,
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
        tiycore::agent::AgentEvent::MessageDiscarded { reason, .. } => {
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
    context_compression_state: &StdMutex<ContextCompressionRuntimeState>,
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

    observe_context_usage_calibration(context_compression_state, usage);

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
            "Search the current workspace with a built-in cross-platform search engine. Supports literal or regex queries, optional context lines, file glob filters, and files/count output modes. Results are preview-limited for safety; omit wildcard-only filePattern values like '*' or '**/*'.",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "query": {
                        "type": "string",
                        "description": "Search text or regex pattern. Defaults to literal mode, so special regex characters are matched as plain text unless queryMode='regex'."
                    },
                    "directory": {
                        "type": "string",
                        "description": "Directory to search in (default: workspace root)."
                    },
                    "filePattern": {
                        "type": "string",
                        "description": "Optional glob filter such as '*.rs' or 'src/**/*.ts'. Omit it to search all files; do not pass '*' or '**/*'."
                    },
                    "type": {
                        "type": "string",
                        "description": "Optional file type filter such as 'rust', 'ts', 'js', 'py', 'go', or 'json'. More natural than filePattern for language-targeted searches."
                    },
                    "maxResults": {
                        "type": "integer",
                        "description": "Optional preview limit for returned matches. Defaults to 100 and is capped for context safety."
                    },
                    "offset": {
                        "type": "integer",
                        "description": "Optional number of matches or files to skip before collecting results."
                    },
                    "queryMode": {
                        "type": "string",
                        "enum": ["literal", "regex"],
                        "description": "Use 'literal' for plain text matching (default) or 'regex' for regular expression search."
                    },
                    "outputMode": {
                        "type": "string",
                        "enum": ["content", "files_with_matches", "count"],
                        "description": "Choose 'content' for matching lines, 'files_with_matches' for unique matching files, or 'count' for per-file match counts."
                    },
                    "caseInsensitive": {
                        "type": "boolean",
                        "description": "Set true for case-insensitive matching."
                    },
                    "context": {
                        "type": "integer",
                        "description": "Optional number of context lines to include before and after each match in content mode."
                    },
                    "beforeContext": {
                        "type": "integer",
                        "description": "Optional number of lines to include before each match in content mode. Overrides the shared context value for the before side."
                    },
                    "afterContext": {
                        "type": "integer",
                        "description": "Optional number of lines to include after each match in content mode. Overrides the shared context value for the after side."
                    },
                    "timeoutMs": {
                        "type": "integer",
                        "description": "Optional search timeout in milliseconds. When the timeout is hit, the tool returns partial results and marks the response as incomplete."
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
            CLARIFY_TOOL_NAME,
            "Clarify",
            "Ask the user one concise question when they need to choose between reasonable options, confirm a preference, approve a risky action, define scope, or provide missing requirements before you continue. Prefer this tool over guessing when multiple valid paths exist. Offer 2-5 short options when possible, mark the recommended option, and keep the wording brief.",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "header": {
                        "type": "string",
                        "description": "Optional short label for the UI, ideally 12 characters or fewer."
                    },
                    "question": {
                        "type": "string",
                        "description": "A single concise question for the user."
                    },
                    "options": {
                        "type": "array",
                        "minItems": 2,
                        "maxItems": 5,
                        "items": {
                            "type": "object",
                            "properties": {
                                "id": { "type": "string" },
                                "label": { "type": "string" },
                                "description": { "type": "string" },
                                "recommended": { "type": "boolean" }
                            },
                            "required": ["label", "description"]
                        }
                    }
                },
                "required": ["question", "options"]
            }),
        ),
        AgentTool::new(
            "update_plan",
            "Update Plan",
            "Publish the current implementation plan and pause for user approval before execution. The plan is saved to disk and persists across runs.\n\n\
## Workflow — complete these phases before calling this tool\n\n\
Phase 1 — Explore and understand:\n\
- Use read, search, find, list, and agent_explore to inspect relevant files, modules, and patterns.\n\
- Identify existing conventions, reusable modules, constraints, and dependencies.\n\
- Do NOT call update_plan until you have grounded your understanding in actual code evidence.\n\n\
Phase 2 — Clarify ambiguities:\n\
- If implementation-blocking uncertainty remains that code exploration cannot resolve, use clarify to ask the user.\n\
- Only ask questions the user must decide: scope, preference between valid approaches, priority tradeoffs.\n\
- Do NOT ask questions that code exploration can answer. Batch related questions. Wait for the answer before continuing.\n\
- Skip this phase if exploration resolved all uncertainties.\n\n\
Phase 3 — Converge on a recommendation:\n\
- Synthesize exploration evidence and clarification answers into ONE recommended approach.\n\
- Do not present multiple unranked alternatives. Every design decision must be grounded in inspected code or user input.\n\n\
Phase 4 — Call update_plan:\n\
- Only after phases 1-3 are complete, call this tool with a plan that satisfies the quality contract below.\n\n\
## Quality contract — every plan must satisfy\n\n\
- summary: what is being changed, why, and expected outcome (2-3 sentences).\n\
- context: write a thorough narrative of confirmed facts from inspected code, docs, or user input. Connect the facts into coherent paragraphs that explain the current state, how the relevant pieces fit together, and what constraints exist. Include file paths, type signatures, data flow direction, and version or compatibility details. The goal is a self-contained briefing a developer unfamiliar with the area can read and fully understand. Never speculate about uninspected files or architecture.\n\
- design: write a detailed prose description of the recommended approach. Explain the architecture or structural changes, walk through the data flow step by step, and articulate why this approach is chosen over alternatives by comparing tradeoffs explicitly. Cover edge cases the design handles and those it defers. The reader should finish this section understanding both the what and the why at a level sufficient to implement without further design questions.\n\
- keyImplementation: write a connected prose description of the specific files, modules, interfaces, data flows, or state transitions that carry the change. For each major component, explain what it does today, what changes, and how the changed pieces interact. Include type names, function signatures, and module boundaries. Vague references like 'update the relevant files' are not acceptable.\n\
- steps: concrete, ordered, actionable steps with affected files and intended outcomes.\n\
- verification: write a thorough description of how to validate the change succeeded. Cover type-checks, unit tests, integration tests, manual smoke tests, and behavioral verification. Mention specific commands, expected outputs, and edge cases worth verifying. Explain what each check proves and why it matters.\n\
- risks: main risks, edge cases, compatibility concerns, regression areas.\n\
- assumptions (optional): only non-blocking assumptions, not open questions.\n\n\
Prohibited: unresolved core ambiguities (use clarify first), TODO placeholders, vague steps, architecture guesses not backed by exploration, lengthy background essays without actionable information.\n\n\
You may call this tool multiple times in a run to incrementally refine the plan. Each call overwrites the previous version.",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "title": { "type": "string" },
                    "summary": { "type": "string" },
                    "context": { "type": "string" },
                    "design": { "type": "string" },
                    "keyImplementation": { "type": "string" },
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
                    "verification": { "type": "string" },
                    "risks": {
                        "type": "array",
                        "items": { "type": "string" }
                    },
                    "assumptions": {
                        "type": "array",
                        "items": { "type": "string" }
                    },
                    "needsContextResetOption": { "type": "boolean" },
                    "plan": {
                        "type": "object",
                        "description": "Optional nested plan payload. If provided, the runtime reads planning fields from this object.",
                        "properties": {
                            "title": { "type": "string" },
                            "summary": { "type": "string" },
                            "context": { "type": "string" },
                            "design": { "type": "string" },
                            "keyImplementation": { "type": "string" },
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
                            "verification": { "type": "string" },
                            "risks": {
                                "type": "array",
                                "items": { "type": "string" }
                            },
                            "assumptions": {
                                "type": "array",
                                "items": { "type": "string" }
                            },
                            "needsContextResetOption": { "type": "boolean" }
                        }
                    }
                }
            }),
        ),
    ];
    tools.extend(runtime_orchestration_tools());

    // Shell is available in both profiles (plan mode applies read-only constraints via prompt).
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

    // Task tracking tools (always available)
    tools.push(AgentTool::new(
        "create_task",
        "Create Task",
        "Create a new task board with steps to track implementation progress. Use this when starting a complex multi-step implementation. After creating a board, keep it current while you work instead of waiting until the very end to update statuses.",
        serde_json::json!({
            "type": "object",
            "properties": {
                "title": {
                    "type": "string",
                    "description": "Human-readable title for the task board."
                },
                "steps": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "properties": {
                            "description": { "type": "string" }
                        },
                        "required": ["description"]
                    },
                    "description": "Ordered list of task steps."
                }
            },
            "required": ["title", "steps"]
        }),
    ));
    tools.push(AgentTool::new(
        "update_task",
        "Update Task",
        "Update a task board or its steps. Call this after completing each implementation step to keep the board in sync with actual progress. The easiest pattern: call with action='advance_step' and no stepId — this completes the current active step and automatically starts the next one (or completes the board if no steps remain). If the app was interrupted or you are unsure which taskBoardId is current, call query_task first. Call after every step, not just at the end.",
        serde_json::json!({
            "type": "object",
            "properties": {
                "taskBoardId": {
                    "type": "string",
                    "description": "ID of the task board to update. If unknown after a restart or interruption, call query_task first."
                },
                "action": {
                    "type": "string",
                    "enum": ["start_step", "advance_step", "complete_step", "fail_step", "complete_board", "abandon_board"],
                    "description": "The action to perform. Use `advance_step` after finishing each step — it completes the current active step and auto-starts the next (or auto-completes the board). No stepId needed. Use `fail_step` if a step cannot be completed. Use `start_step` only to manually start a specific pending step."
                },
                "stepId": {
                    "type": "string",
                    "description": "Step ID. Required for start_step, complete_step, fail_step. Omit for advance_step to automatically target the current active step (recommended)."
                },
                "errorDetail": {
                    "type": "string",
                    "description": "Error description (required for fail_step)."
                },
                "reason": {
                    "type": "string",
                    "description": "Optional reason for abandoning the board."
                }
            },
            "required": ["taskBoardId", "action"]
        }),
    ));
    tools.push(AgentTool::new(
        "query_task",
        "Query Task",
        "Read the current thread's task-board state. Use this when resuming work after an interruption, restart, or any time you need to recover the current taskBoardId before calling update_task.",
        serde_json::json!({
            "type": "object",
            "properties": {
                "scope": {
                    "type": "string",
                    "enum": ["active", "all"],
                    "description": "Which task boards to return. Defaults to `active`. Use `all` only when you need the full thread task-board history."
                }
            }
        }),
    ));

    tools
}

fn runtime_tools_for_profile_with_extensions(
    profile_name: &str,
    extension_tools: Vec<AgentTool>,
) -> Vec<AgentTool> {
    let mut tools = runtime_tools_for_profile(profile_name);
    let mut names = tools
        .iter()
        .map(|tool| tool.name.clone())
        .collect::<std::collections::HashSet<_>>();

    for tool in extension_tools {
        if names.insert(tool.name.clone()) {
            tools.push(tool);
        }
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

/// Maximum characters for a tool result output when replayed from history.
///
/// This deliberately oversizes vs. the aggressive (800) and recent (3200)
/// thresholds in `context_compression` so that history loaded for summary
/// generation retains enough raw material for the LLM to produce a faithful
/// summary. `render_compact_summary_history` applies its own holistic budget
/// downstream, so keeping more here improves summary quality without
/// inflating the final context sent to the primary model.
const HISTORY_TOOL_RESULT_MAX_CHARS: usize = 20_480;

pub(crate) fn convert_history_messages(
    messages: &[MessageRecord],
    tool_calls: &[ToolCallDto],
    model: &Model,
) -> Vec<AgentMessage> {
    // Timeline entries: either a real message or a pending-thinking placeholder
    // that will be merged into the next assistant message after sorting.
    enum TimelineEntry {
        Msg(AgentMessage),
        PendingThinking(ContentBlock),
    }

    // Phase 1: Convert message records into (sort_key, TimelineEntry) pairs.
    // Messages arrive from the DB in chronological order (sorted by UUID v7 id).
    // We use a zero-padded positional index as the primary sort key to preserve
    // this order exactly, then interleave tool calls using their `started_at`
    // timestamp mapped into the same key space.
    //
    // Reasoning messages are placed into the timeline as PendingThinking
    // placeholders instead of being eagerly drained into the next assistant
    // text message.  This allows the post-sort pass (Phase 4) to attach
    // thinking blocks to whichever assistant message follows them — whether
    // that is a plain text reply or a tool-call assistant message inserted by
    // Phase 2.
    let mut timeline: Vec<(SortKey, TimelineEntry)> = Vec::new();

    for (pos, message) in messages.iter().enumerate() {
        let key = SortKey::positional(pos);
        match message.message_type.as_str() {
            "reasoning" if message.role == "assistant" => {
                let signature = message
                    .metadata_json
                    .as_deref()
                    .and_then(|json| serde_json::from_str::<serde_json::Value>(json).ok())
                    .and_then(|v| v.get("thinking_signature")?.as_str().map(String::from));
                timeline.push((
                    key,
                    TimelineEntry::PendingThinking(ContentBlock::Thinking(ThinkingContent {
                        thinking: message.content_markdown.clone(),
                        thinking_signature: signature,
                        redacted: false,
                    })),
                ));
            }
            "plain_message" => match message.role.as_str() {
                "user" => {
                    timeline.push((
                        key,
                        TimelineEntry::Msg(AgentMessage::User(history_user_message(
                            message, model,
                        ))),
                    ));
                }
                "assistant" => {
                    // Skip empty assistant plain_messages — these are left
                    // behind when a provider error interrupts the stream
                    // before any text is generated.  An empty Text("") block
                    // serialises to `content: null` in tiycore, which causes
                    // DeepSeek to reject the request (content or tool_calls
                    // must be set).
                    if message.content_markdown.trim().is_empty() {
                        continue;
                    }
                    let blocks = vec![ContentBlock::Text(TextContent::new(
                        &message.content_markdown,
                    ))];
                    timeline.push((
                        key,
                        TimelineEntry::Msg(AgentMessage::Assistant(assistant_message_with_blocks(
                            blocks, model,
                        ))),
                    ));
                }
                _ => {}
            },
            "plan" if message.role == "assistant" => {
                let blocks = vec![ContentBlock::Text(TextContent::new(
                    &format_plan_history_message(message),
                ))];
                timeline.push((
                    key,
                    TimelineEntry::Msg(AgentMessage::Assistant(assistant_message_with_blocks(
                        blocks, model,
                    ))),
                ));
            }
            "summary_marker" if is_context_summary_marker(message) => {
                let summary = message.content_markdown.trim();
                if !summary.is_empty() {
                    timeline.push((
                        key,
                        TimelineEntry::Msg(AgentMessage::User(UserMessage::text(
                            summary.to_string(),
                        ))),
                    ));
                }
            }
            _ => {}
        }
    }

    // Phase 2: Interleave completed tool calls from the tool_calls table.
    //
    // When the preceding assistant text message in the same run exists in the
    // timeline, the tool call is **merged** into that message rather than
    // creating a separate assistant message.  This is critical for providers
    // like DeepSeek that require `reasoning_content` on every assistant message
    // that originally contained it — the original API response has
    // `{reasoning_content, content, tool_calls}` as a single message, and we
    // must reconstruct it the same way.
    //
    // When no preceding assistant text exists (e.g. reasoning → tool_call with
    // no intermediate text), a standalone assistant message is created so that
    // Phase 4 can attach the pending thinking block to it.
    if !tool_calls.is_empty() {
        // Build a lookup: for each message, record (run_id, created_at, position)
        // so we can find where tool calls slot in.
        let msg_positions: Vec<(Option<&str>, &str, usize)> = messages
            .iter()
            .enumerate()
            .map(|(i, m)| (m.run_id.as_deref(), m.created_at.as_str(), i))
            .collect();

        // Build an index from message position → timeline index so we can
        // find and modify existing assistant entries for merging.
        let mut pos_to_timeline_idx: std::collections::HashMap<usize, usize> =
            std::collections::HashMap::new();
        for (tl_idx, (key, _)) in timeline.iter().enumerate() {
            if key.sub == 2 {
                // sub == 2 means a positional message (not a tool call)
                pos_to_timeline_idx.insert(key.position, tl_idx);
            }
        }

        for (tc_idx, tc) in tool_calls.iter().enumerate() {
            if tc.status != "completed" {
                continue;
            }

            // Find the position for this tool call: the position of the first
            // message in the same run whose created_at >= started_at, minus a
            // small delta so the tool call lands just before it.  If no match
            // is found, place it at the end.
            let insert_pos = msg_positions
                .iter()
                .find(|(run_id, created_at, _)| {
                    run_id.map_or(false, |r| r == tc.run_id)
                        && *created_at >= tc.started_at.as_str()
                })
                .map(|(_, _, pos)| *pos)
                .unwrap_or(messages.len());

            // Try to find the preceding assistant text message in the same run
            // to merge this tool call into.  We search backwards from
            // insert_pos to find the last plain_message assistant in the same
            // run.  This handles the common case where a single API response
            // contains reasoning_content + content + tool_calls.
            //
            // We only merge when there is **no** reasoning message between the
            // candidate text message and insert_pos — a reasoning message in
            // between indicates the tool call came from a different model
            // response and must remain a separate assistant message.
            let merge_target_pos = msg_positions[..insert_pos]
                .iter()
                .rev()
                .find(|(run_id, _, pos)| {
                    run_id.map_or(false, |r| r == tc.run_id) && {
                        let m = &messages[*pos];
                        m.role == "assistant" && m.message_type == "plain_message"
                    }
                })
                .map(|(_, _, pos)| *pos)
                .filter(|&merge_pos| {
                    // Reject when any reasoning message from the same run sits
                    // between the merge target and the insert position.
                    !msg_positions[merge_pos + 1..insert_pos]
                        .iter()
                        .any(|(run_id, _, pos)| {
                            run_id.map_or(false, |r| r == tc.run_id)
                                && messages[*pos].message_type == "reasoning"
                        })
                });

            let tool_call_block =
                ContentBlock::ToolCall(ToolCall::new(&tc.id, &tc.tool_name, tc.tool_input.clone()));

            if let Some(merge_pos) = merge_target_pos {
                if let Some(&tl_idx) = pos_to_timeline_idx.get(&merge_pos) {
                    // Merge the tool call into the existing assistant message.
                    if let (_, TimelineEntry::Msg(AgentMessage::Assistant(ref mut assistant))) =
                        &mut timeline[tl_idx]
                    {
                        assistant.content.push(tool_call_block);
                        assistant.stop_reason = StopReason::ToolUse;
                    }

                    // Place the tool result right after the merged message but
                    // before the next positional entry.
                    let result_text = tc
                        .tool_output
                        .as_ref()
                        .map(|v| {
                            truncate_tool_result_text(&v.to_string(), HISTORY_TOOL_RESULT_MAX_CHARS)
                        })
                        .unwrap_or_else(|| "[no output]".to_string());
                    let tool_result =
                        ToolResultMessage::text(&tc.id, &tc.tool_name, result_text, false);
                    // Use the merge_pos + 1 so the result sorts right after
                    // the merged assistant message (sub=0 < sub=2).
                    timeline.push((
                        SortKey::before_position(merge_pos + 1, tc_idx * 2 + 1),
                        TimelineEntry::Msg(AgentMessage::ToolResult(tool_result)),
                    ));

                    continue;
                }
            }

            // No preceding assistant text to merge into — create a standalone
            // assistant message (Phase 4 will attach any pending thinking).
            //
            // However, if a previous standalone tool-call assistant was already
            // inserted at the same insert_pos (meaning the two tool calls come
            // from the same API response), merge into that message instead of
            // creating yet another standalone.  This avoids the problem where
            // only the first standalone gets reasoning_content from Phase 4.
            let merged_into_prev_standalone = timeline
                .iter()
                .rposition(|(key, entry)| {
                    key.position == insert_pos
                        && key.sub == 0
                        && matches!(entry, TimelineEntry::Msg(AgentMessage::Assistant(_)))
                })
                .and_then(|tl_idx| {
                    if let (_, TimelineEntry::Msg(AgentMessage::Assistant(ref mut prev))) =
                        &mut timeline[tl_idx]
                    {
                        prev.content.push(tool_call_block.clone());
                        Some(tl_idx)
                    } else {
                        None
                    }
                })
                .is_some();

            if !merged_into_prev_standalone {
                let assistant = AssistantMessage::builder()
                    .content(vec![tool_call_block])
                    .api(effective_api_for_model(model))
                    .provider(model.provider.clone())
                    .model(model.id.clone())
                    .usage(Usage::default())
                    .stop_reason(StopReason::ToolUse)
                    .build()
                    .expect("history tool-call assistant message should always build");
                timeline.push((
                    SortKey::before_position(insert_pos, tc_idx * 2),
                    TimelineEntry::Msg(AgentMessage::Assistant(assistant)),
                ));
            }

            // Build the tool result message (truncated to avoid blowing up context).
            let result_text = tc
                .tool_output
                .as_ref()
                .map(|v| truncate_tool_result_text(&v.to_string(), HISTORY_TOOL_RESULT_MAX_CHARS))
                .unwrap_or_else(|| "[no output]".to_string());

            let tool_result = ToolResultMessage::text(&tc.id, &tc.tool_name, result_text, false);
            timeline.push((
                SortKey::before_position(insert_pos, tc_idx * 2 + 1),
                TimelineEntry::Msg(AgentMessage::ToolResult(tool_result)),
            ));
        }
    }

    // Phase 3: Sort by the sort key to produce a chronological sequence.
    timeline.sort_by(|a, b| a.0.cmp(&b.0));

    // Phase 4: Post-sort pass — attach pending thinking blocks to the next
    // assistant message.  This correctly associates reasoning with both plain
    // text replies and tool-call assistant messages that were interleaved by
    // Phase 2.
    let mut result: Vec<AgentMessage> = Vec::new();
    let mut pending_thinking: Vec<ContentBlock> = Vec::new();

    for (_, entry) in timeline {
        match entry {
            TimelineEntry::PendingThinking(block) => {
                pending_thinking.push(block);
            }
            TimelineEntry::Msg(AgentMessage::Assistant(mut assistant)) => {
                if !pending_thinking.is_empty() {
                    // Prepend accumulated thinking blocks before the existing content.
                    let mut merged = pending_thinking.drain(..).collect::<Vec<_>>();
                    merged.append(&mut assistant.content);
                    assistant.content = merged;
                }
                result.push(AgentMessage::Assistant(assistant));
            }
            TimelineEntry::Msg(msg @ AgentMessage::User(_)) => {
                // Discard any pending thinking that precedes a user message
                // (should not happen in normal flow, but be defensive).
                pending_thinking.clear();
                result.push(msg);
            }
            TimelineEntry::Msg(msg) => {
                result.push(msg);
            }
        }
    }
    // Any remaining pending_thinking blocks (orphan reasoning at the end with
    // no following assistant message) are silently dropped.

    result
}

/// Sort key for interleaving messages and tool calls chronologically.
///
/// Messages get a whole-number position (their index in the original list).
/// Tool calls are placed just before a specific position using fractional keys.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct SortKey {
    /// Main position (message index).
    position: usize,
    /// 0 = a tool call slot before this position, 1 = the actual message at this position.
    /// For tool call pairs: sub 0 = assistant+tool_call, sub 1 = tool_result.
    sub: u8,
    /// Tiebreaker for multiple tool calls before the same position.
    seq: usize,
}

impl SortKey {
    fn positional(pos: usize) -> Self {
        Self {
            position: pos,
            sub: 2, // after any tool calls inserted before this position
            seq: 0,
        }
    }

    fn before_position(pos: usize, seq: usize) -> Self {
        Self {
            position: pos,
            sub: 0,
            seq,
        }
    }
}

/// Truncate a tool result string to at most `max_chars` **characters**, appending a marker.
fn truncate_tool_result_text(text: &str, max_chars: usize) -> String {
    let total_chars = text.chars().count();
    if total_chars <= max_chars {
        return text.to_string();
    }
    // Collect the byte offset after the first `max_chars` characters so we
    // slice on a valid UTF-8 boundary (no panic on multi-byte chars).
    let byte_end = text
        .char_indices()
        .nth(max_chars)
        .map(|(idx, _)| idx)
        .unwrap_or(text.len());
    let truncated = &text[..byte_end];
    format!(
        "{}\n\n[Tool output truncated: {} chars → {} chars]",
        truncated, total_chars, max_chars
    )
}

fn history_user_message(message: &MessageRecord, model: &Model) -> UserMessage {
    let text = history_user_message_text(message);
    let attachments = history_message_attachments(message);

    if attachments.is_empty() {
        return UserMessage::text(text);
    }

    let mut blocks = Vec::new();
    let trimmed = text.trim();
    if !trimmed.is_empty() {
        blocks.push(ContentBlock::Text(TextContent::new(trimmed)));
    }

    blocks.extend(attachment_blocks(&attachments, model));

    if blocks.is_empty() {
        UserMessage::text(text)
    } else {
        UserMessage::blocks(blocks)
    }
}

fn history_user_message_text(message: &MessageRecord) -> String {
    message
        .metadata_json
        .as_deref()
        .and_then(command_effective_prompt_from_metadata)
        .unwrap_or_else(|| message.content_markdown.clone())
}

fn command_effective_prompt_from_metadata(raw: &str) -> Option<String> {
    let metadata = serde_json::from_str::<serde_json::Value>(raw).ok()?;
    let composer = metadata.get("composer")?;

    if composer.get("kind")?.as_str()? != "command" {
        return None;
    }

    composer
        .get("effectivePrompt")?
        .as_str()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
}

fn history_message_attachments(message: &MessageRecord) -> Vec<MessageAttachmentDto> {
    message
        .attachments_json
        .as_deref()
        .and_then(|raw| serde_json::from_str::<Vec<MessageAttachmentDto>>(raw).ok())
        .unwrap_or_default()
}

fn attachment_blocks(attachments: &[MessageAttachmentDto], model: &Model) -> Vec<ContentBlock> {
    attachments
        .iter()
        .filter_map(|attachment| attachment_block(attachment, model))
        .collect()
}

fn attachment_block(attachment: &MessageAttachmentDto, model: &Model) -> Option<ContentBlock> {
    if is_image_attachment(attachment) {
        return Some(image_attachment_block(attachment, model));
    }

    if is_text_attachment(attachment) {
        return Some(text_attachment_block(attachment));
    }

    None
}

fn image_attachment_block(attachment: &MessageAttachmentDto, model: &Model) -> ContentBlock {
    let label = attachment_label(attachment);

    if !model.supports_image() {
        return ContentBlock::Text(TextContent::new(format!("[Image attachment: {label}]")));
    }

    let Some(parsed) = attachment
        .url
        .as_deref()
        .and_then(parse_data_url)
        .filter(|parsed| parsed.is_base64)
    else {
        return ContentBlock::Text(TextContent::new(format!(
            "[Image attachment could not be loaded: {label}]"
        )));
    };

    let mime_type = attachment
        .media_type
        .clone()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or(parsed.mime_type);

    ContentBlock::Image(ImageContent::new(parsed.payload, mime_type))
}

fn text_attachment_block(attachment: &MessageAttachmentDto) -> ContentBlock {
    let label = attachment_label(attachment);

    match attachment
        .url
        .as_deref()
        .and_then(decode_data_url_text)
        .map(|text| render_text_attachment(&label, &text))
    {
        Some(content) => ContentBlock::Text(TextContent::new(content)),
        None => ContentBlock::Text(TextContent::new(format!(
            "[Text attachment could not be decoded: {label}]"
        ))),
    }
}

fn attachment_label(attachment: &MessageAttachmentDto) -> String {
    let trimmed = attachment.name.trim();
    if trimmed.is_empty() {
        "unnamed attachment".to_string()
    } else {
        trimmed.to_string()
    }
}

fn is_image_attachment(attachment: &MessageAttachmentDto) -> bool {
    attachment
        .media_type
        .as_deref()
        .map(|value| value.starts_with("image/"))
        .unwrap_or(false)
        || matches!(
            file_extension(&attachment.name).as_deref(),
            Some("png" | "jpg" | "jpeg" | "gif" | "webp" | "bmp" | "svg")
        )
}

fn is_text_attachment(attachment: &MessageAttachmentDto) -> bool {
    attachment
        .media_type
        .as_deref()
        .map(is_text_media_type)
        .unwrap_or(false)
        || matches!(
            file_extension(&attachment.name).as_deref(),
            Some(
                "txt"
                    | "md"
                    | "markdown"
                    | "json"
                    | "js"
                    | "jsx"
                    | "ts"
                    | "tsx"
                    | "rs"
                    | "py"
                    | "java"
                    | "go"
                    | "css"
                    | "scss"
                    | "less"
                    | "html"
                    | "xml"
                    | "yaml"
                    | "yml"
                    | "toml"
                    | "ini"
                    | "sh"
                    | "bash"
                    | "zsh"
                    | "sql"
                    | "c"
                    | "cc"
                    | "cpp"
                    | "h"
                    | "hpp"
                    | "swift"
                    | "kt"
                    | "rb"
                    | "php"
                    | "vue"
                    | "svelte"
                    | "astro"
            )
        )
}

fn is_text_media_type(media_type: &str) -> bool {
    media_type.starts_with("text/")
        || matches!(
            media_type,
            "application/json"
                | "application/xml"
                | "application/yaml"
                | "application/x-yaml"
                | "application/toml"
                | "application/javascript"
                | "application/x-javascript"
                | "application/typescript"
        )
}

fn file_extension(name: &str) -> Option<String> {
    let (_, ext) = name.rsplit_once('.')?;
    let trimmed = ext.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_ascii_lowercase())
    }
}

fn render_text_attachment(name: &str, content: &str) -> String {
    let trimmed = content.trim();
    let truncated = truncate_for_attachment(trimmed, TEXT_ATTACHMENT_MAX_CHARS);
    let language = fence_language(name);
    let suffix = if truncated.chars().count() < trimmed.chars().count() {
        "\n[attachment content truncated]"
    } else {
        ""
    };

    format!("[Text attachment: {name}]\n~~~{language}\n{truncated}\n~~~{suffix}")
}

fn fence_language(name: &str) -> &'static str {
    match file_extension(name).as_deref() {
        Some("md" | "markdown") => "markdown",
        Some("txt") => "text",
        Some("json") => "json",
        Some("js" | "jsx") => "javascript",
        Some("ts" | "tsx") => "typescript",
        Some("rs") => "rust",
        Some("py") => "python",
        Some("java") => "java",
        Some("go") => "go",
        Some("css" | "scss" | "less") => "css",
        Some("html") => "html",
        Some("xml") => "xml",
        Some("yaml" | "yml") => "yaml",
        Some("toml") => "toml",
        Some("sh" | "bash" | "zsh") => "bash",
        Some("sql") => "sql",
        Some("c" | "cc" | "cpp" | "h" | "hpp") => "cpp",
        Some("swift") => "swift",
        Some("kt") => "kotlin",
        Some("rb") => "ruby",
        Some("php") => "php",
        Some("vue") => "vue",
        Some("svelte") => "svelte",
        Some("astro") => "astro",
        _ => "text",
    }
}

fn truncate_for_attachment(value: &str, max_chars: usize) -> String {
    value.chars().take(max_chars).collect()
}

fn decode_data_url_text(url: &str) -> Option<String> {
    let parsed = parse_data_url(url)?;
    let bytes = if parsed.is_base64 {
        general_purpose::STANDARD
            .decode(parsed.payload.as_bytes())
            .ok()?
    } else {
        percent_decode(parsed.payload.as_bytes())?
    };

    Some(String::from_utf8_lossy(&bytes).into_owned())
}

struct ParsedDataUrl {
    mime_type: String,
    payload: String,
    is_base64: bool,
}

fn parse_data_url(url: &str) -> Option<ParsedDataUrl> {
    let payload = url.strip_prefix("data:")?;
    let (meta, data) = payload.split_once(',')?;
    let mut meta_parts = meta.split(';');
    let mime_type = meta_parts
        .next()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or("text/plain")
        .to_string();
    let is_base64 = meta_parts.any(|part| part.eq_ignore_ascii_case("base64"));

    Some(ParsedDataUrl {
        mime_type,
        payload: data.to_string(),
        is_base64,
    })
}

fn percent_decode(bytes: &[u8]) -> Option<Vec<u8>> {
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;

    while index < bytes.len() {
        if bytes[index] == b'%' {
            if index + 2 >= bytes.len() {
                return None;
            }
            let high = decode_hex_digit(bytes[index + 1])?;
            let low = decode_hex_digit(bytes[index + 2])?;
            decoded.push((high << 4) | low);
            index += 3;
            continue;
        }

        decoded.push(bytes[index]);
        index += 1;
    }

    Some(decoded)
}

fn decode_hex_digit(value: u8) -> Option<u8> {
    match value {
        b'0'..=b'9' => Some(value - b'0'),
        b'a'..=b'f' => Some(value - b'a' + 10),
        b'A'..=b'F' => Some(value - b'A' + 10),
        _ => None,
    }
}

fn is_context_reset_marker(message: &MessageRecord) -> bool {
    message.message_type == "summary_marker"
        && metadata_kind_matches(message.metadata_json.as_deref(), "context_reset")
}

fn is_context_summary_marker(message: &MessageRecord) -> bool {
    message.message_type == "summary_marker"
        && metadata_kind_matches(message.metadata_json.as_deref(), "context_summary")
}

fn metadata_kind_matches(raw: Option<&str>, expected: &str) -> bool {
    raw.and_then(|value| serde_json::from_str::<serde_json::Value>(value).ok())
        .and_then(|value| {
            value
                .get("kind")
                .and_then(serde_json::Value::as_str)
                .map(str::to_string)
        })
        .as_deref()
        == Some(expected)
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

#[allow(dead_code)]
fn assistant_message_from_text(content: &str, model: &Model) -> AssistantMessage {
    assistant_message_with_blocks(vec![ContentBlock::Text(TextContent::new(content))], model)
}

fn assistant_message_with_blocks(blocks: Vec<ContentBlock>, model: &Model) -> AssistantMessage {
    AssistantMessage::builder()
        .content(blocks)
        .api(effective_api_for_model(model))
        .provider(model.provider.clone())
        .model(model.id.clone())
        .usage(Usage::default())
        .stop_reason(StopReason::Stop)
        .build()
        .expect("assistant history message should always build")
}

fn effective_api_for_model(model: &Model) -> tiycore::types::Api {
    if let Some(api) = model.api.clone() {
        return api;
    }

    match &model.provider {
        Provider::OpenAI | Provider::OpenAIResponses | Provider::AzureOpenAIResponses => {
            tiycore::types::Api::OpenAIResponses
        }
        Provider::Anthropic | Provider::MiniMax | Provider::MiniMaxCN | Provider::KimiCoding => {
            tiycore::types::Api::AnthropicMessages
        }
        Provider::Google | Provider::GoogleGeminiCli | Provider::GoogleAntigravity => {
            tiycore::types::Api::GoogleGenerativeAi
        }
        Provider::GoogleVertex => tiycore::types::Api::GoogleVertex,
        Provider::Ollama => tiycore::types::Api::Ollama,
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
        | Provider::Zenmux => tiycore::types::Api::OpenAICompletions,
        Provider::AmazonBedrock => tiycore::types::Api::BedrockConverseStream,
        Provider::Custom(name) => tiycore::types::Api::Custom(name.clone()),
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

fn validate_clarify_input(value: &serde_json::Value) -> Result<(), String> {
    let Some(question) = value.get("question").and_then(serde_json::Value::as_str) else {
        return Err("clarify requires a non-empty question".to_string());
    };

    if question.trim().is_empty() {
        return Err("clarify requires a non-empty question".to_string());
    }

    let Some(options) = value.get("options").and_then(serde_json::Value::as_array) else {
        return Err("clarify requires 2 to 5 options".to_string());
    };

    if !(2..=5).contains(&options.len()) {
        return Err("clarify requires 2 to 5 options".to_string());
    }

    let recommended_count = options
        .iter()
        .filter(|option| {
            option
                .get("recommended")
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(false)
        })
        .count();

    if recommended_count > 1 {
        return Err("clarify may mark at most one option as recommended".to_string());
    }

    for option in options {
        let label = option
            .get("label")
            .and_then(serde_json::Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty());
        let description = option
            .get("description")
            .and_then(serde_json::Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty());

        if label.is_none() || description.is_none() {
            return Err("clarify options must include non-empty label and description".to_string());
        }
    }

    Ok(())
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

    let mut primary = resolve_runtime_model_role(pool, primary).await?;
    let auxiliary = match raw_plan.auxiliary.clone() {
        Some(role) => Some(resolve_runtime_model_role(pool, role).await?),
        None => None,
    };
    let lightweight = match raw_plan.lightweight.clone() {
        Some(role) => Some(resolve_runtime_model_role(pool, role).await?),
        None => None,
    };

    let thinking_level = raw_plan
        .thinking_level
        .as_deref()
        .map(ThinkingLevel::from)
        .unwrap_or(ThinkingLevel::Off);

    // When the user has selected a thinking level, ensure the primary model
    // has its `reasoning` flag enabled so that the protocol layer actually
    // includes the reasoning parameters in the API request.  Without this
    // the protocol guard (`if !model.reasoning { return None; }`) silently
    // drops the thinking configuration.
    if thinking_level != ThinkingLevel::Off && !primary.model.reasoning {
        primary.model.reasoning = true;
    }

    Ok(ResolvedRuntimeModelPlan {
        thinking_level,
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
        .reasoning(false)
        .context_window(context_window)
        .max_tokens(max_output_tokens)
        .input({
            let mut input = vec![InputType::Text];
            if role.supports_image_input.unwrap_or(false) {
                input.push(InputType::Image);
            }
            input
        })
        .cost(Cost::default());

    {
        let mut headers = crate::core::tiycode_default_headers();
        if let Some(user_headers) = role.custom_headers.clone() {
            headers.extend(user_headers);
        }
        builder = builder.headers(headers);
    }

    if let Some(compat) = default_openai_compatible_compat(&role.provider_type) {
        builder = builder.compat(compat);
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
    prompt::build_system_prompt(pool, raw_plan, workspace_path, run_mode).await
}

fn plan_mode_missing_checkpoint_error(
    run_mode: &str,
    checkpoint_requested: bool,
) -> Option<&'static str> {
    if run_mode == "plan" && !checkpoint_requested {
        Some(PLAN_MODE_MISSING_CHECKPOINT_ERROR)
    } else {
        None
    }
}

fn default_openai_compatible_compat(provider_type: &str) -> Option<OpenAICompletionsCompat> {
    if !provider_type.eq_ignore_ascii_case("openai-compatible") {
        return None;
    }

    let mut compat = OpenAICompletionsCompat::default();
    compat.supports_developer_role = false;
    Some(compat)
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
            "Response style: balanced. Default to a compact but complete answer. Lead with the answer or outcome first. Use a short paragraph or a short flat list when that makes the reply clearer. Add explanation when it materially helps understanding, but avoid over-explaining routine details. Each point should be a complete thought expressed in a full sentence, not a bare noun phrase or keyword fragment. When multiple points share a single theme, consolidate them into one paragraph rather than scattering them across separate bullets."
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

/// Auto-compression hook body, extracted from the `set_transform_context`
/// closure in [`configure_agent`] so the control flow is testable in isolation
/// and so the closure's capture list stays narrow.
///
/// Contract:
/// - Returns the original `messages` unchanged when compression is not needed.
/// - On success, emits a `ContextCompressing` frontend event, calls the LLM
///   to produce a summary (primary or merge), persists `{reset, summary}`
///   markers to the DB with a conservative boundary id, and returns
///   `[summary, …recent_messages]`.
/// - On LLM error / cancellation, injects a **heuristic** summary (via
///   [`generate_discard_summary`]) at the head, persists the heuristic
///   summary + reset marker, and returns `[heuristic_summary, …recent_messages]`.
///   This prevents the user from losing all earlier context when the LLM
///   call fails.
///
/// The function is `pub(crate)` purely so an integration test can drive it
/// directly rather than via the Agent runtime.
///
/// [`generate_discard_summary`]: crate::core::context_compression::generate_discard_summary
pub(crate) async fn run_auto_compression(
    messages: Vec<AgentMessage>,
    settings: crate::core::context_compression::CompressionSettings,
    model_role: ResolvedModelRole,
    weak: Weak<AgentSession>,
    thread_id: String,
    run_id: String,
    response_language: Option<String>,
) -> Vec<AgentMessage> {
    // Phase 1: check if compression is needed.
    //
    // The hot-path caller (the `set_transform_context` closure) already gates
    // on `should_compress` before cloning the heavy state, so in production
    // this branch should never hit. It stays here defensively so direct
    // callers (e.g. unit tests) still get correct behaviour for under-budget
    // inputs without having to duplicate the check.
    if !crate::core::context_compression::should_compress(&messages, &settings) {
        return messages;
    }

    tracing::info!(
        thread_id = %thread_id,
        message_count = messages.len(),
        "Auto context compression triggered"
    );

    // Phase 2: emit "compressing" event so the frontend shows placeholder
    if let Some(session) = weak.upgrade() {
        let _ = session
            .event_tx
            .send(ThreadStreamEvent::ContextCompressing { run_id });
    }

    // Pick up the session-level abort signal so that a Cancel click during
    // "Compressing context…" short-circuits the LLM call instead of waiting
    // the 90s PRIMARY_SUMMARY_TIMEOUT.
    let abort_signal = weak.upgrade().map(|session| session.abort_signal.clone());
    if abort_signal.as_ref().is_some_and(|s| s.is_cancelled()) {
        // Already cancelled before we even started — skip the LLM call.
        tracing::info!(
            thread_id = %thread_id,
            "Auto compression skipped: cancellation already requested"
        );
        let fallback =
            crate::core::context_compression::compress_context_fallback(messages, &settings);
        // Write back so subsequent turns start from the compressed base.
        if let Some(session) = weak.upgrade() {
            session.agent.replace_messages(fallback.clone());
        }
        return fallback;
    }

    // Phase 3: decide cut point
    let token_estimates: Vec<u32> = messages
        .iter()
        .map(crate::core::context_compression::estimate_message_tokens)
        .collect();
    let cut_point = crate::core::context_compression::find_cut_point(
        &messages,
        &token_estimates,
        settings.keep_recent_tokens,
    );

    let old_messages = &messages[..cut_point];
    let recent_messages = &messages[cut_point..];

    // Skip if nothing to compress. This happens when cut_point is driven all
    // the way to 0 by the tool-call/tool-result boundary adjustment. Falling
    // through to the fallback truncation is better than returning `messages`
    // unchanged (which would exceed the context window on the very next
    // provider call).
    if old_messages.is_empty() {
        tracing::warn!(
            thread_id = %thread_id,
            "Auto compression cut_point == 0 (tool-call boundary prevented compression); using truncation fallback"
        );
        let fallback =
            crate::core::context_compression::compress_context_fallback(messages, &settings);
        // Write back so subsequent turns start from the compressed base.
        if let Some(session) = weak.upgrade() {
            session.agent.replace_messages(fallback.clone());
        }
        return fallback;
    }

    // Phase 4: if the old region already begins with a prior <context_summary>
    // block, merge instead of re-summarise to avoid summary-of-summary quality
    // decay. The prior summary was injected by a previous compression pass and
    // lives at the head of `messages`; the delta to summarise is the rest of
    // `old_messages`.
    let response_language = response_language.as_deref();
    let summary_result = match crate::core::agent_run_manager::detect_prior_summary(old_messages) {
        Some((prior_summary, prefix_len)) if prefix_len < old_messages.len() => {
            let delta = &old_messages[prefix_len..];
            tracing::info!(
                thread_id = %thread_id,
                delta_len = delta.len(),
                "Merging prior <context_summary> with new delta"
            );
            crate::core::agent_run_manager::generate_merge_summary(
                &model_role,
                &prior_summary,
                delta,
                None,
                response_language,
                abort_signal.clone(),
            )
            .await
        }
        Some((prior_summary, _prefix_len)) => {
            // Prior summary with no new delta (unlikely, since
            // should_compress fired). Reuse the prior summary verbatim
            // instead of calling the model for nothing.
            tracing::info!(
                thread_id = %thread_id,
                "Reusing prior <context_summary> — no delta to merge"
            );
            Ok(prior_summary)
        }
        None => {
            crate::core::agent_run_manager::generate_primary_summary(
                &model_role,
                old_messages,
                None,
                response_language,
                abort_signal.clone(),
            )
            .await
        }
    };

    // Boundary buffer used by both the success path (persist markers) and the
    // fallback path (persist markers before truncation). Defined once here so
    // a future tuning change only lands in a single place.
    //
    // Rationale: one DB message may expand to multiple in-memory AgentMessages
    // (a plan/summary marker; a run's tool_calls split into assistant+tool_result
    // pairs), so exact matching isn't feasible. A small buffer lets the boundary
    // id slightly overshoot (include a few more old DB rows in the next reload
    // than strictly necessary) but NEVER undershoot — so no in-memory recent
    // message can get dropped by the next load.
    const BOUNDARY_BUFFER: usize = 16;

    match summary_result {
        Ok(summary) => {
            // Phase 5: persist markers to DB.
            if let Some(session) = weak.upgrade() {
                let boundary_id = resolve_boundary_id(
                    &session.pool,
                    &thread_id,
                    recent_messages.len(),
                    BOUNDARY_BUFFER,
                )
                .await;

                if let Err(e) = session
                    .persist_compression_markers(
                        &thread_id,
                        &summary,
                        "auto",
                        boundary_id.as_deref(),
                    )
                    .await
                {
                    tracing::warn!(
                        thread_id = %thread_id,
                        error = %e,
                        "Failed to persist auto-compression markers, continuing without DB record"
                    );
                }
            } else {
                tracing::warn!(
                    thread_id = %thread_id,
                    "Skipping auto-compression marker persistence: AgentSession dropped mid-compression. \
                    The next run will reload full history and re-trigger compression."
                );
            }

            // Phase 6: build compressed message list
            let result = crate::core::context_compression::build_compressed_messages(
                &summary,
                recent_messages,
            );

            // Phase 6.5: write back compressed messages to Agent internal state
            // so subsequent turns in the same run start from the compressed base
            // instead of re-compressing the full history every turn.
            if let Some(session) = weak.upgrade() {
                session.agent.replace_messages(result.clone());
            }

            tracing::info!(
                thread_id = %thread_id,
                discarded = cut_point,
                kept = result.len(),
                "Auto context compression completed"
            );

            result
        }
        Err(e) => {
            // LLM summary failed — fall back to pure truncation with a
            // **heuristic** summary injected at the head so the user never
            // fully loses the skeleton of earlier context. We also persist
            // that heuristic summary + reset marker to DB so the next run
            // starts from a clean boundary instead of re-loading the full
            // history and triggering compression again in a loop.
            tracing::warn!(
                thread_id = %thread_id,
                error = %e,
                "Auto context compression LLM summary failed, falling back to heuristic summary + truncation"
            );

            // On the fallback path the heuristic summary is much sparser than
            // an LLM-generated one, so the normal 16K recent window would be a
            // double reduction in available information. Recompute the cut
            // point with a larger keep window so users don't lose too much raw
            // recent context when the LLM call fails.
            let fallback_cut_point = crate::core::context_compression::find_cut_point(
                &messages,
                &token_estimates,
                crate::core::context_compression::FALLBACK_KEEP_RECENT_TOKENS,
            );
            // Never widen past the original cut point — the recent slice only
            // ever grows, never shrinks.
            let fallback_cut_point = fallback_cut_point.min(cut_point);
            let old_messages = &messages[..fallback_cut_point];
            let recent_messages = &messages[fallback_cut_point..];

            // Build the heuristic summary once so we can both persist it and
            // hand it to build_compressed_messages.
            // `compress_context_fallback` also generates one internally, but
            // we want the DB record and the in-memory context to agree on the
            // same text.
            let heuristic_summary =
                crate::core::context_compression::generate_discard_summary(old_messages);

            if let Some(session) = weak.upgrade() {
                let boundary_id = resolve_boundary_id(
                    &session.pool,
                    &thread_id,
                    recent_messages.len(),
                    BOUNDARY_BUFFER,
                )
                .await;

                if let Err(persist_err) = session
                    .persist_compression_markers(
                        &thread_id,
                        &heuristic_summary,
                        "auto_fallback",
                        boundary_id.as_deref(),
                    )
                    .await
                {
                    tracing::warn!(
                        thread_id = %thread_id,
                        error = %persist_err,
                        "Failed to persist fallback compression markers"
                    );
                }
            } else {
                tracing::warn!(
                    thread_id = %thread_id,
                    "Skipping fallback compression marker persistence: AgentSession dropped mid-compression. \
                    The next run will reload full history and re-trigger compression."
                );
            }

            let result = crate::core::context_compression::build_compressed_messages(
                &heuristic_summary,
                recent_messages,
            );

            // Write back fallback-compressed messages to Agent internal state
            // so subsequent turns start from the compressed base.
            if let Some(session) = weak.upgrade() {
                session.agent.replace_messages(result.clone());
            }

            tracing::info!(
                thread_id = %thread_id,
                discarded = fallback_cut_point,
                kept = result.len(),
                "Auto context compression fallback completed (heuristic summary)"
            );

            result
        }
    }
}

/// Resolve a conservative DB-backed boundary id for a compression pass.
///
/// Returns the id of the `(recent_len + buffer)`-th message from the end of
/// the thread, or `None` if the lookup fails or there are fewer rows than
/// that in the DB. `None` is always safe: it just means no `boundaryMessageId`
/// will be embedded in the reset marker and `list_since_last_reset` will fall
/// back to the reset row's own id as the lower bound.
///
/// Any error from the query is logged and converted to `None` — we never want
/// a transient DB failure to block compression; the worst-case is a small
/// extra reload on the next run.
async fn resolve_boundary_id(
    pool: &SqlitePool,
    thread_id: &str,
    recent_len: usize,
    buffer: usize,
) -> Option<String> {
    let n_from_end = recent_len.saturating_add(buffer);
    match crate::persistence::repo::message_repo::find_nth_from_end_id(pool, thread_id, n_from_end)
        .await
    {
        Ok(id) => id,
        Err(e) => {
            tracing::warn!(
                thread_id = %thread_id,
                error = %e,
                "Failed to resolve boundary message id; persisting reset marker without it"
            );
            None
        }
    }
}

/// Free-function form of [`AgentSession::persist_compression_markers`].
///
/// Extracted so unit tests can drive the marker-persistence contract against
/// an in-memory `SqlitePool` without having to stand up a full `AgentSession`
/// (which requires a `ToolGateway`, `HelperAgentOrchestrator`, a model plan,
/// etc.). See the module-level tests in `agent_session::persist_marker_tests`.
///
/// Contract (mirrors the method):
/// - Writes a `context_reset` marker first, then a `context_summary` marker.
///   UUID v7 is time-ordered, so `reset.id < summary.id`, ensuring
///   `list_since_last_reset (WHERE id >= reset_id)` includes the summary row.
/// - When `boundary_message_id` is `Some(non_empty)`, it is attached to the
///   reset marker's metadata as `boundaryMessageId`. `None` or `Some("")` are
///   treated identically — no key is added — so the caller doesn't have to
///   pre-validate.
pub(crate) async fn persist_compression_markers_to_pool(
    pool: &SqlitePool,
    thread_id: &str,
    summary: &str,
    source: &str,
    boundary_message_id: Option<&str>,
) -> Result<(), crate::model::errors::AppError> {
    let summary_metadata = serde_json::json!({
        "kind": "context_summary",
        "source": source,
        "label": "Compacted context summary",
    });
    let mut reset_metadata = serde_json::json!({
        "kind": "context_reset",
        "source": source,
        "label": "Context is now reset",
    });
    if let Some(boundary_id) = boundary_message_id {
        if !boundary_id.is_empty() {
            reset_metadata
                .as_object_mut()
                .expect("reset_metadata is an object literal")
                .insert(
                    "boundaryMessageId".to_string(),
                    serde_json::Value::String(boundary_id.to_string()),
                );
        }
    }

    let reset_id = uuid::Uuid::now_v7().to_string();
    let summary_id = uuid::Uuid::now_v7().to_string();
    let reset_metadata_json = reset_metadata.to_string();
    let summary_metadata_json = summary_metadata.to_string();

    // Wrap both inserts in a single transaction so a mid-way failure (crash,
    // constraint violation, disk error) cannot leave the thread in a state
    // where the reset marker exists without the summary. Without this, a
    // partial write would cause `list_since_last_reset` to load from the
    // boundary but with no accompanying summary — effectively showing the
    // user an uncompressed head with a reset marker dangling.
    //
    // This also reduces WAL lock round-trips from 2 → 1 on success.
    let mut tx = pool.begin().await?;

    const INSERT_SQL: &str = "INSERT INTO messages (id, thread_id, run_id, role, content_markdown,
                message_type, status, metadata_json, attachments_json, created_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))";

    sqlx::query(INSERT_SQL)
        .bind(&reset_id)
        .bind(thread_id)
        .bind(None::<String>)
        .bind("system")
        .bind("Context is now reset")
        .bind("summary_marker")
        .bind("completed")
        .bind(&reset_metadata_json)
        .bind(None::<String>)
        .execute(&mut *tx)
        .await?;

    sqlx::query(INSERT_SQL)
        .bind(&summary_id)
        .bind(thread_id)
        .bind(None::<String>)
        .bind("system")
        .bind(summary)
        .bind("summary_marker")
        .bind("completed")
        .bind(&summary_metadata_json)
        .bind(None::<String>)
        .execute(&mut *tx)
        .await?;

    tx.commit().await?;

    Ok(())
}

/// Security config for the **main** agent.  Uses a very large tool timeout so
/// that user-interactive tools (clarify, approval) are never killed by the
/// outer tiycore `tokio::select!` timeout.
fn main_agent_security_config() -> tiycore::types::SecurityConfig {
    let mut security = tiycore::types::SecurityConfig::default();
    security.agent.tool_execution_timeout_secs = MAIN_AGENT_TOOL_TIMEOUT_SECS;
    security.url = crate::core::tiycode_url_policy();
    security
}

/// Security config for **sub-agents** (helpers).  Keeps the tighter 600 s
/// timeout because sub-agents never surface user-interactive tools.
pub(crate) fn runtime_security_config() -> tiycore::types::SecurityConfig {
    let mut security = tiycore::types::SecurityConfig::default();
    security.agent.tool_execution_timeout_secs = SUBAGENT_TOOL_TIMEOUT_SECS;
    security.url = crate::core::tiycode_url_policy();
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
        build_initial_context_token_calibration, build_profile_response_prompt_parts,
        build_system_prompt, convert_history_messages, current_context_token_calibration,
        handle_agent_event, main_agent_security_config, normalize_profile_response_language,
        normalize_profile_response_style, plan_mode_missing_checkpoint_error,
        record_pending_prompt_estimate, resolve_helper_model_role, resolve_helper_profile,
        response_style_system_instruction, runtime_security_config, runtime_tools_for_profile,
        runtime_tools_for_profile_with_extensions, standard_tool_timeout,
        trim_history_to_current_context, ContextCompressionRuntimeState, ProfileResponseStyle,
        ResolvedModelRole, ResolvedRuntimeModelPlan, RuntimeModelPlan, DEFAULT_FULL_TOOL_PROFILE,
        MAIN_AGENT_TOOL_TIMEOUT_SECS, PLAN_MODE_MISSING_CHECKPOINT_ERROR,
        PLAN_READ_ONLY_TOOL_PROFILE, STANDARD_TOOL_TIMEOUT_SECS, SUBAGENT_TOOL_TIMEOUT_SECS,
    };
    use std::fs;
    use std::sync::Mutex as StdMutex;

    use tempfile::tempdir;
    use tiycore::agent::{AgentEvent, AgentMessage, AgentTool};
    use tiycore::thinking::ThinkingLevel;
    use tiycore::types::{
        Api, AssistantMessage, AssistantMessageEvent, ContentBlock, Provider, StopReason,
        TextContent,
    };
    use tokio::sync::mpsc;

    use crate::core::plan_checkpoint::{
        build_plan_artifact_from_tool_input, build_plan_message_metadata,
    };
    use crate::core::prompt::providers::{
        final_response_structure_system_instruction, run_mode_prompt_body,
    };
    use crate::core::subagent::{RuntimeOrchestrationTool, SubagentProfile};
    use crate::ipc::frontend_channels::ThreadStreamEvent;
    use crate::model::provider::AgentProfileRecord;
    use crate::model::thread::{MessageRecord, RunSummaryDto, RunUsageDto, ToolCallDto};
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
            thinking_level: None,
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

    fn sample_resolved_model_role_with_inputs(
        model_id: &str,
        input: Vec<tiycore::types::InputType>,
    ) -> ResolvedModelRole {
        let model = tiycore::types::Model::builder()
            .id(model_id)
            .name(model_id)
            .provider(Provider::OpenAI)
            .base_url("https://api.openai.com/v1")
            .context_window(128_000)
            .max_tokens(32_000)
            .input(input)
            .cost(tiycore::types::Cost::default())
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

    fn sample_resolved_model_role(model_id: &str) -> ResolvedModelRole {
        sample_resolved_model_role_with_inputs(model_id, vec![tiycore::types::InputType::Text])
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
            transport: tiycore::types::Transport::Sse,
        }
    }

    fn make_history_message(id: &str, run_id: &str, role: &str, content: &str) -> MessageRecord {
        MessageRecord {
            id: id.to_string(),
            thread_id: "thread-1".to_string(),
            run_id: Some(run_id.to_string()),
            role: role.to_string(),
            content_markdown: content.to_string(),
            message_type: "plain_message".to_string(),
            status: "completed".to_string(),
            metadata_json: None,
            attachments_json: None,
            created_at: "2026-01-01T00:00:00.000Z".to_string(),
        }
    }

    fn make_run_summary(model_id: &str, input_tokens: u64) -> RunSummaryDto {
        make_run_summary_with_cache(model_id, input_tokens, 0)
    }

    fn make_run_summary_with_cache(
        model_id: &str,
        input_tokens: u64,
        cache_read_tokens: u64,
    ) -> RunSummaryDto {
        RunSummaryDto {
            id: "run-prev".to_string(),
            thread_id: "thread-1".to_string(),
            run_mode: "default".to_string(),
            status: "completed".to_string(),
            model_id: Some(model_id.to_string()),
            model_display_name: Some(model_id.to_string()),
            context_window: Some(TEST_CONTEXT_WINDOW.to_string()),
            error_message: None,
            started_at: "2026-01-01T00:00:00.000Z".to_string(),
            usage: RunUsageDto {
                input_tokens,
                output_tokens: 128,
                cache_read_tokens,
                cache_write_tokens: 0,
                total_tokens: input_tokens + cache_read_tokens + 128,
            },
        }
    }

    fn message_text(message: &AgentMessage) -> String {
        match message {
            AgentMessage::User(user) => match &user.content {
                tiycore::types::UserContent::Text(text) => text.clone(),
                tiycore::types::UserContent::Blocks(blocks) => blocks
                    .iter()
                    .filter_map(|block| match block {
                        tiycore::types::ContentBlock::Text(text) => Some(text.text.as_str()),
                        _ => None,
                    })
                    .collect::<Vec<_>>()
                    .join("\n"),
            },
            AgentMessage::Assistant(assistant) => assistant.text_content(),
            _ => String::new(),
        }
    }

    fn user_blocks(message: &AgentMessage) -> &[ContentBlock] {
        match message {
            AgentMessage::User(user) => match &user.content {
                tiycore::types::UserContent::Blocks(blocks) => blocks,
                _ => panic!("expected block-based user message"),
            },
            _ => panic!("expected user message"),
        }
    }

    fn handle_test_agent_event(
        run_id: &str,
        event_tx: &mpsc::UnboundedSender<ThreadStreamEvent>,
        current_message_id: &StdMutex<Option<String>>,
        current_reasoning_message_id: &StdMutex<Option<String>>,
        last_usage: &StdMutex<Option<tiycore::types::Usage>>,
        reasoning_buffer: &StdMutex<String>,
        event: &AgentEvent,
    ) {
        let context_compression_state = StdMutex::new(ContextCompressionRuntimeState::default());
        handle_test_agent_event_with_context_state(
            run_id,
            event_tx,
            current_message_id,
            current_reasoning_message_id,
            last_usage,
            &context_compression_state,
            reasoning_buffer,
            event,
        );
    }

    fn handle_test_agent_event_with_context_state(
        run_id: &str,
        event_tx: &mpsc::UnboundedSender<ThreadStreamEvent>,
        current_message_id: &StdMutex<Option<String>>,
        current_reasoning_message_id: &StdMutex<Option<String>>,
        last_usage: &StdMutex<Option<tiycore::types::Usage>>,
        context_compression_state: &StdMutex<ContextCompressionRuntimeState>,
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
            context_compression_state,
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
    fn test_main_agent_security_config_uses_large_timeout() {
        let security = main_agent_security_config();

        assert_eq!(
            security.agent.tool_execution_timeout_secs,
            MAIN_AGENT_TOOL_TIMEOUT_SECS
        );
        // Main agent timeout must be much larger than subagent timeout
        // to avoid killing user-interactive tools like clarify/approval.
        assert!(MAIN_AGENT_TOOL_TIMEOUT_SECS > SUBAGENT_TOOL_TIMEOUT_SECS);
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
        let section = format!(
            "## Final Response Structure\n{}",
            final_response_structure_system_instruction()
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
        assert!(plan_prompt.contains("pauses for user approval"));
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
    fn plan_read_only_profile_includes_shell_excludes_mutating_terminal_tools() {
        let tools = runtime_tools_for_profile(PLAN_READ_ONLY_TOOL_PROFILE);
        let tool_names: Vec<&str> = tools.iter().map(|tool| tool.name.as_str()).collect();

        // Shell is available in plan mode (follows normal approval policy).
        assert!(tool_names.contains(&"shell"));
        // Write-oriented terminal tools are excluded.
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

    #[tokio::test]
    async fn system_prompt_includes_enabled_workspace_skills() {
        let temp_dir = tempdir().expect("temp dir");
        let workspace_root = temp_dir.path().join("workspace");
        let skill_dir = workspace_root.join(".tiy/skills/test-skill");
        fs::create_dir_all(&skill_dir).expect("skill dir");
        fs::write(
            skill_dir.join("SKILL.md"),
            r#"---
name: test-skill
description: "Helps with local skill prompt injection tests."
---

# Test Skill

Used for prompt assembly coverage.
"#,
        )
        .expect("write skill");

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

        assert!(prompt.contains("## Skills"));
        assert!(prompt.contains("### Available skills"));
        assert!(prompt.contains("test-skill: Helps with local skill prompt injection tests."));
        // The prompt path is built by joining workspace_path with ".tiy/skills" and then
        // reading child dirs, which may mix separators on Windows.  Match the production
        // path construction instead of canonicalizing.
        let expected_skill_path =
            std::path::Path::new(&workspace_root.to_string_lossy().into_owned())
                .join(".tiy/skills")
                .join("test-skill")
                .join("SKILL.md");
        assert!(
            prompt.contains(&expected_skill_path.display().to_string()),
            "prompt does not contain skill path.\nExpected: {}\nPrompt skills line: {}",
            expected_skill_path.display(),
            prompt
                .lines()
                .find(|l| l.contains("test-skill"))
                .unwrap_or("(not found)")
        );
        assert!(prompt.contains("### How to use skills"));
    }

    #[tokio::test]
    async fn system_prompt_includes_query_task_recovery_guidance() {
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

        assert!(prompt.contains("call `query_task` first"));
        assert!(prompt.contains("call `query_task` with `scope='active'`"));
        assert!(prompt.contains("Use `query_task` with `scope='all'` only"));
    }

    #[test]
    fn reasoning_blocks_reset_message_id_between_thought_segments() {
        let (event_tx, mut event_rx) = mpsc::unbounded_channel();
        let current_message_id = StdMutex::new(None::<String>);
        let current_reasoning_message_id = StdMutex::new(None::<String>);
        let last_usage = StdMutex::new(None::<tiycore::types::Usage>);
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
        let last_usage = StdMutex::new(None::<tiycore::types::Usage>);
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
        let last_usage = StdMutex::new(None::<tiycore::types::Usage>);
        let reasoning_buffer = StdMutex::new(String::new());
        let assistant = AssistantMessage::builder()
            .api(Api::OpenAICompletions)
            .provider(Provider::OpenAI)
            .model("gpt-test")
            .usage(tiycore::types::Usage::from_tokens(256, 32))
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
    fn message_end_usage_updates_consume_pending_prompt_estimate_once() {
        let (event_tx, mut event_rx) = mpsc::unbounded_channel();
        let current_message_id = StdMutex::new(None::<String>);
        let current_reasoning_message_id = StdMutex::new(None::<String>);
        let last_usage = StdMutex::new(None::<tiycore::types::Usage>);
        let context_compression_state = StdMutex::new(ContextCompressionRuntimeState::default());
        let reasoning_buffer = StdMutex::new(String::new());
        let assistant = AssistantMessage::builder()
            .api(Api::OpenAICompletions)
            .provider(Provider::OpenAI)
            .model("gpt-test")
            .usage(tiycore::types::Usage::from_tokens(1_500, 32))
            .build()
            .expect("assistant message with usage");

        record_pending_prompt_estimate(&context_compression_state, 1_000);
        handle_test_agent_event_with_context_state(
            "run-usage-calibration",
            &event_tx,
            &current_message_id,
            &current_reasoning_message_id,
            &last_usage,
            &context_compression_state,
            &reasoning_buffer,
            &AgentEvent::MessageEnd {
                message: AgentMessage::Assistant(assistant.clone()),
            },
        );
        handle_test_agent_event_with_context_state(
            "run-usage-calibration",
            &event_tx,
            &current_message_id,
            &current_reasoning_message_id,
            &last_usage,
            &context_compression_state,
            &reasoning_buffer,
            &AgentEvent::MessageEnd {
                message: AgentMessage::Assistant(assistant),
            },
        );

        let usage_events = std::iter::from_fn(|| event_rx.try_recv().ok())
            .filter(|event| matches!(event, ThreadStreamEvent::ThreadUsageUpdated { .. }))
            .count();
        let calibration = current_context_token_calibration(&context_compression_state);

        assert_eq!(usage_events, 1);
        assert_eq!(calibration.ratio_basis_points(), 15_000);
        assert!(context_compression_state
            .lock()
            .expect("context compression state")
            .pending_prompt_estimate
            .is_none());
    }

    #[test]
    fn usage_calibration_counts_cache_read_when_input_is_zero() {
        let (event_tx, mut event_rx) = mpsc::unbounded_channel();
        let current_message_id = StdMutex::new(None::<String>);
        let last_completed_message_id = StdMutex::new(None::<String>);
        let current_reasoning_message_id = StdMutex::new(None::<String>);
        let last_usage = StdMutex::new(None::<tiycore::types::Usage>);
        let context_compression_state = StdMutex::new(ContextCompressionRuntimeState::default());
        let reasoning_buffer = StdMutex::new(String::new());
        let assistant = AssistantMessage::builder()
            .api(Api::OpenAICompletions)
            .provider(Provider::OpenAI)
            .model("gpt-test")
            .usage(tiycore::types::Usage {
                input: 0,
                output: 32,
                cache_read: 1_500,
                cache_write: 0,
                total_tokens: 1_532,
                cost: tiycore::types::UsageCost::default(),
            })
            .build()
            .expect("assistant message with cache-read usage");

        record_pending_prompt_estimate(&context_compression_state, 1_000);
        handle_agent_event(
            "run-cache-read-calibration",
            &event_tx,
            &current_message_id,
            &last_completed_message_id,
            &current_reasoning_message_id,
            &last_usage,
            &context_compression_state,
            &reasoning_buffer,
            TEST_CONTEXT_WINDOW,
            TEST_MODEL_DISPLAY_NAME,
            &AgentEvent::MessageEnd {
                message: AgentMessage::Assistant(assistant),
            },
        );

        let usage_events = std::iter::from_fn(|| event_rx.try_recv().ok())
            .filter(|event| matches!(event, ThreadStreamEvent::ThreadUsageUpdated { .. }))
            .count();
        let calibration = current_context_token_calibration(&context_compression_state);

        assert_eq!(usage_events, 1);
        assert_eq!(calibration.ratio_basis_points(), 15_000);
        assert!(context_compression_state
            .lock()
            .expect("context compression state")
            .pending_prompt_estimate
            .is_none());
    }

    #[test]
    fn build_initial_context_token_calibration_seeds_from_matching_historical_run() {
        let primary_model = sample_resolved_model_role("primary-model");
        let history_messages = vec![
            make_history_message("msg-1", "run-prev", "user", &"x".repeat(600)),
            make_history_message("msg-2", "run-prev", "assistant", &"y".repeat(600)),
        ];
        let history = convert_history_messages(&history_messages, &[], &primary_model.model);
        let estimated_tokens = crate::core::context_compression::estimate_total_tokens(&history);
        let run_summary = make_run_summary("primary-model", (estimated_tokens as u64) * 2);

        let calibration = build_initial_context_token_calibration(
            Some(&run_summary),
            &history_messages,
            &[],
            &primary_model,
            "",
        );

        assert_eq!(calibration.ratio_basis_points(), 20_000);
        assert_eq!(
            calibration.apply_to_estimate(estimated_tokens),
            estimated_tokens * 2
        );
    }

    #[test]
    fn build_initial_context_token_calibration_counts_cache_read_tokens() {
        let primary_model = sample_resolved_model_role("primary-model");
        let history_messages = vec![
            make_history_message("msg-1", "run-prev", "user", &"x".repeat(600)),
            make_history_message("msg-2", "run-prev", "assistant", &"y".repeat(600)),
        ];
        let history = convert_history_messages(&history_messages, &[], &primary_model.model);
        let estimated_tokens = crate::core::context_compression::estimate_total_tokens(&history);
        let run_summary = make_run_summary_with_cache(
            "primary-model",
            estimated_tokens as u64 / 2,
            estimated_tokens as u64 * 3 / 2,
        );

        let calibration = build_initial_context_token_calibration(
            Some(&run_summary),
            &history_messages,
            &[],
            &primary_model,
            "",
        );

        assert_eq!(calibration.ratio_basis_points(), 20_000);
        assert_eq!(
            calibration.apply_to_estimate(estimated_tokens),
            estimated_tokens * 2
        );
    }

    #[test]
    fn build_initial_context_token_calibration_ignores_mismatched_models_and_zero_usage() {
        let primary_model = sample_resolved_model_role("primary-model");
        let history_messages = vec![make_history_message(
            "msg-1",
            "run-prev",
            "user",
            &"x".repeat(400),
        )];

        let mismatched = build_initial_context_token_calibration(
            Some(&make_run_summary("other-model", 4_096)),
            &history_messages,
            &[],
            &primary_model,
            "",
        );
        let zero_usage = build_initial_context_token_calibration(
            Some(&make_run_summary("primary-model", 0)),
            &history_messages,
            &[],
            &primary_model,
            "",
        );

        assert_eq!(mismatched.ratio_basis_points(), 10_000);
        assert_eq!(zero_usage.ratio_basis_points(), 10_000);
    }

    #[test]
    fn turn_retrying_event_emits_runtime_retry_notice() {
        let (event_tx, mut event_rx) = mpsc::unbounded_channel();
        let current_message_id = StdMutex::new(None::<String>);
        let last_completed_message_id = StdMutex::new(None::<String>);
        let current_reasoning_message_id = StdMutex::new(None::<String>);
        let last_usage = StdMutex::new(None::<tiycore::types::Usage>);
        let context_compression_state = StdMutex::new(ContextCompressionRuntimeState::default());
        let reasoning_buffer = StdMutex::new(String::new());

        handle_agent_event(
            "run-retry",
            &event_tx,
            &current_message_id,
            &last_completed_message_id,
            &current_reasoning_message_id,
            &last_usage,
            &context_compression_state,
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
    fn message_end_empty_content_no_tool_calls_skips_message_completed() {
        // When a provider error interrupts the stream before any text is
        // generated, MessageEnd arrives with empty text_content() and no
        // tool calls.  The handler must NOT emit MessageCompleted —
        // otherwise an empty plain_message record poisons the DB and
        // causes DeepSeek 400 errors on the next run.
        let (event_tx, mut event_rx) = mpsc::unbounded_channel();
        let current_message_id = StdMutex::new(None::<String>);
        let last_completed_message_id = StdMutex::new(None::<String>);
        let current_reasoning_message_id = StdMutex::new(None::<String>);
        let last_usage = StdMutex::new(None::<tiycore::types::Usage>);
        let context_compression_state = StdMutex::new(ContextCompressionRuntimeState::default());
        let reasoning_buffer = StdMutex::new(String::new());

        // Empty assistant: no content blocks, no tool calls.
        let empty_assistant = sample_partial_assistant_message();

        handle_agent_event(
            "run-empty",
            &event_tx,
            &current_message_id,
            &last_completed_message_id,
            &current_reasoning_message_id,
            &last_usage,
            &context_compression_state,
            &reasoning_buffer,
            TEST_CONTEXT_WINDOW,
            TEST_MODEL_DISPLAY_NAME,
            &AgentEvent::MessageEnd {
                message: AgentMessage::Assistant(empty_assistant),
            },
        );

        let events: Vec<_> = std::iter::from_fn(|| event_rx.try_recv().ok()).collect();
        // No MessageCompleted should have been emitted.
        let has_message_completed = events
            .iter()
            .any(|e| matches!(e, ThreadStreamEvent::MessageCompleted { .. }));
        assert!(
            !has_message_completed,
            "MessageCompleted should NOT be emitted for empty assistant without tool calls, got: {:?}",
            events
        );
    }

    #[test]
    fn message_discarded_reuses_last_completed_assistant_message_id() {
        let (event_tx, mut event_rx) = mpsc::unbounded_channel();
        let current_message_id = StdMutex::new(None::<String>);
        let last_completed_message_id = StdMutex::new(None::<String>);
        let current_reasoning_message_id = StdMutex::new(None::<String>);
        let last_usage = StdMutex::new(None::<tiycore::types::Usage>);
        let context_compression_state = StdMutex::new(ContextCompressionRuntimeState::default());
        let reasoning_buffer = StdMutex::new(String::new());
        // Build an assistant message with actual text content so that
        // MessageEnd emits MessageCompleted (empty content is now skipped).
        let assistant = AssistantMessage::builder()
            .content(vec![ContentBlock::Text(TextContent::new(
                "Here is the answer.",
            ))])
            .api(Api::OpenAICompletions)
            .provider(Provider::OpenAI)
            .model("gpt-test")
            .build()
            .expect("partial assistant message with content");

        handle_agent_event(
            "run-discard",
            &event_tx,
            &current_message_id,
            &last_completed_message_id,
            &current_reasoning_message_id,
            &last_usage,
            &context_compression_state,
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
            &context_compression_state,
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
    fn clarify_tool_is_available_in_both_runtime_profiles() {
        for profile in [DEFAULT_FULL_TOOL_PROFILE, PLAN_READ_ONLY_TOOL_PROFILE] {
            let tools = runtime_tools_for_profile(profile);
            let tool_names: Vec<&str> = tools.iter().map(|tool| tool.name.as_str()).collect();

            assert!(tool_names.contains(&"clarify"));
        }
    }

    #[test]
    fn query_task_tool_is_available_in_both_runtime_profiles() {
        for profile in [DEFAULT_FULL_TOOL_PROFILE, PLAN_READ_ONLY_TOOL_PROFILE] {
            let tools = runtime_tools_for_profile(profile);
            let tool_names: Vec<&str> = tools.iter().map(|tool| tool.name.as_str()).collect();

            assert!(tool_names.contains(&"query_task"));
        }
    }

    #[test]
    fn query_task_tool_schema_defaults_to_active_and_supports_all_scope() {
        let tools = runtime_tools_for_profile(DEFAULT_FULL_TOOL_PROFILE);
        let query_task = tools
            .iter()
            .find(|tool| tool.name == "query_task")
            .expect("query_task tool should exist");
        let scope = &query_task.parameters["properties"]["scope"];
        let scope_enum = scope["enum"]
            .as_array()
            .expect("query_task scope enum should be present");
        let description = scope["description"]
            .as_str()
            .expect("query_task scope description should be present");

        assert_eq!(scope_enum.len(), 2);
        assert_eq!(scope_enum[0], "active");
        assert_eq!(scope_enum[1], "all");
        assert!(description.contains("Defaults to `active`"));
    }

    #[test]
    fn runtime_tools_merge_extension_tools_without_overriding_builtin_names() {
        let tools = runtime_tools_for_profile_with_extensions(
            DEFAULT_FULL_TOOL_PROFILE,
            vec![
                AgentTool::new(
                    "__mcp_context7_resolve-library-id",
                    "resolve-library-id",
                    "Context7 MCP tool",
                    serde_json::json!({ "type": "object" }),
                ),
                AgentTool::new(
                    "read",
                    "Read",
                    "should not override builtin tool",
                    serde_json::json!({ "type": "object" }),
                ),
            ],
        );
        let tool_names: Vec<&str> = tools.iter().map(|tool| tool.name.as_str()).collect();

        assert!(tool_names.contains(&"__mcp_context7_resolve-library-id"));
        assert_eq!(tool_names.iter().filter(|name| **name == "read").count(), 1);
    }

    #[test]
    fn update_plan_tool_schema_no_longer_exposes_open_questions() {
        let tools = runtime_tools_for_profile(DEFAULT_FULL_TOOL_PROFILE);
        let update_plan = tools
            .iter()
            .find(|tool| tool.name == "update_plan")
            .expect("update_plan tool should exist");
        let properties = update_plan.parameters["properties"]
            .as_object()
            .expect("update_plan properties should be object");

        assert!(!properties.contains_key("openQuestions"));
    }

    #[test]
    fn update_plan_tool_schema_exposes_structured_plan_sections() {
        let tools = runtime_tools_for_profile(DEFAULT_FULL_TOOL_PROFILE);
        let update_plan = tools
            .iter()
            .find(|tool| tool.name == "update_plan")
            .expect("update_plan tool should exist");
        let properties = update_plan.parameters["properties"]
            .as_object()
            .expect("update_plan properties should be object");

        assert!(properties.contains_key("context"));
        assert!(properties.contains_key("design"));
        assert!(properties.contains_key("keyImplementation"));
        assert!(properties.contains_key("verification"));
        assert!(properties.contains_key("assumptions"));

        let nested_plan_properties = update_plan.parameters["properties"]["plan"]["properties"]
            .as_object()
            .expect("nested plan properties should be object");
        assert!(nested_plan_properties.contains_key("context"));
        assert!(nested_plan_properties.contains_key("design"));
        assert!(nested_plan_properties.contains_key("keyImplementation"));
        assert!(nested_plan_properties.contains_key("verification"));
        assert!(nested_plan_properties.contains_key("assumptions"));
    }

    #[test]
    fn update_plan_tool_description_contains_workflow_and_quality_contract() {
        let tools = runtime_tools_for_profile(DEFAULT_FULL_TOOL_PROFILE);
        let update_plan = tools
            .iter()
            .find(|tool| tool.name == "update_plan")
            .expect("update_plan tool should exist");

        // Workflow phases
        assert!(update_plan.description.contains("Phase 1"));
        assert!(update_plan.description.contains("Explore and understand"));
        assert!(update_plan.description.contains("Phase 2"));
        assert!(update_plan.description.contains("Clarify ambiguities"));
        assert!(update_plan.description.contains("Phase 3"));
        assert!(update_plan
            .description
            .contains("Converge on a recommendation"));
        assert!(update_plan.description.contains("Phase 4"));
        // Quality contract
        assert!(update_plan.description.contains("Quality contract"));
        assert!(update_plan.description.contains("keyImplementation"));
        assert!(update_plan.description.contains("verification"));
        assert!(update_plan.description.contains("Prohibited"));
        assert!(update_plan
            .description
            .contains("incrementally refine the plan"));
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

        assert!(prompt.contains("clarify"));
        assert!(prompt.contains("does NOT complete the run"));
        assert!(prompt.contains("must call update_plan"));
        assert!(prompt.contains("Unresolved core ambiguities pushed to the approval step"));
        assert!(prompt.contains("Once published, the run pauses for user approval"));
        assert!(prompt.contains("`design`: Write a detailed prose description"));
        assert!(prompt.contains("`verification`: Write a thorough description"));
        assert!(prompt.contains("pause"));
        // Verify phased workflow is present
        assert!(prompt.contains("Phase 1: Explore and understand"));
        assert!(prompt.contains("Phase 2: Clarify ambiguities"));
        assert!(prompt.contains("Phase 3: Converge on a recommendation"));
        assert!(prompt.contains("Phase 4: Publish the plan"));
        // Verify quality contract is present
        assert!(prompt.contains("Plan quality contract"));
    }

    #[test]
    fn default_mode_prompt_mentions_clarify_for_missing_information() {
        let prompt = run_mode_prompt_body("default");

        assert!(prompt.contains("Use clarify instead of guessing"));
        assert!(prompt.contains("multiple reasonable approaches"));
        assert!(prompt.contains("approve a risky action"));
    }

    #[test]
    fn default_mode_prompt_references_update_plan_quality_contract() {
        let prompt = run_mode_prompt_body("default");

        assert!(prompt.contains("follow the quality contract"));
        assert!(prompt.contains("update_plan tool description"));
        assert!(prompt.contains("Explore the codebase first"));
    }

    #[test]
    fn plan_mode_requires_checkpoint_before_successful_completion() {
        assert_eq!(
            plan_mode_missing_checkpoint_error("plan", false),
            Some(PLAN_MODE_MISSING_CHECKPOINT_ERROR)
        );
        assert_eq!(plan_mode_missing_checkpoint_error("plan", true), None);
        assert_eq!(plan_mode_missing_checkpoint_error("default", false), None);
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
                attachments_json: None,
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
                attachments_json: None,
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
                attachments_json: None,
                created_at: String::new(),
            },
        ];

        let history =
            convert_history_messages(&messages, &[], &sample_resolved_model_role("primary").model);

        assert_eq!(history.len(), 2);
        assert_eq!(message_text(&history[0]), "Please refine the plan.");
        assert!(message_text(&history[1])
            .contains("Implementation plan checkpoint (revision 2, approval state: pending):"));
        assert!(message_text(&history[1]).contains("# Plan title"));
        assert!(message_text(&history[1]).contains("Keep the plan in follow-up context."));
    }

    #[test]
    fn convert_history_messages_uses_effective_prompt_for_command_messages() {
        let messages = vec![MessageRecord {
            id: "msg-command".to_string(),
            thread_id: "thread-1".to_string(),
            run_id: None,
            role: "user".to_string(),
            content_markdown: "/init".to_string(),
            message_type: "plain_message".to_string(),
            status: "completed".to_string(),
            metadata_json: Some(
                serde_json::json!({
                    "composer": {
                        "kind": "command",
                        "displayText": "/init",
                        "effectivePrompt": "Generate or update a file named AGENTS.md."
                    }
                })
                .to_string(),
            ),
            attachments_json: None,
            created_at: String::new(),
        }];

        let history =
            convert_history_messages(&messages, &[], &sample_resolved_model_role("primary").model);

        assert_eq!(history.len(), 1);
        assert_eq!(
            message_text(&history[0]),
            "Generate or update a file named AGENTS.md."
        );
    }

    #[test]
    fn convert_history_messages_includes_image_and_text_attachments() {
        let messages = vec![MessageRecord {
            id: "msg-attachment".to_string(),
            thread_id: "thread-1".to_string(),
            run_id: None,
            role: "user".to_string(),
            content_markdown: "Please inspect these files.".to_string(),
            message_type: "plain_message".to_string(),
            status: "completed".to_string(),
            metadata_json: None,
            attachments_json: Some(
                serde_json::json!([
                    {
                        "id": "image-1",
                        "name": "diagram.png",
                        "mediaType": "image/png",
                        "url": "data:image/png;base64,aGVsbG8="
                    },
                    {
                        "id": "text-1",
                        "name": "notes.md",
                        "mediaType": "text/markdown",
                        "url": "data:text/markdown;base64,IyBIZWFkZXIKCkJvZHkgbGluZS4="
                    }
                ])
                .to_string(),
            ),
            created_at: String::new(),
        }];

        let history = convert_history_messages(
            &messages,
            &[],
            &sample_resolved_model_role_with_inputs(
                "vision-model",
                vec![
                    tiycore::types::InputType::Text,
                    tiycore::types::InputType::Image,
                ],
            )
            .model,
        );

        assert_eq!(history.len(), 1);
        let blocks = user_blocks(&history[0]);
        assert_eq!(blocks.len(), 3);

        match &blocks[0] {
            ContentBlock::Text(text) => assert_eq!(text.text, "Please inspect these files."),
            _ => panic!("expected prompt text block"),
        }

        match &blocks[1] {
            ContentBlock::Image(image) => {
                assert_eq!(image.mime_type, "image/png");
                assert_eq!(image.data, "aGVsbG8=");
            }
            _ => panic!("expected image block"),
        }

        match &blocks[2] {
            ContentBlock::Text(text) => {
                assert!(text.text.contains("[Text attachment: notes.md]"));
                assert!(text.text.contains("~~~markdown"));
                assert!(text.text.contains("# Header"));
                assert!(text.text.contains("Body line."));
            }
            _ => panic!("expected text attachment block"),
        }
    }

    #[test]
    fn convert_history_messages_falls_back_to_text_for_unsupported_image_models() {
        let messages = vec![MessageRecord {
            id: "msg-image".to_string(),
            thread_id: "thread-1".to_string(),
            run_id: None,
            role: "user".to_string(),
            content_markdown: "Describe this image.".to_string(),
            message_type: "plain_message".to_string(),
            status: "completed".to_string(),
            metadata_json: None,
            attachments_json: Some(
                serde_json::json!([
                    {
                        "id": "image-1",
                        "name": "photo.png",
                        "mediaType": "image/png",
                        "url": "data:image/png;base64,aGVsbG8="
                    }
                ])
                .to_string(),
            ),
            created_at: String::new(),
        }];

        let history =
            convert_history_messages(&messages, &[], &sample_resolved_model_role("primary").model);

        assert_eq!(history.len(), 1);
        let blocks = user_blocks(&history[0]);
        assert_eq!(blocks.len(), 2);

        match &blocks[1] {
            ContentBlock::Text(text) => {
                assert_eq!(text.text, "[Image attachment: photo.png]");
            }
            _ => panic!("expected text fallback block"),
        }
    }

    #[test]
    fn trim_history_to_current_context_keeps_only_messages_after_latest_reset() {
        let messages = vec![
            MessageRecord {
                id: "msg-before".to_string(),
                thread_id: "thread-1".to_string(),
                run_id: Some("run-before".to_string()),
                role: "assistant".to_string(),
                content_markdown: "Old assistant reply".to_string(),
                message_type: "plain_message".to_string(),
                status: "completed".to_string(),
                metadata_json: None,
                attachments_json: None,
                created_at: String::new(),
            },
            MessageRecord {
                id: "msg-reset".to_string(),
                thread_id: "thread-1".to_string(),
                run_id: None,
                role: "system".to_string(),
                content_markdown: "Context is now reset".to_string(),
                message_type: "summary_marker".to_string(),
                status: "completed".to_string(),
                metadata_json: Some(
                    serde_json::json!({
                        "kind": "context_reset",
                    })
                    .to_string(),
                ),
                attachments_json: None,
                created_at: String::new(),
            },
            MessageRecord {
                id: "msg-summary".to_string(),
                thread_id: "thread-1".to_string(),
                run_id: None,
                role: "system".to_string(),
                content_markdown: "<context_summary>\nCarry this forward.\n</context_summary>"
                    .to_string(),
                message_type: "summary_marker".to_string(),
                status: "completed".to_string(),
                metadata_json: Some(
                    serde_json::json!({
                        "kind": "context_summary",
                    })
                    .to_string(),
                ),
                attachments_json: None,
                created_at: String::new(),
            },
            MessageRecord {
                id: "msg-after".to_string(),
                thread_id: "thread-1".to_string(),
                run_id: None,
                role: "user".to_string(),
                content_markdown: "New request".to_string(),
                message_type: "plain_message".to_string(),
                status: "completed".to_string(),
                metadata_json: None,
                attachments_json: None,
                created_at: String::new(),
            },
        ];

        let trimmed = trim_history_to_current_context(&messages);

        assert_eq!(trimmed.len(), 2);
        assert_eq!(trimmed[0].id, "msg-summary");
        assert_eq!(trimmed[1].id, "msg-after");
    }

    #[test]
    fn convert_history_messages_keeps_context_summary_but_skips_reset_markers() {
        let messages = vec![
            MessageRecord {
                id: "msg-reset".to_string(),
                thread_id: "thread-1".to_string(),
                run_id: None,
                role: "system".to_string(),
                content_markdown: "Context is now reset".to_string(),
                message_type: "summary_marker".to_string(),
                status: "completed".to_string(),
                metadata_json: Some(
                    serde_json::json!({
                        "kind": "context_reset",
                    })
                    .to_string(),
                ),
                attachments_json: None,
                created_at: String::new(),
            },
            MessageRecord {
                id: "msg-summary".to_string(),
                thread_id: "thread-1".to_string(),
                run_id: None,
                role: "system".to_string(),
                content_markdown: "<context_summary>\nCarry this forward.\n</context_summary>"
                    .to_string(),
                message_type: "summary_marker".to_string(),
                status: "completed".to_string(),
                metadata_json: Some(
                    serde_json::json!({
                        "kind": "context_summary",
                    })
                    .to_string(),
                ),
                attachments_json: None,
                created_at: String::new(),
            },
        ];

        let history =
            convert_history_messages(&messages, &[], &sample_resolved_model_role("primary").model);

        assert_eq!(history.len(), 1);
        assert_eq!(
            message_text(&history[0]),
            "<context_summary>\nCarry this forward.\n</context_summary>"
        );
    }

    #[test]
    fn convert_history_messages_merges_reasoning_into_assistant() {
        let messages = vec![
            MessageRecord {
                id: "msg-user".to_string(),
                thread_id: "thread-1".to_string(),
                run_id: None,
                role: "user".to_string(),
                content_markdown: "Hello".to_string(),
                message_type: "plain_message".to_string(),
                status: "completed".to_string(),
                metadata_json: None,
                attachments_json: None,
                created_at: "2026-01-01T00:00:00.000Z".to_string(),
            },
            MessageRecord {
                id: "msg-reasoning".to_string(),
                thread_id: "thread-1".to_string(),
                run_id: Some("run-1".to_string()),
                role: "assistant".to_string(),
                content_markdown: "Let me think about this.".to_string(),
                message_type: "reasoning".to_string(),
                status: "completed".to_string(),
                metadata_json: Some(
                    serde_json::json!({"thinking_signature": "reasoning_content"}).to_string(),
                ),
                attachments_json: None,
                created_at: "2026-01-01T00:00:01.000Z".to_string(),
            },
            MessageRecord {
                id: "msg-assistant".to_string(),
                thread_id: "thread-1".to_string(),
                run_id: Some("run-1".to_string()),
                role: "assistant".to_string(),
                content_markdown: "Here is the answer.".to_string(),
                message_type: "plain_message".to_string(),
                status: "completed".to_string(),
                metadata_json: None,
                attachments_json: None,
                created_at: "2026-01-01T00:00:02.000Z".to_string(),
            },
        ];

        let model = &sample_resolved_model_role("primary").model;
        let history = convert_history_messages(&messages, &[], model);

        // Should produce: User + Assistant (with Thinking + Text blocks)
        assert_eq!(history.len(), 2);
        match &history[0] {
            AgentMessage::User(_) => {}
            other => panic!("expected User, got {:?}", other),
        }
        match &history[1] {
            AgentMessage::Assistant(assistant) => {
                assert_eq!(
                    assistant.content.len(),
                    2,
                    "assistant should have Thinking + Text blocks"
                );
                assert!(assistant.content[0].is_thinking());
                assert!(assistant.content[1].is_text());
                let thinking = assistant.content[0].as_thinking().unwrap();
                assert_eq!(thinking.thinking, "Let me think about this.");
                assert_eq!(
                    thinking.thinking_signature.as_deref(),
                    Some("reasoning_content")
                );
                let text = assistant.content[1].as_text().unwrap();
                assert_eq!(text.text, "Here is the answer.");
            }
            other => panic!("expected Assistant, got {:?}", other),
        }
    }

    #[test]
    fn convert_history_messages_reasoning_without_signature_still_merges() {
        let messages = vec![
            MessageRecord {
                id: "msg-reasoning".to_string(),
                thread_id: "thread-1".to_string(),
                run_id: Some("run-1".to_string()),
                role: "assistant".to_string(),
                content_markdown: "Thinking...".to_string(),
                message_type: "reasoning".to_string(),
                status: "completed".to_string(),
                metadata_json: None, // no signature (old data)
                attachments_json: None,
                created_at: "2026-01-01T00:00:01.000Z".to_string(),
            },
            MessageRecord {
                id: "msg-assistant".to_string(),
                thread_id: "thread-1".to_string(),
                run_id: Some("run-1".to_string()),
                role: "assistant".to_string(),
                content_markdown: "Result.".to_string(),
                message_type: "plain_message".to_string(),
                status: "completed".to_string(),
                metadata_json: None,
                attachments_json: None,
                created_at: "2026-01-01T00:00:02.000Z".to_string(),
            },
        ];

        let model = &sample_resolved_model_role("primary").model;
        let history = convert_history_messages(&messages, &[], model);

        assert_eq!(history.len(), 1);
        match &history[0] {
            AgentMessage::Assistant(assistant) => {
                assert_eq!(assistant.content.len(), 2);
                let thinking = assistant.content[0].as_thinking().unwrap();
                assert_eq!(thinking.thinking, "Thinking...");
                assert!(thinking.thinking_signature.is_none());
            }
            other => panic!("expected Assistant, got {:?}", other),
        }
    }

    #[test]
    fn convert_history_messages_orphan_reasoning_at_end_is_dropped() {
        // A reasoning message at the end with no following assistant text
        // (e.g. interrupted run) should be silently dropped.
        let messages = vec![
            MessageRecord {
                id: "msg-user".to_string(),
                thread_id: "thread-1".to_string(),
                run_id: None,
                role: "user".to_string(),
                content_markdown: "Hello".to_string(),
                message_type: "plain_message".to_string(),
                status: "completed".to_string(),
                metadata_json: None,
                attachments_json: None,
                created_at: "2026-01-01T00:00:00.000Z".to_string(),
            },
            MessageRecord {
                id: "msg-reasoning".to_string(),
                thread_id: "thread-1".to_string(),
                run_id: Some("run-1".to_string()),
                role: "assistant".to_string(),
                content_markdown: "Orphan reasoning".to_string(),
                message_type: "reasoning".to_string(),
                status: "completed".to_string(),
                metadata_json: None,
                attachments_json: None,
                created_at: "2026-01-01T00:00:01.000Z".to_string(),
            },
        ];

        let model = &sample_resolved_model_role("primary").model;
        let history = convert_history_messages(&messages, &[], model);

        // Only the user message should appear; the orphan reasoning is dropped.
        assert_eq!(history.len(), 1);
        assert!(matches!(&history[0], AgentMessage::User(_)));
    }

    #[test]
    fn convert_history_messages_attaches_reasoning_to_tool_call() {
        // Scenario: user → reasoning → tool_call (no intermediate text).
        // No text message to merge into, so tool call gets its own assistant
        // message and Phase 4 attaches the pending thinking block.
        let messages = vec![
            MessageRecord {
                id: "msg-user".to_string(),
                thread_id: "thread-1".to_string(),
                run_id: None,
                role: "user".to_string(),
                content_markdown: "Run a command".to_string(),
                message_type: "plain_message".to_string(),
                status: "completed".to_string(),
                metadata_json: None,
                attachments_json: None,
                created_at: "2026-01-01T00:00:00.000Z".to_string(),
            },
            MessageRecord {
                id: "msg-reasoning".to_string(),
                thread_id: "thread-1".to_string(),
                run_id: Some("run-1".to_string()),
                role: "assistant".to_string(),
                content_markdown: "Let me think about the command.".to_string(),
                message_type: "reasoning".to_string(),
                status: "completed".to_string(),
                metadata_json: Some(
                    serde_json::json!({"thinking_signature": "reasoning_content"}).to_string(),
                ),
                attachments_json: None,
                created_at: "2026-01-01T00:00:01.000Z".to_string(),
            },
            // A plain_message from a LATER response (after tool result).
            MessageRecord {
                id: "msg-assistant".to_string(),
                thread_id: "thread-1".to_string(),
                run_id: Some("run-1".to_string()),
                role: "assistant".to_string(),
                content_markdown: "Done.".to_string(),
                message_type: "plain_message".to_string(),
                status: "completed".to_string(),
                metadata_json: None,
                attachments_json: None,
                created_at: "2026-01-01T00:00:05.000Z".to_string(),
            },
        ];

        let tool_calls = vec![ToolCallDto {
            id: "tc-1".to_string(),
            storage_id: "st-1".to_string(),
            run_id: "run-1".to_string(),
            thread_id: "thread-1".to_string(),
            helper_id: None,
            tool_name: "shell".to_string(),
            tool_input: serde_json::json!({"command": "ls"}),
            tool_output: Some(serde_json::json!("file.txt")),
            status: "completed".to_string(),
            approval_status: None,
            started_at: "2026-01-01T00:00:02.000Z".to_string(),
            finished_at: Some("2026-01-01T00:00:03.000Z".to_string()),
        }];

        let model = &sample_resolved_model_role("primary").model;
        let history = convert_history_messages(&messages, &tool_calls, model);

        // No preceding text in same run before TC1 → standalone assistant.
        // Expected: User → Assistant[Thinking, ToolCall] → ToolResult → Assistant[Text]
        assert_eq!(
            history.len(),
            4,
            "should have User + TC-Assistant + ToolResult + Text-Assistant"
        );

        // 1. User message
        assert!(matches!(&history[0], AgentMessage::User(_)));

        // 2. Tool-call assistant with reasoning prepended
        match &history[1] {
            AgentMessage::Assistant(assistant) => {
                assert_eq!(
                    assistant.content.len(),
                    2,
                    "tool-call assistant should have Thinking + ToolCall blocks, got: {:?}",
                    assistant.content
                );
                assert!(
                    assistant.content[0].is_thinking(),
                    "first block should be Thinking"
                );
                let thinking = assistant.content[0].as_thinking().unwrap();
                assert_eq!(thinking.thinking, "Let me think about the command.");
                assert_eq!(
                    thinking.thinking_signature.as_deref(),
                    Some("reasoning_content")
                );
                assert!(
                    assistant.content[1].is_tool_call(),
                    "second block should be ToolCall"
                );
            }
            other => panic!("expected Assistant for tool call, got {:?}", other),
        }

        // 3. Tool result
        assert!(matches!(&history[2], AgentMessage::ToolResult(_)));

        // 4. Final text assistant (no thinking blocks — they were consumed by the tool call)
        match &history[3] {
            AgentMessage::Assistant(assistant) => {
                assert_eq!(
                    assistant.content.len(),
                    1,
                    "text assistant should have only Text"
                );
                assert!(assistant.content[0].is_text());
            }
            other => panic!("expected text Assistant, got {:?}", other),
        }
    }

    #[test]
    fn convert_history_messages_merges_multiple_standalone_tool_calls_at_same_position() {
        // Scenario: one DeepSeek response emits reasoning + two tool calls and no text.
        // Both tool calls insert before the same later assistant text message, so they
        // must be reconstructed as a single assistant message sharing reasoning_content.
        let messages = vec![
            MessageRecord {
                id: "msg-user".to_string(),
                thread_id: "thread-1".to_string(),
                run_id: None,
                role: "user".to_string(),
                content_markdown: "Run two commands".to_string(),
                message_type: "plain_message".to_string(),
                status: "completed".to_string(),
                metadata_json: None,
                attachments_json: None,
                created_at: "2026-01-01T00:00:00.000Z".to_string(),
            },
            MessageRecord {
                id: "msg-reasoning".to_string(),
                thread_id: "thread-1".to_string(),
                run_id: Some("run-1".to_string()),
                role: "assistant".to_string(),
                content_markdown: "I need to run two independent commands.".to_string(),
                message_type: "reasoning".to_string(),
                status: "completed".to_string(),
                metadata_json: Some(
                    serde_json::json!({"thinking_signature": "reasoning_content"}).to_string(),
                ),
                attachments_json: None,
                created_at: "2026-01-01T00:00:01.000Z".to_string(),
            },
            MessageRecord {
                id: "msg-assistant".to_string(),
                thread_id: "thread-1".to_string(),
                run_id: Some("run-1".to_string()),
                role: "assistant".to_string(),
                content_markdown: "Both commands finished.".to_string(),
                message_type: "plain_message".to_string(),
                status: "completed".to_string(),
                metadata_json: None,
                attachments_json: None,
                created_at: "2026-01-01T00:00:10.000Z".to_string(),
            },
        ];

        let tool_calls = vec![
            ToolCallDto {
                id: "tc-1".to_string(),
                storage_id: "st-1".to_string(),
                run_id: "run-1".to_string(),
                thread_id: "thread-1".to_string(),
                helper_id: None,
                tool_name: "shell".to_string(),
                tool_input: serde_json::json!({"command": "pwd"}),
                tool_output: Some(serde_json::json!("/tmp/project")),
                status: "completed".to_string(),
                approval_status: None,
                started_at: "2026-01-01T00:00:02.000Z".to_string(),
                finished_at: Some("2026-01-01T00:00:03.000Z".to_string()),
            },
            ToolCallDto {
                id: "tc-2".to_string(),
                storage_id: "st-2".to_string(),
                run_id: "run-1".to_string(),
                thread_id: "thread-1".to_string(),
                helper_id: None,
                tool_name: "shell".to_string(),
                tool_input: serde_json::json!({"command": "ls"}),
                tool_output: Some(serde_json::json!("file.txt")),
                status: "completed".to_string(),
                approval_status: None,
                started_at: "2026-01-01T00:00:04.000Z".to_string(),
                finished_at: Some("2026-01-01T00:00:05.000Z".to_string()),
            },
        ];

        let model = &sample_resolved_model_role("primary").model;
        let history = convert_history_messages(&messages, &tool_calls, model);

        // Expected: User → Assistant[Thinking, ToolCall1, ToolCall2]
        // → ToolResult1 → ToolResult2 → Assistant[Text]
        assert_eq!(history.len(), 5, "unexpected history: {history:?}");
        assert!(matches!(&history[0], AgentMessage::User(_)));

        match &history[1] {
            AgentMessage::Assistant(assistant) => {
                assert_eq!(
                    assistant.content.len(),
                    3,
                    "assistant should contain Thinking + both ToolCalls"
                );
                let thinking = assistant.content[0].as_thinking().unwrap();
                assert_eq!(thinking.thinking, "I need to run two independent commands.");
                assert_eq!(
                    thinking.thinking_signature.as_deref(),
                    Some("reasoning_content")
                );
                assert!(assistant.content[1].is_tool_call());
                assert!(assistant.content[2].is_tool_call());
                assert_eq!(assistant.stop_reason, StopReason::ToolUse);
            }
            other => panic!("expected merged standalone Assistant, got {:?}", other),
        }

        match &history[2] {
            AgentMessage::ToolResult(result) => assert_eq!(result.tool_call_id, "tc-1"),
            other => panic!("expected first ToolResult, got {:?}", other),
        }
        match &history[3] {
            AgentMessage::ToolResult(result) => assert_eq!(result.tool_call_id, "tc-2"),
            other => panic!("expected second ToolResult, got {:?}", other),
        }

        match &history[4] {
            AgentMessage::Assistant(assistant) => {
                assert_eq!(assistant.content.len(), 1);
                assert!(assistant.content[0].is_text());
                assert_eq!(
                    assistant.content[0].as_text().unwrap().text,
                    "Both commands finished."
                );
            }
            other => panic!("expected final text Assistant, got {:?}", other),
        }
    }

    #[test]
    fn convert_history_messages_merges_tool_call_into_preceding_text() {
        // Scenario mimicking real DeepSeek flow: a single API response produces
        // reasoning + text + tool_call.  The text is saved first, then the tool
        // is executed (started_at > text.created_at).  The tool call must be
        // merged into the text assistant message so they share the same
        // reasoning_content.
        let messages = vec![
            MessageRecord {
                id: "msg-user".to_string(),
                thread_id: "t1".to_string(),
                run_id: None,
                role: "user".to_string(),
                content_markdown: "Do something".to_string(),
                message_type: "plain_message".to_string(),
                status: "completed".to_string(),
                metadata_json: None,
                attachments_json: None,
                created_at: "2026-01-01T00:00:00.000Z".to_string(),
            },
            // Reasoning from the first response
            MessageRecord {
                id: "msg-r1".to_string(),
                thread_id: "t1".to_string(),
                run_id: Some("run-1".to_string()),
                role: "assistant".to_string(),
                content_markdown: "Let me run a command.".to_string(),
                message_type: "reasoning".to_string(),
                status: "completed".to_string(),
                metadata_json: Some(
                    serde_json::json!({"thinking_signature": "reasoning_content"}).to_string(),
                ),
                attachments_json: None,
                created_at: "2026-01-01T00:00:01.000Z".to_string(),
            },
            // Text from the first response (saved during streaming)
            MessageRecord {
                id: "msg-text1".to_string(),
                thread_id: "t1".to_string(),
                run_id: Some("run-1".to_string()),
                role: "assistant".to_string(),
                content_markdown: "Running the command now.".to_string(),
                message_type: "plain_message".to_string(),
                status: "completed".to_string(),
                metadata_json: None,
                attachments_json: None,
                created_at: "2026-01-01T00:00:02.000Z".to_string(),
            },
            // Reasoning from the SECOND response (after tool result)
            MessageRecord {
                id: "msg-r2".to_string(),
                thread_id: "t1".to_string(),
                run_id: Some("run-1".to_string()),
                role: "assistant".to_string(),
                content_markdown: "The command succeeded.".to_string(),
                message_type: "reasoning".to_string(),
                status: "completed".to_string(),
                metadata_json: Some(
                    serde_json::json!({"thinking_signature": "reasoning_content"}).to_string(),
                ),
                attachments_json: None,
                created_at: "2026-01-01T00:00:06.000Z".to_string(),
            },
            // Text from the second response
            MessageRecord {
                id: "msg-text2".to_string(),
                thread_id: "t1".to_string(),
                run_id: Some("run-1".to_string()),
                role: "assistant".to_string(),
                content_markdown: "All done!".to_string(),
                message_type: "plain_message".to_string(),
                status: "completed".to_string(),
                metadata_json: None,
                attachments_json: None,
                created_at: "2026-01-01T00:00:07.000Z".to_string(),
            },
        ];

        // TC1 from the first response — started AFTER the text was saved.
        let tool_calls = vec![ToolCallDto {
            id: "tc-1".to_string(),
            storage_id: "st-1".to_string(),
            run_id: "run-1".to_string(),
            thread_id: "t1".to_string(),
            helper_id: None,
            tool_name: "shell".to_string(),
            tool_input: serde_json::json!({"command": "ls"}),
            tool_output: Some(serde_json::json!("file.txt")),
            status: "completed".to_string(),
            approval_status: None,
            // started_at > text1.created_at but < r2.created_at
            started_at: "2026-01-01T00:00:03.000Z".to_string(),
            finished_at: Some("2026-01-01T00:00:04.000Z".to_string()),
        }];

        let model = &sample_resolved_model_role("primary").model;
        let history = convert_history_messages(&messages, &tool_calls, model);

        // TC1 should merge into text1 (same run, no reasoning between them).
        // Expected: User → Assistant[Thinking(R1), Text, ToolCall] → ToolResult → Assistant[Thinking(R2), Text]
        assert_eq!(
            history.len(),
            4,
            "should have User + merged-Assistant + ToolResult + final-Assistant, got {:?}",
            history
                .iter()
                .map(|m| match m {
                    AgentMessage::User(_) => "User",
                    AgentMessage::Assistant(_) => "Assistant",
                    AgentMessage::ToolResult(_) => "ToolResult",
                    _ => "Other",
                })
                .collect::<Vec<_>>()
        );

        // 1. User
        assert!(matches!(&history[0], AgentMessage::User(_)));

        // 2. Merged assistant with Thinking + Text + ToolCall
        match &history[1] {
            AgentMessage::Assistant(a) => {
                assert_eq!(
                    a.content.len(),
                    3,
                    "merged assistant should have Thinking + Text + ToolCall, got: {:?}",
                    a.content
                );
                assert!(a.content[0].is_thinking(), "block 0 should be Thinking");
                assert_eq!(
                    a.content[0].as_thinking().unwrap().thinking,
                    "Let me run a command."
                );
                assert!(a.content[1].is_text(), "block 1 should be Text");
                assert_eq!(
                    a.content[1].as_text().unwrap().text,
                    "Running the command now."
                );
                assert!(a.content[2].is_tool_call(), "block 2 should be ToolCall");
                assert_eq!(a.stop_reason, StopReason::ToolUse);
            }
            other => panic!("expected merged Assistant, got {:?}", other),
        }

        // 3. Tool result
        assert!(matches!(&history[2], AgentMessage::ToolResult(_)));

        // 4. Final text with R2 reasoning
        match &history[3] {
            AgentMessage::Assistant(a) => {
                assert_eq!(a.content.len(), 2, "final: Thinking(R2) + Text");
                assert!(a.content[0].is_thinking());
                assert_eq!(
                    a.content[0].as_thinking().unwrap().thinking,
                    "The command succeeded."
                );
                assert!(a.content[1].is_text());
                assert_eq!(a.content[1].as_text().unwrap().text, "All done!");
            }
            other => panic!("expected final Assistant, got {:?}", other),
        }
    }

    #[test]
    fn convert_history_messages_multiple_reasoning_tool_call_cycles() {
        // Scenario: user → R1 → text1 → TC1 → R2 → TC2 → R3 → text2
        // R1 should attach to text1, R2 to TC2 (not TC1), R3 to text2.
        let messages = vec![
            MessageRecord {
                id: "msg-01-user".to_string(),
                thread_id: "t1".to_string(),
                run_id: None,
                role: "user".to_string(),
                content_markdown: "Go".to_string(),
                message_type: "plain_message".to_string(),
                status: "completed".to_string(),
                metadata_json: None,
                attachments_json: None,
                created_at: "2026-01-01T00:00:00.000Z".to_string(),
            },
            // R1
            MessageRecord {
                id: "msg-02-r1".to_string(),
                thread_id: "t1".to_string(),
                run_id: Some("run-1".to_string()),
                role: "assistant".to_string(),
                content_markdown: "R1 thinking".to_string(),
                message_type: "reasoning".to_string(),
                status: "completed".to_string(),
                metadata_json: Some(serde_json::json!({"thinking_signature": "sig"}).to_string()),
                attachments_json: None,
                created_at: "2026-01-01T00:00:01.000Z".to_string(),
            },
            // text1 (comes after TC1 in time, but is the anchor message for TC1)
            MessageRecord {
                id: "msg-03-text1".to_string(),
                thread_id: "t1".to_string(),
                run_id: Some("run-1".to_string()),
                role: "assistant".to_string(),
                content_markdown: "Text 1".to_string(),
                message_type: "plain_message".to_string(),
                status: "completed".to_string(),
                metadata_json: None,
                attachments_json: None,
                created_at: "2026-01-01T00:00:05.000Z".to_string(),
            },
            // R2
            MessageRecord {
                id: "msg-04-r2".to_string(),
                thread_id: "t1".to_string(),
                run_id: Some("run-1".to_string()),
                role: "assistant".to_string(),
                content_markdown: "R2 thinking".to_string(),
                message_type: "reasoning".to_string(),
                status: "completed".to_string(),
                metadata_json: Some(serde_json::json!({"thinking_signature": "sig"}).to_string()),
                attachments_json: None,
                created_at: "2026-01-01T00:00:06.000Z".to_string(),
            },
            // R3
            MessageRecord {
                id: "msg-05-r3".to_string(),
                thread_id: "t1".to_string(),
                run_id: Some("run-1".to_string()),
                role: "assistant".to_string(),
                content_markdown: "R3 thinking".to_string(),
                message_type: "reasoning".to_string(),
                status: "completed".to_string(),
                metadata_json: Some(serde_json::json!({"thinking_signature": "sig"}).to_string()),
                attachments_json: None,
                created_at: "2026-01-01T00:00:09.000Z".to_string(),
            },
            // text2 — final reply
            MessageRecord {
                id: "msg-06-text2".to_string(),
                thread_id: "t1".to_string(),
                run_id: Some("run-1".to_string()),
                role: "assistant".to_string(),
                content_markdown: "Text 2".to_string(),
                message_type: "plain_message".to_string(),
                status: "completed".to_string(),
                metadata_json: None,
                attachments_json: None,
                created_at: "2026-01-01T00:00:12.000Z".to_string(),
            },
        ];

        let tool_calls = vec![
            // TC1: started after R1, before text1
            ToolCallDto {
                id: "tc-1".to_string(),
                storage_id: "st-1".to_string(),
                run_id: "run-1".to_string(),
                thread_id: "t1".to_string(),
                helper_id: None,
                tool_name: "shell".to_string(),
                tool_input: serde_json::json!({"command": "ls"}),
                tool_output: Some(serde_json::json!("out1")),
                status: "completed".to_string(),
                approval_status: None,
                started_at: "2026-01-01T00:00:02.000Z".to_string(),
                finished_at: Some("2026-01-01T00:00:03.000Z".to_string()),
            },
            // TC2: started after R2, before R3
            ToolCallDto {
                id: "tc-2".to_string(),
                storage_id: "st-2".to_string(),
                run_id: "run-1".to_string(),
                thread_id: "t1".to_string(),
                helper_id: None,
                tool_name: "shell".to_string(),
                tool_input: serde_json::json!({"command": "pwd"}),
                tool_output: Some(serde_json::json!("out2")),
                status: "completed".to_string(),
                approval_status: None,
                started_at: "2026-01-01T00:00:07.000Z".to_string(),
                finished_at: Some("2026-01-01T00:00:08.000Z".to_string()),
            },
        ];

        let model = &sample_resolved_model_role("primary").model;
        let history = convert_history_messages(&messages, &tool_calls, model);

        // Expected timeline after sort:
        // pos 0: User("Go")
        // pos 1 (reasoning): R1 → PendingThinking
        // before pos 2: TC1-assistant (gets R1 thinking prepended) → [Thinking(R1), ToolCall]
        // before pos 2: TC1-result
        // pos 2: text1 → Assistant[Text("Text 1")]
        // pos 3: R2 → PendingThinking
        // pos 4: R3 → PendingThinking
        // before pos 5: TC2-assistant → ... wait, TC2 started at 00:07, find first msg
        //   in run-1 with created_at >= 00:07 → msg-05-r3 at 00:09 (pos 4)? No,
        //   reasoning messages are not plain_message/assistant so they are in msg_positions
        //   but the find looks for created_at >= started_at.

        // Actually, msg_positions includes ALL messages (including reasoning).
        // TC2 started_at = 00:07. First message in run-1 with created_at >= 00:07:
        //   msg-04-r2 created_at 00:06 < 00:07 → no
        //   msg-05-r3 created_at 00:09 >= 00:07 → pos 4!
        // So TC2 inserts before pos 4 (msg-05-r3).
        //
        // Timeline:
        // (0,2,0) User
        // (1,2,0) PendingThinking(R1)
        // (2,0,0) TC1-assistant  ← gets R1
        // (2,0,1) TC1-result
        // (2,2,0) text1 (Assistant[Text])
        // (3,2,0) PendingThinking(R2)
        // (4,0,2) TC2-assistant  ← gets R2
        // (4,0,3) TC2-result
        // (4,2,0) PendingThinking(R3)
        // (5,2,0) text2 (Assistant[Text]) ← gets R3
        //
        // Result: User, Asst[T(R1),TC1], TR1, Asst[Text1], Asst[T(R2),TC2], TR2, Asst[T(R3),Text2]
        assert_eq!(
            history.len(),
            7,
            "expected 7 messages, got {}: {:?}",
            history.len(),
            history
                .iter()
                .map(|m| match m {
                    AgentMessage::User(_) => "User",
                    AgentMessage::Assistant(_) => "Assistant",
                    AgentMessage::ToolResult(_) => "ToolResult",
                    _ => "Other",
                })
                .collect::<Vec<_>>()
        );

        // 1. User
        assert!(matches!(&history[0], AgentMessage::User(_)));

        // 2. TC1 assistant with R1 thinking
        match &history[1] {
            AgentMessage::Assistant(a) => {
                assert_eq!(a.content.len(), 2, "TC1 assistant: Thinking + ToolCall");
                assert!(a.content[0].is_thinking());
                assert_eq!(a.content[0].as_thinking().unwrap().thinking, "R1 thinking");
                assert!(a.content[1].is_tool_call());
            }
            other => panic!("expected TC1 assistant, got {:?}", other),
        }

        // 3. TC1 result
        assert!(matches!(&history[2], AgentMessage::ToolResult(_)));

        // 4. text1 — no thinking (R1 was consumed by TC1)
        match &history[3] {
            AgentMessage::Assistant(a) => {
                assert_eq!(a.content.len(), 1, "text1 should have only Text");
                assert!(a.content[0].is_text());
                assert_eq!(a.content[0].as_text().unwrap().text, "Text 1");
            }
            other => panic!("expected text1 assistant, got {:?}", other),
        }

        // 5. TC2 assistant with R2 thinking
        match &history[4] {
            AgentMessage::Assistant(a) => {
                assert_eq!(a.content.len(), 2, "TC2 assistant: Thinking + ToolCall");
                assert!(a.content[0].is_thinking());
                assert_eq!(a.content[0].as_thinking().unwrap().thinking, "R2 thinking");
                assert!(a.content[1].is_tool_call());
            }
            other => panic!("expected TC2 assistant, got {:?}", other),
        }

        // 6. TC2 result
        assert!(matches!(&history[5], AgentMessage::ToolResult(_)));

        // 7. text2 with R3 thinking
        match &history[6] {
            AgentMessage::Assistant(a) => {
                assert_eq!(a.content.len(), 2, "text2: Thinking(R3) + Text");
                assert!(a.content[0].is_thinking());
                assert_eq!(a.content[0].as_thinking().unwrap().thinking, "R3 thinking");
                assert!(a.content[1].is_text());
                assert_eq!(a.content[1].as_text().unwrap().text, "Text 2");
            }
            other => panic!("expected text2 assistant, got {:?}", other),
        }
    }

    #[test]
    fn convert_history_messages_skips_empty_assistant_plain_message() {
        // Scenario: a provider error left an empty assistant plain_message
        // in the DB.  The reasoning before it should be treated as orphan
        // and dropped; the empty assistant should be skipped entirely.
        let messages = vec![
            MessageRecord {
                id: "msg-user".to_string(),
                thread_id: "t1".to_string(),
                run_id: None,
                role: "user".to_string(),
                content_markdown: "Do something".to_string(),
                message_type: "plain_message".to_string(),
                status: "completed".to_string(),
                metadata_json: None,
                attachments_json: None,
                created_at: "2026-01-01T00:00:00.000Z".to_string(),
            },
            // Reasoning from a run that later failed
            MessageRecord {
                id: "msg-reasoning".to_string(),
                thread_id: "t1".to_string(),
                run_id: Some("run-1".to_string()),
                role: "assistant".to_string(),
                content_markdown: "Thinking about the task.".to_string(),
                message_type: "reasoning".to_string(),
                status: "completed".to_string(),
                metadata_json: Some(
                    serde_json::json!({"thinking_signature": "reasoning_content"}).to_string(),
                ),
                attachments_json: None,
                created_at: "2026-01-01T00:00:01.000Z".to_string(),
            },
            // Empty assistant plain_message left by the failed run
            MessageRecord {
                id: "msg-empty".to_string(),
                thread_id: "t1".to_string(),
                run_id: Some("run-1".to_string()),
                role: "assistant".to_string(),
                content_markdown: String::new(),
                message_type: "plain_message".to_string(),
                status: "completed".to_string(),
                metadata_json: None,
                attachments_json: None,
                created_at: "2026-01-01T00:00:02.000Z".to_string(),
            },
            // Next user message (new run)
            MessageRecord {
                id: "msg-user-2".to_string(),
                thread_id: "t1".to_string(),
                run_id: None,
                role: "user".to_string(),
                content_markdown: "Continue".to_string(),
                message_type: "plain_message".to_string(),
                status: "completed".to_string(),
                metadata_json: None,
                attachments_json: None,
                created_at: "2026-01-01T00:00:03.000Z".to_string(),
            },
        ];

        let model = &sample_resolved_model_role("primary").model;
        let history = convert_history_messages(&messages, &[], model);

        // Empty assistant is skipped; reasoning before it becomes orphan
        // (no following assistant to attach to before the next user message)
        // and is also dropped.  Result: User, User.
        assert_eq!(
            history.len(),
            2,
            "expected 2 messages (both users), got {}: {:?}",
            history.len(),
            history
                .iter()
                .map(|m| match m {
                    AgentMessage::User(_) => "User",
                    AgentMessage::Assistant(_) => "Assistant",
                    AgentMessage::ToolResult(_) => "ToolResult",
                    _ => "Other",
                })
                .collect::<Vec<_>>()
        );
        assert!(matches!(&history[0], AgentMessage::User(_)));
        assert!(matches!(&history[1], AgentMessage::User(_)));
    }

    #[test]
    fn convert_history_messages_skips_whitespace_only_assistant_plain_message() {
        // Same as above but with whitespace-only content (trimmed to empty).
        let messages = vec![
            MessageRecord {
                id: "msg-user".to_string(),
                thread_id: "t1".to_string(),
                run_id: None,
                role: "user".to_string(),
                content_markdown: "Hello".to_string(),
                message_type: "plain_message".to_string(),
                status: "completed".to_string(),
                metadata_json: None,
                attachments_json: None,
                created_at: "2026-01-01T00:00:00.000Z".to_string(),
            },
            MessageRecord {
                id: "msg-ws".to_string(),
                thread_id: "t1".to_string(),
                run_id: Some("run-1".to_string()),
                role: "assistant".to_string(),
                content_markdown: "   \n  ".to_string(),
                message_type: "plain_message".to_string(),
                status: "completed".to_string(),
                metadata_json: None,
                attachments_json: None,
                created_at: "2026-01-01T00:00:01.000Z".to_string(),
            },
        ];

        let model = &sample_resolved_model_role("primary").model;
        let history = convert_history_messages(&messages, &[], model);

        assert_eq!(history.len(), 1);
        assert!(matches!(&history[0], AgentMessage::User(_)));
    }

    // -----------------------------------------------------------------------
    // persist_compression_markers_to_pool — data-integrity tests
    // -----------------------------------------------------------------------

    mod persist_markers {
        use super::super::persist_compression_markers_to_pool;
        use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
        use sqlx::{Row, SqlitePool};
        use std::str::FromStr;

        async fn setup_pool() -> SqlitePool {
            let options = SqliteConnectOptions::from_str("sqlite::memory:")
                .expect("invalid sqlite options")
                .foreign_keys(true);

            let pool = SqlitePoolOptions::new()
                .max_connections(1)
                .connect_with(options)
                .await
                .expect("failed to create in-memory pool");

            crate::persistence::sqlite::run_migrations(&pool)
                .await
                .expect("migrations failed");

            sqlx::query(
                "INSERT INTO workspaces (id, name, path, canonical_path, display_path,
                        is_default, is_git, auto_work_tree, status, created_at, updated_at)
                 VALUES ('ws-1', 'ws', '/tmp', '/tmp', '/tmp', 0, 0, 0, 'ready',
                         strftime('%Y-%m-%dT%H:%M:%fZ', 'now'),
                         strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))",
            )
            .execute(&pool)
            .await
            .expect("seed workspace");

            sqlx::query(
                "INSERT INTO threads (id, workspace_id, title, status, created_at, updated_at, last_active_at)
                 VALUES ('t1', 'ws-1', 't', 'idle',
                         strftime('%Y-%m-%dT%H:%M:%fZ', 'now'),
                         strftime('%Y-%m-%dT%H:%M:%fZ', 'now'),
                         strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))",
            )
            .execute(&pool)
            .await
            .expect("seed thread");

            pool
        }

        /// Fetch the two markers we wrote, ordered by id ascending (reset then summary
        /// because UUID v7 is time-ordered and `reset` was written first).
        async fn fetch_markers(pool: &SqlitePool) -> Vec<(String, String, String, Option<String>)> {
            sqlx::query(
                "SELECT id, content_markdown, message_type, metadata_json
                   FROM messages
                  WHERE thread_id = 't1'
                    AND message_type = 'summary_marker'
               ORDER BY id ASC",
            )
            .fetch_all(pool)
            .await
            .expect("fetch markers")
            .into_iter()
            .map(|row| {
                (
                    row.get::<String, _>("id"),
                    row.get::<String, _>("content_markdown"),
                    row.get::<String, _>("message_type"),
                    row.get::<Option<String>, _>("metadata_json"),
                )
            })
            .collect()
        }

        #[tokio::test]
        async fn writes_reset_then_summary_with_boundary_id_embedded() {
            let pool = setup_pool().await;

            persist_compression_markers_to_pool(
                &pool,
                "t1",
                "<context_summary>\nState A\n</context_summary>",
                "auto",
                Some("boundary-42"),
            )
            .await
            .expect("markers should persist");

            let rows = fetch_markers(&pool).await;
            assert_eq!(rows.len(), 2, "should have written exactly 2 markers");

            let (reset_id, reset_body, reset_type, reset_meta) = &rows[0];
            let (summary_id, summary_body, summary_type, summary_meta) = &rows[1];

            // Invariant: reset written first → lower UUID v7 id.
            assert!(
                reset_id < summary_id,
                "reset ({}) must have a smaller id than summary ({})",
                reset_id,
                summary_id
            );

            assert_eq!(reset_type, "summary_marker");
            assert_eq!(summary_type, "summary_marker");
            assert_eq!(reset_body, "Context is now reset");
            assert_eq!(
                summary_body,
                "<context_summary>\nState A\n</context_summary>"
            );

            let reset_meta_val: serde_json::Value =
                serde_json::from_str(reset_meta.as_ref().expect("reset metadata present"))
                    .expect("reset metadata is valid json");
            assert_eq!(reset_meta_val["kind"], "context_reset");
            assert_eq!(reset_meta_val["source"], "auto");
            assert_eq!(reset_meta_val["boundaryMessageId"], "boundary-42");

            let summary_meta_val: serde_json::Value =
                serde_json::from_str(summary_meta.as_ref().expect("summary metadata present"))
                    .expect("summary metadata is valid json");
            assert_eq!(summary_meta_val["kind"], "context_summary");
            assert_eq!(summary_meta_val["source"], "auto");
            // boundaryMessageId only ever belongs on the reset row.
            assert!(
                summary_meta_val.get("boundaryMessageId").is_none(),
                "summary metadata should not carry boundaryMessageId"
            );
        }

        #[tokio::test]
        async fn omits_boundary_id_when_none() {
            let pool = setup_pool().await;

            persist_compression_markers_to_pool(
                &pool,
                "t1",
                "<context_summary>\nState\n</context_summary>",
                "auto_fallback",
                None,
            )
            .await
            .expect("markers should persist");

            let rows = fetch_markers(&pool).await;
            assert_eq!(rows.len(), 2);

            let reset_meta: serde_json::Value =
                serde_json::from_str(rows[0].3.as_ref().expect("reset metadata present"))
                    .expect("reset metadata is valid json");
            assert!(
                reset_meta.get("boundaryMessageId").is_none(),
                "None boundary id must not add any metadata key"
            );
            assert_eq!(reset_meta["source"], "auto_fallback");
        }

        #[tokio::test]
        async fn treats_empty_boundary_id_like_none() {
            // A defensive contract: callers that resolve the boundary via
            // `find_nth_from_end_id` may occasionally hand back an empty string.
            // The function must treat that identically to `None` rather than
            // writing `"boundaryMessageId": ""` to the DB (which would make
            // `list_since_last_reset` try to compare against an empty id).
            let pool = setup_pool().await;

            persist_compression_markers_to_pool(
                &pool,
                "t1",
                "<context_summary>\nState\n</context_summary>",
                "auto",
                Some(""),
            )
            .await
            .expect("markers should persist");

            let rows = fetch_markers(&pool).await;
            let reset_meta: serde_json::Value =
                serde_json::from_str(rows[0].3.as_ref().expect("reset metadata present"))
                    .expect("reset metadata is valid json");
            assert!(
                reset_meta.get("boundaryMessageId").is_none(),
                "empty boundary id must be treated identically to None"
            );
        }

        #[tokio::test]
        async fn source_label_flows_through_to_both_markers() {
            let pool = setup_pool().await;

            persist_compression_markers_to_pool(
                &pool,
                "t1",
                "<context_summary>\nState\n</context_summary>",
                "manual_compact",
                Some("boundary-1"),
            )
            .await
            .expect("markers should persist");

            let rows = fetch_markers(&pool).await;
            let reset_meta: serde_json::Value =
                serde_json::from_str(rows[0].3.as_ref().unwrap()).unwrap();
            let summary_meta: serde_json::Value =
                serde_json::from_str(rows[1].3.as_ref().unwrap()).unwrap();
            assert_eq!(reset_meta["source"], "manual_compact");
            assert_eq!(summary_meta["source"], "manual_compact");
        }
    }

    // -----------------------------------------------------------------------
    // run_auto_compression — orchestration path coverage
    //
    // These tests drive the extracted run_auto_compression function directly
    // without standing up a full AgentSession. By using `Weak::new()` (an
    // already-dangling weak reference), we cover the paths that do NOT make
    // an LLM call — should_compress early-return and cut_point==0 truncation
    // fallback. Paths that actually invoke a provider need integration-level
    // mocking and are out of scope here.
    // -----------------------------------------------------------------------

    mod run_auto_compression {
        use super::super::{run_auto_compression, AgentSession};
        use super::sample_resolved_model_role;
        use std::sync::Weak;
        use tiycore::agent::AgentMessage;
        use tiycore::types::{
            Api, AssistantMessage, ContentBlock, Provider, StopReason, TextContent,
            ToolResultMessage, Usage, UserMessage,
        };

        fn make_user(text: &str) -> AgentMessage {
            AgentMessage::User(UserMessage::text(text))
        }

        fn make_assistant(text: &str) -> AgentMessage {
            AgentMessage::Assistant(
                AssistantMessage::builder()
                    .content(vec![ContentBlock::Text(TextContent::new(text))])
                    .api(Api::OpenAICompletions)
                    .provider(Provider::OpenAI)
                    .model("test")
                    .usage(Usage::default())
                    .stop_reason(StopReason::Stop)
                    .build()
                    .unwrap(),
            )
        }

        fn make_tool_result(name: &str, content: &str) -> AgentMessage {
            AgentMessage::ToolResult(ToolResultMessage::text("tc-1", name, content, false))
        }

        fn settings_for_test(
            context_window: u32,
            reserve_tokens: u32,
            keep_recent_tokens: u32,
        ) -> crate::core::context_compression::CompressionSettings {
            crate::core::context_compression::CompressionSettings {
                context_window,
                reserve_tokens,
                keep_recent_tokens,
            }
        }

        #[tokio::test]
        async fn returns_messages_unchanged_when_under_budget() {
            // With a generous budget, should_compress is false and the function
            // is a pure pass-through — no clone of messages, no LLM call, no
            // DB access. This exercises the most common hot-path behaviour.
            let messages = vec![make_user("hi"), make_assistant("hello")];
            let settings = settings_for_test(128_000, 1_024, 1_024);

            let result = run_auto_compression(
                messages.clone(),
                settings,
                sample_resolved_model_role("primary-model"),
                Weak::<AgentSession>::new(),
                "thread-x".to_string(),
                "run-x".to_string(),
                None,
            )
            .await;

            assert_eq!(result.len(), messages.len());
            // Content should be byte-identical — no summary was injected.
            match (&result[0], &messages[0]) {
                (AgentMessage::User(a), AgentMessage::User(b)) => {
                    let at = match &a.content {
                        tiycore::types::UserContent::Text(t) => t.as_str(),
                        _ => panic!("expected text"),
                    };
                    let bt = match &b.content {
                        tiycore::types::UserContent::Text(t) => t.as_str(),
                        _ => panic!("expected text"),
                    };
                    assert_eq!(at, bt);
                }
                _ => panic!("expected user message at head"),
            }
        }

        #[tokio::test]
        async fn cut_point_zero_falls_back_to_truncation_without_llm() {
            // When cut_point resolves to 0 (e.g., a thread dominated by
            // ToolResult messages with no safe cut boundary), the function
            // returns via compress_context_fallback BEFORE making any LLM
            // call. A dangling Weak reference proves no DB or session state
            // is required on this branch.
            let mut messages = Vec::new();
            // A long sequence of tool results with no user/assistant split —
            // find_cut_point will walk all the way back to 0 and the
            // tool-result boundary adjustment keeps it there.
            for i in 0..40 {
                messages.push(make_tool_result(
                    "read",
                    &format!("contents {}: {}", i, "x".repeat(600)),
                ));
            }

            // Tiny budget forces should_compress = true.
            let settings = settings_for_test(2_000, 500, 500);
            assert!(
                crate::core::context_compression::should_compress(&messages, &settings),
                "precondition: messages should be over budget"
            );

            let result = run_auto_compression(
                messages.clone(),
                settings.clone(),
                sample_resolved_model_role("primary-model"),
                Weak::<AgentSession>::new(),
                "thread-y".to_string(),
                "run-y".to_string(),
                None,
            )
            .await;

            // compress_context_fallback was used — result has fewer messages,
            // or (for all-tool-result threads) in-place truncated content.
            // The crucial property is: the function returned successfully
            // despite the dangling Weak, proving the LLM path was skipped.
            assert!(!result.is_empty());
            assert!(result.len() <= messages.len());
        }
    }
}
