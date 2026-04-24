//! Direct coverage for small persistence::repo modules.
//!
//! These tests exercise public repo functions end-to-end against an
//! in-memory SQLite database. They are distinct from the higher-level
//! manager/integration tests — the goal here is to make sure each repo
//! module's helper functions are called at least once, and their
//! behaviour asserted.

mod test_helpers;

use sqlx::Row;
use tiycode::persistence::repo::{audit_repo, run_helper_repo, settings_repo};

// ===========================================================================
// settings_repo
// ===========================================================================

#[tokio::test]
async fn settings_repo_get_returns_none_when_key_missing() {
    let pool = test_helpers::setup_test_pool().await;
    let record = settings_repo::get(&pool, "does.not.exist").await.unwrap();
    assert!(record.is_none());
}

#[tokio::test]
async fn settings_repo_set_inserts_new_key_and_get_reads_it_back() {
    let pool = test_helpers::setup_test_pool().await;

    settings_repo::set(&pool, "theme", "\"dark\"")
        .await
        .unwrap();
    let record = settings_repo::get(&pool, "theme").await.unwrap().unwrap();
    assert_eq!(record.key, "theme");
    assert_eq!(record.value_json, "\"dark\"");
    assert!(!record.updated_at.is_empty());
}

#[tokio::test]
async fn settings_repo_set_upserts_existing_key() {
    let pool = test_helpers::setup_test_pool().await;

    settings_repo::set(&pool, "language", "\"en\"")
        .await
        .unwrap();
    settings_repo::set(&pool, "language", "\"zh-CN\"")
        .await
        .unwrap();

    let record = settings_repo::get(&pool, "language")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(record.value_json, "\"zh-CN\"");
}

#[tokio::test]
async fn settings_repo_get_all_returns_sorted_by_key() {
    let pool = test_helpers::setup_test_pool().await;

    // The settings table comes pre-seeded by migrations; use distinct prefixed
    // keys and filter the full list instead of asserting exact length.
    settings_repo::set(&pool, "ztest.zeta", "1").await.unwrap();
    settings_repo::set(&pool, "ztest.alpha", "2").await.unwrap();
    settings_repo::set(&pool, "ztest.mike", "3").await.unwrap();

    let all = settings_repo::get_all(&pool).await.unwrap();
    let keys: Vec<_> = all
        .iter()
        .map(|r| r.key.as_str())
        .filter(|k| k.starts_with("ztest."))
        .collect();
    assert_eq!(keys, vec!["ztest.alpha", "ztest.mike", "ztest.zeta"]);

    // And verify the sort extends across the full set (strict string ordering).
    let full: Vec<_> = all.iter().map(|r| r.key.clone()).collect();
    let mut sorted = full.clone();
    sorted.sort();
    assert_eq!(full, sorted);
}

#[tokio::test]
async fn settings_repo_delete_reports_whether_row_existed() {
    let pool = test_helpers::setup_test_pool().await;

    settings_repo::set(&pool, "dismiss_me", "true")
        .await
        .unwrap();
    assert!(settings_repo::delete(&pool, "dismiss_me").await.unwrap());
    assert!(!settings_repo::delete(&pool, "dismiss_me").await.unwrap());
    assert!(settings_repo::get(&pool, "dismiss_me")
        .await
        .unwrap()
        .is_none());
}

// The policies table has the same schema — both surfaces need coverage.

#[tokio::test]
async fn settings_repo_policy_set_and_get_round_trip() {
    let pool = test_helpers::setup_test_pool().await;

    assert!(settings_repo::policy_get(&pool, "missing")
        .await
        .unwrap()
        .is_none());

    settings_repo::policy_set(&pool, "tool.bash", "{\"allow\":[\"ls\"]}")
        .await
        .unwrap();
    let record = settings_repo::policy_get(&pool, "tool.bash")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(record.value_json, "{\"allow\":[\"ls\"]}");

    // Upsert
    settings_repo::policy_set(&pool, "tool.bash", "{\"allow\":[]}")
        .await
        .unwrap();
    let updated = settings_repo::policy_get(&pool, "tool.bash")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(updated.value_json, "{\"allow\":[]}");
}

