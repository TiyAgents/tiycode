use std::sync::Arc;

use serde_json::Value;

use crate::core::executors::ToolOutput;
use crate::core::terminal_manager::TerminalManager;
use crate::model::errors::{AppError, ErrorSource};

pub async fn execute(
    tool_name: &str,
    input: &Value,
    thread_id: &str,
    terminal_manager: &Arc<TerminalManager>,
) -> Result<ToolOutput, AppError> {
    match tool_name {
        "term_status" => {
            let session = terminal_manager.get_status(thread_id).await?;
            Ok(ToolOutput {
                success: true,
                result: serde_json::to_value(session).unwrap_or_else(|_| serde_json::json!({})),
            })
        }
        "term_output" => {
            let output = terminal_manager.get_recent_output(thread_id).await?;
            Ok(ToolOutput {
                success: true,
                result: serde_json::json!({ "output": output }),
            })
        }
        "term_write" => {
            let data = input["data"]
                .as_str()
                .or_else(|| input["input"].as_str())
                .ok_or_else(|| {
                    AppError::recoverable(
                        ErrorSource::Terminal,
                        "terminal.input.missing",
                        "Terminal input requires a string `data` field",
                    )
                })?;

            let session = terminal_manager
                .write_input_or_create(thread_id, data)
                .await?;
            Ok(ToolOutput {
                success: true,
                result: serde_json::to_value(session).unwrap_or_else(|_| serde_json::json!({})),
            })
        }
        "term_restart" => {
            let cols = input["cols"].as_u64().map(|value| value as u16);
            let rows = input["rows"].as_u64().map(|value| value as u16);
            let attachment = terminal_manager.restart(thread_id, cols, rows).await?;
            Ok(ToolOutput {
                success: true,
                result: serde_json::to_value(attachment.attach.session)
                    .unwrap_or_else(|_| serde_json::json!({})),
            })
        }
        "term_close" => {
            terminal_manager.close(thread_id).await?;
            Ok(ToolOutput {
                success: true,
                result: serde_json::json!({ "closed": true }),
            })
        }
        _ => Ok(ToolOutput {
            success: false,
            result: serde_json::json!({
                "error": format!("Unknown terminal tool: {tool_name}")
            }),
        }),
    }
}
