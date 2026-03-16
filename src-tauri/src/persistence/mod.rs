pub mod repo;
pub mod sqlite;

use sqlx::SqlitePool;
use std::path::Path;

use crate::model::errors::AppError;

/// Initialize the SQLite connection pool and run migrations.
pub async fn init_database(db_path: &Path) -> Result<SqlitePool, AppError> {
    let pool = sqlite::create_pool(db_path).await?;
    sqlite::run_migrations(&pool).await?;
    Ok(pool)
}
