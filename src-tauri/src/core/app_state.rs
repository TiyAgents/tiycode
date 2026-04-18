use sqlx::SqlitePool;
use std::sync::Arc;
use tauri::AppHandle;

use crate::core::agent_run_manager::AgentRunManager;
use crate::core::built_in_agent_runtime::BuiltInAgentRuntime;
use crate::core::git_manager::GitManager;
use crate::core::index_manager::IndexManager;
use crate::core::prompt_command_manager::PromptCommandManager;
use crate::core::settings_manager::SettingsManager;
use crate::core::sleep_manager::SleepManager;
use crate::core::terminal_manager::TerminalManager;
use crate::core::thread_manager::ThreadManager;
use crate::core::tool_gateway::ToolGateway;
use crate::core::workspace_manager::WorkspaceManager;
use crate::core::worktree_manager::WorktreeManager;
use crate::extensions::ExtensionsManager;

/// Global application state shared across all Tauri commands.
///
/// Holds the database pool and manager instances.
pub struct AppState {
    pub pool: SqlitePool,
    pub workspace_manager: WorkspaceManager,
    pub worktree_manager: Arc<WorktreeManager>,
    pub settings_manager: SettingsManager,
    pub prompt_command_manager: PromptCommandManager,
    pub thread_manager: ThreadManager,
    pub sleep_manager: Arc<SleepManager>,
    pub built_in_agent_runtime: Arc<BuiltInAgentRuntime>,
    pub agent_run_manager: Arc<AgentRunManager>,
    pub tool_gateway: Arc<ToolGateway>,
    pub terminal_manager: Arc<TerminalManager>,
    pub index_manager: IndexManager,
    pub git_manager: GitManager,
    pub extensions_manager: Arc<ExtensionsManager>,
}

impl AppState {
    pub fn new(pool: SqlitePool, app_handle: AppHandle) -> Self {
        let workspace_manager = WorkspaceManager::new(pool.clone());
        let worktree_manager = Arc::new(WorktreeManager::new(pool.clone()));
        workspace_manager.set_worktree_manager(Arc::clone(&worktree_manager));
        let settings_manager = SettingsManager::new(pool.clone());
        let prompt_command_manager = PromptCommandManager::new();
        let thread_manager = ThreadManager::new(pool.clone());
        let sleep_manager = Arc::new(SleepManager::new());
        let terminal_manager = Arc::new(TerminalManager::new(pool.clone()));
        let tool_gateway = Arc::new(ToolGateway::new(
            pool.clone(),
            Arc::clone(&terminal_manager),
        ));
        let extensions_manager = Arc::new(ExtensionsManager::new(pool.clone()));
        let built_in_agent_runtime = Arc::new(BuiltInAgentRuntime::new(
            pool.clone(),
            Arc::clone(&tool_gateway),
        ));
        let agent_run_manager = Arc::new(AgentRunManager::new(
            pool.clone(),
            app_handle,
            Arc::clone(&built_in_agent_runtime),
            Arc::clone(&sleep_manager),
        ));
        let index_manager = IndexManager::new();
        let git_manager = GitManager::new();

        Self {
            pool,
            workspace_manager,
            worktree_manager,
            settings_manager,
            prompt_command_manager,
            thread_manager,
            sleep_manager,
            built_in_agent_runtime,
            agent_run_manager,
            tool_gateway,
            terminal_manager,
            index_manager,
            git_manager,
            extensions_manager,
        }
    }
}
