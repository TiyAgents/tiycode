//! Manages the lifecycle of agent runs backed by the built-in Rust runtime.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use sqlx::SqlitePool;
use tauri::AppHandle;
use tiycore::types::OnPayloadFn;
use tokio::sync::{broadcast, mpsc, Mutex};
use tokio::time::{sleep, Instant};

use crate::core::agent_session::{build_session_spec, ResolvedModelRole};
use crate::core::built_in_agent_runtime::{BuiltInAgentRuntime, RuntimeSessionFinishState};
use crate::core::plan_checkpoint::{
    ApprovalPromptMetadata, PlanApprovalAction, PlanMessageMetadata,
    IMPLEMENTATION_PLAN_APPROVAL_KIND, IMPLEMENTATION_PLAN_APPROVED_STATE,
    IMPLEMENTATION_PLAN_PENDING_STATE, IMPLEMENTATION_PLAN_SUPERSEDED_STATE,
};
use crate::core::sleep_manager::SleepManager;
use crate::ipc::frontend_channels::ThreadStreamEvent;
use crate::model::errors::{AppError, ErrorSource};
use crate::model::thread::{MessageAttachmentDto, MessageRecord};
use crate::persistence::repo::{message_repo, run_repo, thread_repo, workspace_repo};

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
pub(crate) const FRONTEND_EVENT_BUFFER_SIZE: usize = 2048;

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
                    parts_json: None,
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
            let runtime_finish_rx = self
                .runtime
                .start_session(spec, runtime_tx, Arc::clone(&self.active_runs))
                .await?;
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
            parts_json: None,
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
            parts_json: None,
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

    pub(crate) async fn persist_messages(
        &self,
        messages: &[MessageRecord],
    ) -> Result<(), AppError> {
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
#[path = "agent_run_manager_tests.rs"]
mod agent_run_manager_tests;

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
