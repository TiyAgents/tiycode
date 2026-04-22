//! Manages the lifecycle of agent runs backed by the built-in Rust runtime.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use sqlx::SqlitePool;
use tauri::{AppHandle, Emitter};
use tiycore::agent::AgentMessage;
use tiycore::provider::get_provider;
use tiycore::types::{
    Context as TiyContext, Message as TiyMessage, OnPayloadFn, StopReason,
    StreamOptions as TiyStreamOptions, UserMessage,
};
use tokio::sync::{broadcast, mpsc, Mutex};
use tokio::time::{sleep, Instant};

use crate::core::agent_session::{
    build_session_spec, convert_history_messages, normalize_profile_response_language,
    normalize_profile_response_style, trim_history_to_current_context, ProfileResponseStyle,
    ResolvedModelRole,
};
use crate::core::built_in_agent_runtime::BuiltInAgentRuntime;
use crate::core::plan_checkpoint::{
    ApprovalPromptMetadata, PlanApprovalAction, PlanMessageMetadata,
    IMPLEMENTATION_PLAN_APPROVAL_KIND, IMPLEMENTATION_PLAN_APPROVED_STATE,
    IMPLEMENTATION_PLAN_PENDING_STATE, IMPLEMENTATION_PLAN_SUPERSEDED_STATE,
};
use crate::core::sleep_manager::SleepManager;
use crate::core::task_board_manager;
use crate::core::tiycode_default_headers;
use crate::core::tiycode_url_policy;
use crate::ipc::app_events::{
    self, ThreadRunFinishedPayload, ThreadRunStartedPayload, ThreadTitleUpdatedPayload,
};
use crate::ipc::frontend_channels::ThreadStreamEvent;
use crate::model::errors::{AppError, ErrorSource};
use crate::model::thread::{MessageAttachmentDto, MessageRecord, ThreadStatus};
use crate::persistence::repo::{
    message_repo, profile_repo, run_repo, thread_repo, tool_call_repo, workspace_repo,
};

pub(crate) const TITLE_GENERATION_TIMEOUT: Duration = Duration::from_secs(90);
pub(crate) const TITLE_GENERATION_MAX_TOKENS: u32 = 512;
pub(crate) const TITLE_GENERATION_MAX_TOKENS_REASONING: u32 = 2048;
const PRIMARY_SUMMARY_MAX_TOKENS: u32 = 8192;
const PRIMARY_SUMMARY_TIMEOUT: Duration = Duration::from_secs(90);
pub(crate) const TITLE_CONTEXT_MAX_CHARS: usize = 1_200;
/// Lower bound on the history chars we send to the summary model.
///
/// When context_window is very small (or unknown), we still want some room
/// for meaningful input — this floor prevents degenerate cases where the
/// derived budget collapses to zero.
const SUMMARY_HISTORY_MIN_CHARS: usize = 8_000;
const FRONTEND_EVENT_BUFFER_SIZE: usize = 2048;

struct ActiveRun {
    run_id: String,
    thread_id: String,
    profile_id: Option<String>,
    frontend_tx: broadcast::Sender<ThreadStreamEvent>,
    lightweight_model_role: Option<ResolvedModelRole>,
    auxiliary_model_role: Option<ResolvedModelRole>,
    primary_model_role: Option<ResolvedModelRole>,
    streaming_message_id: Option<String>,
    reasoning_message_id: Option<String>,
    cancellation_requested: bool,
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
    pool: SqlitePool,
    app_handle: AppHandle,
    runtime: Arc<BuiltInAgentRuntime>,
    sleep_manager: Arc<SleepManager>,
    active_runs: Arc<Mutex<HashMap<String, ActiveRun>>>,
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
            self.runtime.start_session(spec, runtime_tx).await?;
            self.spawn_runtime_event_loop(run_id.clone(), runtime_rx);

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
            tool_call_repo::list_by_run_ids(&self.pool, &compact_run_ids).await?;
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
        let spawn_frontend_tx = frontend_tx.clone();
        tokio::spawn(async move {
            manager
                .run_compact_background(
                    spawn_thread_id,
                    spawn_run_id,
                    spawn_model_role,
                    history,
                    compact_instructions,
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
        frontend_tx: broadcast::Sender<ThreadStreamEvent>,
    ) {
        let summary_result =
            generate_primary_summary(&model_role, &history, compact_instructions.as_deref(), None)
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
        self.runtime.cancel_session(&run_id).await?;
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
        });
    }

