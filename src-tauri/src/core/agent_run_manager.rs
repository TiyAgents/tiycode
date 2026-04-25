//! Manages the lifecycle of agent runs backed by the built-in Rust runtime.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use sqlx::SqlitePool;
use tauri::AppHandle;
use tiycore::agent::AgentMessage;
use tiycore::types::OnPayloadFn;
use tokio::sync::{broadcast, mpsc, Mutex};
use tokio::time::{sleep, Instant};

use crate::core::agent_session::{
    build_session_spec, convert_history_messages, trim_history_to_current_context,
    ResolvedModelRole,
};
use crate::core::built_in_agent_runtime::{BuiltInAgentRuntime, RuntimeSessionFinishState};
use crate::core::plan_checkpoint::{
    ApprovalPromptMetadata, PlanApprovalAction, PlanMessageMetadata,
    IMPLEMENTATION_PLAN_APPROVAL_KIND, IMPLEMENTATION_PLAN_APPROVED_STATE,
    IMPLEMENTATION_PLAN_PENDING_STATE, IMPLEMENTATION_PLAN_SUPERSEDED_STATE,
};
use crate::core::sleep_manager::SleepManager;
use crate::ipc::frontend_channels::ThreadStreamEvent;
use crate::model::errors::{AppError, ErrorSource};
use crate::model::thread::{MessageAttachmentDto, MessageRecord, ThreadStatus};
use crate::persistence::repo::{
    message_repo, run_repo, thread_repo, tool_call_repo, workspace_repo,
};

pub(crate) use crate::core::agent_run_event_handler::build_orphaned_run_terminal_event;
#[cfg(test)]
pub(crate) use crate::core::agent_run_event_handler::{
    is_terminal_runtime_event, should_complete_reasoning_for_event, terminal_event_status,
};
pub(crate) use crate::core::agent_run_summary::*;
pub(crate) use crate::core::agent_run_title::*;

pub(crate) const TITLE_GENERATION_TIMEOUT: Duration = Duration::from_secs(90);
pub(crate) const TITLE_GENERATION_MAX_TOKENS: u32 = 512;
pub(crate) const TITLE_GENERATION_MAX_TOKENS_REASONING: u32 = 2048;
pub(crate) const PRIMARY_SUMMARY_MAX_TOKENS: u32 = 8192;
pub(crate) const PRIMARY_SUMMARY_TIMEOUT: Duration = Duration::from_secs(90);
pub(crate) const TITLE_CONTEXT_MAX_CHARS: usize = 1_200;
/// Lower bound on the history chars we send to the summary model.
///
/// When context_window is very small (or unknown), we still want some room
/// for meaningful input — this floor prevents degenerate cases where the
/// derived budget collapses to zero.
pub(crate) const SUMMARY_HISTORY_MIN_CHARS: usize = 8_000;
const FRONTEND_EVENT_BUFFER_SIZE: usize = 2048;

pub(crate) struct ActiveRun {
    pub(crate) run_id: String,
    pub(crate) thread_id: String,
    pub(crate) profile_id: Option<String>,
    pub(crate) frontend_tx: broadcast::Sender<ThreadStreamEvent>,
    pub(crate) lightweight_model_role: Option<ResolvedModelRole>,
    pub(crate) auxiliary_model_role: Option<ResolvedModelRole>,
    pub(crate) primary_model_role: Option<ResolvedModelRole>,
    pub(crate) streaming_message_id: Option<String>,
    pub(crate) reasoning_message_id: Option<String>,
    pub(crate) cancellation_requested: bool,
}

#[derive(Default)]
struct StartRunOptions {
    history_override: Option<Vec<MessageRecord>>,
    initial_prompt: Option<String>,
    persist_user_message: bool,
}

struct ContextResetMessageBundle {
    history_override: Vec<MessageRecord>,
    persisted_messages: Vec<MessageRecord>,
}

async fn mark_thread_run_cancellation_requested(
    active_runs: &Mutex<HashMap<String, ActiveRun>>,
    thread_id: &str,
) -> Option<String> {
    let mut runs = active_runs.lock().await;
    let run = runs.values_mut().find(|run| run.thread_id == thread_id)?;
    run.cancellation_requested = true;
    Some(run.run_id.clone())
}

pub struct AgentRunManager {
    pub(crate) pool: SqlitePool,
    pub(crate) app_handle: AppHandle,
    pub(crate) runtime: Arc<BuiltInAgentRuntime>,
    pub(crate) sleep_manager: Arc<SleepManager>,
    pub(crate) active_runs: Arc<Mutex<HashMap<String, ActiveRun>>>,
}

