use chrono::{DateTime, Utc};
use sqlx::SqlitePool;

use crate::model::errors::AppError;
use crate::model::goal::{GoalRecord, GoalStatus, PauseReason};

const SELECT_COLUMNS: &str = "id, thread_id, objective, status, token_budget, tokens_used, \
    time_used_seconds, turns_used, max_turns, pause_reason, pause_detail, evidence, \
    created_at, updated_at";

// ── Database row (raw sqlx types) ──

#[derive(sqlx::FromRow)]
struct GoalRow {
    id: String,
    thread_id: String,
    objective: String,
    status: String,
    token_budget: Option<i64>,
    tokens_used: i64,
    time_used_seconds: i64,
    turns_used: i64,
    max_turns: i64,
    pause_reason: Option<String>,
    pause_detail: Option<String>,
    evidence: Option<String>,
    created_at: String,
    updated_at: String,
}

impl GoalRow {
    fn into_record(self) -> GoalRecord {
        GoalRecord {
            id: self.id,
            thread_id: self.thread_id,
            objective: self.objective,
            status: GoalStatus::from_str(&self.status),
            token_budget: self.token_budget,
            tokens_used: self.tokens_used,
            time_used_seconds: self.time_used_seconds,
            turns_used: self.turns_used,
            max_turns: self.max_turns,
            pause_reason: self.pause_reason.map(|s| PauseReason::from_str(&s)),
            pause_detail: self.pause_detail,
            evidence: self.evidence,
            created_at: DateTime::parse_from_rfc3339(&self.created_at)
                .map(|dt| dt.with_timezone(&Utc))
                .unwrap_or_else(|_| Utc::now()),
            updated_at: DateTime::parse_from_rfc3339(&self.updated_at)
                .map(|dt| dt.with_timezone(&Utc))
                .unwrap_or_else(|_| Utc::now()),
        }
    }
}

// ── Public functions ──

pub async fn find_by_thread_id(
    pool: &SqlitePool,
    thread_id: &str,
) -> Result<Option<GoalRecord>, AppError> {
    let row = sqlx::query_as::<_, GoalRow>(&format!(
        "SELECT {SELECT_COLUMNS} FROM goals WHERE thread_id = ?"
    ))
    .bind(thread_id)
    .fetch_optional(pool)
    .await?;
    Ok(row.map(|r| r.into_record()))
}

pub async fn find_by_id(pool: &SqlitePool, id: &str) -> Result<Option<GoalRecord>, AppError> {
    let row =
        sqlx::query_as::<_, GoalRow>(&format!("SELECT {SELECT_COLUMNS} FROM goals WHERE id = ?"))
            .bind(id)
            .fetch_optional(pool)
            .await?;
    Ok(row.map(|r| r.into_record()))
}

pub async fn insert(pool: &SqlitePool, record: &GoalRecord) -> Result<(), AppError> {
    let now = Utc::now().to_rfc3339();
    sqlx::query(
        "INSERT INTO goals (id, thread_id, objective, status, token_budget, tokens_used, \
         time_used_seconds, turns_used, max_turns, pause_reason, pause_detail, evidence, \
         created_at, updated_at) \
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&record.id)
    .bind(&record.thread_id)
    .bind(&record.objective)
    .bind(record.status.as_str())
    .bind(record.token_budget)
    .bind(record.tokens_used)
    .bind(record.time_used_seconds)
    .bind(record.turns_used)
    .bind(record.max_turns)
    .bind(record.pause_reason.as_ref().map(|r| r.as_str()))
    .bind(record.pause_detail.as_deref())
    .bind(record.evidence.as_deref())
    .bind(&now)
    .bind(&now)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn update_status(
    pool: &SqlitePool,
    id: &str,
    status: GoalStatus,
    pause_reason: Option<PauseReason>,
    pause_detail: Option<&str>,
    evidence: Option<&str>,
) -> Result<bool, AppError> {
    let now = Utc::now().to_rfc3339();
    let result = sqlx::query(
        "UPDATE goals SET status = ?, pause_reason = ?, pause_detail = ?, \
         evidence = COALESCE(NULLIF(?, ''), evidence), updated_at = ? WHERE id = ?",
    )
    .bind(status.as_str())
    .bind(pause_reason.as_ref().map(|r| r.as_str()))
    .bind(pause_detail)
    .bind(evidence)
    .bind(&now)
    .bind(id)
    .execute(pool)
    .await?;
    Ok(result.rows_affected() > 0)
}

/// Atomically increment usage counters and optionally transition to budget_limited.
pub async fn account_usage(
    pool: &SqlitePool,
    id: &str,
    tokens_delta: i64,
    time_delta_seconds: i64,
    turns_delta: i64,
) -> Result<(), AppError> {
    sqlx::query(
        "UPDATE goals SET \
         tokens_used = tokens_used + ?, \
         time_used_seconds = time_used_seconds + ?, \
         turns_used = turns_used + ?, \
         updated_at = ? \
         WHERE id = ?",
    )
    .bind(tokens_delta)
    .bind(time_delta_seconds)
    .bind(turns_delta)
    .bind(Utc::now().to_rfc3339())
    .bind(id)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn delete(pool: &SqlitePool, id: &str) -> Result<bool, AppError> {
    let result = sqlx::query("DELETE FROM goals WHERE id = ?")
        .bind(id)
        .execute(pool)
        .await?;
    Ok(result.rows_affected() > 0)
}

pub async fn delete_by_thread_id(pool: &SqlitePool, thread_id: &str) -> Result<bool, AppError> {
    let result = sqlx::query("DELETE FROM goals WHERE thread_id = ?")
        .bind(thread_id)
        .execute(pool)
        .await?;
    Ok(result.rows_affected() > 0)
}