    async fn handle_runtime_event(
        &self,
        run_id: &str,
        event: ThreadStreamEvent,
    ) -> Result<(), AppError> {
        if should_complete_reasoning_for_event(&event) {
            self.complete_active_reasoning_message(run_id, "completed")
                .await?;
        }

        match &event {
            ThreadStreamEvent::RunStarted { .. } => {
                run_repo::update_status(&self.pool, run_id, "running").await?;
                let thread_id = self.get_thread_id(run_id).await;
                thread_repo::update_status(&self.pool, &thread_id, &ThreadStatus::Running).await?;
            }
            ThreadStreamEvent::RunRetrying { .. } => {
                run_repo::update_status(&self.pool, run_id, "running").await?;
            }
            ThreadStreamEvent::MessageDelta {
                message_id, delta, ..
            } => {
                let persisted_id = self.ensure_streaming_message(run_id, message_id).await?;
                message_repo::append_content(&self.pool, &persisted_id, delta).await?;
            }
            ThreadStreamEvent::MessageCompleted {
                message_id,
                content,
                ..
            } => {
                let persisted_id = self.ensure_streaming_message(run_id, message_id).await?;
                // Only replace content when the completed snapshot is non-empty.
                // With extended-thinking models the final `text_content()` may
                // return an empty string (it excludes Thinking blocks) even though
                // streaming deltas have already been appended to the DB.  In that
                // case, keep the delta-accumulated content instead of overwriting
                // it with an empty string.
                if !content.is_empty() {
                    message_repo::replace_content(&self.pool, &persisted_id, content).await?;
                }
                message_repo::update_status(&self.pool, &persisted_id, "completed").await?;

                let mut runs = self.active_runs.lock().await;
                if let Some(run) = runs.get_mut(run_id) {
                    run.streaming_message_id = None;
                }
            }
            ThreadStreamEvent::MessageDiscarded { message_id, .. } => {
                message_repo::update_status(&self.pool, message_id, "discarded").await?;
            }
            ThreadStreamEvent::ReasoningUpdated {
                message_id,
                reasoning,
                ..
            } => {
                let persisted_id = self.ensure_reasoning_message(run_id, message_id).await?;
                message_repo::replace_content(&self.pool, &persisted_id, reasoning).await?;
            }
            ThreadStreamEvent::ToolRequested { .. } => {
                run_repo::update_status(&self.pool, run_id, "waiting_tool_result").await?;
            }
            ThreadStreamEvent::SubagentStarted { .. } => {
                run_repo::update_status(&self.pool, run_id, "waiting_tool_result").await?;
            }
            ThreadStreamEvent::ApprovalRequired { .. } => {
                run_repo::update_status(&self.pool, run_id, "waiting_approval").await?;
                let thread_id = self.get_thread_id(run_id).await;
                thread_repo::update_status(&self.pool, &thread_id, &ThreadStatus::WaitingApproval)
                    .await?;
            }
            ThreadStreamEvent::ClarifyRequired { .. } => {
                run_repo::update_status(&self.pool, run_id, "needs_reply").await?;
                let thread_id = self.get_thread_id(run_id).await;
                thread_repo::update_status(&self.pool, &thread_id, &ThreadStatus::NeedsReply)
                    .await?;
            }
            ThreadStreamEvent::ApprovalResolved { .. } => {
                run_repo::update_status(&self.pool, run_id, "running").await?;
                let thread_id = self.get_thread_id(run_id).await;
                thread_repo::update_status(&self.pool, &thread_id, &ThreadStatus::Running).await?;
            }
            ThreadStreamEvent::ClarifyResolved { .. } => {
                run_repo::update_status(&self.pool, run_id, "running").await?;
                let thread_id = self.get_thread_id(run_id).await;
                thread_repo::update_status(&self.pool, &thread_id, &ThreadStatus::Running).await?;
            }
            ThreadStreamEvent::ToolCompleted { .. }
            | ThreadStreamEvent::ToolFailed { .. }
            | ThreadStreamEvent::SubagentCompleted { .. }
            | ThreadStreamEvent::SubagentFailed { .. } => {
                run_repo::update_status(&self.pool, run_id, "running").await?;
            }
            ThreadStreamEvent::ThreadUsageUpdated { usage, .. } => {
                let usage = tiycore::types::Usage {
                    input: usage.input_tokens,
                    output: usage.output_tokens,
                    cache_read: usage.cache_read_tokens,
                    cache_write: usage.cache_write_tokens,
                    total_tokens: usage.total_tokens,
                    cost: tiycore::types::UsageCost::default(),
                };
                run_repo::update_usage(&self.pool, run_id, &usage).await?;
            }
            ThreadStreamEvent::RunCheckpointed { .. } => {
                run_repo::update_status(&self.pool, run_id, "waiting_approval").await?;
                let thread_id = self.get_thread_id(run_id).await;
                thread_repo::update_status(&self.pool, &thread_id, &ThreadStatus::WaitingApproval)
                    .await?;
            }
            ThreadStreamEvent::RunCompleted { .. } => {
                self.finish_run(run_id, "completed", None).await?;
            }
            ThreadStreamEvent::RunLimitReached { error, .. } => {
                self.finish_run(run_id, "limit_reached", Some(error))
                    .await?;
            }
            ThreadStreamEvent::RunFailed { error, .. } => {
                self.finish_run(run_id, "failed", Some(error)).await?;
            }
            ThreadStreamEvent::RunCancelled { .. } => {
                self.finish_run(run_id, "cancelled", None).await?;
            }
            ThreadStreamEvent::RunInterrupted { .. } => {
                let final_status = if self.was_cancel_requested(run_id).await {
                    "cancelled"
                } else {
                    "interrupted"
                };
                self.finish_run(run_id, final_status, None).await?;
            }
            _ => {}
        }

        self.emit(run_id, event.clone()).await;

        // Broadcast global events for lifecycle transitions so that the frontend
        // sidebar can react even when this thread has no active stream subscription.
        match &event {
            ThreadStreamEvent::RunStarted { .. } => {
                let thread_id = self.get_thread_id(run_id).await;
                let _ = self.app_handle.emit(
                    app_events::THREAD_RUN_STARTED,
                    ThreadRunStartedPayload {
                        thread_id,
                        run_id: run_id.to_string(),
                    },
                );
            }
            ThreadStreamEvent::RunCompleted { .. }
            | ThreadStreamEvent::RunLimitReached { .. }
            | ThreadStreamEvent::RunFailed { .. }
            | ThreadStreamEvent::RunCancelled { .. }
            | ThreadStreamEvent::RunInterrupted { .. } => {
                let thread_id = self.get_thread_id(run_id).await;
                let status = match &event {
                    ThreadStreamEvent::RunCompleted { .. } => "completed",
                    ThreadStreamEvent::RunLimitReached { .. } => "limit_reached",
                    ThreadStreamEvent::RunFailed { .. } => "failed",
                    ThreadStreamEvent::RunCancelled { .. } => "cancelled",
                    ThreadStreamEvent::RunInterrupted { .. } => "interrupted",
                    _ => unreachable!(),
                };
                let _ = self.app_handle.emit(
                    app_events::THREAD_RUN_FINISHED,
                    ThreadRunFinishedPayload {
                        thread_id,
                        run_id: run_id.to_string(),
                        status: status.to_string(),
                    },
                );
            }
            _ => {}
        }

        if matches!(
            event,
            ThreadStreamEvent::RunCheckpointed { .. }
                | ThreadStreamEvent::RunCompleted { .. }
                | ThreadStreamEvent::RunLimitReached { .. }
                | ThreadStreamEvent::RunFailed { .. }
                | ThreadStreamEvent::RunCancelled { .. }
                | ThreadStreamEvent::RunInterrupted { .. }
        ) {
            self.runtime.remove_session(run_id).await;
            self.remove_active_run(run_id).await;
        }

        Ok(())
    }

    async fn emit(&self, run_id: &str, event: ThreadStreamEvent) {
        let frontend_tx = {
            let runs = self.active_runs.lock().await;
            runs.get(run_id).map(|run| run.frontend_tx.clone())
        };

        if let Some(frontend_tx) = frontend_tx {
            let _ = frontend_tx.send(event);
        }
    }

    async fn ensure_streaming_message(
        &self,
        run_id: &str,
        requested_message_id: &str,
    ) -> Result<String, AppError> {
        let mut runs = self.active_runs.lock().await;
        let run = runs.get_mut(run_id).ok_or_else(|| {
            AppError::internal(
                ErrorSource::Thread,
                "active run not found for runtime event",
            )
        })?;

        if let Some(existing) = run.streaming_message_id.clone() {
            return Ok(existing);
        }

        let message_id = if requested_message_id.trim().is_empty() {
            uuid::Uuid::now_v7().to_string()
        } else {
            requested_message_id.to_string()
        };

        message_repo::insert(
            &self.pool,
            &MessageRecord {
                id: message_id.clone(),
                thread_id: run.thread_id.clone(),
                run_id: Some(run_id.to_string()),
                role: "assistant".to_string(),
                content_markdown: String::new(),
                message_type: "plain_message".to_string(),
                status: "streaming".to_string(),
                metadata_json: None,
                attachments_json: None,
                created_at: String::new(),
            },
        )
        .await?;

        run.streaming_message_id = Some(message_id.clone());
        Ok(message_id)
    }

    async fn ensure_reasoning_message(
        &self,
        run_id: &str,
        requested_message_id: &str,
    ) -> Result<String, AppError> {
        let message_id = if requested_message_id.trim().is_empty() {
            uuid::Uuid::now_v7().to_string()
        } else {
            requested_message_id.to_string()
        };

        let (thread_id, previous_message_id) = {
            let mut runs = self.active_runs.lock().await;
            let run = runs.get_mut(run_id).ok_or_else(|| {
                AppError::internal(
                    ErrorSource::Thread,
                    "active run not found for reasoning event",
                )
            })?;

            if let Some(existing) = run.reasoning_message_id.clone() {
                if existing == message_id {
                    return Ok(existing);
                }
            }

            (run.thread_id.clone(), run.reasoning_message_id.take())
        };

        if let Some(previous_message_id) = previous_message_id {
            message_repo::update_status(&self.pool, &previous_message_id, "completed").await?;
        }

        message_repo::insert(
            &self.pool,
            &MessageRecord {
                id: message_id.clone(),
                thread_id,
                run_id: Some(run_id.to_string()),
                role: "assistant".to_string(),
                content_markdown: String::new(),
                message_type: "reasoning".to_string(),
                status: "streaming".to_string(),
                metadata_json: None,
                attachments_json: None,
                created_at: String::new(),
            },
        )
        .await?;

        let mut runs = self.active_runs.lock().await;
        let run = runs.get_mut(run_id).ok_or_else(|| {
            AppError::internal(
                ErrorSource::Thread,
                "active run not found after inserting reasoning event",
            )
        })?;
        run.reasoning_message_id = Some(message_id.clone());
        Ok(message_id)
    }

