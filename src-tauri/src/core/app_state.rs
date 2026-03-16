use sqlx::SqlitePool;

use crate::core::workspace_manager::WorkspaceManager;

/// Global application state shared across all Tauri commands.
///
/// Holds the database pool and manager instances.
pub struct AppState {
    pub pool: SqlitePool,
    pub workspace_manager: WorkspaceManager,
}

impl AppState {
    pub fn new(pool: SqlitePool) -> Self {
        let workspace_manager = WorkspaceManager::new(pool.clone());
        Self {
            pool,
            workspace_manager,
        }
    }
}
