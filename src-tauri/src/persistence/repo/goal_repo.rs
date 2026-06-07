use chrono::{DateTime, Utc};
use sqlx::SqlitePool;

use crate::model::errors::AppError;
use crate::model::goal::{GoalRecord, GoalStatus, PauseReason};

const SELECT_COLUMNS: &str = "id, thread_id, objective, status, token_budget, tokens_used, \
    time_used_seconds, turns_used, max_turns, pause_reason, pause_detail, evidence, \
    last_evaluated_run_id, judge_passed, judge_completeness, judge_findings, judge_summary, \
    judge_evaluated_run_id, created_at, updated_at";

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
    last_evaluated_run_id: Option<String>,
    judge_passed: i64,
    judge_completeness: Option<i64>,
    judge_findings: Option<String>,
    judge_summary: Option<String>,
    judge_evaluated_run_id: Option<String>,
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
            last_evaluated_run_id: self.last_evaluated_run_id,
            judge_passed: self.judge_passed != 0,
            judge_completeness: self.judge_completeness,
            judge_findings: self.judge_findings,
            judge_summary: self.judge_summary,
            judge_evaluated_run_id: self.judge_evaluated_run_id,
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
    // Note: the judge_* columns are intentionally omitted here and rely on the
    // DDL defaults (judge_passed=0, others NULL) set by the goal_judge_fields
    // migration. New goals always start un-verified, and the Judge verdict is
    // written later via record_judge_verdict().
    let now = Utc::now().to_rfc3339();
    sqlx::query(
        "INSERT INTO goals (id, thread_id, objective, status, token_budget, tokens_used, \
         time_used_seconds, turns_used, max_turns, pause_reason, pause_detail, evidence, \
         last_evaluated_run_id, created_at, updated_at) \
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
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
    .bind(record.last_evaluated_run_id.as_deref())
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

/// Atomically claim evaluation of a terminal run for an active goal.
/// Returns false when the goal is no longer active or the run was already evaluated.
pub async fn mark_evaluated_if_needed(
    pool: &SqlitePool,
    id: &str,
    run_id: &str,
) -> Result<bool, AppError> {
    let result = sqlx::query(
        "UPDATE goals SET \
         last_evaluated_run_id = ?, \
         updated_at = ? \
         WHERE id = ? \
           AND status = 'active' \
           AND (last_evaluated_run_id IS NULL OR last_evaluated_run_id != ?)",
    )
    .bind(run_id)
    .bind(Utc::now().to_rfc3339())
    .bind(id)
    .bind(run_id)
    .execute(pool)
    .await?;
    Ok(result.rows_affected() > 0)
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

/// Persist the most recent Judge verdict for a goal. Always updates the
/// `judge_*` columns. When `passed` is true, the same transaction also writes
/// `status='complete'` and `evidence=summary` so that acceptance
/// (`status=complete` AND `judge_passed=1`) can never be observed as a
/// half-applied state. When `passed` is false the goal's `status` is left
/// unchanged (typically still `active`).
#[allow(clippy::too_many_arguments)]
pub async fn record_judge_verdict(
    pool: &SqlitePool,
    id: &str,
    run_id: &str,
    passed: bool,
    completeness: i64,
    findings_json: &str,
    summary: &str,
) -> Result<bool, AppError> {
    let now = Utc::now().to_rfc3339();
    let mut tx = pool.begin().await?;

    let updated = sqlx::query(
        "UPDATE goals SET \
         judge_passed = ?, \
         judge_completeness = ?, \
         judge_findings = ?, \
         judge_summary = ?, \
         judge_evaluated_run_id = ?, \
         updated_at = ? \
         WHERE id = ?",
    )
    .bind(if passed { 1_i64 } else { 0_i64 })
    .bind(completeness)
    .bind(findings_json)
    .bind(summary)
    .bind(run_id)
    .bind(&now)
    .bind(id)
    .execute(&mut *tx)
    .await?;

    if updated.rows_affected() == 0 {
        tx.rollback().await?;
        return Ok(false);
    }

    if passed {
        sqlx::query(
            "UPDATE goals SET \
             status = 'complete', \
             evidence = COALESCE(NULLIF(?, ''), evidence), \
             updated_at = ? \
             WHERE id = ?",
        )
        .bind(summary)
        .bind(&now)
        .bind(id)
        .execute(&mut *tx)
        .await?;
    }

    tx.commit().await?;
    Ok(true)
}
