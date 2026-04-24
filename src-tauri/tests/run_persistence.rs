//! Run and run_helper persistence tests (SQL-based)
//!
//! Tests thread_runs / run_helpers table CRUD operations using raw SQL,
//! matching the integration-test pattern used by profile.rs.
//!
//! Coverage:
//! - run insert with full/minimal fields
//! - update_status: terminal sets finished_at, non-terminal doesn't
//! - update_usage (token counts)
//! - set_error_message
//! - find_active_by_thread (filters by non-terminal statuses)
//! - find_latest_by_thread (orders by started_at DESC)
//! - list_thread_ids_with_active_runs
//! - interrupt_active_runs (batch status change)
//! - run_helper insert, mark_completed, mark_failed (incl. interrupted)
//! - interrupt_active_helpers
//! - list_by_run_ids filtering
//! - model plan JSON extraction (display_name fallback chain)

mod test_helpers;

use sqlx::Row;

// =========================================================================
// Helper: insert run via SQL (replaces run_repo::RunInsert)
// =========================================================================

async fn sql_insert_run(
    pool: &sqlx::SqlitePool,
    id: &str,
    thread_id: &str,
    profile_id: Option<&str>,
    run_mode: &str,
    provider_id: Option<&str>,
    model_id: Option<&str>,
    plan_json: Option<&str>,
    status: &str,
) {
    sqlx::query(
        "INSERT INTO thread_runs (id, thread_id, profile_id, run_mode, provider_id, model_id,
                effective_model_plan_json, status, started_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))",
    )
    .bind(id)
    .bind(thread_id)
    .bind(profile_id)
    .bind(run_mode)
    .bind(provider_id)
    .bind(model_id)
    .bind(plan_json)
    .bind(status)
    .execute(pool)
    .await
    .expect("failed to insert run");
}

async fn sql_update_run_status(pool: &sqlx::SqlitePool, run_id: &str, status: &str) {
    // Terminal statuses also set finished_at
    let terminal = matches!(
        status,
        "completed" | "failed" | "denied" | "interrupted" | "cancelled" | "limit_reached"
    );
    if terminal {
        sqlx::query(
            "UPDATE thread_runs SET status = ?, finished_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
             WHERE id = ?",
        )
        .bind(status)
        .bind(run_id)
        .execute(pool)
        .await
        .unwrap();
    } else {
        sqlx::query("UPDATE thread_runs SET status = ? WHERE id = ?")
            .bind(status)
            .bind(run_id)
            .execute(pool)
            .await
            .unwrap();
    }
}

async fn sql_update_run_usage(
    pool: &sqlx::SqlitePool,
    run_id: &str,
    input: i64,
    output: i64,
    cache_read: i64,
    cache_write: i64,
    total: i64,
) {
    sqlx::query(
        "UPDATE thread_runs SET input_tokens = ?, output_tokens = ?,
                cache_read_tokens = ?, cache_write_tokens = ?, total_tokens = ?
         WHERE id = ?",
    )
    .bind(input)
    .bind(output)
    .bind(cache_read)
    .bind(cache_write)
    .bind(total)
    .bind(run_id)
    .execute(pool)
    .await
    .unwrap();
}

// =========================================================================
// run insert — full fields
// =========================================================================