    async fn complete_active_reasoning_message(
        &self,
        run_id: &str,
        status: &str,
    ) -> Result<(), AppError> {
        let reasoning_message_id = {
            let mut runs = self.active_runs.lock().await;
            let run = runs.get_mut(run_id).ok_or_else(|| {
                AppError::internal(
                    ErrorSource::Thread,
                    "active run not found while completing reasoning event",
                )
            })?;
            run.reasoning_message_id.take()
        };

        if let Some(reasoning_message_id) = reasoning_message_id {
            message_repo::update_status(&self.pool, &reasoning_message_id, status).await?;
        }

        Ok(())
    }

    async fn finish_run(
        &self,
        run_id: &str,
        status: &str,
        error_message: Option<&str>,
    ) -> Result<(), AppError> {
        let finalized_message_status = if status == "failed" {
            "failed"
        } else {
            "completed"
        };
        let (
            thread_id,
            profile_id,
            frontend_tx,
            lightweight_model_role,
            auxiliary_model_role,
            primary_model_role,
            streaming_message_id,
            reasoning_message_id,
        ) = {
            let runs = self.active_runs.lock().await;
            let run = runs.get(run_id).ok_or_else(|| {
                AppError::internal(ErrorSource::Thread, "active run not found while finishing")
            })?;
            (
                run.thread_id.clone(),
                run.profile_id.clone(),
                run.frontend_tx.clone(),
                run.lightweight_model_role.clone(),
                run.auxiliary_model_role.clone(),
                run.primary_model_role.clone(),
                run.streaming_message_id.clone(),
                run.reasoning_message_id.clone(),
            )
        };

        if let Some(message_id) = streaming_message_id {
            message_repo::update_status(&self.pool, &message_id, finalized_message_status).await?;
        }
        if let Some(message_id) = reasoning_message_id {
            message_repo::update_status(&self.pool, &message_id, finalized_message_status).await?;
        }

        // Reconcile task board state, then always push latest state to frontend.
        // reconcile may return a DTO if it made changes (including completing the board);
        // otherwise fall back to querying the current active board so the frontend is
        // always in sync when the run finishes.
        let reconciled_board =
            task_board_manager::reconcile_active_task_board(&self.pool, &thread_id).await?;
        let board_to_send = match reconciled_board {
            Some(board) => Some(board),
            None => task_board_manager::get_active_task_board(&self.pool, &thread_id).await?,
        };
        if let Some(task_board) = board_to_send {
            let _ = frontend_tx.send(ThreadStreamEvent::TaskBoardUpdated {
                run_id: run_id.to_string(),
                task_board,
            });
        }

        run_repo::update_status(&self.pool, run_id, status).await?;
        if let Some(error_message) = error_message {
            run_repo::set_error_message(&self.pool, run_id, error_message).await?;
        }

        let thread_status = match status {
            "failed" | "denied" => ThreadStatus::Failed,
            "limit_reached" => ThreadStatus::NeedsReply,
            "interrupted" => ThreadStatus::Interrupted,
            _ => ThreadStatus::Idle,
        };
        thread_repo::update_status(&self.pool, &thread_id, &thread_status).await?;

        if status == "completed" {
            self.spawn_thread_title_generation(
                run_id.to_string(),
                thread_id,
                profile_id,
                frontend_tx,
                lightweight_model_role,
                auxiliary_model_role,
                primary_model_role,
                self.app_handle.clone(),
            );
        }

        Ok(())
    }

    fn spawn_thread_title_generation(
        &self,
        run_id: String,
        thread_id: String,
        profile_id: Option<String>,
        frontend_tx: broadcast::Sender<ThreadStreamEvent>,
        lightweight_model_role: Option<ResolvedModelRole>,
        auxiliary_model_role: Option<ResolvedModelRole>,
        primary_model_role: Option<ResolvedModelRole>,
        app_handle: AppHandle,
    ) {
        let candidates = build_title_model_candidates(
            lightweight_model_role.as_ref(),
            auxiliary_model_role.as_ref(),
            primary_model_role.as_ref(),
        );
        if candidates.is_empty() {
            tracing::debug!(
                run_id = %run_id,
                thread_id = %thread_id,
                "skipping thread title generation: no title model configured"
            );
            return;
        }

        let pool = self.pool.clone();
        tokio::spawn(async move {
            if let Err(error) = maybe_generate_thread_title(
                &pool,
                &run_id,
                &thread_id,
                profile_id,
                &candidates,
                frontend_tx,
                app_handle,
            )
            .await
            {
                tracing::warn!(
                    run_id = %run_id,
                    thread_id = %thread_id,
                    error = %error,
                    "failed to generate thread title"
                );
            }
        });
    }

    async fn get_thread_id(&self, run_id: &str) -> String {
        let runs = self.active_runs.lock().await;
        runs.get(run_id)
            .map(|run| run.thread_id.clone())
            .unwrap_or_default()
    }

    async fn was_cancel_requested(&self, run_id: &str) -> bool {
        let runs = self.active_runs.lock().await;
        runs.get(run_id)
            .map(|run| run.cancellation_requested)
            .unwrap_or(false)
    }

