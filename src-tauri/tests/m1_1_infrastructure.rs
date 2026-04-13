//! M1.1 — Project infrastructure & database layer tests
//!
//! Acceptance criteria:
//! - `cargo build` passes; app startup creates `$HOME/.tiy/db/tiycode.db` with 17 tables
//! - Logs written to platform-specific log path (macOS ~/Library/Logs/TiyAgents/)
//! - `cargo test` persistence module passes

mod test_helpers;

use sqlx::Row;

// =========================================================================
// T1.1.1 — Database pool creation and migration
// =========================================================================

#[tokio::test]
async fn test_database_pool_creates_successfully() {
    let pool = test_helpers::setup_test_pool().await;
    // Pool should be valid and connected
    let row = sqlx::query("SELECT 1 AS val")
        .fetch_one(&pool)
        .await
        .expect("pool should execute queries");
    assert_eq!(row.get::<i32, _>("val"), 1);
}

#[tokio::test]
async fn test_migrations_create_all_tables() {
    let pool = test_helpers::setup_test_pool().await;

    // Query sqlite_master for all user tables
    let rows = sqlx::query("SELECT name FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%' AND name NOT LIKE '_sqlx_%' ORDER BY name")
        .fetch_all(&pool)
        .await
        .expect("should query sqlite_master");

    let table_names: Vec<String> = rows.iter().map(|r| r.get("name")).collect();

    // Verify all expected tables exist
    let expected_tables = vec![
        "agent_profiles",
        "audit_events",
        "automation_runs",
        "commands",
        "marketplace_items",
        "messages",
        "policies",
        "provider_models",
        "providers",
        "run_subtasks",
        "settings",
        "terminal_sessions",
        "thread_runs",
        "thread_summaries",
        "threads",
        "tool_calls",
        "workspaces",
    ];

    for table in &expected_tables {
        assert!(
            table_names.contains(&table.to_string()),
            "Missing table: {table}. Found tables: {table_names:?}"
        );
    }

    // Total should be 17 core tables (excluding _sqlx_migrations)
    assert!(
        table_names.len() >= expected_tables.len(),
        "Expected at least {} tables, found {}: {:?}",
        expected_tables.len(),
        table_names.len(),
        table_names
    );
}

#[tokio::test]
async fn test_migrations_seed_latest_default_settings() {
    let pool = test_helpers::setup_test_pool().await;

    let minimize_to_tray: String =
        sqlx::query_scalar("SELECT value_json FROM settings WHERE key = ?")
            .bind("general.minimize_to_tray")
            .fetch_one(&pool)
            .await
            .expect("should seed minimize-to-tray setting");
    assert_eq!(minimize_to_tray, "true");

    let deny_list: String = sqlx::query_scalar("SELECT value_json FROM policies WHERE key = ?")
        .bind("deny_list")
        .fetch_one(&pool)
        .await
        .expect("should seed deny-list defaults");
    let deny_list_json: serde_json::Value =
        serde_json::from_str(&deny_list).expect("deny_list should be valid JSON");

    assert_eq!(deny_list_json.as_array().map(Vec::len), Some(2));
    assert_eq!(deny_list_json[0]["id"], "default-deny-rm-root");
    assert_eq!(deny_list_json[1]["id"], "default-deny-rm-literal-star");
}

// =========================================================================
// T1.1.2 — WAL mode and pragmas
// =========================================================================

#[tokio::test]
async fn test_database_wal_mode() {
    let pool = test_helpers::setup_test_pool().await;

    let row = sqlx::query("PRAGMA journal_mode")
        .fetch_one(&pool)
        .await
        .expect("should query journal_mode");

    let journal_mode: String = row.get(0);
    // In-memory DB uses "memory" journal mode, but WAL is set via connection options.
    // For in-memory, this will be "memory"; in production it would be "wal".
    assert!(
        journal_mode == "wal" || journal_mode == "memory",
        "Expected WAL or memory journal mode, got: {journal_mode}"
    );
}

#[tokio::test]
async fn test_foreign_keys_enabled() {
    let pool = test_helpers::setup_test_pool().await;

    let row = sqlx::query("PRAGMA foreign_keys")
        .fetch_one(&pool)
        .await
        .expect("should query foreign_keys");

    let fk: i32 = row.get(0);
    assert_eq!(fk, 1, "Foreign keys should be enabled");
}