impl AgentRunManager {
    pub fn new(
        pool: SqlitePool,
        app_handle: AppHandle,
        runtime: Arc<BuiltInAgentRuntime>,
        sleep_manager: Arc<SleepManager>,
    ) -> Self {
        Self {
            pool,
            app_handle,
            runtime,
            sleep_manager,
            active_runs: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub async fn start_run(
        self: &Arc<Self>,
        thread_id: &str,
        prompt: &str,
        display_prompt: Option<String>,
        prompt_metadata: Option<serde_json::Value>,
        attachments: Vec<MessageAttachmentDto>,
        run_mode: &str,
        profile_id: Option<String>,
        provider_id: Option<String>,
        model_id: Option<String>,
        model_plan: serde_json::Value,
    ) -> Result<(String, broadcast::Receiver<ThreadStreamEvent>), AppError> {
        self.expire_pending_plan_approval(thread_id).await?;
        self.start_run_with_options(
            thread_id,
            prompt,
            display_prompt,
            prompt_metadata,
            attachments,
            run_mode,
            profile_id,
            provider_id,
            model_id,
            model_plan,
            StartRunOptions {
                persist_user_message: true,
                ..StartRunOptions::default()
            },
        )
        .await
    }

    async fn start_run_with_options(
        self: &Arc<Self>,
        thread_id: &str,
        prompt: &str,
        display_prompt: Option<String>,
        prompt_metadata: Option<serde_json::Value>,
        attachments: Vec<MessageAttachmentDto>,
        run_mode: &str,
        profile_id: Option<String>,
        provider_id: Option<String>,
        model_id: Option<String>,
        model_plan: serde_json::Value,
        options: StartRunOptions,
    ) -> Result<(String, broadcast::Receiver<ThreadStreamEvent>), AppError> {
        let thread = thread_repo::find_by_id(&self.pool, thread_id)
            .await?
            .ok_or_else(|| AppError::not_found(ErrorSource::Thread, "thread"))?;

        let workspace_path = workspace_repo::find_by_id(&self.pool, &thread.workspace_id)
            .await?
            .map(|workspace| workspace.canonical_path)
            .unwrap_or_default();

        let (frontend_tx, frontend_rx) =
            broadcast::channel::<ThreadStreamEvent>(FRONTEND_EVENT_BUFFER_SIZE);
        let run_id = uuid::Uuid::now_v7().to_string();

        {
            let mut runs = self.active_runs.lock().await;
            if runs.values().any(|run| run.thread_id == thread_id) {
                return Err(AppError::recoverable(
                    ErrorSource::Thread,
                    "thread.run.already_active",
                    "A run is already active for this thread",
                ));
            }

            runs.insert(
                run_id.clone(),
                ActiveRun {
                    run_id: run_id.clone(),
                    thread_id: thread_id.to_string(),
                    profile_id: profile_id.clone(),
                    frontend_tx: frontend_tx.clone(),
                    lightweight_model_role: None,
                    auxiliary_model_role: None,
                    primary_model_role: None,
                    streaming_message_id: None,
                    reasoning_message_id: None,
                    cancellation_requested: false,
                },
            );
        }
        self.sleep_manager.set_has_active_runs(true).await;

        let start_result = async {
            if options.persist_user_message {
                let user_message = MessageRecord {
                    id: uuid::Uuid::now_v7().to_string(),
                    thread_id: thread_id.to_string(),
                    run_id: None,
                    role: "user".to_string(),
                    content_markdown: display_prompt.unwrap_or_else(|| prompt.to_string()),
                    message_type: "plain_message".to_string(),
                    status: "completed".to_string(),
                    metadata_json: prompt_metadata.map(|value| value.to_string()),
                    attachments_json: serde_json::to_string(&attachments)
                        .ok()
                        .filter(|value| value != "[]"),
                    created_at: String::new(),
                };
                message_repo::insert(&self.pool, &user_message).await?;
            }
            thread_repo::touch_active(&self.pool, thread_id).await?;

            run_repo::insert(
                &self.pool,
                &run_repo::RunInsert {
                    id: run_id.clone(),
                    thread_id: thread_id.to_string(),
                    profile_id,
                    run_mode: run_mode.to_string(),
                    provider_id,
                    model_id,
                    effective_model_plan_json: Some(model_plan.to_string()),
                    status: "created".to_string(),
                },
            )
            .await?;

            let spec = build_session_spec(
                &self.pool,
                &run_id,
                thread_id,
                &workspace_path,
                run_mode,
                &model_plan,
            )
            .await?;
            let mut spec = spec;
            if let Some(history_override) = options.history_override {
                spec.history_messages = history_override;
                // When history is overridden (e.g. context-reset plan approval),
                // the old tool calls are no longer relevant — the override
                // messages have no matching run_ids, so stale tool calls would
                // otherwise be appended to the LLM context as orphaned entries.
                spec.history_tool_calls = Vec::new();
            }
            spec.initial_prompt = options.initial_prompt;

            {
                let mut runs = self.active_runs.lock().await;
                if let Some(run) = runs.get_mut(&run_id) {
                    run.lightweight_model_role = spec.model_plan.lightweight.clone();
                    run.auxiliary_model_role = spec.model_plan.auxiliary.clone();
                    run.primary_model_role = Some(spec.model_plan.primary.clone());
                }
            }

            let (runtime_tx, runtime_rx) = mpsc::unbounded_channel::<ThreadStreamEvent>();
            let runtime_finish_rx = self.runtime.start_session(spec, runtime_tx).await?;
            self.spawn_runtime_event_loop(run_id.clone(), runtime_rx);
            self.spawn_runtime_finish_watchdog(run_id.clone(), runtime_finish_rx);

            Ok::<(), AppError>(())
        }
        .await;

        if let Err(error) = start_result {
            self.remove_active_run(&run_id).await;
            return Err(error);
        }

        Ok((run_id, frontend_rx))
    }

    pub async fn execute_approved_plan(
        self: &Arc<Self>,
        thread_id: &str,
        approval_message_id: &str,
        action: PlanApprovalAction,
    ) -> Result<(String, broadcast::Receiver<ThreadStreamEvent>), AppError> {
        let (approval_message, approval_metadata) = self
            .load_latest_pending_plan_approval(thread_id, Some(approval_message_id))
            .await?;

        let plan_message = message_repo::find_by_id(&self.pool, &approval_metadata.plan_message_id)
            .await?
            .ok_or_else(|| {
                AppError::recoverable(
                    ErrorSource::Thread,
                    "thread.plan_approval.plan_missing",
                    "The approved plan message could not be found.",
                )
            })?;
        let mut plan_metadata = parse_message_metadata::<PlanMessageMetadata>(&plan_message)?;
        let planning_run_id = approval_message.run_id.clone().ok_or_else(|| {
            AppError::recoverable(
                ErrorSource::Thread,
                "thread.plan_approval.run_missing",
                "The planning run for this approval is missing.",
            )
        })?;
        let model_plan_json =
            run_repo::find_effective_model_plan_json(&self.pool, &planning_run_id)
                .await?
                .ok_or_else(|| {
                    AppError::recoverable(
                        ErrorSource::Thread,
                        "thread.plan_approval.model_plan_missing",
                        "The approved plan is missing its runtime model plan.",
                    )
                })?;
        let model_plan_value = serde_json::from_str::<serde_json::Value>(&model_plan_json)
            .map_err(|error| {
                AppError::recoverable(
                    ErrorSource::Thread,
                    "thread.plan_approval.model_plan_invalid",
                    format!("Failed to parse runtime model plan: {error}"),
                )
            })?;
        let (profile_id, provider_id, model_id) = extract_run_model_refs(&model_plan_value);
        let mut approval_metadata = approval_metadata;
        approval_metadata.state = IMPLEMENTATION_PLAN_APPROVED_STATE.to_string();
        approval_metadata.approved_action = Some(action.clone());

        plan_metadata.approval_state = IMPLEMENTATION_PLAN_APPROVED_STATE.to_string();

        let implementation_prompt =
            build_implementation_handoff_prompt(thread_id, &plan_metadata, action.clone());
        let (history_override, context_seed_messages) = match action {
            PlanApprovalAction::ApplyPlan => (None, None),
            PlanApprovalAction::ApplyPlanWithContextReset => {
                let message_bundle = self
                    .build_context_reset_message_bundle(thread_id, &plan_metadata)
                    .await?;
                (
                    Some(message_bundle.history_override),
                    Some(message_bundle.persisted_messages),
                )
            }
        };

        let result = self
            .start_run_with_options(
                thread_id,
                "",
                None,
                None,
                Vec::new(),
                "default",
                profile_id,
                provider_id,
                model_id,
                model_plan_value,
                StartRunOptions {
                    history_override,
                    initial_prompt: Some(implementation_prompt),
                    persist_user_message: false,
                },
            )
            .await?;

        if let Some(seed_messages) = context_seed_messages.as_ref() {
            self.persist_messages(seed_messages).await?;
        }

        message_repo::update_metadata(
            &self.pool,
            &approval_message.id,
            serde_json::to_string(&approval_metadata).ok().as_deref(),
        )
        .await?;

        message_repo::update_metadata(
            &self.pool,
            &plan_message.id,
            serde_json::to_string(&plan_metadata).ok().as_deref(),
        )
        .await?;

        // Terminate the planning run that was parked in waiting_approval so it
        // does not linger as a zombie with no finished_at timestamp.
        run_repo::update_status(&self.pool, &planning_run_id, "completed").await?;

        Ok(result)
    }

    pub async fn clear_thread_context(&self, thread_id: &str) -> Result<(), AppError> {
        if self.cancel_run_if_active(thread_id).await? {
            tracing::info!(thread_id = %thread_id, "Cancelled active run before clearing context");
        }

        let thread = thread_repo::find_by_id(&self.pool, thread_id)
            .await?
            .ok_or_else(|| AppError::not_found(ErrorSource::Thread, "thread"))?;

        let command_metadata = serde_json::json!({
            "composer": {
                "kind": "command",
                "displayText": "/clear",
                "effectivePrompt": "Clear conversation history and free up context.",
                "command": {
                    "source": "builtin",
                    "name": "clear",
                    "path": "/clear",
                    "description": "Clear conversation history and free up context.",
                    "argumentHint": "(no arguments)",
                    "argumentsText": "",
                    "prompt": "Clear conversation history and free up context.",
                    "behavior": "clear"
                }
            }
        });
        let reset_metadata = serde_json::json!({
            "kind": "context_reset",
            "source": "clear",
            "label": "Context is now reset",
        });

        let messages = vec![
            MessageRecord {
                id: uuid::Uuid::now_v7().to_string(),
                thread_id: thread_id.to_string(),
                run_id: None,
                role: "user".to_string(),
                content_markdown: "/clear".to_string(),
                message_type: "plain_message".to_string(),
                status: "completed".to_string(),
                metadata_json: Some(command_metadata.to_string()),
                attachments_json: None,
                created_at: String::new(),
            },
            MessageRecord {
                id: uuid::Uuid::now_v7().to_string(),
                thread_id: thread_id.to_string(),
                run_id: None,
                role: "system".to_string(),
                content_markdown: "Context is now reset".to_string(),
                message_type: "summary_marker".to_string(),
                status: "completed".to_string(),
                metadata_json: Some(reset_metadata.to_string()),
                attachments_json: None,
                created_at: String::new(),
            },
        ];
        self.persist_messages(&messages).await?;
        thread_repo::touch_active(&self.pool, thread_id).await?;
        thread_repo::update_status(&self.pool, &thread.id, &ThreadStatus::Idle).await?;
        Ok(())
    }

    /// Run a manual `/compact` against the given thread.
    ///
    /// Unlike its previous synchronous form, this method now integrates with
    /// the standard run lifecycle so the frontend sees a "thinking" placeholder
    /// and the thread is flagged Running during the potentially long LLM call:
    ///
    /// 1. The user `/compact` message is persisted up front (no optimistic
    ///    loss on page reload before the summary finishes).
    /// 2. An `ActiveRun` is registered with a dedicated broadcast channel so
    ///    the frontend can subscribe via `thread_subscribe_run` if it misses
    ///    the initial receiver.
    /// 3. `RunStarted` + `ContextCompressing` events are emitted immediately
    ///    (driving the thinking placeholder and the "Compressing context…"
    ///    label on the frontend).
    /// 4. The LLM call + marker persistence runs in a spawned task so the
    ///    Tauri command returns right away, giving the UI a responsive feel.
    /// 5. On completion (success or failure), `RunCompleted` / `RunFailed` is
    ///    emitted and the active run is torn down, returning the thread to
    ///    Idle.
    ///
    /// Returns `(run_id, event_rx)` so the caller can forward events over a
    /// Tauri `Channel` identical to `start_run`.
    pub async fn compact_thread_context(
        self: &Arc<Self>,
        thread_id: &str,
        instructions: Option<String>,
        model_plan_value: serde_json::Value,
    ) -> Result<(String, broadcast::Receiver<ThreadStreamEvent>), AppError> {
        if self.cancel_run_if_active(thread_id).await? {
            tracing::info!(thread_id = %thread_id, "Cancelled active run before compacting context");
        }

        let thread = thread_repo::find_by_id(&self.pool, thread_id)
            .await?
            .ok_or_else(|| AppError::not_found(ErrorSource::Thread, "thread"))?;
        let messages = message_repo::list_recent(&self.pool, thread_id, None, 1024).await?;
        let current_context_messages = trim_history_to_current_context(&messages);
        let compact_run_ids: Vec<String> = current_context_messages
            .iter()
            .filter_map(|m| m.run_id.clone())
            .collect::<std::collections::HashSet<_>>()
            .into_iter()
            .collect();
        let compact_tool_calls =
            tool_call_repo::list_parent_visible_by_run_ids(&self.pool, &compact_run_ids).await?;
        let workspace_path = workspace_repo::find_by_id(&self.pool, &thread.workspace_id)
            .await?
            .map(|workspace| workspace.canonical_path)
            .unwrap_or_default();
        let preview_spec = build_session_spec(
            &self.pool,
            "compact-preview",
            thread_id,
            &workspace_path,
            "default",
            &model_plan_value,
        )
        .await?;
        let model = primary_summary_model(&preview_spec.model_plan);
        let response_language = preview_spec.model_plan.raw.response_language.as_deref();
        let history =
            convert_history_messages(&current_context_messages, &compact_tool_calls, &model);
        let compact_instructions = instructions
            .as_ref()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty());

        let command_display_text = if let Some(extra) = compact_instructions.as_ref() {
            format!("/compact {}", extra)
        } else {
            "/compact".to_string()
        };

        let command_metadata = serde_json::json!({
            "composer": {
                "kind": "command",
                "displayText": command_display_text,
                "effectivePrompt": "Compact the current conversation history and preserve a summary in context.",
                "command": {
                    "source": "builtin",
                    "name": "compact",
                    "path": "/compact",
                    "description": "Clear history but keep a summary in context.",
                    "argumentHint": "[instructions=...]",
                    "argumentsText": instructions.clone().unwrap_or_default(),
                    "prompt": "Compact the current conversation history and preserve a summary in context.",
                    "behavior": "compact"
                }
            }
        });

        // Register a pseudo-run so the frontend can subscribe to events, the
        // thread is marked Running, and the thinking placeholder has a real
        // run_id to target.
        let (frontend_tx, frontend_rx) =
            broadcast::channel::<ThreadStreamEvent>(FRONTEND_EVENT_BUFFER_SIZE);
        let run_id = uuid::Uuid::now_v7().to_string();

        {
            let mut runs = self.active_runs.lock().await;
            if runs.values().any(|run| run.thread_id == thread_id) {
                return Err(AppError::recoverable(
                    ErrorSource::Thread,
                    "thread.run.already_active",
                    "A run is already active for this thread",
                ));
            }
            runs.insert(
                run_id.clone(),
                ActiveRun {
                    run_id: run_id.clone(),
                    thread_id: thread_id.to_string(),
                    profile_id: None,
                    frontend_tx: frontend_tx.clone(),
                    lightweight_model_role: None,
                    auxiliary_model_role: None,
                    primary_model_role: None,
                    streaming_message_id: None,
                    reasoning_message_id: None,
                    cancellation_requested: false,
                },
            );
        }
        self.sleep_manager.set_has_active_runs(true).await;

        // Persist the user message, reset marker, and a bare run row up front
        // so anything the frontend reloads before the LLM completes already
        // shows the correct structural state. The summary marker will be
        // written in the spawned task once we have a summary body.
        let user_message = MessageRecord {
            id: uuid::Uuid::now_v7().to_string(),
            thread_id: thread_id.to_string(),
            run_id: None,
            role: "user".to_string(),
            content_markdown: command_display_text.clone(),
            message_type: "plain_message".to_string(),
            status: "completed".to_string(),
            metadata_json: Some(command_metadata.to_string()),
            attachments_json: None,
            created_at: String::new(),
        };
        let reset_metadata = serde_json::json!({
            "kind": "context_reset",
            "source": "compact",
            "label": "Context is now reset",
        });
        let reset_message = MessageRecord {
            id: uuid::Uuid::now_v7().to_string(),
            thread_id: thread_id.to_string(),
            run_id: None,
            role: "system".to_string(),
            content_markdown: "Context is now reset".to_string(),
            message_type: "summary_marker".to_string(),
            status: "completed".to_string(),
            metadata_json: Some(reset_metadata.to_string()),
            attachments_json: None,
            created_at: String::new(),
        };

        let setup = async {
            message_repo::insert(&self.pool, &user_message).await?;
            message_repo::insert(&self.pool, &reset_message).await?;
            thread_repo::touch_active(&self.pool, thread_id).await?;
            run_repo::insert(
                &self.pool,
                &run_repo::RunInsert {
                    id: run_id.clone(),
                    thread_id: thread_id.to_string(),
                    profile_id: None,
                    run_mode: "compact".to_string(),
                    provider_id: None,
                    model_id: None,
                    effective_model_plan_json: Some(model_plan_value.to_string()),
                    status: "running".to_string(),
                },
            )
            .await?;
            thread_repo::update_status(&self.pool, thread_id, &ThreadStatus::Running).await?;
            Ok::<(), AppError>(())
        }
        .await;

        if let Err(error) = setup {
            self.remove_active_run(&run_id).await;
            return Err(error);
        }

        // Announce the run + compression state to subscribers. These two
        // events drive the frontend's thinking placeholder (`run_started`
        // flips the composer to disabled, `context_compressing` relabels the
        // placeholder to "Compressing context…").
        let _ = frontend_tx.send(ThreadStreamEvent::RunStarted {
            run_id: run_id.clone(),
            run_mode: "compact".to_string(),
        });
        let _ = frontend_tx.send(ThreadStreamEvent::ContextCompressing {
            run_id: run_id.clone(),
        });

        // Spawn the LLM call so the Tauri command returns immediately; the
        // broadcast channel keeps the frontend updated via its subscription.
        let manager = Arc::clone(self);
        let spawn_thread_id = thread_id.to_string();
        let spawn_run_id = run_id.clone();
        let spawn_model_role = preview_spec.model_plan.primary.clone();
        let spawn_response_language = response_language.map(str::to_owned);
        let spawn_frontend_tx = frontend_tx.clone();
        tokio::spawn(async move {
            manager
                .run_compact_background(
                    spawn_thread_id,
                    spawn_run_id,
                    spawn_model_role,
                    history,
                    compact_instructions,
                    spawn_response_language,
                    spawn_frontend_tx,
                )
                .await;
        });

        Ok((run_id, frontend_rx))
    }

    /// Body of the manual `/compact` background task.
    ///
    /// This is the LLM call + post-run bookkeeping, extracted so the
    /// front-end-visible ceremony in `compact_thread_context` is easy to
    /// audit. It always emits a terminal event (RunCompleted / RunFailed /
    /// RunCancelled) and always clears the `ActiveRun`, even on panic-like
    /// early returns, so the thread can't get stuck in Running state.
    async fn run_compact_background(
        self: Arc<Self>,
        thread_id: String,
        run_id: String,
        model_role: ResolvedModelRole,
        history: Vec<AgentMessage>,
        compact_instructions: Option<String>,
        response_language: Option<String>,
        frontend_tx: broadcast::Sender<ThreadStreamEvent>,
    ) {
        let summary_result = generate_primary_summary(
            &model_role,
            &history,
            compact_instructions.as_deref(),
            response_language.as_deref(),
            None,
        )
        .await;

        let final_event = match summary_result {
            Ok(summary) => {
                let summary_metadata = serde_json::json!({
                    "kind": "context_summary",
                    "source": "compact",
                    "label": "Compacted context summary",
                });
                let summary_message = MessageRecord {
                    id: uuid::Uuid::now_v7().to_string(),
                    thread_id: thread_id.clone(),
                    run_id: None,
                    role: "system".to_string(),
                    content_markdown: summary,
                    message_type: "summary_marker".to_string(),
                    status: "completed".to_string(),
                    metadata_json: Some(summary_metadata.to_string()),
                    attachments_json: None,
                    created_at: String::new(),
                };

                if let Err(e) = message_repo::insert(&self.pool, &summary_message).await {
                    tracing::error!(
                        thread_id = %thread_id,
                        run_id = %run_id,
                        error = %e,
                        "Failed to persist compact summary marker"
                    );
                    ThreadStreamEvent::RunFailed {
                        run_id: run_id.clone(),
                        error: format!("Failed to persist compact summary: {e}"),
                    }
                } else {
                    ThreadStreamEvent::RunCompleted {
                        run_id: run_id.clone(),
                    }
                }
            }
            Err(e) => {
                tracing::warn!(
                    thread_id = %thread_id,
                    run_id = %run_id,
                    error = %e,
                    "Manual /compact LLM summary failed"
                );
                // Honour the cancellation error code so the frontend can
                // distinguish a user-initiated cancel from a real failure.
                if e.error_code == "runtime.context_compression.cancelled" {
                    ThreadStreamEvent::RunCancelled {
                        run_id: run_id.clone(),
                    }
                } else {
                    // The reset marker was already persisted synchronously in
                    // `compact_thread_context`, so without a summary marker the
                    // thread would be left with a reset boundary but no record
                    // of prior context — the next run would resume from an
                    // empty head. Persist a heuristic summary as a safety net
                    // (mirrors the auto-compression fallback in
                    // `run_auto_compression`) so the skeleton of earlier
                    // context survives even when the LLM call fails. We still
                    // emit `RunFailed` so the user sees the error and can
                    // retry, but the conversation is no longer silently
                    // truncated on the way out.
                    let heuristic_summary =
                        crate::core::context_compression::generate_discard_summary(&history);
                    let summary_metadata = serde_json::json!({
                        "kind": "context_summary",
                        "source": "compact_fallback",
                        "label": "Compacted context summary",
                    });
                    let summary_message = MessageRecord {
                        id: uuid::Uuid::now_v7().to_string(),
                        thread_id: thread_id.clone(),
                        run_id: None,
                        role: "system".to_string(),
                        content_markdown: heuristic_summary,
                        message_type: "summary_marker".to_string(),
                        status: "completed".to_string(),
                        metadata_json: Some(summary_metadata.to_string()),
                        attachments_json: None,
                        created_at: String::new(),
                    };
                    if let Err(persist_err) =
                        message_repo::insert(&self.pool, &summary_message).await
                    {
                        tracing::warn!(
                            thread_id = %thread_id,
                            run_id = %run_id,
                            error = %persist_err,
                            "Failed to persist heuristic summary marker after /compact LLM failure"
                        );
                    }
                    ThreadStreamEvent::RunFailed {
                        run_id: run_id.clone(),
                        error: e.to_string(),
                    }
                }
            }
        };

        // Final bookkeeping: run row status, thread status, active-run cleanup.
        let final_status = match &final_event {
            ThreadStreamEvent::RunCompleted { .. } => "completed",
            ThreadStreamEvent::RunCancelled { .. } => "cancelled",
            ThreadStreamEvent::RunFailed { .. } => "failed",
            _ => "completed",
        };
        if let Err(e) = run_repo::update_status(&self.pool, &run_id, final_status).await {
            tracing::warn!(run_id = %run_id, error = %e, "Failed to update compact run status");
        }
        if let Err(e) =
            thread_repo::update_status(&self.pool, &thread_id, &ThreadStatus::Idle).await
        {
            tracing::warn!(thread_id = %thread_id, error = %e, "Failed to reset thread status after compact");
        }

        let _ = frontend_tx.send(final_event);
        self.remove_active_run(&run_id).await;
    }

    pub async fn subscribe_run(
        &self,
        thread_id: &str,
    ) -> Result<Option<(String, broadcast::Receiver<ThreadStreamEvent>)>, AppError> {
        let runs = self.active_runs.lock().await;
        let Some(run) = runs.values().find(|run| run.thread_id == thread_id) else {
            return Ok(None);
        };

        Ok(Some((run.run_id.clone(), run.frontend_tx.subscribe())))
    }

    pub async fn cancel_run(&self, thread_id: &str) -> Result<bool, AppError> {
        self.cancel_run_if_active(thread_id).await
    }

    pub async fn cancel_run_if_active(&self, thread_id: &str) -> Result<bool, AppError> {
        let Some(run_id) =
            mark_thread_run_cancellation_requested(&self.active_runs, thread_id).await
        else {
            return Ok(false);
        };

        run_repo::update_status(&self.pool, &run_id, "cancelling").await?;
        let cancel_delivered = self.runtime.cancel_session(&run_id).await?;

        if !cancel_delivered {
            tracing::warn!(
                run_id = %run_id,
                "runtime session missing during cancel; forcing cancelled cleanup"
            );
            self.handle_runtime_event(&run_id, build_orphaned_run_terminal_event(&run_id, true))
                .await?;
            return Ok(true);
        }

        if matches!(
            self.runtime.session_finish_state(&run_id).await,
            Some(RuntimeSessionFinishState::Panicked | RuntimeSessionFinishState::Cancelled)
        ) {
            tracing::warn!(
                run_id = %run_id,
                "runtime session was already finished when cancel was requested; forcing cancelled cleanup"
            );
            self.handle_runtime_event(&run_id, build_orphaned_run_terminal_event(&run_id, true))
                .await?;
            return Ok(true);
        }

        tracing::info!(run_id = %run_id, "run cancel requested");
        Ok(true)
    }

    pub async fn wait_until_thread_inactive(
        &self,
        thread_id: &str,
        timeout: Duration,
    ) -> Result<(), AppError> {
        let deadline = Instant::now() + timeout;

        loop {
            let has_active_run = {
                let runs = self.active_runs.lock().await;
                runs.values().any(|run| run.thread_id == thread_id)
            };

            if !has_active_run {
                return Ok(());
            }

            if Instant::now() >= deadline {
                return Err(AppError::recoverable(
                    ErrorSource::Thread,
                    "thread.run.cancel_timeout",
                    "Timed out while waiting for the active thread run to stop",
                ));
            }

            sleep(Duration::from_millis(50)).await;
        }
    }

    async fn expire_pending_plan_approval(&self, thread_id: &str) -> Result<(), AppError> {
        let Some((approval_message, mut approval_metadata)) =
            self.find_latest_pending_plan_approval(thread_id).await?
        else {
            return Ok(());
        };

        approval_metadata.state = IMPLEMENTATION_PLAN_SUPERSEDED_STATE.to_string();
        message_repo::update_metadata(
            &self.pool,
            &approval_message.id,
            serde_json::to_string(&approval_metadata).ok().as_deref(),
        )
        .await?;

        if let Some(plan_message) =
            message_repo::find_by_id(&self.pool, &approval_metadata.plan_message_id).await?
        {
            let mut plan_metadata = parse_message_metadata::<PlanMessageMetadata>(&plan_message)?;
            plan_metadata.approval_state = IMPLEMENTATION_PLAN_SUPERSEDED_STATE.to_string();
            message_repo::update_metadata(
                &self.pool,
                &plan_message.id,
                serde_json::to_string(&plan_metadata).ok().as_deref(),
            )
            .await?;
        }

        // Terminate any waiting_approval runs so they don't linger as zombies
        // in the database with no finished_at timestamp.
        run_repo::cancel_waiting_approval_by_thread(&self.pool, thread_id).await?;

        Ok(())
    }

    async fn build_context_reset_message_bundle(
        &self,
        thread_id: &str,
        plan_metadata: &PlanMessageMetadata,
    ) -> Result<ContextResetMessageBundle, AppError> {
        // Clear & Implement: no summary generation — the plan itself serves
        // as the context seed for the next run.
        let reset_message = MessageRecord {
            id: uuid::Uuid::now_v7().to_string(),
            thread_id: thread_id.to_string(),
            run_id: None,
            role: "system".to_string(),
            content_markdown: "Context is now reset".to_string(),
            message_type: "summary_marker".to_string(),
            status: "completed".to_string(),
            metadata_json: Some(
                serde_json::json!({
                    "kind": "context_reset",
                    "source": "plan_approval",
                    "label": "Context is now reset",
                })
                .to_string(),
            ),
            attachments_json: None,
            created_at: String::new(),
        };
        let approved_plan_message = MessageRecord {
            id: uuid::Uuid::now_v7().to_string(),
            thread_id: thread_id.to_string(),
            run_id: None,
            role: "assistant".to_string(),
            content_markdown: crate::core::plan_checkpoint::plan_markdown(plan_metadata),
            message_type: "plan".to_string(),
            status: "completed".to_string(),
            metadata_json: serde_json::to_string(plan_metadata).ok(),
            attachments_json: None,
            created_at: String::new(),
        };

        let history_override = vec![approved_plan_message.clone()];
        let persisted_messages = vec![reset_message, approved_plan_message];

        Ok(ContextResetMessageBundle {
            history_override,
            persisted_messages,
        })
    }

    async fn persist_messages(&self, messages: &[MessageRecord]) -> Result<(), AppError> {
        for message in messages {
            message_repo::insert(&self.pool, message).await?;
        }

        Ok(())
    }

    async fn find_latest_pending_plan_approval(
        &self,
        thread_id: &str,
    ) -> Result<Option<(MessageRecord, ApprovalPromptMetadata)>, AppError> {
        let messages = message_repo::list_recent(&self.pool, thread_id, None, 128).await?;
        for message in messages.into_iter().rev() {
            if message.message_type != "approval_prompt" {
                continue;
            }
            let Ok(metadata) = parse_message_metadata::<ApprovalPromptMetadata>(&message) else {
                continue;
            };
            if metadata.kind == IMPLEMENTATION_PLAN_APPROVAL_KIND
                && metadata.state == IMPLEMENTATION_PLAN_PENDING_STATE
            {
                return Ok(Some((message, metadata)));
            }
        }

        Ok(None)
    }

    async fn load_latest_pending_plan_approval(
        &self,
        thread_id: &str,
        requested_message_id: Option<&str>,
    ) -> Result<(MessageRecord, ApprovalPromptMetadata), AppError> {
        let Some((message, metadata)) = self.find_latest_pending_plan_approval(thread_id).await?
        else {
            return Err(AppError::recoverable(
                ErrorSource::Thread,
                "thread.plan_approval.not_found",
                "No pending implementation-plan approval was found for this thread.",
            ));
        };

        if let Some(requested_message_id) = requested_message_id {
            if message.id != requested_message_id {
                return Err(AppError::recoverable(
                    ErrorSource::Thread,
                    "thread.plan_approval.stale_revision",
                    "This approval request is no longer current. Please approve the latest plan revision.",
                ));
            }
        }

        Ok((message, metadata))
    }

    pub fn spawn_runtime_event_loop(
        self: &Arc<Self>,
        run_id: String,
        mut event_rx: mpsc::UnboundedReceiver<ThreadStreamEvent>,
    ) {
        let manager = Arc::clone(self);
        tokio::spawn(async move {
            while let Some(event) = event_rx.recv().await {
                if let Err(error) = manager.handle_runtime_event(&run_id, event).await {
                    tracing::error!(run_id = %run_id, error = %error, "failed to handle runtime event");
                }
            }

            if let Err(error) = manager.handle_runtime_channel_closed(&run_id).await {
                tracing::error!(
                    run_id = %run_id,
                    error = %error,
                    "failed to reconcile run after runtime event channel closed"
                );
            }
        });
    }

    pub(crate) fn spawn_runtime_finish_watchdog(
        self: &Arc<Self>,
        run_id: String,
        mut finish_rx: tokio::sync::watch::Receiver<RuntimeSessionFinishState>,
    ) {
        let manager = Arc::clone(self);
        tokio::spawn(async move {
            if finish_rx.changed().await.is_err() {
                return;
            }

            let finish_state = *finish_rx.borrow();
            if !manager.has_active_run(&run_id).await {
                return;
            }

            let event = match finish_state {
                RuntimeSessionFinishState::Running | RuntimeSessionFinishState::Completed => return,
                RuntimeSessionFinishState::Panicked => ThreadStreamEvent::RunInterrupted {
                    run_id: run_id.clone(),
                },
                RuntimeSessionFinishState::Cancelled => {
                    build_orphaned_run_terminal_event(&run_id, true)
                }
            };

            if let Err(error) = manager.handle_runtime_event(&run_id, event).await {
                tracing::error!(
                    run_id = %run_id,
                    error = %error,
                    finish_state = ?finish_state,
                    "failed to reconcile run after runtime task finished"
                );
            }
        });
    }

    pub(crate) async fn get_thread_id(&self, run_id: &str) -> String {
        let runs = self.active_runs.lock().await;
        runs.get(run_id)
            .map(|run| run.thread_id.clone())
            .unwrap_or_default()
    }

    pub(crate) async fn has_active_run(&self, run_id: &str) -> bool {
        let runs = self.active_runs.lock().await;
        runs.contains_key(run_id)
    }

    pub(crate) async fn was_cancel_requested(&self, run_id: &str) -> bool {
        let runs = self.active_runs.lock().await;
        runs.get(run_id)
            .map(|run| run.cancellation_requested)
            .unwrap_or(false)
    }

    pub(crate) async fn remove_active_run(&self, run_id: &str) {
        let has_active_runs = {
            let mut runs = self.active_runs.lock().await;
            runs.remove(run_id);
            !runs.is_empty()
        };

        self.sleep_manager
            .set_has_active_runs(has_active_runs)
            .await;
    }
}

#[cfg(test)]
mod tests {
    use super::{
        append_compact_instructions, await_summary_with_abort, build_compact_summary_messages,
        build_compact_summary_system_prompt, build_implementation_handoff_prompt,
        build_merge_summary_messages, build_merge_summary_system_prompt,
        build_orphaned_run_terminal_event, build_title_model_candidates, build_title_prompt,
        build_title_prompt_from_messages, collapse_whitespace, detect_prior_summary,
        extract_context_summary_block, extract_run_model_refs, extract_run_string,
        mark_thread_run_cancellation_requested, merge_json_value, normalize_compact_summary,
        normalize_generated_title, render_compact_summary_history,
        should_complete_reasoning_for_event, summary_history_char_budget, terminal_event_status,
        truncate_chars, truncate_chars_keep_tail, truncate_tool_result_head_tail, ActiveRun,
        SUMMARY_HISTORY_MIN_CHARS, SUMMARY_TOOL_RESULT_MAX_CHARS,
    };
    use crate::core::agent_session::{ProfileResponseStyle, ResolvedModelRole};
    use crate::core::plan_checkpoint::{
        build_plan_artifact_from_tool_input, build_plan_message_metadata, PlanApprovalAction,
    };
    use crate::ipc::frontend_channels::ThreadStreamEvent;
    use crate::model::thread::MessageRecord;
    use std::collections::HashMap;
    use tiycore::agent::AgentMessage;
    use tiycore::types::{Message as TiyMessage, UserMessage};
    use tokio::sync::{broadcast, Mutex};

