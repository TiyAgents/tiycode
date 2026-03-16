pub mod filesystem;
pub mod process;
pub mod search;

use serde_json::Value;

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
) -> Result<ToolOutput, AppError> {
    match tool_name {
        "read_file" => filesystem::read_file(input, workspace_path).await,
        "write_file" => filesystem::write_file(input, workspace_path).await,
        "list_dir" => filesystem::list_dir(input, workspace_path).await,
        "search_repo" => search::search_repo(input, workspace_path).await,
        "run_command" => process::run_command(input, workspace_path).await,
        _ => Ok(ToolOutput {
            success: false,
            result: serde_json::json!({
                "error": format!("Unknown tool: {tool_name}")
            }),
        }),
    }
}
