use chrono::{DateTime, Utc};
use sqlx::SqlitePool;

use crate::model::errors::{AppError, ErrorSource};
use crate::model::workspace::{WorkspaceKind, WorkspaceRecord, WorkspaceStatus};

/// Row returned by sqlx queries (intermediate mapping).
#[derive(sqlx::FromRow)]
struct WorkspaceRow {
    id: String,
    name: String,
    path: String,
    canonical_path: String,
    display_path: String,
    is_default: i32,
    is_git: i32,
    auto_work_tree: i32,
    status: String,
    last_validated_at: Option<String>,
    created_at: String,
    updated_at: String,
    kind: String,
    parent_workspace_id: Option<String>,
    git_common_dir: Option<String>,
    branch: Option<String>,
    worktree_name: Option<String>,
}

impl WorkspaceRow {
    fn into_record(self) -> WorkspaceRecord {
        WorkspaceRecord {
            id: self.id,
            name: self.name,
            path: self.path,
            canonical_path: self.canonical_path,
            display_path: self.display_path,
            is_default: self.is_default != 0,
            is_git: self.is_git != 0,
            auto_work_tree: self.auto_work_tree != 0,
            status: WorkspaceStatus::from_str(&self.status),
            last_validated_at: self
                .last_validated_at
                .and_then(|s| DateTime::parse_from_rfc3339(&s).ok())
                .map(|dt| dt.with_timezone(&Utc)),
            created_at: DateTime::parse_from_rfc3339(&self.created_at)
                .map(|dt| dt.with_timezone(&Utc))
                .unwrap_or_else(|_| Utc::now()),
            updated_at: DateTime::parse_from_rfc3339(&self.updated_at)
                .map(|dt| dt.with_timezone(&Utc))
                .unwrap_or_else(|_| Utc::now()),
            kind: WorkspaceKind::from_str(&self.kind),
            parent_workspace_id: self.parent_workspace_id,
            git_common_dir: self.git_common_dir,
            branch: self.branch,
            worktree_name: self.worktree_name,
        }
    }
}

const SELECT_COLUMNS: &str = "id, name, path, canonical_path, display_path, is_default, is_git,\n                auto_work_tree, status, last_validated_at, created_at, updated_at,\n                kind, parent_workspace_id, git_common_dir, branch, worktree_name";

pub async fn list_all(pool: &SqlitePool) -> Result<Vec<WorkspaceRecord>, AppError> {
    let rows = sqlx::query_as::<_, WorkspaceRow>(&format!(
        "SELECT {SELECT_COLUMNS}
         FROM workspaces
         ORDER BY is_default DESC, name ASC, kind ASC, created_at DESC"
    ))
    .fetch_all(pool)
    .await?;

    Ok(rows.into_iter().map(|r| r.into_record()).collect())
}

pub async fn find_by_id(pool: &SqlitePool, id: &str) -> Result<Option<WorkspaceRecord>, AppError> {
    let row = sqlx::query_as::<_, WorkspaceRow>(&format!(
        "SELECT {SELECT_COLUMNS} FROM workspaces WHERE id = ?"
    ))
    .bind(id)
    .fetch_optional(pool)
    .await?;

    Ok(row.map(|r| r.into_record()))
}

pub async fn find_by_canonical_path(
    pool: &SqlitePool,
    canonical_path: &str,
) -> Result<Option<WorkspaceRecord>, AppError> {
    let row = sqlx::query_as::<_, WorkspaceRow>(&format!(
        "SELECT {SELECT_COLUMNS} FROM workspaces WHERE canonical_path = ?"
    ))
    .bind(canonical_path)
    .fetch_optional(pool)
    .await?;

    Ok(row.map(|r| r.into_record()))
}

