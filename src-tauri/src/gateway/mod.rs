//! IM Gateway subsystem for TiyCode.
//!
//! Provides WeChat and WeCom communication channels that drive the agent runtime
//! directly (bypassing ACP) via a command-routing + event-pump architecture.

pub mod approval_bridge;
pub mod command_router;
pub mod config;
pub mod gateway_runner;
pub mod message_formatter;
pub mod platforms;
pub mod traits;
pub mod user_session;

use std::sync::Arc;

use sqlx::SqlitePool;

use crate::core::agent_run_manager::AgentRunManager;
use crate::core::app_event_emitter::{AppEventEmitterRef, NoopAppEventEmitter};
use crate::core::built_in_agent_runtime::BuiltInAgentRuntime;
use crate::core::prompt_command_manager::PromptCommandManager;
use crate::core::settings_manager::SettingsManager;
use crate::core::sleep_manager::SleepManager;
use crate::core::terminal_manager::TerminalManager;
use crate::core::thread_manager::ThreadManager;
use crate::core::tool_gateway::ToolGateway;
use crate::core::workspace_manager::WorkspaceManager;
use crate::core::worktree_manager::WorktreeManager;
use crate::extensions::ExtensionsManager;

/// Shared state for the gateway subsystem — mirrors `AcpServerState` structure
/// but without ACP session tracking.
#[derive(Clone)]
pub struct GatewayState {
    pub pool: SqlitePool,
    pub workspace_manager: Arc<WorkspaceManager>,
    pub worktree_manager: Arc<WorktreeManager>,
    pub settings_manager: Arc<SettingsManager>,
    pub prompt_command_manager: Arc<PromptCommandManager>,
    pub thread_manager: Arc<ThreadManager>,
    pub agent_run_manager: Arc<AgentRunManager>,
    pub tool_gateway: Arc<ToolGateway>,
    pub terminal_manager: Arc<TerminalManager>,
    pub extensions_manager: Arc<ExtensionsManager>,
    pub app_events: AppEventEmitterRef,
}

impl GatewayState {
    /// Construct a headless gateway state from a database pool.
    ///
    /// Follows the same initialization pattern as `AcpServerState::new_headless`.
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
        let built_in_agent_runtime = Arc::new(BuiltInAgentRuntime::new(
            pool.clone(),
            Arc::clone(&tool_gateway),
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
            agent_run_manager,
            tool_gateway,
            terminal_manager,
            extensions_manager,
            app_events,
        }
    }
}

/// Entry point for running the IM gateway as a headless service.
///
/// Called from `lib.rs::run_gateway()` after runtime initialization.
pub async fn run(state: GatewayState, config: config::GatewayConfig) -> anyhow::Result<()> {
    gateway_runner::run(state, config).await
}
