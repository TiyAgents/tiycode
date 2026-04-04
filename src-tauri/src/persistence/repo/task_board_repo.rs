use chrono::Utc;
use sqlx::SqlitePool;

use crate::model::errors::AppError;
use crate::model::task_board::{TaskBoardRecord, TaskBoardStatus};

#[derive(sqlx::FromRow)]
struct TaskBoardRow {
    id: String,
    thread_id: String,
    title: String,
    status: String,
    active_task_id: Option<String>,
    created_at: String,
    updated_at: String,
}

impl TaskBoardRow {
    fn into_record(self) -> TaskBoardRecord {
        TaskBoardRecord {
            id: self.id,
            thread_id: self.thread_id,
            title: self.title,
            status: TaskBoardStatus::from_db_str(&self.status),
            active_task_id: self.active_task_id,
            created_at: self.created_at,
            updated_at: self.updated_at,
        }
    }
}

pub async fn list_by_thread(
    pool: &SqlitePool,
    thread_id: &str,
) -> Result<Vec<TaskBoardRecord>, AppError> {
    let rows = sqlx::query_as::<_, TaskBoardRow>(
        "SELECT id, thread_id, title, status, active_task_id, created_at, updated_at
         FROM task_boards
         WHERE thread_id = ?
         ORDER BY created_at ASC",
    )
    .bind(thread_id)
    .fetch_all(pool)
    .await?;

    Ok(rows.into_iter().map(|r| r.into_record()).collect())
}

pub async fn find_by_id(pool: &SqlitePool, id: &str) -> Result<Option<TaskBoardRecord>, AppError> {
    let row = sqlx::query_as::<_, TaskBoardRow>(
        "SELECT id, thread_id, title, status, active_task_id, created_at, updated_at
         FROM task_boards WHERE id = ?",
    )
    .bind(id)
    .fetch_optional(pool)
    .await?;

    Ok(row.map(|r| r.into_record()))
}

pub async fn find_active_by_thread(
    pool: &SqlitePool,
    thread_id: &str,
) -> Result<Option<TaskBoardRecord>, AppError> {
    let row = sqlx::query_as::<_, TaskBoardRow>(
        "SELECT id, thread_id, title, status, active_task_id, created_at, updated_at
         FROM task_boards WHERE thread_id = ? AND status = 'active'
         ORDER BY created_at DESC LIMIT 1",
    )
    .bind(thread_id)
    .fetch_optional(pool)
    .await?;

    Ok(row.map(|r| r.into_record()))
}

pub async fn update_status(
    pool: &SqlitePool,
    id: &str,
    status: &TaskBoardStatus,
) -> Result<(), AppError> {
    let now = Utc::now().to_rfc3339();
    sqlx::query("UPDATE task_boards SET status = ?, updated_at = ? WHERE id = ?")
        .bind(status.as_str())
        .bind(&now)
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn update_active_task(
    pool: &SqlitePool,
    id: &str,
    active_task_id: Option<&str>,
) -> Result<(), AppError> {
    let now = Utc::now().to_rfc3339();
    sqlx::query("UPDATE task_boards SET active_task_id = ?, updated_at = ? WHERE id = ?")
        .bind(active_task_id)
        .bind(&now)
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}
