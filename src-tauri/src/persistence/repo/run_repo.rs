use chrono::Utc;
use sqlx::SqlitePool;
use tiy_core::types::Usage;

use crate::model::errors::AppError;
use crate::model::thread::{RunSummaryDto, RunUsageDto};

#[derive(sqlx::FromRow)]
struct RunRow {
    id: String,
    thread_id: String,
    run_mode: String,
    status: String,
    model_id: Option<String>,
    effective_model_plan_json: Option<String>,
    error_message: Option<String>,
    started_at: String,
    input_tokens: i64,
    output_tokens: i64,
    cache_read_tokens: i64,
    cache_write_tokens: i64,
    total_tokens: i64,
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

pub async fn update_usage(pool: &SqlitePool, id: &str, usage: &Usage) -> Result<(), AppError> {
    sqlx::query(
        "UPDATE thread_runs
         SET input_tokens = ?, output_tokens = ?, cache_read_tokens = ?,
             cache_write_tokens = ?, total_tokens = ?
         WHERE id = ?",
    )
    .bind(i64::try_from(usage.input).unwrap_or(i64::MAX))
    .bind(i64::try_from(usage.output).unwrap_or(i64::MAX))
    .bind(i64::try_from(usage.cache_read).unwrap_or(i64::MAX))
    .bind(i64::try_from(usage.cache_write).unwrap_or(i64::MAX))
    .bind(i64::try_from(usage.total_tokens).unwrap_or(i64::MAX))
    .bind(id)
    .execute(pool)
    .await?;

    Ok(())
}

pub async fn set_error_message(
    pool: &SqlitePool,
    id: &str,
    error_message: &str,
) -> Result<(), AppError> {
    sqlx::query("UPDATE thread_runs SET error_message = ? WHERE id = ?")
        .bind(error_message)
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}

/// Find the currently active (non-terminal) run for a thread.
pub async fn find_active_by_thread(
    pool: &SqlitePool,
    thread_id: &str,
) -> Result<Option<RunSummaryDto>, AppError> {
    let row = sqlx::query_as::<_, RunRow>(
        "SELECT id, thread_id, run_mode, status, model_id, effective_model_plan_json,
                error_message, started_at, input_tokens, output_tokens,
                cache_read_tokens, cache_write_tokens, total_tokens
         FROM thread_runs
         WHERE thread_id = ?
           AND status NOT IN ('completed', 'failed', 'denied', 'interrupted', 'cancelled')
         ORDER BY started_at DESC
         LIMIT 1",
    )
    .bind(thread_id)
    .fetch_optional(pool)
    .await?;

    Ok(row.map(map_run_summary))
}

/// Find the latest run for a thread (any status), used for ThreadStatus derivation.
pub async fn find_latest_by_thread(
    pool: &SqlitePool,
    thread_id: &str,
) -> Result<Option<RunSummaryDto>, AppError> {
    let row = sqlx::query_as::<_, RunRow>(
        "SELECT id, thread_id, run_mode, status, model_id, effective_model_plan_json,
                error_message, started_at, input_tokens, output_tokens,
                cache_read_tokens, cache_write_tokens, total_tokens
         FROM thread_runs
         WHERE thread_id = ?
         ORDER BY started_at DESC
         LIMIT 1",
    )
    .bind(thread_id)
    .fetch_optional(pool)
    .await?;

    Ok(row.map(map_run_summary))
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

fn map_run_summary(row: RunRow) -> RunSummaryDto {
    let (model_display_name, context_window) =
        extract_primary_model_details(row.effective_model_plan_json.as_deref());

    RunSummaryDto {
        id: row.id,
        thread_id: row.thread_id,
        run_mode: row.run_mode,
        status: row.status,
        model_id: row.model_id,
        model_display_name,
        context_window,
        error_message: row.error_message,
        started_at: row.started_at,
        usage: RunUsageDto {
            input_tokens: row.input_tokens.max(0) as u64,
            output_tokens: row.output_tokens.max(0) as u64,
            cache_read_tokens: row.cache_read_tokens.max(0) as u64,
            cache_write_tokens: row.cache_write_tokens.max(0) as u64,
            total_tokens: row.total_tokens.max(0) as u64,
        },
    }
}

fn extract_primary_model_details(
    effective_model_plan_json: Option<&str>,
) -> (Option<String>, Option<String>) {
    let Some(raw) = effective_model_plan_json else {
        return (None, None);
    };

    let Ok(value) = serde_json::from_str::<serde_json::Value>(raw) else {
        return (None, None);
    };

    let primary = value.get("primary").and_then(|entry| entry.as_object());
    let model_display_name = primary
        .and_then(|entry| {
            entry
                .get("modelDisplayName")
                .and_then(|value| value.as_str())
        })
        .or_else(|| primary.and_then(|entry| entry.get("model").and_then(|value| value.as_str())))
        .map(str::to_string);
    let context_window = primary
        .and_then(|entry| entry.get("contextWindow").and_then(|value| value.as_str()))
        .map(str::to_string);

    (model_display_name, context_window)
}
