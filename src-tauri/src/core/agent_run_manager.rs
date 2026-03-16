//! Manages the lifecycle of agent runs.
//!
//! Coordinates between:
//! - Frontend (receives user prompt, displays stream events)
//! - ThreadManager (message persistence)
//! - SidecarManager (agent loop execution)
//! - ToolGateway (tool execution, approval — M1.6)

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{mpsc, Mutex};

use sqlx::SqlitePool;

use crate::core::sidecar_manager::SidecarManager;
use crate::core::tool_gateway::{ToolGateway, ToolGatewayResult};
use crate::ipc::frontend_channels::ThreadStreamEvent;
use crate::ipc::sidecar_protocol::{RunStartPayload, SidecarEvent, ToolResultPayload};
use crate::model::errors::{AppError, ErrorSource};
use crate::model::thread::MessageRecord;
use crate::persistence::repo::{message_repo, run_repo, thread_repo, tool_call_repo, workspace_repo};

/// In-memory state for an active run.
struct ActiveRun {
    run_id: String,
    thread_id: String,
    run_mode: String,
    workspace_path: String,
    /// Sender to push events to the frontend for this thread.
    frontend_tx: mpsc::Sender<ThreadStreamEvent>,
    /// Current assistant message being streamed (if any).
    streaming_message_id: Option<String>,
}

pub struct AgentRunManager {
    pool: SqlitePool,
    sidecar: Arc<SidecarManager>,
    tool_gateway: Arc<ToolGateway>,
    /// Active runs indexed by run_id.
    active_runs: Arc<Mutex<HashMap<String, ActiveRun>>>,
}

