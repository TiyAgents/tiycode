//! M1.2 — Workspace management tests
//!
//! Acceptance criteria:
//! - Add workspace via folder, path canonicalized, stored in DB
//! - Duplicate paths rejected
//! - Sidebar workspace list loaded from SQLite
//! - Startup re-validates path status (Missing/Ready)

mod test_helpers;

use sqlx::Row;
use std::ffi::OsString;
use std::path::Path;
use std::sync::{Mutex, OnceLock};
use tiycode::core::workspace_manager::WorkspaceManager;
use tiycode::model::workspace::WorkspaceAddInput;

fn home_env_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

struct HomeEnvGuard {
    original_home: Option<OsString>,
    #[cfg(target_os = "windows")]
    original_userprofile: Option<OsString>,
}

impl HomeEnvGuard {
    fn set(home: &Path) -> Self {
        let original_home = std::env::var_os("HOME");
        std::env::set_var("HOME", home);

        #[cfg(target_os = "windows")]
        let original_userprofile = {
            let prev = std::env::var_os("USERPROFILE");
            std::env::set_var("USERPROFILE", home);
            prev
        };

        Self {
            original_home,
            #[cfg(target_os = "windows")]
            original_userprofile,
        }
    }
}

impl Drop for HomeEnvGuard {
    fn drop(&mut self) {
        match &self.original_home {
            Some(home) => std::env::set_var("HOME", home),
            None => std::env::remove_var("HOME"),
        }

        #[cfg(target_os = "windows")]
        match &self.original_userprofile {
            Some(profile) => std::env::set_var("USERPROFILE", profile),
            None => std::env::remove_var("USERPROFILE"),
        }
    }
}

// =========================================================================
// T1.2.1 — Workspace CRUD operations (repo layer)
// =========================================================================

#[tokio::test]
async fn test_workspace_insert_and_list() {
    let pool = test_helpers::setup_test_pool().await;

    test_helpers::seed_workspace(&pool, "ws-001", "/tmp/project-alpha").await;
    test_helpers::seed_workspace(&pool, "ws-002", "/tmp/project-beta").await;

    let rows = sqlx::query("SELECT id FROM workspaces ORDER BY updated_at DESC")
        .fetch_all(&pool)
        .await
        .unwrap();

    assert_eq!(rows.len(), 2);
}

#[tokio::test]
async fn test_workspace_duplicate_canonical_path_rejected() {
    let pool = test_helpers::setup_test_pool().await;

    test_helpers::seed_workspace(&pool, "ws-001", "/tmp/project").await;

    // Insert same canonical_path with different id → should fail UNIQUE constraint
    let result = sqlx::query(
        "INSERT INTO workspaces (id, name, path, canonical_path, display_path,
                is_default, is_git, auto_work_tree, status, created_at, updated_at)
         VALUES ('ws-002', 'Dup', '/tmp/project', '/tmp/project', '/tmp/project',
                 0, 0, 0, 'ready',
                 strftime('%Y-%m-%dT%H:%M:%fZ','now'),
                 strftime('%Y-%m-%dT%H:%M:%fZ','now'))",
    )
    .execute(&pool)
    .await;

    assert!(
        result.is_err(),
        "Duplicate canonical_path should be rejected by UNIQUE constraint"
    );
}

#[tokio::test]
async fn test_workspace_manager_add_reuses_existing_workspace_for_same_canonical_path() {
    let workspace_root = tempfile::tempdir().expect("should create temp workspace root");
    let workspace_path = workspace_root.path().join("project");
    tokio::fs::create_dir_all(&workspace_path)
        .await
        .expect("should create workspace directory");

    let pool = test_helpers::setup_test_pool().await;
    let manager = WorkspaceManager::new(pool.clone());

    let first = manager
        .add(WorkspaceAddInput {
            path: workspace_path.to_string_lossy().to_string(),
            name: Some("Project".to_string()),
        })
        .await
        .expect("first workspace add should succeed");

    let second = manager
        .add(WorkspaceAddInput {
            path: workspace_path.join(".").to_string_lossy().to_string(),
            name: Some("Project Duplicate".to_string()),
        })
        .await
        .expect("second workspace add should reuse existing workspace");

    assert_eq!(second.id, first.id);
    assert_eq!(second.canonical_path, first.canonical_path);
    assert_eq!(second.path, first.path);
    assert_eq!(second.name, first.name);

    let row = sqlx::query("SELECT COUNT(*) as count FROM workspaces WHERE canonical_path = ?")
        .bind(&first.canonical_path)
        .fetch_one(&pool)
        .await
        .unwrap();

    assert_eq!(row.get::<i64, _>("count"), 1);
}

