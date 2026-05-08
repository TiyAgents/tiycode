//! Thread core tests
//!
//! Acceptance criteria:
//! - Thread belongs to workspace, sidebar sorted by last_active_at
//! - Messages persisted, survive restart
//! - Long threads support pagination (UUID v7 cursor-based)
//! - ThreadStatus derived from latest run status

mod test_helpers;

use sqlx::Row;

use tiycode_lib::core::thread_manager::ThreadManager;

// =========================================================================
// T1.4.1 — Thread CRUD
// =========================================================================

#[tokio::test]
async fn test_thread_create() {
    let pool = test_helpers::setup_test_pool().await;
    test_helpers::seed_workspace(&pool, "ws-t", "/tmp/threads").await;
    test_helpers::seed_thread(&pool, "t-001", "ws-t", None).await;

    let row = sqlx::query("SELECT title, status FROM threads WHERE id = 't-001'")
        .fetch_one(&pool)
        .await
        .unwrap();

    assert_eq!(row.get::<String, _>("title"), "Test Thread");
    assert_eq!(row.get::<String, _>("status"), "idle");
}

#[tokio::test]
async fn test_thread_manager_create_persists_profile_id() {
    let pool = test_helpers::setup_test_pool().await;
    test_helpers::seed_workspace(&pool, "ws-create-profile", "/tmp/create-profile").await;
    let manager = ThreadManager::new(pool.clone());

    let thread = manager
        .create(
            "ws-create-profile",
            Some("Profile thread".to_string()),
            Some("profile-1".to_string()),
        )
        .await
        .expect("thread create should succeed");

    assert_eq!(thread.profile_id.as_deref(), Some("profile-1"));

    let row = sqlx::query("SELECT profile_id FROM threads WHERE id = ?")
        .bind(&thread.id)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(row.get::<String, _>("profile_id"), "profile-1");
}

#[tokio::test]
async fn test_thread_manager_update_profile_persists_new_value() {
    let pool = test_helpers::setup_test_pool().await;
    test_helpers::seed_workspace(&pool, "ws-update-profile", "/tmp/update-profile").await;
    test_helpers::seed_thread(
        &pool,
        "t-update-profile",
        "ws-update-profile",
        Some("profile-old"),
    )
    .await;
    let manager = ThreadManager::new(pool.clone());

    manager
        .update_profile("t-update-profile", Some("profile-new"))
        .await
        .expect("thread profile update should succeed");

    let row = sqlx::query("SELECT profile_id FROM threads WHERE id = 't-update-profile'")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(row.get::<String, _>("profile_id"), "profile-new");
}

#[tokio::test]
async fn test_thread_manager_update_profile_can_clear_to_none() {
    let pool = test_helpers::setup_test_pool().await;
    test_helpers::seed_workspace(&pool, "ws-clear-profile", "/tmp/clear-profile").await;
    test_helpers::seed_thread(
        &pool,
        "t-clear-profile",
        "ws-clear-profile",
        Some("profile-to-clear"),
    )
    .await;
    let manager = ThreadManager::new(pool.clone());

    manager
        .update_profile("t-clear-profile", None)
        .await
        .expect("thread profile clear should succeed");

    let row = sqlx::query("SELECT profile_id FROM threads WHERE id = 't-clear-profile'")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert!(
        row.get::<Option<String>, _>("profile_id").is_none(),
        "profile_id should be NULL after clearing"
    );
}

#[tokio::test]
async fn test_thread_list_returns_profile_id() {
    let pool = test_helpers::setup_test_pool().await;
    test_helpers::seed_workspace(&pool, "ws-list-profile", "/tmp/list-profile").await;
    test_helpers::seed_thread(
        &pool,
        "t-list-profile",
        "ws-list-profile",
        Some("profile-list"),
    )
    .await;
    let manager = ThreadManager::new(pool.clone());

    let threads = manager
        .list("ws-list-profile", None, None)
        .await
        .expect("thread list should succeed");

    assert_eq!(threads.len(), 1);
    assert_eq!(threads[0].profile_id.as_deref(), Some("profile-list"));
}

