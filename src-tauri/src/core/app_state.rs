use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use chrono::{DateTime, Utc};
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
    /// Pause start timestamp per run while it waits for user action.
    pub run_pause_started_at: HashMap<String, DateTime<Utc>>,
    /// Accumulated user-wait pause seconds per run.
    pub run_paused_seconds: HashMap<String, i64>,
    /// Thread ID for each run with pause accounting state.
    pub run_pause_thread_ids: HashMap<String, String>,
}

impl GoalRuntimeState {
    /// Remove all per-thread entries for a given thread_id.
    /// Call when a thread is deleted or a goal is cleared to prevent
    /// unbounded memory growth in the shared state HashMaps.
    pub fn cleanup_thread(&mut self, thread_id: &str) {
        self.thread_tool_calls.remove(thread_id);
        self.idle_turn_count.remove(thread_id);
        self.completion_claim_count.remove(thread_id);

        let run_ids: Vec<String> = self
            .run_pause_thread_ids
            .iter()
            .filter_map(|(run_id, stored_thread_id)| {
                (stored_thread_id == thread_id).then(|| run_id.clone())
            })
            .collect();
        for run_id in run_ids {
            self.cleanup_run_pause(&run_id);
        }
    }

    /// Begin timing a run's user-action pause. Repeated starts are ignored so
    /// nested or duplicate waiting events do not lose the original start time.
    pub fn start_run_pause(&mut self, thread_id: &str, run_id: &str) {
        self.run_pause_thread_ids
            .entry(run_id.to_string())
            .or_insert_with(|| thread_id.to_string());
        self.start_run_pause_at(run_id, Utc::now());
    }

    fn start_run_pause_at(&mut self, run_id: &str, started_at: DateTime<Utc>) {
        self.run_pause_started_at
            .entry(run_id.to_string())
            .or_insert(started_at);
    }

    /// Finish the current pause interval for a run and accumulate whole seconds.
    pub fn finish_run_pause(&mut self, run_id: &str) -> i64 {
        self.finish_run_pause_at(run_id, Utc::now())
    }

    fn finish_run_pause_at(&mut self, run_id: &str, finished_at: DateTime<Utc>) -> i64 {
        let Some(started_at) = self.run_pause_started_at.remove(run_id) else {
            return *self.run_paused_seconds.get(run_id).unwrap_or(&0);
        };

        let paused_seconds = (finished_at - started_at).num_seconds().max(0);
        let total = self
            .run_paused_seconds
            .entry(run_id.to_string())
            .or_insert(0);
        *total += paused_seconds;
        *total
    }

    /// Take and clear the accumulated pause seconds for a run.
    pub fn take_run_paused_seconds(&mut self, run_id: &str) -> i64 {
        self.finish_run_pause(run_id);
        let seconds = self.run_paused_seconds.remove(run_id).unwrap_or(0);
        self.run_pause_thread_ids.remove(run_id);
        seconds
    }

    /// Clear all pause accounting state for a run.
    pub fn cleanup_run_pause(&mut self, run_id: &str) {
        self.run_pause_started_at.remove(run_id);
        self.run_paused_seconds.remove(run_id);
        self.run_pause_thread_ids.remove(run_id);
    }
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
        let agent_run_manager = Arc::new(AgentRunManager::new_with_goal_continuation(
            pool.clone(),
            Arc::clone(&app_events),
            Arc::clone(&built_in_agent_runtime),
            Arc::clone(&sleep_manager),
            Arc::clone(&goal_runtime_state),
            true,
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

#[cfg(test)]
mod tests {
    use super::GoalRuntimeState;
    use chrono::{Duration, TimeZone, Utc};

    #[test]
    fn run_pause_tracking_is_idempotent_accumulative_and_cleared_on_take() {
        let mut state = GoalRuntimeState::default();
        let start = Utc.with_ymd_and_hms(2026, 5, 31, 12, 0, 0).unwrap();

        state
            .run_pause_thread_ids
            .insert("run-1".to_string(), "thread-1".to_string());
        state.start_run_pause_at("run-1", start);
        state.start_run_pause_at("run-1", start + Duration::seconds(10));

        assert_eq!(
            state.finish_run_pause_at("run-1", start + Duration::seconds(5)),
            5,
        );
        assert_eq!(
            state.finish_run_pause_at("run-1", start + Duration::seconds(20)),
            5,
        );

        state.start_run_pause_at("run-1", start + Duration::seconds(30));
        assert_eq!(
            state.finish_run_pause_at("run-1", start + Duration::seconds(37)),
            12,
        );

        assert_eq!(state.take_run_paused_seconds("run-1"), 12);
        assert_eq!(state.take_run_paused_seconds("run-1"), 0);
        assert!(!state.run_pause_started_at.contains_key("run-1"));
        assert!(!state.run_paused_seconds.contains_key("run-1"));
        assert!(!state.run_pause_thread_ids.contains_key("run-1"));
    }

    #[test]
    fn cleanup_thread_removes_run_pause_state_for_that_thread() {
        let mut state = GoalRuntimeState::default();
        let start = Utc.with_ymd_and_hms(2026, 5, 31, 12, 0, 0).unwrap();

        state
            .run_pause_thread_ids
            .insert("run-1".to_string(), "thread-1".to_string());
        state.start_run_pause_at("run-1", start);
        state
            .run_pause_thread_ids
            .insert("run-2".to_string(), "thread-2".to_string());
        state.start_run_pause_at("run-2", start);
        state.run_paused_seconds.insert("run-1".to_string(), 3);
        state.run_paused_seconds.insert("run-2".to_string(), 5);

        state.cleanup_thread("thread-1");

        assert!(!state.run_pause_started_at.contains_key("run-1"));
        assert!(!state.run_paused_seconds.contains_key("run-1"));
        assert!(!state.run_pause_thread_ids.contains_key("run-1"));
        assert!(state.run_pause_started_at.contains_key("run-2"));
        assert_eq!(state.run_paused_seconds.get("run-2"), Some(&5));
        assert_eq!(
            state.run_pause_thread_ids.get("run-2").map(String::as_str),
            Some("thread-2"),
        );
    }

    #[test]
    fn run_pause_tracking_clamps_negative_intervals() {
        let mut state = GoalRuntimeState::default();
        let start = Utc.with_ymd_and_hms(2026, 5, 31, 12, 0, 0).unwrap();

        state.start_run_pause_at("run-1", start);

        assert_eq!(
            state.finish_run_pause_at("run-1", start - Duration::seconds(5)),
            0,
        );
    }
}
