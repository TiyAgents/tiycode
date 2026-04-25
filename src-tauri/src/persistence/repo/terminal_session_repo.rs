use chrono::Utc;
use sqlx::SqlitePool;

use crate::model::errors::AppError;
use crate::model::terminal::{TerminalSessionRecord, TerminalSessionStatus};

#[derive(sqlx::FromRow)]
struct TerminalSessionRow {
    id: String,
    thread_id: String,
    workspace_id: String,
    shell_path: Option<String>,
    cwd: Option<String>,
    status: String,
    pid: Option<i64>,
    exit_code: Option<i32>,
    created_at: String,
    exited_at: Option<String>,
}

impl TerminalSessionRow {
    fn into_record(self) -> TerminalSessionRecord {
        TerminalSessionRecord {
            id: self.id,
            thread_id: self.thread_id,
            workspace_id: self.workspace_id,
            shell_path: self.shell_path,
            cwd: self.cwd,
            status: TerminalSessionStatus::from_str(&self.status),
            pid: self.pid,
            exit_code: self.exit_code,
            created_at: self.created_at,
            exited_at: self.exited_at,
        }
    }
}

pub async fn insert(pool: &SqlitePool, record: &TerminalSessionRecord) -> Result<(), AppError> {
    sqlx::query(
        "INSERT INTO terminal_sessions (id, thread_id, workspace_id, shell_path, cwd, status, pid, exit_code, created_at, exited_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&record.id)
    .bind(&record.thread_id)
    .bind(&record.workspace_id)
    .bind(&record.shell_path)
    .bind(&record.cwd)
    .bind(record.status.as_str())
    .bind(record.pid)
    .bind(record.exit_code)
    .bind(&record.created_at)
    .bind(&record.exited_at)
    .execute(pool)
    .await?;

    Ok(())
}

pub async fn find_active_by_thread(
    pool: &SqlitePool,
    thread_id: &str,
) -> Result<Option<TerminalSessionRecord>, AppError> {
    let row = sqlx::query_as::<_, TerminalSessionRow>(
        "SELECT id, thread_id, workspace_id, shell_path, cwd, status, pid, exit_code, created_at, exited_at
         FROM terminal_sessions
         WHERE thread_id = ? AND status IN ('starting', 'running')
         ORDER BY created_at DESC
         LIMIT 1",
    )
    .bind(thread_id)
    .fetch_optional(pool)
    .await?;

    Ok(row.map(|row| row.into_record()))
}

pub async fn update_running(pool: &SqlitePool, id: &str, pid: Option<i64>) -> Result<(), AppError> {
    sqlx::query(
        "UPDATE terminal_sessions
         SET status = 'running', pid = ?, exit_code = NULL, exited_at = NULL
         WHERE id = ?",
    )
    .bind(pid)
    .bind(id)
    .execute(pool)
    .await?;

    Ok(())
}

pub async fn update_exited(
    pool: &SqlitePool,
    id: &str,
    exit_code: Option<i32>,
) -> Result<(), AppError> {
    let now = Utc::now().to_rfc3339();

    sqlx::query(
        "UPDATE terminal_sessions
         SET status = 'exited', exit_code = ?, exited_at = ?
         WHERE id = ?",
    )
    .bind(exit_code)
    .bind(&now)
    .bind(id)
    .execute(pool)
    .await?;

    Ok(())
}

pub async fn mark_all_active_exited(pool: &SqlitePool) -> Result<u64, AppError> {
    let now = Utc::now().to_rfc3339();

    let result = sqlx::query(
        "UPDATE terminal_sessions
         SET status = 'exited', exited_at = COALESCE(exited_at, ?)
         WHERE status IN ('starting', 'running')",
    )
    .bind(&now)
    .execute(pool)
    .await?;

    Ok(result.rows_affected())
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

    fn make_session(id: &str, status: TerminalSessionStatus) -> TerminalSessionRecord {
        TerminalSessionRecord {
            id: id.into(),
            thread_id: "t1".into(),
            workspace_id: "ws-1".into(),
            shell_path: Some("/bin/zsh".into()),
            cwd: Some("/tmp".into()),
            status,
            pid: None,
            exit_code: None,
            created_at: Utc::now().to_rfc3339(),
            exited_at: None,
        }
    }

    #[tokio::test]
    async fn insert_and_find_active_session() {
        let pool = setup_test_pool().await;
        insert(&pool, &make_session("s-1", TerminalSessionStatus::Starting))
            .await
            .unwrap();

        let found = find_active_by_thread(&pool, "t1")
            .await
            .unwrap()
            .expect("should exist");
        assert_eq!(found.id, "s-1");
    }

    #[tokio::test]
    async fn find_active_skips_exited() {
        let pool = setup_test_pool().await;
        insert(&pool, &make_session("s-1", TerminalSessionStatus::Exited))
            .await
            .unwrap();

        let result = find_active_by_thread(&pool, "t1").await.unwrap();
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn update_running_clears_exit_fields() {
        let pool = setup_test_pool().await;
        let mut s = make_session("s-1", TerminalSessionStatus::Exited);
        s.exit_code = Some(1);
        s.exited_at = Some("yesterday".into());
        insert(&pool, &s).await.unwrap();

        update_running(&pool, "s-1", Some(12345)).await.unwrap();

        let found = find_active_by_thread(&pool, "t1")
            .await
            .unwrap()
            .expect("should be active");
        assert_eq!(found.status, TerminalSessionStatus::Running);
        assert_eq!(found.pid, Some(12345));
        assert_eq!(found.exit_code, None);
        assert_eq!(found.exited_at, None);
    }

    #[tokio::test]
    async fn update_exited_sets_timestamp() {
        let pool = setup_test_pool().await;
        insert(&pool, &make_session("s-1", TerminalSessionStatus::Running))
            .await
            .unwrap();

        update_exited(&pool, "s-1", Some(0)).await.unwrap();

        let found = find_active_by_thread(&pool, "t1").await.unwrap();
        assert!(found.is_none()); // No longer active
    }

    #[tokio::test]
    async fn mark_all_active_exited_batch() {
        let pool = setup_test_pool().await;
        insert(&pool, &make_session("s-1", TerminalSessionStatus::Starting))
            .await
            .unwrap();
        insert(&pool, &make_session("s-2", TerminalSessionStatus::Running))
            .await
            .unwrap();
        insert(&pool, &make_session("s-3", TerminalSessionStatus::Exited))
            .await
            .unwrap();

        let affected = mark_all_active_exited(&pool).await.unwrap();
        assert_eq!(affected, 2); // Only Starting and Running
    }
}
