use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use sqlx::SqlitePool;
use tauri::AppHandle;

use crate::core::agent_run_manager::AgentRunManager;
use crate::core::app_event_emitter::{AppEventEmitterRef, TauriAppEventEmitter};
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

/// Shared goal tracking state that survives across command invocations.
/// Stored on AppState so both AgentSession (execution) and Tauri commands
/// (goal_evaluate) can read/write the same counters.
#[derive(Default)]
pub struct GoalRuntimeState {
    /// Accumulated tool call names per thread for the current turn.
    pub thread_tool_calls: HashMap<String, Vec<String>>,
    /// Consecutive idle turn counter per thread.
    pub idle_turn_count: HashMap<String, u32>,
    /// Consecutive completion claim counter per thread.
    pub completion_claim_count: HashMap<String, u32>,
}

/// Global application state shared across all Tauri commands.
///
/// Holds the database pool and manager instances.
pub struct AppState {
    pub pool: SqlitePool,
    pub workspace_manager: Arc<WorkspaceManager>,
    pub worktree_manager: Arc<WorktreeManager>,
    pub settings_manager: Arc<SettingsManager>,
    pub prompt_command_manager: Arc<PromptCommandManager>,
    pub thread_manager: Arc<ThreadManager>,
    pub sleep_manager: Arc<SleepManager>,
    pub built_in_agent_runtime: Arc<BuiltInAgentRuntime>,
    pub agent_run_manager: Arc<AgentRunManager>,
    pub tool_gateway: Arc<ToolGateway>,
    pub terminal_manager: Arc<TerminalManager>,
    pub index_manager: IndexManager,
    pub git_manager: GitManager,
    pub extensions_manager: Arc<ExtensionsManager>,
    pub app_events: AppEventEmitterRef,
    /// Shared goal runtime state for tool call tracking and idle/completion counters.
    pub goal_runtime_state: Arc<Mutex<GoalRuntimeState>>,
}

impl AppState {
    pub fn new(pool: SqlitePool, app_handle: AppHandle) -> Self {
        let app_events: AppEventEmitterRef = Arc::new(TauriAppEventEmitter::new(app_handle));
        let workspace_manager = Arc::new(WorkspaceManager::new(pool.clone()));
        let worktree_manager = Arc::new(WorktreeManager::new(pool.clone()));
        workspace_manager.set_worktree_manager(Arc::clone(&worktree_manager));
        let settings_manager = Arc::new(SettingsManager::new(pool.clone()));
        let prompt_command_manager = Arc::new(PromptCommandManager::new());
        let thread_manager = Arc::new(ThreadManager::new(pool.clone()));
        let sleep_manager = Arc::new(SleepManager::new());
        let terminal_manager = Arc::new(TerminalManager::new(pool.clone()));
        let tool_gateway = Arc::new(ToolGateway::new(
            pool.clone(),
            Arc::clone(&terminal_manager),
        ));
        let extensions_manager = Arc::new(ExtensionsManager::new(pool.clone()));
        let goal_runtime_state = Arc::new(Mutex::new(GoalRuntimeState::default()));
        let built_in_agent_runtime = Arc::new(BuiltInAgentRuntime::new(
            pool.clone(),
            Arc::clone(&tool_gateway),
            Arc::clone(&goal_runtime_state),
        ));
        let agent_run_manager = Arc::new(AgentRunManager::new(
            pool.clone(),
            Arc::clone(&app_events),
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
            app_events,
            goal_runtime_state,
        }
    }
}
