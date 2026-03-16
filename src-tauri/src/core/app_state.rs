use sqlx::SqlitePool;

use crate::core::settings_manager::SettingsManager;
use crate::core::thread_manager::ThreadManager;
use crate::core::workspace_manager::WorkspaceManager;

/// Global application state shared across all Tauri commands.
///
/// Holds the database pool and manager instances.
pub struct AppState {
    pub pool: SqlitePool,
    pub workspace_manager: WorkspaceManager,
    pub settings_manager: SettingsManager,
    pub thread_manager: ThreadManager,
}

impl AppState {
    pub fn new(pool: SqlitePool) -> Self {
        let workspace_manager = WorkspaceManager::new(pool.clone());
        let settings_manager = SettingsManager::new(pool.clone());
        let thread_manager = ThreadManager::new(pool.clone());
        Self {
            pool,
            workspace_manager,
            settings_manager,
            thread_manager,
        }
    }
}
