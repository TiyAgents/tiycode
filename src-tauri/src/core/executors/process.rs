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
        .kill_on_drop(true)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());

    let mut child = cmd.spawn().map_err(|e| {
        AppError::recoverable(
            crate::model::errors::ErrorSource::Tool,
            "tool.shell.spawn_failed",
            format!("Command execution failed: {e}"),
        )
    })?;

    // Take pipe handles before async operations so we can drain them
    // concurrently with `child.wait()` and still explicitly kill+reap
    // the child on timeout (preventing zombie processes).
    let child_stdout = child.stdout.take();
    let child_stderr = child.stderr.take();

    let stdout_task = tokio::spawn(async move {
        let mut buf = Vec::new();
        if let Some(mut out) = child_stdout {
            use tokio::io::AsyncReadExt;
            let _ = out.read_to_end(&mut buf).await;
        }
        buf
    });

    let stderr_task = tokio::spawn(async move {
        let mut buf = Vec::new();
        if let Some(mut err) = child_stderr {
            use tokio::io::AsyncReadExt;
            let _ = err.read_to_end(&mut buf).await;
        }
        buf
    });

    let timeout = tokio::time::sleep(std::time::Duration::from_secs(timeout_secs));
    tokio::pin!(timeout);

    let wait_result = tokio::select! {
        result = child.wait() => Some(result),
        _ = &mut timeout => {
            // Explicitly kill and reap the child to prevent zombie processes.
            let _ = child.kill().await;
            let _ = child.wait().await;
            None
        },
    };

    match wait_result {
        Some(Ok(status)) => {
            let stdout_bytes = stdout_task.await.unwrap_or_default();
            let stderr_bytes = stderr_task.await.unwrap_or_default();
            let exit_code = status.code().unwrap_or(-1);
            let (stdout, stdout_truncated) =
                truncate_tail_bytes(&stdout_bytes, COMMAND_MAX_BYTES, COMMAND_MAX_LINES);
            let (stderr, stderr_truncated) =
                truncate_tail_bytes(&stderr_bytes, COMMAND_MAX_BYTES, COMMAND_MAX_LINES);

            Ok(ToolOutput {
                success: status.success(),
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
        Some(Err(e)) => Ok(ToolOutput {
            success: false,
            result: serde_json::json!({
                "error": format!("Command execution failed: {e}"),
                "command": command,
            }),
        }),
        None => {
            // Abort pipe reader tasks on timeout
            stdout_task.abort();
            stderr_task.abort();
            Ok(ToolOutput {
                success: false,
                result: serde_json::json!({
                    "error": format!("Command timed out after {timeout_secs}s"),
                    "command": command,
                }),
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::run_command;
    use serde_json::json;

    #[tokio::test]
    async fn run_command_returns_error_output_when_command_is_missing() {
        let output = run_command(&json!({}), "/tmp")
            .await
            .expect("missing command should not fail transport");

        assert!(!output.success);
        assert_eq!(output.result["error"], "Missing 'command' field");
    }

    #[cfg(not(target_os = "windows"))]
    #[tokio::test]
    async fn run_command_captures_stdout_stderr_and_exit_code() {
        let tempdir = tempfile::tempdir().expect("tempdir");
        let output = run_command(
            &json!({
                "command": "printf 'hello'; printf 'warn' >&2",
                "cwd": tempdir.path(),
                "timeout": 5,
            }),
            "/tmp",
        )
        .await
        .expect("command should run");

        assert!(output.success);
        assert_eq!(
            output.result["command"],
            "printf 'hello'; printf 'warn' >&2"
        );
        assert_eq!(output.result["exitCode"], 0);
        assert_eq!(output.result["stdout"], "hello");
        assert_eq!(output.result["stderr"], "warn");
        assert_eq!(output.result["stdoutTruncated"], false);
        assert_eq!(output.result["stderrTruncated"], false);
    }

    #[cfg(not(target_os = "windows"))]
    #[tokio::test]
    async fn run_command_reports_nonzero_exit_status_without_transport_error() {
        let output = run_command(
            &json!({
                "command": "printf 'bad' >&2; exit 7",
                "timeout": 5,
            }),
            "/tmp",
        )
        .await
        .expect("command should run");

        assert!(!output.success);
        assert_eq!(output.result["exitCode"], 7);
        assert_eq!(output.result["stderr"], "bad");
    }

    #[cfg(not(target_os = "windows"))]
    #[tokio::test]
    async fn run_command_times_out_and_reports_command() {
        let output = run_command(
            &json!({
                "command": "sleep 2",
                "timeout": 0,
            }),
            "/tmp",
        )
        .await
        .expect("timeout should be reported as tool output");

        assert!(!output.success);
        assert_eq!(output.result["command"], "sleep 2");
        assert_eq!(output.result["error"], "Command timed out after 0s");
    }
}
