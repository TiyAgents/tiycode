use tauri::{AppHandle, Emitter};
use tokio::sync::broadcast;

use crate::core::agent_run_title::{build_title_model_candidates, maybe_generate_thread_title};
use crate::core::agent_session::ResolvedModelRole;
use crate::core::built_in_agent_runtime::RuntimeSessionFinishState;
use crate::core::task_board_manager;
use crate::ipc::app_events::{self, ThreadRunFinishedPayload, ThreadRunStartedPayload};
use crate::ipc::frontend_channels::ThreadStreamEvent;
use crate::model::errors::{AppError, ErrorSource};
use crate::model::thread::{MessageRecord, ThreadStatus};
use crate::persistence::repo::{message_repo, run_repo, thread_repo};

use super::agent_run_manager::AgentRunManager;

pub(crate) fn build_orphaned_run_terminal_event(
    run_id: &str,
    cancellation_requested: bool,
) -> ThreadStreamEvent {
    if cancellation_requested {
        ThreadStreamEvent::RunCancelled {
            run_id: run_id.to_string(),
        }
    } else {
        ThreadStreamEvent::RunInterrupted {
            run_id: run_id.to_string(),
        }
    }
}

pub(crate) fn terminal_event_status(
    event: &ThreadStreamEvent,
    cancellation_requested: bool,
) -> Option<&'static str> {
    match event {
        ThreadStreamEvent::RunCompleted { .. } => Some("completed"),
        ThreadStreamEvent::RunLimitReached { .. } => Some("limit_reached"),
        ThreadStreamEvent::RunFailed { .. } => Some("failed"),
        ThreadStreamEvent::RunCancelled { .. } => Some("cancelled"),
        ThreadStreamEvent::RunInterrupted { .. } => Some(if cancellation_requested {
            "cancelled"
        } else {
            "interrupted"
        }),
        _ => None,
    }
}

pub(crate) fn is_terminal_runtime_event(event: &ThreadStreamEvent) -> bool {
    matches!(
        event,
        ThreadStreamEvent::RunCheckpointed { .. }
            | ThreadStreamEvent::RunCompleted { .. }
            | ThreadStreamEvent::RunLimitReached { .. }
            | ThreadStreamEvent::RunFailed { .. }
            | ThreadStreamEvent::RunCancelled { .. }
            | ThreadStreamEvent::RunInterrupted { .. }
    )
}

impl AgentRunManager {
    pub(crate) async fn handle_runtime_channel_closed(&self, run_id: &str) -> Result<(), AppError> {
        if !self.has_active_run(run_id).await {
            return Ok(());
        }

        let runtime_state = self.runtime.session_state(run_id).await;
        let runtime_finish_state = self.runtime.session_finish_state(run_id).await;
        let cancellation_requested = self.was_cancel_requested(run_id).await;

        let Some(terminal_event) = (match runtime_finish_state {
            Some(RuntimeSessionFinishState::Completed) => None,
            Some(RuntimeSessionFinishState::Panicked) => Some(ThreadStreamEvent::RunInterrupted {
                run_id: run_id.to_string(),
            }),
            Some(RuntimeSessionFinishState::Cancelled) => {
                Some(build_orphaned_run_terminal_event(run_id, true))
            }
            Some(RuntimeSessionFinishState::Running) | None => Some(
                build_orphaned_run_terminal_event(run_id, cancellation_requested),
            ),
        }) else {
            tracing::debug!(
                run_id = %run_id,
                runtime_state = ?runtime_state,
                "runtime event channel closed after session completed; skipping forced cleanup"
            );
            return Ok(());
        };

        tracing::warn!(
            run_id = %run_id,
            cancellation_requested,
            runtime_state = ?runtime_state,
            runtime_finish_state = ?runtime_finish_state,
            "runtime event channel closed before a terminal event; forcing run cleanup"
        );

        self.handle_runtime_event(run_id, terminal_event).await
    }

    pub(crate) async fn handle_runtime_event(
        &self,
        run_id: &str,
        event: ThreadStreamEvent,
    ) -> Result<(), AppError> {
        if is_terminal_runtime_event(&event) && !self.has_active_run(run_id).await {
            tracing::debug!(
                run_id = %run_id,
                event = ?event,
                "ignoring duplicate terminal runtime event after run cleanup"
            );
            return Ok(());
        }

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
                turn_index,
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
                // Persist turn_index into metadata for response boundary tracking
                if let Some(ti) = turn_index {
                    let meta = serde_json::json!({"turnIndex": ti}).to_string();
                    message_repo::update_metadata(&self.pool, &persisted_id, Some(&meta)).await?;
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
                thinking_signature,
                turn_index,
                ..
            } => {
                let persisted_id = self.ensure_reasoning_message(run_id, message_id).await?;
                message_repo::replace_content(&self.pool, &persisted_id, reasoning).await?;
                // Merge thinking_signature and turn_index into metadata_json
                let has_sig = thinking_signature.is_some();
                let has_ti = turn_index.is_some();
                if has_sig || has_ti {
                    let mut meta = serde_json::Map::new();
                    if let Some(sig) = thinking_signature {
                        meta.insert(
                            "thinking_signature".to_string(),
                            serde_json::Value::String(sig.clone()),
                        );
                    }
                    if let Some(ti) = turn_index {
                        meta.insert("turnIndex".to_string(), serde_json::json!(ti));
                    }
                    message_repo::update_metadata(
                        &self.pool,
                        &persisted_id,
                        Some(&serde_json::Value::Object(meta).to_string()),
                    )
                    .await?;
                }
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
                // When the run is already being cancelled, do not revert the
                // status back to "running" — keep it at "cancelling" so the
                // terminal RunCancelled event sees a consistent state.
                if !self.was_cancel_requested(run_id).await {
                    run_repo::update_status(&self.pool, run_id, "running").await?;
                }
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
                let status = terminal_event_status(&event, self.was_cancel_requested(run_id).await)
                    .expect("terminal run event should resolve to a status");
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

        if is_terminal_runtime_event(&event) {
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
                parts_json: None,
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
                parts_json: None,
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
        // Reasoning termination: classify by content/signature validity
        if let Some(message_id) = reasoning_message_id {
            let should_discard = if status != "completed" {
                // Non-normal termination: check if reasoning has valid content + signature
                match message_repo::find_by_id(&self.pool, &message_id).await? {
                    Some(msg) => {
                        let is_empty = msg.content_markdown.trim().is_empty();
                        let has_signature = msg
                            .metadata_json
                            .as_deref()
                            .and_then(|j| serde_json::from_str::<serde_json::Value>(j).ok())
                            .and_then(|v| v.get("thinking_signature")?.as_str().map(|_| ()))
                            .is_some();
                        is_empty || !has_signature
                    }
                    None => true,
                }
            } else {
                false
            };

            let effective_status = if should_discard {
                "discarded"
            } else {
                finalized_message_status
            };
            message_repo::update_status(&self.pool, &message_id, effective_status).await?;
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
}

pub(crate) fn should_complete_reasoning_for_event(event: &ThreadStreamEvent) -> bool {
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
