//! M1.5 — Agent Run & Sidecar connection tests
//!
//! Acceptance criteria:
//! - Run state machine: Created → Dispatching → Running ⇄ WaitingApproval → Completed/Failed/Cancelled/Interrupted
//! - Crash recovery marks dangling runs as interrupted
//! - Sidecar protocol types parse correctly

mod test_helpers;

use sqlx::Row;

// =========================================================================
// T1.5.1 — Run lifecycle state machine
// =========================================================================

#[tokio::test]
async fn test_run_creation_with_default_status() {
    let pool = test_helpers::setup_test_pool().await;
    test_helpers::seed_workspace(&pool, "ws-run", "/tmp/run").await;
    test_helpers::seed_thread(&pool, "t-run", "ws-run").await;
    test_helpers::seed_run(&pool, "r-create", "t-run", "created", "default").await;

    let row = sqlx::query("SELECT status, run_mode FROM thread_runs WHERE id = 'r-create'")
        .fetch_one(&pool)
        .await
        .unwrap();

    assert_eq!(row.get::<String, _>("status"), "created");
    assert_eq!(row.get::<String, _>("run_mode"), "default");
}

#[tokio::test]
async fn test_run_state_transitions() {
    let pool = test_helpers::setup_test_pool().await;
    test_helpers::seed_workspace(&pool, "ws-sm", "/tmp/sm").await;
    test_helpers::seed_thread(&pool, "t-sm", "ws-sm").await;
    test_helpers::seed_run(&pool, "r-sm", "t-sm", "created", "default").await;

    // Transition: created → dispatching
    sqlx::query("UPDATE thread_runs SET status = 'dispatching' WHERE id = 'r-sm'")
        .execute(&pool)
        .await
        .unwrap();

    // Transition: dispatching → running
    sqlx::query("UPDATE thread_runs SET status = 'running' WHERE id = 'r-sm'")
        .execute(&pool)
        .await
        .unwrap();

    // Transition: running → waiting_approval
    sqlx::query("UPDATE thread_runs SET status = 'waiting_approval' WHERE id = 'r-sm'")
        .execute(&pool)
        .await
        .unwrap();

    // Transition: waiting_approval → running (after approval)
    sqlx::query("UPDATE thread_runs SET status = 'running' WHERE id = 'r-sm'")
        .execute(&pool)
        .await
        .unwrap();

    // Transition: running → completed
    sqlx::query("UPDATE thread_runs SET status = 'completed', finished_at = strftime('%Y-%m-%dT%H:%M:%fZ','now') WHERE id = 'r-sm'")
        .execute(&pool)
        .await
        .unwrap();

    let row = sqlx::query("SELECT status, finished_at FROM thread_runs WHERE id = 'r-sm'")
        .fetch_one(&pool)
        .await
        .unwrap();

    assert_eq!(row.get::<String, _>("status"), "completed");
    assert!(row.get::<Option<String>, _>("finished_at").is_some());
}

#[tokio::test]
async fn test_run_failure_with_error() {
    let pool = test_helpers::setup_test_pool().await;
    test_helpers::seed_workspace(&pool, "ws-fail", "/tmp/fail").await;
    test_helpers::seed_thread(&pool, "t-fail", "ws-fail").await;
    test_helpers::seed_run(&pool, "r-fail", "t-fail", "running", "default").await;

    sqlx::query(
        "UPDATE thread_runs SET status = 'failed', error_message = 'LLM timeout', finished_at = strftime('%Y-%m-%dT%H:%M:%fZ','now') WHERE id = 'r-fail'",
    )
    .execute(&pool)
    .await
    .unwrap();

    let row = sqlx::query("SELECT status, error_message FROM thread_runs WHERE id = 'r-fail'")
        .fetch_one(&pool)
        .await
        .unwrap();

    assert_eq!(row.get::<String, _>("status"), "failed");
    assert_eq!(
        row.get::<Option<String>, _>("error_message").unwrap(),
        "LLM timeout"
    );
}

#[tokio::test]
async fn test_run_cancellation() {
    let pool = test_helpers::setup_test_pool().await;
    test_helpers::seed_workspace(&pool, "ws-cancel", "/tmp/cancel").await;
    test_helpers::seed_thread(&pool, "t-cancel", "ws-cancel").await;
    test_helpers::seed_run(&pool, "r-cancel", "t-cancel", "running", "default").await;

    sqlx::query("UPDATE thread_runs SET status = 'cancelled', finished_at = strftime('%Y-%m-%dT%H:%M:%fZ','now') WHERE id = 'r-cancel'")
        .execute(&pool)
        .await
        .unwrap();

    let row = sqlx::query("SELECT status FROM thread_runs WHERE id = 'r-cancel'")
        .fetch_one(&pool)
        .await
        .unwrap();

    assert_eq!(row.get::<String, _>("status"), "cancelled");
}

// =========================================================================
// T1.5.2 — Crash recovery (interrupted runs)
// =========================================================================

