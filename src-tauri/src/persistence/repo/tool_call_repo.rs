use chrono::Utc;
use sqlx::QueryBuilder;
use sqlx::SqlitePool;

use crate::model::errors::AppError;
use crate::model::thread::ToolCallDto;

#[derive(sqlx::FromRow)]
struct ToolCallRow {
    id: String,
    run_id: String,
    thread_id: String,
    tool_name: String,
    tool_input_json: String,
    tool_output_json: Option<String>,
    status: String,
    approval_status: Option<String>,
    started_at: String,
    finished_at: Option<String>,
}

impl ToolCallRow {
    fn into_dto(self) -> ToolCallDto {
        ToolCallDto {
            id: self.id,
            run_id: self.run_id,
            thread_id: self.thread_id,
            tool_name: self.tool_name,
            tool_input: serde_json::from_str(&self.tool_input_json)
                .unwrap_or(serde_json::Value::String(self.tool_input_json)),
            tool_output: self
                .tool_output_json
                .and_then(|value| serde_json::from_str(&value).ok()),
            status: self.status,
            approval_status: self.approval_status,
            started_at: self.started_at,
            finished_at: self.finished_at,
        }
    }
}

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

pub async fn list_by_run_ids(
    pool: &SqlitePool,
    run_ids: &[String],
) -> Result<Vec<ToolCallDto>, AppError> {
    if run_ids.is_empty() {
        return Ok(Vec::new());
    }

    let mut query = QueryBuilder::new(
        "SELECT id, run_id, thread_id, tool_name, tool_input_json, tool_output_json,
                status, approval_status, started_at, finished_at
         FROM tool_calls
         WHERE run_id IN (",
    );
    {
        let mut separated = query.separated(", ");
        for run_id in run_ids {
            separated.push_bind(run_id);
        }
    }
    query.push(") ORDER BY started_at ASC, id ASC");

    let rows = query
        .build_query_as::<ToolCallRow>()
        .fetch_all(pool)
        .await?;

    Ok(rows.into_iter().map(ToolCallRow::into_dto).collect())
}
