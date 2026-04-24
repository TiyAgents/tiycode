//! End-to-end full chain integration tests
//!
//! Verifies the complete data flow:
//! Workspace → Thread → Message → Run → ToolCall → Audit
//!
//! These tests simulate the full lifecycle without Tauri or Sidecar,
//! operating directly on the database layer.

mod test_helpers;

use sqlx::Row;

// =========================================================================
// E2E.1 — Complete workspace-to-thread-to-message chain
// =========================================================================

#[tokio::test]
async fn test_full_workspace_thread_message_chain() {
    let pool = test_helpers::setup_test_pool().await;

    // 1. Create workspace
    test_helpers::seed_workspace(&pool, "ws-e2e", "/tmp/e2e-project").await;

    // 2. Create thread under workspace
    test_helpers::seed_thread(&pool, "t-e2e", "ws-e2e", None).await;

    // 3. Add user message
    test_helpers::seed_message(&pool, "m-user", "t-e2e", "user", "Explain this codebase").await;

    // 4. Create a run
    test_helpers::seed_run(&pool, "r-e2e", "t-e2e", "running", "default").await;

    // 5. Add assistant response (linked to run)
    sqlx::query(
        "INSERT INTO messages (id, thread_id, run_id, role, content_markdown, message_type, status)
         VALUES ('m-asst', 't-e2e', 'r-e2e', 'assistant', 'This is a Rust project...', 'plain_message', 'completed')",
    )
    .execute(&pool)
    .await
    .unwrap();

    // 6. Record a tool call
    test_helpers::seed_tool_call(&pool, "tc-e2e", "r-e2e", "t-e2e", "read", "completed").await;
    sqlx::query(
        "UPDATE tool_calls SET tool_input_json = ?, tool_output_json = ? WHERE id = 'tc-e2e'",
    )
    .bind(r#"{"path":"src/main.rs"}"#)
    .bind(r#"{"content":"fn main() {}"}"#)
    .execute(&pool)
    .await
    .unwrap();

    // 7. Complete the run
    sqlx::query("UPDATE thread_runs SET status = 'completed', finished_at = strftime('%Y-%m-%dT%H:%M:%fZ','now') WHERE id = 'r-e2e'")
        .execute(&pool)
        .await
        .unwrap();

    // 8. Record audit event
    sqlx::query(
        "INSERT INTO audit_events (id, actor_type, actor_id, source, workspace_id, thread_id, run_id, tool_call_id, action, target_type, target_id, result_json)
         VALUES ('audit-e2e', 'agent', 'sidecar', 'tool_gateway', 'ws-e2e', 't-e2e', 'r-e2e', 'tc-e2e', 'tool_execute', 'tool_call', 'tc-e2e',
                 '{\"tool\":\"read\",\"verdict\":\"auto_allow\"}')",
    )
    .execute(&pool)
    .await
    .unwrap();

    // ---- Verification ----

    // Verify workspace exists
    let ws = sqlx::query("SELECT name FROM workspaces WHERE id = 'ws-e2e'")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(ws.get::<String, _>("name"), "Test Workspace");

    // Verify thread belongs to workspace
    let thread = sqlx::query("SELECT workspace_id FROM threads WHERE id = 't-e2e'")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(thread.get::<String, _>("workspace_id"), "ws-e2e");

    // Verify messages in thread
    let messages =
        sqlx::query("SELECT id, role FROM messages WHERE thread_id = 't-e2e' ORDER BY created_at")
            .fetch_all(&pool)
            .await
            .unwrap();
    assert_eq!(messages.len(), 2);
    assert_eq!(messages[0].get::<String, _>("role"), "user");
    assert_eq!(messages[1].get::<String, _>("role"), "assistant");

    // Verify run completed
    let run = sqlx::query("SELECT status FROM thread_runs WHERE id = 'r-e2e'")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(run.get::<String, _>("status"), "completed");

    // Verify tool call completed with output
    let tc = sqlx::query("SELECT status, tool_output_json FROM tool_calls WHERE id = 'tc-e2e'")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(tc.get::<String, _>("status"), "completed");
    assert!(tc.get::<Option<String>, _>("tool_output_json").is_some());

    // Verify audit trail
    let audit = sqlx::query("SELECT action FROM audit_events WHERE id = 'audit-e2e'")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(audit.get::<String, _>("action"), "tool_execute");
}

// =========================================================================
// E2E.2 — Tool approval flow simulation
// =========================================================================

#[tokio::test]
async fn test_full_approval_flow() {
    let pool = test_helpers::setup_test_pool().await;
    test_helpers::seed_workspace(&pool, "ws-appr", "/tmp/approval").await;
    test_helpers::seed_thread(&pool, "t-appr", "ws-appr", None).await;
    test_helpers::seed_run(&pool, "r-appr", "t-appr", "running", "default").await;

    // 1. Tool requested
    test_helpers::seed_tool_call(&pool, "tc-appr", "r-appr", "t-appr", "write", "requested").await;

    // 2. Policy evaluates → require approval
    sqlx::query(
        "UPDATE tool_calls SET status = 'waiting_approval',
         policy_verdict_json = '{\"verdict\":\"require_approval\",\"reason\":\"Mutating tool\"}'
         WHERE id = 'tc-appr'",
    )
    .execute(&pool)
    .await
    .unwrap();

    // 3. Run moves to waiting_approval
    sqlx::query("UPDATE thread_runs SET status = 'waiting_approval' WHERE id = 'r-appr'")
        .execute(&pool)
        .await
        .unwrap();

    // 4. User approves
    sqlx::query(
        "UPDATE tool_calls SET status = 'running', approval_status = 'approved' WHERE id = 'tc-appr'",
    )
    .execute(&pool)
    .await
    .unwrap();

    // 5. Run resumes
    sqlx::query("UPDATE thread_runs SET status = 'running' WHERE id = 'r-appr'")
        .execute(&pool)
        .await
        .unwrap();

    // 6. Tool completes
    sqlx::query(
        "UPDATE tool_calls SET status = 'completed', tool_output_json = '{\"ok\":true}',
         finished_at = strftime('%Y-%m-%dT%H:%M:%fZ','now') WHERE id = 'tc-appr'",
    )
    .execute(&pool)
    .await
    .unwrap();

    // 7. Run completes
    sqlx::query(
        "UPDATE thread_runs SET status = 'completed',
         finished_at = strftime('%Y-%m-%dT%H:%M:%fZ','now') WHERE id = 'r-appr'",
    )
    .execute(&pool)
    .await
    .unwrap();

    // Verify final states
    let tc = sqlx::query("SELECT status, approval_status FROM tool_calls WHERE id = 'tc-appr'")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(tc.get::<String, _>("status"), "completed");
    assert_eq!(
        tc.get::<Option<String>, _>("approval_status").unwrap(),
        "approved"
    );

    let run = sqlx::query("SELECT status FROM thread_runs WHERE id = 'r-appr'")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(run.get::<String, _>("status"), "completed");
}

// =========================================================================
// E2E.3 — Multiple runs in a thread (only latest matters)
// =========================================================================

#[tokio::test]
async fn test_multiple_runs_in_thread() {
    let pool = test_helpers::setup_test_pool().await;
    test_helpers::seed_workspace(&pool, "ws-multi", "/tmp/multi").await;
    test_helpers::seed_thread(&pool, "t-multi", "ws-multi", None).await;

    // Run 1: completed
    test_helpers::seed_run(&pool, "r-1", "t-multi", "completed", "default").await;
    test_helpers::seed_message(&pool, "m-1", "t-multi", "user", "First question").await;
    test_helpers::seed_message(&pool, "m-2", "t-multi", "assistant", "First answer").await;

    // Run 2: completed
    test_helpers::seed_run(&pool, "r-2", "t-multi", "completed", "default").await;
    test_helpers::seed_message(&pool, "m-3", "t-multi", "user", "Second question").await;
    test_helpers::seed_message(&pool, "m-4", "t-multi", "assistant", "Second answer").await;

    // Verify all messages in thread
    let messages = sqlx::query("SELECT id FROM messages WHERE thread_id = 't-multi'")
        .fetch_all(&pool)
        .await
        .unwrap();
    assert_eq!(messages.len(), 4);

    // Verify latest run query
    let latest_run = sqlx::query(
        "SELECT id, status FROM thread_runs WHERE thread_id = 't-multi' ORDER BY started_at DESC LIMIT 1",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(latest_run.get::<String, _>("id"), "r-2");
}

// =========================================================================
// E2E.4 — Settings + Provider + Profile chain
// =========================================================================

#[tokio::test]
async fn test_settings_provider_profile_chain() {
    let pool = test_helpers::setup_test_pool().await;

    // 1. Create provider
    sqlx::query(
        "INSERT INTO providers (id, name, protocol_type, base_url, api_key_encrypted, enabled)
         VALUES ('prov-e2e', 'OpenAI', 'openai', 'https://api.openai.com/v1', 'enc_key', 1)",
    )
    .execute(&pool)
    .await
    .unwrap();

    // 2. Add models to provider
    sqlx::query(
        "INSERT INTO provider_models (id, provider_id, model_name, display_name, enabled)
         VALUES ('pm-1', 'prov-e2e', 'gpt-4', 'GPT-4', 1)",
    )
    .execute(&pool)
    .await
    .unwrap();

    sqlx::query(
        "INSERT INTO provider_models (id, provider_id, model_name, display_name, enabled)
         VALUES ('pm-2', 'prov-e2e', 'gpt-3.5-turbo', 'GPT-3.5', 1)",
    )
    .execute(&pool)
    .await
    .unwrap();

    // 3. Create profile referencing provider/model
    sqlx::query(
        "INSERT INTO agent_profiles (id, name, primary_provider_id, primary_model_id, lightweight_provider_id, lightweight_model_id, is_default)
         VALUES ('prof-e2e', 'Default Profile', 'prov-e2e', 'pm-1', 'prov-e2e', 'pm-2', 1)",
    )
    .execute(&pool)
    .await
    .unwrap();

    // 4. Store settings
    test_helpers::seed_setting(&pool, "active_profile_id", r#""prof-e2e""#).await;

    // Verify the chain
    let setting = sqlx::query("SELECT value_json FROM settings WHERE key = 'active_profile_id'")
        .fetch_one(&pool)
        .await
        .unwrap();
    let profile_id: String = serde_json::from_str(&setting.get::<String, _>("value_json")).unwrap();
    assert_eq!(profile_id, "prof-e2e");

    let profile = sqlx::query(
        "SELECT primary_provider_id, primary_model_id FROM agent_profiles WHERE id = ?",
    )
    .bind(&profile_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    let provider_id = profile
        .get::<Option<String>, _>("primary_provider_id")
        .unwrap();
    let model_id = profile
        .get::<Option<String>, _>("primary_model_id")
        .unwrap();

    let model = sqlx::query("SELECT model_name FROM provider_models WHERE id = ?")
        .bind(&model_id)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(model.get::<String, _>("model_name"), "gpt-4");

    let provider = sqlx::query("SELECT name, base_url FROM providers WHERE id = ?")
        .bind(&provider_id)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(provider.get::<String, _>("name"), "OpenAI");
    assert_eq!(
        provider.get::<String, _>("base_url"),
        "https://api.openai.com/v1"
    );
}

// =========================================================================
// E2E.5 — Workspace deletion cascading behavior
// =========================================================================

#[tokio::test]
async fn test_workspace_deletion_blocks_with_threads() {
    let pool = test_helpers::setup_test_pool().await;
    test_helpers::seed_workspace(&pool, "ws-cas", "/tmp/cascade").await;
    test_helpers::seed_thread(&pool, "t-cas", "ws-cas", None).await;

    // Deleting workspace should fail because thread references it (FK constraint)
    let result = sqlx::query("DELETE FROM workspaces WHERE id = 'ws-cas'")
        .execute(&pool)
        .await;

    assert!(
        result.is_err(),
        "Should not be able to delete workspace with active threads (FK constraint)"
    );
}

// =========================================================================
// E2E.6 — Snapshot recovery after simulated crash
// =========================================================================

#[tokio::test]
async fn test_snapshot_recovery_after_crash() {
    let pool = test_helpers::setup_test_pool().await;
    test_helpers::seed_workspace(&pool, "ws-crash", "/tmp/crash").await;
    test_helpers::seed_thread(&pool, "t-crash", "ws-crash", None).await;
    test_helpers::seed_message(&pool, "m-c1", "t-crash", "user", "Help me").await;
    test_helpers::seed_run(&pool, "r-crash", "t-crash", "running", "default").await;
    test_helpers::seed_tool_call(&pool, "tc-crash", "r-crash", "t-crash", "read", "running").await;

    // Simulate crash recovery: mark dangling runs as interrupted
    sqlx::query(
        "UPDATE thread_runs SET status = 'interrupted', finished_at = strftime('%Y-%m-%dT%H:%M:%fZ','now')
         WHERE status NOT IN ('completed', 'failed', 'denied', 'interrupted', 'cancelled')",
    )
    .execute(&pool)
    .await
    .unwrap();

    // Simulate crash recovery: mark dangling tool calls as failed
    sqlx::query(
        "UPDATE tool_calls SET status = 'failed', finished_at = strftime('%Y-%m-%dT%H:%M:%fZ','now')
         WHERE status NOT IN ('completed', 'failed', 'denied')",
    )
    .execute(&pool)
    .await
    .unwrap();

    // Now verify we can rebuild a snapshot
    let thread = sqlx::query("SELECT id, status FROM threads WHERE id = 't-crash'")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(thread.get::<String, _>("id"), "t-crash");

    let messages = sqlx::query("SELECT id FROM messages WHERE thread_id = 't-crash'")
        .fetch_all(&pool)
        .await
        .unwrap();
    assert_eq!(messages.len(), 1);

    let run = sqlx::query("SELECT status FROM thread_runs WHERE id = 'r-crash'")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(run.get::<String, _>("status"), "interrupted");

    let tc = sqlx::query("SELECT status FROM tool_calls WHERE id = 'tc-crash'")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(tc.get::<String, _>("status"), "failed");
}
