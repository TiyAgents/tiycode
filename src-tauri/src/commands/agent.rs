use tauri::{ipc::Channel, State};
use tokio::sync::broadcast;

use crate::core::app_state::AppState;
use crate::core::plan_checkpoint::PlanApprovalAction;
use crate::ipc::frontend_channels::ThreadStreamEvent;
use crate::model::errors::AppError;
use crate::model::thread::MessageAttachmentDto;

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

fn forward_thread_stream_events(
    run_id: String,
    mut event_rx: broadcast::Receiver<ThreadStreamEvent>,
    on_event: Channel<ThreadStreamEvent>,
) {
    tokio::spawn(async move {
        loop {
            match event_rx.recv().await {
                Ok(event) => {
                    if on_event.send(event).is_err() {
                        break;
                    }
                }
                Err(broadcast::error::RecvError::Closed) => break,
                Err(broadcast::error::RecvError::Lagged(dropped_events)) => {
                    if on_event
                        .send(ThreadStreamEvent::StreamResyncRequired {
                            run_id: run_id.clone(),
                            dropped_events: dropped_events as u64,
                        })
                        .is_err()
                    {
                        break;
                    }
                }
            }
        }
    });
}

async fn handle_tool_approval_response(
    tool_gateway: &crate::core::tool_gateway::ToolGateway,
    tool_call_id: &str,
    run_id: &str,
    approved: bool,
) -> Result<(), AppError> {
    let found = tool_gateway
        .resolve_approval(tool_call_id, approved)
        .await?;

    if !found {
        // Silently ignore duplicate approval responses (e.g. user double-clicked the button).
        tracing::warn!(
            tool_call_id = %tool_call_id,
            run_id = %run_id,
            "Ignoring duplicate tool approval response — no pending approval found"
        );
    }

    Ok(())
}

#[tauri::command]
pub async fn thread_start_run(
    state: State<'_, AppState>,
    thread_id: String,
    prompt: String,
    display_prompt: Option<String>,
    prompt_metadata: Option<serde_json::Value>,
    attachments: Option<Vec<MessageAttachmentDto>>,
    run_mode: Option<String>,
    model_plan: Option<serde_json::Value>,
    on_event: Channel<ThreadStreamEvent>,
) -> Result<String, AppError> {
    let run_mode = run_mode.unwrap_or_else(|| "default".to_string());
    let model_plan = model_plan.unwrap_or_default();
    let (profile_id, provider_id, model_id) = extract_run_model_refs(&model_plan);

    let (run_id, event_rx) = state
        .agent_run_manager
        .clone()
        .start_run(
            &thread_id,
            &prompt,
            display_prompt,
            prompt_metadata,
            attachments.unwrap_or_default(),
            &run_mode,
            profile_id,
            provider_id,
            model_id,
            model_plan,
        )
        .await?;

    // Forward events from the internal channel to the Tauri Channel
    forward_thread_stream_events(run_id.clone(), event_rx, on_event);

    Ok(run_id)
}

#[tauri::command]
pub async fn thread_subscribe_run(
    state: State<'_, AppState>,
    thread_id: String,
    on_event: Channel<ThreadStreamEvent>,
) -> Result<Option<String>, AppError> {
    let Some((run_id, event_rx)) = state.agent_run_manager.subscribe_run(&thread_id).await? else {
        return Ok(None);
    };

    forward_thread_stream_events(run_id.clone(), event_rx, on_event);
    Ok(Some(run_id))
}

