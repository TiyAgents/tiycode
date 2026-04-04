use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;
use std::sync::Mutex as StdMutex;

use serde::Serialize;
use sqlx::SqlitePool;
use tiy_core::agent::{Agent, AgentEvent, AgentMessage, AgentToolResult, ToolExecutionMode};
use tiy_core::types::{ContentBlock, TextContent, Usage};
use tokio::sync::Mutex;

use crate::core::agent_session::ResolvedModelRole;
use crate::core::executors::ToolOutput;
use crate::core::subagent::runtime_orchestration::{RuntimeOrchestrationTool, SubagentProfile};
use crate::core::tool_gateway::{
    ToolExecutionOptions, ToolExecutionRequest, ToolGateway, ToolGatewayResult,
};
use crate::ipc::frontend_channels::ThreadStreamEvent;
use crate::model::errors::{AppError, ErrorSource};
use crate::model::thread::RunUsageDto;
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
}

pub struct HelperRunResult {
    pub summary: String,
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
    pub usage: RunUsageDto,
}

pub struct HelperAgentOrchestrator {
    pool: SqlitePool,
    tool_gateway: Arc<ToolGateway>,
    active_helpers: Arc<Mutex<HashMap<String, Vec<Arc<Agent>>>>>,
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
        let last_usage = Arc::new(StdMutex::new(None::<Usage>));