    #[tokio::test]
    async fn mark_thread_run_cancellation_requested_returns_falsey_none_when_thread_is_inactive() {
        let active_runs = Mutex::new(HashMap::<String, ActiveRun>::new());

        let run_id = mark_thread_run_cancellation_requested(&active_runs, "thread-missing").await;

        assert_eq!(run_id, None);
    }

    #[tokio::test]
    async fn mark_thread_run_cancellation_requested_marks_matching_run_and_returns_run_id() {
        let (frontend_tx, _) = broadcast::channel::<ThreadStreamEvent>(1);
        let active_runs = Mutex::new(HashMap::from([(
            "run-1".to_string(),
            ActiveRun {
                run_id: "run-1".to_string(),
                thread_id: "thread-1".to_string(),
                profile_id: None,
                frontend_tx,
                lightweight_model_role: None,
                auxiliary_model_role: None,
                primary_model_role: None,
                streaming_message_id: None,
                reasoning_message_id: None,
                cancellation_requested: false,
            },
        )]));

        let run_id = mark_thread_run_cancellation_requested(&active_runs, "thread-1").await;

        assert_eq!(run_id.as_deref(), Some("run-1"));
        let runs = active_runs.lock().await;
        assert!(runs
            .get("run-1")
            .is_some_and(|run| run.cancellation_requested));
    }

