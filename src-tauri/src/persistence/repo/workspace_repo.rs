use chrono::{DateTime, Utc};
use sqlx::SqlitePool;

use crate::model::errors::{AppError, ErrorSource};
use crate::model::workspace::{WorkspaceRecord, WorkspaceStatus};

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
        }
    }
}

pub async fn list_all(pool: &SqlitePool) -> Result<Vec<WorkspaceRecord>, AppError> {
    let rows = sqlx::query_as::<_, WorkspaceRow>(
        "SELECT id, name, path, canonical_path, display_path, is_default, is_git,
                auto_work_tree, status, last_validated_at, created_at, updated_at
         FROM workspaces
         ORDER BY is_default DESC, updated_at DESC",
    )
    .fetch_all(pool)
    .await?;

    Ok(rows.into_iter().map(|r| r.into_record()).collect())
}

pub async fn find_by_id(pool: &SqlitePool, id: &str) -> Result<Option<WorkspaceRecord>, AppError> {
    let row = sqlx::query_as::<_, WorkspaceRow>(
        "SELECT id, name, path, canonical_path, display_path, is_default, is_git,
                auto_work_tree, status, last_validated_at, created_at, updated_at
         FROM workspaces WHERE id = ?",
    )
    .bind(id)
    .fetch_optional(pool)
    .await?;

    Ok(row.map(|r| r.into_record()))
}

pub async fn find_by_canonical_path(
    pool: &SqlitePool,
    canonical_path: &str,
) -> Result<Option<WorkspaceRecord>, AppError> {
    let row = sqlx::query_as::<_, WorkspaceRow>(
        "SELECT id, name, path, canonical_path, display_path, is_default, is_git,
                auto_work_tree, status, last_validated_at, created_at, updated_at
         FROM workspaces WHERE canonical_path = ?",
    )
    .bind(canonical_path)
    .fetch_optional(pool)
    .await?;

    Ok(row.map(|r| r.into_record()))
}

pub async fn insert(pool: &SqlitePool, record: &WorkspaceRecord) -> Result<(), AppError> {
    let now = Utc::now().to_rfc3339();
    sqlx::query(
        "INSERT INTO workspaces (id, name, path, canonical_path, display_path,
                is_default, is_git, auto_work_tree, status, last_validated_at,
                created_at, updated_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
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
    .execute(pool)
    .await?;

    Ok(())
}

pub async fn delete(pool: &SqlitePool, id: &str) -> Result<bool, AppError> {
    let result = sqlx::query("DELETE FROM workspaces WHERE id = ?")
        .bind(id)
        .execute(pool)
        .await?;

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

pub async fn set_default(pool: &SqlitePool, id: &str) -> Result<(), AppError> {
    let now = Utc::now().to_rfc3339();

    // Clear all defaults first, then set the new one — in a transaction.
    let mut tx = pool.begin().await?;

    sqlx::query("UPDATE workspaces SET is_default = 0, updated_at = ? WHERE is_default = 1")
        .bind(&now)
        .execute(&mut *tx)
        .await?;

    let result =
        sqlx::query("UPDATE workspaces SET is_default = 1, updated_at = ? WHERE id = ?")
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
