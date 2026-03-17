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