    // ------------------------------------------------------------------
    // ActiveRun lifecycle invariants for `compact_thread_context`
    // ------------------------------------------------------------------
    //
    // `compact_thread_context` and `start_run` share the same guard pattern
    // (see lines ~176 and ~544): an insert into `active_runs` guarded by
    // `runs.values().any(|run| run.thread_id == thread_id)`. The concurrency
    // correctness of /compact hinges on that guard and on `remove_active_run`
    // clearing the entry after the background task finishes, *even on
    // failure*. A full end-to-end test would need a mock `LlmProvider` plus
    // a real `BuiltInAgentRuntime`, which is disproportionate for verifying
    // what is fundamentally a `HashMap` insert/remove contract. Instead we
    // drive the guard pattern directly against the same data structure.

    fn make_active_run(thread_id: &str, run_id: &str) -> ActiveRun {
        let (frontend_tx, _) = broadcast::channel::<ThreadStreamEvent>(1);
        ActiveRun {
            run_id: run_id.to_string(),
            thread_id: thread_id.to_string(),
            profile_id: None,
            frontend_tx,
            lightweight_model_role: None,
            auxiliary_model_role: None,
            primary_model_role: None,
            streaming_message_id: None,
            reasoning_message_id: None,
            cancellation_requested: false,
        }
    }