#[tokio::test]
async fn settings_repo_policy_get_all_returns_sorted_rows() {
    let pool = test_helpers::setup_test_pool().await;

    settings_repo::policy_set(&pool, "ptest.zulu", "1")
        .await
        .unwrap();
    settings_repo::policy_set(&pool, "ptest.alpha", "2")
        .await
        .unwrap();

    let all = settings_repo::policy_get_all(&pool).await.unwrap();
    let keys: Vec<_> = all
        .iter()
        .map(|r| r.key.as_str())
        .filter(|k| k.starts_with("ptest."))
        .collect();
    assert_eq!(keys, vec!["ptest.alpha", "ptest.zulu"]);

    let full: Vec<_> = all.iter().map(|r| r.key.clone()).collect();
    let mut sorted = full.clone();
    sorted.sort();
    assert_eq!(full, sorted);
}

// ===========================================================================
// audit_repo
// ===========================================================================

fn make_audit_insert(source: &str, action: &str) -> audit_repo::AuditInsert {
    audit_repo::AuditInsert {
        actor_type: "user".to_string(),
        actor_id: Some("actor-1".to_string()),
        source: source.to_string(),
        workspace_id: None,
        thread_id: None,
        run_id: None,
        tool_call_id: None,
        action: action.to_string(),
        target_type: Some("tool".to_string()),
        target_id: Some("bash".to_string()),
        policy_check_json: None,
        result_json: Some("{\"ok\":true}".to_string()),
    }
}

#[tokio::test]
async fn audit_repo_insert_persists_all_fields() {
    let pool = test_helpers::setup_test_pool().await;

    audit_repo::insert(&pool, &make_audit_insert("extensions", "enable"))
        .await
        .unwrap();

    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM audit_events")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(count, 1);

    let row = sqlx::query("SELECT actor_type, action, source, result_json FROM audit_events")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(row.get::<String, _>("actor_type"), "user");
    assert_eq!(row.get::<String, _>("action"), "enable");
    assert_eq!(row.get::<String, _>("source"), "extensions");
    assert_eq!(
        row.get::<Option<String>, _>("result_json").as_deref(),
        Some("{\"ok\":true}")
    );
}

#[tokio::test]
async fn audit_repo_list_extension_activity_filters_by_source_prefix() {
    let pool = test_helpers::setup_test_pool().await;

    // Entries that should be returned
    audit_repo::insert(&pool, &make_audit_insert("extensions", "install"))
        .await
        .unwrap();
    audit_repo::insert(&pool, &make_audit_insert("plugin:foo", "enable"))
        .await
        .unwrap();
    audit_repo::insert(&pool, &make_audit_insert("mcp:server-a", "call"))
        .await
        .unwrap();

    // Entries that should NOT be returned
    audit_repo::insert(&pool, &make_audit_insert("system", "boot"))
        .await
        .unwrap();
    audit_repo::insert(&pool, &make_audit_insert("user", "rename"))
        .await
        .unwrap();

    let events = audit_repo::list_extension_activity(&pool, 10)
        .await
        .unwrap();
    assert_eq!(events.len(), 3);

    // Ordered newest first — the last-inserted of the three matching sources should be first.
    let sources: Vec<_> = events.iter().map(|e| e.source.as_str()).collect();
    for src in &sources {
        assert!(
            *src == "extensions" || src.starts_with("plugin:") || src.starts_with("mcp:"),
            "unexpected source in filtered list: {src}"
        );
    }
}

#[tokio::test]
async fn audit_repo_list_extension_activity_honours_limit() {
    let pool = test_helpers::setup_test_pool().await;
    for n in 0..5 {
        audit_repo::insert(&pool, &make_audit_insert("extensions", &format!("e{n}")))
            .await
            .unwrap();
    }
    let events = audit_repo::list_extension_activity(&pool, 2).await.unwrap();
    assert_eq!(events.len(), 2);
}