#[tokio::test]
async fn test_workspace_ensure_default_creates_and_reuses_default_workspace() {
    let _home_lock = home_env_lock().lock().unwrap_or_else(|e| e.into_inner());
    let temp_home = tempfile::tempdir().expect("should create temp home");
    let _home_guard = HomeEnvGuard::set(temp_home.path());

    let pool = test_helpers::setup_test_pool().await;
    let manager = WorkspaceManager::new(pool.clone());

    let record = manager.ensure_default_thread_workspace().await.unwrap();
    let expected_path = temp_home
        .path()
        .join(".tiy")
        .join("workspace")
        .join("Default");
    let expected_canonical = dunce::canonicalize(&expected_path)
        .unwrap()
        .to_string_lossy()
        .to_string();
    let expected_display = format!(
        "~/{}",
        Path::new(".tiy")
            .join("workspace")
            .join("Default")
            .display()
    );

    assert_eq!(record.name, "Default");
    assert_eq!(record.path, expected_path.to_string_lossy().to_string());
    assert_eq!(record.canonical_path, expected_canonical);
    assert_eq!(record.display_path, expected_display);
    assert!(expected_path.is_dir());

    let reused = manager.ensure_default_thread_workspace().await.unwrap();
    assert_eq!(reused.id, record.id);

    let row = sqlx::query("SELECT COUNT(*) as count FROM workspaces WHERE canonical_path = ?")
        .bind(&expected_canonical)
        .fetch_one(&pool)
        .await
        .unwrap();

    assert_eq!(row.get::<i64, _>("count"), 1);
}

#[tokio::test]
async fn test_workspace_ensure_default_rejects_file_at_default_path() {
    let _home_lock = home_env_lock().lock().unwrap_or_else(|e| e.into_inner());
    let temp_home = tempfile::tempdir().expect("should create temp home");
    let _home_guard = HomeEnvGuard::set(temp_home.path());

    let default_path = temp_home
        .path()
        .join(".tiy")
        .join("workspace")
        .join("Default");
    tokio::fs::create_dir_all(default_path.parent().unwrap())
        .await
        .unwrap();
    tokio::fs::write(&default_path, b"not-a-directory")
        .await
        .unwrap();

    let pool = test_helpers::setup_test_pool().await;
    let manager = WorkspaceManager::new(pool.clone());

    let error = manager.ensure_default_thread_workspace().await.unwrap_err();
    assert_eq!(error.error_code, "workspace.path.not_directory");

    let row = sqlx::query("SELECT COUNT(*) as count FROM workspaces")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(row.get::<i64, _>("count"), 0);
}

#[tokio::test]
async fn test_workspace_find_by_id() {
    let pool = test_helpers::setup_test_pool().await;

    test_helpers::seed_workspace(&pool, "ws-find", "/tmp/findme").await;

    let row = sqlx::query("SELECT id, canonical_path FROM workspaces WHERE id = ?")
        .bind("ws-find")
        .fetch_optional(&pool)
        .await
        .unwrap();

    assert!(row.is_some());
    let row = row.unwrap();
    assert_eq!(row.get::<String, _>("canonical_path"), "/tmp/findme");
}

#[tokio::test]
async fn test_workspace_find_by_canonical_path() {
    let pool = test_helpers::setup_test_pool().await;

    test_helpers::seed_workspace(&pool, "ws-path", "/tmp/by-path").await;

    let row = sqlx::query("SELECT id FROM workspaces WHERE canonical_path = ?")
        .bind("/tmp/by-path")
        .fetch_optional(&pool)
        .await
        .unwrap();

    assert!(row.is_some());
    assert_eq!(row.unwrap().get::<String, _>("id"), "ws-path");
}

#[tokio::test]
async fn test_workspace_delete() {
    let pool = test_helpers::setup_test_pool().await;

    test_helpers::seed_workspace(&pool, "ws-del", "/tmp/delete-me").await;

    let result = sqlx::query("DELETE FROM workspaces WHERE id = ?")
        .bind("ws-del")
        .execute(&pool)
        .await
        .unwrap();

    assert_eq!(result.rows_affected(), 1);

    let row = sqlx::query("SELECT id FROM workspaces WHERE id = ?")
        .bind("ws-del")
        .fetch_optional(&pool)
        .await
        .unwrap();

    assert!(row.is_none());
}