/// List worktree workspaces attached to the given parent repo workspace.
pub async fn list_worktrees_of(
    pool: &SqlitePool,
    parent_id: &str,
) -> Result<Vec<WorkspaceRecord>, AppError> {
    let rows = sqlx::query_as::<_, WorkspaceRow>(&format!(
        "SELECT {SELECT_COLUMNS}
         FROM workspaces
         WHERE parent_workspace_id = ? AND kind = 'worktree'
         ORDER BY created_at ASC"
    ))
    .bind(parent_id)
    .fetch_all(pool)
    .await?;

    Ok(rows.into_iter().map(|r| r.into_record()).collect())
}

pub async fn insert(pool: &SqlitePool, record: &WorkspaceRecord) -> Result<(), AppError> {
    let now = Utc::now().to_rfc3339();
    sqlx::query(
        "INSERT INTO workspaces (id, name, path, canonical_path, display_path,
                is_default, is_git, auto_work_tree, status, last_validated_at,
                created_at, updated_at,
                kind, parent_workspace_id, git_common_dir, branch, worktree_name)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&record.id)
    .bind(&record.name)
    .bind(&record.path)
    .bind(&record.canonical_path)
    .bind(&record.display_path)
    .bind(record.is_default as i32)
    .bind(record.is_git as i32)
    .bind(record.auto_work_tree as i32)
    .bind(record.status.as_str())
    .bind(record.last_validated_at.map(|t| t.to_rfc3339()))
    .bind(&now)
    .bind(&now)
    .bind(record.kind.as_str())
    .bind(record.parent_workspace_id.as_deref())
    .bind(record.git_common_dir.as_deref())
    .bind(record.branch.as_deref())
    .bind(record.worktree_name.as_deref())
    .execute(pool)
    .await?;

    Ok(())
}

pub async fn delete(pool: &SqlitePool, id: &str) -> Result<bool, AppError> {
    let mut tx = pool.begin().await?;

    sqlx::query(
        "DELETE FROM audit_events
         WHERE workspace_id = ?
            OR thread_id IN (SELECT id FROM threads WHERE workspace_id = ?)",
    )
    .bind(id)
    .bind(id)
    .execute(&mut *tx)
    .await?;

    sqlx::query(
        "DELETE FROM tool_calls
         WHERE thread_id IN (SELECT id FROM threads WHERE workspace_id = ?)",
    )
    .bind(id)
    .execute(&mut *tx)
    .await?;

    sqlx::query(
        "DELETE FROM run_subtasks
         WHERE thread_id IN (SELECT id FROM threads WHERE workspace_id = ?)",
    )
    .bind(id)
    .execute(&mut *tx)
    .await?;

    sqlx::query(
        "DELETE FROM run_helpers
         WHERE thread_id IN (SELECT id FROM threads WHERE workspace_id = ?)",
    )
    .bind(id)
    .execute(&mut *tx)
    .await?;

    sqlx::query(
        "DELETE FROM thread_summaries
         WHERE thread_id IN (SELECT id FROM threads WHERE workspace_id = ?)",
    )
    .bind(id)
    .execute(&mut *tx)
    .await?;

    sqlx::query(
        "DELETE FROM messages
         WHERE thread_id IN (SELECT id FROM threads WHERE workspace_id = ?)",
    )
    .bind(id)
    .execute(&mut *tx)
    .await?;

    sqlx::query(
        "DELETE FROM thread_runs
         WHERE thread_id IN (SELECT id FROM threads WHERE workspace_id = ?)",
    )
    .bind(id)
    .execute(&mut *tx)
    .await?;

    sqlx::query(
        "DELETE FROM terminal_sessions
         WHERE workspace_id = ?
            OR thread_id IN (SELECT id FROM threads WHERE workspace_id = ?)",
    )
    .bind(id)
    .bind(id)
    .execute(&mut *tx)
    .await?;

    sqlx::query(
        "DELETE FROM task_items
         WHERE task_board_id IN (
             SELECT id FROM task_boards
             WHERE thread_id IN (SELECT id FROM threads WHERE workspace_id = ?)
         )",
    )
    .bind(id)
    .execute(&mut *tx)
    .await?;

    sqlx::query(
        "DELETE FROM task_boards
         WHERE thread_id IN (SELECT id FROM threads WHERE workspace_id = ?)",
    )
    .bind(id)
    .execute(&mut *tx)
    .await?;

    sqlx::query("DELETE FROM threads WHERE workspace_id = ?")
        .bind(id)
        .execute(&mut *tx)
        .await?;

    let result = sqlx::query("DELETE FROM workspaces WHERE id = ?")
        .bind(id)
        .execute(&mut *tx)
        .await?;

    tx.commit().await?;

    Ok(result.rows_affected() > 0)
}