    #[test]
    fn orphaned_run_terminal_event_uses_cancelled_when_user_requested_stop() {
        let cancelled = build_orphaned_run_terminal_event("run-1", true);
        let interrupted = build_orphaned_run_terminal_event("run-2", false);

        assert!(matches!(
            cancelled,
            ThreadStreamEvent::RunCancelled { ref run_id } if run_id == "run-1"
        ));
        assert!(matches!(
            interrupted,
            ThreadStreamEvent::RunInterrupted { ref run_id } if run_id == "run-2"
        ));
    }

    #[test]
    fn terminal_event_status_maps_interrupted_to_cancelled_when_cancel_was_requested() {
        let interrupted = ThreadStreamEvent::RunInterrupted {
            run_id: "run-1".to_string(),
        };
        let cancelled = ThreadStreamEvent::RunCancelled {
            run_id: "run-1".to_string(),
        };

        assert_eq!(terminal_event_status(&interrupted, true), Some("cancelled"));
        assert_eq!(
            terminal_event_status(&interrupted, false),
            Some("interrupted")
        );
        assert_eq!(terminal_event_status(&cancelled, false), Some("cancelled"));
    }

    /// Mirrors the check in `compact_thread_context` (and `start_run`): a
    /// second concurrent run on the same thread must be rejected so the
    /// thread can't accumulate overlapping ActiveRun entries.
    #[tokio::test]
    async fn active_run_guard_rejects_second_run_on_same_thread() {
        let active_runs = Mutex::new(HashMap::<String, ActiveRun>::new());

        // First compact inserts successfully.
        {
            let mut runs = active_runs.lock().await;
            let already_active = runs.values().any(|run| run.thread_id == "thread-1");
            assert!(!already_active);
            runs.insert("run-1".to_string(), make_active_run("thread-1", "run-1"));
        }

        // Second compact (same thread) must see the guard fire.
        {
            let runs = active_runs.lock().await;
            let already_active = runs.values().any(|run| run.thread_id == "thread-1");
            assert!(
                already_active,
                "Guard must reject overlapping compact on same thread"
            );
        }

        // A different thread is unaffected.
        {
            let runs = active_runs.lock().await;
            let other_thread_active = runs.values().any(|run| run.thread_id == "thread-2");
            assert!(!other_thread_active);
        }
    }

    /// After `run_compact_background` finishes — whether the LLM call
    /// succeeded or failed — the ActiveRun entry must be gone so subsequent
    /// /compact invocations are accepted. The doc comment on
    /// `run_compact_background` promises this invariant; this test pins
    /// it to the data-structure contract it ultimately relies on.
    #[tokio::test]
    async fn active_run_removed_unblocks_future_compacts_on_same_thread() {
        let active_runs = Mutex::new(HashMap::<String, ActiveRun>::new());

        // Simulate a compact run being registered and then cleaned up.
        {
            let mut runs = active_runs.lock().await;
            runs.insert(
                "run-compact".to_string(),
                make_active_run("thread-1", "run-compact"),
            );
        }
        {
            let mut runs = active_runs.lock().await;
            runs.remove("run-compact");
        }

        // A follow-up compact must now observe no active run on thread-1.
        let runs = active_runs.lock().await;
        let blocked = runs.values().any(|run| run.thread_id == "thread-1");
        assert!(
            !blocked,
            "Stale ActiveRun entry would leave the thread stuck in Running"
        );
    }

    /// Setup failure in `compact_thread_context` (e.g. DB insert rejecting
    /// the user message) must roll back the ActiveRun insert — otherwise
    /// every subsequent /compact on the same thread would return
    /// `thread.run.already_active` until the process restarts.
    #[tokio::test]
    async fn active_run_rolled_back_on_setup_failure() {
        let active_runs = Mutex::new(HashMap::<String, ActiveRun>::new());

        // Simulated setup sequence: insert, then encounter a failure and
        // clean up — exactly what `compact_thread_context` does at the
        // `if let Err(error) = setup { self.remove_active_run(...) }`
        // branch.
        {
            let mut runs = active_runs.lock().await;
            runs.insert(
                "run-setup-fail".to_string(),
                make_active_run("thread-1", "run-setup-fail"),
            );
        }
        // ... setup returns Err here in production ...
        {
            let mut runs = active_runs.lock().await;
            runs.remove("run-setup-fail");
        }

        // Next /compact attempt on the same thread must succeed.
        let runs = active_runs.lock().await;
        let blocked = runs.values().any(|run| run.thread_id == "thread-1");
        assert!(
            !blocked,
            "Setup-failure path must leave active_runs empty for the thread"
        );
    }

    #[test]
    fn normalize_generated_title_strips_prefixes_and_wrappers() {
        assert_eq!(
            normalize_generated_title("Title: \"Fix terminal resize drift\"").as_deref(),
            Some("Fix terminal resize drift")
        );
        assert_eq!(
            normalize_generated_title("标题：   新建线程标题生成   ").as_deref(),
            Some("新建线程标题生成")
        );
    }

    #[test]
    fn collapse_whitespace_compacts_internal_spacing() {
        assert_eq!(collapse_whitespace("foo   bar\nbaz"), "foo bar baz");
    }

    #[test]
    fn truncate_chars_limits_character_count() {
        assert_eq!(truncate_chars("abcdef", 4), "abcd");
        assert_eq!(truncate_chars("你好世界标题", 4), "你好世界");
    }

    #[test]
    fn normalize_compact_summary_wraps_plain_text_output() {
        assert_eq!(
            normalize_compact_summary("Goal: fix compact summary".to_string(), None).as_deref(),
            Some("<context_summary>\nGoal: fix compact summary\n</context_summary>")
        );
    }

    #[test]
    fn normalize_compact_summary_extracts_single_wrapped_block_from_noisy_output() {
        let summary = normalize_compact_summary(
            "Here is the summary:\n<context_summary>\nState\n</context_summary>\nTrailing note"
                .to_string(),
            None,
        )
        .expect("summary should be present");

        assert_eq!(summary, "<context_summary>\nState\n</context_summary>");
    }

    #[test]
    fn normalize_compact_summary_recovers_from_missing_closing_wrapper() {
        let summary =
            normalize_compact_summary("<context_summary>\nGoal\n- Pending item".to_string(), None)
                .expect("summary should be present");

        assert_eq!(
            summary,
            "<context_summary>\nGoal\n- Pending item\n</context_summary>"
        );
    }

    #[test]
    fn compact_summary_system_prompt_includes_wrapper_example() {
        let prompt = build_compact_summary_system_prompt(None);

        assert!(prompt.contains("Output rules:"));
        assert!(prompt.contains("Do not output any text before or after the wrapper."));
        assert!(prompt.contains("Example output:"));
        assert!(prompt.contains("<context_summary>"));
        assert!(prompt.contains("</context_summary>"));
    }

    #[test]
    fn compact_summary_system_prompt_uses_response_language_when_present() {
        let prompt = build_compact_summary_system_prompt(Some(" 简体中文 "));

        assert!(prompt.contains(
            "Respond in 简体中文 unless the user explicitly asks for a different language."
        ));
    }

    #[test]
    fn compact_summary_messages_split_instructions_and_history() {
        let history = vec![AgentMessage::User(UserMessage::text(
            "User asked for a compact summary",
        ))];
        let messages =
            build_compact_summary_messages(&history, Some("Keep unresolved risks"), 100_000);

        assert_eq!(messages.len(), 2);

        match &messages[0] {
            TiyMessage::User(user) => {
                let text = match &user.content {
                    tiycore::types::UserContent::Text(text) => text,
                    _ => panic!("expected text user message for instructions"),
                };
                assert!(text.contains("Additional user instructions for this compact"));
                assert!(text.contains("Keep unresolved risks"));
            }
            _ => panic!("expected first compact message to be user instructions"),
        }

        match &messages[1] {
            TiyMessage::User(user) => {
                let text = match &user.content {
                    tiycore::types::UserContent::Text(text) => text,
                    _ => panic!("expected text user message for history"),
                };
                assert!(text.starts_with("Conversation history to compact:"));
                assert!(text.contains("[user]"));
                assert!(text.contains("User asked for a compact summary"));
            }
            _ => panic!("expected second compact message to be user history"),
        }
    }

    #[test]
    fn extract_context_summary_block_returns_first_complete_block() {
        let extracted = extract_context_summary_block(
            "prefix\n<context_summary>\nFirst\n</context_summary>\n<context_summary>\nSecond\n</context_summary>",
        )
        .expect("context summary block should be extracted");

        assert_eq!(extracted, "<context_summary>\nFirst\n</context_summary>");
    }

    #[test]
    fn append_compact_instructions_adds_extra_block() {
        let summary = append_compact_instructions(
            "<context_summary>\nState\n</context_summary>".to_string(),
            Some("Preserve pending migration notes"),
        );

        assert!(summary.contains("<extra_instructions>"));
        assert!(summary.contains("Preserve pending migration notes"));
    }

    #[test]
    fn normalize_compact_summary_keeps_existing_wrapper_and_appends_instructions() {
        let summary = normalize_compact_summary(
            "<context_summary>\nState\n</context_summary>".to_string(),
            Some("Keep unresolved API choice"),
        )
        .expect("summary should be present");

        assert!(summary.starts_with("<context_summary>"));
        assert!(summary.contains("<extra_instructions>"));
        assert!(summary.contains("Keep unresolved API choice"));
    }

    #[test]
    fn title_prompt_uses_profile_response_language_when_present() {
        let prompt = build_title_prompt(
            "请帮我排查窗口缩放问题",
            "我已经定位到标题栏重绘时机。",
            Some("Japanese"),
            ProfileResponseStyle::Guide,
        );

        assert!(prompt.contains("Write the title in Japanese."));
        assert!(prompt.contains("signals the user's goal or decision focus clearly"));
    }