#[tokio::test]
async fn test_workspace_repo_delete_removes_messages_before_runs() {
    let pool = test_helpers::setup_test_pool().await;
    let manager = WorkspaceManager::new(pool.clone());

    test_helpers::seed_workspace(&pool, "ws-del-repo", "/tmp/delete-repo").await;
    test_helpers::seed_thread(&pool, "t-del-repo", "ws-del-repo").await;
    test_helpers::seed_run(&pool, "r-del-repo", "t-del-repo", "completed", "default").await;

    sqlx::query(
        "INSERT INTO messages (
            id, thread_id, run_id, role, content_markdown, message_type, status
         ) VALUES (?, ?, ?, ?, ?, 'plain_message', 'completed')",
    )
    .bind("m-del-repo")
    .bind("t-del-repo")
    .bind("r-del-repo")
    .bind("assistant")
    .bind("cleanup me")
    .execute(&pool)
    .await
    .unwrap();

    manager.remove("ws-del-repo").await.unwrap();

    let workspace = sqlx::query("SELECT id FROM workspaces WHERE id = ?")
        .bind("ws-del-repo")
        .fetch_optional(&pool)
        .await
        .unwrap();
    let thread = sqlx::query("SELECT id FROM threads WHERE id = ?")
        .bind("t-del-repo")
        .fetch_optional(&pool)
        .await
        .unwrap();
    let run = sqlx::query("SELECT id FROM thread_runs WHERE id = ?")
        .bind("r-del-repo")
        .fetch_optional(&pool)
        .await
        .unwrap();
    let message = sqlx::query("SELECT id FROM messages WHERE id = ?")
        .bind("m-del-repo")
        .fetch_optional(&pool)
        .await
        .unwrap();

    assert!(workspace.is_none());
    assert!(thread.is_none());
    assert!(run.is_none());
    assert!(message.is_none());
}

// =========================================================================
// T1.2.2 — Set default workspace
// =========================================================================

#[tokio::test]
async fn test_workspace_set_default() {
    let pool = test_helpers::setup_test_pool().await;

    test_helpers::seed_workspace(&pool, "ws-a", "/tmp/a").await;
    test_helpers::seed_workspace(&pool, "ws-b", "/tmp/b").await;

    // Set ws-a as default
    sqlx::query("UPDATE workspaces SET is_default = 1 WHERE id = 'ws-a'")
        .execute(&pool)
        .await
        .unwrap();

    // Now set ws-b as default (should clear ws-a's default)
    let mut tx = pool.begin().await.unwrap();
    sqlx::query("UPDATE workspaces SET is_default = 0 WHERE is_default = 1")
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query("UPDATE workspaces SET is_default = 1 WHERE id = 'ws-b'")
        .execute(&mut *tx)
        .await
        .unwrap();
    tx.commit().await.unwrap();

    // Verify only ws-b is default
    let defaults = sqlx::query("SELECT id FROM workspaces WHERE is_default = 1")
        .fetch_all(&pool)
        .await
        .unwrap();

    assert_eq!(defaults.len(), 1);
    assert_eq!(defaults[0].get::<String, _>("id"), "ws-b");
}

// =========================================================================
// T1.2.3 — Workspace status update and validation
// =========================================================================

#[tokio::test]
async fn test_workspace_status_update() {
    let pool = test_helpers::setup_test_pool().await;

    test_helpers::seed_workspace(&pool, "ws-status", "/tmp/status-test").await;

    // Update status to "missing"
    sqlx::query("UPDATE workspaces SET status = 'missing', last_validated_at = strftime('%Y-%m-%dT%H:%M:%fZ','now') WHERE id = 'ws-status'")
        .execute(&pool)
        .await
        .unwrap();

    let row = sqlx::query("SELECT status FROM workspaces WHERE id = 'ws-status'")
        .fetch_one(&pool)
        .await
        .unwrap();

    assert_eq!(row.get::<String, _>("status"), "missing");
}

// =========================================================================
// T1.2.4 — Workspace ordering (default first, then by updated_at)
// =========================================================================

#[tokio::test]
async fn test_workspace_list_ordering() {
    let pool = test_helpers::setup_test_pool().await;

    test_helpers::seed_workspace(&pool, "ws-1", "/tmp/first").await;
    test_helpers::seed_workspace(&pool, "ws-2", "/tmp/second").await;

    // Make ws-2 default
    sqlx::query("UPDATE workspaces SET is_default = 1 WHERE id = 'ws-2'")
        .execute(&pool)
        .await
        .unwrap();

    let rows = sqlx::query("SELECT id FROM workspaces ORDER BY is_default DESC, updated_at DESC")
        .fetch_all(&pool)
        .await
        .unwrap();

    // Default workspace should come first
    assert_eq!(rows[0].get::<String, _>("id"), "ws-2");
}