        let agent = Arc::new(Agent::with_model(request.model_role.model.clone()));
        agent.set_max_turns(crate::desktop_agent_max_turns!());
        agent.set_system_prompt(build_helper_system_prompt(
            &request.system_prompt,
            helper_profile,
        ));
        agent.set_tools(helper_profile.helper_tools());
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
        let helper_kind = resolved_helper_kind.clone();
        let helper_id_for_tools = helper_id.clone();
        let helper_id_for_events = helper_id.clone();
        let helper_started_at_for_events = helper_started_at.clone();
        let helper_agent = Arc::clone(&agent);
        let escalation_summary_ref = Arc::clone(&escalation_summary);
        let progress_state_ref = Arc::clone(&progress_state);
        let progress_event_tx = request.event_tx.clone();
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
            let helper_started_at_for_events = helper_started_at_for_events.clone();

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
            helpers
                .entry(request.run_id.clone())
                .or_default()
                .push(Arc::clone(&agent));
        }

        let helper_run_id_for_usage = request.run_id.clone();
        let helper_id_for_usage = helper_id.clone();
        let helper_kind_for_usage = resolved_helper_kind.clone();
        let helper_started_at_for_usage = helper_started_at.clone();
        let helper_event_tx_for_usage = request.event_tx.clone();
        let progress_state_for_usage = Arc::clone(&progress_state);
        let last_usage_ref = Arc::clone(&last_usage);
        let unsubscribe = agent.subscribe(move |event| {
            handle_helper_agent_event(
                &helper_run_id_for_usage,
                &helper_id_for_usage,
                &helper_kind_for_usage,
                &helper_started_at_for_usage,
                &helper_event_tx_for_usage,
                &progress_state_for_usage,
                &last_usage_ref,
                &helper_context_window,
                &helper_model_display_name,
                event,
            );
        });

        let result = agent.prompt(request.task.clone()).await;
        unsubscribe();
        self.remove_helper(&request.run_id, &agent).await;

        if let Some(summary) = take_escalation_summary(&escalation_summary) {
            let snapshot = snapshot_from_progress(&progress_state);
            run_helper_repo::mark_completed(&self.pool, &helper_id, &summary, &Usage::default())
                .await?;

            let _ = request.event_tx.send(ThreadStreamEvent::SubagentCompleted {
                run_id: request.run_id,
                subtask_id: helper_id,
                helper_kind: resolved_helper_kind.clone(),
                started_at: helper_started_at.clone(),
                summary: Some(summary.clone()),
                snapshot: snapshot.clone(),
            });

            return Ok(HelperRunResult { summary, snapshot });
        }

        match result {
            Ok(messages) => {
                let summary = extract_summary(&messages)
                    .unwrap_or_else(|| "Helper completed without a textual summary.".to_string());
                let usage = extract_usage(&messages).unwrap_or_default();
                if let Ok(mut progress) = progress_state.lock() {
                    progress.record_usage(&usage);
                }
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

                Ok(HelperRunResult { summary, snapshot })
            }
            Err(error) => {
                let interrupted = error.to_string().to_lowercase().contains("aborted");
                let snapshot = snapshot_from_progress(&progress_state);
                run_helper_repo::mark_failed(
                    &self.pool,
                    &helper_id,
                    &error.to_string(),
                    interrupted,
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

    fn record_usage(&mut self, usage: &Usage) {
        self.snapshot.usage = RunUsageDto::from(*usage);
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
    run_id: &str,
    subtask_id: &str,
    helper_kind: &str,
    started_at: &str,
    event_tx: &tokio::sync::mpsc::UnboundedSender<ThreadStreamEvent>,
    progress_state: &Arc<StdMutex<SubagentProgressState>>,
    last_usage: &StdMutex<Option<Usage>>,
    context_window: &str,
    model_display_name: &str,
    event: &AgentEvent,
) {
    match event {
        AgentEvent::MessageUpdate {
            assistant_event, ..
        } => {
            if let Some(partial) = assistant_event.partial_message() {
                emit_subagent_usage_update_if_changed(
                    run_id,
                    subtask_id,
                    helper_kind,
                    started_at,
                    event_tx,
                    progress_state,
                    last_usage,
                    &partial.usage,
                    context_window,
                    model_display_name,
                );
            }
        }
        AgentEvent::MessageEnd { message } => {
            if let AgentMessage::Assistant(assistant) = message {
                emit_subagent_usage_update_if_changed(
                    run_id,
                    subtask_id,
                    helper_kind,
                    started_at,
                    event_tx,
                    progress_state,
                    last_usage,
                    &assistant.usage,
                    context_window,
                    model_display_name,
                );
            }
        }
        _ => {}
    }
}

#[allow(clippy::too_many_arguments)]
fn emit_subagent_usage_update_if_changed(
    run_id: &str,
    subtask_id: &str,
    helper_kind: &str,
    started_at: &str,
    event_tx: &tokio::sync::mpsc::UnboundedSender<ThreadStreamEvent>,
    progress_state: &Arc<StdMutex<SubagentProgressState>>,
    last_usage: &StdMutex<Option<Usage>>,
    usage: &Usage,
    _context_window: &str,
    _model_display_name: &str,
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

    let snapshot = if let Ok(mut progress) = progress_state.lock() {
        progress.record_usage(usage);
        progress.snapshot.clone()
    } else {
        SubagentProgressSnapshot {
            usage: RunUsageDto::from(*usage),
            ..SubagentProgressSnapshot::default()
        }
    };

    let _ = event_tx.send(ThreadStreamEvent::SubagentUsageUpdated {
        run_id: run_id.to_string(),
        subtask_id: subtask_id.to_string(),
        helper_kind: helper_kind.to_string(),
        started_at: started_at.to_string(),
        snapshot,
    });
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
    messages.iter().rev().find_map(|message| match message {
        AgentMessage::Assistant(message) => Some(message.usage),
        _ => None,
    })
}

const HELPER_INHERITED_SECTION_TITLES: &[&str] = &[
    "Project Context (workspace instructions)",
    "Profile Instructions",
    "System Environment",
    "Sandbox & Permissions",
    "Shell Tooling Guide",
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

    if inherited_prompt.trim().is_empty() {
        format!(
            "{}\n\nYour output will be consumed by the parent agent, not the user. \
Follow any response language and response style instructions inherited above unless the parent explicitly overrides them. \
If the inherited prompt specifies a response language, write your entire output in that language. \
Produce a concise, structured summary. Lead with the key conclusion, then supporting details. \
Reference specific file paths and code locations where relevant. Skip preamble.",
            helper_profile.system_prompt()
        )
    } else {
        format!(
            "{}\n\n{}\n\nYour output will be consumed by the parent agent, not the user. \
Follow any response language and response style instructions inherited above unless the parent explicitly overrides them. \
If the inherited prompt specifies a response language, write your entire output in that language. \
Produce a concise, structured summary. Lead with the key conclusion, then supporting details. \
Reference specific file paths and code locations where relevant. Skip preamble.",
            inherited_prompt,
            helper_profile.system_prompt()
        )
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
    use super::{
        build_helper_system_prompt, collect_prompt_sections, inherited_helper_prompt_sections,
    };
    use crate::core::subagent::SubagentProfile;

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
        let parent_prompt = "## Role\nYou are Tiy Agent.\n\n## Project Context (workspace instructions)\nFollow AGENTS.md.\n\n## Behavioral Guidelines\nUse clarify when needed.\n\n## Profile Instructions\nRespond in 简体中文 unless the user explicitly asks for a different language.\n\n## Sandbox & Permissions\n- Approval policy: auto.\n\n## Final Response Structure\nUse structured markdown.";

        let prompt = build_helper_system_prompt(parent_prompt, SubagentProfile::Explore);

        assert!(prompt.contains("## Project Context (workspace instructions)"));
        assert!(prompt.contains("## Profile Instructions"));
        assert!(prompt.contains("## Sandbox & Permissions"));
        assert!(!prompt.contains("## Role\nYou are Tiy Agent."));
        assert!(!prompt.contains("## Behavioral Guidelines"));
        assert!(!prompt.contains("## Final Response Structure"));
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
}