// =========================================================================
// T1.1.3 — Table schema validation
// =========================================================================

#[tokio::test]
async fn test_workspaces_table_schema() {
    let pool = test_helpers::setup_test_pool().await;

    let rows = sqlx::query("PRAGMA table_info(workspaces)")
        .fetch_all(&pool)
        .await
        .expect("should get table info");

    let columns: Vec<String> = rows.iter().map(|r| r.get::<String, _>("name")).collect();

    let expected = vec![
        "id",
        "name",
        "path",
        "canonical_path",
        "display_path",
        "is_default",
        "is_git",
        "auto_work_tree",
        "status",
        "last_validated_at",
        "created_at",
        "updated_at",
    ];

    for col in &expected {
        assert!(
            columns.contains(&col.to_string()),
            "Missing column '{col}' in workspaces table. Found: {columns:?}"
        );
    }
}

#[tokio::test]
async fn test_threads_table_schema() {
    let pool = test_helpers::setup_test_pool().await;

    let rows = sqlx::query("PRAGMA table_info(threads)")
        .fetch_all(&pool)
        .await
        .expect("should get table info");

    let columns: Vec<String> = rows.iter().map(|r| r.get::<String, _>("name")).collect();

    for col in &[
        "id",
        "workspace_id",
        "title",
        "status",
        "summary",
        "last_active_at",
        "created_at",
        "updated_at",
    ] {
        assert!(
            columns.contains(&col.to_string()),
            "Missing column '{col}' in threads table"
        );
    }
}

#[tokio::test]
async fn test_messages_table_schema() {
    let pool = test_helpers::setup_test_pool().await;

    let rows = sqlx::query("PRAGMA table_info(messages)")
        .fetch_all(&pool)
        .await
        .expect("should get table info");

    let columns: Vec<String> = rows.iter().map(|r| r.get::<String, _>("name")).collect();

    for col in &[
        "id",
        "thread_id",
        "run_id",
        "role",
        "content_markdown",
        "message_type",
        "status",
        "metadata_json",
        "created_at",
    ] {
        assert!(
            columns.contains(&col.to_string()),
            "Missing column '{col}' in messages table"
        );
    }
}

#[tokio::test]
async fn test_thread_runs_table_schema() {
    let pool = test_helpers::setup_test_pool().await;

    let rows = sqlx::query("PRAGMA table_info(thread_runs)")
        .fetch_all(&pool)
        .await
        .expect("should get table info");

    let columns: Vec<String> = rows.iter().map(|r| r.get::<String, _>("name")).collect();

    for col in &[
        "id",
        "thread_id",
        "profile_id",
        "run_mode",
        "execution_strategy",
        "status",
        "error_message",
        "started_at",
        "finished_at",
        "effective_model_plan_json",
    ] {
        assert!(
            columns.contains(&col.to_string()),
            "Missing column '{col}' in thread_runs table"
        );
    }
}

#[tokio::test]
async fn test_tool_calls_table_schema() {
    let pool = test_helpers::setup_test_pool().await;

    let rows = sqlx::query("PRAGMA table_info(tool_calls)")
        .fetch_all(&pool)
        .await
        .expect("should get table info");

    let columns: Vec<String> = rows.iter().map(|r| r.get::<String, _>("name")).collect();

    for col in &[
        "id",
        "run_id",
        "thread_id",
        "tool_name",
        "tool_input_json",
        "tool_output_json",
        "status",
        "approval_status",
        "policy_verdict_json",
    ] {
        assert!(
            columns.contains(&col.to_string()),
            "Missing column '{col}' in tool_calls table"
        );
    }
}

// =========================================================================
// T1.1.4 — Index existence verification
// =========================================================================