#[tokio::test]
async fn test_thread_belongs_to_workspace() {
    let pool = test_helpers::setup_test_pool().await;
    test_helpers::seed_workspace(&pool, "ws-own", "/tmp/own").await;
    test_helpers::seed_thread(&pool, "t-own-1", "ws-own", None).await;
    test_helpers::seed_thread(&pool, "t-own-2", "ws-own", None).await;

    let rows = sqlx::query("SELECT id FROM threads WHERE workspace_id = 'ws-own'")
        .fetch_all(&pool)
        .await
        .unwrap();

    assert_eq!(rows.len(), 2);
}

#[tokio::test]
async fn test_thread_list_sorted_by_last_active() {
    let pool = test_helpers::setup_test_pool().await;
    test_helpers::seed_workspace(&pool, "ws-sort", "/tmp/sort").await;

    // Insert threads with different last_active_at
    sqlx::query(
        "INSERT INTO threads (id, workspace_id, title, status, last_active_at, created_at, updated_at)
         VALUES ('t-old', 'ws-sort', 'Old', 'idle', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')",
    )
    .execute(&pool)
    .await
    .unwrap();

    sqlx::query(
        "INSERT INTO threads (id, workspace_id, title, status, last_active_at, created_at, updated_at)
         VALUES ('t-new', 'ws-sort', 'New', 'idle', '2026-03-16T00:00:00Z', '2026-03-16T00:00:00Z', '2026-03-16T00:00:00Z')",
    )
    .execute(&pool)
    .await
    .unwrap();

    let rows = sqlx::query(
        "SELECT id FROM threads WHERE workspace_id = 'ws-sort'
         ORDER BY last_active_at DESC",
    )
    .fetch_all(&pool)
    .await
    .unwrap();

    assert_eq!(rows[0].get::<String, _>("id"), "t-new");
    assert_eq!(rows[1].get::<String, _>("id"), "t-old");
}

#[tokio::test]
async fn test_thread_update_title() {
    let pool = test_helpers::setup_test_pool().await;
    test_helpers::seed_workspace(&pool, "ws-title", "/tmp/title").await;
    test_helpers::seed_thread(&pool, "t-title", "ws-title", None).await;

    sqlx::query("UPDATE threads SET title = 'New Title' WHERE id = 't-title'")
        .execute(&pool)
        .await
        .unwrap();

    let row = sqlx::query("SELECT title FROM threads WHERE id = 't-title'")
        .fetch_one(&pool)
        .await
        .unwrap();

    assert_eq!(row.get::<String, _>("title"), "New Title");
}

#[tokio::test]
async fn test_thread_delete() {
    let pool = test_helpers::setup_test_pool().await;
    test_helpers::seed_workspace(&pool, "ws-del", "/tmp/del").await;
    test_helpers::seed_thread(&pool, "t-del", "ws-del", None).await;

    sqlx::query("DELETE FROM threads WHERE id = 't-del'")
        .execute(&pool)
        .await
        .unwrap();

    let row = sqlx::query("SELECT id FROM threads WHERE id = 't-del'")
        .fetch_optional(&pool)
        .await
        .unwrap();

    assert!(row.is_none());
}

#[tokio::test]
async fn test_thread_delete_cascades_runtime_records() {
    let pool = test_helpers::setup_test_pool().await;
    test_helpers::seed_workspace(&pool, "ws-del-cascade", "/tmp/del-cascade").await;
    test_helpers::seed_thread(&pool, "t-del-cascade", "ws-del-cascade", None).await;
    test_helpers::seed_run(
        &pool,
        "r-del-cascade",
        "t-del-cascade",
        "completed",
        "default",
    )
    .await;
    test_helpers::seed_message(
        &pool,
        "m-del-cascade",
        "t-del-cascade",
        "assistant",
        "Cascade me",
    )
    .await;
    test_helpers::seed_tool_call(
        &pool,
        "tool-del-cascade",
        "r-del-cascade",
        "t-del-cascade",
        "read",
        "completed",
    )
    .await;
    test_helpers::seed_run_helper(
        &pool,
        "helper-del-cascade",
        "r-del-cascade",
        "t-del-cascade",
        "review",
        "completed",
    )
    .await;

    sqlx::query("UPDATE messages SET run_id = 'r-del-cascade' WHERE id = 'm-del-cascade'")
        .execute(&pool)
        .await
        .unwrap();

    let manager = ThreadManager::new(pool.clone());
    manager
        .delete("t-del-cascade")
        .await
        .expect("thread delete should succeed");

    let thread_count =
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM threads WHERE id = 't-del-cascade'")
            .fetch_one(&pool)
            .await
            .unwrap();
    let message_count = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM messages WHERE thread_id = 't-del-cascade'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    let run_count = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM thread_runs WHERE thread_id = 't-del-cascade'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    let tool_call_count = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM tool_calls WHERE thread_id = 't-del-cascade'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    let helper_count = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM run_helpers WHERE thread_id = 't-del-cascade'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();

    assert_eq!(thread_count, 0);
    assert_eq!(message_count, 0);
    assert_eq!(run_count, 0);
    assert_eq!(tool_call_count, 0);
    assert_eq!(helper_count, 0);
}

// =========================================================================
// T1.4.2 — Message persistence
// =========================================================================

#[tokio::test]
async fn test_message_append_and_persist() {
    let pool = test_helpers::setup_test_pool().await;
    test_helpers::seed_workspace(&pool, "ws-msg", "/tmp/msg").await;
    test_helpers::seed_thread(&pool, "t-msg", "ws-msg", None).await;
    test_helpers::seed_message(&pool, "m-001", "t-msg", "user", "Hello AI").await;
    test_helpers::seed_message(&pool, "m-002", "t-msg", "assistant", "Hello human").await;

    let rows = sqlx::query(
        "SELECT role, content_markdown FROM messages WHERE thread_id = 't-msg' ORDER BY created_at",
    )
    .fetch_all(&pool)
    .await
    .unwrap();

    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].get::<String, _>("role"), "user");
    assert_eq!(rows[1].get::<String, _>("role"), "assistant");
    assert_eq!(rows[1].get::<String, _>("content_markdown"), "Hello human");
}

