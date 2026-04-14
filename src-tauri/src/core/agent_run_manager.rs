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
use crate::core::context_compression::summarize_messages;
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
use crate::persistence::repo::{message_repo, profile_repo, run_repo, thread_repo, workspace_repo};

const TITLE_GENERATION_TIMEOUT: Duration = Duration::from_secs(60);
const COMPACT_SUMMARY_TIMEOUT: Duration = Duration::from_secs(20);
const TITLE_GENERATION_MAX_TOKENS: u32 = 512;
const TITLE_GENERATION_MAX_TOKENS_REASONING: u32 = 2048;
const COMPACT_SUMMARY_MAX_TOKENS: u32 = 700;
const COMPACT_SUMMARY_MAX_TOKENS_REASONING: u32 = 2048;
const TITLE_CONTEXT_MAX_CHARS: usize = 1_200;
const COMPACT_SUMMARY_CONTEXT_MAX_CHARS: usize = 18_000;
const FRONTEND_EVENT_BUFFER_SIZE: usize = 2048;

struct ActiveRun {
    run_id: String,
    thread_id: String,
    profile_id: Option<String>,
    frontend_tx: broadcast::Sender<ThreadStreamEvent>,
    lightweight_model_role: Option<ResolvedModelRole>,
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
            build_implementation_handoff_prompt(&plan_metadata, action.clone());
        let (history_override, context_seed_messages) = match action {
            PlanApprovalAction::ApplyPlan => (None, None),
            PlanApprovalAction::ApplyPlanWithContextReset => {
                let message_bundle = self
                    .build_context_reset_message_bundle(
                        thread_id,
                        &plan_message,
                        &plan_metadata,
                        &model_plan_value,
                    )
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

    pub async fn compact_thread_context(
        &self,
        thread_id: &str,
        instructions: Option<String>,
        model_plan_value: serde_json::Value,
    ) -> Result<(), AppError> {
        if self.cancel_run_if_active(thread_id).await? {
            tracing::info!(thread_id = %thread_id, "Cancelled active run before compacting context");
        }

        let thread = thread_repo::find_by_id(&self.pool, thread_id)
            .await?
            .ok_or_else(|| AppError::not_found(ErrorSource::Thread, "thread"))?;
        let messages = message_repo::list_recent(&self.pool, thread_id, None, 1024).await?;
        let current_context_messages = trim_history_to_current_context(&messages);
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
        let model = compact_summary_model(&preview_spec.model_plan);
        let history = convert_history_messages(&current_context_messages, &model);
        let compact_instructions = instructions
            .as_ref()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty());
        let summary = self
            .build_compact_summary(
                thread_id,
                &history,
                preview_spec.model_plan.lightweight.as_ref(),
                compact_instructions.as_deref(),
            )
            .await;

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
        let summary_metadata = serde_json::json!({
            "kind": "context_summary",
            "source": "compact",
            "label": "Compacted context summary",
        });
        let reset_metadata = serde_json::json!({
            "kind": "context_reset",
            "source": "compact",
            "label": "Context is now reset",
        });

        let messages = vec![
            MessageRecord {
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
            MessageRecord {
                id: uuid::Uuid::now_v7().to_string(),
                thread_id: thread_id.to_string(),
                run_id: None,
                role: "system".to_string(),
                content_markdown: summary,
                message_type: "summary_marker".to_string(),
                status: "completed".to_string(),
                metadata_json: Some(summary_metadata.to_string()),
                attachments_json: None,
                created_at: String::new(),
            },
        ];
        self.persist_messages(&messages).await?;
        thread_repo::touch_active(&self.pool, thread_id).await?;
        thread_repo::update_status(&self.pool, &thread.id, &ThreadStatus::Idle).await?;
        Ok(())
    }

    async fn build_compact_summary(
        &self,
        thread_id: &str,
        history: &[AgentMessage],
        lightweight_model_role: Option<&ResolvedModelRole>,
        instructions: Option<&str>,
    ) -> String {
        let fallback_summary = build_fallback_compact_summary(history, instructions);

        if history.is_empty() {
            return fallback_summary;
        }

        let Some(model_role) = lightweight_model_role else {
            tracing::info!(
                thread_id = %thread_id,
                "compact summary falling back to heuristic summary: no lightweight model configured"
            );
            return fallback_summary;
        };

        match generate_compact_summary(model_role, history, instructions).await {
            Ok(Some(summary)) => summary,
            Ok(None) => {
                tracing::warn!(
                    thread_id = %thread_id,
                    provider_id = %model_role.provider_id,
                    model_id = %model_role.model_id,
                    "compact summary generation returned empty result, falling back to heuristic summary"
                );
                fallback_summary
            }
            Err(error) => {
                tracing::warn!(
                    thread_id = %thread_id,
                    provider_id = %model_role.provider_id,
                    model_id = %model_role.model_id,
                    error = %error,
                    "compact summary generation failed, falling back to heuristic summary"
                );
                fallback_summary
            }
        }
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

    pub async fn cancel_run(&self, thread_id: &str) -> Result<(), AppError> {
        if self.cancel_run_if_active(thread_id).await? {
            return Ok(());
        }

        Err(AppError::recoverable(
            ErrorSource::Thread,
            "thread.run.not_active",
            "No active run for this thread",
        ))
    }

    pub async fn cancel_run_if_active(&self, thread_id: &str) -> Result<bool, AppError> {
        let run_id = {
            let mut runs = self.active_runs.lock().await;
            let run = runs.values_mut().find(|run| run.thread_id == thread_id);
            let Some(run) = run else {
                return Ok(false);
            };
            run.cancellation_requested = true;
            run.run_id.clone()
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
        plan_message: &MessageRecord,
        plan_metadata: &PlanMessageMetadata,
        model_plan_value: &serde_json::Value,
    ) -> Result<ContextResetMessageBundle, AppError> {
        let model = build_session_spec(
            &self.pool,
            "plan-reset-preview",
            thread_id,
            "",
            "default",
            model_plan_value,
        )
        .await?
        .model_plan
        .primary
        .model;
        let pre_plan_messages =
            message_repo::list_recent(&self.pool, thread_id, Some(&plan_message.id), 1024).await?;
        let current_context_messages = trim_history_to_current_context(&pre_plan_messages);
        let summary = if current_context_messages.is_empty() {
            "<context_summary>\nNo earlier plain-message context was available before this plan.\n</context_summary>"
                .to_string()
        } else {
            let history = convert_history_messages(&current_context_messages, &model);
            summarize_messages(&history)
        };

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
        let summary_message = MessageRecord {
            id: uuid::Uuid::now_v7().to_string(),
            thread_id: thread_id.to_string(),
            run_id: None,
            role: "system".to_string(),
            content_markdown: summary,
            message_type: "summary_marker".to_string(),
            status: "completed".to_string(),
            metadata_json: Some(
                serde_json::json!({
                    "kind": "context_summary",
                    "source": "plan_approval",
                    "label": "Historical context summary",
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

        let history_override = vec![summary_message.clone(), approved_plan_message.clone()];
        let persisted_messages = vec![reset_message, summary_message, approved_plan_message];

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
        app_handle: AppHandle,
    ) {
        let Some(model_role) = lightweight_model_role else {
            tracing::debug!(
                run_id = %run_id,
                thread_id = %thread_id,
                "skipping thread title generation: no lightweight model configured"
            );
            return;
        };

        let pool = self.pool.clone();
        tokio::spawn(async move {
            if let Err(error) = maybe_generate_thread_title(
                &pool,
                &run_id,
                &thread_id,
                profile_id,
                model_role,
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
    match action {
        PlanApprovalAction::ApplyPlan => {
            let plan_markdown = crate::core::plan_checkpoint::plan_markdown(metadata);

            format!(
                "Implementation handoff:\n- {action_note}\n- Plan revision: {}\n- Treat the approved plan below as the implementation baseline.\n- If the plan turns out to be invalid or incomplete, pause and return to planning before making a different change.\n\nApproved plan:\n{}",
                metadata.artifact.plan_revision,
                plan_markdown
            )
        }
        PlanApprovalAction::ApplyPlanWithContextReset => format!(
            "Implementation handoff:\n- {action_note}\n- Plan revision: {}\n- The reset context already includes a historical summary and the approved plan.\n- Treat the approved plan in context as the implementation baseline.\n- If the plan turns out to be invalid or incomplete, pause and return to planning before making a different change.",
            metadata.artifact.plan_revision,
        ),
    }
}

fn compact_summary_model(
    model_plan: &crate::core::agent_session::ResolvedRuntimeModelPlan,
) -> tiycore::types::Model {
    model_plan
        .auxiliary
        .as_ref()
        .unwrap_or(&model_plan.primary)
        .model
        .clone()
}

fn build_fallback_compact_summary(history: &[AgentMessage], instructions: Option<&str>) -> String {
    let base_summary = if history.is_empty() {
        "<context_summary>\nNo previous conversation was available to compact.\n</context_summary>"
            .to_string()
    } else {
        summarize_messages(history)
    };

    append_compact_instructions(base_summary, instructions)
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
        render_compact_summary_history(history)
    ))));

    messages
}

async fn generate_compact_summary(
    model_role: &ResolvedModelRole,
    history: &[AgentMessage],
    instructions: Option<&str>,
) -> Result<Option<String>, AppError> {
    // Compact summary generation does not benefit from reasoning/thinking tokens.
    // Disable reasoning so the protocol layer omits thinking/reasoning parameters,
    // preventing reasoning tokens from consuming the COMPACT_SUMMARY_MAX_TOKENS budget.
    // If the original model had reasoning enabled, bump max_tokens as a fallback —
    // some reasoning-only models ignore the disable and still produce reasoning tokens.
    let was_reasoning = model_role.model.reasoning;
    let mut model_role = model_role.clone();
    model_role.model.reasoning = false;

    let provider = get_provider(&model_role.model.provider).ok_or_else(|| {
        AppError::recoverable(
            ErrorSource::Settings,
            "settings.compact_summary.provider_missing",
            format!(
                "Provider type '{:?}' is not registered for lightweight compact summary generation.",
                model_role.model.provider
            ),
        )
    })?;

    let context = TiyContext {
        system_prompt: Some(build_compact_summary_system_prompt()),
        messages: build_compact_summary_messages(history, instructions),
        tools: None,
    };

    let options = TiyStreamOptions {
        api_key: model_role.api_key.clone(),
        max_tokens: Some(if was_reasoning {
            COMPACT_SUMMARY_MAX_TOKENS_REASONING
        } else {
            COMPACT_SUMMARY_MAX_TOKENS
        }),
        headers: Some(tiycode_default_headers()),
        on_payload: build_provider_options_payload_hook(model_role.provider_options.clone()),
        security: Some(tiycore::types::SecurityConfig::default().with_url(tiycode_url_policy())),
        ..TiyStreamOptions::default()
    };

    let completion = provider
        .stream(&model_role.model, &context, options)
        .try_result(COMPACT_SUMMARY_TIMEOUT)
        .await;

    let message = match completion {
        Some(message) => message,
        None => return Ok(None),
    };

    if message.stop_reason == StopReason::Error {
        let detail = message
            .error_message
            .clone()
            .unwrap_or_else(|| "lightweight compact summary generation failed".to_string());
        return Err(AppError::recoverable(
            ErrorSource::Settings,
            "settings.compact_summary.failed",
            detail,
        ));
    }

    Ok(normalize_compact_summary(
        message.text_content(),
        instructions,
    ))
}

fn render_compact_summary_history(history: &[AgentMessage]) -> String {
    let mut rendered = String::new();

    for message in history {
        match message {
            AgentMessage::User(user) => {
                let text = user_message_to_text(user);
                if text.is_empty() {
                    continue;
                }
                rendered.push_str("[user]\n");
                rendered.push_str(&text);
                rendered.push_str("\n\n");
            }
            AgentMessage::Assistant(assistant) => {
                let text = assistant_message_to_text(assistant);
                if text.is_empty() {
                    continue;
                }
                rendered.push_str("[assistant]\n");
                rendered.push_str(&text);
                rendered.push_str("\n\n");
            }
            AgentMessage::ToolResult(tool_result) => {
                let text = tool_result_to_text(tool_result);
                if text.is_empty() {
                    continue;
                }
                rendered.push_str("[tool_result]");
                if !tool_result.tool_name.is_empty() {
                    rendered.push(' ');
                    rendered.push_str(&tool_result.tool_name);
                }
                rendered.push('\n');
                rendered.push_str(&text);
                rendered.push_str("\n\n");
            }
            AgentMessage::Custom { data, .. } => {
                let text = truncate_chars(&collapse_whitespace(&data.to_string()), 600);
                if text.is_empty() {
                    continue;
                }
                rendered.push_str("[custom]\n");
                rendered.push_str(&text);
                rendered.push_str("\n\n");
            }
        }

        if rendered.chars().count() >= COMPACT_SUMMARY_CONTEXT_MAX_CHARS {
            break;
        }
    }

    truncate_chars(&rendered, COMPACT_SUMMARY_CONTEXT_MAX_CHARS)
}

fn user_message_to_text(user: &UserMessage) -> String {
    match &user.content {
        tiycore::types::UserContent::Text(text) => truncate_chars(text.trim(), 1_200),
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
            truncate_chars(&parts.join("\n"), 1_200)
        }
    }
}

fn assistant_message_to_text(assistant: &tiycore::types::AssistantMessage) -> String {
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
                    truncate_chars(&collapse_whitespace(&tool_call.arguments.to_string()), 300)
                ));
            }
            tiycore::types::ContentBlock::Image(_) => parts.push("[image]".to_string()),
        }
    }
    truncate_chars(&parts.join("\n"), 1_500)
}

fn tool_result_to_text(tool_result: &tiycore::types::ToolResultMessage) -> String {
    let mut parts = Vec::new();
    for block in &tool_result.content {
        if let tiycore::types::ContentBlock::Text(text) = block {
            let trimmed = text.text.trim();
            if !trimmed.is_empty() {
                parts.push(trimmed.to_string());
            }
        }
    }
    truncate_chars(&parts.join("\n"), 1_200)
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

async fn maybe_generate_thread_title(
    pool: &SqlitePool,
    run_id: &str,
    thread_id: &str,
    profile_id: Option<String>,
    model_role: ResolvedModelRole,
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

    let Some((user_message, assistant_message)) =
        load_initial_title_context(pool, thread_id).await?
    else {
        tracing::debug!(
            run_id = %run_id,
            thread_id = %thread_id,
            "skipping thread title generation: could not load initial title context"
        );
        return Ok(());
    };

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

    let Some(title) = generate_thread_title(
        &model_role,
        &user_message,
        &assistant_message,
        response_language.as_deref(),
        response_style,
    )
    .await?
    else {
        tracing::warn!(
            run_id = %run_id,
            thread_id = %thread_id,
            "thread title generation returned empty result (timeout or empty response)"
        );
        return Ok(());
    };

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

    Ok(())
}

async fn load_initial_title_context(
    pool: &SqlitePool,
    thread_id: &str,
) -> Result<Option<(String, String)>, AppError> {
    let messages = message_repo::list_recent(pool, thread_id, None, 12).await?;
    let first_user_message = messages
        .iter()
        .find(|message| message.role == "user" && message.message_type == "plain_message")
        .map(|message| message.content_markdown.trim())
        .filter(|content| !content.is_empty());
    let first_assistant_message = messages
        .iter()
        .find(|message| {
            message.role == "assistant"
                && message.message_type == "plain_message"
                && message.status == "completed"
        })
        .map(|message| message.content_markdown.trim())
        .filter(|content| !content.is_empty());

    match (first_user_message, first_assistant_message) {
        (Some(user_message), Some(assistant_message)) => Ok(Some((
            truncate_chars(user_message, TITLE_CONTEXT_MAX_CHARS),
            truncate_chars(assistant_message, TITLE_CONTEXT_MAX_CHARS),
        ))),
        _ => Ok(None),
    }
}

async fn generate_thread_title(
    model_role: &ResolvedModelRole,
    user_message: &str,
    assistant_message: &str,
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

    let prompt = build_title_prompt(
        user_message,
        assistant_message,
        response_language,
        response_style,
    );
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

fn build_title_prompt(
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

fn normalize_generated_title(raw: &str) -> Option<String> {
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

fn collapse_whitespace(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn should_complete_reasoning_for_event(event: &ThreadStreamEvent) -> bool {
    !matches!(
        event,
        ThreadStreamEvent::RunStarted { .. }
            | ThreadStreamEvent::ReasoningUpdated { .. }
            | ThreadStreamEvent::ThreadUsageUpdated { .. }
            | ThreadStreamEvent::RunCheckpointed { .. }
            | ThreadStreamEvent::RunCompleted { .. }
            | ThreadStreamEvent::RunLimitReached { .. }
            | ThreadStreamEvent::RunFailed { .. }
            | ThreadStreamEvent::RunCancelled { .. }
            | ThreadStreamEvent::RunInterrupted { .. }
    )
}

fn truncate_chars(value: &str, max_chars: usize) -> String {
    let truncated: String = value.chars().take(max_chars).collect();
    if value.chars().count() > max_chars {
        truncated.trim_end().to_string()
    } else {
        value.to_string()
    }
}

fn build_provider_options_payload_hook(
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
        append_compact_instructions, build_compact_summary_messages,
        build_compact_summary_system_prompt, build_implementation_handoff_prompt,
        build_title_prompt, collapse_whitespace, extract_context_summary_block,
        normalize_compact_summary, normalize_generated_title, should_complete_reasoning_for_event,
        truncate_chars,
    };
    use crate::core::agent_session::ProfileResponseStyle;
    use crate::core::plan_checkpoint::{
        build_plan_artifact_from_tool_input, build_plan_message_metadata, PlanApprovalAction,
    };
    use crate::ipc::frontend_channels::ThreadStreamEvent;
    use tiycore::agent::AgentMessage;
    use tiycore::types::{Message as TiyMessage, UserMessage};

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
        let messages = build_compact_summary_messages(&history, Some("Keep unresolved risks"));

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
    }
}
