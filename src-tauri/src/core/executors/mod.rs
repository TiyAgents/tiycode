pub mod edit;
pub mod filesystem;
pub mod git;
pub mod output_sanitizer;
pub mod process;
pub mod search;
pub mod terminal;
pub mod truncation;

pub mod web_search;

use serde_json::Value;
use std::sync::Arc;

use crate::core::terminal_manager::TerminalManager;
use crate::model::errors::AppError;
use sqlx::SqlitePool;

/// Result of a tool execution.
pub struct ToolOutput {
    pub success: bool,
    pub result: Value,
}

/// Bundled execution context passed to every tool invocation.
pub struct ToolContext<'a> {
    pub workspace_path: &'a str,
    pub writable_roots: &'a [String],
    pub thread_id: &'a str,
    pub terminal_manager: Option<&'a Arc<TerminalManager>>,
    pub pool: &'a SqlitePool,
}

/// Execute a tool by name with the given input.
pub async fn execute_tool(
    tool_name: &str,
    input: &Value,
    ctx: &ToolContext<'_>,
) -> Result<ToolOutput, AppError> {
    match tool_name {
        "read" => filesystem::read_file(input, ctx.workspace_path, ctx.writable_roots).await,
        "write" => filesystem::write_file(input, ctx.workspace_path, ctx.writable_roots).await,
        "list" => filesystem::list_dir(input, ctx.workspace_path, ctx.writable_roots).await,
        "find" => filesystem::find_files(input, ctx.workspace_path, ctx.writable_roots).await,
        "search" => search::search_repo(input, ctx.workspace_path, ctx.writable_roots).await,
        "web_search" => web_search::execute(input, ctx.pool).await,
        "edit" => edit::edit_file(input, ctx.workspace_path, ctx.writable_roots).await,
        "patch" => edit::edit_file(input, ctx.workspace_path, ctx.writable_roots).await,
        "shell" => process::run_command(input, ctx.workspace_path).await,
        "git_status" | "git_diff" | "git_log" | "git_add" | "git_stage" | "git_unstage"
        | "git_commit" | "git_fetch" | "git_pull" | "git_push" | "git_restore" | "git_clean" => {
            git::execute(tool_name, input, ctx.workspace_path).await
        }
        "term_status" | "term_output" | "term_write" | "term_restart" | "term_close" => {
            let manager = ctx.terminal_manager.ok_or_else(|| {
                AppError::internal(
                    crate::model::errors::ErrorSource::Terminal,
                    "terminal manager unavailable",
                )
            })?;
            terminal::execute(tool_name, input, ctx.thread_id, manager).await
        }
        _ => Ok(ToolOutput {
            success: false,
            result: serde_json::json!({
                "error": format!("Unknown tool: {tool_name}")
            }),
        }),
    }
}