impl AgentRunManager {
    pub fn new(pool: SqlitePool, sidecar: Arc<SidecarManager>, tool_gateway: Arc<ToolGateway>) -> Self {
        Self {
            pool,
            sidecar,
            tool_gateway,
            active_runs: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Start a new run for a thread.
    ///
    /// Returns a channel receiver for frontend stream events.
    pub async fn start_run(
        &self,
        thread_id: &str,
        prompt: &str,
        run_mode: &str,
        model_plan: serde_json::Value,
    ) -> Result<(String, mpsc::Receiver<ThreadStreamEvent>), AppError> {
        // 1. Look up thread's workspace to get canonical_path for tool boundary checks
        let thread = thread_repo::find_by_id(&self.pool, thread_id)
            .await?
            .ok_or_else(|| AppError::not_found(ErrorSource::Thread, "thread"))?;

        let workspace_path = workspace_repo::find_by_id(&self.pool, &thread.workspace_id)
            .await?
            .map(|w| w.canonical_path)
            .unwrap_or_default();

        // 2. Check no active run + register atomically (hold lock through insert)
        let (frontend_tx, frontend_rx) = mpsc::channel::<ThreadStreamEvent>(128);
        let run_id = uuid::Uuid::now_v7().to_string();
        {
            let mut runs = self.active_runs.lock().await;
            if runs.values().any(|r| r.thread_id == thread_id) {
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
                    run_mode: run_mode.to_string(),
                    workspace_path: workspace_path.clone(),
                    frontend_tx: frontend_tx.clone(),
                    streaming_message_id: None,
                },
            );
        }

        // 3. Persist user message
        let user_msg = MessageRecord {
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
        message_repo::insert(&self.pool, &user_msg).await?;
        thread_repo::touch_active(&self.pool, thread_id).await?;

        // 4. Create run record
        let run_insert = run_repo::RunInsert {
            id: run_id.clone(),
            thread_id: thread_id.to_string(),
            profile_id: None,
            run_mode: run_mode.to_string(),
            provider_id: None,
            model_id: None,
            effective_model_plan_json: Some(model_plan.to_string()),
            status: "created".to_string(),
        };
        run_repo::insert(&self.pool, &run_insert).await?;

        // 5. Update thread status to running
        thread_repo::update_status(
            &self.pool,
            thread_id,
            &crate::model::thread::ThreadStatus::Running,
        )
        .await?;

        // 7. Dispatch to sidecar
        run_repo::update_status(&self.pool, &run_id, "dispatching").await?;

        let snapshot = serde_json::json!({
            "threadId": thread_id,
            "prompt": prompt,
        });

        let payload = serde_json::to_value(RunStartPayload {
            run_id: run_id.clone(),
            thread_id: thread_id.to_string(),
            run_mode: run_mode.to_string(),
            prompt: prompt.to_string(),
            model_plan,
            thread_snapshot: snapshot,
        })
        .unwrap_or_default();

        self.sidecar
            .send_request(&format!("run_{run_id}"), "agent.run.start", payload)
            .await?;

        tracing::info!(
            run_id = %run_id,
            thread_id = %thread_id,
            run_mode = %run_mode,
            "run dispatched to sidecar"
        );

        Ok((run_id, frontend_rx))
    }

    /// Cancel an active run.
    pub async fn cancel_run(&self, thread_id: &str) -> Result<(), AppError> {
        let run_id = {
            let runs = self.active_runs.lock().await;
            runs.values()
                .find(|r| r.thread_id == thread_id)
                .map(|r| r.run_id.clone())
                .ok_or_else(|| {
                    AppError::recoverable(
                        ErrorSource::Thread,
                        "thread.run.not_active",
                        "No active run for this thread",
                    )
                })?
        };

        run_repo::update_status(&self.pool, &run_id, "cancelling").await?;

        let payload = serde_json::json!({ "runId": run_id });
        let _ = self
            .sidecar
            .send_request(&format!("cancel_{run_id}"), "agent.run.cancel", payload)
            .await;

        tracing::info!(run_id = %run_id, "run cancel requested");
        Ok(())
    }

    /// Send a tool execution result back to the sidecar.
    pub async fn send_tool_result(
        &self,
        tool_call_id: &str,
        run_id: &str,
        result: serde_json::Value,
        success: bool,
    ) -> Result<(), AppError> {
        let payload = serde_json::to_value(ToolResultPayload {
            tool_call_id: tool_call_id.to_string(),
            run_id: run_id.to_string(),
            result,
            success,
        })
        .unwrap_or_default();

        self.sidecar
            .send_request(
                &format!("tool_{tool_call_id}"),
                "agent.tool.result",
                payload,
            )
            .await?;

        Ok(())
    }

    /// Process a sidecar event. Called from the event loop.
    pub async fn handle_sidecar_event(&self, event: SidecarEvent) -> Result<(), AppError> {
        let run_id = event.run_id().to_string();

        match event {
            SidecarEvent::RunStarted { .. } => {
                run_repo::update_status(&self.pool, &run_id, "running").await?;
                self.emit(&run_id, ThreadStreamEvent::RunStarted {
                    run_id: run_id.clone(),
                    run_mode: self.get_run_mode(&run_id).await,
                })
                .await;
            }

            SidecarEvent::MessageDelta {
                message_id, delta, ..
            } => {
                // Create or append to streaming assistant message
                let msg_id = self
                    .ensure_streaming_message(&run_id, &message_id)
                    .await?;
                message_repo::append_content(&self.pool, &msg_id, &delta).await?;

                self.emit(&run_id, ThreadStreamEvent::MessageDelta {
                    run_id: run_id.clone(),
                    message_id: msg_id,
                    delta,
                })
                .await;
            }

            SidecarEvent::MessageCompleted {
                message_id,
                content,
                ..
            } => {
                let msg_id = self
                    .ensure_streaming_message(&run_id, &message_id)
                    .await?;
                message_repo::update_status(&self.pool, &msg_id, "completed").await?;

                // Clear streaming message
                {
                    let mut runs = self.active_runs.lock().await;
                    if let Some(run) = runs.get_mut(&run_id) {
                        run.streaming_message_id = None;
                    }
                }

                self.emit(&run_id, ThreadStreamEvent::MessageCompleted {
                    run_id: run_id.clone(),
                    message_id: msg_id,
                    content,
                })
                .await;
            }

            SidecarEvent::PlanUpdated { plan, .. } => {
                self.emit(&run_id, ThreadStreamEvent::PlanUpdated {
                    run_id: run_id.clone(),
                    plan,
                })
                .await;
            }

            SidecarEvent::ReasoningUpdated { reasoning, .. } => {
                self.emit(&run_id, ThreadStreamEvent::ReasoningUpdated {
                    run_id: run_id.clone(),
                    reasoning,
                })
                .await;
            }

            SidecarEvent::QueueUpdated { queue, .. } => {
                self.emit(&run_id, ThreadStreamEvent::QueueUpdated {
                    run_id: run_id.clone(),
                    queue,
                })
                .await;
            }

            SidecarEvent::SubagentStarted { subtask_id, .. } => {
                self.emit(&run_id, ThreadStreamEvent::SubagentStarted {
                    run_id: run_id.clone(),
                    subtask_id,
                })
                .await;
            }

            SidecarEvent::SubagentCompleted {
                subtask_id,
                summary,
                ..
            } => {
                self.emit(&run_id, ThreadStreamEvent::SubagentCompleted {
                    run_id: run_id.clone(),
                    subtask_id,
                    summary,
                })
                .await;
            }

            SidecarEvent::SubagentFailed {
                subtask_id, error, ..
            } => {
                self.emit(&run_id, ThreadStreamEvent::SubagentFailed {
                    run_id: run_id.clone(),
                    subtask_id,
                    error,
                })
                .await;
            }

            SidecarEvent::ToolRequested {
                tool_call_id,
                tool_name,
                tool_input,
                ..
            } => {
                // Persist tool call
                let thread_id = self.get_thread_id(&run_id).await;
                let workspace_path = self.get_workspace_path(&run_id).await;
                let run_mode = self.get_run_mode(&run_id).await;

                tool_call_repo::insert(
                    &self.pool,
                    &tool_call_repo::ToolCallInsert {
                        id: tool_call_id.clone(),
                        run_id: run_id.clone(),
                        thread_id: thread_id.clone(),
                        tool_name: tool_name.clone(),
                        tool_input_json: tool_input.to_string(),
                        status: "requested".to_string(),
                    },
                )
                .await?;

                run_repo::update_status(&self.pool, &run_id, "waiting_tool_result").await?;

                // Route through ToolGateway for policy evaluation + execution
                let gw_result = self
                    .tool_gateway
                    .handle_tool_request(
                        &run_id,
                        &thread_id,
                        &tool_call_id,
                        &tool_name,
                        &tool_input,
                        &workspace_path,
                        &run_mode,
                    )
                    .await?;

                match gw_result {
                    ToolGatewayResult::Executed { tool_call_id, output } => {
                        // Auto-allowed and executed — send result to sidecar
                        self.emit(&run_id, ThreadStreamEvent::ToolCompleted {
                            run_id: run_id.clone(),
                            tool_call_id: tool_call_id.clone(),
                            result: output.result.clone(),
                        })
                        .await;
                        run_repo::update_status(&self.pool, &run_id, "running").await?;
                        self.send_tool_result(&tool_call_id, &run_id, output.result, output.success)
                            .await?;
                    }
                    ToolGatewayResult::ApprovalRequired { event } => {
                        // Needs user approval — emit to frontend, wait for response
                        run_repo::update_status(&self.pool, &run_id, "waiting_approval").await?;
                        self.emit(&run_id, event).await;
                    }
                    ToolGatewayResult::Denied { tool_call_id, reason } => {
                        // Denied by policy — send denial to sidecar
                        self.emit(&run_id, ThreadStreamEvent::ToolFailed {
                            run_id: run_id.clone(),
                            tool_call_id: tool_call_id.clone(),
                            error: reason.clone(),
                        })
                        .await;
                        run_repo::update_status(&self.pool, &run_id, "running").await?;
                        self.send_tool_result(
                            &tool_call_id,
                            &run_id,
                            serde_json::json!({"denied": true, "reason": reason}),
                            false,
                        )
                        .await?;
                    }
                }
            }

            SidecarEvent::RunCompleted { .. } => {
                self.finish_run(&run_id, "completed").await?;
                self.emit(&run_id, ThreadStreamEvent::RunCompleted {
                    run_id: run_id.clone(),
                })
                .await;
                self.remove_active_run(&run_id).await;
            }

            SidecarEvent::RunFailed { error, .. } => {
                run_repo::update_status(&self.pool, &run_id, "failed").await?;

                // Persist error as system message
                let thread_id = self.get_thread_id(&run_id).await;
                let err_msg = MessageRecord {
                    id: uuid::Uuid::now_v7().to_string(),
                    thread_id: thread_id.clone(),
                    run_id: Some(run_id.clone()),
                    role: "system".to_string(),
                    content_markdown: format!("Run failed: {error}"),
                    message_type: "plain_message".to_string(),
                    status: "completed".to_string(),
                    metadata_json: None,
                    created_at: String::new(),
                };
                let _ = message_repo::insert(&self.pool, &err_msg).await;

                self.finish_run(&run_id, "failed").await?;
                self.emit(&run_id, ThreadStreamEvent::RunFailed {
                    run_id: run_id.clone(),
                    error,
                })
                .await;
                self.remove_active_run(&run_id).await;
            }
        }

        Ok(())
    }

    /// Start the background event processing loop.
    pub fn spawn_event_loop(self: &Arc<Self>, mut event_rx: mpsc::Receiver<SidecarEvent>) {
        let manager = Arc::clone(self);
        tokio::spawn(async move {
            while let Some(event) = event_rx.recv().await {
                let run_id = event.run_id().to_string();
                if let Err(e) = manager.handle_sidecar_event(event).await {
                    tracing::error!(run_id = %run_id, error = %e, "failed to handle sidecar event");
                }
            }
            tracing::info!("sidecar event loop exited");
        });
    }

    // -----------------------------------------------------------------------
    // Internal helpers
    // -----------------------------------------------------------------------

    async fn emit(&self, run_id: &str, event: ThreadStreamEvent) {
        let runs = self.active_runs.lock().await;
        if let Some(run) = runs.get(run_id) {
            let _ = run.frontend_tx.send(event).await;
        }
    }

    async fn get_run_mode(&self, run_id: &str) -> String {
        let runs = self.active_runs.lock().await;
        runs.get(run_id)
            .map(|r| r.run_mode.clone())
            .unwrap_or_else(|| "default".to_string())
    }

    async fn get_thread_id(&self, run_id: &str) -> String {
        let runs = self.active_runs.lock().await;
        runs.get(run_id)
            .map(|r| r.thread_id.clone())
            .unwrap_or_default()
    }

    async fn get_workspace_path(&self, run_id: &str) -> String {
        let runs = self.active_runs.lock().await;
        runs.get(run_id)
            .map(|r| r.workspace_path.clone())
            .unwrap_or_default()
    }

    /// Ensure a streaming assistant message exists for the current run.
    /// If the sidecar provides a message_id, use it; otherwise create one.
    async fn ensure_streaming_message(
        &self,
        run_id: &str,
        sidecar_msg_id: &str,
    ) -> Result<String, AppError> {
        let mut runs = self.active_runs.lock().await;
        let run = runs.get_mut(run_id).ok_or_else(|| {
            AppError::internal(ErrorSource::Thread, "Active run not found for event")
        })?;

        if let Some(ref id) = run.streaming_message_id {
            return Ok(id.clone());
        }

        // Create a new streaming assistant message
        let msg_id = if sidecar_msg_id.is_empty() {
            uuid::Uuid::now_v7().to_string()
        } else {
            sidecar_msg_id.to_string()
        };

        let record = MessageRecord {
            id: msg_id.clone(),
            thread_id: run.thread_id.clone(),
            run_id: Some(run_id.to_string()),
            role: "assistant".to_string(),
            content_markdown: String::new(),
            message_type: "plain_message".to_string(),
            status: "streaming".to_string(),
            metadata_json: None,
            created_at: String::new(),
        };

        message_repo::insert(&self.pool, &record).await?;
        run.streaming_message_id = Some(msg_id.clone());

        Ok(msg_id)
    }

    async fn finish_run(&self, run_id: &str, status: &str) -> Result<(), AppError> {
        run_repo::update_status(&self.pool, run_id, status).await?;

        let thread_id = self.get_thread_id(run_id).await;
        if !thread_id.is_empty() {
            // Derive thread status from the now-terminal run
            let new_status = match status {
                "completed" | "cancelled" => crate::model::thread::ThreadStatus::Idle,
                "failed" | "denied" => crate::model::thread::ThreadStatus::Failed,
                "interrupted" => crate::model::thread::ThreadStatus::Interrupted,
                _ => crate::model::thread::ThreadStatus::Idle,
            };
            thread_repo::update_status(&self.pool, &thread_id, &new_status).await?;
        }

        Ok(())
    }

    async fn remove_active_run(&self, run_id: &str) {
        let mut runs = self.active_runs.lock().await;
        runs.remove(run_id);
    }
}