#[tauri::command]
pub async fn thread_execute_approved_plan(
    state: State<'_, AppState>,
    thread_id: String,
    approval_message_id: String,
    action: String,
    on_event: Channel<ThreadStreamEvent>,
) -> Result<String, AppError> {
    let action = match action.as_str() {
        "apply_plan" => PlanApprovalAction::ApplyPlan,
        "apply_plan_with_context_reset" => PlanApprovalAction::ApplyPlanWithContextReset,
        other => {
            return Err(AppError::recoverable(
                crate::model::errors::ErrorSource::Thread,
                "thread.plan_approval.invalid_action",
                format!("Unsupported plan approval action '{other}'"),
            ));
        }
    };

    let (run_id, event_rx) = state
        .agent_run_manager
        .clone()
        .execute_approved_plan(&thread_id, &approval_message_id, action)
        .await?;

    forward_thread_stream_events(run_id.clone(), event_rx, on_event);
    Ok(run_id)
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;
    use std::sync::Arc;

    use serde_json::json;
    use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
    use sqlx::Row;

    use super::{extract_run_model_refs, handle_tool_approval_response};
    use crate::core::terminal_manager::TerminalManager;
    use crate::core::tool_gateway::{
        ToolExecutionOptions, ToolExecutionRequest, ToolGateway, ToolGatewayResult,
    };

    async fn setup_test_pool() -> sqlx::SqlitePool {
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

    async fn seed_workspace(pool: &sqlx::SqlitePool, id: &str, canonical_path: &str) {
        let now = chrono::Utc::now().to_rfc3339();
        sqlx::query(
            "INSERT INTO workspaces (id, name, path, canonical_path, display_path,
                    is_default, is_git, auto_work_tree, status, created_at, updated_at)
             VALUES (?, ?, ?, ?, ?, 0, 0, 0, 'ready', ?, ?)",
        )
        .bind(id)
        .bind("Test Workspace")
        .bind(canonical_path)
        .bind(canonical_path)
        .bind(canonical_path)
        .bind(&now)
        .bind(&now)
        .execute(pool)
        .await
        .expect("failed to seed workspace");
    }

    async fn seed_thread(
        pool: &sqlx::SqlitePool,
        thread_id: &str,
        workspace_id: &str,
        profile_id: Option<&str>,
    ) {
        let now = chrono::Utc::now().to_rfc3339();
        sqlx::query(
            "INSERT INTO threads (id, workspace_id, profile_id, title, status, last_active_at, created_at, updated_at)
             VALUES (?, ?, ?, 'Test Thread', 'idle', ?, ?, ?)",
        )
        .bind(thread_id)
        .bind(workspace_id)
        .bind(profile_id)
        .bind(&now)
        .bind(&now)
        .bind(&now)
        .execute(pool)
        .await
        .expect("failed to seed thread");
    }

    async fn seed_run(
        pool: &sqlx::SqlitePool,
        run_id: &str,
        thread_id: &str,
        status: &str,
        run_mode: &str,
    ) {
        let now = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Nanos, true);
        sqlx::query(
            "INSERT INTO thread_runs (id, thread_id, run_mode, status, started_at)
             VALUES (?, ?, ?, ?, ?)",
        )
        .bind(run_id)
        .bind(thread_id)
        .bind(run_mode)
        .bind(status)
        .bind(&now)
        .execute(pool)
        .await
        .expect("failed to seed run");
    }

    async fn seed_tool_call(
        pool: &sqlx::SqlitePool,
        tool_call_id: &str,
        run_id: &str,
        thread_id: &str,
        tool_name: &str,
        status: &str,
    ) {
        sqlx::query(
            "INSERT INTO tool_calls (id, run_id, thread_id, tool_name, tool_input_json, status, started_at)
             VALUES (?, ?, ?, ?, '{}', ?, strftime('%Y-%m-%dT%H:%M:%fZ','now'))",
        )
        .bind(tool_call_id)
        .bind(run_id)
        .bind(thread_id)
        .bind(tool_name)
        .bind(status)
        .execute(pool)
        .await
        .expect("failed to seed tool call");
    }

    async fn seed_policy(pool: &sqlx::SqlitePool, key: &str, value_json: &str) {
        sqlx::query("INSERT OR REPLACE INTO policies (key, value_json) VALUES (?, ?)")
            .bind(key)
            .bind(value_json)
            .execute(pool)
            .await
            .expect("failed to seed policy");
    }

    #[test]
    fn extracts_profile_and_primary_refs_from_model_plan() {
        let model_plan = json!({
            "profileId": "profile-1",
            "primary": {
                "providerId": "provider-1",
                "modelRecordId": "model-record-1",
                "modelId": "gpt-5"
            }
        });

        let (profile_id, provider_id, model_id) = extract_run_model_refs(&model_plan);

        assert_eq!(profile_id.as_deref(), Some("profile-1"));
        assert_eq!(provider_id.as_deref(), Some("provider-1"));
        assert_eq!(model_id.as_deref(), Some("model-record-1"));
    }

    #[test]
    fn falls_back_to_primary_model_id_when_record_id_is_missing() {
        let model_plan = json!({
            "primary": {
                "providerId": "provider-1",
                "modelId": "gpt-5"
            }
        });

        let (_, provider_id, model_id) = extract_run_model_refs(&model_plan);

        assert_eq!(provider_id.as_deref(), Some("provider-1"));
        assert_eq!(model_id.as_deref(), Some("gpt-5"));
    }

    #[tokio::test]
    async fn duplicate_tool_approval_responses_are_ignored_after_first_success() {
        let pool = setup_test_pool().await;
        let workspace_root = tempfile::tempdir().expect("failed to create temp dir");
        let workspace_path = workspace_root.path().join("workspace");
        std::fs::create_dir_all(&workspace_path).expect("failed to create workspace directory");

        let target_file = workspace_path.join("README.md");
        std::fs::write(&target_file, "hello\n").expect("failed to write test file");

        let workspace_path =
            std::fs::canonicalize(&workspace_path).expect("failed to canonicalize workspace");
        let target_file =
            std::fs::canonicalize(&target_file).expect("failed to canonicalize test file");

        seed_workspace(&pool, "ws-dup-approval", workspace_path.to_str().unwrap()).await;
        seed_thread(&pool, "t-dup-approval", "ws-dup-approval", None).await;
        seed_run(
            &pool,
            "r-dup-approval",
            "t-dup-approval",
            "running",
            "default",
        )
        .await;
        seed_tool_call(
            &pool,
            "tc-dup-approval",
            "r-dup-approval",
            "t-dup-approval",
            "write",
            "requested",
        )
        .await;
        seed_policy(&pool, "approval_policy", r#""require_all""#).await;

        let terminal_manager = Arc::new(TerminalManager::new(pool.clone()));
        let gateway = Arc::new(ToolGateway::new(pool.clone(), terminal_manager));
        let (approval_requested_tx, approval_requested_rx) = tokio::sync::oneshot::channel();

        let execution_gateway = Arc::clone(&gateway);
        let workspace_path_text = workspace_path.display().to_string();
        let target_file_text = target_file.display().to_string();
        let execution = tokio::spawn(async move {
            execution_gateway
                .execute_tool_call(
                    ToolExecutionRequest {
                        run_id: "r-dup-approval".into(),
                        thread_id: "t-dup-approval".into(),
                        tool_call_id: "tc-dup-approval".into(),
                        tool_name: "write".into(),
                        tool_input: serde_json::json!({
                            "path": target_file_text,
                            "content": "updated by approval\n",
                        }),
                        workspace_path: workspace_path_text,
                        run_mode: "default".into(),
                    },
                    tiycore::agent::AbortSignal::new(),
                    ToolExecutionOptions::default(),
                    {
                        let mut approval_requested_tx = Some(approval_requested_tx);
                        move |_| {
                            if let Some(tx) = approval_requested_tx.take() {
                                let _ = tx.send(());
                            }
                        }
                    },
                    || {},
                )
                .await
        });

        approval_requested_rx
            .await
            .expect("approval should have been requested");

        handle_tool_approval_response(gateway.as_ref(), "tc-dup-approval", "r-dup-approval", true)
            .await
            .expect("first approval response should succeed");

        handle_tool_approval_response(gateway.as_ref(), "tc-dup-approval", "r-dup-approval", true)
            .await
            .expect("duplicate approval response should be ignored");

        let outcome = execution
            .await
            .expect("execution task should join")
            .expect("tool execution should succeed");

        match outcome.result {
            ToolGatewayResult::Executed { .. } => {}
            other => panic!(
                "expected executed outcome after approval, got {:?}",
                std::mem::discriminant(&other)
            ),
        }

        let row = sqlx::query(
            "SELECT status, approval_status FROM tool_calls WHERE id = 'tc-dup-approval'",
        )
        .fetch_one(&pool)
        .await
        .expect("tool call row should exist");

        assert_eq!(row.get::<String, _>("status"), "completed");
        assert_eq!(
            row.get::<Option<String>, _>("approval_status").unwrap(),
            "approved"
        );
        assert_eq!(
            std::fs::read_to_string(&target_file).expect("failed to read updated file"),
            "updated by approval\n"
        );
    }
}

