#[cfg(target_os = "windows")]
use tokio::process::Command;

use super::truncation::{truncate_tail_bytes, COMMAND_MAX_BYTES, COMMAND_MAX_LINES};
use super::ToolOutput;
#[cfg(not(target_os = "windows"))]
use crate::core::shell_runtime::{build_unix_shell_command, UnixShellMode};
#[cfg(target_os = "windows")]
use crate::core::windows_process::configure_background_tokio_command;
use crate::model::errors::AppError;

/// Default timeout for shell (60 seconds).
const DEFAULT_TIMEOUT_SECS: u64 = 60;

/// Execute a non-interactive, one-shot shell command.
/// Output is truncated from the tail (keeps the last N lines/bytes) since
/// the most recent output (errors, final results) is usually most useful.
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

    // Use the platform-appropriate shell
    let mut cmd = {
        #[cfg(target_os = "windows")]
        {
            let mut c = Command::new("cmd.exe");
            configure_background_tokio_command(&mut c);
            c.arg("/C").arg(command);
            c
        }
        #[cfg(not(target_os = "windows"))]
        {
            build_unix_shell_command(command, UnixShellMode::NonLogin)
        }
    };
    cmd.current_dir(cwd)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());

    let result =
        tokio::time::timeout(std::time::Duration::from_secs(timeout_secs), cmd.output()).await;

    match result {
        Ok(Ok(output)) => {
            let exit_code = output.status.code().unwrap_or(-1);
            let (stdout, stdout_truncated) =
                truncate_tail_bytes(&output.stdout, COMMAND_MAX_BYTES, COMMAND_MAX_LINES);
            let (stderr, stderr_truncated) =
                truncate_tail_bytes(&output.stderr, COMMAND_MAX_BYTES, COMMAND_MAX_LINES);

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
