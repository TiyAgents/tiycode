//! Terminal management tests
//!
//! Coverage:
//! - Shell detection (bash vs cmd.exe)
//! - Terminal session lifecycle
//! - Unicode marker commands and output polling

mod test_helpers;

use std::ffi::OsString;
use std::path::Path;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use sqlx::Row;
use tiycode::core::terminal_manager::TerminalManager;

#[cfg(not(target_os = "windows"))]
fn locale_env_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

#[cfg(not(target_os = "windows"))]
struct LocaleEnvGuard {
    original_lang: Option<OsString>,
    original_lc_ctype: Option<OsString>,
    original_lc_all: Option<OsString>,
}

#[cfg(not(target_os = "windows"))]
impl LocaleEnvGuard {
    fn clear() -> Self {
        let original_lang = std::env::var_os("LANG");
        let original_lc_ctype = std::env::var_os("LC_CTYPE");
        let original_lc_all = std::env::var_os("LC_ALL");
        std::env::remove_var("LANG");
        std::env::remove_var("LC_CTYPE");
        std::env::remove_var("LC_ALL");

        Self {
            original_lang,
            original_lc_ctype,
            original_lc_all,
        }
    }
}

#[cfg(not(target_os = "windows"))]
impl Drop for LocaleEnvGuard {
    fn drop(&mut self) {
        match &self.original_lang {
            Some(value) => std::env::set_var("LANG", value),
            None => std::env::remove_var("LANG"),
        }
        match &self.original_lc_ctype {
            Some(value) => std::env::set_var("LC_CTYPE", value),
            None => std::env::remove_var("LC_CTYPE"),
        }
        match &self.original_lc_all {
            Some(value) => std::env::set_var("LC_ALL", value),
            None => std::env::remove_var("LC_ALL"),
        }
    }
}

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

#[cfg(not(target_os = "windows"))]
fn echo_unicode_marker_command() -> &'static str {
    "printf '中文终端验证\n'\r\n"
}

async fn wait_for_output_contains(
    manager: &Arc<TerminalManager>,
    thread_id: &str,
    needle: &str,
) -> String {
    let mut output = String::new();
    for _ in 0..30 {
        tokio::time::sleep(Duration::from_millis(200)).await;
        output = manager
            .get_recent_output(thread_id)
            .await
            .expect("should read replay output");
        if output.contains(needle) {
            break;
        }
    }

    output
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

    let output =
        wait_for_output_contains(&manager, "thread-terminal", "__TIY_TERMINAL_TEST__").await;

    // Always close the terminal to avoid blocking the test runtime on exit.
    let close_result = manager.close("thread-terminal").await;

    assert!(
        output.contains("__TIY_TERMINAL_TEST__"),
        "expected terminal output to contain marker, got: {output:?}"
    );

    close_result.expect("terminal close should succeed");
    assert!(manager.list().await.is_empty());
}

/// Unicode回归当前只在非 Windows shell 上自动验证。
/// Windows 默认走 `cmd.exe`，其 code page / ConPTY Unicode 行为需要单独手工验证。
#[cfg(not(target_os = "windows"))]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_terminal_unicode_output_round_trip() {
    let _locale_guard = locale_env_lock().lock().expect("locale env lock poisoned");

    let pool = test_helpers::setup_test_pool().await;
    let workspace_path = std::env::current_dir()
        .expect("workspace path")
        .to_string_lossy()
        .to_string();

    test_helpers::seed_workspace(&pool, "ws-terminal-unicode", &workspace_path).await;
    test_helpers::seed_thread(
        &pool,
        "thread-terminal-unicode",
        "ws-terminal-unicode",
        None,
    )
    .await;

    let shell = test_shell();
    let manager = Arc::new(TerminalManager::new(pool.clone()));
    manager
        .create_or_attach(
            "thread-terminal-unicode",
            Some(80),
            Some(24),
            Some(&shell),
            None,
            None,
        )
        .await
        .expect("terminal attach should succeed");

    tokio::time::sleep(Duration::from_millis(500)).await;

    manager
        .write_input("thread-terminal-unicode", echo_unicode_marker_command())
        .await
        .expect("unicode terminal write should succeed");

    let output =
        wait_for_output_contains(&manager, "thread-terminal-unicode", "中文终端验证").await;
    let close_result = manager.close("thread-terminal-unicode").await;

    assert!(
        output.contains("中文终端验证"),
        "expected terminal output to contain unicode marker, got: {output:?}"
    );

    close_result.expect("terminal close should succeed");
}

#[cfg(not(target_os = "windows"))]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_terminal_injects_utf8_locale_when_missing() {
    let _locale_guard = locale_env_lock().lock().expect("locale env lock poisoned");
    let _locale_env = LocaleEnvGuard::clear();

    let pool = test_helpers::setup_test_pool().await;
    let workspace_path = std::env::current_dir()
        .expect("workspace path")
        .to_string_lossy()
        .to_string();

    test_helpers::seed_workspace(&pool, "ws-terminal-locale", &workspace_path).await;
    test_helpers::seed_thread(&pool, "thread-terminal-locale", "ws-terminal-locale", None).await;

    let shell = if Path::new("/bin/sh").exists() {
        "/bin/sh".to_string()
    } else {
        test_shell()
    };
    let manager = Arc::new(TerminalManager::new(pool.clone()));
    manager
        .create_or_attach(
            "thread-terminal-locale",
            Some(80),
            Some(24),
            Some(&shell),
            None,
            None,
        )
        .await
        .expect("terminal attach should succeed without inherited locale");

    tokio::time::sleep(Duration::from_millis(500)).await;

    manager
        .write_input("thread-terminal-locale", echo_unicode_marker_command())
        .await
        .expect("unicode terminal write should succeed with injected locale");

    let output = wait_for_output_contains(&manager, "thread-terminal-locale", "中文终端验证").await;
    let close_result = manager.close("thread-terminal-locale").await;

    assert!(
        output.contains("中文终端验证"),
        "expected terminal output to contain unicode marker after locale fallback, got: {output:?}"
    );

    close_result.expect("terminal close should succeed");
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