    #[test]
    fn title_prompt_from_messages_renders_newest_messages_first() {
        let messages = vec![
            MessageRecord {
                id: "msg-1".into(),
                thread_id: "thread-1".into(),
                run_id: None,
                role: "user".into(),
                content_markdown: "oldest user message".into(),
                message_type: "plain_message".into(),
                status: "completed".into(),
                metadata_json: None,
                attachments_json: None,
                created_at: String::new(),
            },
            MessageRecord {
                id: "msg-2".into(),
                thread_id: "thread-1".into(),
                run_id: None,
                role: "assistant".into(),
                content_markdown: "newer assistant reply".into(),
                message_type: "plain_message".into(),
                status: "completed".into(),
                metadata_json: None,
                attachments_json: None,
                created_at: String::new(),
            },
            MessageRecord {
                id: "msg-3".into(),
                thread_id: "thread-1".into(),
                run_id: None,
                role: "user".into(),
                content_markdown: "newest user follow-up".into(),
                message_type: "plain_message".into(),
                status: "completed".into(),
                metadata_json: None,
                attachments_json: None,
                created_at: String::new(),
            },
        ];

        let prompt = build_title_prompt_from_messages(
            &messages,
            Some("English"),
            ProfileResponseStyle::Balanced,
        );

        let newest_idx = prompt
            .find("User:\nnewest user follow-up")
            .expect("newest message should be present");
        let older_idx = prompt
            .find("Assistant:\nnewer assistant reply")
            .expect("older assistant message should be present");
        let oldest_idx = prompt
            .find("User:\noldest user message")
            .expect("oldest message should be present");

        assert!(newest_idx < older_idx);
        assert!(older_idx < oldest_idx);
        assert!(prompt.contains("Write the title in English."));
    }

    #[test]
    fn terminal_event_status_and_runtime_event_classification_cover_terminal_outcomes() {
        let completed = ThreadStreamEvent::RunCompleted {
            run_id: "run-1".to_string(),
        };
        assert_eq!(terminal_event_status(&completed, false), Some("completed"));
        assert!(super::is_terminal_runtime_event(&completed));

        let limit = ThreadStreamEvent::RunLimitReached {
            run_id: "run-1".to_string(),
            error: "too many turns".to_string(),
            max_turns: 10,
        };
        assert_eq!(terminal_event_status(&limit, false), Some("limit_reached"));
        assert!(super::is_terminal_runtime_event(&limit));

        let failed = ThreadStreamEvent::RunFailed {
            run_id: "run-1".to_string(),
            error: "boom".to_string(),
        };
        assert_eq!(terminal_event_status(&failed, false), Some("failed"));

        let cancelled = ThreadStreamEvent::RunCancelled {
            run_id: "run-1".to_string(),
        };
        assert_eq!(terminal_event_status(&cancelled, false), Some("cancelled"));

        let interrupted = ThreadStreamEvent::RunInterrupted {
            run_id: "run-1".to_string(),
        };
        assert_eq!(
            terminal_event_status(&interrupted, false),
            Some("interrupted")
        );
        assert_eq!(terminal_event_status(&interrupted, true), Some("cancelled"));

        let delta = ThreadStreamEvent::MessageDelta {
            run_id: "run-1".to_string(),
            message_id: "message-1".to_string(),
            delta: "hi".to_string(),
        };
        assert_eq!(terminal_event_status(&delta, false), None);
        assert!(!super::is_terminal_runtime_event(&delta));
    }

    #[test]
    fn extract_run_model_refs_reads_profile_provider_and_model_fallbacks() {
        let plan = serde_json::json!({
            "profileId": "profile-1",
            "primary": {
                "providerId": "provider-1",
                "modelRecordId": "record-1",
                "modelId": "model-1"
            }
        });
        assert_eq!(
            extract_run_string(&plan, &["primary", "providerId"]).as_deref(),
            Some("provider-1")
        );
        assert_eq!(
            extract_run_model_refs(&plan),
            (
                Some("profile-1".to_string()),
                Some("provider-1".to_string()),
                Some("record-1".to_string())
            )
        );

        let fallback = serde_json::json!({
            "primary": { "providerId": "provider-2", "modelId": "model-2" }
        });
        assert_eq!(
            extract_run_model_refs(&fallback),
            (
                None,
                Some("provider-2".to_string()),
                Some("model-2".to_string())
            )
        );
        assert_eq!(extract_run_string(&fallback, &["primary", "missing"]), None);
    }

    #[test]
    fn agent_run_manager_merge_json_value_recursively_merges_payload_options() {
        let mut base = serde_json::json!({
            "messages": [],
            "providerOptions": { "temperature": 0.1, "nested": { "a": 1 } },
            "replace": { "old": true }
        });
        let patch = serde_json::json!({
            "providerOptions": { "topP": 0.9, "nested": { "b": 2 } },
            "replace": null
        });

        merge_json_value(&mut base, &patch);

        assert_eq!(base["providerOptions"]["temperature"], 0.1);
        assert_eq!(base["providerOptions"]["topP"], 0.9);
        assert_eq!(
            base["providerOptions"]["nested"],
            serde_json::json!({ "a": 1, "b": 2 })
        );
        assert_eq!(base["replace"], serde_json::Value::Null);
    }

    #[test]
    fn reasoning_completion_helper_keeps_only_live_reasoning_events_open() {
        assert!(!should_complete_reasoning_for_event(
            &ThreadStreamEvent::RunStarted {
                run_id: "run-1".into(),
                run_mode: "default".into(),
            }
        ));
        assert!(!should_complete_reasoning_for_event(
            &ThreadStreamEvent::ReasoningUpdated {
                run_id: "run-1".into(),
                message_id: "reasoning-1".into(),
                reasoning: "Inspecting".into(),
                thinking_signature: None,
            }
        ));
        assert!(should_complete_reasoning_for_event(
            &ThreadStreamEvent::ToolRequested {
                run_id: "run-1".into(),
                tool_call_id: "tool-1".into(),
                tool_name: "search".into(),
                tool_input: serde_json::json!({ "query": "Thought" }),
            }
        ));
    }

    #[test]
    fn implementation_handoff_prompt_embeds_the_approved_plan() {
        let artifact = build_plan_artifact_from_tool_input(
            &serde_json::json!({
                "title": "Approved plan",
                "summary": "Execute the plan exactly.",
                "steps": ["Apply the checkpointed implementation plan."]
            }),
            4,
        );
        let metadata = build_plan_message_metadata(artifact, "run-plan", "plan");

        let prompt = build_implementation_handoff_prompt(
            "thread-handoff-test",
            &metadata,
            PlanApprovalAction::ApplyPlanWithContextReset,
        );

        assert!(prompt.contains("Plan revision: 4"));
        assert!(prompt.contains(
            "The reset context already includes a historical summary and the approved plan."
        ));
        assert!(
            prompt.contains("Treat the approved plan in context as the implementation baseline.")
        );
        assert!(prompt.contains("after clearing the planning conversation"));
        assert!(prompt.contains("agent_review with planFilePath"));
    }

    #[test]
    fn detect_prior_summary_matches_wrapped_first_user_message() {
        let messages = vec![
            AgentMessage::User(UserMessage::text(
                "<context_summary>\nState A\n</context_summary>",
            )),
            AgentMessage::User(UserMessage::text("follow-up question")),
        ];

        let (prior, prefix_len) =
            detect_prior_summary(&messages).expect("prior summary should be detected");
        assert!(prior.contains("State A"));
        assert_eq!(prefix_len, 1);
    }

    #[test]
    fn detect_prior_summary_tolerates_leading_whitespace() {
        let messages = vec![AgentMessage::User(UserMessage::text(
            "   \n<context_summary>\nState\n</context_summary>",
        ))];
        assert!(detect_prior_summary(&messages).is_some());
    }

    #[test]
    fn detect_prior_summary_rejects_non_user_first_message() {
        let messages = vec![AgentMessage::User(UserMessage::text(
            "not a summary, just a question",
        ))];
        assert!(detect_prior_summary(&messages).is_none());
    }

    #[test]
    fn detect_prior_summary_rejects_truncated_block_without_closing_tag() {
        // An unterminated wrapper is not a well-formed prior summary — fall
        // back to the normal re-summarise path rather than merging into a
        // partial block.
        let messages = vec![AgentMessage::User(UserMessage::text(
            "<context_summary>\nState without close tag",
        ))];
        assert!(detect_prior_summary(&messages).is_none());
    }

    #[test]
    fn detect_prior_summary_accepts_blocks_content_with_single_text_block() {
        // The Blocks variant is used when an attachment or image is attached.
        // For detection we only require a single Text block to carry a
        // well-formed <context_summary>. Multi-modal Blocks should still
        // detect the summary from whichever block contains the text.
        use tiycore::types::{ContentBlock, TextContent};
        let user_msg = UserMessage::blocks(vec![ContentBlock::Text(TextContent::new(
            "<context_summary>\nBlocks-wrapped state\n</context_summary>",
        ))]);
        let messages = vec![AgentMessage::User(user_msg)];

        let (prior, prefix_len) =
            detect_prior_summary(&messages).expect("Blocks path should still detect summary");
        assert!(prior.contains("Blocks-wrapped state"));
        assert_eq!(prefix_len, 1);
    }

    #[test]
    fn detect_prior_summary_rejects_blocks_with_only_image() {
        // A Blocks user message that contains no Text at all (e.g. an
        // image-only attachment) cannot carry a summary — the detector
        // should treat it like "no summary present" and fall through.
        use tiycore::types::{ContentBlock, ImageContent};
        let user_msg = UserMessage::blocks(vec![ContentBlock::Image(ImageContent::new(
            "AAAA",
            "image/png",
        ))]);
        let messages = vec![AgentMessage::User(user_msg)];
        assert!(detect_prior_summary(&messages).is_none());
    }

    #[test]
    fn merge_summary_system_prompt_explains_the_merge_contract() {
        let prompt = build_merge_summary_system_prompt(None);
        assert!(prompt.contains("PRIOR summary"));
        assert!(prompt.contains("DELTA"));
        assert!(prompt.contains("<context_summary>"));
        assert!(prompt.contains("</context_summary>"));
    }

    #[test]
    fn merge_summary_system_prompt_uses_response_language_when_present() {
        let prompt = build_merge_summary_system_prompt(Some("Japanese"));

        assert!(prompt.contains(
            "Respond in Japanese unless the user explicitly asks for a different language."
        ));
    }

    #[test]
    fn merge_summary_system_prompt_ignores_blank_response_language() {
        let prompt = build_merge_summary_system_prompt(Some("   "));

        assert!(!prompt.contains("Respond in"));
    }

    #[test]
    fn merge_summary_messages_include_prior_and_delta_in_order() {
        let delta = vec![AgentMessage::User(UserMessage::text(
            "New user input to fold into the summary",
        ))];
        let messages = build_merge_summary_messages(
            "<context_summary>\nOld state\n</context_summary>",
            &delta,
            Some("Keep API choice intact"),
            100_000,
        );

        assert_eq!(messages.len(), 3);

        // 0: instructions, 1: prior summary, 2: delta history
        match &messages[1] {
            TiyMessage::User(user) => {
                let text = match &user.content {
                    tiycore::types::UserContent::Text(t) => t,
                    _ => panic!("expected text user message"),
                };
                assert!(text.starts_with("Prior summary"));
                assert!(text.contains("Old state"));
            }
            _ => panic!("expected the prior-summary slot to be a user message"),
        }

        match &messages[2] {
            TiyMessage::User(user) => {
                let text = match &user.content {
                    tiycore::types::UserContent::Text(t) => t,
                    _ => panic!("expected text user message"),
                };
                assert!(text.starts_with("New conversation delta"));
                assert!(text.contains("New user input to fold"));
            }
            _ => panic!("expected the delta slot to be a user message"),
        }
    }

