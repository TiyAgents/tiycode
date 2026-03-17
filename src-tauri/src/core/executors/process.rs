use tokio::process::Command;

use super::ToolOutput;
use crate::model::errors::AppError;

/// Default timeout for run_command (60 seconds).
const DEFAULT_TIMEOUT_SECS: u64 = 60;

/// Maximum output size (256 KB).
const MAX_OUTPUT_SIZE: usize = 256_000;

/// Execute a non-interactive, one-shot shell command.
/// Input: { "command": "ls -la", "cwd": "/optional/path", "timeout": 30 }
pub async fn run_command(
    input: &serde_json::Value,
    workspace_path: &str,
) -> Result<ToolOutput, AppError> {
    let command = input["command"].as_str().unwrap_or("");
    if command.is_empty() {
        return Ok(ToolOutput {
            success: false,
            result: serde_json::json!({"error": "Missing 'command' field"}),
        });
    }

    let cwd = input["cwd"].as_str().unwrap_or(workspace_path);

    let timeout_secs = input["timeout"].as_u64().unwrap_or(DEFAULT_TIMEOUT_SECS);

    // Use the user's default shell
    let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_string());

    let result = tokio::time::timeout(
        std::time::Duration::from_secs(timeout_secs),
        Command::new(&shell)
            .arg("-c")
            .arg(command)
            .current_dir(cwd)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .output(),
    )
    .await;

    match result {
        Ok(Ok(output)) => {
            let exit_code = output.status.code().unwrap_or(-1);
            let (stdout, stdout_truncated) = truncate_output(&output.stdout);
            let (stderr, stderr_truncated) = truncate_output(&output.stderr);

            Ok(ToolOutput {
                success: output.status.success(),
                result: serde_json::json!({
                    "command": command,
                    "exitCode": exit_code,
                    "stdout": stdout,
                    "stderr": stderr,
                    "stdoutTruncated": stdout_truncated,
                    "stderrTruncated": stderr_truncated,
                }),
            })
        }
        Ok(Err(e)) => Ok(ToolOutput {
            success: false,
            result: serde_json::json!({
                "error": format!("Command execution failed: {e}"),
                "command": command,
            }),
        }),
        Err(_) => Ok(ToolOutput {
            success: false,
            result: serde_json::json!({
                "error": format!("Command timed out after {timeout_secs}s"),
                "command": command,
            }),
        }),
    }
}

fn truncate_output(bytes: &[u8]) -> (String, bool) {
    let s = String::from_utf8_lossy(bytes);
    if s.len() > MAX_OUTPUT_SIZE {
        (s[..MAX_OUTPUT_SIZE].to_string(), true)
    } else {
        (s.to_string(), false)
    }
}
