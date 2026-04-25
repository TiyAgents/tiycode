use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex as StdMutex, Weak};

use sqlx::SqlitePool;
use tiycore::agent::{Agent, AgentError, AgentToolResult, ToolExecutionMode};
use tiycore::thinking::ThinkingLevel;
use tiycore::types::{ContentBlock, Cost, InputType, Model, Provider, TextContent, Usage};
use tokio::sync::mpsc;

use crate::core::plan_checkpoint::{
    approval_prompt_markdown, build_approval_prompt_metadata, build_plan_artifact_from_tool_input,
    build_plan_message_metadata, plan_markdown, write_plan_file,
};
use crate::core::prompt;
use crate::core::subagent::{
    extract_review_report, HelperAgentOrchestrator, HelperRunRequest, ReviewRequest,
    RuntimeOrchestrationTool,
};
use crate::core::tool_gateway::{
    ApprovalRequest, ToolExecutionOptions, ToolExecutionRequest, ToolGateway, ToolGatewayResult,
};
use crate::extensions::ExtensionsManager;
use crate::ipc::frontend_channels::ThreadStreamEvent;
use crate::model::errors::{AppError, ErrorSource};
use crate::model::thread::MessageRecord;
use crate::persistence::repo::{message_repo, provider_repo, run_repo, tool_call_repo};

/// Deprecated: previously used as the hard limit for `message_repo::list_recent` in
/// `build_session_spec`.  Replaced by `message_repo::list_since_last_reset()` which
/// queries the DB for the reset boundary directly.  Retained for reference only.
#[allow(dead_code)]
const MESSAGE_HISTORY_LIMIT: i64 = 200;
pub(crate) const DEFAULT_CONTEXT_WINDOW: u32 = 128_000;
const DEFAULT_MAX_OUTPUT_TOKENS: u32 = 32_000;
pub(crate) const DEFAULT_FULL_TOOL_PROFILE: &str = "default_full";
pub(crate) const PLAN_READ_ONLY_TOOL_PROFILE: &str = "plan_read_only";
const STANDARD_TOOL_TIMEOUT_SECS: u64 = 120;
const SUBAGENT_TOOL_TIMEOUT_SECS: u64 = 600;
/// Main agent timeout is effectively unlimited (24 h) because user-interactive
/// tools like `clarify` and approval prompts must wait for human input without
/// being killed by the outer tiycore timeout.  The per-tool execution timeout
/// inside `tool_gateway` (120 s) still guards against runaway non-interactive
/// tool calls.
const MAIN_AGENT_TOOL_TIMEOUT_SECS: u64 = 86_400;
pub(crate) const CLARIFY_TOOL_NAME: &str = "clarify";
pub(crate) const PLAN_MODE_MISSING_CHECKPOINT_ERROR: &str =
    "Plan mode requires publishing a plan with update_plan before the run can finish.";
pub(crate) const TEXT_ATTACHMENT_MAX_CHARS: usize = 12_000;

use crate::core::agent_session_compression::{
    build_initial_context_token_calibration, current_context_token_calibration,
    persist_compression_markers_to_pool, record_pending_prompt_estimate, run_auto_compression,
    ContextCompressionRuntimeState,
};
use crate::core::agent_session_events::handle_agent_event;
pub(crate) use crate::core::agent_session_history::*;
pub(crate) use crate::core::agent_session_tools::*;
pub use crate::core::agent_session_types::*;

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