#[tauri::command]
pub async fn thread_clear_context(
    state: State<'_, AppState>,
    thread_id: String,
) -> Result<(), AppError> {
    state
        .agent_run_manager
        .clear_thread_context(&thread_id)
        .await
}

#[tauri::command]
pub async fn thread_compact_context(
    state: State<'_, AppState>,
    thread_id: String,
    instructions: Option<String>,
    model_plan: Option<serde_json::Value>,
) -> Result<(), AppError> {
    state
        .agent_run_manager
        .compact_thread_context(&thread_id, instructions, model_plan.unwrap_or_default())
        .await
}

#[tauri::command]
pub async fn thread_cancel_run(
    state: State<'_, AppState>,
    thread_id: String,
) -> Result<bool, AppError> {
    state.agent_run_manager.cancel_run(&thread_id).await
}

#[tauri::command]
pub async fn tool_approval_respond(
    state: State<'_, AppState>,
    tool_call_id: String,
    run_id: String,
    approved: bool,
) -> Result<(), AppError> {
    handle_tool_approval_response(
        state.tool_gateway.as_ref(),
        &tool_call_id,
        &run_id,
        approved,
    )
    .await
}

#[tauri::command]
pub async fn tool_clarify_respond(
    state: State<'_, AppState>,
    tool_call_id: String,
    response: serde_json::Value,
) -> Result<(), AppError> {
    let found = state
        .tool_gateway
        .resolve_clarification(&tool_call_id, response)
        .await?;

    if !found {
        return Err(AppError::recoverable(
            crate::model::errors::ErrorSource::Tool,
            "tool.clarify.not_found",
            format!("No pending clarification was found for tool call '{tool_call_id}'"),
        ));
    }

    Ok(())
}
