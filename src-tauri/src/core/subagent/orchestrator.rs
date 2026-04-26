use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;
use std::sync::Mutex as StdMutex;

use serde::Serialize;
use sqlx::SqlitePool;
use tiycore::agent::{Agent, AgentEvent, AgentMessage, AgentToolResult, ToolExecutionMode};
use tiycore::thinking::ThinkingLevel;
use tiycore::types::{ContentBlock, TextContent, Usage};
use tokio::sync::Mutex;

use crate::core::agent_session::{
    is_deepseek_provider, merge_payload, normalize_deepseek_thinking_payload, ResolvedModelRole,
};
use crate::core::executors::ToolOutput;
use crate::core::subagent::review_contract::{extract_review_report, render_parent_summary};
use crate::core::subagent::runtime_orchestration::{RuntimeOrchestrationTool, SubagentProfile};
use crate::core::tool_gateway::{
    ToolExecutionOptions, ToolExecutionRequest, ToolGateway, ToolGatewayResult,
};
use crate::ipc::frontend_channels::ThreadStreamEvent;
use crate::model::errors::{AppError, ErrorSource};
use crate::persistence::repo::{run_helper_repo, tool_call_repo};

const MAX_RECENT_ACTIONS: usize = 5;

pub struct HelperRunRequest {
    pub run_id: String,
    pub thread_id: String,
    pub tool: RuntimeOrchestrationTool,
    pub helper_profile: Option<SubagentProfile>,
    pub parent_tool_call_id: Option<String>,
    pub task: String,
    pub model_role: ResolvedModelRole,
    pub system_prompt: String,
    pub workspace_path: String,
    pub run_mode: String,
    pub event_tx: tokio::sync::mpsc::UnboundedSender<ThreadStreamEvent>,
    /// Session-level abort signal. Child tokens derived from this signal are
    /// passed to each subagent tool call so that session cancellation cascades
    /// into in-flight tool executions.
    pub session_abort_signal: tiycore::agent::AbortSignal,
    /// Thinking level inherited from the parent session's model plan so that
    /// DeepSeek thinking-enabled payloads are normalised correctly in the
    /// subagent payload hook.
    pub thinking_level: ThinkingLevel,
}