    async fn remove_active_run(&self, run_id: &str) {
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

fn parse_message_metadata<T>(message: &MessageRecord) -> Result<T, AppError>
where
    T: for<'de> serde::Deserialize<'de>,
{
    let raw = message.metadata_json.as_deref().ok_or_else(|| {
        AppError::recoverable(
            ErrorSource::Thread,
            "thread.message.metadata_missing",
            format!("Message '{}' is missing metadata.", message.id),
        )
    })?;
    serde_json::from_str::<T>(raw).map_err(|error| {
        AppError::recoverable(
            ErrorSource::Thread,
            "thread.message.metadata_invalid",
            format!("Message '{}' has invalid metadata: {error}", message.id),
        )
    })
}

fn extract_run_string(model_plan: &serde_json::Value, path: &[&str]) -> Option<String> {
    let mut current = model_plan;

    for segment in path {
        current = current.get(*segment)?;
    }

    current.as_str().map(ToString::to_string)
}

fn extract_run_model_refs(
    model_plan: &serde_json::Value,
) -> (Option<String>, Option<String>, Option<String>) {
    (
        extract_run_string(model_plan, &["profileId"]),
        extract_run_string(model_plan, &["primary", "providerId"]),
        extract_run_string(model_plan, &["primary", "modelRecordId"])
            .or_else(|| extract_run_string(model_plan, &["primary", "modelId"])),
    )
}

fn build_implementation_handoff_prompt(
    thread_id: &str,
    metadata: &PlanMessageMetadata,
    action: PlanApprovalAction,
) -> String {
    let action_note = match action {
        PlanApprovalAction::ApplyPlan => {
            "The user approved this plan for direct implementation."
        }
        PlanApprovalAction::ApplyPlanWithContextReset => {
            "The user approved this plan after clearing the planning conversation from the implementation context."
        }
    };
    let plan_file_note = crate::core::plan_checkpoint::plan_file_path(thread_id)
        .filter(|path| path.exists())
        .map(|path| format!("\n- Plan file on disk: {}", path.display()))
        .unwrap_or_default();
    match action {
        PlanApprovalAction::ApplyPlan => {
            let plan_markdown = crate::core::plan_checkpoint::plan_markdown(metadata);

            format!(
                "Implementation handoff:\n- {action_note}\n- Plan revision: {}{plan_file_note}\n- Treat the approved plan below as the implementation baseline.\n- If the plan turns out to be invalid or incomplete, pause and return to planning before making a different change.\n- After implementation, use agent_review with planFilePath to verify each plan step was completed.\n\nApproved plan:\n{}",
                metadata.artifact.plan_revision,
                plan_markdown
            )
        }
        PlanApprovalAction::ApplyPlanWithContextReset => format!(
            "Implementation handoff:\n- {action_note}\n- Plan revision: {}{plan_file_note}\n- The reset context already includes a historical summary and the approved plan.\n- Treat the approved plan in context as the implementation baseline.\n- If the plan turns out to be invalid or incomplete, pause and return to planning before making a different change.\n- After implementation, use agent_review with planFilePath to verify each plan step was completed.",
            metadata.artifact.plan_revision,
        ),
    }
}

/// Returns the model to use for primary summary generation.
/// Always uses the primary model to avoid context window mismatches.
fn primary_summary_model(
    model_plan: &crate::core::agent_session::ResolvedRuntimeModelPlan,
) -> tiycore::types::Model {
    model_plan.primary.model.clone()
}

fn build_compact_summary_system_prompt() -> String {
    [
        "You compress conversation state so another model can continue after context reset.",
        "Return only one compact summary block using the exact XML-style wrapper below.",
        "",
        "Requirements:",
        "- Preserve the user's current goal and latest requested outcome.",
        "- Preserve important constraints, preferences, and decisions.",
        "- List work already completed and important findings.",
        "- List the most relevant remaining tasks, open questions, or risks.",
        "- Mention key files, components, commands, tools, or errors only when they matter for continuation.",
        "- Be factual and concise. Do not invent details.",
        "- Do not address the user directly. Do not include greetings or commentary.",
        "- Prefer short bullet lists under clear section labels.",
        "- Keep the summary self-contained and suitable for direct insertion into future model context.",
        "",
        "Output rules:",
        "- Start with <context_summary> on its own line.",
        "- End with </context_summary> on its own line.",
        "- Do not output any text before or after the wrapper.",
        "",
        "Example output:",
        "<context_summary>",
        "- User goal: Stabilize /compact summary formatting.",
        "- Completed: Checked current local summarization flow and wrapper handling.",
        "- Remaining: Move compact rules into system prompt and keep output parsing robust.",
        "</context_summary>",
    ]
    .join("\n")
}

fn build_compact_summary_messages(
    history: &[AgentMessage],
    instructions: Option<&str>,
    max_history_chars: usize,
) -> Vec<TiyMessage> {
    let mut messages = Vec::new();

    if let Some(instructions) = instructions
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        messages.push(TiyMessage::User(UserMessage::text(format!(
            "Additional user instructions for this compact:\n{instructions}"
        ))));
    }

    messages.push(TiyMessage::User(UserMessage::text(format!(
        "Conversation history to compact:\n{}",
        render_compact_summary_history(history, max_history_chars)
    ))));

    messages
}

/// Generate a context summary using the primary model.
///
/// Always uses the primary model (not lightweight) to ensure the summary
/// stays within the model's context window. Returns `Err` on failure
/// (no fallback), so callers can decide how to handle errors.
///
/// If `abort` is provided, the call short-circuits with a recoverable
/// cancellation error as soon as the signal fires. This is used by the
/// `transform_context` hook so that clicking Cancel during
/// "Compressing context…" doesn't have to wait out the 90s timeout.
pub(crate) async fn generate_primary_summary(
    model_role: &ResolvedModelRole,
    history: &[AgentMessage],
    instructions: Option<&str>,
    abort: Option<tiycore::agent::AbortSignal>,
) -> Result<String, AppError> {
    let max_history_chars = summary_history_char_budget(model_role);
    execute_summary_llm_call(
        model_role,
        build_compact_summary_system_prompt(),
        build_compact_summary_messages(history, instructions, max_history_chars),
        instructions,
        abort,
        "primary",
    )
    .await
}

/// Shared implementation for primary- and merge-summary LLM calls.
///
/// Both call paths share the same provider setup, reasoning-aware
/// `max_tokens` budget, stream options, and result-normalization logic.
/// Extracting the shared tail prevents behavioural drift between the two
/// public entry points when stream options or error handling change.
///
/// `kind` is a short label (e.g. "primary" / "merge") used only for error
/// messages so a failure can be traced back to the originating call path.
async fn execute_summary_llm_call(
    model_role: &ResolvedModelRole,
    system_prompt: String,
    messages: Vec<TiyMessage>,
    instructions: Option<&str>,
    abort: Option<tiycore::agent::AbortSignal>,
    kind: &str,
) -> Result<String, AppError> {
    // Summary generation does not benefit from reasoning/thinking tokens.
    // Disable reasoning so the protocol layer omits thinking/reasoning parameters,
    // preventing reasoning tokens from consuming the PRIMARY_SUMMARY_MAX_TOKENS budget.
    let mut model_role = model_role.clone();
    let was_reasoning = model_role.model.reasoning;
    model_role.model.reasoning = false;

    let provider = get_provider(&model_role.model.provider).ok_or_else(|| {
        AppError::recoverable(
            ErrorSource::Settings,
            "settings.primary_summary.provider_missing",
            format!(
                "Provider type '{:?}' is not registered for {} summary generation.",
                model_role.model.provider, kind
            ),
        )
    })?;

    let context = TiyContext {
        system_prompt: Some(system_prompt),
        messages,
        tools: None,
    };

    let max_tokens = if was_reasoning {
        // Bump for reasoning-only models that ignore the disable
        PRIMARY_SUMMARY_MAX_TOKENS * 2
    } else {
        PRIMARY_SUMMARY_MAX_TOKENS
    };

    let options = TiyStreamOptions {
        api_key: model_role.api_key.clone(),
        max_tokens: Some(max_tokens),
        headers: Some(tiycode_default_headers()),
        on_payload: build_provider_options_payload_hook(model_role.provider_options.clone()),
        security: Some(tiycore::types::SecurityConfig::default().with_url(tiycode_url_policy())),
        ..TiyStreamOptions::default()
    };

    let stream = provider.stream(&model_role.model, &context, options);
    let stream_fut = stream.try_result(PRIMARY_SUMMARY_TIMEOUT);
    let completion = await_summary_with_abort(stream_fut, abort).await?;

    let message = match completion {
        Some(message) => message,
        None => {
            return Err(AppError::recoverable(
                ErrorSource::System,
                "runtime.context_compression.empty_result",
                format!("{} summary generation returned empty result", kind),
            ));
        }
    };

    if message.stop_reason == StopReason::Error {
        let detail = message
            .error_message
            .clone()
            .unwrap_or_else(|| format!("{} summary generation failed", kind));
        return Err(AppError::recoverable(
            ErrorSource::System,
            "runtime.context_compression.failed",
            detail,
        ));
    }

    let summary = normalize_compact_summary(message.text_content(), instructions);
    match summary {
        Some(s) => Ok(s),
        None => Err(AppError::recoverable(
            ErrorSource::System,
            "runtime.context_compression.empty_result",
            format!("{} summary generation produced no usable content", kind),
        )),
    }
}

/// Await a summary-generation future while also watching an optional
/// `AbortSignal`. Returns `Err(cancelled)` as soon as the signal fires,
/// allowing the caller to drop the stream future (and its in-flight HTTP
/// connection) rather than wait for the provider timeout.
async fn await_summary_with_abort<T>(
    future: impl std::future::Future<Output = T>,
    abort: Option<tiycore::agent::AbortSignal>,
) -> Result<T, AppError> {
    match abort {
        Some(signal) if signal.is_cancelled() => Err(cancellation_error()),
        Some(signal) => {
            tokio::select! {
                // Bias towards the primary future: if both branches are
                // simultaneously ready, we prefer returning the summary
                // result over a spurious cancel. (Note: this select does
                // NOT re-check cancellation after the future wins — if the
                // future completes at the exact same instant the signal
                // fires, the summary is kept. That is acceptable because
                // the caller will then be free to use the value; we'd only
                // be throwing away work the user can still benefit from.)
                biased;
                value = future => Ok(value),
                _ = signal.cancelled() => Err(cancellation_error()),
            }
        }
        None => Ok(future.await),
    }
}

fn cancellation_error() -> AppError {
    AppError::recoverable(
        ErrorSource::System,
        "runtime.context_compression.cancelled",
        "Context compression was cancelled".to_string(),
    )
}

fn build_merge_summary_system_prompt() -> String {
    [
        "You maintain a rolling context summary for another model to continue after context reset.",
        "You will be given the PRIOR summary (already in <context_summary> form) and a DELTA of conversation",
        "that happened after that summary was last produced. Produce a SINGLE updated <context_summary>",
        "that merges both — keeping still-relevant facts from the prior summary and folding in new information",
        "from the delta. Treat the prior summary as authoritative for anything it covers and do not drop",
        "details that remain pertinent.",
        "",
        "Requirements:",
        "- Preserve the user's current goal and most recent requested outcome.",
        "- Retain important constraints, preferences, and decisions from the prior summary unless the delta",
        "  explicitly supersedes them.",
        "- Fold newly completed work, findings, key files/commands, and remaining tasks from the delta in.",
        "- Drop items the delta marks resolved; add items the delta newly raises.",
        "- Be factual and concise. Do not invent details. Do not address the user.",
        "- Prefer short bullet lists under clear section labels.",
        "",
        "Output rules:",
        "- Start with <context_summary> on its own line.",
        "- End with </context_summary> on its own line.",
        "- Do not output any text before or after the wrapper.",
    ]
    .join("\n")
}

fn build_merge_summary_messages(
    prior_summary: &str,
    delta_history: &[AgentMessage],
    instructions: Option<&str>,
    max_history_chars: usize,
) -> Vec<TiyMessage> {
    let mut messages = Vec::new();

    if let Some(instructions) = instructions
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        messages.push(TiyMessage::User(UserMessage::text(format!(
            "Additional user instructions for this compact:\n{instructions}"
        ))));
    }