// =========================================================================
// T1.4.3 — Message pagination (cursor-based, UUID v7 ordering)
// =========================================================================

#[tokio::test]
async fn test_message_pagination_cursor() {
    let pool = test_helpers::setup_test_pool().await;
    test_helpers::seed_workspace(&pool, "ws-page", "/tmp/page").await;
    test_helpers::seed_thread(&pool, "t-page", "ws-page", None).await;

    // Insert messages with UUID v7-like IDs (lexicographically sortable)
    // Use a pattern that ensures ordering: 019... prefix is UUIDv7 format
    for i in 0..10 {
        let id = format!("01900000-0000-7000-8000-{i:012}");
        let content = format!("Message {i}");
        test_helpers::seed_message(&pool, &id, "t-page", "user", &content).await;
    }

    // First page: latest 3
    let page1 = sqlx::query(
        "SELECT id FROM messages WHERE thread_id = 't-page'
         ORDER BY id DESC LIMIT 3",
    )
    .fetch_all(&pool)
    .await
    .unwrap();

    assert_eq!(page1.len(), 3);
    let last_id = page1.last().unwrap().get::<String, _>("id");

    // Second page: next 3 after cursor
    let page2 = sqlx::query(
        "SELECT id FROM messages WHERE thread_id = 't-page' AND id < ?
         ORDER BY id DESC LIMIT 3",
    )
    .bind(&last_id)
    .fetch_all(&pool)
    .await
    .unwrap();

    assert_eq!(page2.len(), 3);

    // Ensure no overlap
    let page1_ids: Vec<String> = page1.iter().map(|r| r.get("id")).collect();
    let page2_ids: Vec<String> = page2.iter().map(|r| r.get("id")).collect();
    for id in &page2_ids {
        assert!(!page1_ids.contains(id), "Pages should not overlap");
    }
}

