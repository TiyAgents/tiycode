use std::sync::Arc;

use sqlx::SqlitePool;
use tokio::sync::broadcast;

use crate::core::agent_run_summary::extract_run_model_refs;
use crate::core::agent_run_title::{build_title_model_candidates, maybe_generate_thread_title};
use crate::core::agent_session::ResolvedModelRole;
use crate::core::app_event_emitter::{emit_app_event, AppEventEmitter, AppEventEmitterRef};
use crate::core::built_in_agent_runtime::RuntimeSessionFinishState;
use crate::core::task_board_manager;
use crate::ipc::app_events::{
    self, AgentFileChangedPayload, ThreadRunFinishedPayload, ThreadRunStartedPayload,
    ThreadRunStatusChangedPayload,
};
use crate::ipc::frontend_channels::ThreadStreamEvent;
use crate::model::errors::{AppError, ErrorSource};
use crate::model::thread::{MessageRecord, RunStatus, ThreadStatus};
use crate::persistence::repo::{message_repo, run_helper_repo, run_repo, thread_repo};

use super::agent_run_manager::{AgentRunManager, StartRunOptions};

pub(crate) async fn persist_final_run_state(
    pool: &SqlitePool,
    run_id: &str,
    thread_id: &str,
    status: RunStatus,
    error_message: Option<&str>,
) -> Result<(), AppError> {
    run_repo::update_status(pool, run_id, status).await?;
    if let Some(msg) = error_message {
        run_repo::set_error_message(pool, run_id, msg).await?;
    }

    let thread_status = status.to_thread_status();
    thread_repo::update_status(pool, thread_id, &thread_status).await?;

    Ok(())
}

pub(crate) async fn persist_user_message_recorded(
    pool: &SqlitePool,
    thread_id: &str,
    message_id: &str,
    content: &str,
    created_at: &str,
    metadata: Option<&serde_json::Value>,
) -> Result<(), AppError> {
    message_repo::insert(
        pool,
        &MessageRecord {
            id: message_id.to_string(),
            thread_id: thread_id.to_string(),
            run_id: None,
            role: "user".to_string(),
            content_markdown: content.to_string(),
            parts_json: None,
            message_type: "plain_message".to_string(),
            status: "completed".to_string(),
            metadata_json: metadata.map(|value| value.to_string()),
            attachments_json: None,
            created_at: created_at.to_string(),
        },
    )
    .await
}

/// Core run finalization: update DB statuses + emit THREAD_RUN_FINISHED.
///
/// Called by both `finish_run` (normal runs) and compact flow.
/// The caller remains responsible for run-specific cleanup (message finalization,
/// task board reconciliation, title generation, active_run removal).
pub(crate) async fn finalize_run(
    pool: &SqlitePool,
    app_events: &dyn AppEventEmitter,
    run_id: &str,
    thread_id: &str,
    status: RunStatus,
    error_message: Option<&str>,
) -> Result<(), AppError> {
    persist_final_run_state(pool, run_id, thread_id, status, error_message).await?;

    emit_app_event(
        app_events,
        app_events::THREAD_RUN_FINISHED,
        ThreadRunFinishedPayload {
            thread_id: thread_id.to_string(),
            run_id: run_id.to_string(),
            status: status.as_str().to_string(),
        },
    );

    Ok(())
}

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

pub(crate) fn runtime_panic_error_message(detail: &str) -> String {
    if detail.trim().is_empty() {
        "Runtime task panicked before completion.".to_string()
    } else {
        detail.to_string()
    }
}

