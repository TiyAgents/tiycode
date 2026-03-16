use chrono::Utc;
use sqlx::SqlitePool;

use crate::model::errors::AppError;
use crate::model::thread::RunSummaryDto;

#[derive(sqlx::FromRow)]
struct RunRow {
    id: String,
    thread_id: String,
    run_mode: String,
    status: String,
    model_id: Option<String>,
    started_at: String,
}

/// Full run record for insert.
pub struct RunInsert {
    pub id: String,
    pub thread_id: String,
    pub profile_id: Option<String>,
    pub run_mode: String,
    pub provider_id: Option<String>,
    pub model_id: Option<String>,
    pub effective_model_plan_json: Option<String>,
    pub status: String,
}

pub async fn insert(pool: &SqlitePool, r: &RunInsert) -> Result<(), AppError> {
    let now = Utc::now().to_rfc3339();
    sqlx::query(
        "INSERT INTO thread_runs (id, thread_id, profile_id, run_mode,
                provider_id, model_id, effective_model_plan_json, status, started_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&r.id)
    .bind(&r.thread_id)
    .bind(&r.profile_id)
    .bind(&r.run_mode)
    .bind(&r.provider_id)
    .bind(&r.model_id)
    .bind(&r.effective_model_plan_json)
    .bind(&r.status)
    .bind(&now)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn update_status(pool: &SqlitePool, id: &str, status: &str) -> Result<(), AppError> {
    let is_terminal = matches!(
        status,
        "completed" | "failed" | "denied" | "interrupted" | "cancelled"
    );

    if is_terminal {
        let now = Utc::now().to_rfc3339();
        sqlx::query("UPDATE thread_runs SET status = ?, finished_at = ? WHERE id = ?")
            .bind(status)
            .bind(&now)
            .bind(id)
            .execute(pool)
            .await?;
    } else {
        sqlx::query("UPDATE thread_runs SET status = ? WHERE id = ?")
            .bind(status)
            .bind(id)
            .execute(pool)
            .await?;
    }

    Ok(())
}

/// Find the currently active (non-terminal) run for a thread.
pub async fn find_active_by_thread(
    pool: &SqlitePool,
    thread_id: &str,
) -> Result<Option<RunSummaryDto>, AppError> {
    let row = sqlx::query_as::<_, RunRow>(
        "SELECT id, thread_id, run_mode, status, model_id, started_at
         FROM thread_runs
         WHERE thread_id = ?
           AND status NOT IN ('completed', 'failed', 'denied', 'interrupted', 'cancelled')
         ORDER BY started_at DESC
         LIMIT 1",
    )
    .bind(thread_id)
    .fetch_optional(pool)
    .await?;

    Ok(row.map(|r| RunSummaryDto {
        id: r.id,
        thread_id: r.thread_id,
        run_mode: r.run_mode,
        status: r.status,
        model_id: r.model_id,
        started_at: r.started_at,
    }))
}

/// Find the latest run for a thread (any status), used for ThreadStatus derivation.
pub async fn find_latest_by_thread(
    pool: &SqlitePool,
    thread_id: &str,
) -> Result<Option<RunSummaryDto>, AppError> {
    let row = sqlx::query_as::<_, RunRow>(
        "SELECT id, thread_id, run_mode, status, model_id, started_at
         FROM thread_runs
         WHERE thread_id = ?
         ORDER BY started_at DESC
         LIMIT 1",
    )
    .bind(thread_id)
    .fetch_optional(pool)
    .await?;

    Ok(row.map(|r| RunSummaryDto {
        id: r.id,
        thread_id: r.thread_id,
        run_mode: r.run_mode,
        status: r.status,
        model_id: r.model_id,
        started_at: r.started_at,
    }))
}

/// Mark all non-terminal runs for a thread as interrupted (crash recovery).
pub async fn interrupt_active_runs(pool: &SqlitePool) -> Result<u64, AppError> {
    let result = sqlx::query(
        "UPDATE thread_runs
         SET status = 'interrupted', finished_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
         WHERE status NOT IN ('completed', 'failed', 'denied', 'interrupted', 'cancelled')
           AND finished_at IS NULL",
    )
    .execute(pool)
    .await?;

    Ok(result.rows_affected())
}
