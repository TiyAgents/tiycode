use chrono::Utc;
use serde::Serialize;
use sqlx::SqlitePool;

use crate::model::errors::AppError;
use crate::model::extensions::ExtensionActivityEventDto;

pub struct AuditInsert {
    pub actor_type: String,
    pub actor_id: Option<String>,
    pub source: String,
    pub workspace_id: Option<String>,
    pub thread_id: Option<String>,
    pub run_id: Option<String>,
    pub tool_call_id: Option<String>,
    pub action: String,
    pub target_type: Option<String>,
    pub target_id: Option<String>,
    pub policy_check_json: Option<String>,
    pub result_json: Option<String>,
}

pub async fn insert(pool: &SqlitePool, r: &AuditInsert) -> Result<(), AppError> {
    let id = uuid::Uuid::now_v7().to_string();
    let now = Utc::now().to_rfc3339();

    sqlx::query(
        "INSERT INTO audit_events (id, actor_type, actor_id, source, workspace_id,
                thread_id, run_id, tool_call_id, action, target_type, target_id,
                policy_check_json, result_json, created_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&id)
    .bind(&r.actor_type)
    .bind(&r.actor_id)
    .bind(&r.source)
    .bind(&r.workspace_id)
    .bind(&r.thread_id)
    .bind(&r.run_id)
    .bind(&r.tool_call_id)
    .bind(&r.action)
    .bind(&r.target_type)
    .bind(&r.target_id)
    .bind(&r.policy_check_json)
    .bind(&r.result_json)
    .bind(&now)
    .execute(pool)
    .await?;

    Ok(())
}

#[derive(sqlx::FromRow, Serialize)]
struct ExtensionActivityRow {
    id: String,
    source: String,
    action: String,
    target_type: Option<String>,
    target_id: Option<String>,
    result_json: Option<String>,
    created_at: String,
}

pub async fn list_extension_activity(
    pool: &SqlitePool,
    limit: usize,
) -> Result<Vec<ExtensionActivityEventDto>, AppError> {
    let rows = sqlx::query_as::<_, ExtensionActivityRow>(
        "SELECT id, source, action, target_type, target_id, result_json, created_at
         FROM audit_events
         WHERE source = 'extensions'
            OR source LIKE 'plugin:%'
            OR source LIKE 'mcp:%'
         ORDER BY created_at DESC
         LIMIT ?",
    )
    .bind(limit as i64)
    .fetch_all(pool)
    .await?;

    Ok(rows
        .into_iter()
        .map(|row| ExtensionActivityEventDto {
            id: row.id,
            source: row.source,
            action: row.action,
            target_type: row.target_type,
            target_id: row.target_id,
            result: row
                .result_json
                .as_deref()
                .and_then(|value| serde_json::from_str(value).ok()),
            created_at: row.created_at,
        })
        .collect())
}
