pub mod agent_run_manager;
pub mod agent_runtime_limits;
pub mod agent_session;
pub mod app_state;
pub mod built_in_agent_runtime;
pub mod context_compression;
pub mod desktop_runtime;
pub mod executors;
pub mod git_manager;
pub mod index_manager;
pub mod local_search;
pub mod plan_checkpoint;
pub mod policy_engine;
pub mod prompt;
pub mod settings_manager;
pub mod shell_runtime;
pub mod sleep_manager;
pub mod startup_manager;
pub mod subagent;
pub mod task_board_manager;
pub mod terminal_manager;
pub mod thread_manager;
pub mod tool_gateway;
pub mod windows_process;
pub mod workspace_manager;
pub mod workspace_paths;

/// Returns the default HTTP headers that identify TiyCode in every LLM API request.
pub fn tiycode_default_headers() -> std::collections::HashMap<String, String> {
    let mut headers = std::collections::HashMap::new();
    headers.insert("X-Title".to_string(), "TiyCode".to_string());
    headers.insert(
        "HTTP-Referer".to_string(),
        "https://github.com/TiyAgents/tiycode".to_string(),
    );
    headers
}