#[tokio::test]
async fn test_message_has_more_detection() {
    let pool = test_helpers::setup_test_pool().await;
    test_helpers::seed_workspace(&pool, "ws-more", "/tmp/more").await;
    test_helpers::seed_thread(&pool, "t-more", "ws-more", None).await;

    // Insert 5 messages
    for i in 0..5 {
        let id = format!("01900000-0000-7000-8000-{i:012}");
        test_helpers::seed_message(&pool, &id, "t-more", "user", "msg").await;
    }

    // Request limit+1 to detect has_more
    let limit: i64 = 3;
    let rows = sqlx::query(
        "SELECT id FROM messages WHERE thread_id = 't-more'
         ORDER BY id DESC LIMIT ?",
    )
    .bind(limit + 1)
    .fetch_all(&pool)
    .await
    .unwrap();

    let has_more = rows.len() as i64 > limit;
    assert!(has_more, "Should detect more messages exist");
}

#[tokio::test]
async fn test_thread_snapshot_keeps_latest_message_when_more_than_default_page_size() {
    let pool = test_helpers::setup_test_pool().await;
    test_helpers::seed_workspace(&pool, "ws-snap-page", "/tmp/snap-page").await;
    test_helpers::seed_thread(&pool, "t-snap-page", "ws-snap-page", None).await;

    for i in 0..51 {
        let id = format!("01900000-0000-7000-8000-{i:012}");
        let content = format!("Message {i}");
        test_helpers::seed_message(&pool, &id, "t-snap-page", "assistant", &content).await;
    }

    let manager = ThreadManager::new(pool);
    let snapshot = manager.load("t-snap-page", None, None).await.unwrap();

    assert!(snapshot.has_more_messages);
    assert_eq!(snapshot.messages.len(), 50);
    assert_eq!(
        snapshot.messages.first().map(|message| message.id.as_str()),
        Some("01900000-0000-7000-8000-000000000001")
    );
    assert_eq!(
        snapshot.messages.last().map(|message| message.id.as_str()),
        Some("01900000-0000-7000-8000-000000000050")
    );
    assert_eq!(
        snapshot
            .messages
            .last()
            .map(|message| message.content_markdown.as_str()),
        Some("Message 50")
    );
}

// =========================================================================
// T1.4.4 — ThreadStatus derivation from run state
// =========================================================================

#[tokio::test]
async fn test_thread_status_idle_when_no_runs() {
    let pool = test_helpers::setup_test_pool().await;
    test_helpers::seed_workspace(&pool, "ws-stat", "/tmp/stat").await;
    test_helpers::seed_thread(&pool, "t-stat", "ws-stat", None).await;

    let row = sqlx::query("SELECT status FROM threads WHERE id = 't-stat'")
        .fetch_one(&pool)
        .await
        .unwrap();

    assert_eq!(row.get::<String, _>("status"), "idle");
}

#[tokio::test]
async fn test_thread_status_running_with_active_run() {
    let pool = test_helpers::setup_test_pool().await;
    test_helpers::seed_workspace(&pool, "ws-run", "/tmp/run").await;
    test_helpers::seed_thread(&pool, "t-run", "ws-run", None).await;
    test_helpers::seed_run(&pool, "r-001", "t-run", "running", "default").await;

    // Thread status should reflect latest run
    sqlx::query("UPDATE threads SET status = 'running' WHERE id = 't-run'")
        .execute(&pool)
        .await
        .unwrap();

    let row = sqlx::query("SELECT status FROM threads WHERE id = 't-run'")
        .fetch_one(&pool)
        .await
        .unwrap();

    assert_eq!(row.get::<String, _>("status"), "running");
}

// =========================================================================
// T1.4.5 — Thread snapshot assembly
// =========================================================================

