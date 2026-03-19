pub mod filesystem;
pub mod git;
pub mod process;
pub mod search;
pub mod terminal;

use serde_json::Value;
use std::sync::Arc;

use crate::core::terminal_manager::TerminalManager;
use crate::model::errors::AppError;

/// Result of a tool execution.
pub struct ToolOutput {
    pub success: bool,
    pub result: Value,
}

/// Execute a tool by name with the given input.
pub async fn execute_tool(
    tool_name: &str,
    input: &Value,
    workspace_path: &str,
    thread_id: &str,
    terminal_manager: Option<&Arc<TerminalManager>>,
) -> Result<ToolOutput, AppError> {
    match tool_name {
        "read_file" => filesystem::read_file(input, workspace_path).await,
        "write_file" => filesystem::write_file(input, workspace_path).await,
        "list_dir" => filesystem::list_dir(input, workspace_path).await,
        "search_repo" => search::search_repo(input, workspace_path).await,
        "run_command" => process::run_command(input, workspace_path).await,
        "git_add" | "git_stage" | "git_unstage" | "git_commit" | "git_fetch" | "git_pull"
        | "git_push" => git::execute(tool_name, input, workspace_path).await,
        "terminal_get_status"
        | "terminal_get_recent_output"
        | "terminal_write_input"
        | "terminal_write"
        | "terminal_restart" => {
            let manager = terminal_manager.ok_or_else(|| {
                AppError::internal(
                    crate::model::errors::ErrorSource::Terminal,
                    "terminal manager unavailable",
                )
            })?;
            terminal::execute(tool_name, input, thread_id, manager).await
        }
        _ => Ok(ToolOutput {
            success: false,
            result: serde_json::json!({
                "error": format!("Unknown tool: {tool_name}")
            }),
        }),
    }
}