    #[test]
    fn merge_summary_messages_omit_instructions_slot_when_none() {
        // No instructions = no leading instructions slot → exactly 2 messages
        // (prior summary, delta history). If we accidentally start sending 3
        // messages with an empty instructions block, the model would waste
        // tokens on a stub prompt.
        let delta = vec![AgentMessage::User(UserMessage::text("delta message"))];
        let messages = build_merge_summary_messages(
            "<context_summary>\nOld state\n</context_summary>",
            &delta,
            None,
            100_000,
        );

        assert_eq!(
            messages.len(),
            2,
            "without instructions the merge-summary payload should be prior+delta only"
        );

        // Slot 0 must now be the prior-summary (not instructions).
        match &messages[0] {
            TiyMessage::User(user) => {
                let text = match &user.content {
                    tiycore::types::UserContent::Text(t) => t,
                    _ => panic!("expected text user message"),
                };
                assert!(text.starts_with("Prior summary"));
            }
            _ => panic!("expected user message at slot 0"),
        }
    }

    #[test]
    fn merge_summary_messages_treat_whitespace_instructions_as_none() {
        // Whitespace-only instructions are semantically equivalent to None and
        // must not produce a dangling empty instructions slot.
        let delta = vec![AgentMessage::User(UserMessage::text("delta"))];
        let messages = build_merge_summary_messages(
            "<context_summary>\nOld\n</context_summary>",
            &delta,
            Some("   \n\t  "),
            100_000,
        );
        assert_eq!(
            messages.len(),
            2,
            "whitespace-only instructions should behave like None"
        );
    }

    #[tokio::test]
    async fn await_summary_with_abort_returns_future_value_when_not_cancelled() {
        let signal = tiycore::agent::AbortSignal::new();
        let result = await_summary_with_abort(async { 42_u32 }, Some(signal))
            .await
            .expect("future should complete");
        assert_eq!(result, 42);
    }

    #[tokio::test]
    async fn await_summary_with_abort_short_circuits_when_signal_already_cancelled() {
        let signal = tiycore::agent::AbortSignal::new();
        signal.cancel();

        // The future below would block indefinitely — the test only passes if
        // the pre-check on the already-cancelled signal returns Err without
        // polling the future.
        let blocker = std::future::pending::<u32>();
        let error = await_summary_with_abort(blocker, Some(signal))
            .await
            .expect_err("expected cancellation to short-circuit");
        assert_eq!(error.error_code, "runtime.context_compression.cancelled");
    }

    #[tokio::test]
    async fn await_summary_with_abort_cancels_midflight_future() {
        use std::sync::Arc;
        use tokio::sync::Notify;

        let signal = tiycore::agent::AbortSignal::new();
        let signal_for_task = signal.clone();

        // Deterministic mid-flight handshake: the future below notifies once
        // it has been polled at least once, and the canceller only fires
        // *after* receiving that notification. This replaces a timing-based
        // `sleep(20ms)` that could flake under CI load.
        let polled = Arc::new(Notify::new());
        let polled_for_canceller = polled.clone();

        let canceller = tokio::spawn(async move {
            polled_for_canceller.notified().await;
            signal_for_task.cancel();
        });

        let polled_for_future = polled.clone();
        let blocker = async move {
            // Signal on the first poll, then block forever — the test only
            // passes if the select branch picks up the subsequent cancel.
            polled_for_future.notify_one();
            std::future::pending::<u32>().await
        };

        let error = await_summary_with_abort(blocker, Some(signal))
            .await
            .expect_err("expected cancellation to race the pending future");
        assert_eq!(error.error_code, "runtime.context_compression.cancelled");

        // The canceller always completes: it only ever awaits `polled` once
        // and then cancels. Awaiting it here guarantees no test leaks a
        // dangling task on success.
        canceller.await.expect("canceller task should complete");
    }

    #[tokio::test]
    async fn await_summary_with_abort_passes_through_when_no_signal_provided() {
        let result = await_summary_with_abort(async { "hi".to_string() }, None)
            .await
            .expect("without a signal, errors are not produced");
        assert_eq!(result, "hi");
    }

    // ----- render_compact_summary_history: budget-aware packing -----