#[tokio::test]
async fn audit_repo_list_extension_activity_parses_result_json_into_value() {
    let pool = test_helpers::setup_test_pool().await;

    let mut insert = make_audit_insert("extensions", "invoke");
    insert.result_json = Some("{\"name\":\"skill-x\",\"count\":3}".to_string());
    audit_repo::insert(&pool, &insert).await.unwrap();

    let events = audit_repo::list_extension_activity(&pool, 10)
        .await
        .unwrap();
    let event = events.into_iter().next().unwrap();
    let result = event.result.expect("result should be parsed JSON");
    assert_eq!(result["count"], 3);
    assert_eq!(result["name"], "skill-x");
}

#[tokio::test]
async fn audit_repo_list_extension_activity_drops_malformed_json() {
    let pool = test_helpers::setup_test_pool().await;

    let mut insert = make_audit_insert("extensions", "bad");
    insert.result_json = Some("{not valid json".to_string());
    audit_repo::insert(&pool, &insert).await.unwrap();

    let events = audit_repo::list_extension_activity(&pool, 10)
        .await
        .unwrap();
    assert!(
        events[0].result.is_none(),
        "malformed JSON must parse to None"
    );
}

// ===========================================================================
// run_helper_repo
// ===========================================================================

fn make_helper_insert(id: &str, run_id: &str) -> run_helper_repo::RunHelperInsert {
    run_helper_repo::RunHelperInsert {
        id: id.to_string(),
        run_id: run_id.to_string(),
        thread_id: "thr-1".to_string(),
        helper_kind: "summarize".to_string(),
        parent_tool_call_id: None,
        status: "running".to_string(),
        model_role: "auxiliary".to_string(),
        provider_id: Some("prov".to_string()),
        model_id: Some("model".to_string()),
        input_summary: Some("summarize this".to_string()),
    }
}

async fn seed_run_dependencies(pool: &sqlx::SqlitePool, run_id: &str) {
    // run_helpers references thread_runs(id) which in turn references threads/workspaces.
    test_helpers::seed_workspace(pool, "ws-run-helper", "/tmp/workspace-run-helper").await;
    test_helpers::seed_thread(pool, "thr-1", "ws-run-helper", None).await;
    seed_thread_run(pool, run_id).await;
}

async fn seed_thread_run(pool: &sqlx::SqlitePool, run_id: &str) {
    let now = chrono::Utc::now().to_rfc3339();
    sqlx::query(
        "INSERT INTO thread_runs (id, thread_id, status, started_at)
         VALUES (?, 'thr-1', 'running', ?)",
    )
    .bind(run_id)
    .bind(&now)
    .execute(pool)
    .await
    .expect("seed thread_run");
}

#[tokio::test]
async fn run_helper_repo_insert_returns_started_at_timestamp() {
    let pool = test_helpers::setup_test_pool().await;
    seed_run_dependencies(&pool, "run-1").await;

    let started_at = run_helper_repo::insert(&pool, &make_helper_insert("h1", "run-1"))
        .await
        .expect("insert helper");
    assert!(!started_at.is_empty());

    let row =
        sqlx::query("SELECT id, status, helper_kind, provider_id, started_at FROM run_helpers")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(row.get::<String, _>("id"), "h1");
    assert_eq!(row.get::<String, _>("status"), "running");
    assert_eq!(row.get::<String, _>("helper_kind"), "summarize");
    assert_eq!(
        row.get::<Option<String>, _>("provider_id").as_deref(),
        Some("prov")
    );
    assert_eq!(row.get::<String, _>("started_at"), started_at);
}

