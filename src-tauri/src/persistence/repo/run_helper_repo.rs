use chrono::Utc;
use sqlx::SqlitePool;

use crate::model::errors::AppError;

pub struct RunHelperInsert {
    pub id: String,
    pub run_id: String,
    pub thread_id: String,
    pub helper_kind: String,
    pub parent_tool_call_id: Option<String>,
    pub status: String,
    pub model_role: String,
    pub provider_id: Option<String>,
    pub model_id: Option<String>,
    pub input_summary: Option<String>,
}

pub async fn insert(pool: &SqlitePool, helper: &RunHelperInsert) -> Result<(), AppError> {
    let now = Utc::now().to_rfc3339();
    sqlx::query(
        "INSERT INTO run_helpers (
            id, run_id, thread_id, helper_kind, parent_tool_call_id, status, model_role,
            provider_id, model_id, input_summary, started_at
         ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&helper.id)
    .bind(&helper.run_id)
    .bind(&helper.thread_id)
    .bind(&helper.helper_kind)
    .bind(&helper.parent_tool_call_id)
    .bind(&helper.status)
    .bind(&helper.model_role)
    .bind(&helper.provider_id)
    .bind(&helper.model_id)
    .bind(&helper.input_summary)
    .bind(&now)
    .execute(pool)
    .await?;

    Ok(())
}

pub async fn mark_completed(
    pool: &SqlitePool,
    id: &str,
    output_summary: &str,
) -> Result<(), AppError> {
    let now = Utc::now().to_rfc3339();
    sqlx::query(
        "UPDATE run_helpers
         SET status = 'completed', output_summary = ?, finished_at = ?
         WHERE id = ?",
    )
    .bind(output_summary)
    .bind(&now)
    .bind(id)
    .execute(pool)
    .await?;

    Ok(())
}

pub async fn mark_failed(
    pool: &SqlitePool,
    id: &str,
    error_summary: &str,
    interrupted: bool,
) -> Result<(), AppError> {
    let now = Utc::now().to_rfc3339();
    let status = if interrupted { "interrupted" } else { "failed" };

    sqlx::query(
        "UPDATE run_helpers
         SET status = ?, error_summary = ?, finished_at = ?
         WHERE id = ?",
    )
    .bind(status)
    .bind(error_summary)
    .bind(&now)
    .bind(id)
    .execute(pool)
    .await?;

    Ok(())
}