pub async fn update_status(
    pool: &SqlitePool,
    id: &str,
    status: &WorkspaceStatus,
    validated_at: DateTime<Utc>,
) -> Result<(), AppError> {
    let now = Utc::now().to_rfc3339();
    sqlx::query(
        "UPDATE workspaces SET status = ?, last_validated_at = ?, updated_at = ? WHERE id = ?",
    )
    .bind(status.as_str())
    .bind(validated_at.to_rfc3339())
    .bind(&now)
    .bind(id)
    .execute(pool)
    .await?;

    Ok(())
}

pub async fn update_is_git(pool: &SqlitePool, id: &str, is_git: bool) -> Result<(), AppError> {
    let now = Utc::now().to_rfc3339();
    sqlx::query("UPDATE workspaces SET is_git = ?, updated_at = ? WHERE id = ?")
        .bind(is_git as i32)
        .bind(&now)
        .bind(id)
        .execute(pool)
        .await?;

    Ok(())
}

/// Update the workspace `kind` (and optional related fields). Used when
/// upgrading an existing `standalone` workspace to `repo` after Git detection,
/// or when re-synchronizing worktree metadata after a git operation.
pub async fn update_kind_metadata(
    pool: &SqlitePool,
    id: &str,
    kind: WorkspaceKind,
    parent_workspace_id: Option<&str>,
    worktree_name: Option<&str>,
    branch: Option<&str>,
    git_common_dir: Option<&str>,
) -> Result<(), AppError> {
    let now = Utc::now().to_rfc3339();
    sqlx::query(
        "UPDATE workspaces
         SET kind = ?, parent_workspace_id = ?, worktree_name = ?,
             branch = ?, git_common_dir = ?, updated_at = ?
         WHERE id = ?",
    )
    .bind(kind.as_str())
    .bind(parent_workspace_id)
    .bind(worktree_name)
    .bind(branch)
    .bind(git_common_dir)
    .bind(&now)
    .bind(id)
    .execute(pool)
    .await?;

    Ok(())
}