pub(crate) fn terminal_event_status(
    event: &ThreadStreamEvent,
    cancellation_requested: bool,
) -> Option<RunStatus> {
    match event {
        ThreadStreamEvent::RunCompleted { .. } => Some(RunStatus::Completed),
        ThreadStreamEvent::RunLimitReached { .. } => Some(RunStatus::LimitReached),
        ThreadStreamEvent::RunFailed { .. } => Some(RunStatus::Failed),
        ThreadStreamEvent::RunCancelled { .. } => Some(RunStatus::Cancelled),
        ThreadStreamEvent::RunInterrupted { .. } => Some(if cancellation_requested {
            RunStatus::Cancelled
        } else {
            RunStatus::Interrupted
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

/// Map a runtime stream event to the sidebar-visible status string that should
/// be broadcast globally. Returns `None` for events that do not change the
/// thread's user-visible run status (e.g. message deltas, usage updates).
///
/// The returned `RunStatus` is converted to a string for the frontend payload.
pub(crate) fn sidebar_status_for_runtime_event(
    event: &ThreadStreamEvent,
    cancellation_requested: bool,
) -> Option<RunStatus> {
    match event {
        ThreadStreamEvent::RunStarted { .. }
        | ThreadStreamEvent::RunRetrying { .. }
        | ThreadStreamEvent::RequestRetrying { .. }
        | ThreadStreamEvent::ApprovalResolved { .. }
        | ThreadStreamEvent::ClarifyResolved { .. } => Some(RunStatus::Running),

        ThreadStreamEvent::ApprovalRequired { .. } | ThreadStreamEvent::RunCheckpointed { .. } => {
            Some(RunStatus::WaitingApproval)
        }

        ThreadStreamEvent::ClarifyRequired { .. } => Some(RunStatus::NeedsReply),

        ThreadStreamEvent::RunCompleted { .. } => Some(RunStatus::Completed),
        ThreadStreamEvent::RunLimitReached { .. } => Some(RunStatus::LimitReached),
        ThreadStreamEvent::RunFailed { .. } => Some(RunStatus::Failed),
        ThreadStreamEvent::RunCancelled { .. } => Some(RunStatus::Cancelled),
        ThreadStreamEvent::RunInterrupted { .. } => {
            if cancellation_requested {
                Some(RunStatus::Cancelled)
            } else {
                Some(RunStatus::Interrupted)
            }
        }
        _ => None,
    }
}

impl AgentRunManager {
    pub(crate) async fn handle_runtime_channel_closed(
        self: &Arc<Self>,
        run_id: &str,
    ) -> Result<(), AppError> {
        if !self.has_active_run(run_id).await {
            return Ok(());
        }

        let runtime_state = self.runtime.session_state(run_id).await;
        let runtime_finish_state = self.runtime.session_finish_state(run_id).await;
        let cancellation_requested = self.was_cancel_requested(run_id).await;

        let Some(terminal_event) = (match runtime_finish_state {
            Some(RuntimeSessionFinishState::Completed) => None,
            Some(RuntimeSessionFinishState::Panicked(ref detail)) => {
                let error_message = runtime_panic_error_message(detail);
                run_repo::set_error_message(&self.pool, run_id, &error_message).await?;
                Some(ThreadStreamEvent::RunInterrupted {
                    run_id: run_id.to_string(),
                })
            }
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
        self: &Arc<Self>,
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

        let mut terminal_goal_context: Option<(
            String,
            RunStatus,
            broadcast::Sender<ThreadStreamEvent>,
        )> = None;

        match &event {
            ThreadStreamEvent::RunStarted { .. } => {
                run_repo::update_status(&self.pool, run_id, RunStatus::Running).await?;
                let thread_id = self.get_thread_id(run_id).await;
                thread_repo::update_status(&self.pool, &thread_id, &ThreadStatus::Running).await?;
            }
            ThreadStreamEvent::RunRetrying { .. } => {
                run_repo::update_status(&self.pool, run_id, RunStatus::Running).await?;
            }
            ThreadStreamEvent::RequestRetrying { .. } => {
                run_repo::update_status(&self.pool, run_id, RunStatus::Running).await?;
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
                    // Move the ID to last_completed so that tools executing
                    // after this event (e.g. render) can still find the target
                    // message for artifact attachment.
                    run.last_completed_message_id = run.streaming_message_id.take();
                }
            }
            ThreadStreamEvent::MessageDiscarded { message_id, .. } => {
                message_repo::update_status(&self.pool, message_id, "discarded").await?;
                // Clear streaming_message_id when the discarded message is the
                // one currently being streamed (e.g. request-level retry).
                // This ensures the next TextDelta triggers
                // ensure_streaming_message to INSERT a fresh DB row.
                let mut runs = self.active_runs.lock().await;
                if let Some(run) = runs.get_mut(run_id) {
                    if run.streaming_message_id.as_deref() == Some(message_id) {
                        run.streaming_message_id = None;
                    }
                }
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
            ThreadStreamEvent::UserMessageRecorded {
                message_id,
                content,
                created_at,
                metadata,
                ..
            } => {
                let thread_id = self.get_thread_id(run_id).await;
                if thread_id.is_empty() {
                    tracing::warn!(
                        run_id = %run_id,
                        message_id = %message_id,
                        "skipping consumed queue user message because active thread id is unavailable"
                    );
                } else {
                    persist_user_message_recorded(
                        &self.pool,
                        &thread_id,
                        message_id,
                        content,
                        created_at,
                        metadata.as_ref(),
                    )
                    .await?;
                }
            }
            ThreadStreamEvent::ToolRequested { .. } => {
                run_repo::update_status(&self.pool, run_id, RunStatus::WaitingToolResult).await?;
            }
            ThreadStreamEvent::SubagentStarted { .. } => {
                run_repo::update_status(&self.pool, run_id, RunStatus::WaitingToolResult).await?;
            }
            ThreadStreamEvent::ApprovalRequired { .. } => {
                let thread_id = self.get_thread_id(run_id).await;
                run_repo::update_status(&self.pool, run_id, RunStatus::WaitingApproval).await?;
                thread_repo::update_status(&self.pool, &thread_id, &ThreadStatus::WaitingApproval)
                    .await?;
            }
            ThreadStreamEvent::ClarifyRequired { .. } => {
                let thread_id = self.get_thread_id(run_id).await;
                run_repo::update_status(&self.pool, run_id, RunStatus::NeedsReply).await?;
                thread_repo::update_status(&self.pool, &thread_id, &ThreadStatus::NeedsReply)
                    .await?;
            }
            ThreadStreamEvent::ApprovalResolved { .. } => {
                run_repo::update_status(&self.pool, run_id, RunStatus::Running).await?;
                let thread_id = self.get_thread_id(run_id).await;
                thread_repo::update_status(&self.pool, &thread_id, &ThreadStatus::Running).await?;
            }
            ThreadStreamEvent::ClarifyResolved { .. } => {
                run_repo::update_status(&self.pool, run_id, RunStatus::Running).await?;
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
                    run_repo::update_status(&self.pool, run_id, RunStatus::Running).await?;
                }
            }
            ThreadStreamEvent::ThreadUsageUpdated { usage, .. } => {
                // `usage` here is a `RunUsageDto` populated by tiycore via
                // `From<tiycore::types::Usage>`. Its `context_size` field is
                // the cross-protocol unified "context occupancy" value
                // (`Usage::context_size()` = input + output + cache_read +
                // cache_write), NOT the wire-level `total_tokens` (which is
                // per-response and provider-dependent). We round-trip the
                // whole struct (input/output/cache fields + total_tokens)
                // through `tiycore::types::Usage` because the persistence
                // layer is typed against `Usage`; `context_size` is
                // re-derived inside the `From` impl on read, so storing only
                // the raw fields is sufficient and forward-compatible.
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
                let thread_id = self.get_thread_id(run_id).await;
                run_repo::update_status(&self.pool, run_id, RunStatus::WaitingApproval).await?;
                thread_repo::update_status(&self.pool, &thread_id, &ThreadStatus::WaitingApproval)
                    .await?;
            }
            ThreadStreamEvent::RunCompleted { .. }
            | ThreadStreamEvent::RunLimitReached { .. }
            | ThreadStreamEvent::RunFailed { .. }
            | ThreadStreamEvent::RunCancelled { .. }
            | ThreadStreamEvent::RunInterrupted { .. } => {
                let final_status =
                    terminal_event_status(&event, self.was_cancel_requested(run_id).await);
                if let Some(final_status) = final_status {
                    let error_message = match &event {
                        ThreadStreamEvent::RunLimitReached { error, .. }
                        | ThreadStreamEvent::RunFailed { error, .. } => Some(error.as_str()),
                        _ => None,
                    };
                    self.finish_run(run_id, final_status, error_message).await?;
                    let thread_id = self.get_thread_id(run_id).await;
                    if let Some(frontend_tx) = self.frontend_tx_for_run(run_id).await {
                        terminal_goal_context = Some((thread_id, final_status, frontend_tx));
                    }
                }
            }
            _ => {}
        }

        self.emit(run_id, event.clone()).await;

        // Broadcast global events for lifecycle transitions so that the frontend
        // sidebar can react even when this thread has no active stream subscription.
        match &event {
            ThreadStreamEvent::RunStarted { .. } => {
                let thread_id = self.get_thread_id(run_id).await;
                emit_app_event(
                    self.app_events.as_ref(),
                    app_events::THREAD_RUN_STARTED,
                    ThreadRunStartedPayload {
                        thread_id,
                        run_id: run_id.to_string(),
                    },
                );
            }
            _ => {}
        }

        // Broadcast file-changed event when a file-mutating tool completes,
        // so the frontend editor can reload open tabs.
        if let ThreadStreamEvent::ToolCompleted {
            tool_name, result, ..
        } = &event
        {
            if matches!(tool_name.as_str(), "write" | "edit" | "patch") {
                if let Some(path) = result.get("path").and_then(|v| v.as_str()) {
                    let thread_id = self.get_thread_id(run_id).await;
                    emit_app_event(
                        self.app_events.as_ref(),
                        app_events::AGENT_FILE_CHANGED,
                        AgentFileChangedPayload {
                            thread_id,
                            run_id: run_id.to_string(),
                            tool_name: tool_name.clone(),
                            path: path.to_string(),
                        },
                    );
                }
            }
        }

        // Broadcast unified status-changed event so the frontend sidebar can
        // track intermediate states (waiting_approval, needs_reply, etc.) for
        // background threads that have no active per-thread stream.
        if let Some(sidebar_status) =
            sidebar_status_for_runtime_event(&event, self.was_cancel_requested(run_id).await)
        {
            let thread_id = self.get_thread_id(run_id).await;
            emit_app_event(
                self.app_events.as_ref(),
                app_events::THREAD_RUN_STATUS_CHANGED,
                ThreadRunStatusChangedPayload {
                    thread_id,
                    run_id: run_id.to_string(),
                    status: sidebar_status.as_str().to_string(),
                },
            );
        }

        if is_terminal_runtime_event(&event) {
            self.runtime.remove_session(run_id).await;
            self.remove_active_run(run_id).await;
            if let Some((thread_id, final_status, frontend_tx)) = terminal_goal_context {
                if let Err(error) = self
                    .maybe_continue_goal_after_terminal_run(
                        run_id.to_string(),
                        thread_id,
                        final_status,
                        frontend_tx,
                    )
                    .await
                {
                    tracing::warn!(
                        run_id = %run_id,
                        error = %error,
                        "failed to orchestrate goal continuation after terminal run"
                    );
                }
            }
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

    pub(crate) async fn frontend_tx_for_run(
        &self,
        run_id: &str,
    ) -> Option<broadcast::Sender<ThreadStreamEvent>> {
        let runs = self.active_runs.lock().await;
        runs.get(run_id).map(|run| run.frontend_tx.clone())
    }

    pub(crate) async fn maybe_continue_goal_after_terminal_run(
        self: &Arc<Self>,
        completed_run_id: String,
        thread_id: String,
        final_status: RunStatus,
        frontend_tx: broadcast::Sender<ThreadStreamEvent>,
    ) -> Result<(), AppError> {
        if !self.goal_continuation_enabled {
            return Ok(());
        }

        if !matches!(final_status, RunStatus::Completed | RunStatus::Interrupted) {
            return Ok(());
        }

        if thread_id.is_empty() {
            return Ok(());
        }

        let mgr = crate::core::goal_manager::GoalManager::new(
            self.pool.clone(),
            thread_id.clone(),
            Arc::clone(&self.goal_runtime_state),
        );
        let Some(outcome) = mgr.evaluate_after_run(&completed_run_id, None).await? else {
            return Ok(());
        };

        let _ = frontend_tx.send(ThreadStreamEvent::GoalStateUpdated {
            thread_id: thread_id.clone(),
            goal: Some(outcome.goal.clone()),
        });

        if outcome.verdict == "paused" {
            let _ = frontend_tx.send(ThreadStreamEvent::GoalPaused {
                thread_id: thread_id.clone(),
                reason: outcome
                    .goal
                    .pause_reason
                    .as_ref()
                    .map(|reason| reason.as_str().to_string())
                    .unwrap_or_else(|| "unknown".to_string()),
                detail: outcome.goal.pause_detail.clone(),
            });
            return Ok(());
        }

        if outcome.verdict == "budget_limited" || outcome.verdict == "skipped" {
            return Ok(());
        }

        if outcome.verdict != "continue" && outcome.verdict != "challenge_evidence" {
            return Ok(());
        }

        let Some(prompt) = outcome
            .continuation_prompt
            .clone()
            .filter(|value| !value.trim().is_empty())
        else {
            tracing::warn!(
                run_id = %completed_run_id,
                thread_id = %thread_id,
                verdict = %outcome.verdict,
                "goal evaluation requested continuation without a prompt"
            );
            return Ok(());
        };

        let Some(start_context) =
            run_repo::find_start_context_by_id(&self.pool, &completed_run_id).await?
        else {
            tracing::warn!(
                run_id = %completed_run_id,
                thread_id = %thread_id,
                "skipping goal continuation because completed run context is missing"
            );
            return Ok(());
        };

        let Some(model_plan_json) = start_context.effective_model_plan_json else {
            tracing::warn!(
                run_id = %completed_run_id,
                thread_id = %thread_id,
                "skipping goal continuation because completed run model plan is missing"
            );
            return Ok(());
        };

        let model_plan_value = serde_json::from_str::<serde_json::Value>(&model_plan_json)
            .map_err(|error| {
                AppError::recoverable(
                    ErrorSource::Thread,
                    "thread.goal_continuation.model_plan_invalid",
                    format!("Failed to parse goal continuation model plan: {error}"),
                )
            })?;
        let (profile_id, provider_id, model_id) = extract_run_model_refs(&model_plan_value);

        // Build a clean display version of the continuation prompt: extract just the
        // first line "[Goal continuation — turns X/Y]" so the user sees a concise
        // label rather than the full LLM-internal system instruction.
        let display_prompt = prompt.lines().next().unwrap_or(&prompt).trim().to_string();
        let goal_continuation_metadata = serde_json::json!({ "goalContinuation": true, "turnsUsed": outcome.goal.turns_used, "maxTurns": outcome.goal.max_turns });

        let (continuation_run_id, _event_rx) = self
            .start_run_with_options(
                &thread_id,
                &prompt,
                Some(display_prompt),
                Some(goal_continuation_metadata),
                Vec::new(),
                &start_context.run_mode,
                profile_id,
                provider_id,
                model_id,
                model_plan_value,
                StartRunOptions {
                    history_override: None,
                    initial_prompt: Some(prompt.clone()),
                    persist_user_message: true,
                },
            )
            .await?;

        let _ = frontend_tx.send(ThreadStreamEvent::GoalContinuation {
            thread_id,
            run_id: continuation_run_id,
            prompt,
            turns_used: outcome.goal.turns_used,
            max_turns: outcome.goal.max_turns,
        });

        Ok(())
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
        // A new streaming message means any previous completed message is no
        // longer the "last" — clear it so tools attach to the new message.
        run.last_completed_message_id = None;
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
        status: RunStatus,
        error_message: Option<&str>,
    ) -> Result<(), AppError> {
        let finalized_message_status = if status == RunStatus::Failed {
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
            let should_discard = if status != RunStatus::Completed {
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

        // Backstop: converge any helper that is still non-terminal in the DB now
        // that the run is finishing. A subagent (e.g. a Judge) whose own cleanup
        // future was dropped during an interrupt/cancel can otherwise linger at
        // `running` forever, since the frontend only flips helper status from
        // live SubagentCompleted/Failed events. We mark them `interrupted` and
        // emit a matching SubagentFailed so both the DB snapshot and the live
        // event stream converge.
        let helper_interrupt_reason = match status {
            RunStatus::Cancelled => "Subagent interrupted because the run was cancelled.",
            RunStatus::Interrupted => "Subagent interrupted because the run was interrupted.",
            _ => "Subagent interrupted because the run finished while it was still active.",
        };
        match run_helper_repo::interrupt_non_terminal_by_run(
            &self.pool,
            run_id,
            helper_interrupt_reason,
        )
        .await
        {
            Ok(orphaned) => {
                if !orphaned.is_empty() {
                    tracing::warn!(
                        run_id = %run_id,
                        count = orphaned.len(),
                        "converged orphaned non-terminal run helpers to interrupted"
                    );
                }
                for helper in orphaned {
                    let _ = frontend_tx.send(ThreadStreamEvent::SubagentFailed {
                        run_id: run_id.to_string(),
                        subtask_id: helper.id,
                        helper_kind: helper.helper_kind,
                        started_at: helper.started_at,
                        error: helper_interrupt_reason.to_string(),
                        snapshot:
                            crate::core::subagent::orchestrator::SubagentProgressSnapshot::default(
                            ),
                    });
                }
            }
            Err(error) => {
                tracing::warn!(
                    run_id = %run_id,
                    %error,
                    "failed to converge orphaned run helpers while finishing run"
                );
            }
        }

        finalize_run(
            &self.pool,
            self.app_events.as_ref(),
            run_id,
            &thread_id,
            status,
            error_message,
        )
        .await?;

        if status == RunStatus::Completed {
            self.spawn_thread_title_generation(
                run_id.to_string(),
                thread_id,
                profile_id,
                frontend_tx,
                lightweight_model_role,
                auxiliary_model_role,
                primary_model_role,
                Arc::clone(&self.app_events),
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
        app_events: AppEventEmitterRef,
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
                app_events,
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
            | ThreadStreamEvent::RunRetrying { .. }
            | ThreadStreamEvent::RequestRetrying { .. }
            | ThreadStreamEvent::ReasoningUpdated { .. }
            | ThreadStreamEvent::UserMessageRecorded { .. }
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

#[cfg(test)]
mod tests {
    use super::{
        persist_final_run_state, persist_user_message_recorded, runtime_panic_error_message,
    };
    use crate::model::thread::{RunStatus, ThreadRecord, ThreadStatus};
    use crate::model::workspace::{WorkspaceKind, WorkspaceRecord, WorkspaceStatus};
    use crate::persistence::repo::{message_repo, run_repo, thread_repo, workspace_repo};
    use chrono::Utc;
    use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
    use sqlx::SqlitePool;
    use std::str::FromStr;

    async fn setup_test_pool() -> SqlitePool {
        let options = SqliteConnectOptions::from_str("sqlite::memory:")
            .expect("invalid sqlite options")
            .foreign_keys(true);

        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(options)
            .await
            .expect("failed to create in-memory pool");

        crate::persistence::sqlite::run_migrations(&pool)
            .await
            .expect("migrations failed");

        pool
    }

    async fn seed_run(pool: &SqlitePool) {
        let now = Utc::now();
        workspace_repo::insert(
            pool,
            &WorkspaceRecord {
                id: "workspace-1".to_string(),
                name: "Workspace".to_string(),
                path: "/tmp/workspace".to_string(),
                canonical_path: "/tmp/workspace".to_string(),
                display_path: "/tmp/workspace".to_string(),
                is_default: false,
                is_git: false,
                auto_work_tree: false,
                status: WorkspaceStatus::Ready,
                last_validated_at: None,
                created_at: now,
                updated_at: now,
                kind: WorkspaceKind::Standalone,
                parent_workspace_id: None,
                git_common_dir: None,
                branch: None,
                worktree_name: None,
            },
        )
        .await
        .expect("workspace insert");

        thread_repo::insert(
            pool,
            &ThreadRecord {
                id: "thread-1".to_string(),
                workspace_id: "workspace-1".to_string(),
                profile_id: None,
                title: "Thread".to_string(),
                status: ThreadStatus::Running,
                summary: None,
                last_active_at: now.to_rfc3339(),
                created_at: now.to_rfc3339(),
                updated_at: now.to_rfc3339(),
            },
        )
        .await
        .expect("thread insert");

        run_repo::insert(
            pool,
            &run_repo::RunInsert {
                id: "run-1".to_string(),
                thread_id: "thread-1".to_string(),
                profile_id: None,
                run_mode: "default".to_string(),
                provider_id: None,
                model_id: None,
                effective_model_plan_json: None,
                status: RunStatus::Running.as_str().to_string(),
            },
        )
        .await
        .expect("run insert");
    }

    #[tokio::test]
    async fn persist_user_message_recorded_inserts_plain_user_message() {
        let pool = setup_test_pool().await;
        seed_run(&pool).await;

        persist_user_message_recorded(
            &pool,
            "thread-1",
            "019e4470-0000-7000-8000-000000000001",
            "Use the simpler approach",
            "2026-05-20T12:00:00.000Z",
            None,
        )
        .await
        .expect("persist user message");

        let message = message_repo::find_by_id(&pool, "019e4470-0000-7000-8000-000000000001")
            .await
            .expect("message lookup")
            .expect("message exists");

        assert_eq!(message.thread_id, "thread-1");
        assert_eq!(message.run_id, None);
        assert_eq!(message.role, "user");
        assert_eq!(message.content_markdown, "Use the simpler approach");
        assert_eq!(message.message_type, "plain_message");
        assert_eq!(message.status, "completed");
        assert_eq!(message.parts_json, None);
        assert_eq!(message.metadata_json, None);
        assert_eq!(message.attachments_json, None);
        assert_eq!(message.created_at, "2026-05-20T12:00:00.000Z");
    }

    #[tokio::test]
    async fn persist_user_message_recorded_preserves_command_metadata() {
        let pool = setup_test_pool().await;
        seed_run(&pool).await;
        let metadata = serde_json::json!({
            "composer": {
                "kind": "command",
                "displayText": "/init",
                "effectivePrompt": "Generate or update AGENTS.md"
            }
        });

        persist_user_message_recorded(
            &pool,
            "thread-1",
            "019e4470-0000-7000-8000-000000000002",
            "/init",
            "2026-05-20T13:00:00.000Z",
            Some(&metadata),
        )
        .await
        .expect("persist command user message");

        let message = message_repo::find_by_id(&pool, "019e4470-0000-7000-8000-000000000002")
            .await
            .expect("message lookup")
            .expect("message exists");
        let persisted_metadata = message
            .metadata_json
            .as_deref()
            .and_then(|value| serde_json::from_str::<serde_json::Value>(value).ok())
            .expect("metadata json");

        assert_eq!(message.content_markdown, "/init");
        assert_eq!(message.created_at, "2026-05-20T13:00:00.000Z");
        assert_eq!(
            persisted_metadata
                .pointer("/composer/effectivePrompt")
                .and_then(serde_json::Value::as_str),
            Some("Generate or update AGENTS.md"),
        );
    }

    #[tokio::test]
    async fn persist_final_run_state_updates_run_error_and_thread_status() {
        let pool = setup_test_pool().await;
        seed_run(&pool).await;

        persist_final_run_state(
            &pool,
            "run-1",
            "thread-1",
            RunStatus::Failed,
            Some("runtime failed"),
        )
        .await
        .expect("persist final run state");

        let row = sqlx::query_as::<_, (String, Option<String>, Option<String>)>(
            "SELECT status, error_message, finished_at FROM thread_runs WHERE id = ?",
        )
        .bind("run-1")
        .fetch_one(&pool)
        .await
        .expect("run row");
        assert_eq!(row.0, "failed");
        assert_eq!(row.1.as_deref(), Some("runtime failed"));
        assert!(row.2.is_some(), "terminal run should receive finished_at");

        let thread = thread_repo::find_by_id(&pool, "thread-1")
            .await
            .expect("thread lookup")
            .expect("thread exists");
        assert_eq!(thread.status, ThreadStatus::Failed);
    }

    #[tokio::test]
    async fn persist_final_run_state_preserves_runtime_panic_detail_for_interrupted_run() {
        let pool = setup_test_pool().await;
        seed_run(&pool).await;
        let panic_detail = runtime_panic_error_message("Runtime task panicked: bad char boundary");

        persist_final_run_state(
            &pool,
            "run-1",
            "thread-1",
            RunStatus::Interrupted,
            Some(&panic_detail),
        )
        .await
        .expect("persist final run state");

        let row = sqlx::query_as::<_, (String, Option<String>, Option<String>)>(
            "SELECT status, error_message, finished_at FROM thread_runs WHERE id = ?",
        )
        .bind("run-1")
        .fetch_one(&pool)
        .await
        .expect("run row");
        assert_eq!(row.0, "interrupted");
        assert_eq!(
            row.1.as_deref(),
            Some("Runtime task panicked: bad char boundary")
        );
        assert!(row.2.is_some(), "terminal run should receive finished_at");

        let thread = thread_repo::find_by_id(&pool, "thread-1")
            .await
            .expect("thread lookup")
            .expect("thread exists");
        assert_eq!(thread.status, ThreadStatus::Interrupted);
    }

    #[test]
    fn runtime_panic_error_message_uses_default_for_empty_detail() {
        assert_eq!(
            runtime_panic_error_message("   "),
            "Runtime task panicked before completion."
        );
    }

    #[tokio::test]
    async fn persist_final_run_state_maps_limit_reached_to_needs_reply() {
        let pool = setup_test_pool().await;
        seed_run(&pool).await;

        persist_final_run_state(&pool, "run-1", "thread-1", RunStatus::LimitReached, None)
            .await
            .expect("persist final run state");

        let row = sqlx::query_as::<_, (String, Option<String>)>(
            "SELECT status, error_message FROM thread_runs WHERE id = ?",
        )
        .bind("run-1")
        .fetch_one(&pool)
        .await
        .expect("run row");
        assert_eq!(row.0, "limit_reached");
        assert_eq!(row.1, None);

        let thread = thread_repo::find_by_id(&pool, "thread-1")
            .await
            .expect("thread lookup")
            .expect("thread exists");
        assert_eq!(thread.status, ThreadStatus::NeedsReply);
    }
}
