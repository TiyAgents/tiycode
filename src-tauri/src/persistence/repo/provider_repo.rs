use chrono::Utc;
use sqlx::SqlitePool;

use crate::model::errors::{AppError, ErrorSource};
use crate::model::provider::{ProviderKind, ProviderModelRecord, ProviderRecord};

// ---------------------------------------------------------------------------
// Provider row mapping
// ---------------------------------------------------------------------------

#[derive(sqlx::FromRow)]
struct ProviderRow {
    id: String,
    provider_kind: String,
    provider_key: String,
    protocol_type: String,
    name: String,
    base_url: String,
    api_key_encrypted: Option<String>,
    enabled: i32,
    mapping_locked: i32,
    custom_headers_json: Option<String>,
    created_at: String,
    updated_at: String,
}

impl ProviderRow {
    fn into_record(self) -> ProviderRecord {
        ProviderRecord {
            id: self.id,
            provider_kind: ProviderKind::from(self.provider_kind),
            provider_key: self.provider_key,
            provider_type: self.protocol_type,
            display_name: self.name,
            base_url: self.base_url,
            api_key_encrypted: self.api_key_encrypted,
            enabled: self.enabled != 0,
            mapping_locked: self.mapping_locked != 0,
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
        "SELECT id, provider_kind, provider_key, protocol_type, name, base_url,
                api_key_encrypted, enabled, mapping_locked, custom_headers_json,
                created_at, updated_at
         FROM providers
         ORDER BY mapping_locked DESC, name ASC",
    )
    .fetch_all(pool)
    .await?;

    Ok(rows.into_iter().map(|r| r.into_record()).collect())
}

pub async fn find_by_id(pool: &SqlitePool, id: &str) -> Result<Option<ProviderRecord>, AppError> {
    let row = sqlx::query_as::<_, ProviderRow>(
        "SELECT id, provider_kind, provider_key, protocol_type, name, base_url,
                api_key_encrypted, enabled, mapping_locked, custom_headers_json,
                created_at, updated_at
         FROM providers WHERE id = ?",
    )
    .bind(id)
    .fetch_optional(pool)
    .await?;

    Ok(row.map(|r| r.into_record()))
}

pub async fn find_by_key(
    pool: &SqlitePool,
    provider_key: &str,
) -> Result<Option<ProviderRecord>, AppError> {
    let row = sqlx::query_as::<_, ProviderRow>(
        "SELECT id, provider_kind, provider_key, protocol_type, name, base_url,
                api_key_encrypted, enabled, mapping_locked, custom_headers_json,
                created_at, updated_at
         FROM providers WHERE provider_key = ?",
    )
    .bind(provider_key)
    .fetch_optional(pool)
    .await?;

    Ok(row.map(|r| r.into_record()))
}

pub async fn insert(pool: &SqlitePool, record: &ProviderRecord) -> Result<(), AppError> {
    let now = Utc::now().to_rfc3339();
    sqlx::query(
        "INSERT INTO providers (
            id, provider_kind, provider_key, protocol_type, name, base_url,
            api_key_encrypted, enabled, mapping_locked, custom_headers_json,
            created_at, updated_at
         ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&record.id)
    .bind(record.provider_kind.as_str())
    .bind(&record.provider_key)
    .bind(&record.provider_type)
    .bind(&record.display_name)
    .bind(&record.base_url)
    .bind(&record.api_key_encrypted)
    .bind(record.enabled as i32)
    .bind(record.mapping_locked as i32)
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
        "UPDATE providers SET
            provider_kind = ?,
            provider_key = ?,
            protocol_type = ?,
            name = ?,
            base_url = ?,
            api_key_encrypted = ?,
            enabled = ?,
            mapping_locked = ?,
            custom_headers_json = ?,
            updated_at = ?
         WHERE id = ?",
    )
    .bind(record.provider_kind.as_str())
    .bind(&record.provider_key)
    .bind(&record.provider_type)
    .bind(&record.display_name)
    .bind(&record.base_url)
    .bind(&record.api_key_encrypted)
    .bind(record.enabled as i32)
    .bind(record.mapping_locked as i32)
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

pub async fn delete_all(pool: &SqlitePool) -> Result<(), AppError> {
    sqlx::query("DELETE FROM providers").execute(pool).await?;
    Ok(())
}

// ---------------------------------------------------------------------------
// ProviderModel row mapping
// ---------------------------------------------------------------------------

#[derive(sqlx::FromRow)]
struct ModelRow {
    id: String,
    provider_id: String,
    model_name: String,
    sort_index: i64,
    display_name: Option<String>,
    enabled: i32,
    context_window: Option<String>,
    max_output_tokens: Option<String>,
    capabilities_json: Option<String>,
    provider_options_json: Option<String>,
    is_manual: i32,
    created_at: String,
}

impl ModelRow {
    fn into_record(self) -> ProviderModelRecord {
        ProviderModelRecord {
            id: self.id,
            provider_id: self.provider_id,
            model_name: self.model_name,
            sort_index: self.sort_index,
            display_name: self.display_name,
            enabled: self.enabled != 0,
            context_window: self.context_window,
            max_output_tokens: self.max_output_tokens,
            capabilities_json: self.capabilities_json,
            provider_options_json: self.provider_options_json,
            is_manual: self.is_manual != 0,
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
        "SELECT id, provider_id, model_name, sort_index, display_name, enabled, context_window,
                max_output_tokens, capabilities_json, provider_options_json, is_manual, created_at
         FROM provider_models
         WHERE provider_id = ?
         ORDER BY is_manual DESC, sort_index ASC, created_at ASC, model_name ASC",
    )
    .bind(provider_id)
    .fetch_all(pool)
    .await?;

    Ok(rows.into_iter().map(|r| r.into_record()).collect())
}

pub async fn upsert_model(pool: &SqlitePool, record: &ProviderModelRecord) -> Result<(), AppError> {
    let now = Utc::now().to_rfc3339();
    sqlx::query(
        "INSERT INTO provider_models (
            id, provider_id, model_name, sort_index, display_name, enabled, context_window,
            max_output_tokens, capabilities_json, provider_options_json, is_manual, created_at
         ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
         ON CONFLICT(id) DO UPDATE SET
            provider_id = excluded.provider_id,
            model_name = excluded.model_name,
            sort_index = excluded.sort_index,
            display_name = excluded.display_name,
            enabled = excluded.enabled,
            context_window = excluded.context_window,
            max_output_tokens = excluded.max_output_tokens,
            capabilities_json = excluded.capabilities_json,
            provider_options_json = excluded.provider_options_json,
            is_manual = excluded.is_manual",
    )
    .bind(&record.id)
    .bind(&record.provider_id)
    .bind(&record.model_name)
    .bind(record.sort_index)
    .bind(&record.display_name)
    .bind(record.enabled as i32)
    .bind(&record.context_window)
    .bind(&record.max_output_tokens)
    .bind(&record.capabilities_json)
    .bind(&record.provider_options_json)
    .bind(record.is_manual as i32)
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