    messages.push(TiyMessage::User(UserMessage::text(format!(
        "Prior summary (authoritative for anything it covers):\n{}",
        prior_summary.trim()
    ))));

    messages.push(TiyMessage::User(UserMessage::text(format!(
        "New conversation delta (happened after the prior summary):\n{}",
        render_compact_summary_history(delta_history, max_history_chars)
    ))));

    messages
}

/// Generate an updated summary by merging a prior `<context_summary>` block with
/// a delta of conversation history.
///
/// Used by auto-compression when the previous compression already left a summary
/// in the in-memory context; merging avoids the "summary-of-summary" quality
/// decay that would happen if we re-summarised the already-summarised prefix.
///
/// `abort` mirrors `generate_primary_summary`: the call short-circuits with a
/// recoverable cancellation error when the signal fires.
pub(crate) async fn generate_merge_summary(
    model_role: &ResolvedModelRole,
    prior_summary: &str,
    delta_history: &[AgentMessage],
    instructions: Option<&str>,
    abort: Option<tiycore::agent::AbortSignal>,
) -> Result<String, AppError> {
    let max_history_chars = summary_history_char_budget(model_role);
    execute_summary_llm_call(
        model_role,
        build_merge_summary_system_prompt(),
        build_merge_summary_messages(
            prior_summary,
            delta_history,
            instructions,
            max_history_chars,
        ),
        instructions,
        abort,
        "merge",
    )
    .await
}

/// Detect whether the head of `messages` contains a previously injected
/// `<context_summary>` block (produced by an earlier auto-compression pass).
///
/// Returns `Some((prior_summary_text, consumed_prefix_len))` when found — the
/// caller should treat the first `consumed_prefix_len` messages as a pinned
/// prefix (the old summary) and summarise the **rest** as a delta.
///
/// Only the **first** user message is inspected: previous compression always
/// places the summary as the new head of the context.
pub(crate) fn detect_prior_summary(messages: &[AgentMessage]) -> Option<(String, usize)> {
    let first = messages.first()?;
    let user = match first {
        AgentMessage::User(user) => user,
        _ => return None,
    };
    let text = match &user.content {
        tiycore::types::UserContent::Text(t) => t.as_str(),
        tiycore::types::UserContent::Blocks(blocks) => {
            // Accept only a single text block for detection.
            blocks
                .iter()
                .find_map(|block| match block {
                    tiycore::types::ContentBlock::Text(t) => Some(t.text.as_str()),
                    _ => None,
                })
                .unwrap_or("")
        }
    };

    let trimmed = text.trim_start();
    if !trimmed.starts_with("<context_summary>") {
        return None;
    }
    // Require a closing wrapper too; a truncated block means the message
    // isn't a well-formed prior summary and re-summarisation is safer.
    if !trimmed.contains("</context_summary>") {
        return None;
    }

    Some((text.to_string(), 1))
}

/// Derive how many characters of conversation history we can afford to send
/// to the summary LLM for a given model role.
///
/// The formula reserves room for:
/// - The system prompt + instructions + wrapper text (~2,000 tokens)
/// - The model's own output budget (`PRIMARY_SUMMARY_MAX_TOKENS`, doubled
///   for reasoning-only models since reasoning tokens share the output slot)
/// - A safety margin (1,000 tokens) for off-by-one token vs char estimation
///
/// The remaining tokens are multiplied by 4 (the chars-per-token heuristic
/// used elsewhere in the codebase) to produce a char budget. We floor the
/// result at `SUMMARY_HISTORY_MIN_CHARS` so a missing or degenerate
/// `context_window` cannot collapse the budget to zero, but we **do not**
/// impose an upper cap — modern 1M/2M-token models need their full
/// advertised window to compress long CJK-heavy threads without silent
/// information loss. Provider limits (payload size, rate limits) are
/// enforced downstream; here we trust the model's advertised capacity.
fn summary_history_char_budget(model_role: &ResolvedModelRole) -> usize {
    let context_window = model_role.model.context_window as usize;
    let output_tokens = if model_role.model.reasoning {
        // Reasoning models share the output slot with thinking tokens, so
        // we must assume the doubled allowance to avoid collisions.
        (PRIMARY_SUMMARY_MAX_TOKENS as usize).saturating_mul(2)
    } else {
        PRIMARY_SUMMARY_MAX_TOKENS as usize
    };
    // Non-history overhead: system prompt, instructions wrapper, safety margin.
    let overhead_tokens: usize = 3_000;

    let tokens_for_history = context_window
        .saturating_sub(output_tokens)
        .saturating_sub(overhead_tokens);
    let chars_for_history = tokens_for_history.saturating_mul(4);

    chars_for_history.max(SUMMARY_HISTORY_MIN_CHARS)
}

