use sqlx::SqlitePool;
use std::sync::Arc;

use crate::core::agent_run_manager::AgentRunManager;
use crate::core::git_manager::GitManager;
use crate::core::index_manager::IndexManager;
use crate::core::settings_manager::SettingsManager;
use crate::core::sidecar_manager::SidecarManager;
use crate::core::sleep_manager::SleepManager;
use crate::core::terminal_manager::TerminalManager;
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
    pub sleep_manager: Arc<SleepManager>,
    pub agent_run_manager: Arc<AgentRunManager>,
    pub tool_gateway: Arc<ToolGateway>,
    pub terminal_manager: Arc<TerminalManager>,
    pub index_manager: IndexManager,
    pub git_manager: GitManager,
}

impl AppState {
    pub fn new(pool: SqlitePool, sidecar_path: String) -> Self {
        let workspace_manager = WorkspaceManager::new(pool.clone());
        let settings_manager = SettingsManager::new(pool.clone());
        let thread_manager = ThreadManager::new(pool.clone());
        let sidecar_manager = Arc::new(SidecarManager::new(sidecar_path));
        let sleep_manager = Arc::new(SleepManager::new());
        let terminal_manager = Arc::new(TerminalManager::new(pool.clone()));
        let tool_gateway = Arc::new(ToolGateway::new(
            pool.clone(),
            Arc::clone(&terminal_manager),
        ));
        let agent_run_manager = Arc::new(AgentRunManager::new(
            pool.clone(),
            Arc::clone(&sidecar_manager),
            Arc::clone(&sleep_manager),
            Arc::clone(&tool_gateway),
        ));
        let index_manager = IndexManager::new();
        let git_manager = GitManager::new();

        Self {
            pool,
            workspace_manager,
            settings_manager,
            thread_manager,
            sidecar_manager,
            sleep_manager,
            agent_run_manager,
            tool_gateway,
            terminal_manager,
            index_manager,
            git_manager,
        }
    }
}