#[tokio::test]
async fn test_thread_snapshot_assembly() {
    let pool = test_helpers::setup_test_pool().await;
    test_helpers::seed_workspace(&pool, "ws-snap", "/tmp/snap").await;
    test_helpers::seed_thread(&pool, "t-snap", "ws-snap", None).await;
    test_helpers::seed_message(&pool, "m-s1", "t-snap", "user", "Q").await;
    test_helpers::seed_message(&pool, "m-s2", "t-snap", "assistant", "A").await;
    test_helpers::seed_run(&pool, "r-snap", "t-snap", "completed", "default").await;

    // Verify we can query all snapshot components
    let thread = sqlx::query("SELECT id, title, status FROM threads WHERE id = 't-snap'")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(thread.get::<String, _>("id"), "t-snap");

    let messages =
        sqlx::query("SELECT id FROM messages WHERE thread_id = 't-snap' ORDER BY id DESC")
            .fetch_all(&pool)
            .await
            .unwrap();
    assert_eq!(messages.len(), 2);

    let runs = sqlx::query("SELECT id, status FROM thread_runs WHERE thread_id = 't-snap' ORDER BY started_at DESC LIMIT 1")
        .fetch_all(&pool)
        .await
        .unwrap();
    assert_eq!(runs.len(), 1);
    assert_eq!(runs[0].get::<String, _>("status"), "completed");
}

#[tokio::test]
async fn test_thread_snapshot_includes_latest_failed_run_error() {
    let pool = test_helpers::setup_test_pool().await;
    test_helpers::seed_workspace(&pool, "ws-fail", "/tmp/fail").await;
    test_helpers::seed_thread(&pool, "t-fail", "ws-fail", None).await;
    test_helpers::seed_message(&pool, "m-f1", "t-fail", "user", "Q").await;

    sqlx::query(
        "INSERT INTO thread_runs (id, thread_id, run_mode, status, error_message)
         VALUES ('r-fail', 't-fail', 'default', 'failed', 'Upstream API timeout')",
    )
    .execute(&pool)
    .await
    .unwrap();

    let manager = ThreadManager::new(pool);
    let snapshot = manager.load("t-fail", None, None).await.unwrap();

    assert!(
        snapshot.active_run.is_none(),
        "failed runs should not be treated as active"
    );
    let latest_run = snapshot.latest_run.expect("latest run should be returned");
    assert_eq!(latest_run.id, "r-fail");
    assert_eq!(latest_run.status, "failed");
    assert_eq!(
        latest_run.error_message.as_deref(),
        Some("Upstream API timeout")
    );
}

#[tokio::test]
async fn test_thread_snapshot_includes_runtime_artifacts_for_visible_runs() {
    let pool = test_helpers::setup_test_pool().await;
    test_helpers::seed_workspace(&pool, "ws-runtime", "/tmp/runtime").await;
    test_helpers::seed_thread(&pool, "t-runtime", "ws-runtime", None).await;

    sqlx::query(
        "INSERT INTO thread_runs (
            id, thread_id, run_mode, status, effective_model_plan_json,
            input_tokens, output_tokens, total_tokens
         )
         VALUES (
            'r-runtime',
            't-runtime',
            'default',
            'completed',
            '{\"primary\":{\"modelDisplayName\":\"GPT-4.1\",\"contextWindow\":\"128000\"}}',
            512,
            96,
            608
         )",
    )
    .execute(&pool)
    .await
    .unwrap();

    sqlx::query(
        "INSERT INTO messages (id, thread_id, run_id, role, content_markdown, message_type, status)
         VALUES
         ('m-runtime-user', 't-runtime', NULL, 'user', 'Investigate this', 'plain_message', 'completed'),
         ('m-runtime-reasoning', 't-runtime', 'r-runtime', 'assistant', 'Inspecting files', 'reasoning', 'completed'),
         ('m-runtime-answer', 't-runtime', 'r-runtime', 'assistant', 'Done', 'plain_message', 'completed')",
    )
    .execute(&pool)
    .await
    .unwrap();

    test_helpers::seed_tool_call(
        &pool,
        "tc-runtime",
        "r-runtime",
        "t-runtime",
        "search",
        "completed",
    )
    .await;
    test_helpers::seed_run_helper(
        &pool,
        "rh-runtime",
        "r-runtime",
        "t-runtime",
        "helper_explore",
        "completed",
    )
    .await;

    sqlx::query(
        "UPDATE run_helpers
         SET input_tokens = 120, output_tokens = 24, total_tokens = 144
         WHERE id = 'rh-runtime'",
    )
    .execute(&pool)
    .await
    .unwrap();

    let manager = ThreadManager::new(pool);
    let snapshot = manager.load("t-runtime", None, None).await.unwrap();

    assert_eq!(snapshot.tool_calls.len(), 1);
    assert_eq!(snapshot.tool_calls[0].tool_name, "search");
    assert_eq!(snapshot.helpers.len(), 1);
    assert_eq!(snapshot.helpers[0].helper_kind, "helper_explore");
    assert_eq!(snapshot.helpers[0].usage.input_tokens, 120);
    assert_eq!(snapshot.helpers[0].usage.output_tokens, 24);
    assert_eq!(snapshot.helpers[0].usage.total_tokens, 144);
    assert_eq!(
        snapshot
            .latest_run
            .as_ref()
            .and_then(|run| run.model_display_name.as_deref()),
        Some("GPT-4.1")
    );
    assert_eq!(
        snapshot
            .latest_run
            .as_ref()
            .and_then(|run| run.context_window.as_deref()),
        Some("128000")
    );
    assert_eq!(
        snapshot
            .latest_run
            .as_ref()
            .map(|run| run.usage.total_tokens),
        Some(608)
    );
    assert!(
        snapshot
            .messages
            .iter()
            .any(|message| message.message_type == "reasoning"),
        "reasoning messages should persist in thread snapshots"
    );
}

