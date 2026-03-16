use std::sync::Arc;
use sqlx::SqlitePool;

use crate::core::agent_run_manager::AgentRunManager;
use crate::core::index_manager::IndexManager;
use crate::core::settings_manager::SettingsManager;
use crate::core::sidecar_manager::SidecarManager;
use crate::core::thread_manager::ThreadManager;
use crate::core::tool_gateway::ToolGateway;
use crate::core::workspace_manager::WorkspaceManager;

/// Global application state shared across all Tauri commands.
///
/// Holds the database pool and manager instances.
pub struct AppState {
    pub pool: SqlitePool,
    pub workspace_manager: WorkspaceManager,
    pub settings_manager: SettingsManager,
    pub thread_manager: ThreadManager,
    pub sidecar_manager: Arc<SidecarManager>,
    pub agent_run_manager: Arc<AgentRunManager>,
    pub tool_gateway: Arc<ToolGateway>,
    pub index_manager: IndexManager,
}

impl AppState {
    pub fn new(pool: SqlitePool, sidecar_path: String) -> Self {
        let workspace_manager = WorkspaceManager::new(pool.clone());
        let settings_manager = SettingsManager::new(pool.clone());
        let thread_manager = ThreadManager::new(pool.clone());
        let sidecar_manager = Arc::new(SidecarManager::new(sidecar_path));
        let tool_gateway = Arc::new(ToolGateway::new(pool.clone()));
        let agent_run_manager = Arc::new(AgentRunManager::new(
            pool.clone(),
            Arc::clone(&sidecar_manager),
            Arc::clone(&tool_gateway),
        ));
        let index_manager = IndexManager::new();

        Self {
            pool,
            workspace_manager,
            settings_manager,
            thread_manager,
            sidecar_manager,
            agent_run_manager,
            tool_gateway,
            index_manager,
        }
    }
}
