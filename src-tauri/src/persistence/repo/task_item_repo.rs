use chrono::Utc;
use sqlx::SqlitePool;

use crate::model::errors::AppError;
use crate::model::task_item::{TaskItemDto, TaskItemRecord, TaskStage};

#[derive(sqlx::FromRow)]
struct TaskItemRow {
    id: String,
    task_board_id: String,
    description: String,
    stage: String,
    sort_order: i32,
    error_detail: Option<String>,
    created_at: String,
    updated_at: String,
}

impl TaskItemRow {
    fn into_record(self) -> TaskItemRecord {
        TaskItemRecord {
            id: self.id,
            task_board_id: self.task_board_id,
            description: self.description,
            stage: TaskStage::from_db_str(&self.stage),
            sort_order: self.sort_order,
            error_detail: self.error_detail,
            created_at: self.created_at,
            updated_at: self.updated_at,
        }
    }
}

pub async fn list_by_task_board(
    pool: &SqlitePool,
    task_board_id: &str,
) -> Result<Vec<TaskItemRecord>, AppError> {
    let rows = sqlx::query_as::<_, TaskItemRow>(
        "SELECT id, task_board_id, description, stage, sort_order, error_detail, created_at, updated_at
         FROM task_items
         WHERE task_board_id = ?
         ORDER BY sort_order ASC",
    )
    .bind(task_board_id)
    .fetch_all(pool)
    .await?;

    Ok(rows.into_iter().map(|r| r.into_record()).collect())
}

pub async fn find_by_id(pool: &SqlitePool, id: &str) -> Result<Option<TaskItemRecord>, AppError> {
    let row = sqlx::query_as::<_, TaskItemRow>(
        "SELECT id, task_board_id, description, stage, sort_order, error_detail, created_at, updated_at
         FROM task_items WHERE id = ?",
    )
    .bind(id)
    .fetch_optional(pool)
    .await?;

    Ok(row.map(|r| r.into_record()))
}

pub async fn update_stage(
    pool: &SqlitePool,
    id: &str,
    stage: &TaskStage,
    error_detail: Option<&str>,
) -> Result<(), AppError> {
    let now = Utc::now().to_rfc3339();
    sqlx::query("UPDATE task_items SET stage = ?, error_detail = ?, updated_at = ? WHERE id = ?")
        .bind(stage.as_str())
        .bind(error_detail)
        .bind(&now)
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}

/// Load all task items for multiple task boards and return as DTOs grouped by task_board_id.
pub async fn list_dtos_by_task_boards(
    pool: &SqlitePool,
    task_board_ids: &[String],
) -> Result<Vec<TaskItemDto>, AppError> {
    if task_board_ids.is_empty() {
        return Ok(Vec::new());
    }

    // Build placeholders for IN clause
    let placeholders: Vec<String> = task_board_ids.iter().map(|_| "?".to_string()).collect();
    let query_str = format!(
        "SELECT id, task_board_id, description, stage, sort_order, error_detail, created_at, updated_at
         FROM task_items
         WHERE task_board_id IN ({})
         ORDER BY sort_order ASC",
        placeholders.join(", ")
    );

    let mut query = sqlx::query_as::<_, TaskItemRow>(&query_str);
    for id in task_board_ids {
        query = query.bind(id);
    }

    let rows = query.fetch_all(pool).await?;
    Ok(rows.into_iter().map(|r| r.into_record().into()).collect())
}