#[tokio::test]
async fn test_recover_interrupted_runs_reconciles_thread_statuses() {
    let pool = test_helpers::setup_test_pool().await;
    test_helpers::seed_workspace(&pool, "ws-rec", "/tmp/rec").await;
    test_helpers::seed_thread(&pool, "t-active", "ws-rec", None).await;
    test_helpers::seed_thread(&pool, "t-stale", "ws-rec", None).await;
    test_helpers::seed_thread(&pool, "t-empty", "ws-rec", None).await;

    sqlx::query(
        "UPDATE threads
         SET status = CASE id
             WHEN 't-active' THEN 'running'
             WHEN 't-stale' THEN 'running'
             WHEN 't-empty' THEN 'waiting_approval'
         END
         WHERE id IN ('t-active', 't-stale', 't-empty')",
    )
    .execute(&pool)
    .await
    .unwrap();

    test_helpers::seed_run(&pool, "r-active", "t-active", "running", "default").await;
    test_helpers::seed_run(&pool, "r-stale", "t-stale", "interrupted", "default").await;

    let manager = ThreadManager::new(pool.clone());
    manager.recover_interrupted_runs().await.unwrap();

    let active_run = sqlx::query("SELECT status FROM thread_runs WHERE id = 'r-active'")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(active_run.get::<String, _>("status"), "interrupted");

    let active_thread = sqlx::query("SELECT status FROM threads WHERE id = 't-active'")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(active_thread.get::<String, _>("status"), "interrupted");

    let stale_thread = sqlx::query("SELECT status FROM threads WHERE id = 't-stale'")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(stale_thread.get::<String, _>("status"), "interrupted");

    let empty_thread = sqlx::query("SELECT status FROM threads WHERE id = 't-empty'")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(empty_thread.get::<String, _>("status"), "idle");
}

// =========================================================================
// T1.4.6 — Message metadata JSON storage
// =========================================================================