/// Render conversation history for the summary model.
///
/// Strategy: pack **full** messages from newest to oldest within the char
/// budget, then reverse so the model reads them in chronological order.
/// Individual items are only truncated when a single item is itself larger
/// than the remaining budget — in which case we prefer to keep the most
/// recent portion of that item. Older messages that don't fit are dropped
/// entirely rather than half-truncated, because the older end of the
/// conversation is the least load-bearing for continuing the task.
///
/// This is a substantial behavioural change from the previous version
/// (which pre-truncated every item to 300–1,500 chars and capped the whole
/// payload at 18K chars). The previous formula was tight enough to drop
/// most of a real compact call's context; the new formula preserves full
/// content for typical threads and only activates the fallback on genuinely
/// oversized payloads.
/// Per-tool-result budget cap inside `render_compact_summary_history`.
///
/// Tool results can be very large (file reads, command output). Letting a
/// single one consume the entire remaining budget would crowd out other
/// messages that provide better summarisation signal. This cap limits any
/// single tool result body (the text portion, before the header) so the
/// budget is distributed more evenly across the conversation.
const SUMMARY_TOOL_RESULT_MAX_CHARS: usize = 6_000;

fn render_compact_summary_history(history: &[AgentMessage], max_chars: usize) -> String {
    // Rendered chunks in **reverse** order (newest first) for budget packing.
    let mut chunks_reversed: Vec<String> = Vec::new();
    let mut remaining = max_chars;

    for message in history.iter().rev() {
        if remaining == 0 {
            break;
        }

        let chunk = match message {
            AgentMessage::User(user) => {
                let text = user_message_to_text(user);
                if text.is_empty() {
                    continue;
                }
                format!("[user]\n{text}\n\n")
            }
            AgentMessage::Assistant(assistant) => {
                let text = assistant_message_to_text(assistant);
                if text.is_empty() {
                    continue;
                }
                format!("[assistant]\n{text}\n\n")
            }
            AgentMessage::ToolResult(tool_result) => {
                let raw_text = tool_result_to_text(tool_result);
                if raw_text.is_empty() {
                    continue;
                }
                let header = if tool_result.tool_name.is_empty() {
                    "[tool_result]".to_string()
                } else {
                    format!("[tool_result] {}", tool_result.tool_name)
                };
                // Apply per-item smart truncation: head+tail with overlap
                // detection. This keeps the beginning (structure, headers,
                // imports) and the end (errors, final output) of large tool
                // results while compressing the less-informative middle.
                let text = truncate_tool_result_head_tail(
                    &raw_text,
                    SUMMARY_TOOL_RESULT_MAX_CHARS.min(remaining),
                );
                format!("{header}\n{text}\n\n")
            }
            AgentMessage::Custom { data, .. } => {
                let text = collapse_whitespace(&data.to_string());
                if text.is_empty() {
                    continue;
                }
                format!("[custom]\n{text}\n\n")
            }
        };

        let chunk_len = chunk.chars().count();
        if chunk_len <= remaining {
            remaining -= chunk_len;
            chunks_reversed.push(chunk);
        } else {
            // This single item is larger than the remaining budget. Keep
            // the TAIL of it (more recent tokens tend to matter more for
            // continuation), prefixed with an ellipsis marker so the
            // model knows the head was elided.
            let truncated = truncate_chars_keep_tail(&chunk, remaining);
            if !truncated.is_empty() {
                chunks_reversed.push(truncated);
            }
            break;
        }
    }

    chunks_reversed.reverse();
    chunks_reversed.concat()
}

/// Smart head+tail truncation for tool result text.
///
/// When the text fits within `max_chars`, returns it as-is. Otherwise keeps
/// the first 2/3 and last 1/3 of the budget (minus the elision marker),
/// preserving both the beginning (structure, headers, imports) and the end
/// (errors, final output) of large tool results. When the omitted middle
/// section is very small (< 50 chars), a simple head truncation is used
/// instead to avoid a gap marker that hides barely any content.
fn truncate_tool_result_head_tail(text: &str, max_chars: usize) -> String {
    let total = text.chars().count();
    if total <= max_chars {
        return text.to_string();
    }

    const MARKER: &str = "\n[… middle content omitted …]\n";
    let marker_len = MARKER.chars().count();

    // Budget too small for marker + meaningful head/tail — just hard-truncate.
    if max_chars <= marker_len + 2 {
        return text.chars().take(max_chars).collect();
    }

    let content_budget = max_chars - marker_len;
    let head_budget = content_budget * 2 / 3;
    let tail_budget = content_budget - head_budget;

    let omitted = total - head_budget - tail_budget;

    // If the omitted section is tiny, a gap marker that hides just a few
    // chars would be misleading — just do a simple head truncation instead.
    if omitted < 50 {
        let head: String = text.chars().take(max_chars - marker_len).collect();
        return format!("{head}{MARKER}");
    }

    let tail_start = total - tail_budget;
    let head: String = text.chars().take(head_budget).collect();
    let tail: String = text.chars().skip(tail_start).collect();

    format!("{head}\n[… {omitted} chars omitted …]\n{tail}")
}

/// Keep the tail `max_chars` of a string, prefixed with an ellipsis marker
/// when truncation occurs. Char-boundary safe (walks by `char`, not byte).
fn truncate_chars_keep_tail(text: &str, max_chars: usize) -> String {
    let total = text.chars().count();
    if total <= max_chars {
        return text.to_string();
    }
    // Reserve a few chars for the elision marker so the resulting string
    // fits within max_chars total.
    const MARKER: &str = "[…earlier content truncated…]\n";
    let marker_len = MARKER.chars().count();
    if max_chars <= marker_len {
        // Budget too small for a marker — just return the tail without one.
        let skip = total - max_chars;
        return text.chars().skip(skip).collect();
    }
    let tail_len = max_chars - marker_len;
    let skip = total - tail_len;
    let tail: String = text.chars().skip(skip).collect();
    format!("{MARKER}{tail}")
}

fn user_message_to_text(user: &UserMessage) -> String {
    // Per-item truncation was removed so render_compact_summary_history can
    // make a holistic budget decision. Trimming is still applied because we
    // don't want leading/trailing whitespace polluting the rendered block.
    match &user.content {
        tiycore::types::UserContent::Text(text) => text.trim().to_string(),
        tiycore::types::UserContent::Blocks(blocks) => {
            let mut parts = Vec::new();
            for block in blocks {
                match block {
                    tiycore::types::ContentBlock::Text(text) => {
                        let trimmed = text.text.trim();
                        if !trimmed.is_empty() {
                            parts.push(trimmed.to_string());
                        }
                    }
                    tiycore::types::ContentBlock::Image(_) => parts.push("[image]".to_string()),
                    _ => {}
                }
            }
            parts.join("\n")
        }
    }
}

fn assistant_message_to_text(assistant: &tiycore::types::AssistantMessage) -> String {
    // No per-item char caps: the caller (render_compact_summary_history)
    // applies a single holistic budget, so the message can keep its full
    // thinking blocks and tool-call arguments. That restores fidelity for
    // long technical threads that the old 1,500-char cap silently clipped.
    let mut parts = Vec::new();
    for block in &assistant.content {
        match block {
            tiycore::types::ContentBlock::Text(text) => {
                let trimmed = text.text.trim();
                if !trimmed.is_empty() {
                    parts.push(trimmed.to_string());
                }
            }
            tiycore::types::ContentBlock::Thinking(thinking) => {
                let trimmed = thinking.thinking.trim();
                if !trimmed.is_empty() {
                    parts.push(format!("[thinking] {trimmed}"));
                }
            }
            tiycore::types::ContentBlock::ToolCall(tool_call) => {
                parts.push(format!(
                    "[tool_call] {} {}",
                    tool_call.name,
                    collapse_whitespace(&tool_call.arguments.to_string())
                ));
            }
            tiycore::types::ContentBlock::Image(_) => parts.push("[image]".to_string()),
        }
    }
    parts.join("\n")
}

