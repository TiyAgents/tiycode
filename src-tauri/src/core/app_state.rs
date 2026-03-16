use sqlx::SqlitePool;

/// Global application state shared across all Tauri commands.
///
/// Holds the database pool and will later hold manager instances
/// (WorkspaceManager, ThreadManager, etc.) as they are implemented.
pub struct AppState {
    pub pool: SqlitePool,
}

impl AppState {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }
}
