//! Shared test helpers for TiyCode integration tests.
//!
//! Provides an in-memory SQLite pool with migrations applied,
//! useful for testing repo and manager layers without touching disk.
#![allow(dead_code)]

use chrono::SecondsFormat;
use sqlx::sqlite::{SqliteConnectOptions, SqlitePool, SqlitePoolOptions};
use std::str::FromStr;

/// Create an in-memory SQLite pool with all migrations applied.
/// Each call creates an isolated database — safe for parallel tests.
pub async fn setup_test_pool() -> SqlitePool {
    let options = SqliteConnectOptions::from_str("sqlite::memory:")
        .expect("invalid sqlite options")
        .foreign_keys(true);

    let pool = SqlitePoolOptions::new()
        .max_connections(1) // in-memory DB requires single connection to persist
        .connect_with(options)
        .await
        .expect("failed to create in-memory pool");

    // Run migrations
    sqlx::migrate!("./migrations")
        .run(&pool)
        .await
        .expect("migrations failed");

    pool
}

/// Create a workspace record directly in the database for test setup.
pub async fn seed_workspace(pool: &SqlitePool, id: &str, canonical_path: &str) {
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

/// Create a thread record for test setup.
pub async fn seed_thread(
    pool: &SqlitePool,
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

/// Create a message record for test setup.
pub async fn seed_message(
    pool: &SqlitePool,
    message_id: &str,
    thread_id: &str,
    role: &str,
    content: &str,
) {
    sqlx::query(
        "INSERT INTO messages (id, thread_id, role, content_markdown, message_type, status)
         VALUES (?, ?, ?, ?, 'plain_message', 'completed')",
    )
    .bind(message_id)
    .bind(thread_id)
    .bind(role)
    .bind(content)
    .execute(pool)
    .await
    .expect("failed to seed message");
}

/// Create a run record for test setup.
pub async fn seed_run(
    pool: &SqlitePool,
    run_id: &str,
    thread_id: &str,
    status: &str,
    run_mode: &str,
) {
    let now = chrono::Utc::now().to_rfc3339_opts(SecondsFormat::Nanos, true);
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

/// Create a tool call record for test setup.
pub async fn seed_tool_call(
    pool: &SqlitePool,
    tool_call_id: &str,
    run_id: &str,
    thread_id: &str,
    tool_name: &str,
    status: &str,
) {
    sqlx::query(
        "INSERT INTO tool_calls (id, tool_call_id, run_id, thread_id, tool_name, status)
         VALUES (?, ?, ?, ?, ?, ?)",
    )
    .bind(tool_call_id)
    .bind(tool_call_id)
    .bind(run_id)
    .bind(thread_id)
    .bind(tool_name)
    .bind(status)
    .execute(pool)
    .await
    .expect("failed to seed tool_call");
}

/// Create a helper run record for test setup.
pub async fn seed_run_helper(
    pool: &SqlitePool,
    helper_id: &str,
    run_id: &str,
    thread_id: &str,
    helper_kind: &str,
    status: &str,
) {
    sqlx::query(
        "INSERT INTO run_helpers (id, run_id, thread_id, helper_kind, status)
         VALUES (?, ?, ?, ?, ?)",
    )
    .bind(helper_id)
    .bind(run_id)
    .bind(thread_id)
    .bind(helper_kind)
    .bind(status)
    .execute(pool)
    .await
    .expect("failed to seed run_helper");
}

/// Create a settings entry.
pub async fn seed_setting(pool: &SqlitePool, key: &str, value_json: &str) {
    sqlx::query("INSERT OR REPLACE INTO settings (key, value_json) VALUES (?, ?)")
        .bind(key)
        .bind(value_json)
        .execute(pool)
        .await
        .expect("failed to seed setting");
}

/// Create a policy entry.
pub async fn seed_policy(pool: &SqlitePool, key: &str, value_json: &str) {
    sqlx::query("INSERT OR REPLACE INTO policies (key, value_json) VALUES (?, ?)")
        .bind(key)
        .bind(value_json)
        .execute(pool)
        .await
        .expect("failed to seed policy");
}
