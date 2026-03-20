//! M1.5 — Built-in agent runtime tests
//!
//! Acceptance criteria:
//! - Run state machine: Created → Dispatching → Running ⇄ WaitingApproval → Completed/Failed/Cancelled/Interrupted
//! - Crash recovery marks dangling runs as interrupted
//! - Runtime model plan resolves into executable built-in agent sessions

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
// T1.5.6 — Built-in runtime session configuration
// =========================================================================

#[tokio::test]
async fn test_build_session_spec_resolves_primary_model_and_profile_prompt() {
    use tiy_agent_lib::core::agent_session::build_session_spec;

    let pool = test_helpers::setup_test_pool().await;
    test_helpers::seed_workspace(&pool, "ws-runtime", "/tmp/runtime").await;
    test_helpers::seed_thread(&pool, "t-runtime", "ws-runtime").await;
    test_helpers::seed_message(
        &pool,
        "m-runtime",
        "t-runtime",
        "user",
        "Explain this project",
    )
    .await;

    sqlx::query(
        "INSERT INTO providers (
            id, provider_kind, provider_key, name, protocol_type, base_url,
            api_key_encrypted, enabled, mapping_locked
         ) VALUES ('prov-runtime', 'builtin', 'openai', 'OpenAI', 'openai',
                   'https://api.openai.com/v1', 'sk-test', 1, 1)",
    )
    .execute(&pool)
    .await
    .unwrap();

    sqlx::query(
        "INSERT INTO agent_profiles (id, name, custom_instructions, primary_provider_id, primary_model_id, is_default)
         VALUES ('profile-runtime', 'Runtime Profile', 'Always answer in concise engineering prose.', 'prov-runtime', 'model-record-runtime', 1)",
    )
    .execute(&pool)
    .await
    .unwrap();

    let model_plan = serde_json::json!({
        "profileId": "profile-runtime",
        "primary": {
            "providerId": "prov-runtime",
            "modelRecordId": "model-record-runtime",
            "providerType": "openai",
            "providerName": "OpenAI",
            "model": "gpt-4.1",
            "modelId": "gpt-4.1",
            "modelDisplayName": "GPT-4.1",
            "baseUrl": "https://api.openai.com/v1",
            "contextWindow": "128000",
            "maxOutputTokens": "16384"
        }
    });

    let spec = build_session_spec(
        &pool,
        "run-runtime",
        "t-runtime",
        "/tmp/runtime",
        "default",
        &model_plan,
    )
    .await
    .unwrap();

    assert_eq!(spec.run_id, "run-runtime");
    assert_eq!(spec.model_plan.primary.model.id, "gpt-4.1");
    assert_eq!(spec.model_plan.primary.provider_id, "prov-runtime");
    assert_eq!(spec.model_plan.primary.api_key.as_deref(), Some("sk-test"));
    assert_eq!(spec.tool_profile_name, "default_full");
    assert!(spec
        .system_prompt
        .contains("Always answer in concise engineering prose."));
    assert_eq!(spec.history_messages.len(), 1);
}

#[tokio::test]
async fn test_build_session_spec_adds_plan_mode_guardrails() {
    use tiy_agent_lib::core::agent_session::build_session_spec;

    let pool = test_helpers::setup_test_pool().await;
    test_helpers::seed_workspace(&pool, "ws-plan", "/tmp/plan").await;
    test_helpers::seed_thread(&pool, "t-plan", "ws-plan").await;
    test_helpers::seed_message(
        &pool,
        "m-plan",
        "t-plan",
        "user",
        "Draft an implementation plan",
    )
    .await;

    sqlx::query(
        "INSERT INTO providers (
            id, provider_kind, provider_key, name, protocol_type, base_url,
            api_key_encrypted, enabled, mapping_locked
         ) VALUES ('prov-plan', 'builtin', 'openai', 'OpenAI', 'openai',
                   'https://api.openai.com/v1', 'sk-test', 1, 1)",
    )
    .execute(&pool)
    .await
    .unwrap();

    let model_plan = serde_json::json!({
        "primary": {
            "providerId": "prov-plan",
            "modelRecordId": "model-record-plan",
            "providerType": "openai",
            "providerName": "OpenAI",
            "model": "gpt-4.1-mini",
            "modelId": "gpt-4.1-mini",
            "modelDisplayName": "GPT-4.1 Mini",
            "baseUrl": "https://api.openai.com/v1"
        }
    });

    let spec = build_session_spec(
        &pool,
        "run-plan",
        "t-plan",
        "/tmp/plan",
        "plan",
        &model_plan,
    )
    .await
    .unwrap();

    assert_eq!(spec.tool_profile_name, "plan_read_only");
    assert!(spec.system_prompt.contains("Plan mode is active."));
}

#[tokio::test]
async fn test_run_helpers_table_persists_collapsed_helper_summary() {
    let pool = test_helpers::setup_test_pool().await;
    test_helpers::seed_workspace(&pool, "ws-helper", "/tmp/helper").await;
    test_helpers::seed_thread(&pool, "t-helper", "ws-helper").await;
    test_helpers::seed_run(&pool, "r-helper", "t-helper", "running", "default").await;

    sqlx::query(
        "INSERT INTO run_helpers (
            id, run_id, thread_id, helper_kind, status, model_role, provider_id, model_id,
            input_summary, output_summary
         ) VALUES (
            'helper-1', 'r-helper', 't-helper', 'agent_research', 'completed', 'assistant',
            'prov-helper', 'gpt-4.1-mini', 'Inspect the repository layout', 'Repository layout summarized'
         )",
    )
    .execute(&pool)
    .await
    .unwrap();

    let row = sqlx::query(
        "SELECT helper_kind, status, output_summary FROM run_helpers WHERE id = 'helper-1'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();

    assert_eq!(row.get::<String, _>("helper_kind"), "agent_research");
    assert_eq!(row.get::<String, _>("status"), "completed");
    assert_eq!(
        row.get::<Option<String>, _>("output_summary").unwrap(),
        "Repository layout summarized"
    );
}
