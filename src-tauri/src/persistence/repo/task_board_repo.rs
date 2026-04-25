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

#[cfg(test)]
mod tests {
    use super::*;
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

        pool
    }

    async fn seed_board(pool: &SqlitePool, id: &str, status: &str) {
        sqlx::query(
            "INSERT INTO task_boards (id, thread_id, title, status, created_at, updated_at)
             VALUES (?, 't1', 'Board', ?, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'), strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))",
        )
        .bind(id)
        .bind(status)
        .execute(pool)
        .await
        .expect("seed board");
    }

    #[tokio::test]
    async fn list_by_thread_returns_boards() {
        let pool = setup_test_pool().await;
        seed_board(&pool, "b-1", "active").await;
        seed_board(&pool, "b-2", "completed").await;

        let boards = list_by_thread(&pool, "t1").await.unwrap();
        assert_eq!(boards.len(), 2);
    }

    #[tokio::test]
    async fn find_active_by_thread_returns_only_active() {
        let pool = setup_test_pool().await;
        seed_board(&pool, "b-1", "completed").await;
        seed_board(&pool, "b-2", "active").await;

        let active = find_active_by_thread(&pool, "t1")
            .await
            .unwrap()
            .expect("should exist");
        assert_eq!(active.id, "b-2");
        assert_eq!(active.status, TaskBoardStatus::Active);
    }

    #[tokio::test]
    async fn find_active_by_thread_returns_none_when_no_active() {
        let pool = setup_test_pool().await;
        seed_board(&pool, "b-1", "completed").await;

        let result = find_active_by_thread(&pool, "t1").await.unwrap();
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn update_status_persists() {
        let pool = setup_test_pool().await;
        seed_board(&pool, "b-1", "active").await;

        update_status(&pool, "b-1", &TaskBoardStatus::Completed)
            .await
            .unwrap();

        let board = find_by_id(&pool, "b-1")
            .await
            .unwrap()
            .expect("should exist");
        assert_eq!(board.status, TaskBoardStatus::Completed);
    }

    #[tokio::test]
    async fn update_active_task_sets_and_clears() {
        let pool = setup_test_pool().await;
        seed_board(&pool, "b-1", "active").await;

        update_active_task(&pool, "b-1", Some("task-1"))
            .await
            .unwrap();
        let board = find_by_id(&pool, "b-1")
            .await
            .unwrap()
            .expect("should exist");
        assert_eq!(board.active_task_id, Some("task-1".into()));

        update_active_task(&pool, "b-1", None).await.unwrap();
        let board = find_by_id(&pool, "b-1")
            .await
            .unwrap()
            .expect("should exist");
        assert_eq!(board.active_task_id, None);
    }
}