#[tokio::test]
async fn test_run_insert_with_all_fields() {
    let pool = test_helpers::setup_test_pool().await;
    test_helpers::seed_workspace(&pool, "ws-ri", "/tmp/ri").await;
    test_helpers::seed_thread(&pool, "t-ri", "ws-ri", None).await;

    sql_insert_run(
        &pool,
        "r-full",
        "t-ri",
        Some("prof-1"),
        "plan",
        Some("prov-1"),
        Some("gpt-4"),
        Some(
            r#"{"primary":{"model":"gpt-4","modelDisplayName":"GPT-4","contextWindow":"128000"}}"#,
        ),
        "created",
    )
    .await;

    let row = sqlx::query(
        "SELECT id, thread_id, profile_id, run_mode, provider_id, model_id,
                effective_model_plan_json, status, started_at
         FROM thread_runs WHERE id = 'r-full'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();

    assert_eq!(row.get::<String, _>("thread_id"), "t-ri");
    assert_eq!(
        row.get::<Option<String>, _>("profile_id").as_deref(),
        Some("prof-1")
    );
    assert_eq!(row.get::<String, _>("run_mode"), "plan");
    assert_eq!(
        row.get::<Option<String>, _>("provider_id").as_deref(),
        Some("prov-1")
    );
    assert_eq!(
        row.get::<Option<String>, _>("model_id").as_deref(),
        Some("gpt-4")
    );
    assert!(row
        .get::<Option<String>, _>("effective_model_plan_json")
        .is_some());
    assert_eq!(row.get::<String, _>("status"), "created");
    assert!(!row.get::<String, _>("started_at").is_empty());
}

#[tokio::test]
async fn test_run_insert_with_minimal_fields() {
    let pool = test_helpers::setup_test_pool().await;
    test_helpers::seed_workspace(&pool, "ws-rmin", "/tmp/rmin").await;
    test_helpers::seed_thread(&pool, "t-rmin", "ws-rmin", None).await;

    sql_insert_run(
        &pool, "r-min", "t-rmin", None, "default", None, None, None, "created",
    )
    .await;

    let row =
        sqlx::query("SELECT profile_id, provider_id, model_id FROM thread_runs WHERE id = 'r-min'")
            .fetch_one(&pool)
            .await
            .unwrap();

    assert!(row.get::<Option<String>, _>("profile_id").is_none());
    assert!(row.get::<Option<String>, _>("provider_id").is_none());
    assert!(row.get::<Option<String>, _>("model_id").is_none());
}

// =========================================================================
// update_status — terminal vs non-terminal
// =========================================================================

#[tokio::test]
async fn test_run_update_status_terminal_sets_finished_at() {
    let pool = test_helpers::setup_test_pool().await;
    test_helpers::seed_workspace(&pool, "ws-st", "/tmp/st").await;
    test_helpers::seed_thread(&pool, "t-st", "ws-st", None).await;
    test_helpers::seed_run(&pool, "r-st", "t-st", "running", "default").await;

    for terminal_status in [
        "completed",
        "failed",
        "denied",
        "interrupted",
        "cancelled",
        "limit_reached",
    ] {
        // Reset to running
        sqlx::query(
            "UPDATE thread_runs SET status = 'running', finished_at = NULL WHERE id = 'r-st'",
        )
        .execute(&pool)
        .await
        .unwrap();

        sql_update_run_status(&pool, "r-st", terminal_status).await;

        let row = sqlx::query("SELECT status, finished_at FROM thread_runs WHERE id = 'r-st'")
            .fetch_one(&pool)
            .await
            .unwrap();

        assert_eq!(
            row.get::<String, _>("status"),
            terminal_status,
            "status should be {terminal_status}"
        );
        assert!(
            row.get::<Option<String>, _>("finished_at").is_some(),
            "finished_at should be set for terminal status {terminal_status}"
        );
    }
}

#[tokio::test]
async fn test_run_update_status_non_terminal_does_not_set_finished_at() {
    let pool = test_helpers::setup_test_pool().await;
    test_helpers::seed_workspace(&pool, "ws-nt", "/tmp/nt").await;
    test_helpers::seed_thread(&pool, "t-nt", "ws-nt", None).await;
    test_helpers::seed_run(&pool, "r-nt", "t-nt", "created", "default").await;

    sql_update_run_status(&pool, "r-nt", "running").await;

    let row = sqlx::query("SELECT status, finished_at FROM thread_runs WHERE id = 'r-nt'")
        .fetch_one(&pool)
        .await
        .unwrap();

    assert_eq!(row.get::<String, _>("status"), "running");
    assert!(row.get::<Option<String>, _>("finished_at").is_none());
}

// =========================================================================
// update_usage
// =========================================================================

#[tokio::test]
async fn test_run_update_usage() {
    let pool = test_helpers::setup_test_pool().await;
    test_helpers::seed_workspace(&pool, "ws-usage", "/tmp/usage").await;
    test_helpers::seed_thread(&pool, "t-usage", "ws-usage", None).await;
    test_helpers::seed_run(&pool, "r-usage", "t-usage", "running", "default").await;

    sql_update_run_usage(&pool, "r-usage", 100, 50, 200, 25, 375).await;

    let row = sqlx::query(
        "SELECT input_tokens, output_tokens, cache_read_tokens, cache_write_tokens, total_tokens
         FROM thread_runs WHERE id = 'r-usage'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();

    assert_eq!(row.get::<i64, _>("input_tokens"), 100);
    assert_eq!(row.get::<i64, _>("output_tokens"), 50);
    assert_eq!(row.get::<i64, _>("cache_read_tokens"), 200);
    assert_eq!(row.get::<i64, _>("cache_write_tokens"), 25);
    assert_eq!(row.get::<i64, _>("total_tokens"), 375);
}

// =========================================================================
// set_error_message
// =========================================================================

#[tokio::test]
async fn test_run_set_error_message() {
    let pool = test_helpers::setup_test_pool().await;
    test_helpers::seed_workspace(&pool, "ws-err", "/tmp/err").await;
    test_helpers::seed_thread(&pool, "t-err", "ws-err", None).await;
    test_helpers::seed_run(&pool, "r-err", "t-err", "failed", "default").await;

    sqlx::query("UPDATE thread_runs SET error_message = ? WHERE id = ?")
        .bind("API rate limited")
        .bind("r-err")
        .execute(&pool)
        .await
        .unwrap();

    let row = sqlx::query("SELECT error_message FROM thread_runs WHERE id = 'r-err'")
        .fetch_one(&pool)
        .await
        .unwrap();

    assert_eq!(
        row.get::<Option<String>, _>("error_message").as_deref(),
        Some("API rate limited")
    );
}

// =========================================================================
// find_active_by_thread (non-terminal runs: created/dispatching/running/waiting_approval)
// =========================================================================

#[tokio::test]
async fn test_run_find_active_by_thread() {
    let pool = test_helpers::setup_test_pool().await;
    test_helpers::seed_workspace(&pool, "ws-active", "/tmp/active").await;
    test_helpers::seed_thread(&pool, "t-active", "ws-active", None).await;

    // No active runs initially
    let rows = sqlx::query(
        "SELECT id, status FROM thread_runs
         WHERE thread_id = 't-active'
           AND status IN ('created','dispatching','running','waiting_approval')",
    )
    .fetch_all(&pool)
    .await
    .unwrap();
    assert!(rows.is_empty());

    // Completed run should NOT appear as active
    test_helpers::seed_run(&pool, "r-done", "t-active", "completed", "default").await;
    let still_none = sqlx::query(
        "SELECT id FROM thread_runs
         WHERE thread_id = 't-active'
           AND status IN ('created','dispatching','running','waiting_approval')",
    )
    .fetch_all(&pool)
    .await
    .unwrap();
    assert!(still_none.is_empty());

    // Running run SHOULD appear
    test_helpers::seed_run(&pool, "r-running", "t-active", "running", "default").await;
    let found = sqlx::query(
        "SELECT id, status FROM thread_runs
         WHERE thread_id = 't-active'
           AND status IN ('created','dispatching','running','waiting_approval')",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(found.get::<String, _>("id"), "r-running");
    assert_eq!(found.get::<String, _>("status"), "running");
}

// =========================================================================
// find_latest_by_thread
// =========================================================================

#[tokio::test]
async fn test_run_find_latest_by_thread() {
    let pool = test_helpers::setup_test_pool().await;
    test_helpers::seed_workspace(&pool, "ws-latest", "/tmp/latest").await;
    test_helpers::seed_thread(&pool, "t-latest", "ws-latest", None).await;

    // None when no runs exist
    let none = sqlx::query(
        "SELECT id FROM thread_runs WHERE thread_id = 't-latest' ORDER BY started_at DESC LIMIT 1",
    )
    .fetch_optional(&pool)
    .await
    .unwrap();
    assert!(none.is_none());

    // Insert runs with controlled ordering
    sqlx::query(
        "INSERT INTO thread_runs (id, thread_id, run_mode, status, started_at)
         VALUES ('r-old', 't-latest', 'default', 'completed', '2026-01-01T00:00:00Z')",
    )
    .execute(&pool)
    .await
    .unwrap();

    sqlx::query(
        "INSERT INTO thread_runs (id, thread_id, run_mode, status, started_at)
         VALUES ('r-new', 't-latest', 'default', 'failed', '2026-06-01T00:00:00Z')",
    )
    .execute(&pool)
    .await
    .unwrap();

    let latest = sqlx::query(
        "SELECT id, status FROM thread_runs WHERE thread_id = 't-latest' ORDER BY started_at DESC LIMIT 1",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(latest.get::<String, _>("id"), "r-new");
    assert_eq!(latest.get::<String, _>("status"), "failed");
}

// =========================================================================
// find_effective_model_plan_json
// =========================================================================

#[tokio::test]
async fn test_run_effective_model_plan_json() {
    let pool = test_helpers::setup_test_pool().await;
    test_helpers::seed_workspace(&pool, "ws-plan", "/tmp/plan").await;
    test_helpers::seed_thread(&pool, "t-plan", "ws-plan", None).await;

    let plan_json =
        r#"{"primary":{"model":"gpt-4","modelDisplayName":"GPT-4","contextWindow":"128000"}}"#;

    sql_insert_run(
        &pool,
        "r-plan",
        "t-plan",
        None,
        "default",
        None,
        None,
        Some(plan_json),
        "running",
    )
    .await;

    let found =
        sqlx::query("SELECT effective_model_plan_json FROM thread_runs WHERE id = 'r-plan'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(
        found
            .get::<Option<String>, _>("effective_model_plan_json")
            .as_deref(),
        Some(plan_json)
    );

    // Non-existent run returns NULL
    let missing =
        sqlx::query("SELECT effective_model_plan_json FROM thread_runs WHERE id = 'nonexistent'")
            .fetch_optional(&pool)
            .await
            .unwrap();
    assert!(
        missing.is_none()
            || missing
                .unwrap()
                .get::<Option<String>, _>("effective_model_plan_json")
                .is_none()
    );
}

// =========================================================================
// list_thread_ids_with_active_runs
// =========================================================================

#[tokio::test]
async fn test_run_list_thread_ids_with_active_runs() {
    let pool = test_helpers::setup_test_pool().await;
    test_helpers::seed_workspace(&pool, "ws-tids", "/tmp/tids").await;
    test_helpers::seed_thread(&pool, "t-active1", "ws-tids", None).await;
    test_helpers::seed_thread(&pool, "t-active2", "ws-tids", None).await;
    test_helpers::seed_thread(&pool, "t-done", "ws-tids", None).await;

    test_helpers::seed_run(&pool, "r-a1", "t-active1", "running", "default").await;
    test_helpers::seed_run(&pool, "r-a2", "t-active2", "dispatching", "default").await;
    test_helpers::seed_run(&pool, "r-d", "t-done", "completed", "default").await;

    let rows = sqlx::query(
        "SELECT DISTINCT thread_id FROM thread_runs
         WHERE status IN ('created','dispatching','running','waiting_approval')",
    )
    .fetch_all(&pool)
    .await
    .unwrap();

    let active_threads: Vec<String> = rows
        .iter()
        .map(|r| r.get::<String, _>("thread_id"))
        .collect();
    assert!(active_threads.iter().any(|s| s == "t-active1"));
    assert!(active_threads.iter().any(|s| s == "t-active2"));
    assert!(!active_threads.iter().any(|s| s == "t-done"));
}

// =========================================================================
// interrupt_active_runs
// =========================================================================

#[tokio::test]
async fn test_run_interrupt_active_runs() {
    let pool = test_helpers::setup_test_pool().await;
    test_helpers::seed_workspace(&pool, "ws-int", "/tmp/int").await;
    test_helpers::seed_thread(&pool, "t-int", "ws-int", None).await;

    test_helpers::seed_run(&pool, "r-run", "t-int", "running", "default").await;
    test_helpers::seed_run(&pool, "r-disp", "t-int", "dispatching", "default").await;
    test_helpers::seed_run(&pool, "r-ok", "t-int", "completed", "default").await;
    test_helpers::seed_run(&pool, "r-fail", "t-int", "failed", "default").await;

    let result = sqlx::query(
        "UPDATE thread_runs
         SET status = 'interrupted',
             error_message = 'Run interrupted on shutdown',
             finished_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
         WHERE status IN ('created','dispatching','running','waiting_approval')",
    )
    .execute(&pool)
    .await
    .unwrap();

    assert_eq!(result.rows_affected(), 2);

    let run_row = sqlx::query(
        "SELECT status, error_message, finished_at FROM thread_runs WHERE id = 'r-run'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(run_row.get::<String, _>("status"), "interrupted");
    assert!(run_row.get::<Option<String>, _>("error_message").is_some());
    assert!(run_row.get::<Option<String>, _>("finished_at").is_some());

    // Completed run unchanged
    let ok_row = sqlx::query("SELECT status FROM thread_runs WHERE id = 'r-ok'")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(ok_row.get::<String, _>("status"), "completed");
}

// =========================================================================
// run_helper insert
// =========================================================================

#[tokio::test]
async fn test_run_helper_insert() {
    let pool = test_helpers::setup_test_pool().await;
    test_helpers::seed_workspace(&pool, "ws-rh", "/tmp/rh").await;
    test_helpers::seed_thread(&pool, "t-rh", "ws-rh", None).await;
    test_helpers::seed_run(&pool, "r-rh", "t-rh", "running", "default").await;

    sqlx::query(
        "INSERT INTO run_helpers (id, run_id, thread_id, helper_kind, parent_tool_call_id,
                status, model_role, provider_id, model_id, input_summary, started_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?,
                 strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))",
    )
    .bind("h-1")
    .bind("r-rh")
    .bind("t-rh")
    .bind("helper_explore")
    .bind::<Option<String>>(Some("tc-1".to_string()))
    .bind("running")
    .bind("assistant")
    .bind::<Option<String>>(Some("prov-1".to_string()))
    .bind::<Option<String>>(Some("gpt-4".to_string()))
    .bind::<Option<String>>(Some("Explore the repo".to_string()))
    .execute(&pool)
    .await
    .unwrap();

    let row = sqlx::query(
        "SELECT id, run_id, thread_id, helper_kind, parent_tool_call_id, status,
                model_role, provider_id, model_id, input_summary
         FROM run_helpers WHERE id = 'h-1'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();

    assert_eq!(row.get::<String, _>("helper_kind"), "helper_explore");
    assert_eq!(
        row.get::<Option<String>, _>("parent_tool_call_id")
            .as_deref(),
        Some("tc-1")
    );
    assert_eq!(row.get::<String, _>("status"), "running");
    assert_eq!(row.get::<String, _>("model_role"), "assistant");
    assert_eq!(
        row.get::<Option<String>, _>("input_summary").as_deref(),
        Some("Explore the repo")
    );
}

// =========================================================================
// run_helper mark_completed
// =========================================================================

#[tokio::test]
async fn test_run_helper_mark_completed() {
    let pool = test_helpers::setup_test_pool().await;
    test_helpers::seed_workspace(&pool, "ws-hc", "/tmp/hc").await;
    test_helpers::seed_thread(&pool, "t-hc", "ws-hc", None).await;
    test_helpers::seed_run(&pool, "r-hc", "t-hc", "running", "default").await;
    test_helpers::seed_run_helper(&pool, "h-c", "r-hc", "t-hc", "helper_explore", "running").await;

    sqlx::query(
        "UPDATE run_helpers SET status = 'completed', output_summary = ?,
                finished_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now'),
                input_tokens = 500, output_tokens = 200,
                cache_read_tokens = 100, cache_write_tokens = 50, total_tokens = 850
         WHERE id = 'h-c'",
    )
    .bind("Summary of findings")
    .execute(&pool)
    .await
    .unwrap();

    let row = sqlx::query(
        "SELECT status, output_summary, finished_at,
                input_tokens, output_tokens, cache_read_tokens, cache_write_tokens, total_tokens
         FROM run_helpers WHERE id = 'h-c'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();

    assert_eq!(row.get::<String, _>("status"), "completed");
    assert_eq!(
        row.get::<Option<String>, _>("output_summary").as_deref(),
        Some("Summary of findings")
    );
    assert!(row.get::<Option<String>, _>("finished_at").is_some());
    assert_eq!(row.get::<i64, _>("input_tokens"), 500);
    assert_eq!(row.get::<i64, _>("output_tokens"), 200);
    assert_eq!(row.get::<i64, _>("cache_read_tokens"), 100);
    assert_eq!(row.get::<i64, _>("cache_write_tokens"), 50);
    assert_eq!(row.get::<i64, _>("total_tokens"), 850);
}

// =========================================================================
// run_helper mark_failed
// =========================================================================

#[tokio::test]
async fn test_run_helper_mark_failed() {
    let pool = test_helpers::setup_test_pool().await;
    test_helpers::seed_workspace(&pool, "ws-hf", "/tmp/hf").await;
    test_helpers::seed_thread(&pool, "t-hf", "ws-hf", None).await;
    test_helpers::seed_run(&pool, "r-hf", "t-hf", "running", "default").await;
    test_helpers::seed_run_helper(&pool, "h-f", "r-hf", "t-hf", "helper_explore", "running").await;

    sqlx::query(
        "UPDATE run_helpers SET status = 'failed', error_summary = ?,
                finished_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now'),
                input_tokens = 50, output_tokens = 0,
                cache_read_tokens = 0, cache_write_tokens = 0, total_tokens = 50
         WHERE id = 'h-f'",
    )
    .bind("Timeout error")
    .execute(&pool)
    .await
    .unwrap();

    let row =
        sqlx::query("SELECT status, error_summary, finished_at FROM run_helpers WHERE id = 'h-f'")
            .fetch_one(&pool)
            .await
            .unwrap();

    assert_eq!(row.get::<String, _>("status"), "failed");
    assert_eq!(
        row.get::<Option<String>, _>("error_summary").as_deref(),
        Some("Timeout error")
    );
    assert!(row.get::<Option<String>, _>("finished_at").is_some());
}

#[tokio::test]
async fn test_run_helper_mark_failed_interrupted() {
    let pool = test_helpers::setup_test_pool().await;
    test_helpers::seed_workspace(&pool, "ws-hi", "/tmp/hi").await;
    test_helpers::seed_thread(&pool, "t-hi", "ws-hi", None).await;
    test_helpers::seed_run(&pool, "r-hi", "t-hi", "running", "default").await;
    test_helpers::seed_run_helper(&pool, "h-i", "r-hi", "t-hi", "helper_explore", "running").await;

    sqlx::query(
        "UPDATE run_helpers SET status = 'interrupted', error_summary = ?,
                finished_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now'),
                input_tokens = 10, output_tokens = 5,
                cache_read_tokens = 0, cache_write_tokens = 0, total_tokens = 15
         WHERE id = 'h-i'",
    )
    .bind("User cancelled")
    .execute(&pool)
    .await
    .unwrap();

    let row = sqlx::query("SELECT status FROM run_helpers WHERE id = 'h-i'")
        .fetch_one(&pool)
        .await
        .unwrap();

    assert_eq!(row.get::<String, _>("status"), "interrupted");
}

// =========================================================================
// run_helper interrupt_active_helpers
// =========================================================================

#[tokio::test]
async fn test_run_helper_interrupt_active_helpers() {
    let pool = test_helpers::setup_test_pool().await;
    test_helpers::seed_workspace(&pool, "ws-hai", "/tmp/hai").await;
    test_helpers::seed_thread(&pool, "t-hai", "ws-hai", None).await;
    test_helpers::seed_run(&pool, "r-hai", "t-hai", "running", "default").await;

    test_helpers::seed_run_helper(
        &pool,
        "h-run",
        "r-hai",
        "t-hai",
        "helper_explore",
        "running",
    )
    .await;
    test_helpers::seed_run_helper(
        &pool,
        "h-done",
        "r-hai",
        "t-hai",
        "helper_explore",
        "completed",
    )
    .await;

    let result = sqlx::query(
        "UPDATE run_helpers SET status = 'interrupted',
                error_summary = 'Helper interrupted on shutdown',
                finished_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
         WHERE status IN ('running','dispatching','created')",
    )
    .execute(&pool)
    .await
    .unwrap();

    assert_eq!(result.rows_affected(), 1);

    let row = sqlx::query(
        "SELECT status, error_summary, finished_at FROM run_helpers WHERE id = 'h-run'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(row.get::<String, _>("status"), "interrupted");
    assert!(row.get::<Option<String>, _>("error_summary").is_some());
    assert!(row.get::<Option<String>, _>("finished_at").is_some());

    // Completed helper unchanged
    let done_row = sqlx::query("SELECT status FROM run_helpers WHERE id = 'h-done'")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(done_row.get::<String, _>("status"), "completed");
}

// =========================================================================
// run_helper list_by_run_ids
// =========================================================================

#[tokio::test]
async fn test_run_helper_list_by_run_ids_empty() {
    let pool = test_helpers::setup_test_pool().await;

    let helpers = sqlx::query("SELECT id, run_id FROM run_helpers WHERE 0=1")
        .fetch_all(&pool)
        .await
        .unwrap();
    assert!(helpers.is_empty());
}

#[tokio::test]
async fn test_run_helper_list_by_run_ids() {
    let pool = test_helpers::setup_test_pool().await;
    test_helpers::seed_workspace(&pool, "ws-lri", "/tmp/lri").await;
    test_helpers::seed_thread(&pool, "t-lri", "ws-lri", None).await;
    test_helpers::seed_run(&pool, "r-lri-1", "t-lri", "running", "default").await;
    test_helpers::seed_run(&pool, "r-lri-2", "t-lri", "running", "default").await;
    test_helpers::seed_run(&pool, "r-lri-3", "t-lri", "running", "default").await;

    test_helpers::seed_run_helper(
        &pool,
        "h-lri-1",
        "r-lri-1",
        "t-lri",
        "helper_explore",
        "completed",
    )
    .await;
    test_helpers::seed_run_helper(
        &pool,
        "h-lri-2",
        "r-lri-2",
        "t-lri",
        "helper_code",
        "running",
    )
    .await;
    test_helpers::seed_run_helper(
        &pool,
        "h-lri-3",
        "r-lri-3",
        "t-lri",
        "helper_explore",
        "failed",
    )
    .await;

    // Query helpers for runs 1 and 2 only (using dynamic SQL simulation)
    let rows = sqlx::query(
        "SELECT id, run_id FROM run_helpers
         WHERE run_id IN ('r-lri-1', 'r-lri-2')
         ORDER BY id",
    )
    .fetch_all(&pool)
    .await
    .unwrap();

    assert_eq!(rows.len(), 2);
    let ids: Vec<String> = rows.iter().map(|r| r.get::<String, _>("id")).collect();
    assert!(ids.iter().any(|s| s == "h-lri-1"));
    assert!(ids.iter().any(|s| s == "h-lri-2"));
    assert!(!ids.iter().any(|s| s == "h-lri-3"));
}

#[tokio::test]
async fn test_run_helper_list_by_run_ids_maps_usage_correctly() {
    let pool = test_helpers::setup_test_pool().await;
    test_helpers::seed_workspace(&pool, "ws-lu", "/tmp/lu").await;
    test_helpers::seed_thread(&pool, "t-lu", "ws-lu", None).await;
    test_helpers::seed_run(&pool, "r-lu", "t-lu", "running", "default").await;
    test_helpers::seed_run_helper(&pool, "h-lu", "r-lu", "t-lu", "helper_explore", "running").await;

    // Set usage tokens directly
    sqlx::query(
        "UPDATE run_helpers SET input_tokens = 100, output_tokens = 50,
                cache_read_tokens = 200, cache_write_tokens = 10, total_tokens = 360
         WHERE id = 'h-lu'",
    )
    .execute(&pool)
    .await
    .unwrap();

    let row = sqlx::query(
        "SELECT id, run_id, input_tokens, output_tokens,
                cache_read_tokens, cache_write_tokens, total_tokens
         FROM run_helpers WHERE run_id = 'r-lu'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();

    assert_eq!(row.get::<String, _>("id"), "h-lu");
    assert_eq!(row.get::<i64, _>("input_tokens"), 100);
    assert_eq!(row.get::<i64, _>("output_tokens"), 50);
    assert_eq!(row.get::<i64, _>("cache_read_tokens"), 200);
    assert_eq!(row.get::<i64, _>("cache_write_tokens"), 10);
    assert_eq!(row.get::<i64, _>("total_tokens"), 360);
}

// =========================================================================
// Model plan extraction — display_name from JSON
// =========================================================================

#[tokio::test]
async fn test_run_latest_returns_model_display_name_from_plan() {
    let pool = test_helpers::setup_test_pool().await;
    test_helpers::seed_workspace(&pool, "ws-mdl", "/tmp/mdl").await;
    test_helpers::seed_thread(&pool, "t-mdl", "ws-mdl", None).await;

    let plan = r#"{"primary":{"model":"gpt-4","modelDisplayName":"GPT-4 Turbo","contextWindow":"128000"}}"#;
    sqlx::query(
        "INSERT INTO thread_runs (id, thread_id, run_mode, status, effective_model_plan_json, started_at)
         VALUES ('r-mdl', 't-mdl', 'default', 'completed', ?, '2026-01-01T00:00:00Z')",
    )
    .bind(plan)
    .execute(&pool)
    .await
    .unwrap();

    // Verify we can read back the plan JSON
    let row = sqlx::query("SELECT effective_model_plan_json FROM thread_runs WHERE id = 'r-mdl'")
        .fetch_one(&pool)
        .await
        .unwrap();
    let stored: Option<String> = row.get("effective_model_plan_json");
    assert_eq!(stored.as_deref(), Some(plan));

    // Simulate what find_latest does: extract display_name from plan
    if let Some(json_str) = stored {
        let val: serde_json::Value = serde_json::from_str(&json_str).unwrap();
        let primary = &val["primary"];
        let display_name = primary
            .get("modelDisplayName")
            .and_then(|v| v.as_str())
            .or_else(|| primary.get("model").and_then(|v| v.as_str()));
        assert_eq!(display_name, Some("GPT-4 Turbo"));
    }
}

#[tokio::test]
async fn test_run_latest_falls_back_to_model_field_when_no_display_name() {
    let pool = test_helpers::setup_test_pool().await;
    test_helpers::seed_workspace(&pool, "ws-fb", "/tmp/fb").await;
    test_helpers::seed_thread(&pool, "t-fb", "ws-fb", None).await;

    let plan = r#"{"primary":{"model":"gpt-4.1-mini"}}"#;
    sqlx::query(
        "INSERT INTO thread_runs (id, thread_id, run_mode, status, effective_model_plan_json, started_at)
         VALUES ('r-fb', 't-fb', 'default', 'completed', ?, '2026-01-01T00:00:00Z')",
    )
    .bind(plan)
    .execute(&pool)
    .await
    .unwrap();

    let row = sqlx::query("SELECT effective_model_plan_json FROM thread_runs WHERE id = 'r-fb'")
        .fetch_one(&pool)
        .await
        .unwrap();
    let stored: Option<String> = row.get("effective_model_plan_json");

    if let Some(json_str) = stored {
        let val: serde_json::Value = serde_json::from_str(&json_str).unwrap();
        let primary = &val["primary"];
        let display_name = primary
            .get("modelDisplayName")
            .and_then(|v| v.as_str())
            .or_else(|| primary.get("model").and_then(|v| v.as_str()));
        assert_eq!(display_name, Some("gpt-4.1-mini"));
    } else {
        panic!("plan should be present");
    }
}

#[tokio::test]
async fn test_run_latest_returns_none_model_info_without_plan() {
    let pool = test_helpers::setup_test_pool().await;
    test_helpers::seed_workspace(&pool, "ws-np", "/tmp/np").await;
    test_helpers::seed_thread(&pool, "t-np", "ws-np", None).await;

    test_helpers::seed_run(&pool, "r-np", "t-np", "completed", "default").await;

    let row = sqlx::query("SELECT effective_model_plan_json FROM thread_runs WHERE id = 'r-np'")
        .fetch_one(&pool)
        .await
        .unwrap();
    let stored: Option<String> = row.get("effective_model_plan_json");
    assert!(stored.is_none());
}