    fn build_assistant_text_message(text: &str) -> AgentMessage {
        use tiycore::types::{
            Api, AssistantMessage, ContentBlock, Provider, StopReason, TextContent, Usage,
        };
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

    #[test]
    fn render_compact_summary_history_preserves_full_messages_when_within_budget() {
        // Pre-refactor behaviour capped user messages at 1,200 chars; a
        // 3,000-char user message would get silently clipped. Verify the new
        // holistic budget keeps it intact end-to-end.
        let long = "x".repeat(3_000);
        let history = vec![
            AgentMessage::User(UserMessage::text(&long)),
            build_assistant_text_message("short reply"),
        ];

        let rendered = render_compact_summary_history(&history, 100_000);
        assert!(
            rendered.contains(&long),
            "expected full 3000-char user message to be preserved"
        );
        assert!(rendered.contains("short reply"));
        // Chronological order: user before assistant.
        let user_pos = rendered.find("[user]").unwrap();
        let assistant_pos = rendered.find("[assistant]").unwrap();
        assert!(user_pos < assistant_pos);
    }

    #[test]
    fn render_compact_summary_history_drops_oldest_when_budget_exhausted() {
        // With a tiny budget that only fits one message, the NEWEST should
        // survive and the oldest should be dropped (newest-to-oldest packing,
        // then reversed).
        let history = vec![
            AgentMessage::User(UserMessage::text("OLDEST: ancient message")),
            build_assistant_text_message("MIDDLE: intermediate"),
            AgentMessage::User(UserMessage::text("NEWEST: recent message")),
        ];

        // ~60 chars budget: fits only one short chunk (including header + \n\n).
        let rendered = render_compact_summary_history(&history, 60);
        assert!(rendered.contains("NEWEST"));
        assert!(!rendered.contains("OLDEST"));
    }

    #[test]
    fn render_compact_summary_history_tail_truncates_single_oversized_item() {
        // A single item larger than the entire budget should not be dropped
        // — the tail should be kept (more recent portion is more relevant)
        // and an elision marker inserted so the model knows content was cut.
        let massive = "line ".repeat(2_000); // 10_000 chars
        let history = vec![AgentMessage::User(UserMessage::text(&massive))];

        let rendered = render_compact_summary_history(&history, 500);
        assert!(rendered.chars().count() <= 500);
        assert!(rendered.contains("earlier content truncated"));
    }

    #[test]
    fn render_compact_summary_history_skips_empty_chunks_instead_of_counting_them() {
        // An empty user message must not consume any budget and must not
        // emit a stray [user] header — those chunks are `continue`d.
        use tiycore::types::{ContentBlock, TextContent};
        let empty_blocks = UserMessage::blocks(vec![ContentBlock::Text(TextContent::new(""))]);
        let history = vec![
            AgentMessage::User(empty_blocks),
            build_assistant_text_message("reply"),
        ];
        let rendered = render_compact_summary_history(&history, 100_000);
        assert!(!rendered.contains("[user]"));
        assert!(rendered.contains("reply"));
    }

    // ----- truncate_chars_keep_tail -----

    #[test]
    fn truncate_chars_keep_tail_is_noop_when_under_limit() {
        assert_eq!(truncate_chars_keep_tail("hello", 10), "hello");
    }

    #[test]
    fn truncate_chars_keep_tail_keeps_tail_with_marker_when_over_limit() {
        let s = "abcdefghijklmnopqrstuvwxyz"; // 26 chars
        let out = truncate_chars_keep_tail(s, 40); // > 26 → no-op
        assert_eq!(out, s);

        // Over limit: result must fit budget, end with tail, contain marker.
        let big: String = "0123456789".repeat(20); // 200 chars
        let out = truncate_chars_keep_tail(&big, 60);
        assert!(out.chars().count() <= 60);
        assert!(out.contains("earlier content truncated"));
        assert!(out.ends_with("9")); // tail of big ends with '9'
    }

    #[test]
    fn truncate_chars_keep_tail_handles_cjk_safely() {
        // Each CJK char is 3 bytes in UTF-8; keep_tail must walk by char
        // boundaries, not bytes, or it would panic mid-character.
        let cjk = "一二三四五六七八九十"; // 10 chars, 30 bytes
        let out = truncate_chars_keep_tail(cjk, 5);
        // Output is either just the 5-char tail (no marker) or a mix — but
        // must never panic and must not exceed the budget.
        assert!(out.chars().count() <= 5);
    }

    // ----- truncate_tool_result_head_tail -----

    #[test]
    fn truncate_tool_result_head_tail_noop_when_under_limit() {
        let text = "short tool output";
        assert_eq!(truncate_tool_result_head_tail(text, 100), text.to_string());
    }

    #[test]
    fn truncate_tool_result_head_tail_preserves_head_and_tail() {
        // 10_000 chars, budget 200 → head ~2/3 + tail ~1/3 of (200 - marker).
        let text: String = (0..10_000)
            .map(|i| char::from(b'A' + (i % 26) as u8))
            .collect();
        let out = truncate_tool_result_head_tail(&text, 200);
        assert!(out.chars().count() <= 200);
        // Must contain the omission marker.
        assert!(out.contains("chars omitted"));
        // Head starts with same chars as original.
        assert!(out.starts_with("ABCDE"));
        // Tail ends with same chars as original.
        let original_tail: String = text
            .chars()
            .rev()
            .take(10)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect();
        assert!(out.ends_with(&original_tail));
    }

    #[test]
    fn truncate_tool_result_head_tail_small_gap_falls_back_to_head_truncation() {
        // When the text is only slightly over budget, the omitted middle
        // section is tiny (< 50 chars). The function should skip the
        // head+tail gap marker and use simple head truncation instead.
        //
        // marker_len = 31, budget = 100, content_budget = 69,
        // head = 46, tail = 23, omitted = total - 69.
        // For omitted < 50: total < 69 + 50 = 119.
        // Use total = 110 → omitted = 110 - 69 = 41 < 50 → head fallback.
        let text = "x".repeat(110);
        let out = truncate_tool_result_head_tail(&text, 100);
        assert!(out.chars().count() <= 100);
        // Should NOT contain "chars omitted" (small gap → plain head truncation).
        assert!(
            !out.contains("chars omitted"),
            "small-gap case should use plain head truncation, got: {}",
            out
        );
        // Should contain the generic middle-omitted marker instead.
        assert!(out.contains("middle content omitted"));
    }

    #[test]
    fn truncate_tool_result_head_tail_handles_cjk() {
        // CJK chars: 3 bytes each. Must not panic on char boundary.
        let cjk = "你好世界测试数据".repeat(500); // 4000 chars
        let out = truncate_tool_result_head_tail(&cjk, 200);
        assert!(out.chars().count() <= 200);
        // Must start with original head.
        assert!(out.starts_with("你好世界"));
    }

    #[test]
    fn truncate_tool_result_head_tail_tiny_budget() {
        // Budget smaller than marker: just hard-truncate.
        let text = "abcdefghijklmnop";
        let out = truncate_tool_result_head_tail(text, 5);
        assert!(out.chars().count() <= 5);
    }

    #[test]
    fn render_compact_summary_history_applies_per_tool_result_cap() {
        // A massive tool result should be capped by SUMMARY_TOOL_RESULT_MAX_CHARS
        // even when the overall budget is much larger.
        use tiycore::types::ToolResultMessage;
        let big_content = "x".repeat(20_000);
        let history = vec![
            AgentMessage::User(UserMessage::text("do something")),
            AgentMessage::ToolResult(ToolResultMessage::text("tc-1", "read", &big_content, false)),
        ];

        let rendered = render_compact_summary_history(&history, 100_000);
        // The tool result body should be capped — the full 20K should NOT appear.
        assert!(!rendered.contains(&big_content));
        // But the header and some content should be present.
        assert!(rendered.contains("[tool_result] read"));
        // The overall rendered output should be well under the uncapped size.
        assert!(rendered.chars().count() < 20_000 + 500);
        // The tool result portion should respect SUMMARY_TOOL_RESULT_MAX_CHARS.
        let tool_section_start = rendered.find("[tool_result] read").unwrap();
        let tool_section = &rendered[tool_section_start..];
        // The tool section (header + body + trailing \n\n) should be bounded.
        assert!(
            tool_section.chars().count() <= SUMMARY_TOOL_RESULT_MAX_CHARS + 200,
            "tool section should be bounded by per-item cap + header overhead"
        );
    }

    // ----- summary_history_char_budget -----

    fn model_role_with(context_window: u32, reasoning: bool) -> ResolvedModelRole {
        let model = tiycore::types::Model::builder()
            .id("test-model")
            .name("test-model")
            .provider(tiycore::types::Provider::OpenAI)
            .base_url("https://api.openai.com/v1")
            .context_window(context_window)
            .max_tokens(32_000)
            .input(vec![tiycore::types::InputType::Text])
            .cost(tiycore::types::Cost::default())
            .reasoning(reasoning)
            .build()
            .expect("sample model");

        ResolvedModelRole {
            provider_id: "provider-test".to_string(),
            model_record_id: "record-test".to_string(),
            model_id: "test-model".to_string(),
            model_name: "test-model".to_string(),
            provider_type: "openai".to_string(),
            provider_name: "OpenAI".to_string(),
            api_key: None,
            provider_options: None,
            model,
        }
    }

    fn model_role_with_id(model_id: &str) -> ResolvedModelRole {
        let mut role = model_role_with(128_000, false);
        role.model_id = model_id.to_string();
        role.model_name = model_id.to_string();
        role.model = tiycore::types::Model::builder()
            .id(model_id)
            .name(model_id)
            .provider(tiycore::types::Provider::OpenAI)
            .base_url("https://api.openai.com/v1")
            .context_window(128_000)
            .max_tokens(32_000)
            .input(vec![tiycore::types::InputType::Text])
            .cost(tiycore::types::Cost::default())
            .reasoning(false)
            .build()
            .expect("sample model with custom id");
        role
    }

    #[test]
    fn build_title_model_candidates_prefers_priority_order() {
        let lightweight = model_role_with_id("lite");
        let auxiliary = model_role_with_id("aux");
        let primary = model_role_with_id("primary");

        let candidates =
            build_title_model_candidates(Some(&lightweight), Some(&auxiliary), Some(&primary));

        let ids: Vec<&str> = candidates
            .iter()
            .map(|role| role.model_id.as_str())
            .collect();
        assert_eq!(ids, vec!["lite", "aux", "primary"]);
    }

    #[test]
    fn build_title_model_candidates_skips_missing_entries() {
        let auxiliary = model_role_with_id("aux");
        let primary = model_role_with_id("primary");

        let candidates = build_title_model_candidates(None, Some(&auxiliary), Some(&primary));

        let ids: Vec<&str> = candidates
            .iter()
            .map(|role| role.model_id.as_str())
            .collect();
        assert_eq!(ids, vec!["aux", "primary"]);
    }

    #[test]
    fn build_title_model_candidates_returns_empty_when_all_missing() {
        let candidates = build_title_model_candidates(None, None, None);
        assert!(candidates.is_empty());
    }

    #[test]
    fn build_title_model_candidates_deduplicates_same_model_id() {
        let lightweight = model_role_with_id("shared");
        let auxiliary = model_role_with_id("shared");
        let primary = model_role_with_id("primary");

        let candidates =
            build_title_model_candidates(Some(&lightweight), Some(&auxiliary), Some(&primary));

        let ids: Vec<&str> = candidates
            .iter()
            .map(|role| role.model_id.as_str())
            .collect();
        assert_eq!(ids, vec!["shared", "primary"]);
    }

    #[test]
    fn title_prompt_from_messages_matches_conversation_language_when_none() {
        let messages = vec![MessageRecord {
            id: "msg-1".into(),
            thread_id: "thread-1".into(),
            run_id: None,
            role: "user".into(),
            content_markdown: "请帮我分析标题生成策略".into(),
            message_type: "plain_message".into(),
            status: "completed".into(),
            metadata_json: None,
            attachments_json: None,
            created_at: String::new(),
        }];

        let prompt =
            build_title_prompt_from_messages(&messages, None, ProfileResponseStyle::Balanced);

        assert!(prompt.contains("Match the conversation language."));
        assert!(prompt.contains("Keep the title clear and natural"));
    }

    #[test]
    fn title_prompt_from_messages_includes_concise_style_rule() {
        let messages = vec![MessageRecord {
            id: "msg-1".into(),
            thread_id: "thread-1".into(),
            run_id: None,
            role: "user".into(),
            content_markdown: "Need a short title".into(),
            message_type: "plain_message".into(),
            status: "completed".into(),
            metadata_json: None,
            attachments_json: None,
            created_at: String::new(),
        }];

        let prompt = build_title_prompt_from_messages(
            &messages,
            Some("English"),
            ProfileResponseStyle::Concise,
        );

        assert!(prompt.contains("Write the title in English."));
        assert!(prompt.contains("especially terse, direct, and low-friction"));
    }

    #[test]
    fn title_prompt_from_messages_includes_guide_style_rule() {
        let messages = vec![MessageRecord {
            id: "msg-1".into(),
            thread_id: "thread-1".into(),
            run_id: None,
            role: "assistant".into(),
            content_markdown: "Let's decide whether to keep fallback behavior.".into(),
            message_type: "plain_message".into(),
            status: "completed".into(),
            metadata_json: None,
            attachments_json: None,
            created_at: String::new(),
        }];

        let prompt = build_title_prompt_from_messages(
            &messages,
            Some("English"),
            ProfileResponseStyle::Guide,
        );

        assert!(prompt.contains("Write the title in English."));
        assert!(prompt.contains("signals the user's goal or decision focus clearly"));
    }

    #[test]
    fn summary_history_char_budget_zero_context_window_returns_floor() {
        // context_window = 0 → budget should collapse to the SUMMARY_HISTORY_MIN_CHARS
        // floor rather than to zero (which would produce useless LLM inputs).
        let role = model_role_with(0, false);
        assert_eq!(
            summary_history_char_budget(&role),
            SUMMARY_HISTORY_MIN_CHARS
        );
    }

    #[test]
    fn summary_history_char_budget_tiny_context_window_returns_floor() {
        // When context_window < output_tokens + overhead, saturating_sub
        // drives tokens_for_history to 0 and we must return the floor.
        let role = model_role_with(4_096, false); // < 8192 output + 3000 overhead
        assert_eq!(
            summary_history_char_budget(&role),
            SUMMARY_HISTORY_MIN_CHARS
        );
    }

    #[test]
    fn summary_history_char_budget_scales_with_context_window() {
        // 128K window → (128_000 - 8192 - 3000) * 4 = 466_432 chars.
        let role = model_role_with(128_000, false);
        let budget = summary_history_char_budget(&role);
        let expected = (128_000usize - 8_192 - 3_000).saturating_mul(4);
        assert_eq!(budget, expected);
        // Also sanity-check it's well above the floor.
        assert!(budget > SUMMARY_HISTORY_MIN_CHARS);
    }

    #[test]
    fn summary_history_char_budget_doubles_output_budget_for_reasoning_models() {
        // Reasoning models share the output slot with thinking tokens, so
        // we subtract PRIMARY_SUMMARY_MAX_TOKENS * 2 = 16_384 instead of 8_192.
        let reasoning = model_role_with(128_000, true);
        let non_reasoning = model_role_with(128_000, false);
        let reasoning_budget = summary_history_char_budget(&reasoning);
        let non_reasoning_budget = summary_history_char_budget(&non_reasoning);

        // Reasoning budget should be exactly 8_192 tokens smaller = 32_768 chars smaller.
        assert_eq!(
            non_reasoning_budget - reasoning_budget,
            8_192usize.saturating_mul(4)
        );
    }

    #[test]
    fn summary_history_char_budget_1m_context_window_has_no_upper_cap() {
        // Regression guard for the 400K-char cap removal. A 1M-context model
        // should get its full advertised capacity (minus overhead) as budget.
        let role = model_role_with(1_000_000, false);
        let budget = summary_history_char_budget(&role);
        let expected = (1_000_000usize - 8_192 - 3_000).saturating_mul(4);
        assert_eq!(budget, expected);
        // And must be well above any previous artificial cap.
        assert!(budget > 400_000);
    }
}

pub(crate) fn truncate_chars(value: &str, max_chars: usize) -> String {
    let truncated: String = value.chars().take(max_chars).collect();
    if value.chars().count() > max_chars {
        truncated.trim_end().to_string()
    } else {
        value.to_string()
    }
}

pub(crate) fn build_provider_options_payload_hook(
    provider_options: Option<serde_json::Value>,
) -> Option<OnPayloadFn> {
    let provider_options = provider_options?;

    Some(Arc::new(move |payload, _model| {
        let provider_options = provider_options.clone();
        Box::pin(async move {
            let mut merged = payload;
            merge_json_value(&mut merged, &provider_options);
            Some(merged)
        })
    }))
}

pub(crate) fn merge_json_value(base: &mut serde_json::Value, patch: &serde_json::Value) {
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
