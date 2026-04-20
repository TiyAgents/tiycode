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