#[tokio::test]
async fn test_message_with_metadata() {
    let pool = test_helpers::setup_test_pool().await;
    test_helpers::seed_workspace(&pool, "ws-meta", "/tmp/meta").await;
    test_helpers::seed_thread(&pool, "t-meta", "ws-meta", None).await;

    sqlx::query(
        "INSERT INTO messages (id, thread_id, role, content_markdown, message_type, status, metadata_json)
         VALUES ('m-meta', 't-meta', 'assistant', 'With metadata', 'plain_message', 'completed', ?)",
    )
    .bind(r#"{"model":"gpt-4","tokens":150}"#)
    .execute(&pool)
    .await
    .unwrap();

    let row = sqlx::query("SELECT metadata_json FROM messages WHERE id = 'm-meta'")
        .fetch_one(&pool)
        .await
        .unwrap();

    let meta: serde_json::Value =
        serde_json::from_str(&row.get::<String, _>("metadata_json")).unwrap();
    assert_eq!(meta["model"].as_str().unwrap(), "gpt-4");
    assert_eq!(meta["tokens"].as_u64().unwrap(), 150);
}

// =========================================================================
// T1.4.8 — replace_parts persistence
// =========================================================================

#[tokio::test]
async fn test_replace_parts_stores_json() {
    let pool = test_helpers::setup_test_pool().await;
    test_helpers::seed_workspace(&pool, "ws-parts", "/tmp/parts").await;
    test_helpers::seed_thread(&pool, "t-parts", "ws-parts", None).await;
    test_helpers::seed_message(&pool, "m-parts-1", "t-parts", "assistant", "Hello").await;

    let parts_json = serde_json::json!([
        { "type": "text", "text": "Hello" },
        { "type": "chart", "artifactId": "a-1", "library": "vega-lite", "spec": {} }
    ]);

    tiycode_lib::persistence::repo::message_repo::replace_parts(
        &pool,
        "m-parts-1",
        Some(&parts_json.to_string()),
    )
    .await
    .expect("replace_parts should succeed");

    let row = sqlx::query("SELECT parts_json FROM messages WHERE id = 'm-parts-1'")
        .fetch_one(&pool)
        .await
        .unwrap();

    let stored: serde_json::Value =
        serde_json::from_str(&row.get::<String, _>("parts_json")).unwrap();
    assert_eq!(stored[0]["type"].as_str().unwrap(), "text");
    assert_eq!(stored[1]["type"].as_str().unwrap(), "chart");
    assert_eq!(stored[1]["artifactId"].as_str().unwrap(), "a-1");
}

#[tokio::test]
async fn test_replace_parts_nullifies_column() {
    let pool = test_helpers::setup_test_pool().await;
    test_helpers::seed_workspace(&pool, "ws-null", "/tmp/null").await;
    test_helpers::seed_thread(&pool, "t-null", "ws-null", None).await;
    test_helpers::seed_message(&pool, "m-null-1", "t-null", "assistant", "text").await;

    // Set parts first
    tiycode_lib::persistence::repo::message_repo::replace_parts(
        &pool,
        "m-null-1",
        Some(r#"[{"type":"text","text":"hi"}]"#),
    )
    .await
    .unwrap();

    // Nullify
    tiycode_lib::persistence::repo::message_repo::replace_parts(&pool, "m-null-1", None)
        .await
        .unwrap();

    let row = sqlx::query("SELECT parts_json FROM messages WHERE id = 'm-null-1'")
        .fetch_one(&pool)
        .await
        .unwrap();
    let val: Option<String> = row.get("parts_json");
    assert!(val.is_none());
}

// =========================================================================
// T1.4.9 — merge_chart_artifact_part
// =========================================================================

#[tokio::test]
async fn test_merge_chart_artifact_part_creates_parts_from_empty() {
    let pool = test_helpers::setup_test_pool().await;
    test_helpers::seed_workspace(&pool, "ws-merge", "/tmp/merge").await;
    test_helpers::seed_thread(&pool, "t-merge", "ws-merge", None).await;
    test_helpers::seed_message(&pool, "m-merge-1", "t-merge", "assistant", "Analysis:").await;

    let chart_payload = serde_json::json!({
        "library": "vega-lite",
        "spec": { "mark": "bar", "encoding": {} },
        "title": "Sales",
        "caption": null,
        "status": "ready",
    });

    tiycode_lib::persistence::repo::message_repo::merge_chart_artifact_part(
        &pool,
        "m-merge-1",
        "art-001",
        chart_payload,
    )
    .await
    .expect("merge should succeed");

    let row = sqlx::query("SELECT parts_json FROM messages WHERE id = 'm-merge-1'")
        .fetch_one(&pool)
        .await
        .unwrap();

    let parts: Vec<serde_json::Value> =
        serde_json::from_str(&row.get::<String, _>("parts_json")).unwrap();

    // Should contain migrated text part + new chart part
    assert_eq!(parts.len(), 2);
    assert_eq!(parts[0]["type"].as_str().unwrap(), "text");
    assert_eq!(parts[0]["text"].as_str().unwrap(), "Analysis:");
    assert_eq!(parts[1]["type"].as_str().unwrap(), "chart");
    assert_eq!(parts[1]["artifactId"].as_str().unwrap(), "art-001");
    assert_eq!(parts[1]["library"].as_str().unwrap(), "vega-lite");
    assert_eq!(parts[1]["title"].as_str().unwrap(), "Sales");
}

#[tokio::test]
async fn test_merge_chart_artifact_part_updates_existing_chart() {
    let pool = test_helpers::setup_test_pool().await;
    test_helpers::seed_workspace(&pool, "ws-upd", "/tmp/upd").await;
    test_helpers::seed_thread(&pool, "t-upd", "ws-upd", None).await;
    test_helpers::seed_message(&pool, "m-upd-1", "t-upd", "assistant", "").await;

    // First insert
    let payload_v1 = serde_json::json!({
        "library": "vega-lite",
        "spec": { "mark": "line" },
        "title": "V1",
        "status": "ready",
    });
    tiycode_lib::persistence::repo::message_repo::merge_chart_artifact_part(
        &pool, "m-upd-1", "art-002", payload_v1,
    )
    .await
    .unwrap();

    // Update same artifact
    let payload_v2 = serde_json::json!({
        "library": "vega-lite",
        "spec": { "mark": "area" },
        "title": "V2",
        "status": "ready",
    });
    tiycode_lib::persistence::repo::message_repo::merge_chart_artifact_part(
        &pool, "m-upd-1", "art-002", payload_v2,
    )
    .await
    .unwrap();

    let row = sqlx::query("SELECT parts_json FROM messages WHERE id = 'm-upd-1'")
        .fetch_one(&pool)
        .await
        .unwrap();

    let parts: Vec<serde_json::Value> =
        serde_json::from_str(&row.get::<String, _>("parts_json")).unwrap();

    // Should still be only 1 chart part (upsert, not duplicate)
    let chart_parts: Vec<_> = parts
        .iter()
        .filter(|p| p["type"].as_str() == Some("chart"))
        .collect();
    assert_eq!(chart_parts.len(), 1);
    assert_eq!(chart_parts[0]["title"].as_str().unwrap(), "V2");
    assert_eq!(chart_parts[0]["spec"]["mark"].as_str().unwrap(), "area");
}

#[tokio::test]
async fn test_merge_chart_artifact_part_skips_empty_content_migration() {
    let pool = test_helpers::setup_test_pool().await;
    test_helpers::seed_workspace(&pool, "ws-empty", "/tmp/empty").await;
    test_helpers::seed_thread(&pool, "t-empty", "ws-empty", None).await;
    // Message with empty content — should NOT create a text part
    test_helpers::seed_message(&pool, "m-empty-1", "t-empty", "assistant", "").await;

    let chart_payload = serde_json::json!({
        "library": "vega-lite",
        "spec": { "mark": "point" },
        "status": "ready",
    });

    tiycode_lib::persistence::repo::message_repo::merge_chart_artifact_part(
        &pool,
        "m-empty-1",
        "art-003",
        chart_payload,
    )
    .await
    .unwrap();

    let row = sqlx::query("SELECT parts_json FROM messages WHERE id = 'm-empty-1'")
        .fetch_one(&pool)
        .await
        .unwrap();

    let parts: Vec<serde_json::Value> =
        serde_json::from_str(&row.get::<String, _>("parts_json")).unwrap();

    // Only chart part, no text part from empty content
    assert_eq!(parts.len(), 1);
    assert_eq!(parts[0]["type"].as_str().unwrap(), "chart");
}