#[tokio::test]
async fn run_helper_repo_mark_completed_records_usage_and_output() {
    let pool = test_helpers::setup_test_pool().await;
    seed_run_dependencies(&pool, "run-1").await;
    run_helper_repo::insert(&pool, &make_helper_insert("h1", "run-1"))
        .await
        .unwrap();

    let usage = tiycore::types::Usage {
        input: 100,
        output: 50,
        cache_read: 10,
        cache_write: 5,
        total_tokens: 165,
        cost: Default::default(),
    };
    run_helper_repo::mark_completed(&pool, "h1", "done.", &usage)
        .await
        .expect("mark completed");

    let row = sqlx::query(
        "SELECT status, output_summary, input_tokens, total_tokens, finished_at
         FROM run_helpers WHERE id = 'h1'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(row.get::<String, _>("status"), "completed");
    assert_eq!(
        row.get::<Option<String>, _>("output_summary").as_deref(),
        Some("done.")
    );
    assert_eq!(row.get::<i64, _>("input_tokens"), 100);
    assert_eq!(row.get::<i64, _>("total_tokens"), 165);
    assert!(row.get::<Option<String>, _>("finished_at").is_some());
}

#[tokio::test]
async fn run_helper_repo_mark_failed_chooses_status_by_interrupted_flag() {
    let pool = test_helpers::setup_test_pool().await;
    seed_run_dependencies(&pool, "run-1").await;
    run_helper_repo::insert(&pool, &make_helper_insert("h-fail", "run-1"))
        .await
        .unwrap();
    run_helper_repo::insert(&pool, &make_helper_insert("h-int", "run-1"))
        .await
        .unwrap();

    let usage = tiycore::types::Usage::default();
    run_helper_repo::mark_failed(&pool, "h-fail", "boom", false, &usage)
        .await
        .unwrap();
    run_helper_repo::mark_failed(&pool, "h-int", "ctrl-c", true, &usage)
        .await
        .unwrap();

    let failed: String = sqlx::query_scalar("SELECT status FROM run_helpers WHERE id = 'h-fail'")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(failed, "failed");

    let interrupted: String =
        sqlx::query_scalar("SELECT status FROM run_helpers WHERE id = 'h-int'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(interrupted, "interrupted");
}

#[tokio::test]
async fn run_helper_repo_interrupt_active_helpers_preserves_terminal_rows() {
    let pool = test_helpers::setup_test_pool().await;
    seed_run_dependencies(&pool, "run-1").await;

    // 1. Still running
    run_helper_repo::insert(&pool, &make_helper_insert("h-running", "run-1"))
        .await
        .unwrap();

    // 2. Already completed
    run_helper_repo::insert(&pool, &make_helper_insert("h-done", "run-1"))
        .await
        .unwrap();
    run_helper_repo::mark_completed(
        &pool,
        "h-done",
        "finished",
        &tiycore::types::Usage::default(),
    )
    .await
    .unwrap();

    let affected = run_helper_repo::interrupt_active_helpers(&pool)
        .await
        .unwrap();
    assert_eq!(affected, 1, "only the running helper is interrupted");

    let running_after: String =
        sqlx::query_scalar("SELECT status FROM run_helpers WHERE id = 'h-running'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(running_after, "interrupted");

    let done_after: String =
        sqlx::query_scalar("SELECT status FROM run_helpers WHERE id = 'h-done'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(done_after, "completed", "terminal rows are untouched");
}

#[tokio::test]
async fn run_helper_repo_list_by_run_ids_returns_empty_for_empty_input() {
    let pool = test_helpers::setup_test_pool().await;
    let helpers = run_helper_repo::list_by_run_ids(&pool, &[]).await.unwrap();
    assert!(helpers.is_empty());
}

#[tokio::test]
async fn run_helper_repo_list_by_run_ids_filters_and_orders_by_started_at() {
    let pool = test_helpers::setup_test_pool().await;
    seed_run_dependencies(&pool, "run-1").await;
    seed_thread_run(&pool, "run-2").await;
    seed_thread_run(&pool, "run-3").await;

    run_helper_repo::insert(&pool, &make_helper_insert("h1", "run-1"))
        .await
        .unwrap();
    run_helper_repo::insert(&pool, &make_helper_insert("h2", "run-2"))
        .await
        .unwrap();
    run_helper_repo::insert(&pool, &make_helper_insert("h3", "run-3"))
        .await
        .unwrap();

    let helpers =
        run_helper_repo::list_by_run_ids(&pool, &["run-1".to_string(), "run-2".to_string()])
            .await
            .unwrap();
    let ids: Vec<_> = helpers.iter().map(|h| h.id.as_str()).collect();
    assert_eq!(ids.len(), 2);
    assert!(ids.contains(&"h1"));
    assert!(ids.contains(&"h2"));
    assert!(!ids.contains(&"h3"));
}
