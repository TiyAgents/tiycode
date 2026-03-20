use chrono::Utc;
use sqlx::QueryBuilder;
use sqlx::SqlitePool;

use crate::model::errors::AppError;
use crate::model::thread::RunHelperDto;

#[derive(sqlx::FromRow)]
struct RunHelperRow {
    id: String,
    run_id: String,
    thread_id: String,
    helper_kind: String,
    parent_tool_call_id: Option<String>,
    status: String,
    input_summary: Option<String>,
    output_summary: Option<String>,
    error_summary: Option<String>,
    started_at: String,
    finished_at: Option<String>,
}

impl RunHelperRow {
    fn into_dto(self) -> RunHelperDto {
        RunHelperDto {
            id: self.id,
            run_id: self.run_id,
            thread_id: self.thread_id,
            helper_kind: self.helper_kind,
            parent_tool_call_id: self.parent_tool_call_id,
            status: self.status,
            input_summary: self.input_summary,
            output_summary: self.output_summary,
            error_summary: self.error_summary,
            started_at: self.started_at,
            finished_at: self.finished_at,
        }
    }
}

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

pub async fn insert(pool: &SqlitePool, helper: &RunHelperInsert) -> Result<String, AppError> {
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

    Ok(now)
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

pub async fn list_by_run_ids(
    pool: &SqlitePool,
    run_ids: &[String],
) -> Result<Vec<RunHelperDto>, AppError> {
    if run_ids.is_empty() {
        return Ok(Vec::new());
    }

    let mut query = QueryBuilder::new(
        "SELECT id, run_id, thread_id, helper_kind, parent_tool_call_id, status,
                input_summary, output_summary, error_summary, started_at, finished_at
         FROM run_helpers
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
        .build_query_as::<RunHelperRow>()
        .fetch_all(pool)
        .await?;

    Ok(rows.into_iter().map(RunHelperRow::into_dto).collect())
}
