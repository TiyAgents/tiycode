pub mod agent_handlers;
pub mod event_bridge;
pub mod permission_bridge;
pub mod session_map;
pub mod transport;

use std::sync::Arc;

use sqlx::SqlitePool;

use crate::core::agent_run_manager::AgentRunManager;
use crate::core::app_event_emitter::{AppEventEmitterRef, NoopAppEventEmitter};
use crate::core::app_state::{AppState, GoalRuntimeState};
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

use self::session_map::AcpSessionMap;

#[derive(Clone)]
pub struct AcpServerState {
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
    pub index_manager: Arc<IndexManager>,
    pub git_manager: Arc<GitManager>,
    pub extensions_manager: Arc<ExtensionsManager>,
    pub app_events: AppEventEmitterRef,
    pub sessions: Arc<AcpSessionMap>,
}

impl AcpServerState {
    pub fn from_app_state(state: &AppState) -> Self {
        Self {
            pool: state.pool.clone(),
            workspace_manager: Arc::clone(&state.workspace_manager),
            worktree_manager: Arc::clone(&state.worktree_manager),
            settings_manager: Arc::clone(&state.settings_manager),
            prompt_command_manager: Arc::clone(&state.prompt_command_manager),
            thread_manager: Arc::clone(&state.thread_manager),
            sleep_manager: Arc::clone(&state.sleep_manager),
            built_in_agent_runtime: Arc::clone(&state.built_in_agent_runtime),
            agent_run_manager: Arc::clone(&state.agent_run_manager),
            tool_gateway: Arc::clone(&state.tool_gateway),
            terminal_manager: Arc::clone(&state.terminal_manager),
            index_manager: Arc::new(IndexManager::new()),
            git_manager: Arc::new(GitManager::new()),
            extensions_manager: Arc::clone(&state.extensions_manager),
            app_events: Arc::clone(&state.app_events),
            sessions: Arc::new(AcpSessionMap::new()),
        }
    }

    pub fn new_headless(pool: SqlitePool) -> Self {
        let app_events: AppEventEmitterRef = Arc::new(NoopAppEventEmitter);
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
        // Each headless server path creates an isolated GoalRuntimeState.
        // This is intentional — goal features (create_goal, goal_evaluate)
        // are only supported through the Tauri command path (AppState).
        // ACP/gateway paths operate independently and do not share goal
        // tracking state with the GUI.
        let goal_runtime_state = Arc::new(std::sync::Mutex::new(GoalRuntimeState::default()));
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
            index_manager: Arc::new(IndexManager::new()),
            git_manager: Arc::new(GitManager::new()),
            extensions_manager,
            app_events,
            sessions: Arc::new(AcpSessionMap::new()),
        }
    }
}

pub async fn run_stdio(state: AcpServerState) -> Result<(), agent_client_protocol::Error> {
    transport::run_stdio(state).await
}

pub async fn run_http(state: AcpServerState, addr: &str) -> anyhow::Result<()> {
    transport::run_http_server_standalone(state, addr).await
}