#[tokio::test]
async fn test_recover_interrupted_runs() {
    let pool = test_helpers::setup_test_pool().await;
    test_helpers::seed_workspace(&pool, "ws-rec", "/tmp/rec").await;
    test_helpers::seed_thread(&pool, "t-rec", "ws-rec").await;

    // Create "dangling" runs that were in-progress when app crashed
    test_helpers::seed_run(&pool, "r-dangling-1", "t-rec", "running", "default").await;
    test_helpers::seed_run(&pool, "r-dangling-2", "t-rec", "dispatching", "default").await;
    test_helpers::seed_run(&pool, "r-ok", "t-rec", "completed", "default").await;

    // Simulate startup recovery: mark all non-terminal runs as interrupted
    sqlx::query(
        "UPDATE thread_runs SET status = 'interrupted', finished_at = strftime('%Y-%m-%dT%H:%M:%fZ','now')
         WHERE status NOT IN ('completed', 'failed', 'denied', 'interrupted', 'cancelled')",
    )
    .execute(&pool)
    .await
    .unwrap();

    // Verify dangling runs are now interrupted
    let dangling1 = sqlx::query("SELECT status FROM thread_runs WHERE id = 'r-dangling-1'")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(dangling1.get::<String, _>("status"), "interrupted");

    let dangling2 = sqlx::query("SELECT status FROM thread_runs WHERE id = 'r-dangling-2'")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(dangling2.get::<String, _>("status"), "interrupted");

    // Verify completed run is untouched
    let ok = sqlx::query("SELECT status FROM thread_runs WHERE id = 'r-ok'")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(ok.get::<String, _>("status"), "completed");
}

// =========================================================================
// T1.5.3 — Active runs index (only non-terminal runs)
// =========================================================================

#[tokio::test]
async fn test_active_runs_index() {
    let pool = test_helpers::setup_test_pool().await;
    test_helpers::seed_workspace(&pool, "ws-idx", "/tmp/idx").await;
    test_helpers::seed_thread(&pool, "t-idx", "ws-idx").await;

    test_helpers::seed_run(&pool, "r-active", "t-idx", "running", "default").await;
    test_helpers::seed_run(&pool, "r-done", "t-idx", "completed", "default").await;
    test_helpers::seed_run(&pool, "r-wait", "t-idx", "waiting_approval", "default").await;

    // Query active runs using the partial index pattern
    let rows = sqlx::query(
        "SELECT id FROM thread_runs WHERE thread_id = 't-idx'
         AND status NOT IN ('completed', 'failed', 'denied', 'interrupted', 'cancelled')",
    )
    .fetch_all(&pool)
    .await
    .unwrap();

    let ids: Vec<String> = rows.iter().map(|r| r.get("id")).collect();
    assert!(ids.contains(&"r-active".to_string()));
    assert!(ids.contains(&"r-wait".to_string()));
    assert!(!ids.contains(&"r-done".to_string()));
}

// =========================================================================
// T1.5.4 — One active run per thread constraint
// =========================================================================

#[tokio::test]
async fn test_only_one_active_run_per_thread_check() {
    let pool = test_helpers::setup_test_pool().await;
    test_helpers::seed_workspace(&pool, "ws-1run", "/tmp/1run").await;
    test_helpers::seed_thread(&pool, "t-1run", "ws-1run").await;
    test_helpers::seed_run(&pool, "r-existing", "t-1run", "running", "default").await;

    // Application-level check: count active runs before starting a new one
    let active_count = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM thread_runs WHERE thread_id = 't-1run'
         AND status NOT IN ('completed', 'failed', 'denied', 'interrupted', 'cancelled')",
    )
    .fetch_one(&pool)
    .await
    .unwrap();

    assert_eq!(active_count, 1, "Should have exactly 1 active run");
    // Application code should reject starting a new run when active_count > 0
}

// =========================================================================
// T1.5.5 — Effective model plan freeze
// =========================================================================

#[tokio::test]
async fn test_effective_model_plan_stored() {
    let pool = test_helpers::setup_test_pool().await;
    test_helpers::seed_workspace(&pool, "ws-mp", "/tmp/mp").await;
    test_helpers::seed_thread(&pool, "t-mp", "ws-mp").await;

    let model_plan = r#"{"primary":{"provider":"openai","model":"gpt-4"},"auxiliary":{"provider":"anthropic","model":"claude-3"}}"#;

    sqlx::query(
        "INSERT INTO thread_runs (id, thread_id, run_mode, status, effective_model_plan_json)
         VALUES ('r-mp', 't-mp', 'default', 'running', ?)",
    )
    .bind(model_plan)
    .execute(&pool)
    .await
    .unwrap();

    let row = sqlx::query("SELECT effective_model_plan_json FROM thread_runs WHERE id = 'r-mp'")
        .fetch_one(&pool)
        .await
        .unwrap();

    let plan: serde_json::Value =
        serde_json::from_str(&row.get::<String, _>("effective_model_plan_json")).unwrap();
    assert_eq!(plan["primary"]["model"].as_str().unwrap(), "gpt-4");
}

// =========================================================================
// T1.5.6 — Sidecar protocol event parsing
// =========================================================================

