//! Manages the lifecycle of agent runs backed by the built-in Rust runtime.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use sqlx::SqlitePool;
use tiy_core::provider::get_provider;
use tiy_core::types::{
    Context as TiyContext, Message as TiyMessage, OnPayloadFn, StopReason,
    StreamOptions as TiyStreamOptions, UserMessage,
};
use tokio::sync::{mpsc, Mutex};
use tokio::time::{sleep, Instant};

use crate::core::agent_session::{
    build_session_spec, normalize_profile_response_language, normalize_profile_response_style,
    ProfileResponseStyle, ResolvedModelRole,
};
use crate::core::built_in_agent_runtime::BuiltInAgentRuntime;
use crate::core::sleep_manager::SleepManager;
use crate::ipc::frontend_channels::ThreadStreamEvent;
use crate::model::errors::{AppError, ErrorSource};
use crate::model::thread::{MessageRecord, ThreadStatus};
use crate::persistence::repo::{message_repo, profile_repo, run_repo, thread_repo, workspace_repo};

const TITLE_GENERATION_TIMEOUT: Duration = Duration::from_secs(12);
const TITLE_GENERATION_MAX_TOKENS: u32 = 32;
const TITLE_CONTEXT_MAX_CHARS: usize = 1_200;

struct ActiveRun {
    run_id: String,
    thread_id: String,
    profile_id: Option<String>,
    run_mode: String,
    frontend_tx: mpsc::Sender<ThreadStreamEvent>,
    lightweight_model_role: Option<ResolvedModelRole>,
    streaming_message_id: Option<String>,
    reasoning_message_id: Option<String>,
    cancellation_requested: bool,
}

pub struct AgentRunManager {
    pool: SqlitePool,
    runtime: Arc<BuiltInAgentRuntime>,
    sleep_manager: Arc<SleepManager>,
    active_runs: Arc<Mutex<HashMap<String, ActiveRun>>>,
}

impl AgentRunManager {
    pub fn new(
        pool: SqlitePool,
        runtime: Arc<BuiltInAgentRuntime>,
        sleep_manager: Arc<SleepManager>,
    ) -> Self {
        Self {
            pool,
            runtime,
            sleep_manager,
            active_runs: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub async fn start_run(
        self: &Arc<Self>,
        thread_id: &str,
        prompt: &str,
        run_mode: &str,
        profile_id: Option<String>,
        provider_id: Option<String>,
        model_id: Option<String>,
        model_plan: serde_json::Value,
    ) -> Result<(String, mpsc::Receiver<ThreadStreamEvent>), AppError> {
        let thread = thread_repo::find_by_id(&self.pool, thread_id)
            .await?
            .ok_or_else(|| AppError::not_found(ErrorSource::Thread, "thread"))?;

        let workspace_path = workspace_repo::find_by_id(&self.pool, &thread.workspace_id)
            .await?
            .map(|workspace| workspace.canonical_path)
            .unwrap_or_default();

        let (frontend_tx, frontend_rx) = mpsc::channel::<ThreadStreamEvent>(128);
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
                    run_mode: run_mode.to_string(),
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
            let user_message = MessageRecord {
                id: uuid::Uuid::now_v7().to_string(),
                thread_id: thread_id.to_string(),
                run_id: None,
                role: "user".to_string(),
                content_markdown: prompt.to_string(),
                message_type: "plain_message".to_string(),
                status: "completed".to_string(),
                metadata_json: None,
                created_at: String::new(),
            };
            message_repo::insert(&self.pool, &user_message).await?;
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
                message_repo::replace_content(&self.pool, &persisted_id, content).await?;
                message_repo::update_status(&self.pool, &persisted_id, "completed").await?;

                let mut runs = self.active_runs.lock().await;
                if let Some(run) = runs.get_mut(run_id) {
                    run.streaming_message_id = None;
                }
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
            ThreadStreamEvent::ApprovalResolved { .. } => {
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
                let usage = tiy_core::types::Usage {
                    input: usage.input_tokens,
                    output: usage.output_tokens,
                    cache_read: usage.cache_read_tokens,
                    cache_write: usage.cache_write_tokens,
                    total_tokens: usage.total_tokens,
                    cost: tiy_core::types::UsageCost::default(),
                };
                run_repo::update_usage(&self.pool, run_id, &usage).await?;
            }
            ThreadStreamEvent::RunCompleted { .. } => {
                self.finish_run(run_id, "completed", None).await?;
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

        if matches!(
            event,
            ThreadStreamEvent::RunCompleted { .. }
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
            let _ = frontend_tx.send(event).await;
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
            run_mode,
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
                run.run_mode.clone(),
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

        run_repo::update_status(&self.pool, run_id, status).await?;
        if let Some(error_message) = error_message {
            run_repo::set_error_message(&self.pool, run_id, error_message).await?;
        }

        let thread_status = match status {
            "failed" | "denied" => ThreadStatus::Failed,
            "interrupted" => ThreadStatus::Interrupted,
            _ => ThreadStatus::Idle,
        };
        thread_repo::update_status(&self.pool, &thread_id, &thread_status).await?;

        if status == "completed" && run_mode == "default" {
            self.spawn_thread_title_generation(
                run_id.to_string(),
                thread_id,
                profile_id,
                frontend_tx,
                lightweight_model_role,
            );
        }

        Ok(())
    }

    fn spawn_thread_title_generation(
        &self,
        run_id: String,
        thread_id: String,
        profile_id: Option<String>,
        frontend_tx: mpsc::Sender<ThreadStreamEvent>,
        lightweight_model_role: Option<ResolvedModelRole>,
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

async fn maybe_generate_thread_title(
    pool: &SqlitePool,
    run_id: &str,
    thread_id: &str,
    profile_id: Option<String>,
    model_role: ResolvedModelRole,
    frontend_tx: mpsc::Sender<ThreadStreamEvent>,
) -> Result<(), AppError> {
    if message_repo::count_completed_assistant_plain_messages(pool, thread_id).await? != 1 {
        tracing::debug!(
            run_id = %run_id,
            thread_id = %thread_id,
            "skipping thread title generation: not exactly one completed assistant message"
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

    if frontend_tx
        .send(ThreadStreamEvent::ThreadTitleUpdated {
            run_id: run_id.to_string(),
            thread_id: thread_id.to_string(),
            title,
        })
        .await
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
        max_tokens: Some(TITLE_GENERATION_MAX_TOKENS),
        on_payload: build_provider_options_payload_hook(model_role.provider_options.clone()),
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
            | ThreadStreamEvent::RunCompleted { .. }
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
        build_title_prompt, collapse_whitespace, normalize_generated_title,
        should_complete_reasoning_for_event, truncate_chars,
    };
    use crate::core::agent_session::ProfileResponseStyle;
    use crate::ipc::frontend_channels::ThreadStreamEvent;

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
                tool_name: "search_repo".into(),
                tool_input: serde_json::json!({ "query": "Thought" }),
            }
        ));
    }
}
