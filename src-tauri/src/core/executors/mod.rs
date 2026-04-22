pub mod edit;
pub mod filesystem;
pub mod git;
pub mod output_sanitizer;
pub mod process;
pub mod search;
pub mod terminal;
pub mod truncation;

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
    writable_roots: &[String],
    thread_id: &str,
    terminal_manager: Option<&Arc<TerminalManager>>,
) -> Result<ToolOutput, AppError> {
    match tool_name {
        "read" => filesystem::read_file(input, workspace_path, writable_roots).await,
        "write" => filesystem::write_file(input, workspace_path, writable_roots).await,
        "list" => filesystem::list_dir(input, workspace_path, writable_roots).await,
        "find" => filesystem::find_files(input, workspace_path, writable_roots).await,
        "search" => search::search_repo(input, workspace_path, writable_roots).await,
        "edit" => edit::edit_file(input, workspace_path, writable_roots).await,
        "patch" => edit::edit_file(input, workspace_path, writable_roots).await,
        "shell" => process::run_command(input, workspace_path).await,
        "git_status" | "git_diff" | "git_log" | "git_add" | "git_stage" | "git_unstage"
        | "git_commit" | "git_fetch" | "git_pull" | "git_push" => {
            git::execute(tool_name, input, workspace_path).await
        }
        "term_status" | "term_output" | "term_write" | "term_restart" | "term_close" => {
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
