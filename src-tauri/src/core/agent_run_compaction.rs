use std::sync::Arc;

use sqlx::SqlitePool;
use tiycore::agent::AgentMessage;
use tokio::sync::broadcast;

use crate::core::agent_run_summary::{generate_primary_summary, primary_summary_model};
use crate::core::agent_session::{
    build_session_spec, convert_history_messages, trim_history_to_current_context,
    ResolvedModelRole,
};
use crate::ipc::frontend_channels::ThreadStreamEvent;
use crate::model::errors::{AppError, ErrorSource};
use crate::model::thread::{MessageRecord, ThreadStatus};
use crate::persistence::repo::{
    message_repo, run_repo, thread_repo, tool_call_repo, workspace_repo,
};

use super::agent_run_manager::{ActiveRun, AgentRunManager, FRONTEND_EVENT_BUFFER_SIZE};

pub(crate) async fn persist_clear_context_reset_to_pool(
    pool: &SqlitePool,
    thread_id: &str,
) -> Result<(), AppError> {
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

    for message in &messages {
        message_repo::insert(pool, message).await?;
    }
    thread_repo::touch_active(pool, thread_id).await?;
    thread_repo::update_status(pool, thread_id, &ThreadStatus::Idle).await?;
    Ok(())
}

impl AgentRunManager {
    pub async fn clear_thread_context(&self, thread_id: &str) -> Result<(), AppError> {
        if self.cancel_run_if_active(thread_id).await? {
            tracing::info!(thread_id = %thread_id, "Cancelled active run before clearing context");
        }

        let thread = thread_repo::find_by_id(&self.pool, thread_id)
            .await?
            .ok_or_else(|| AppError::not_found(ErrorSource::Thread, "thread"))?;

        persist_clear_context_reset_to_pool(&self.pool, &thread.id).await
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
}

#[cfg(test)]
mod tests {
    use super::persist_clear_context_reset_to_pool;
    use crate::model::thread::{ThreadRecord, ThreadStatus};
    use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
    use sqlx::Row;
    use std::str::FromStr;

    async fn setup_test_pool() -> sqlx::SqlitePool {
        let options = SqliteConnectOptions::from_str("sqlite::memory:")
            .expect("invalid sqlite options")
            .foreign_keys(true);
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(options)
            .await
            .expect("failed to create in-memory pool");
        sqlx::migrate!("./migrations")
            .run(&pool)
            .await
            .expect("migrations failed");
        pool
    }

    async fn seed_workspace_and_thread(pool: &sqlx::SqlitePool) {
        let now = chrono::Utc::now().to_rfc3339();
        sqlx::query(
            "INSERT INTO workspaces (id, name, path, canonical_path, display_path,
                    is_default, is_git, auto_work_tree, status, created_at, updated_at)
             VALUES ('workspace-clear', 'Workspace', '/tmp/workspace-clear', '/tmp/workspace-clear', '/tmp/workspace-clear',
                    0, 0, 0, 'ready', ?, ?)",
        )
        .bind(&now)
        .bind(&now)
        .execute(pool)
        .await
        .expect("failed to seed workspace");

        crate::persistence::repo::thread_repo::insert(
            pool,
            &ThreadRecord {
                id: "thread-clear".to_string(),
                workspace_id: "workspace-clear".to_string(),
                profile_id: None,
                title: "Clear Thread".to_string(),
                status: ThreadStatus::Running,
                summary: None,
                last_active_at: now.clone(),
                created_at: now.clone(),
                updated_at: now,
            },
        )
        .await
        .expect("failed to seed thread");
    }

    #[tokio::test]
    async fn agent_run_compaction_persist_clear_context_reset_inserts_command_and_marker() {
        let pool = setup_test_pool().await;
        seed_workspace_and_thread(&pool).await;

        persist_clear_context_reset_to_pool(&pool, "thread-clear")
            .await
            .expect("clear context reset should persist");

        let rows = sqlx::query(
            r#"SELECT id, role, content_markdown, message_type, status, metadata_json
               FROM messages
               WHERE thread_id = ?
               ORDER BY id ASC"#,
        )
        .bind("thread-clear")
        .fetch_all(&pool)
        .await
        .expect("failed to read messages");

        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].get::<String, _>("role"), "user");
        assert_eq!(rows[0].get::<String, _>("content_markdown"), "/clear");
        assert_eq!(rows[0].get::<String, _>("message_type"), "plain_message");
        assert_eq!(rows[0].get::<String, _>("status"), "completed");
        let command_metadata: serde_json::Value = serde_json::from_str(
            rows[0]
                .get::<Option<String>, _>("metadata_json")
                .as_deref()
                .expect("command metadata should exist"),
        )
        .expect("command metadata should be valid json");
        assert_eq!(command_metadata["composer"]["kind"], "command");
        assert_eq!(command_metadata["composer"]["displayText"], "/clear");
        assert_eq!(command_metadata["composer"]["command"]["behavior"], "clear");

        assert_eq!(rows[1].get::<String, _>("role"), "system");
        assert_eq!(
            rows[1].get::<String, _>("content_markdown"),
            "Context is now reset"
        );
        assert_eq!(rows[1].get::<String, _>("message_type"), "summary_marker");
        assert_eq!(rows[1].get::<String, _>("status"), "completed");
        let reset_metadata: serde_json::Value = serde_json::from_str(
            rows[1]
                .get::<Option<String>, _>("metadata_json")
                .as_deref()
                .expect("reset metadata should exist"),
        )
        .expect("reset metadata should be valid json");
        assert_eq!(reset_metadata["kind"], "context_reset");
        assert_eq!(reset_metadata["source"], "clear");
        assert_eq!(reset_metadata["label"], "Context is now reset");

        let thread = crate::persistence::repo::thread_repo::find_by_id(&pool, "thread-clear")
            .await
            .expect("thread lookup should succeed")
            .expect("thread should exist");
        assert_eq!(thread.status, ThreadStatus::Idle);
    }
}
