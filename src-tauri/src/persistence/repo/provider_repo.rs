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

pub async fn find_all_by_key(
    pool: &SqlitePool,
    provider_key: &str,
) -> Result<Vec<ProviderRecord>, AppError> {
    let rows = sqlx::query_as::<_, ProviderRow>(
        "SELECT id, provider_kind, provider_key, protocol_type, name, base_url,
                api_key_encrypted, enabled, mapping_locked, custom_headers_json,
                created_at, updated_at
         FROM providers WHERE provider_key = ?
         ORDER BY updated_at DESC, id DESC",
    )
    .bind(provider_key)
    .fetch_all(pool)
    .await?;

    Ok(rows.into_iter().map(|r| r.into_record()).collect())
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

pub async fn find_model_by_id(
    pool: &SqlitePool,
    id: &str,
) -> Result<Option<ProviderModelRecord>, AppError> {
    let row = sqlx::query_as::<_, ModelRow>(
        "SELECT id, provider_id, model_name, sort_index, display_name, enabled, context_window,
                max_output_tokens, capabilities_json, provider_options_json, is_manual, created_at
         FROM provider_models
         WHERE id = ?",
    )
    .bind(id)
    .fetch_optional(pool)
    .await?;

    Ok(row.map(|r| r.into_record()))
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

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
    use std::str::FromStr;

    async fn setup_test_pool() -> SqlitePool {
        let options = SqliteConnectOptions::from_str("sqlite::memory:")
            .expect("invalid sqlite options")
            .foreign_keys(true);

        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(options)
            .await
            .expect("failed to create in-memory pool");

        crate::persistence::sqlite::run_migrations(&pool)
            .await
            .expect("migrations failed");

        pool
    }

    fn make_provider(id: &str, key: &str, name: &str) -> ProviderRecord {
        ProviderRecord {
            id: id.into(),
            provider_kind: ProviderKind::Builtin,
            provider_key: key.into(),
            provider_type: "openai".into(),
            display_name: name.into(),
            base_url: "https://api.openai.com/v1".into(),
            api_key_encrypted: None,
            enabled: true,
            mapping_locked: false,
            custom_headers_json: None,
            created_at: "2026-01-01T00:00:00Z".into(),
            updated_at: "2026-01-01T00:00:00Z".into(),
        }
    }

    fn make_model(id: &str, provider_id: &str, name: &str) -> ProviderModelRecord {
        ProviderModelRecord {
            id: id.into(),
            provider_id: provider_id.into(),
            model_name: name.into(),
            sort_index: 0,
            display_name: Some(name.into()),
            enabled: true,
            context_window: Some("128000".into()),
            max_output_tokens: Some("4096".into()),
            capabilities_json: None,
            provider_options_json: None,
            is_manual: false,
            created_at: "2026-01-01T00:00:00Z".into(),
        }
    }

    #[tokio::test]
    async fn insert_and_find_provider_by_id() {
        let pool = setup_test_pool().await;
        insert(&pool, &make_provider("p-1", "openai-key", "OpenAI"))
            .await
            .unwrap();

        let found = find_by_id(&pool, "p-1")
            .await
            .unwrap()
            .expect("should exist");
        assert_eq!(found.display_name, "OpenAI");
    }

    #[tokio::test]
    async fn find_by_key_returns_none_for_missing() {
        let pool = setup_test_pool().await;
        assert!(find_by_key(&pool, "nobody").await.unwrap().is_none());
    }

    #[tokio::test]
    async fn find_all_by_key_returns_single_match() {
        let pool = setup_test_pool().await;
        // find_all_by_key uses exact match on provider_key, which has a UNIQUE constraint,
        // so it can only return at most one result per key.
        insert(&pool, &make_provider("p-1", "openai-key", "Test"))
            .await
            .unwrap();

        let list = find_all_by_key(&pool, "openai-key").await.unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].display_name, "Test");
    }

    #[tokio::test]
    async fn delete_provider_returns_bool() {
        let pool = setup_test_pool().await;
        insert(&pool, &make_provider("p-1", "k", "Test"))
            .await
            .unwrap();
        assert!(delete(&pool, "p-1").await.unwrap());
        assert!(!delete(&pool, "p-1").await.unwrap());
    }

    #[tokio::test]
    async fn delete_all_clears_table() {
        let pool = setup_test_pool().await;
        insert(&pool, &make_provider("p-1", "k1", "A"))
            .await
            .unwrap();
        insert(&pool, &make_provider("p-2", "k2", "B"))
            .await
            .unwrap();
        delete_all(&pool).await.unwrap();

        let list = list_all(&pool).await.unwrap();
        assert!(list.is_empty());
    }

    #[tokio::test]
    async fn list_models_returns_models_for_provider() {
        let pool = setup_test_pool().await;
        insert(&pool, &make_provider("p-1", "k", "Test"))
            .await
            .unwrap();
        upsert_model(&pool, &make_model("m-1", "p-1", "gpt-4"))
            .await
            .unwrap();
        upsert_model(&pool, &make_model("m-2", "p-1", "gpt-3.5"))
            .await
            .unwrap();

        let models = list_models(&pool, "p-1").await.unwrap();
        assert_eq!(models.len(), 2);
    }

    #[tokio::test]
    async fn upsert_model_inserts_then_updates() {
        let pool = setup_test_pool().await;
        insert(&pool, &make_provider("p-1", "k", "Test"))
            .await
            .unwrap();

        upsert_model(&pool, &make_model("m-1", "p-1", "gpt-4-mini"))
            .await
            .unwrap();
        let found = find_model_by_id(&pool, "m-1")
            .await
            .unwrap()
            .expect("should exist");
        assert_eq!(found.display_name, Some("gpt-4-mini".into()));

        // Update: change name then upsert again
        let mut updated = make_model("m-1", "p-1", "GPT-4 Mini");
        updated.display_name = Some("GPT-4 Mini".into());
        upsert_model(&pool, &updated).await.unwrap();

        let found = find_model_by_id(&pool, "m-1")
            .await
            .unwrap()
            .expect("should exist");
        assert_eq!(found.display_name, Some("GPT-4 Mini".into()));
    }

    #[tokio::test]
    async fn delete_model_returns_bool() {
        let pool = setup_test_pool().await;
        insert(&pool, &make_provider("p-1", "k", "Test"))
            .await
            .unwrap();
        upsert_model(&pool, &make_model("m-1", "p-1", "gpt-4"))
            .await
            .unwrap();

        assert!(delete_model(&pool, "m-1").await.unwrap());
        assert!(!delete_model(&pool, "m-1").await.unwrap());
    }
}
