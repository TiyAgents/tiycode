//! Manages the lifecycle of agent runs backed by the built-in Rust runtime.

use std::collections::HashMap;
use std::sync::Arc;

use sqlx::SqlitePool;
use tokio::sync::{mpsc, Mutex};

use crate::core::agent_session::build_session_spec;
use crate::core::built_in_agent_runtime::BuiltInAgentRuntime;
use crate::core::sleep_manager::SleepManager;
use crate::ipc::frontend_channels::ThreadStreamEvent;
use crate::model::errors::{AppError, ErrorSource};
use crate::model::thread::{MessageRecord, ThreadStatus};
use crate::persistence::repo::{message_repo, run_repo, thread_repo, workspace_repo};

struct ActiveRun {
    run_id: String,
    thread_id: String,
    frontend_tx: mpsc::Sender<ThreadStreamEvent>,
    streaming_message_id: Option<String>,
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
                    frontend_tx: frontend_tx.clone(),
                    streaming_message_id: None,
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
        let run_id = {
            let mut runs = self.active_runs.lock().await;
            let run = runs
                .values_mut()
                .find(|run| run.thread_id == thread_id)
                .ok_or_else(|| {
                    AppError::recoverable(
                        ErrorSource::Thread,
                        "thread.run.not_active",
                        "No active run for this thread",
                    )
                })?;
            run.cancellation_requested = true;
            run.run_id.clone()
        };

        run_repo::update_status(&self.pool, &run_id, "cancelling").await?;
        self.runtime.cancel_session(&run_id).await?;
        tracing::info!(run_id = %run_id, "run cancel requested");
        Ok(())
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
            ThreadStreamEvent::ToolRequested { .. } => {
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
            ThreadStreamEvent::ToolCompleted { .. } | ThreadStreamEvent::ToolFailed { .. } => {
                run_repo::update_status(&self.pool, run_id, "running").await?;
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
            AppError::internal(ErrorSource::Thread, "active run not found for runtime event")
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

    async fn finish_run(
        &self,
        run_id: &str,
        status: &str,
        error_message: Option<&str>,
    ) -> Result<(), AppError> {
        run_repo::update_status(&self.pool, run_id, status).await?;
        if let Some(error_message) = error_message {
            run_repo::set_error_message(&self.pool, run_id, error_message).await?;
        }

        let thread_id = self.get_thread_id(run_id).await;
        let thread_status = match status {
            "failed" | "denied" => ThreadStatus::Failed,
            "interrupted" => ThreadStatus::Interrupted,
            _ => ThreadStatus::Idle,
        };
        thread_repo::update_status(&self.pool, &thread_id, &thread_status).await?;

        Ok(())
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
