use chrono::Utc;
use sqlx::SqlitePool;

use crate::model::errors::{AppError, ErrorSource};
use crate::model::provider::{ProviderModelRecord, ProviderRecord};

// ---------------------------------------------------------------------------
// Provider row mapping
// ---------------------------------------------------------------------------

#[derive(sqlx::FromRow)]
struct ProviderRow {
    id: String,
    name: String,
    protocol_type: String,
    base_url: String,
    api_key_encrypted: Option<String>,
    enabled: i32,
    custom_headers_json: Option<String>,
    created_at: String,
    updated_at: String,
}

impl ProviderRow {
    fn into_record(self) -> ProviderRecord {
        ProviderRecord {
            id: self.id,
            name: self.name,
            protocol_type: self.protocol_type,
            base_url: self.base_url,
            api_key_encrypted: self.api_key_encrypted,
            enabled: self.enabled != 0,
            custom_headers_json: self.custom_headers_json,
            created_at: self.created_at,
            updated_at: self.updated_at,
        }
    }
}

// ---------------------------------------------------------------------------
// Provider CRUD
// ---------------------------------------------------------------------------

pub async fn list_all(pool: &SqlitePool) -> Result<Vec<ProviderRecord>, AppError> {
    let rows = sqlx::query_as::<_, ProviderRow>(
        "SELECT id, name, protocol_type, base_url, api_key_encrypted, enabled,
                custom_headers_json, created_at, updated_at
         FROM providers ORDER BY name",
    )
    .fetch_all(pool)
    .await?;

    Ok(rows.into_iter().map(|r| r.into_record()).collect())
}

pub async fn find_by_id(pool: &SqlitePool, id: &str) -> Result<Option<ProviderRecord>, AppError> {
    let row = sqlx::query_as::<_, ProviderRow>(
        "SELECT id, name, protocol_type, base_url, api_key_encrypted, enabled,
                custom_headers_json, created_at, updated_at
         FROM providers WHERE id = ?",
    )
    .bind(id)
    .fetch_optional(pool)
    .await?;

    Ok(row.map(|r| r.into_record()))
}

pub async fn insert(pool: &SqlitePool, record: &ProviderRecord) -> Result<(), AppError> {
    let now = Utc::now().to_rfc3339();
    sqlx::query(
        "INSERT INTO providers (id, name, protocol_type, base_url, api_key_encrypted,
                enabled, custom_headers_json, created_at, updated_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&record.id)
    .bind(&record.name)
    .bind(&record.protocol_type)
    .bind(&record.base_url)
    .bind(&record.api_key_encrypted)
    .bind(record.enabled as i32)
    .bind(&record.custom_headers_json)
    .bind(&now)
    .bind(&now)
    .execute(pool)
    .await?;

    Ok(())
}

pub async fn update(pool: &SqlitePool, record: &ProviderRecord) -> Result<(), AppError> {
    let now = Utc::now().to_rfc3339();
    let result = sqlx::query(
        "UPDATE providers SET name = ?, protocol_type = ?, base_url = ?,
                api_key_encrypted = ?, enabled = ?, custom_headers_json = ?, updated_at = ?
         WHERE id = ?",
    )
    .bind(&record.name)
    .bind(&record.protocol_type)
    .bind(&record.base_url)
    .bind(&record.api_key_encrypted)
    .bind(record.enabled as i32)
    .bind(&record.custom_headers_json)
    .bind(&now)
    .bind(&record.id)
    .execute(pool)
    .await?;

    if result.rows_affected() == 0 {
        return Err(AppError::not_found(ErrorSource::Settings, "provider"));
    }
    Ok(())
}

pub async fn delete(pool: &SqlitePool, id: &str) -> Result<bool, AppError> {
    let result = sqlx::query("DELETE FROM providers WHERE id = ?")
        .bind(id)
        .execute(pool)
        .await?;
    Ok(result.rows_affected() > 0)
}

// ---------------------------------------------------------------------------
// ProviderModel row mapping
// ---------------------------------------------------------------------------

#[derive(sqlx::FromRow)]
struct ModelRow {
    id: String,
    provider_id: String,
    model_name: String,
    display_name: Option<String>,
    enabled: i32,
    capabilities_json: Option<String>,
    created_at: String,
}

impl ModelRow {
    fn into_record(self) -> ProviderModelRecord {
        ProviderModelRecord {
            id: self.id,
            provider_id: self.provider_id,
            model_name: self.model_name,
            display_name: self.display_name,
            enabled: self.enabled != 0,
            capabilities_json: self.capabilities_json,
            created_at: self.created_at,
        }
    }
}

// ---------------------------------------------------------------------------
// ProviderModel CRUD
// ---------------------------------------------------------------------------

pub async fn list_models(
    pool: &SqlitePool,
    provider_id: &str,
) -> Result<Vec<ProviderModelRecord>, AppError> {
    let rows = sqlx::query_as::<_, ModelRow>(
        "SELECT id, provider_id, model_name, display_name, enabled, capabilities_json, created_at
         FROM provider_models WHERE provider_id = ? ORDER BY model_name",
    )
    .bind(provider_id)
    .fetch_all(pool)
    .await?;

    Ok(rows.into_iter().map(|r| r.into_record()).collect())
}

pub async fn insert_model(pool: &SqlitePool, record: &ProviderModelRecord) -> Result<(), AppError> {
    let now = Utc::now().to_rfc3339();
    sqlx::query(
        "INSERT INTO provider_models (id, provider_id, model_name, display_name,
                enabled, capabilities_json, created_at)
         VALUES (?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&record.id)
    .bind(&record.provider_id)
    .bind(&record.model_name)
    .bind(&record.display_name)
    .bind(record.enabled as i32)
    .bind(&record.capabilities_json)
    .bind(&now)
    .execute(pool)
    .await?;

    Ok(())
}

pub async fn delete_model(pool: &SqlitePool, id: &str) -> Result<bool, AppError> {
    let result = sqlx::query("DELETE FROM provider_models WHERE id = ?")
        .bind(id)
        .execute(pool)
        .await?;
    Ok(result.rows_affected() > 0)
}
