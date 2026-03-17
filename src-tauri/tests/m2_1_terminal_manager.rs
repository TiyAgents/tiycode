mod test_helpers;

use std::sync::Arc;
use std::time::Duration;

use sqlx::Row;
use tiy_agent_lib::core::terminal_manager::TerminalManager;

#[tokio::test]
async fn test_terminal_session_lifecycle_and_output() {
    let pool = test_helpers::setup_test_pool().await;
    let workspace_path = std::env::current_dir()
        .expect("workspace path")
        .to_string_lossy()
        .to_string();

    test_helpers::seed_workspace(&pool, "ws-terminal", &workspace_path).await;
    test_helpers::seed_thread(&pool, "thread-terminal", "ws-terminal").await;

    let manager = Arc::new(TerminalManager::new(pool.clone()));
    let attachment = manager
        .create_or_attach("thread-terminal", Some(80), Some(24))
        .await
        .expect("terminal attach should succeed");

    assert_eq!(attachment.attach.session.thread_id, "thread-terminal");
    assert_eq!(attachment.attach.session.status.as_str(), "running");

    manager
        .write_input("thread-terminal", "printf '__TIY_TERMINAL_TEST__\\n'\n")
        .await
        .expect("terminal write should succeed");

    let mut output = String::new();
    for _ in 0..20 {
        tokio::time::sleep(Duration::from_millis(100)).await;
        output = manager
            .get_recent_output("thread-terminal")
            .await
            .expect("should read replay output");
        if output.contains("__TIY_TERMINAL_TEST__") {
            break;
        }
    }

    assert!(
        output.contains("__TIY_TERMINAL_TEST__"),
        "expected terminal output to contain marker, got: {output:?}"
    );

    manager
        .close("thread-terminal")
        .await
        .expect("terminal close should succeed");

    assert!(manager.list().await.is_empty());
}

#[tokio::test]
async fn test_terminal_recovery_marks_active_sessions_exited() {
    let pool = test_helpers::setup_test_pool().await;
    let now = chrono::Utc::now().to_rfc3339();

    test_helpers::seed_workspace(&pool, "ws-recovery", "/tmp/terminal-recovery").await;
    test_helpers::seed_thread(&pool, "thread-recovery", "ws-recovery").await;

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
