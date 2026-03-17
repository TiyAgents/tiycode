use chrono::Utc;
use sqlx::SqlitePool;

use crate::model::errors::AppError;

pub struct ToolCallInsert {
    pub id: String,
    pub run_id: String,
    pub thread_id: String,
    pub tool_name: String,
    pub tool_input_json: String,
    pub status: String,
}

pub async fn insert(pool: &SqlitePool, r: &ToolCallInsert) -> Result<(), AppError> {
    let now = Utc::now().to_rfc3339();
    sqlx::query(
        "INSERT INTO tool_calls (id, run_id, thread_id, tool_name, tool_input_json, status, started_at)
         VALUES (?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&r.id)
    .bind(&r.run_id)
    .bind(&r.thread_id)
    .bind(&r.tool_name)
    .bind(&r.tool_input_json)
    .bind(&r.status)
    .bind(&now)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn update_status(pool: &SqlitePool, id: &str, status: &str) -> Result<(), AppError> {
    let is_terminal = matches!(status, "completed" | "failed" | "denied" | "cancelled");

    if is_terminal {
        let now = Utc::now().to_rfc3339();
        sqlx::query("UPDATE tool_calls SET status = ?, finished_at = ? WHERE id = ?")
            .bind(status)
            .bind(&now)
            .bind(id)
            .execute(pool)
            .await?;
    } else {
        sqlx::query("UPDATE tool_calls SET status = ? WHERE id = ?")
            .bind(status)
            .bind(id)
            .execute(pool)
            .await?;
    }
    Ok(())
}

pub async fn update_result(
    pool: &SqlitePool,
    id: &str,
    output_json: &str,
    status: &str,
) -> Result<(), AppError> {
    let now = Utc::now().to_rfc3339();
    sqlx::query(
        "UPDATE tool_calls SET tool_output_json = ?, status = ?, finished_at = ? WHERE id = ?",
    )
    .bind(output_json)
    .bind(status)
    .bind(&now)
    .bind(id)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn update_approval(
    pool: &SqlitePool,
    id: &str,
    approval_status: &str,
    tool_status: &str,
) -> Result<(), AppError> {
    sqlx::query("UPDATE tool_calls SET approval_status = ?, status = ? WHERE id = ?")
        .bind(approval_status)
        .bind(tool_status)
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}