pub async fn set_default(pool: &SqlitePool, id: &str) -> Result<(), AppError> {
    let mut tx = pool.begin().await?;
    let now = Utc::now().to_rfc3339();

    sqlx::query("UPDATE workspaces SET is_default = 0, updated_at = ? WHERE is_default = 1")
        .bind(&now)
        .execute(&mut *tx)
        .await?;

    let result = sqlx::query("UPDATE workspaces SET is_default = 1, updated_at = ? WHERE id = ?")
        .bind(&now)
        .bind(id)
        .execute(&mut *tx)
        .await?;

    if result.rows_affected() == 0 {
        return Err(AppError::not_found(ErrorSource::Workspace, "workspace"));
    }

    tx.commit().await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;
    use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
    use std::str::FromStr;

    async fn setup_test_pool() -> SqlitePool {
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

    fn make_ws(
        id: &str,
        name: &str,
        path: &str,
        is_default: bool,
        kind: WorkspaceKind,
    ) -> WorkspaceRecord {
        WorkspaceRecord {
            id: id.into(),
            name: name.into(),
            path: path.into(),
            canonical_path: path.into(),
            display_path: path.into(),
            is_default,
            is_git: false,
            auto_work_tree: false,
            status: WorkspaceStatus::Ready,
            last_validated_at: Some(Utc::now()),
            created_at: Utc::now(),
            updated_at: Utc::now(),
            kind,
            parent_workspace_id: None,
            git_common_dir: None,
            branch: None,
            worktree_name: None,
        }
    }

    #[tokio::test]
    async fn list_all_returns_workspaces_ordered() {
        let pool = setup_test_pool().await;
        insert(
            &pool,
            &make_ws("ws-2", "Beta", "/beta", false, WorkspaceKind::Repo),
        )
        .await
        .unwrap();
        insert(
            &pool,
            &make_ws("ws-1", "Alpha", "/alpha", true, WorkspaceKind::Repo),
        )
        .await
        .unwrap();
        insert(
            &pool,
            &make_ws("ws-3", "Alpha", "/alpha2", false, WorkspaceKind::Standalone),
        )
        .await
        .unwrap();

        let list = list_all(&pool).await.unwrap();
        assert_eq!(list.len(), 3);
        // Default first
        assert_eq!(list[0].id, "ws-1");
    }

    #[tokio::test]
    async fn find_by_id_returns_workspace() {
        let pool = setup_test_pool().await;
        insert(
            &pool,
            &make_ws("ws-1", "Test", "/test", false, WorkspaceKind::Repo),
        )
        .await
        .unwrap();

        let found = find_by_id(&pool, "ws-1")
            .await
            .unwrap()
            .expect("should exist");
        assert_eq!(found.name, "Test");
        assert_eq!(found.kind, WorkspaceKind::Repo);

        let missing = find_by_id(&pool, "nonexistent").await.unwrap();
        assert!(missing.is_none());
    }

    #[tokio::test]
    async fn find_by_canonical_path_matches_path() {
        let pool = setup_test_pool().await;
        insert(
            &pool,
            &make_ws(
                "ws-1",
                "Test",
                "/home/user/project",
                false,
                WorkspaceKind::Repo,
            ),
        )
        .await
        .unwrap();

        let found = find_by_canonical_path(&pool, "/home/user/project")
            .await
            .unwrap()
            .expect("should exist");
        assert_eq!(found.id, "ws-1");

        let missing = find_by_canonical_path(&pool, "/other").await.unwrap();
        assert!(missing.is_none());
    }

    #[tokio::test]
    async fn list_worktrees_of_returns_only_worktrees() {
        let pool = setup_test_pool().await;
        insert(
            &pool,
            &make_ws("parent", "Parent", "/parent", false, WorkspaceKind::Repo),
        )
        .await
        .unwrap();

        let mut wt1 = make_ws("wt-1", "WT1", "/wt1", false, WorkspaceKind::Worktree);
        wt1.parent_workspace_id = Some("parent".into());
        insert(&pool, &wt1).await.unwrap();

        let mut wt2 = make_ws("wt-2", "WT2", "/wt2", false, WorkspaceKind::Worktree);
        wt2.parent_workspace_id = Some("parent".into());
        insert(&pool, &wt2).await.unwrap();

        let worktrees = list_worktrees_of(&pool, "parent").await.unwrap();
        assert_eq!(worktrees.len(), 2);
        assert!(worktrees.iter().all(|w| w.kind == WorkspaceKind::Worktree));
    }

    #[tokio::test]
    async fn delete_removes_workspace_and_cascades() {
        let pool = setup_test_pool().await;
        insert(
            &pool,
            &make_ws("ws-1", "Test", "/test", false, WorkspaceKind::Repo),
        )
        .await
        .unwrap();

        // Seed child records so we can verify cascade deletion
        sqlx::query(
            "INSERT INTO threads (id, workspace_id, title, status, created_at, updated_at, last_active_at)
             VALUES ('t-1', 'ws-1', 'Thread', 'idle',
                     strftime('%Y-%m-%dT%H:%M:%fZ', 'now'),
                     strftime('%Y-%m-%dT%H:%M:%fZ', 'now'),
                     strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))",
        )
        .execute(&pool)
        .await
        .unwrap();

        sqlx::query(
            "INSERT INTO terminal_sessions (id, thread_id, workspace_id, status, created_at)
             VALUES ('term-1', 't-1', 'ws-1', 'running',
                     strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))",
        )
        .execute(&pool)
        .await
        .unwrap();

        let deleted = delete(&pool, "ws-1").await.unwrap();
        assert!(deleted);

        // Workspace itself should be gone
        let result = find_by_id(&pool, "ws-1").await.unwrap();
        assert!(result.is_none());

        // Child records should have been cascade-deleted
        let thread_count: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM threads WHERE id = 't-1'")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(
            thread_count.0, 0,
            "thread should be deleted along with workspace"
        );

        let session_count: (i64,) =
            sqlx::query_as("SELECT COUNT(*) FROM terminal_sessions WHERE id = 'term-1'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(
            session_count.0, 0,
            "terminal session should be deleted along with workspace"
        );

        // Non-existent should return false
        let deleted = delete(&pool, "ws-1").await.unwrap();
        assert!(!deleted);
    }

    #[tokio::test]
    async fn update_status_changes_status_and_validated_at() {
        let pool = setup_test_pool().await;
        insert(
            &pool,
            &make_ws("ws-1", "Test", "/test", false, WorkspaceKind::Repo),
        )
        .await
        .unwrap();

        let validated_at = Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap();
        update_status(&pool, "ws-1", &WorkspaceStatus::Missing, validated_at)
            .await
            .unwrap();

        let ws = find_by_id(&pool, "ws-1")
            .await
            .unwrap()
            .expect("should exist");
        assert_eq!(ws.status, WorkspaceStatus::Missing);
        assert!(ws.last_validated_at.is_some());
    }

    #[tokio::test]
    async fn update_is_git_flips_flag() {
        let pool = setup_test_pool().await;
        insert(
            &pool,
            &make_ws("ws-1", "Test", "/test", false, WorkspaceKind::Repo),
        )
        .await
        .unwrap();

        update_is_git(&pool, "ws-1", true).await.unwrap();
        let ws = find_by_id(&pool, "ws-1")
            .await
            .unwrap()
            .expect("should exist");
        assert!(ws.is_git);

        update_is_git(&pool, "ws-1", false).await.unwrap();
        let ws = find_by_id(&pool, "ws-1")
            .await
            .unwrap()
            .expect("should exist");
        assert!(!ws.is_git);
    }

    #[tokio::test]
    async fn update_kind_metadata_sets_fields() {
        let pool = setup_test_pool().await;
        insert(
            &pool,
            &make_ws("ws-1", "Test", "/test", false, WorkspaceKind::Standalone),
        )
        .await
        .unwrap();

        update_kind_metadata(
            &pool,
            "ws-1",
            WorkspaceKind::Repo,
            None,
            Some("abcdef-fix"),
            Some("main"),
            Some("/common"),
        )
        .await
        .unwrap();

        let ws = find_by_id(&pool, "ws-1")
            .await
            .unwrap()
            .expect("should exist");
        assert_eq!(ws.kind, WorkspaceKind::Repo);
        assert_eq!(ws.worktree_name, Some("abcdef-fix".into()));
        assert_eq!(ws.branch, Some("main".into()));
        assert_eq!(ws.git_common_dir, Some("/common".into()));
    }

    #[tokio::test]
    async fn set_default_marks_workspace_as_default() {
        let pool = setup_test_pool().await;
        insert(
            &pool,
            &make_ws("ws-1", "First", "/first", true, WorkspaceKind::Repo),
        )
        .await
        .unwrap();
        insert(
            &pool,
            &make_ws("ws-2", "Second", "/second", false, WorkspaceKind::Repo),
        )
        .await
        .unwrap();

        set_default(&pool, "ws-2").await.unwrap();

        let ws1 = find_by_id(&pool, "ws-1")
            .await
            .unwrap()
            .expect("should exist");
        assert!(!ws1.is_default);

        let ws2 = find_by_id(&pool, "ws-2")
            .await
            .unwrap()
            .expect("should exist");
        assert!(ws2.is_default);
    }

    #[tokio::test]
    async fn set_default_errors_for_nonexistent_workspace() {
        let pool = setup_test_pool().await;
        let err = set_default(&pool, "nowhere").await.unwrap_err();
        assert!(err.to_string().contains("workspace"));
    }
}
