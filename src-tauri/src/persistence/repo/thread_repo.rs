use chrono::Utc;
use sqlx::SqlitePool;

use crate::model::errors::AppError;
use crate::model::thread::{ThreadRecord, ThreadStatus};

#[derive(sqlx::FromRow)]
struct ThreadRow {
    id: String,
    workspace_id: String,
    title: String,
    status: String,
    summary: Option<String>,
    last_active_at: String,
    created_at: String,
    updated_at: String,
}

impl ThreadRow {
    fn into_record(self) -> ThreadRecord {
        ThreadRecord {
            id: self.id,
            workspace_id: self.workspace_id,
            title: self.title,
            status: ThreadStatus::from_str(&self.status),
            summary: self.summary,
            last_active_at: self.last_active_at,
            created_at: self.created_at,
            updated_at: self.updated_at,
        }
    }
}

pub async fn list_by_workspace(
    pool: &SqlitePool,
    workspace_id: &str,
    limit: i64,
    offset: i64,
) -> Result<Vec<ThreadRecord>, AppError> {
    let rows = sqlx::query_as::<_, ThreadRow>(
        "SELECT id, workspace_id, title, status, summary, last_active_at, created_at, updated_at
         FROM threads
         WHERE workspace_id = ?
         ORDER BY last_active_at DESC
         LIMIT ? OFFSET ?",
    )
    .bind(workspace_id)
    .bind(limit)
    .bind(offset)
    .fetch_all(pool)
    .await?;

    Ok(rows.into_iter().map(|r| r.into_record()).collect())
}

pub async fn find_by_id(pool: &SqlitePool, id: &str) -> Result<Option<ThreadRecord>, AppError> {
    let row = sqlx::query_as::<_, ThreadRow>(
        "SELECT id, workspace_id, title, status, summary, last_active_at, created_at, updated_at
         FROM threads WHERE id = ?",
    )
    .bind(id)
    .fetch_optional(pool)
    .await?;

    Ok(row.map(|r| r.into_record()))
}

pub async fn insert(pool: &SqlitePool, record: &ThreadRecord) -> Result<(), AppError> {
    let now = Utc::now().to_rfc3339();
    sqlx::query(
        "INSERT INTO threads (id, workspace_id, title, status, summary, last_active_at, created_at, updated_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&record.id)
    .bind(&record.workspace_id)
    .bind(&record.title)
    .bind(record.status.as_str())
    .bind(&record.summary)
    .bind(&now)
    .bind(&now)
    .bind(&now)
    .execute(pool)
    .await?;

    Ok(())
}

pub async fn update_title(pool: &SqlitePool, id: &str, title: &str) -> Result<(), AppError> {
    let now = Utc::now().to_rfc3339();
    sqlx::query("UPDATE threads SET title = ?, updated_at = ? WHERE id = ?")
        .bind(title)
        .bind(&now)
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn update_status(
    pool: &SqlitePool,
    id: &str,
    status: &ThreadStatus,
) -> Result<(), AppError> {
    let now = Utc::now().to_rfc3339();
    sqlx::query("UPDATE threads SET status = ?, updated_at = ? WHERE id = ?")
        .bind(status.as_str())
        .bind(&now)
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn touch_active(pool: &SqlitePool, id: &str) -> Result<(), AppError> {
    let now = Utc::now().to_rfc3339();
    sqlx::query("UPDATE threads SET last_active_at = ?, updated_at = ? WHERE id = ?")
        .bind(&now)
        .bind(&now)
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn delete(pool: &SqlitePool, id: &str) -> Result<bool, AppError> {
    // Cascade delete all related records in dependency order.
    let mut tx = pool.begin().await?;

    sqlx::query("DELETE FROM audit_events WHERE thread_id = ?")
        .bind(id)
        .execute(&mut *tx)
        .await?;

    sqlx::query("DELETE FROM tool_calls WHERE thread_id = ?")
        .bind(id)
        .execute(&mut *tx)
        .await?;

    sqlx::query("DELETE FROM run_subtasks WHERE thread_id = ?")
        .bind(id)
        .execute(&mut *tx)
        .await?;

    sqlx::query("DELETE FROM run_helpers WHERE thread_id = ?")
        .bind(id)
        .execute(&mut *tx)
        .await?;

    sqlx::query("DELETE FROM messages WHERE thread_id = ?")
        .bind(id)
        .execute(&mut *tx)
        .await?;

    sqlx::query("DELETE FROM thread_summaries WHERE thread_id = ?")
        .bind(id)
        .execute(&mut *tx)
        .await?;

    sqlx::query("DELETE FROM terminal_sessions WHERE thread_id = ?")
        .bind(id)
        .execute(&mut *tx)
        .await?;

    sqlx::query("DELETE FROM thread_runs WHERE thread_id = ?")
        .bind(id)
        .execute(&mut *tx)
        .await?;

    let result = sqlx::query("DELETE FROM threads WHERE id = ?")
        .bind(id)
        .execute(&mut *tx)
        .await?;

    tx.commit().await?;
    Ok(result.rows_affected() > 0)
}