pub struct AgentSession {
    spec: AgentSessionSpec,
    pub(crate) pool: SqlitePool,
    tool_gateway: Arc<ToolGateway>,
    helper_orchestrator: Arc<HelperAgentOrchestrator>,
    pub(crate) event_tx: mpsc::UnboundedSender<ThreadStreamEvent>,
    pub(crate) agent: Arc<Agent>,
    cancel_requested: Arc<AtomicBool>,
    checkpoint_requested: AtomicBool,
    pub(crate) abort_signal: tiycore::agent::AbortSignal,
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
        // 1. Cascade-cancel all in-flight tool_gateway calls (including subagent ones
        //    via child tokens) so they return Cancelled immediately.
        self.abort_signal.cancel();
        // 2. Abort subagent Agent instances so their LLM streaming / run loops stop.
        self.helper_orchestrator.cancel_run(&self.spec.run_id).await;
        // 3. Yield to let already-aborted subagents finish cleanup and emit
        //    SubagentFailed before the main agent terminates.
        tokio::task::yield_now().await;
        // 4. Finally abort the main agent.
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
                session_abort_signal: self.abort_signal.clone(),
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
        ResolvedModelRole, ResolvedRuntimeModelPlan, RuntimeModelPlan, SortKey,
        DEFAULT_FULL_TOOL_PROFILE, MAIN_AGENT_TOOL_TIMEOUT_SECS,
        PLAN_MODE_MISSING_CHECKPOINT_ERROR, PLAN_READ_ONLY_TOOL_PROFILE,
        STANDARD_TOOL_TIMEOUT_SECS, SUBAGENT_TOOL_TIMEOUT_SECS,
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
        // No text message to merge into, so tool call gets its own standalone
        // assistant message.  The reasoning message's position IS the
        // insert_pos for the tool call.  With SortKey::after_position the
        // standalone sorts after the reasoning, so Phase 4 attaches the
        // PendingThinking correctly.
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
            // Reasoning arrives at 00:00:02 — AFTER the tool call's started_at
            // so that insert_pos points here (the first message in run-1 with
            // created_at >= tc.started_at).
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
                created_at: "2026-01-01T00:00:02.000Z".to_string(),
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
            started_at: "2026-01-01T00:00:01.000Z".to_string(),
            finished_at: Some("2026-01-01T00:00:03.000Z".to_string()),
        }];

        let model = &sample_resolved_model_role("primary").model;
        let history = convert_history_messages(&messages, &tool_calls, model);

        // insert_pos = reasoning (index 1), no preceding text → standalone.
        // Phase 4: reasoning PendingThinking at (1,2) → standalone at (1,3)
        // gets Thinking attached.
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
        // Both tool calls insert at the same position (reasoning message), so they
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
            // Reasoning arrives at 00:00:02 — AFTER both tool calls' started_at
            // so that insert_pos points here for both tool calls.
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
                created_at: "2026-01-01T00:00:02.000Z".to_string(),
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
                started_at: "2026-01-01T00:00:01.000Z".to_string(),
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
                started_at: "2026-01-01T00:00:01.500Z".to_string(),
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
    fn sortkey_ordering() {
        // Same position: before (sub=0) < positional (sub=2) < after (sub=3)
        let before = SortKey::before_position(5, 1);
        let positional = SortKey::positional(5);
        let after = SortKey::after_position(5, 2);
        assert!(
            before < positional,
            "before_position should sort before positional"
        );
        assert!(
            positional < after,
            "positional should sort before after_position"
        );
        assert!(
            before < after,
            "before_position should sort before after_position"
        );

        // Seq tiebreaker for same (position, sub)
        let a = SortKey::before_position(5, 10);
        let b = SortKey::before_position(5, 20);
        assert!(
            a < b,
            "lower seq should sort before higher seq at same (position, sub)"
        );

        // Different positions should still respect sub ordering when positions differ
        let later_pos_before = SortKey::before_position(10, 0);
        let earlier_pos_after = SortKey::after_position(5, 0);
        assert!(
            earlier_pos_after < later_pos_before,
            "position should be primary sort key"
        );
    }

    #[test]
    fn convert_history_messages_multiple_reasoning_tool_call_cycles() {
        // Scenario: user → R1 → text1 → R2 → TC2 → R3 → text2
        // TC1 merges into text1 (no intervening reasoning).
        // TC2 cannot merge because R2 sits between text1 and its insert_pos.
        // R1 attaches to text1, R2 attaches to TC2 standalone, R3 to text2.
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
            // text1 — TC1 merges into this (no reasoning between text1 and TC1)
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
            // R2 — blocks TC2 from merging into text1
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
            // TC1: started AFTER text1, no reasoning between text1 and TC1's
            // insert_pos → merges into text1.
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
                started_at: "2026-01-01T00:00:05.500Z".to_string(),
                finished_at: Some("2026-01-01T00:00:05.800Z".to_string()),
            },
            // TC2: started after R2 but before R3.  insert_pos points to R3
            // (first msg in run-1 with created_at >= 00:00:07).
            // R2 sits between text1 and R3 → merge blocked → standalone.
            // after_position: standalone sorts AFTER R3's PendingThinking.
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

        // Timeline after sort:
        //   (0,2) User
        //   (1,2) PendingThinking(R1)
        //   (2,0,0) TC1-result (merged into text1, result before pos 3)
        //   (2,2) text1 merged with TC1 → Assistant[Thinking(R1), Text, ToolCall]
        //   (3,2) PendingThinking(R2)
        //   (4,2) PendingThinking(R3)
        //   (4,3,2) TC2-standalone → gets R2+R3
        //   (4,3,3) TC2-result
        //   (5,2) text2 → Assistant[Text]
        //
        // Wait, TC1 merge: text1 is at index 2. TC1 insert_pos: first msg in
        // run-1 with created_at >= 00:00:05.5 → msg-04-r2 at 00:06 (index 3).
        // merge_target: search backwards from index 3 for plain_message in run-1
        // → msg-03-text1 at index 2. Check no reasoning between index 2+1=3 and
        // insert_pos 3 → range [3..3) is empty → merge allowed!
        // TC1 merges into text1 at index 2.
        //
        // TC2: insert_pos → msg-05-r3 at 00:09 (index 4).
        // merge_target: search backwards from index 4 → msg-03-text1 at index 2.
        // Check reasoning between 3..4 → msg-04-r2 at index 3 is reasoning → blocked!
        // Standalone at (4, 3, ...).
        //
        // Phase 4:
        //   R1(PendingThinking) → accumulated
        //   text1+TC1 at (2,2) → consumes R1, becomes [Thinking(R1), Text, ToolCall]
        //   TC1-result at (3,0) → pass through
        //   R2(PendingThinking at 3,2) → accumulated
        //   R3(PendingThinking at 4,2) → accumulated
        //   TC2-standalone at (4,3) → consumes R2+R3
        //   TC2-result → pass through
        //   text2 at (5,2) → no pending thinking → just Text
        //
        // Result: User, Asst[T(R1),Text1,TC1], TR1, Asst[T(R2),T(R3),TC2], TR2, Asst[Text2]
        assert_eq!(
            history.len(),
            6,
            "expected 6 messages: {:?}",
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

        // 2. text1 merged with TC1, R1 thinking prepended
        match &history[1] {
            AgentMessage::Assistant(a) => {
                assert_eq!(
                    a.content.len(),
                    3,
                    "text1+TC1: Thinking(R1) + Text + ToolCall, got: {:?}",
                    a.content
                );
                assert!(a.content[0].is_thinking());
                assert_eq!(a.content[0].as_thinking().unwrap().thinking, "R1 thinking");
                assert!(a.content[1].is_text());
                assert!(a.content[2].is_tool_call());
            }
            other => panic!("expected text1+TC1 assistant, got {:?}", other),
        }

        // 3. TC1 result
        assert!(matches!(&history[2], AgentMessage::ToolResult(_)));

        // 4. TC2 standalone with R2+R3 thinking
        match &history[3] {
            AgentMessage::Assistant(a) => {
                // R2 and R3 both accumulated as PendingThinking before TC2
                assert_eq!(
                    a.content.len(),
                    3,
                    "TC2 should have Thinking(R2) + Thinking(R3) + ToolCall"
                );
                assert!(a.content[0].is_thinking());
                assert_eq!(a.content[0].as_thinking().unwrap().thinking, "R2 thinking");
                assert!(a.content[1].is_thinking());
                assert_eq!(a.content[1].as_thinking().unwrap().thinking, "R3 thinking");
                assert!(a.content[2].is_tool_call());
            }
            other => panic!("expected TC2 standalone assistant, got {:?}", other),
        }

        // 5. TC2 result
        assert!(matches!(&history[4], AgentMessage::ToolResult(_)));

        // 6. text2 — no pending thinking, just text
        match &history[5] {
            AgentMessage::Assistant(a) => {
                assert_eq!(
                    a.content.len(),
                    1,
                    "text2 should have only Text, no thinking"
                );
                assert!(a.content[0].is_text());
                assert_eq!(a.content[0].as_text().unwrap().text, "Text 2");
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

    #[test]
    fn convert_history_messages_standalone_tool_call_gets_reasoning_when_at_same_position() {
        // Scenario that triggered DeepSeek 400:
        //   user → reasoning-1 → reasoning-2 → text (later)
        //   tool_call starts between reasoning-1 and reasoning-2
        //
        // No preceding plain_message in run-1 → standalone.
        // insert_pos points to reasoning-2 (first msg >= tc.started_at).
        // With SortKey::after_position, the standalone sorts AFTER
        // reasoning-2's PendingThinking at the same position, so Phase 4
        // attaches reasoning-1 + reasoning-2 to the standalone.
        let messages = vec![
            MessageRecord {
                id: "msg-user".to_string(),
                thread_id: "thread-1".to_string(),
                run_id: None,
                role: "user".to_string(),
                content_markdown: "Do something".to_string(),
                message_type: "plain_message".to_string(),
                status: "completed".to_string(),
                metadata_json: None,
                attachments_json: None,
                created_at: "2026-01-01T00:00:00.000Z".to_string(),
            },
            MessageRecord {
                id: "msg-reasoning-1".to_string(),
                thread_id: "thread-1".to_string(),
                run_id: Some("run-1".to_string()),
                role: "assistant".to_string(),
                content_markdown: "Let me plan this.".to_string(),
                message_type: "reasoning".to_string(),
                status: "completed".to_string(),
                metadata_json: Some(
                    serde_json::json!({"thinking_signature": "reasoning_content"}).to_string(),
                ),
                attachments_json: None,
                created_at: "2026-01-01T00:00:01.000Z".to_string(),
            },
            // reasoning-2 created AFTER tc.started_at → this is the insert_pos
            MessageRecord {
                id: "msg-reasoning-2".to_string(),
                thread_id: "thread-1".to_string(),
                run_id: Some("run-1".to_string()),
                role: "assistant".to_string(),
                content_markdown: "Checking output now.".to_string(),
                message_type: "reasoning".to_string(),
                status: "completed".to_string(),
                metadata_json: Some(
                    serde_json::json!({"thinking_signature": "reasoning_content"}).to_string(),
                ),
                attachments_json: None,
                created_at: "2026-01-01T00:00:04.000Z".to_string(),
            },
            // A later text response after the tool result
            MessageRecord {
                id: "msg-text".to_string(),
                thread_id: "thread-1".to_string(),
                run_id: Some("run-1".to_string()),
                role: "assistant".to_string(),
                content_markdown: "All done.".to_string(),
                message_type: "plain_message".to_string(),
                status: "completed".to_string(),
                metadata_json: None,
                attachments_json: None,
                created_at: "2026-01-01T00:00:08.000Z".to_string(),
            },
        ];

        // TC started_at=00:00:03 < reasoning-2 created_at=00:00:04
        // → insert_pos = reasoning-2 (index 2)
        // No preceding plain_message in run-1 → standalone
        let tool_calls = vec![ToolCallDto {
            id: "tc-1".to_string(),
            storage_id: "st-1".to_string(),
            run_id: "run-1".to_string(),
            thread_id: "thread-1".to_string(),
            helper_id: None,
            tool_name: "shell".to_string(),
            tool_input: serde_json::json!({"command": "cargo test"}),
            tool_output: Some(serde_json::json!("ok")),
            status: "completed".to_string(),
            approval_status: None,
            started_at: "2026-01-01T00:00:03.000Z".to_string(),
            finished_at: Some("2026-01-01T00:00:03.500Z".to_string()),
        }];

        let model = &sample_resolved_model_role("primary").model;
        let history = convert_history_messages(&messages, &tool_calls, model);

        // Timeline after sort:
        //   (0,2) User
        //   (1,2) PendingThinking(reasoning-1)
        //   (2,2) PendingThinking(reasoning-2)
        //   (2,3) Standalone assistant → gets reasoning-1 + reasoning-2
        //   (2,3) ToolResult
        //   (3,2) Assistant[Text("All done.")]
        //
        // Expected: User → Assistant[Thinking×2, ToolCall] → ToolResult → Assistant[Text]
        assert_eq!(
            history.len(),
            4,
            "should have User + standalone-TC + ToolResult + text-assistant, got {}: {:?}",
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

        // The standalone tool-call assistant MUST have Thinking blocks
        match &history[1] {
            AgentMessage::Assistant(assistant) => {
                // reasoning-1 and reasoning-2 both attached
                assert!(
                    assistant.content.len() >= 2,
                    "standalone should have Thinking(s) + ToolCall, got: {:?}",
                    assistant.content
                );
                assert!(
                    assistant.content[0].is_thinking(),
                    "first block should be Thinking, got: {:?}",
                    assistant.content[0]
                );
                let thinking = assistant.content[0].as_thinking().unwrap();
                assert_eq!(thinking.thinking, "Let me plan this.");
                assert_eq!(
                    thinking.thinking_signature.as_deref(),
                    Some("reasoning_content"),
                    "thinking_signature must be preserved for DeepSeek"
                );
                // Last block must be ToolCall
                assert!(
                    assistant.content.last().unwrap().is_tool_call(),
                    "last block should be ToolCall"
                );
            }
            other => panic!("expected standalone Assistant at index 1, got {:?}", other),
        }

        // Tool result at index 2
        assert!(
            matches!(&history[2], AgentMessage::ToolResult(_)),
            "index 2 should be ToolResult"
        );

        // Final text assistant has no orphan thinking
        match &history[3] {
            AgentMessage::Assistant(assistant) => {
                assert_eq!(assistant.content.len(), 1);
                assert!(assistant.content[0].is_text());
            }
            other => panic!("expected text Assistant at index 3, got {:?}", other),
        }
    }
}