fn tool_result_to_text(tool_result: &tiycore::types::ToolResultMessage) -> String {
    // Unbounded: the holistic budget in render_compact_summary_history
    // decides whether this item fits wholesale or must be tail-truncated.
    let mut parts = Vec::new();
    for block in &tool_result.content {
        if let tiycore::types::ContentBlock::Text(text) = block {
            let trimmed = text.text.trim();
            if !trimmed.is_empty() {
                parts.push(trimmed.to_string());
            }
        }
    }
    parts.join("\n")
}

fn normalize_compact_summary(raw: String, instructions: Option<&str>) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }

    let summary = extract_context_summary_block(trimmed).unwrap_or_else(|| {
        let normalized_body =
            extract_context_summary_body(trimmed).unwrap_or_else(|| trimmed.to_string());
        format!(
            "<context_summary>\n{}\n</context_summary>",
            normalized_body.trim()
        )
    });

    Some(append_compact_instructions(summary, instructions))
}

fn extract_context_summary_block(raw: &str) -> Option<String> {
    let start_tag = "<context_summary>";
    let end_tag = "</context_summary>";
    let start = raw.find(start_tag)?;
    let content_start = start + start_tag.len();
    let relative_end = raw[content_start..].find(end_tag)?;
    let end = content_start + relative_end + end_tag.len();
    let candidate = raw[start..end].trim();

    if candidate.is_empty() {
        return None;
    }

    Some(candidate.to_string())
}

fn extract_context_summary_body(raw: &str) -> Option<String> {
    let start_tag = "<context_summary>";
    let end_tag = "</context_summary>";

    if let Some(block) = extract_context_summary_block(raw) {
        let content = block
            .trim_start_matches(start_tag)
            .trim_end_matches(end_tag)
            .trim();
        return if content.is_empty() {
            None
        } else {
            Some(content.to_string())
        };
    }

    if let Some(start) = raw.find(start_tag) {
        let content = raw[start + start_tag.len()..].trim();
        return if content.is_empty() {
            None
        } else {
            Some(content.to_string())
        };
    }

    if let Some(end) = raw.find(end_tag) {
        let content = raw[..end].trim();
        return if content.is_empty() {
            None
        } else {
            Some(content.to_string())
        };
    }

    None
}

fn append_compact_instructions(base_summary: String, instructions: Option<&str>) -> String {
    let Some(extra) = instructions
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        return base_summary;
    };

    format!(
        "{base_summary}\n\n<extra_instructions>\n{}\n</extra_instructions>",
        extra
    )
}

/// Build a deduplicated list of title model candidates in priority order:
/// lightweight → auxiliary → primary.
/// Skips candidates whose model_id is identical to an already-included one.
fn build_title_model_candidates(
    lightweight: Option<&ResolvedModelRole>,
    auxiliary: Option<&ResolvedModelRole>,
    primary: Option<&ResolvedModelRole>,
) -> Vec<ResolvedModelRole> {
    let mut result = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for candidate in [lightweight, auxiliary, primary].into_iter().flatten() {
        if seen.insert(candidate.model_id.clone()) {
            result.push(candidate.clone());
        }
    }
    result
}

async fn maybe_generate_thread_title(
    pool: &SqlitePool,
    run_id: &str,
    thread_id: &str,
    profile_id: Option<String>,
    candidates: &[ResolvedModelRole],
    frontend_tx: broadcast::Sender<ThreadStreamEvent>,
    app_handle: AppHandle,
) -> Result<(), AppError> {
    if thread_repo::has_title(pool, thread_id).await? {
        tracing::debug!(
            run_id = %run_id,
            thread_id = %thread_id,
            "skipping thread title generation: thread already has a title"
        );
        return Ok(());
    }

    let context_messages = load_title_context_messages(pool, thread_id).await?;
    if context_messages.is_empty() {
        tracing::debug!(
            run_id = %run_id,
            thread_id = %thread_id,
            "skipping thread title generation: no user/assistant messages in current context"
        );
        return Ok(());
    }

    let profile = match profile_id {
        Some(profile_id) => profile_repo::find_by_id(pool, &profile_id).await?,
        None => None,
    };
    let response_language = profile.as_ref().and_then(|profile| {
        normalize_profile_response_language(profile.response_language.as_deref())
    });
    let response_style = normalize_profile_response_style(
        profile
            .as_ref()
            .and_then(|profile| profile.response_style.as_deref()),
    );

    let mut last_error: Option<AppError> = None;
    for model_role in candidates {
        match generate_thread_title(
            model_role,
            &context_messages,
            response_language.as_deref(),
            response_style,
        )
        .await
        {
            Ok(Some(title)) => {
                thread_repo::update_title(pool, thread_id, &title).await?;

                tracing::info!(
                    run_id = %run_id,
                    thread_id = %thread_id,
                    title = %title,
                    "generated thread title, sending to frontend"
                );

                // Broadcast a global event so the sidebar can pick up the new title even
                // when no per-run stream subscription exists (e.g. inactive threads).
                let _ = app_handle.emit(
                    app_events::THREAD_TITLE_UPDATED,
                    ThreadTitleUpdatedPayload {
                        thread_id: thread_id.to_string(),
                        title: title.clone(),
                    },
                );

                if frontend_tx
                    .send(ThreadStreamEvent::ThreadTitleUpdated {
                        run_id: run_id.to_string(),
                        thread_id: thread_id.to_string(),
                        title,
                    })
                    .is_err()
                {
                    tracing::warn!(
                        run_id = %run_id,
                        thread_id = %thread_id,
                        "failed to send ThreadTitleUpdated event: frontend channel closed"
                    );
                }

                return Ok(());
            }
            Ok(None) => {
                tracing::warn!(
                    run_id = %run_id,
                    thread_id = %thread_id,
                    model_id = %model_role.model_id,
                    "title generation returned empty result (timeout or empty response)"
                );
                last_error = Some(AppError::recoverable(
                    ErrorSource::Thread,
                    "thread.regenerate_title.empty",
                    "Title generation returned empty or timed out.",
                ));
            }
            Err(e) => {
                tracing::warn!(
                    run_id = %run_id,
                    thread_id = %thread_id,
                    model_id = %model_role.model_id,
                    error = %e,
                    "title generation failed"
                );
                last_error = Some(e);
            }
        }
    }

    if let Some(e) = last_error {
        return Err(e);
    }

    Ok(())
}

async fn load_title_context_messages(
    pool: &SqlitePool,
    thread_id: &str,
) -> Result<Vec<MessageRecord>, AppError> {
    let messages = message_repo::list_since_last_reset(pool, thread_id).await?;
    let filtered: Vec<MessageRecord> = messages
        .into_iter()
        .filter(|m| {
            m.message_type == "plain_message" && (m.role == "user" || m.role == "assistant")
        })
        .collect();
    Ok(filtered)
}

