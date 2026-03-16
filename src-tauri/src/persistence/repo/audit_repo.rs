use chrono::Utc;
use sqlx::SqlitePool;

use crate::model::errors::AppError;

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
