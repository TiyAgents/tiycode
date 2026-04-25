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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::task_board::TaskBoardRecord;
    use crate::model::task_board::TaskBoardStatus;
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

        // Seed FK chain: workspaces → threads → task_boards
        sqlx::query(
            "INSERT INTO workspaces (id, name, path, canonical_path, display_path,
                    is_default, is_git, auto_work_tree, status, created_at, updated_at)
             VALUES ('ws-1', 'ws', '/tmp', '/tmp', '/tmp', 0, 0, 0, 'ready',
                     strftime('%Y-%m-%dT%H:%M:%fZ', 'now'),
                     strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))",
        )
        .execute(&pool)
        .await
        .expect("seed workspace");

        sqlx::query(
            "INSERT INTO threads (id, workspace_id, title, status, created_at, updated_at, last_active_at)
             VALUES ('t1', 'ws-1', 't', 'idle',
                     strftime('%Y-%m-%dT%H:%M:%fZ', 'now'),
                     strftime('%Y-%m-%dT%H:%M:%fZ', 'now'),
                     strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))",
        )
        .execute(&pool)
        .await
        .expect("seed thread");

        sqlx::query(
            "INSERT INTO task_boards (id, thread_id, title, status, created_at, updated_at)
             VALUES ('tb-1', 't1', 'Board 1', 'active',
                     strftime('%Y-%m-%dT%H:%M:%fZ', 'now'),
                     strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))",
        )
        .execute(&pool)
        .await
        .expect("seed task_board");

        sqlx::query(
            "INSERT INTO task_boards (id, thread_id, title, status, created_at, updated_at)
             VALUES ('tb-2', 't1', 'Board 2', 'completed',
                     strftime('%Y-%m-%dT%H:%M:%fZ', 'now'),
                     strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))",
        )
        .execute(&pool)
        .await
        .expect("seed task_board 2");

        pool
    }

    async fn seed_item(pool: &SqlitePool, id: &str, board_id: &str, desc: &str, sort: i32) {
        sqlx::query(
            "INSERT INTO task_items (id, task_board_id, description, stage, sort_order, created_at, updated_at)
             VALUES (?, ?, ?, 'pending', ?, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'), strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))",
        )
        .bind(id)
        .bind(board_id)
        .bind(desc)
        .bind(sort)
        .execute(pool)
        .await
        .expect("seed task_item");
    }

    #[tokio::test]
    async fn list_by_task_board_orders_by_sort_order() {
        let pool = setup_test_pool().await;
        seed_item(&pool, "ti-2", "tb-1", "Item 2", 2).await;
        seed_item(&pool, "ti-1", "tb-1", "Item 1", 1).await;

        let items = list_by_task_board(&pool, "tb-1").await.unwrap();
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].id, "ti-1");
        assert_eq!(items[1].id, "ti-2");
    }

    #[tokio::test]
    async fn find_by_id_returns_none_for_missing() {
        let pool = setup_test_pool().await;
        assert!(find_by_id(&pool, "nope").await.unwrap().is_none());
    }

    #[tokio::test]
    async fn update_stage_sets_error_detail() {
        let pool = setup_test_pool().await;
        seed_item(&pool, "ti-1", "tb-1", "Item", 1).await;

        update_stage(&pool, "ti-1", &TaskStage::Failed, Some("oops"))
            .await
            .unwrap();

        let item = find_by_id(&pool, "ti-1")
            .await
            .unwrap()
            .expect("should exist");
        assert_eq!(item.stage, TaskStage::Failed);
        assert_eq!(item.error_detail, Some("oops".into()));
    }

    #[tokio::test]
    async fn list_dtos_by_task_boards_returns_empty_for_empty_input() {
        let pool = setup_test_pool().await;
        let dtos = list_dtos_by_task_boards(&pool, &[]).await.unwrap();
        assert!(dtos.is_empty());
    }

    #[tokio::test]
    async fn list_dtos_by_task_boards_filters_by_board_ids() {
        let pool = setup_test_pool().await;
        seed_item(&pool, "ti-1", "tb-1", "For tb-1", 1).await;
        seed_item(&pool, "ti-2", "tb-2", "For tb-2", 1).await;

        let dtos = list_dtos_by_task_boards(&pool, &["tb-1".into()])
            .await
            .unwrap();
        assert_eq!(dtos.len(), 1);
        assert_eq!(dtos[0].id, "ti-1");
    }
}
