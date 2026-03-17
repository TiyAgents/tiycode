use sqlx::sqlite::{
    SqliteConnectOptions, SqliteJournalMode, SqlitePool, SqlitePoolOptions, SqliteSynchronous,
};
use std::path::Path;
use std::str::FromStr;
use std::time::Duration;

use crate::model::errors::AppError;

/// Create a SQLite connection pool with WAL mode and recommended settings.
pub async fn create_pool(db_path: &Path) -> Result<SqlitePool, AppError> {
    let db_url = format!("sqlite://{}?mode=rwc", db_path.display());

    let options = SqliteConnectOptions::from_str(&db_url)
        .map_err(|e| {
            AppError::internal(
                crate::model::errors::ErrorSource::Database,
                format!("Invalid database path: {e}"),
            )
        })?
        .journal_mode(SqliteJournalMode::Wal)
        .synchronous(SqliteSynchronous::Normal)
        .foreign_keys(true)
        .busy_timeout(Duration::from_secs(5))
        .pragma("cache_size", "-8000"); // 8MB

    let pool = SqlitePoolOptions::new()
        .max_connections(5)
        .acquire_timeout(Duration::from_secs(5))
        .connect_with(options)
        .await?;

    Ok(pool)
}

/// Run all pending database migrations.
pub async fn run_migrations(pool: &SqlitePool) -> Result<(), AppError> {
    sqlx::migrate!("./migrations")
        .run(pool)
        .await
        .map_err(|e| {
            AppError::internal(
                crate::model::errors::ErrorSource::Database,
                format!("Migration failed: {e}"),
            )
        })?;

    tracing::info!("database migrations completed");
    Ok(())
}