pub struct HelperRunResult {
    pub summary: String,
    pub raw_summary: Option<String>,
    pub snapshot: SubagentProgressSnapshot,
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SubagentActivityStatus {
    Started,
    Succeeded,
    Failed,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct SubagentProgressSnapshot {
    pub total_tool_calls: u32,
    pub completed_steps: u32,
    pub current_action: Option<String>,
    pub tool_counts: BTreeMap<String, u32>,
    pub recent_actions: Vec<String>,
}

/// Tracks active helper agents for a single run, with a cancellation guard
/// that prevents new helpers from registering after `cancel_run` has fired.
struct RunHelpersState {
    helpers: Vec<Arc<Agent>>,
    cancelled: bool,
}

pub struct HelperAgentOrchestrator {
    pool: SqlitePool,
    tool_gateway: Arc<ToolGateway>,
    active_helpers: Arc<Mutex<HashMap<String, RunHelpersState>>>,
}

#[derive(Debug, Clone)]
struct SubagentActionDescriptor {
    current_action: String,
    started_message: String,
    succeeded_message: String,
    failed_message: String,
}

#[derive(Default)]
struct SubagentProgressState {
    snapshot: SubagentProgressSnapshot,
}

impl HelperAgentOrchestrator {
    pub fn new(pool: SqlitePool, tool_gateway: Arc<ToolGateway>) -> Self {
        Self {
            pool,
            tool_gateway,
            active_helpers: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub async fn run_helper(&self, request: HelperRunRequest) -> Result<HelperRunResult, AppError> {
        let helper_profile = request
            .helper_profile
            .unwrap_or_else(|| request.tool.profile());
        let helper_id = uuid::Uuid::now_v7().to_string();
        let resolved_helper_kind = helper_profile.helper_kind().to_string();
        let escalation_summary = Arc::new(StdMutex::new(None::<String>));
        let progress_state = Arc::new(StdMutex::new(SubagentProgressState::default()));

        let helper_started_at = run_helper_repo::insert(
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
            started_at: helper_started_at.clone(),
            snapshot: snapshot_from_progress(&progress_state),
        });

        let helper_context_window = request.model_role.model.context_window.to_string();
        let helper_model_display_name = request.model_role.model_name.clone();

        let agent = Arc::new(Agent::with_model(request.model_role.model.clone()));
        let max_turns =
            crate::core::agent_runtime_limits::desktop_agent_max_turns(&self.pool).await;
        agent.set_max_turns(max_turns);
        agent.set_system_prompt(build_helper_system_prompt(
            &request.system_prompt,
            helper_profile,
        ));
        agent.set_tools(helper_profile.helper_tools());
        agent.set_tool_execution(ToolExecutionMode::Sequential);

        // Propagate thinking level from the parent session so that the helper
        // agent sends the correct reasoning parameters to the API.  Only
        // enable thinking when the model actually supports it (reasoning flag).
        if request.thinking_level != ThinkingLevel::Off && request.model_role.model.reasoning {
            agent.set_thinking_level(request.thinking_level);
        }

        if let Some(api_key) = request.model_role.api_key.clone() {
            agent.set_api_key(api_key);
        }

        // Inject default TiyCode identification headers for all LLM API requests.
        agent.set_custom_headers(crate::core::tiycode_default_headers());

        // Apply the same URL policy (including HTTPS exemptions for .oa.com domains)
        // used by the main agent so that subagent LLM requests are not rejected
        // when connecting to internal HTTP-only endpoints.
        agent.set_security_config(crate::core::agent_session::runtime_security_config());

        // Always set the payload hook so the DeepSeek thinking normalizer runs
        // even when there are no provider_options to merge.
        {
            let provider_options = request.model_role.provider_options.clone();
            let provider_type = request.model_role.provider_type.clone();
            let base_url = request
                .model_role
                .model
                .base_url
                .clone()
                .unwrap_or_default();
            let thinking_enabled =
                request.thinking_level != ThinkingLevel::Off && request.model_role.model.reasoning;
            agent.set_on_payload(move |payload, _model| {
                let provider_options = provider_options.clone();
                let provider_type = provider_type.clone();
                let base_url = base_url.clone();
                Box::pin(async move {
                    let mut p = payload;
                    if let Some(ref opts) = provider_options {
                        p = merge_payload(p, opts);
                    }
                    let is_ds = is_deepseek_provider(&provider_type, &base_url);
                    p = normalize_deepseek_thinking_payload(p, is_ds, thinking_enabled);
                    Some(p)
                })
            });
        }

        let helper_pool = self.pool.clone();
        let helper_gateway = Arc::clone(&self.tool_gateway);
        let helper_run_id = request.run_id.clone();
        let helper_thread_id = request.thread_id.clone();
        let helper_workspace_path = request.workspace_path.clone();
        let helper_run_mode = request.run_mode.clone();
        let helper_kind = resolved_helper_kind.clone();
        let helper_id_for_tools = helper_id.chars().take(8).collect::<String>();
        let helper_id_for_events = helper_id.clone();
        let helper_started_at_for_events = helper_started_at.clone();
        let helper_agent = Arc::clone(&agent);
        let escalation_summary_ref = Arc::clone(&escalation_summary);
        let progress_state_ref = Arc::clone(&progress_state);
        let progress_event_tx = request.event_tx.clone();
        let helper_session_abort_signal = request.session_abort_signal.clone();
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
            let helper_kind = helper_kind.clone();
            let helper_agent = Arc::clone(&helper_agent);
            let escalation_summary_ref = Arc::clone(&escalation_summary_ref);
            let progress_state_ref = Arc::clone(&progress_state_ref);
            let progress_event_tx = progress_event_tx.clone();
            let helper_id_for_events = helper_id_for_events.clone();
            let helper_id_for_storage = helper_id_for_events.clone();
            let helper_started_at_for_events = helper_started_at_for_events.clone();
            let helper_abort_signal = helper_session_abort_signal.clone();

            async move {
                let action = describe_subagent_action(&tool_name, &tool_input);
                emit_subagent_progress(
                    &progress_event_tx,
                    &helper_run_id,
                    &helper_id_for_events,
                    &helper_kind,
                    &helper_started_at_for_events,
                    SubagentActivityStatus::Started,
                    &progress_state_ref,
                    action.started_message.clone(),
                    |progress| progress.record_started(&tool_name, &action.current_action),
                );

                let tool_call_storage_id = uuid::Uuid::now_v7().to_string();
                if let Err(error) = tool_call_repo::insert(
                    &helper_pool,
                    &tool_call_repo::ToolCallInsert {
                        id: tool_call_storage_id.clone(),
                        tool_call_id: persisted_tool_call_id.clone(),
                        run_id: helper_run_id.clone(),
                        thread_id: helper_thread_id.clone(),
                        helper_id: Some(helper_id_for_storage.clone()),
                        tool_name: tool_name.clone(),
                        tool_input_json: tool_input.to_string(),
                        status: "requested".to_string(),
                    },
                )
                .await
                {
                    emit_subagent_progress(
                        &progress_event_tx,
                        &helper_run_id,
                        &helper_id_for_events,
                        &helper_kind,
                        &helper_started_at_for_events,
                        SubagentActivityStatus::Failed,
                        &progress_state_ref,
                        format!("{} ({error})", action.failed_message),
                        |progress| progress.record_finished(None),
                    );
                    return helper_agent_error_result(format!(
                        "failed to persist helper tool call: {error}"
                    ));
                }

                let request = ToolExecutionRequest {
                    run_id: helper_run_id.clone(),
                    thread_id: helper_thread_id.clone(),
                    tool_call_id: persisted_tool_call_id.clone(),
                    tool_call_storage_id: tool_call_storage_id.clone(),
                    tool_name: tool_name.clone(),
                    tool_input: tool_input.clone(),
                    workspace_path: helper_workspace_path.clone(),
                    run_mode: helper_run_mode.clone(),
                };

                match helper_gateway
                    .execute_tool_call(
                        request,
                        helper_abort_signal.child_token(),
                        ToolExecutionOptions {
                            allow_user_approval: false,
                            execution_timeout: None,
                        },
                        |_| {},
                        || {},
                    )
                    .await
                {
                    Ok(outcome) => match outcome.result {
                        ToolGatewayResult::Executed { output, .. } => {
                            let activity = if output.success {
                                SubagentActivityStatus::Succeeded
                            } else {
                                SubagentActivityStatus::Failed
                            };
                            let message = if output.success {
                                action.succeeded_message.clone()
                            } else {
                                action.failed_message.clone()
                            };
                            emit_subagent_progress(
                                &progress_event_tx,
                                &helper_run_id,
                                &helper_id_for_events,
                                &helper_kind,
                                &helper_started_at_for_events,
                                activity,
                                &progress_state_ref,
                                message,
                                |progress| progress.record_finished(None),
                            );
                            helper_agent_tool_result_from_output(output)
                        }
                        ToolGatewayResult::Denied { reason, .. } => {
                            emit_subagent_progress(
                                &progress_event_tx,
                                &helper_run_id,
                                &helper_id_for_events,
                                &helper_kind,
                                &helper_started_at_for_events,
                                SubagentActivityStatus::Failed,
                                &progress_state_ref,
                                format!("{} ({reason})", action.failed_message),
                                |progress| progress.record_finished(Some(reason.clone())),
                            );
                            helper_agent_error_result(reason)
                        }
                        ToolGatewayResult::EscalationRequired { reason, .. } => {
                            let summary = format!(
                                "Escalation required from the parent run: `{}` cannot complete this step because {}. Ask the parent run to perform or approve it directly.",
                                helper_kind, reason
                            );
                            emit_subagent_progress(
                                &progress_event_tx,
                                &helper_run_id,
                                &helper_id_for_events,
                                &helper_kind,
                                &helper_started_at_for_events,
                                SubagentActivityStatus::Failed,
                                &progress_state_ref,
                                format!("{} ({reason})", action.failed_message),
                                |progress| progress.record_finished(Some(reason.clone())),
                            );
                            if let Ok(mut slot) = escalation_summary_ref.lock() {
                                *slot = Some(summary.clone());
                            }
                            helper_agent.abort();
                            helper_agent_error_result(summary)
                        }
                        ToolGatewayResult::Cancelled { .. } => {
                            emit_subagent_progress(
                                &progress_event_tx,
                                &helper_run_id,
                                &helper_id_for_events,
                                &helper_kind,
                                &helper_started_at_for_events,
                                SubagentActivityStatus::Failed,
                                &progress_state_ref,
                                "Helper tool execution cancelled".to_string(),
                                |progress| {
                                    progress.record_finished(Some(
                                        "Helper tool execution cancelled".to_string(),
                                    ))
                                },
                            );
                            helper_agent_error_result("Helper tool execution cancelled")
                        }
                        // NOTE: Currently unreachable because subagent passes
                        // `execution_timeout: None`. This arm exists for forward-
                        // compatibility so enabling a timeout later won't cause a
                        // non-exhaustive match error.
                        ToolGatewayResult::TimedOut { timeout_secs, .. } => {
                            let message =
                                format!("Helper tool timed out after {timeout_secs}s");
                            emit_subagent_progress(
                                &progress_event_tx,
                                &helper_run_id,
                                &helper_id_for_events,
                                &helper_kind,
                                &helper_started_at_for_events,
                                SubagentActivityStatus::Failed,
                                &progress_state_ref,
                                message.clone(),
                                |progress| {
                                    progress.record_finished(Some(message.clone()))
                                },
                            );
                            helper_agent_error_result(message)
                        }
                    },
                    Err(error) => {
                        emit_subagent_progress(
                            &progress_event_tx,
                            &helper_run_id,
                            &helper_id_for_events,
                            &helper_kind,
                            &helper_started_at_for_events,
                            SubagentActivityStatus::Failed,
                            &progress_state_ref,
                            format!("{} ({error})", action.failed_message),
                            |progress| progress.record_finished(Some(error.to_string())),
                        );
                        helper_agent_error_result(error.to_string())
                    }
                }
            }
        });

        {
            let mut helpers = self.active_helpers.lock().await;
            let state = helpers
                .entry(request.run_id.clone())
                .or_insert_with(|| RunHelpersState {
                    helpers: Vec::new(),
                    cancelled: false,
                });
            if state.cancelled {
                return Err(AppError::internal(
                    ErrorSource::Thread,
                    "run cancelled".to_string(),
                ));
            }
            state.helpers.push(Arc::clone(&agent));
        }

        let helper_run_id_for_usage = request.run_id.clone();
        let helper_id_for_usage = helper_id.clone();
        let helper_kind_for_usage = resolved_helper_kind.clone();
        let helper_started_at_for_usage = helper_started_at.clone();
        let helper_event_tx_for_usage = request.event_tx.clone();
        let progress_state_for_usage = Arc::clone(&progress_state);
        let unsubscribe = agent.subscribe(move |event| {
            handle_helper_agent_event(
                &helper_run_id_for_usage,
                &helper_id_for_usage,
                &helper_kind_for_usage,
                &helper_started_at_for_usage,
                &helper_event_tx_for_usage,
                &progress_state_for_usage,
                &helper_context_window,
                &helper_model_display_name,
                event,
            );
        });

        let result = agent.prompt(request.task.clone()).await;
        unsubscribe();
        self.remove_helper(&request.run_id, &agent).await;

        if let Some(raw_summary) = take_escalation_summary(&escalation_summary) {
            let usage = Usage::default();
            let snapshot = snapshot_from_progress(&progress_state);
            let summary = finalize_helper_summary(helper_profile, &raw_summary);
            run_helper_repo::mark_completed(&self.pool, &helper_id, &summary, &usage).await?;

            let _ = request.event_tx.send(ThreadStreamEvent::SubagentCompleted {
                run_id: request.run_id,
                subtask_id: helper_id,
                helper_kind: resolved_helper_kind.clone(),
                started_at: helper_started_at.clone(),
                summary: Some(summary.clone()),
                snapshot: snapshot.clone(),
            });

            return Ok(HelperRunResult {
                summary,
                raw_summary: Some(raw_summary),
                snapshot,
            });
        }

        match result {
            Ok(messages) => {
                let raw_summary = extract_summary(&messages)
                    .unwrap_or_else(|| "Helper completed without a textual summary.".to_string());
                let summary = finalize_helper_summary(helper_profile, &raw_summary);
                let usage = extract_usage(&messages).unwrap_or_default();
                let snapshot = snapshot_from_progress(&progress_state);

                run_helper_repo::mark_completed(&self.pool, &helper_id, &summary, &usage).await?;

                let _ = request.event_tx.send(ThreadStreamEvent::SubagentCompleted {
                    run_id: request.run_id,
                    subtask_id: helper_id,
                    helper_kind: resolved_helper_kind,
                    started_at: helper_started_at.clone(),
                    summary: Some(summary.clone()),
                    snapshot: snapshot.clone(),
                });

                Ok(HelperRunResult {
                    summary,
                    raw_summary: Some(raw_summary),
                    snapshot,
                })
            }
            Err(error) => {
                let interrupted = error.to_string().to_lowercase().contains("aborted");
                let usage = Usage::default();
                let snapshot = snapshot_from_progress(&progress_state);
                run_helper_repo::mark_failed(
                    &self.pool,
                    &helper_id,
                    &error.to_string(),
                    interrupted,
                    &usage,
                )
                .await?;

                let _ = request.event_tx.send(ThreadStreamEvent::SubagentFailed {
                    run_id: request.run_id,
                    subtask_id: helper_id,
                    helper_kind: resolved_helper_kind,
                    started_at: helper_started_at,
                    error: error.to_string(),
                    snapshot,
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
            if let Some(state) = active.get_mut(run_id) {
                state.cancelled = true;
                std::mem::take(&mut state.helpers)
            } else {
                Vec::new()
            }
        };

        for helper in helpers {
            helper.abort();
        }
    }

    async fn remove_helper(&self, run_id: &str, helper: &Arc<Agent>) {
        let mut active = self.active_helpers.lock().await;
        if let Some(state) = active.get_mut(run_id) {
            state
                .helpers
                .retain(|candidate| !Arc::ptr_eq(candidate, helper));
            if state.helpers.is_empty() {
                active.remove(run_id);
            }
        }
    }
}

fn finalize_helper_summary(helper_profile: SubagentProfile, raw_summary: &str) -> String {
    if helper_profile == SubagentProfile::Review {
        extract_review_report(raw_summary)
            .map(|report| render_parent_summary(&report))
            .unwrap_or_else(|| raw_summary.to_string())
    } else {
        raw_summary.to_string()
    }
}

impl SubagentProgressState {
    fn record_started(&mut self, tool_name: &str, current_action: &str) {
        self.snapshot.total_tool_calls += 1;
        self.snapshot.current_action = Some(current_action.to_string());
        *self
            .snapshot
            .tool_counts
            .entry(tool_name.to_string())
            .or_insert(0) += 1;
        self.push_action(format!("Started {current_action}"));
    }

    fn record_finished(&mut self, note: Option<String>) {
        self.snapshot.completed_steps += 1;
        self.snapshot.current_action = None;
        if let Some(note) = note {
            self.push_action(note);
        }
    }

    fn push_action(&mut self, action: String) {
        self.snapshot.recent_actions.push(action);
        if self.snapshot.recent_actions.len() > MAX_RECENT_ACTIONS {
            let overflow = self.snapshot.recent_actions.len() - MAX_RECENT_ACTIONS;
            self.snapshot.recent_actions.drain(0..overflow);
        }
    }
}

fn snapshot_from_progress(
    progress_state: &Arc<StdMutex<SubagentProgressState>>,
) -> SubagentProgressSnapshot {
    progress_state
        .lock()
        .map(|state| state.snapshot.clone())
        .unwrap_or_default()
}

fn handle_helper_agent_event(
    _run_id: &str,
    _subtask_id: &str,
    _helper_kind: &str,
    _started_at: &str,
    _event_tx: &tokio::sync::mpsc::UnboundedSender<ThreadStreamEvent>,
    _progress_state: &Arc<StdMutex<SubagentProgressState>>,
    _context_window: &str,
    _model_display_name: &str,
    _event: &AgentEvent,
) {
}

fn emit_subagent_progress<F>(
    event_tx: &tokio::sync::mpsc::UnboundedSender<ThreadStreamEvent>,
    run_id: &str,
    subtask_id: &str,
    helper_kind: &str,
    started_at: &str,
    activity: SubagentActivityStatus,
    progress_state: &Arc<StdMutex<SubagentProgressState>>,
    message: String,
    update: F,
) where
    F: FnOnce(&mut SubagentProgressState),
{
    let snapshot = if let Ok(mut progress) = progress_state.lock() {
        update(&mut progress);
        progress.snapshot.clone()
    } else {
        SubagentProgressSnapshot::default()
    };

    let _ = event_tx.send(ThreadStreamEvent::SubagentProgress {
        run_id: run_id.to_string(),
        subtask_id: subtask_id.to_string(),
        helper_kind: helper_kind.to_string(),
        started_at: started_at.to_string(),
        activity,
        message,
        snapshot,
    });
}

fn describe_subagent_action(
    tool_name: &str,
    input: &serde_json::Value,
) -> SubagentActionDescriptor {
    match tool_name {
        "read" => {
            let path = input
                .get("path")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("workspace file");
            SubagentActionDescriptor {
                current_action: format!("reading {path}"),
                started_message: format!("Reading {path}"),
                succeeded_message: format!("Finished reading {path}"),
                failed_message: format!("Failed reading {path}"),
            }
        }
        "list" => {
            let path = input
                .get("path")
                .and_then(serde_json::Value::as_str)
                .unwrap_or(".");
            SubagentActionDescriptor {
                current_action: format!("listing {path}"),
                started_message: format!("Listing {path}"),
                succeeded_message: format!("Finished listing {path}"),
                failed_message: format!("Failed listing {path}"),
            }
        }
        "search" => {
            let query = input
                .get("query")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("query");
            let directory = input
                .get("directory")
                .and_then(serde_json::Value::as_str)
                .filter(|value| !value.trim().is_empty())
                .unwrap_or("workspace");
            SubagentActionDescriptor {
                current_action: format!("searching {directory} for \"{query}\""),
                started_message: format!("Searching {directory} for \"{query}\""),
                succeeded_message: format!("Finished searching {directory} for \"{query}\""),
                failed_message: format!("Failed searching {directory} for \"{query}\""),
            }
        }
        "find" => {
            let pattern = input
                .get("pattern")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("*");
            SubagentActionDescriptor {
                current_action: format!("finding files matching \"{pattern}\""),
                started_message: format!("Finding files matching \"{pattern}\""),
                succeeded_message: format!("Finished finding files matching \"{pattern}\""),
                failed_message: format!("Failed finding files matching \"{pattern}\""),
            }
        }
        "git_status" => {
            let path = input
                .get("path")
                .and_then(serde_json::Value::as_str)
                .filter(|value| !value.trim().is_empty())
                .unwrap_or("repository");
            SubagentActionDescriptor {
                current_action: format!("checking git status for {path}"),
                started_message: format!("Checking git status for {path}"),
                succeeded_message: format!("Finished checking git status for {path}"),
                failed_message: format!("Failed checking git status for {path}"),
            }
        }
        "git_diff" => {
            let path = input
                .get("path")
                .and_then(serde_json::Value::as_str)
                .filter(|value| !value.trim().is_empty())
                .unwrap_or("current changes");
            SubagentActionDescriptor {
                current_action: format!("reading git diff for {path}"),
                started_message: format!("Reading git diff for {path}"),
                succeeded_message: format!("Finished reading git diff for {path}"),
                failed_message: format!("Failed reading git diff for {path}"),
            }
        }
        "git_log" => {
            let path = input
                .get("path")
                .and_then(serde_json::Value::as_str)
                .filter(|value| !value.trim().is_empty())
                .unwrap_or("repository");
            SubagentActionDescriptor {
                current_action: format!("reading git history for {path}"),
                started_message: format!("Reading git history for {path}"),
                succeeded_message: format!("Finished reading git history for {path}"),
                failed_message: format!("Failed reading git history for {path}"),
            }
        }
        "term_status" => SubagentActionDescriptor {
            current_action: "checking the thread Terminal panel status".to_string(),
            started_message: "Inspecting the thread Terminal panel status".to_string(),
            succeeded_message: "Captured the thread Terminal panel status".to_string(),
            failed_message: "Failed to inspect the thread Terminal panel status".to_string(),
        },
        "term_output" => SubagentActionDescriptor {
            current_action: "reading recent thread Terminal panel output".to_string(),
            started_message: "Reading recent thread Terminal panel output".to_string(),
            succeeded_message: "Captured recent thread Terminal panel output".to_string(),
            failed_message: "Failed reading recent thread Terminal panel output".to_string(),
        },
        "shell" => {
            let command = input
                .get("command")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("command");
            let short_command = if command.len() > 60 {
                format!("{}…", &command[..57])
            } else {
                command.to_string()
            };
            SubagentActionDescriptor {
                current_action: format!("running `{short_command}`"),
                started_message: format!("Running `{short_command}`"),
                succeeded_message: format!("Finished running `{short_command}`"),
                failed_message: format!("Failed running `{short_command}`"),
            }
        }
        _ => SubagentActionDescriptor {
            current_action: format!("running {tool_name}"),
            started_message: format!("Running {tool_name}"),
            succeeded_message: format!("Finished {tool_name}"),
            failed_message: format!("Failed {tool_name}"),
        },
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

fn extract_usage(messages: &[AgentMessage]) -> Option<Usage> {
    let mut usage = Usage::default();

    for message in messages {
        if let AgentMessage::Assistant(message) = message {
            usage.add(&message.usage);
        }
    }

    has_usage(&usage).then_some(usage)
}

fn has_usage(usage: &Usage) -> bool {
    usage.input > 0
        || usage.output > 0
        || usage.cache_read > 0
        || usage.cache_write > 0
        || usage.total_tokens > 0
}

const HELPER_INHERITED_SECTION_TITLES: &[&str] = &[
    "Project Context (workspace instructions)",
    "Profile Instructions",
    "System Environment",
    "Sandbox & Permissions",
    "Runtime Context",
];

fn is_helper_inherited_section(title: &str) -> bool {
    let normalized = title.trim();
    HELPER_INHERITED_SECTION_TITLES
        .iter()
        .any(|allowed| normalized == *allowed || normalized.starts_with(&format!("{allowed} ")))
}

fn build_helper_system_prompt(
    parent_system_prompt: &str,
    helper_profile: SubagentProfile,
) -> String {
    let inherited_prompt = inherited_helper_prompt_sections(parent_system_prompt);
    let helper_shell_tooling_guide = helper_shell_tooling_guide(helper_profile);
    let output_tail = match helper_profile {
        SubagentProfile::Explore => {
            "Your output will be consumed by the parent agent, not the user. \
Follow any response language and response style instructions inherited above unless the parent explicitly overrides them. \
If the inherited prompt specifies a response language, write your entire output in that language. \
Produce a concise, structured summary. Lead with the key conclusion, then supporting details. \
Reference specific file paths and code locations where relevant. Skip preamble."
        }
        SubagentProfile::Review => {
            "Your output will be consumed by the parent agent, not the user. \
Follow any response language instructions inherited above unless the parent explicitly overrides them. \
If the inherited prompt specifies a response language, use that language in all natural-language JSON fields. \
Follow the review helper's JSON contract exactly. Do not add markdown fences, headings, or prose outside the JSON object."
        }
    };

    if inherited_prompt.trim().is_empty() {
        format!(
            "{}\n\n{}\n\n{}",
            helper_shell_tooling_guide,
            helper_profile.system_prompt(),
            output_tail
        )
    } else {
        format!(
            "{}\n\n{}\n\n{}\n\n{}",
            inherited_prompt,
            helper_shell_tooling_guide,
            helper_profile.system_prompt(),
            output_tail
        )
    }
}

fn helper_shell_tooling_guide(helper_profile: SubagentProfile) -> &'static str {
    match helper_profile {
        SubagentProfile::Explore => {
            "## Shell Tooling Guide\n- This helper does not have `shell`, `edit`, or Terminal panel control tools.\n- Use the workspace-aware tools you actually have: `read`, `list`, `find`, and `search`.\n- Prefer `find` to locate likely files, `search` to locate relevant text or symbols, and `read` to inspect exact implementation details.\n- `search` defaults to literal matching. Set `queryMode` to `regex` only when you intentionally need regular expressions."
        }
        SubagentProfile::Review => {
            "## Shell Tooling Guide\n- This helper may use `read`, `list`, `find`, `search`, `term_status`, `term_output`, and `shell`.\n- Use `shell` only for non-interactive diagnostic and verification commands in the workspace, such as type-checks, test suites, diffs, or other read-only inspection.\n- `term_status` and `term_output` refer only to the desktop app's embedded Terminal panel for the current thread.\n- This helper does not have `edit`, `term_write`, `term_restart`, or `term_close`."
        }
    }
}

fn inherited_helper_prompt_sections(parent_system_prompt: &str) -> String {
    collect_prompt_sections(parent_system_prompt)
        .into_iter()
        .filter(|(title, _)| is_helper_inherited_section(title))
        .map(|(_, body)| body)
        .collect::<Vec<_>>()
        .join("\n\n")
}

fn collect_prompt_sections(prompt: &str) -> Vec<(&str, String)> {
    let mut sections = Vec::new();
    let mut current_title: Option<&str> = None;
    let mut current_lines: Vec<&str> = Vec::new();

    for line in prompt.lines() {
        if let Some(title) = line.strip_prefix("## ") {
            if let Some(previous_title) = current_title.take() {
                sections.push((previous_title, current_lines.join("\n").trim().to_string()));
            }
            current_title = Some(title.trim());
            current_lines = vec![line];
        } else if current_title.is_some() {
            current_lines.push(line);
        }
    }

    if let Some(previous_title) = current_title {
        sections.push((previous_title, current_lines.join("\n").trim().to_string()));
    }

    sections
        .into_iter()
        .filter(|(_, body)| !body.trim().is_empty())
        .collect()
}

fn take_escalation_summary(summary: &Arc<StdMutex<Option<String>>>) -> Option<String> {
    summary.lock().ok().and_then(|mut slot| slot.take())
}

/// Maximum size for a single tool result sent to the LLM (8 MB).
const MAX_TOOL_RESULT_SIZE: usize = 8_000_000;

fn helper_agent_tool_result_from_output(output: ToolOutput) -> AgentToolResult {
    let mut rendered =
        serde_json::to_string(&output.result).unwrap_or_else(|_| output.result.to_string());

    if rendered.len() > MAX_TOOL_RESULT_SIZE {
        rendered.truncate(MAX_TOOL_RESULT_SIZE);
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

fn helper_agent_error_result(message: impl Into<String>) -> AgentToolResult {
    AgentToolResult {
        content: vec![ContentBlock::Text(TextContent::new(format!(
            "Error: {}",
            message.into()
        )))],
        details: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::subagent::SubagentProfile;
    use std::sync::Arc;

    #[test]
    fn helper_system_prompt_preserves_parent_language_instruction() {
        let prompt = build_helper_system_prompt(
            "## Profile Instructions\nRespond in 简体中文 unless the user explicitly asks for a different language.",
            SubagentProfile::Explore,
        );

        assert!(prompt.contains("Respond in 简体中文"));
        assert!(prompt.contains(
            "Follow any response language and response style instructions inherited above"
        ));
        assert!(prompt.contains("write your entire output in that language"));
    }

    #[test]
    fn helper_system_prompt_inherits_only_allowed_sections() {
        let parent_prompt = "## Role\nYou are TiyCode.\n\n## Project Context (workspace instructions)\nFollow AGENTS.md.\n\n## Behavioral Guidelines\nUse clarify when needed.\n\n## Profile Instructions\nRespond in 简体中文 unless the user explicitly asks for a different language.\n\n## Sandbox & Permissions\n- Approval policy: auto.\n\n## Shell Tooling Guide\n- Generic shell guidance.\n\n## Final Response Structure\nUse structured markdown.";

        let prompt = build_helper_system_prompt(parent_prompt, SubagentProfile::Explore);

        assert!(prompt.contains("## Project Context (workspace instructions)"));
        assert!(prompt.contains("## Profile Instructions"));
        assert!(prompt.contains("## Sandbox & Permissions"));
        assert!(prompt.contains("## Shell Tooling Guide"));
        assert!(!prompt.contains("## Role\nYou are TiyCode."));
        assert!(!prompt.contains("## Behavioral Guidelines"));
        assert!(!prompt.contains("## Final Response Structure"));
        assert!(!prompt.contains("Generic shell guidance."));
    }

    #[test]
    fn helper_system_prompt_preserves_environment_and_runtime_context_sections() {
        let parent_prompt = "## System Environment\n- Operating system: macos\n\n## Runtime Context\nCurrent date: 2026-04-04\nWorkspace path: /tmp/project\n\n## Run Mode\nDefault execution mode is active.";

        let inherited = inherited_helper_prompt_sections(parent_prompt);

        assert!(inherited.contains("## System Environment"));
        assert!(inherited.contains("## Runtime Context"));
        assert!(!inherited.contains("## Run Mode"));
    }

    #[test]
    fn explore_helper_shell_guide_only_mentions_read_only_tools() {
        let prompt = build_helper_system_prompt("", SubagentProfile::Explore);

        assert!(prompt.contains(
            "This helper does not have `shell`, `edit`, or Terminal panel control tools."
        ));
        assert!(prompt.contains("`read`, `list`, `find`, and `search`"));
        assert!(prompt.contains("`search` defaults to literal matching."));
        assert!(!prompt.contains("`term_write`"));
        assert!(!prompt.contains("`term_restart`"));
        assert!(!prompt.contains("`term_close`"));
    }

    #[test]
    fn review_helper_shell_guide_matches_review_tool_whitelist() {
        let prompt = build_helper_system_prompt("", SubagentProfile::Review);

        assert!(prompt.contains("`term_status`, `term_output`, and `shell`"));
        assert!(
            prompt.contains("does not have `edit`, `term_write`, `term_restart`, or `term_close`")
        );
        assert!(!prompt.contains("This helper may use `term_write`"));
    }

    #[test]
    fn helper_inherited_sections_preserve_parent_order() {
        let parent_prompt = "## Runtime Context\nCurrent date: 2026-04-04\n\n## Project Context (workspace instructions)\nFollow AGENTS.md.\n\n## Profile Instructions\nRespond in 简体中文 unless the user explicitly asks for a different language.\n\n## Final Response Structure\nUse structured markdown.";

        let inherited = inherited_helper_prompt_sections(parent_prompt);
        let runtime_index = inherited.find("## Runtime Context").unwrap();
        let project_index = inherited
            .find("## Project Context (workspace instructions)")
            .unwrap();
        let profile_index = inherited.find("## Profile Instructions").unwrap();

        assert!(runtime_index < project_index);
        assert!(project_index < profile_index);
        assert!(!inherited.contains("## Final Response Structure"));
    }

    #[test]
    fn collect_prompt_sections_keeps_section_boundaries() {
        let sections =
            collect_prompt_sections("## One\nalpha\n\n## Two\nbeta\nline two\n\n## Three\ngamma");

        assert_eq!(sections.len(), 3);
        assert_eq!(sections[0].0, "One");
        assert_eq!(sections[1].0, "Two");
        assert!(sections[1].1.contains("beta\nline two"));
        assert_eq!(sections[2].0, "Three");
    }

    #[test]
    fn finalize_helper_summary_renders_review_json() {
        let summary = finalize_helper_summary(
            SubagentProfile::Review,
            r#"{"verdict":"pass","directFindings":[],"globalFindings":[],"verification":[],"coverage":{"diffReviewed":true,"globalScanPerformed":false,"changedFilesReviewed":[],"scannedPaths":[],"unscannedPaths":[],"limitations":[]},"followUp":[]}"#,
        );

        assert!(summary.contains("Verdict: PASS"));
        assert!(summary.contains("Direct Diff Findings"));
    }

    #[test]
    fn describe_subagent_action_supports_git_tools_with_and_without_paths() {
        let status = describe_subagent_action("git_status", &serde_json::json!({}));
        assert_eq!(status.current_action, "checking git status for repository");

        let diff =
            describe_subagent_action("git_diff", &serde_json::json!({ "path": "src/lib.rs" }));
        assert_eq!(diff.current_action, "reading git diff for src/lib.rs");

        let log = describe_subagent_action("git_log", &serde_json::json!({ "path": "" }));
        assert_eq!(log.current_action, "reading git history for repository");
    }

    #[test]
    fn subagent_progress_state_tracks_counts_and_recent_actions() {
        let mut state = SubagentProgressState::default();

        state.record_started("read", "reading src/lib.rs");
        assert_eq!(state.snapshot.total_tool_calls, 1);
        assert_eq!(
            state.snapshot.current_action.as_deref(),
            Some("reading src/lib.rs")
        );
        assert_eq!(state.snapshot.tool_counts.get("read"), Some(&1));

        state.record_started("read", "reading src/main.rs");
        state.record_started("search", "searching code");
        state.record_finished(Some("Search complete".to_string()));
        assert_eq!(state.snapshot.completed_steps, 1);
        assert_eq!(state.snapshot.current_action, None);
        assert_eq!(state.snapshot.tool_counts.get("read"), Some(&2));
        assert_eq!(state.snapshot.tool_counts.get("search"), Some(&1));

        for index in 0..10 {
            state.push_action(format!("action {index}"));
        }
        assert_eq!(
            state.snapshot.recent_actions.len(),
            super::MAX_RECENT_ACTIONS
        );
        assert_eq!(
            state.snapshot.recent_actions.first().map(String::as_str),
            Some("action 5")
        );
        assert_eq!(
            state.snapshot.recent_actions.last().map(String::as_str),
            Some("action 9")
        );
    }

    #[test]
    fn snapshot_from_progress_clones_state_and_defaults_on_poison() {
        let progress = Arc::new(std::sync::Mutex::new(SubagentProgressState::default()));
        {
            let mut state = progress.lock().unwrap();
            state.record_started("list", "listing workspace");
        }

        let snapshot = snapshot_from_progress(&progress);
        assert_eq!(snapshot.total_tool_calls, 1);
        assert_eq!(
            snapshot.current_action.as_deref(),
            Some("listing workspace")
        );
    }

    #[test]
    fn helper_inherited_section_accepts_exact_and_suffixed_titles() {
        assert!(is_helper_inherited_section(
            "Project Context (workspace instructions)"
        ));
        assert!(is_helper_inherited_section("Runtime Context (workspace)"));
        assert!(is_helper_inherited_section("  Profile Instructions  "));
        assert!(!is_helper_inherited_section("Behavioral Guidelines"));
    }

    #[test]
    fn merge_payload_recursively_merges_json() {
        let base = serde_json::json!({
            "model": "demo",
            "options": { "temperature": 0.1, "nested": { "a": true } },
            "replace": [1]
        });
        let patch = serde_json::json!({
            "options": { "top_p": 0.8, "nested": { "b": false } },
            "replace": "done"
        });

        let merged = merge_payload(base, &patch);
        assert_eq!(merged["model"], "demo");
        assert_eq!(merged["options"]["temperature"], 0.1);
        assert_eq!(merged["options"]["top_p"], 0.8);
        assert_eq!(
            merged["options"]["nested"],
            serde_json::json!({ "a": true, "b": false })
        );
        assert_eq!(merged["replace"], "done");
    }

    #[test]
    fn take_escalation_summary_consumes_value_once() {
        let summary = Arc::new(std::sync::Mutex::new(Some("Need parent help".to_string())));
        assert_eq!(
            take_escalation_summary(&summary).as_deref(),
            Some("Need parent help")
        );
        assert_eq!(take_escalation_summary(&summary), None);
    }

    #[test]
    fn helper_agent_tool_result_from_output_wraps_success_and_error() {
        let success = helper_agent_tool_result_from_output(ToolOutput {
            success: true,
            result: serde_json::json!({ "ok": true }),
        });
        assert_eq!(success.details, Some(serde_json::json!({ "ok": true })));
        assert!(success.content[0]
            .as_text()
            .unwrap()
            .text
            .contains("\"ok\":true"));

        let failure = helper_agent_tool_result_from_output(ToolOutput {
            success: false,
            result: serde_json::json!({ "message": "denied" }),
        });
        assert_eq!(
            failure.details,
            Some(serde_json::json!({ "message": "denied" }))
        );
        assert!(failure.content[0]
            .as_text()
            .unwrap()
            .text
            .starts_with("Error: "));

        let direct_error = helper_agent_error_result("boom");
        assert_eq!(direct_error.details, None);
        assert_eq!(
            direct_error.content[0].as_text().unwrap().text,
            "Error: boom"
        );
    }
}