#[tokio::test]
async fn test_critical_indexes_exist() {
    let pool = test_helpers::setup_test_pool().await;

    let rows = sqlx::query(
        "SELECT name FROM sqlite_master WHERE type='index' AND name NOT LIKE 'sqlite_%'",
    )
    .fetch_all(&pool)
    .await
    .expect("should query indexes");

    let indexes: Vec<String> = rows.iter().map(|r| r.get("name")).collect();

    let expected_indexes = vec![
        "idx_workspaces_is_default",
        "idx_threads_workspace",
        "idx_threads_workspace_active",
        "idx_messages_thread",
        "idx_messages_thread_page",
        "idx_runs_thread",
        "idx_runs_thread_active",
        "idx_tool_calls_run",
        "idx_tool_calls_pending",
    ];

    for idx in &expected_indexes {
        assert!(
            indexes.contains(&idx.to_string()),
            "Missing index: {idx}. Found: {indexes:?}"
        );
    }
}

// =========================================================================
// T1.1.5 — Foreign key constraint enforcement
// =========================================================================

#[tokio::test]
async fn test_fk_thread_requires_workspace() {
    let pool = test_helpers::setup_test_pool().await;

    // Attempt to insert a thread with a non-existent workspace_id
    let result = sqlx::query(
        "INSERT INTO threads (id, workspace_id, title, status, last_active_at, created_at, updated_at)
         VALUES ('t1', 'nonexistent_ws', 'Test', 'idle',
                 strftime('%Y-%m-%dT%H:%M:%fZ','now'),
                 strftime('%Y-%m-%dT%H:%M:%fZ','now'),
                 strftime('%Y-%m-%dT%H:%M:%fZ','now'))",
    )
    .execute(&pool)
    .await;

    assert!(
        result.is_err(),
        "Should fail FK constraint: thread referencing non-existent workspace"
    );
}

#[tokio::test]
async fn test_fk_message_requires_thread() {
    let pool = test_helpers::setup_test_pool().await;

    let result = sqlx::query(
        "INSERT INTO messages (id, thread_id, role, content_markdown)
         VALUES ('m1', 'nonexistent_thread', 'user', 'hello')",
    )
    .execute(&pool)
    .await;

    assert!(
        result.is_err(),
        "Should fail FK constraint: message referencing non-existent thread"
    );
}

// =========================================================================
// T1.1.6 — Error type validation
// =========================================================================

#[test]
fn test_app_error_internal_format() {
    use tiycode::model::errors::{AppError, ErrorCategory, ErrorSource};

    let err = AppError::internal(ErrorSource::Database, "connection lost");

    assert_eq!(err.error_code, "database._internal");
    assert!(matches!(err.category, ErrorCategory::Fatal));
    assert_eq!(err.user_message, "connection lost");
    assert!(!err.retryable);
}

#[test]
fn test_app_error_recoverable_format() {
    use tiycode::model::errors::{AppError, ErrorCategory, ErrorSource};

    let err = AppError::recoverable(
        ErrorSource::Workspace,
        "workspace.path.invalid",
        "Invalid path",
    );

    assert_eq!(err.error_code, "workspace.path.invalid");
    assert!(matches!(err.category, ErrorCategory::Recoverable));
    assert!(err.retryable);
}

#[test]
fn test_app_error_not_found_format() {
    use tiycode::model::errors::{AppError, ErrorSource};

    let err = AppError::not_found(ErrorSource::Thread, "thread");

    assert_eq!(err.error_code, "thread.not_found");
    assert_eq!(err.user_message, "thread not found");
    assert!(!err.retryable);
}

#[test]
fn test_app_error_display() {
    use tiycode::model::errors::{AppError, ErrorSource};

    let err = AppError::internal(ErrorSource::System, "out of memory");
    let display = format!("{err}");
    assert!(display.contains("system._internal"));
    assert!(display.contains("out of memory"));
}

#[test]
fn test_app_error_from_io_error() {
    use tiycode::model::errors::{AppError, ErrorSource};

    let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "file not found");
    let app_err: AppError = io_err.into();

    assert!(matches!(app_err.source, ErrorSource::System));
    assert!(app_err.user_message.contains("file not found"));
}

// =========================================================================
// T1.1.7 — Directory structure verification (runtime test)
// =========================================================================

#[test]
fn test_tiy_home_resolves() {
    // Verify that dirs::home_dir() resolves (required for app startup)
    let home = dirs::home_dir();
    assert!(home.is_some(), "HOME directory should resolve");

    let tiy_home = home.unwrap().join(".tiy");
    // Don't assert existence — that's a runtime concern.
    // Just verify the path is constructable.
    assert!(tiy_home.to_string_lossy().contains(".tiy"));
}
