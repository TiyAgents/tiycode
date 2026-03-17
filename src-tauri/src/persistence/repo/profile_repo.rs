use chrono::Utc;
use sqlx::SqlitePool;

use crate::model::errors::{AppError, ErrorSource};
use crate::model::provider::AgentProfileRecord;

#[derive(sqlx::FromRow)]
struct ProfileRow {
    id: String,
    name: String,
    custom_instructions: Option<String>,
    response_style: Option<String>,
    response_language: Option<String>,
    primary_provider_id: Option<String>,
    primary_model_id: Option<String>,
    auxiliary_provider_id: Option<String>,
    auxiliary_model_id: Option<String>,
    lightweight_provider_id: Option<String>,
    lightweight_model_id: Option<String>,
    is_default: i32,
    created_at: String,
    updated_at: String,
}

impl ProfileRow {
    fn into_record(self) -> AgentProfileRecord {
        AgentProfileRecord {
            id: self.id,
            name: self.name,
            custom_instructions: self.custom_instructions,
            response_style: self.response_style,
            response_language: self.response_language,
            primary_provider_id: self.primary_provider_id,
            primary_model_id: self.primary_model_id,
            auxiliary_provider_id: self.auxiliary_provider_id,
            auxiliary_model_id: self.auxiliary_model_id,
            lightweight_provider_id: self.lightweight_provider_id,
            lightweight_model_id: self.lightweight_model_id,
            is_default: self.is_default != 0,
            created_at: self.created_at,
            updated_at: self.updated_at,
        }
    }
}

pub async fn list_all(pool: &SqlitePool) -> Result<Vec<AgentProfileRecord>, AppError> {
    let rows = sqlx::query_as::<_, ProfileRow>(
        "SELECT id, name, custom_instructions, response_style, response_language,
                primary_provider_id, primary_model_id,
                auxiliary_provider_id, auxiliary_model_id,
                lightweight_provider_id, lightweight_model_id,
                is_default, created_at, updated_at
         FROM agent_profiles ORDER BY is_default DESC, name",
    )
    .fetch_all(pool)
    .await?;

    Ok(rows.into_iter().map(|r| r.into_record()).collect())
}

pub async fn find_by_id(
    pool: &SqlitePool,
    id: &str,
) -> Result<Option<AgentProfileRecord>, AppError> {
    let row = sqlx::query_as::<_, ProfileRow>(
        "SELECT id, name, custom_instructions, response_style, response_language,
                primary_provider_id, primary_model_id,
                auxiliary_provider_id, auxiliary_model_id,
                lightweight_provider_id, lightweight_model_id,
                is_default, created_at, updated_at
         FROM agent_profiles WHERE id = ?",
    )
    .bind(id)
    .fetch_optional(pool)
    .await?;

    Ok(row.map(|r| r.into_record()))
}

pub async fn insert(pool: &SqlitePool, record: &AgentProfileRecord) -> Result<(), AppError> {
    let now = Utc::now().to_rfc3339();
    sqlx::query(
        "INSERT INTO agent_profiles (id, name, custom_instructions, response_style,
                response_language, primary_provider_id, primary_model_id,
                auxiliary_provider_id, auxiliary_model_id,
                lightweight_provider_id, lightweight_model_id,
                is_default, created_at, updated_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&record.id)
    .bind(&record.name)
    .bind(&record.custom_instructions)
    .bind(&record.response_style)
    .bind(&record.response_language)
    .bind(&record.primary_provider_id)
    .bind(&record.primary_model_id)
    .bind(&record.auxiliary_provider_id)
    .bind(&record.auxiliary_model_id)
    .bind(&record.lightweight_provider_id)
    .bind(&record.lightweight_model_id)
    .bind(record.is_default as i32)
    .bind(&now)
    .bind(&now)
    .execute(pool)
    .await?;

    Ok(())
}

pub async fn update(pool: &SqlitePool, record: &AgentProfileRecord) -> Result<(), AppError> {
    let now = Utc::now().to_rfc3339();
    let result = sqlx::query(
        "UPDATE agent_profiles SET name = ?, custom_instructions = ?, response_style = ?,
                response_language = ?, primary_provider_id = ?, primary_model_id = ?,
                auxiliary_provider_id = ?, auxiliary_model_id = ?,
                lightweight_provider_id = ?, lightweight_model_id = ?,
                is_default = ?, updated_at = ?
         WHERE id = ?",
    )
    .bind(&record.name)
    .bind(&record.custom_instructions)
    .bind(&record.response_style)
    .bind(&record.response_language)
    .bind(&record.primary_provider_id)
    .bind(&record.primary_model_id)
    .bind(&record.auxiliary_provider_id)
    .bind(&record.auxiliary_model_id)
    .bind(&record.lightweight_provider_id)
    .bind(&record.lightweight_model_id)
    .bind(record.is_default as i32)
    .bind(&now)
    .bind(&record.id)
    .execute(pool)
    .await?;

    if result.rows_affected() == 0 {
        return Err(AppError::not_found(ErrorSource::Settings, "agent profile"));
    }
    Ok(())
}

pub async fn delete(pool: &SqlitePool, id: &str) -> Result<bool, AppError> {
    let result = sqlx::query("DELETE FROM agent_profiles WHERE id = ?")
        .bind(id)
        .execute(pool)
        .await?;
    Ok(result.rows_affected() > 0)
}

pub async fn set_default(pool: &SqlitePool, id: &str) -> Result<(), AppError> {
    let now = Utc::now().to_rfc3339();
    let mut tx = pool.begin().await?;

    sqlx::query("UPDATE agent_profiles SET is_default = 0, updated_at = ? WHERE is_default = 1")
        .bind(&now)
        .execute(&mut *tx)
        .await?;

    let result =
        sqlx::query("UPDATE agent_profiles SET is_default = 1, updated_at = ? WHERE id = ?")
            .bind(&now)
            .bind(id)
            .execute(&mut *tx)
            .await?;

    if result.rows_affected() == 0 {
        return Err(AppError::not_found(ErrorSource::Settings, "agent profile"));
    }

    tx.commit().await?;
    Ok(())
}