#[test]
fn test_sidecar_event_parse_message_delta() {
    use tiy_agent_lib::ipc::sidecar_protocol::SidecarEvent;

    let payload = serde_json::json!({
        "runId": "run-123",
        "messageId": "msg-456",
        "delta": "Hello "
    });

    let event = SidecarEvent::parse("agent.message.delta", payload);
    assert!(event.is_some());

    match event.unwrap() {
        SidecarEvent::MessageDelta {
            run_id,
            message_id,
            delta,
        } => {
            assert_eq!(run_id, "run-123");
            assert_eq!(message_id, "msg-456");
            assert_eq!(delta, "Hello ");
        }
        _ => panic!("Expected MessageDelta"),
    }
}

#[test]
fn test_sidecar_event_parse_tool_requested() {
    use tiy_agent_lib::ipc::sidecar_protocol::SidecarEvent;

    let payload = serde_json::json!({
        "runId": "run-789",
        "toolCallId": "tc-001",
        "toolName": "read_file",
        "toolInput": {"path": "/src/main.rs"}
    });

    let event = SidecarEvent::parse("agent.tool.requested", payload);
    assert!(event.is_some());

    match event.unwrap() {
        SidecarEvent::ToolRequested {
            run_id,
            tool_call_id,
            tool_name,
            tool_input,
        } => {
            assert_eq!(run_id, "run-789");
            assert_eq!(tool_call_id, "tc-001");
            assert_eq!(tool_name, "read_file");
            assert_eq!(tool_input["path"].as_str().unwrap(), "/src/main.rs");
        }
        _ => panic!("Expected ToolRequested"),
    }
}

#[test]
fn test_sidecar_event_parse_run_completed() {
    use tiy_agent_lib::ipc::sidecar_protocol::SidecarEvent;

    let payload = serde_json::json!({"runId": "run-done"});
    let event = SidecarEvent::parse("agent.run.completed", payload);

    match event.unwrap() {
        SidecarEvent::RunCompleted { run_id } => assert_eq!(run_id, "run-done"),
        _ => panic!("Expected RunCompleted"),
    }
}

#[test]
fn test_sidecar_event_parse_run_failed() {
    use tiy_agent_lib::ipc::sidecar_protocol::SidecarEvent;

    let payload = serde_json::json!({"runId": "run-err", "error": "provider timeout"});
    let event = SidecarEvent::parse("agent.run.failed", payload);

    match event.unwrap() {
        SidecarEvent::RunFailed { run_id, error } => {
            assert_eq!(run_id, "run-err");
            assert_eq!(error, "provider timeout");
        }
        _ => panic!("Expected RunFailed"),
    }
}

#[test]
fn test_sidecar_event_parse_unknown_returns_none() {
    use tiy_agent_lib::ipc::sidecar_protocol::SidecarEvent;

    let payload = serde_json::json!({"data": "something"});
    let event = SidecarEvent::parse("unknown.event.type", payload);
    assert!(event.is_none());
}

#[test]
fn test_sidecar_event_run_id_accessor() {
    use tiy_agent_lib::ipc::sidecar_protocol::SidecarEvent;

    let event = SidecarEvent::RunCompleted {
        run_id: "test-run".to_string(),
    };
    assert_eq!(event.run_id(), "test-run");
}

#[test]
fn test_sidecar_event_parse_plan_updated() {
    use tiy_agent_lib::ipc::sidecar_protocol::SidecarEvent;

    let payload = serde_json::json!({
        "runId": "run-plan",
        "plan": {"steps": ["analyze", "implement"]}
    });

    let event = SidecarEvent::parse("agent.plan.updated", payload);
    match event.unwrap() {
        SidecarEvent::PlanUpdated { run_id, plan } => {
            assert_eq!(run_id, "run-plan");
            assert!(plan["steps"].is_array());
        }
        _ => panic!("Expected PlanUpdated"),
    }
}

#[test]
fn test_sidecar_event_parse_subagent_events() {
    use tiy_agent_lib::ipc::sidecar_protocol::SidecarEvent;

    // SubagentStarted
    let payload = serde_json::json!({"runId": "r1", "subtaskId": "st1"});
    match SidecarEvent::parse("agent.subagent.started", payload).unwrap() {
        SidecarEvent::SubagentStarted { run_id, subtask_id } => {
            assert_eq!(run_id, "r1");
            assert_eq!(subtask_id, "st1");
        }
        _ => panic!("Expected SubagentStarted"),
    }

    // SubagentCompleted
    let payload = serde_json::json!({"runId": "r1", "subtaskId": "st1", "summary": "done"});
    match SidecarEvent::parse("agent.subagent.completed", payload).unwrap() {
        SidecarEvent::SubagentCompleted { summary, .. } => {
            assert_eq!(summary.unwrap(), "done");
        }
        _ => panic!("Expected SubagentCompleted"),
    }

    // SubagentFailed
    let payload = serde_json::json!({"runId": "r1", "subtaskId": "st1", "error": "timeout"});
    match SidecarEvent::parse("agent.subagent.failed", payload).unwrap() {
        SidecarEvent::SubagentFailed { error, .. } => {
            assert_eq!(error, "timeout");
        }
        _ => panic!("Expected SubagentFailed"),
    }
}
