use sqlx::SqlitePool;

use crate::model::errors::AppError;
use crate::model::settings::SettingRecord;

#[derive(sqlx::FromRow)]
struct KvRow {
    key: String,
    value_json: String,
    updated_at: String,
}

impl KvRow {
    fn into_record(self) -> SettingRecord {
        SettingRecord {
            key: self.key,
            value_json: self.value_json,
            updated_at: self.updated_at,
        }
    }
}

// ---------------------------------------------------------------------------
// Settings table
// ---------------------------------------------------------------------------

pub async fn get(pool: &SqlitePool, key: &str) -> Result<Option<SettingRecord>, AppError> {
    let row = sqlx::query_as::<_, KvRow>(
        "SELECT key, value_json, updated_at FROM settings WHERE key = ?",
    )
    .bind(key)
    .fetch_optional(pool)
    .await?;

    Ok(row.map(|r| r.into_record()))
}

pub async fn get_all(pool: &SqlitePool) -> Result<Vec<SettingRecord>, AppError> {
    let rows =
        sqlx::query_as::<_, KvRow>("SELECT key, value_json, updated_at FROM settings ORDER BY key")
            .fetch_all(pool)
            .await?;

    Ok(rows.into_iter().map(|r| r.into_record()).collect())
}

pub async fn set(pool: &SqlitePool, key: &str, value_json: &str) -> Result<(), AppError> {
    sqlx::query(
        "INSERT INTO settings (key, value_json, updated_at)
         VALUES (?, ?, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
         ON CONFLICT(key) DO UPDATE SET value_json = excluded.value_json,
         updated_at = excluded.updated_at",
    )
    .bind(key)
    .bind(value_json)
    .execute(pool)
    .await?;

    Ok(())
}

// ---------------------------------------------------------------------------
// Policies table (identical schema, separate table)
// ---------------------------------------------------------------------------

pub async fn policy_get(pool: &SqlitePool, key: &str) -> Result<Option<SettingRecord>, AppError> {
    let row = sqlx::query_as::<_, KvRow>(
        "SELECT key, value_json, updated_at FROM policies WHERE key = ?",
    )
    .bind(key)
    .fetch_optional(pool)
    .await?;

    Ok(row.map(|r| r.into_record()))
}

pub async fn policy_get_all(pool: &SqlitePool) -> Result<Vec<SettingRecord>, AppError> {
    let rows =
        sqlx::query_as::<_, KvRow>("SELECT key, value_json, updated_at FROM policies ORDER BY key")
            .fetch_all(pool)
            .await?;

    Ok(rows.into_iter().map(|r| r.into_record()).collect())
}

pub async fn policy_set(pool: &SqlitePool, key: &str, value_json: &str) -> Result<(), AppError> {
    sqlx::query(
        "INSERT INTO policies (key, value_json, updated_at)
         VALUES (?, ?, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
         ON CONFLICT(key) DO UPDATE SET value_json = excluded.value_json,
         updated_at = excluded.updated_at",
    )
    .bind(key)
    .bind(value_json)
    .execute(pool)
    .await?;

    Ok(())
}
