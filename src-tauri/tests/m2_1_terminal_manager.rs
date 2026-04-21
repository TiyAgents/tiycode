mod test_helpers;

use std::sync::Arc;
use std::time::Duration;

use sqlx::Row;
use tiycode::core::terminal_manager::TerminalManager;

/// Resolve a shell binary that works in the Windows PTY environment.
/// In MSYS2/Git-Bash `$SHELL` is an MSYS path (`/bin/bash.exe`) which the
/// Win32 PTY layer cannot spawn.  We fall back to `cmd.exe` on Windows.
fn test_shell() -> String {
    #[cfg(target_os = "windows")]
    {
        "cmd.exe".to_string()
    }
    #[cfg(not(target_os = "windows"))]
    {
        std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_string())
    }
}

/// Build a command string that echoes a marker, compatible with both
/// bash and cmd.exe.  The trailing `\r\n` is important for cmd.exe.
fn echo_marker_command() -> &'static str {
    "echo __TIY_TERMINAL_TEST__\r\n"
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_terminal_session_lifecycle_and_output() {
    let pool = test_helpers::setup_test_pool().await;
    let workspace_path = std::env::current_dir()
        .expect("workspace path")
        .to_string_lossy()
        .to_string();

    test_helpers::seed_workspace(&pool, "ws-terminal", &workspace_path).await;
    test_helpers::seed_thread(&pool, "thread-terminal", "ws-terminal", None).await;

    let shell = test_shell();
    let manager = Arc::new(TerminalManager::new(pool.clone()));
    let attachment = manager
        .create_or_attach(
            "thread-terminal",
            Some(80),
            Some(24),
            Some(&shell),
            None,
            None,
        )
        .await
        .expect("terminal attach should succeed");

    assert_eq!(attachment.attach.session.thread_id, "thread-terminal");
    assert_eq!(attachment.attach.session.status.as_str(), "running");

    // Give the shell time to start up before sending input.
    tokio::time::sleep(Duration::from_millis(500)).await;

    manager
        .write_input("thread-terminal", echo_marker_command())
        .await
        .expect("terminal write should succeed");

    let mut output = String::new();
    for _ in 0..30 {
        tokio::time::sleep(Duration::from_millis(200)).await;
        output = manager
            .get_recent_output("thread-terminal")
            .await
            .expect("should read replay output");
        if output.contains("__TIY_TERMINAL_TEST__") {
            break;
        }
    }

    // Always close the terminal to avoid blocking the test runtime on exit.
    let close_result = manager.close("thread-terminal").await;

    assert!(
        output.contains("__TIY_TERMINAL_TEST__"),
        "expected terminal output to contain marker, got: {output:?}"
    );

    close_result.expect("terminal close should succeed");
    assert!(manager.list().await.is_empty());
}

#[tokio::test]
async fn test_terminal_recovery_marks_active_sessions_exited() {
    let pool = test_helpers::setup_test_pool().await;
    let now = chrono::Utc::now().to_rfc3339();

    test_helpers::seed_workspace(&pool, "ws-recovery", "/tmp/terminal-recovery").await;
    test_helpers::seed_thread(&pool, "thread-recovery", "ws-recovery", None).await;

    sqlx::query(
        "INSERT INTO terminal_sessions (id, thread_id, workspace_id, shell_path, cwd, status, created_at)
         VALUES (?, ?, ?, ?, ?, 'running', ?)",
    )
    .bind("session-recovery")
    .bind("thread-recovery")
    .bind("ws-recovery")
    .bind("/bin/zsh")
    .bind("/tmp/terminal-recovery")
    .bind(&now)
    .execute(&pool)
    .await
    .expect("seed terminal session");

    let manager = TerminalManager::new(pool.clone());
    manager
        .recover_orphaned_sessions()
        .await
        .expect("recovery should succeed");

    let row = sqlx::query("SELECT status, exited_at FROM terminal_sessions WHERE id = ?")
        .bind("session-recovery")
        .fetch_one(&pool)
        .await
        .expect("terminal session row");

    assert_eq!(row.get::<String, _>("status"), "exited");
    assert!(row.get::<Option<String>, _>("exited_at").is_some());
}