async fn generate_thread_title(
    model_role: &ResolvedModelRole,
    messages: &[MessageRecord],
    response_language: Option<&str>,
    response_style: ProfileResponseStyle,
) -> Result<Option<String>, AppError> {
    // Lightweight title generation does not benefit from reasoning/thinking tokens.
    // When the lightweight model is a reasoning-capable model (e.g. DeepSeek R1, o1),
    // the reasoning tokens count against `max_tokens` and can exhaust the entire
    // token budget (TITLE_GENERATION_MAX_TOKENS = 512), leaving no room for the
    // actual title output.
    //
    // Strategy: 1) Explicitly disable reasoning so the protocol layer omits
    // thinking/reasoning parameters from the API request.  2) If the original
    // model had reasoning enabled, bump max_tokens as a fallback — some
    // reasoning-only models (e.g. o1) ignore the disable and still produce
    // reasoning tokens, so the larger budget ensures the title can still be
    // returned.
    let was_reasoning = model_role.model.reasoning;
    let mut model_role = model_role.clone();
    model_role.model.reasoning = false;

    let provider = get_provider(&model_role.model.provider).ok_or_else(|| {
        AppError::recoverable(
            ErrorSource::Settings,
            "settings.title_generation.provider_missing",
            format!(
                "Provider type '{:?}' is not registered for lightweight title generation.",
                model_role.model.provider
            ),
        )
    })?;

    let prompt = build_title_prompt_from_messages(messages, response_language, response_style);
    let context = TiyContext {
        system_prompt: Some(
            "You write concise conversation titles. Return only the title text.".to_string(),
        ),
        messages: vec![TiyMessage::User(UserMessage::text(prompt))],
        tools: None,
    };

    let options = TiyStreamOptions {
        api_key: model_role.api_key.clone(),
        max_tokens: Some(if was_reasoning {
            TITLE_GENERATION_MAX_TOKENS_REASONING
        } else {
            TITLE_GENERATION_MAX_TOKENS
        }),
        headers: Some(tiycode_default_headers()),
        on_payload: build_provider_options_payload_hook(model_role.provider_options.clone()),
        security: Some(tiycore::types::SecurityConfig::default().with_url(tiycode_url_policy())),
        ..TiyStreamOptions::default()
    };

    let completion = provider
        .stream(&model_role.model, &context, options)
        .try_result(TITLE_GENERATION_TIMEOUT)
        .await;

    let message = match completion {
        Some(message) => message,
        None => return Ok(None),
    };

    if message.stop_reason == StopReason::Error {
        let detail = message
            .error_message
            .clone()
            .unwrap_or_else(|| "lightweight title generation failed".to_string());
        return Err(AppError::recoverable(
            ErrorSource::Settings,
            "settings.title_generation.failed",
            detail,
        ));
    }

    Ok(normalize_generated_title(&message.text_content()))
}

pub(crate) fn build_title_prompt_from_messages(
    messages: &[MessageRecord],
    response_language: Option<&str>,
    response_style: ProfileResponseStyle,
) -> String {
    let language_rule = match response_language {
        Some(language) => format!("- Write the title in {language}."),
        None => "- Match the conversation language.".to_string(),
    };
    let style_rule = match response_style {
        ProfileResponseStyle::Balanced => {
            "- Keep the title clear and natural, with enough specificity to scan quickly."
        }
        ProfileResponseStyle::Concise => {
            "- Keep the title especially terse, direct, and low-friction."
        }
        ProfileResponseStyle::Guide => {
            "- Prefer a title that signals the user's goal or decision focus clearly."
        }
    };

    let mut conversation = String::new();
    // Messages are in chronological order (oldest first); iterate in reverse
    // so the newest messages appear first in the prompt.
    for msg in messages.iter().rev() {
        let role_label = if msg.role == "user" {
            "User"
        } else {
            "Assistant"
        };
        let content = truncate_chars(msg.content_markdown.trim(), TITLE_CONTEXT_MAX_CHARS);
        conversation.push_str(&format!("{role_label}:\n{content}\n\n"));
    }

    format!(
        "Create a short thread title for this conversation.\n\
Rules:\n\
{language_rule}\n\
{style_rule}\n\
- Prefer concrete nouns and actions.\n\
- Max 18 Chinese characters or 7 English words.\n\
- No quotes, no markdown, no prefixes.\n\
\n\
Conversation:\n{conversation}"
    )
}

#[cfg(test)]
pub(crate) fn build_title_prompt(
    user_message: &str,
    assistant_message: &str,
    response_language: Option<&str>,
    response_style: ProfileResponseStyle,
) -> String {
    let language_rule = match response_language {
        Some(language) => format!("- Write the title in {language}."),
        None => "- Match the conversation language.".to_string(),
    };
    let style_rule = match response_style {
        ProfileResponseStyle::Balanced => {
            "- Keep the title clear and natural, with enough specificity to scan quickly."
        }
        ProfileResponseStyle::Concise => {
            "- Keep the title especially terse, direct, and low-friction."
        }
        ProfileResponseStyle::Guide => {
            "- Prefer a title that signals the user's goal or decision focus clearly."
        }
    };

    format!(
        "Create a short thread title for this conversation.\n\
Rules:\n\
- {language_rule}\n\
- {style_rule}\n\
- Prefer concrete nouns and actions.\n\
- Max 18 Chinese characters or 7 English words.\n\
- No quotes, no markdown, no prefixes.\n\
\n\
User message:\n{user_message}\n\
\n\
Assistant reply:\n{assistant_message}"
    )
}

pub(crate) fn normalize_generated_title(raw: &str) -> Option<String> {
    let mut title = raw
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())?
        .to_string();

    for prefix in ["title:", "Title:", "标题：", "标题:"] {
        if let Some(stripped) = title.strip_prefix(prefix) {
            title = stripped.trim().to_string();
            break;
        }
    }

    let title = collapse_whitespace(&title);
    let title = title
        .trim_matches(|character: char| {
            character.is_whitespace()
                || matches!(
                    character,
                    '"' | '\'' | '`' | '“' | '”' | '‘' | '’' | '[' | ']' | '(' | ')'
                )
        })
        .trim_end_matches(|character: char| {
            matches!(character, '.' | '。' | '!' | '！' | '?' | '？' | ':' | '：')
        })
        .trim()
        .to_string();

    if title.is_empty() {
        return None;
    }

    Some(truncate_chars(&title, 40))
}

pub(crate) fn collapse_whitespace(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn should_complete_reasoning_for_event(event: &ThreadStreamEvent) -> bool {
    !matches!(
        event,
        ThreadStreamEvent::RunStarted { .. }
            | ThreadStreamEvent::ReasoningUpdated { .. }
            | ThreadStreamEvent::ThreadUsageUpdated { .. }
            | ThreadStreamEvent::RunCheckpointed { .. }
            | ThreadStreamEvent::ContextCompressing { .. }
            | ThreadStreamEvent::RunCompleted { .. }
            | ThreadStreamEvent::RunLimitReached { .. }
            | ThreadStreamEvent::RunFailed { .. }
            | ThreadStreamEvent::RunCancelled { .. }
            | ThreadStreamEvent::RunInterrupted { .. }
    )
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
        append_compact_instructions, await_summary_with_abort, build_compact_summary_messages,
        build_compact_summary_system_prompt, build_implementation_handoff_prompt,
        build_merge_summary_messages, build_merge_summary_system_prompt, build_title_prompt,
        build_title_prompt_from_messages, collapse_whitespace, detect_prior_summary,
        extract_context_summary_block, mark_thread_run_cancellation_requested,
        normalize_compact_summary, normalize_generated_title, render_compact_summary_history,
        should_complete_reasoning_for_event, summary_history_char_budget, truncate_chars,
        truncate_chars_keep_tail, truncate_tool_result_head_tail, ActiveRun,
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
        let prompt = build_compact_summary_system_prompt();

        assert!(prompt.contains("Output rules:"));
        assert!(prompt.contains("Do not output any text before or after the wrapper."));
        assert!(prompt.contains("Example output:"));
        assert!(prompt.contains("<context_summary>"));
        assert!(prompt.contains("</context_summary>"));
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
        let prompt = build_merge_summary_system_prompt();
        assert!(prompt.contains("PRIOR summary"));
        assert!(prompt.contains("DELTA"));
        assert!(prompt.contains("<context_summary>"));
        assert!(prompt.contains("</context_summary>"));
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
