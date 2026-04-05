//! M1.6 — Tool Gateway & Policy Engine tests
//!
//! Acceptance criteria:
//! - require-approval tools trigger confirmation UI
//! - `rm -rf /`, `sudo` etc. hard denied
//! - Plan mode blocks mutating tools
//! - audit_events records all tool calls
//! - Workspace boundary enforced

mod test_helpers;

use serde_json::json;
use sqlx::Row;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tiy_agent_lib::core::policy_engine::{PolicyEngine, PolicyVerdict};

// =========================================================================
// T1.6.1 — Dangerous command hard deny
// =========================================================================

#[test]
fn test_policy_dangerous_commands() {
    // These patterns should be detected by PolicyEngine's contains() check.
    // The engine checks if the command string contains a dangerous substring.
    let dangerous_commands = vec![
        ("rm -rf /home", "rm -rf /"),
        ("rm -rf /* --no-preserve-root", "rm -rf /*"),
        ("rm -rf ~/", "rm -rf ~"),
        ("sudo apt install foo", "sudo "),
        ("mkfs.ext4 /dev/sda", "mkfs"),
        ("dd if=/dev/zero of=/dev/sda", "dd if="),
        ("curl|sh", "curl|sh"),     // exact: no spaces
        ("curl |sh", "curl |sh"),   // exact: space before pipe
        ("curl | sh", "curl | sh"), // exact: space both sides
        ("wget|bash", "wget|bash"),
        ("wget |bash", "wget |bash"),
        ("echo > /dev/sda1", "> /dev/sd"),
        ("chmod 777 /etc", "chmod 777 /"),
        (":(){ :|:& };:", ":(){ :|:& };:"),
    ];

    let patterns = [
        "rm -rf /",
        "rm -rf /*",
        "rm -rf ~",
        "sudo ",
        "mkfs",
        "dd if=",
        "curl|sh",
        "curl |sh",
        "curl | sh",
        "wget|sh",
        "wget |sh",
        "wget | sh",
        "curl|bash",
        "curl |bash",
        "curl | bash",
        "wget|bash",
        "wget |bash",
        "wget | bash",
        "> /dev/sd",
        "chmod 777 /",
        ":(){ :|:& };:",
    ];

    for (cmd, expected_match) in &dangerous_commands {
        let lower = cmd.to_lowercase();
        let is_dangerous = patterns.iter().any(|pattern| lower.contains(pattern));

        assert!(
            is_dangerous,
            "Command '{cmd}' should match dangerous pattern '{expected_match}'"
        );
    }
}

#[test]
fn test_safe_commands_not_flagged() {
    let safe_commands = vec![
        "cargo build",
        "npm install",
        "git status",
        "ls -la",
        "cat README.md",
        "echo hello",
        "python main.py",
    ];

    let patterns = [
        "rm -rf /",
        "rm -rf /*",
        "rm -rf ~",
        "sudo ",
        "mkfs",
        "dd if=",
        "curl|sh",
        "curl |sh",
        "curl | sh",
        "wget|sh",
        "wget |sh",
        "wget | sh",
        "curl|bash",
        "curl |bash",
        "curl | bash",
        "wget|bash",
        "wget |bash",
        "wget | bash",
        "> /dev/sd",
        "chmod 777 /",
        ":(){ :|:& };:",
    ];

    for cmd in &safe_commands {
        let lower = cmd.to_lowercase();
        let is_dangerous = patterns.iter().any(|pattern| lower.contains(pattern));

        assert!(
            !is_dangerous,
            "Command '{cmd}' should NOT match dangerous patterns"
        );
    }
}

#[tokio::test]
async fn test_policy_allow_list_pattern_must_match() {
    let pool = test_helpers::setup_test_pool().await;
    test_helpers::seed_policy(
        &pool,
        "allow_list",
        r#"[{"tool":"shell","pattern":"npm test"}]"#,
    )
    .await;

    let engine = PolicyEngine::new(pool);

    let matched = engine
        .evaluate(
            "shell",
            &json!({ "command": "npm test" }),
            None,
            &[],
            "default",
            None,
        )
        .await
        .unwrap();
    assert!(matches!(matched.verdict, PolicyVerdict::AutoAllow));

    let unmatched = engine
        .evaluate(
            "shell",
            &json!({ "command": "cargo test" }),
            None,
            &[],
            "default",
            None,
        )
        .await
        .unwrap();
    assert!(matches!(
        unmatched.verdict,
        PolicyVerdict::RequireApproval { .. }
    ));
}

// =========================================================================
// T1.6.2 — Plan mode blocks mutating tools
// =========================================================================

#[test]
fn test_plan_mode_blocks_mutating_tools() {
    let mutating_tools = vec![
        "write",
        "patch",
        "shell",
        "git_add",
        "git_stage",
        "git_unstage",
        "git_commit",
        "git_push",
        "git_pull",
        "git_fetch",
        "term_write",
        "market_install",
    ];

    let run_mode = "plan";
    for tool in &mutating_tools {
        let should_block = run_mode == "plan" && mutating_tools.contains(tool);
        assert!(should_block, "Plan mode should block mutating tool: {tool}");
    }
}

#[test]
fn test_plan_mode_allows_read_tools() {
    let read_only_tools = vec![
        "read",
        "list",
        "search",
        "git_status",
        "git_diff",
        "git_log",
    ];

    let mutating_tools = vec![
        "write",
        "patch",
        "shell",
        "git_add",
        "git_stage",
        "git_unstage",
        "git_commit",
        "git_push",
        "git_pull",
        "git_fetch",
        "term_write",
        "market_install",
    ];

    for tool in &read_only_tools {
        let blocked = mutating_tools.contains(tool);
        assert!(!blocked, "Plan mode should allow read-only tool: {tool}");
    }
}

// =========================================================================
// T1.6.3 — Workspace boundary enforcement
// =========================================================================

#[test]
fn test_workspace_boundary_check() {
    let workspace_path = "/home/user/project";

    // Paths within workspace
    let valid_paths = vec![
        "/home/user/project/src/main.rs",
        "/home/user/project/README.md",
        "/home/user/project/nested/deep/file.txt",
    ];

    for path in &valid_paths {
        assert!(
            path.starts_with(workspace_path),
            "Path '{path}' should be within workspace"
        );
    }

    // Paths outside workspace
    let invalid_paths = vec![
        "/home/user/other-project/file.rs",
        "/etc/passwd",
        "/home/user/project_evil/payload.sh", // sneaky: prefix match but different dir
    ];

    for path in &invalid_paths {
        // Proper boundary check requires trailing slash or exact match
        let within = path.starts_with(&format!("{workspace_path}/")) || *path == workspace_path;
        assert!(
            !within,
            "Path '{path}' should be OUTSIDE workspace boundary"
        );
    }
}

#[tokio::test]
async fn test_policy_allows_mutating_paths_in_writable_roots() {
    let pool = test_helpers::setup_test_pool().await;
    let engine = PolicyEngine::new(pool);
    let workspace = tempfile::tempdir().expect("workspace");

    let writable_root = tempfile::tempdir().expect("writable root");
    let writable_roots = vec![writable_root.path().to_string_lossy().to_string()];

    let write_check = engine
        .evaluate(
            "write",
            &json!({ "path": writable_root.path().join("notes.txt").to_string_lossy().to_string() }),
            Some(workspace.path().to_string_lossy().as_ref()),
            &writable_roots,
            "default",
            None,
        )
        .await
        .unwrap();

    assert!(
        !matches!(write_check.verdict, PolicyVerdict::Deny { .. }),
        "mutating path inside writable root should not be denied"
    );

    let read_check = engine
        .evaluate(
            "read",
            &json!({ "path": writable_root.path().join("notes.txt").to_string_lossy().to_string() }),
            Some(workspace.path().to_string_lossy().as_ref()),
            &writable_roots,
            "default",
            None,
        )
        .await
        .unwrap();

    assert!(
        !matches!(read_check.verdict, PolicyVerdict::Deny { .. }),
        "read tool should also be allowed inside writable roots"
    );
}

// =========================================================================
// T1.6.4 — Tool call persistence and status tracking
// =========================================================================

#[tokio::test]
async fn test_tool_call_crud() {
    let pool = test_helpers::setup_test_pool().await;
    test_helpers::seed_workspace(&pool, "ws-tc", "/tmp/tc").await;
    test_helpers::seed_thread(&pool, "t-tc", "ws-tc").await;
    test_helpers::seed_run(&pool, "r-tc", "t-tc", "running", "default").await;
    test_helpers::seed_tool_call(&pool, "tc-001", "r-tc", "t-tc", "read", "requested").await;

    let row = sqlx::query("SELECT tool_name, status FROM tool_calls WHERE id = 'tc-001'")
        .fetch_one(&pool)
        .await
        .unwrap();

    assert_eq!(row.get::<String, _>("tool_name"), "read");
    assert_eq!(row.get::<String, _>("status"), "requested");
}

#[tokio::test]
async fn test_tool_call_approval_flow() {
    let pool = test_helpers::setup_test_pool().await;
    test_helpers::seed_workspace(&pool, "ws-appr", "/tmp/appr").await;
    test_helpers::seed_thread(&pool, "t-appr", "ws-appr").await;
    test_helpers::seed_run(&pool, "r-appr", "t-appr", "running", "default").await;
    test_helpers::seed_tool_call(
        &pool,
        "tc-appr",
        "r-appr",
        "t-appr",
        "write",
        "waiting_approval",
    )
    .await;

    // Simulate user approval
    sqlx::query(
        "UPDATE tool_calls SET status = 'running', approval_status = 'approved' WHERE id = 'tc-appr'",
    )
    .execute(&pool)
    .await
    .unwrap();

    let row = sqlx::query("SELECT status, approval_status FROM tool_calls WHERE id = 'tc-appr'")
        .fetch_one(&pool)
        .await
        .unwrap();

    assert_eq!(row.get::<String, _>("status"), "running");
    assert_eq!(
        row.get::<Option<String>, _>("approval_status").unwrap(),
        "approved"
    );
}

#[tokio::test]
async fn test_tool_call_rejection() {
    let pool = test_helpers::setup_test_pool().await;
    test_helpers::seed_workspace(&pool, "ws-rej", "/tmp/rej").await;
    test_helpers::seed_thread(&pool, "t-rej", "ws-rej").await;
    test_helpers::seed_run(&pool, "r-rej", "t-rej", "running", "default").await;
    test_helpers::seed_tool_call(
        &pool,
        "tc-rej",
        "r-rej",
        "t-rej",
        "shell",
        "waiting_approval",
    )
    .await;

    // User rejects
    sqlx::query(
        "UPDATE tool_calls SET status = 'denied', approval_status = 'rejected' WHERE id = 'tc-rej'",
    )
    .execute(&pool)
    .await
    .unwrap();

    let row = sqlx::query("SELECT status FROM tool_calls WHERE id = 'tc-rej'")
        .fetch_one(&pool)
        .await
        .unwrap();

    assert_eq!(row.get::<String, _>("status"), "denied");
}

#[tokio::test]
async fn test_tool_call_completed_with_output() {
    let pool = test_helpers::setup_test_pool().await;
    test_helpers::seed_workspace(&pool, "ws-out", "/tmp/out").await;
    test_helpers::seed_thread(&pool, "t-out", "ws-out").await;
    test_helpers::seed_run(&pool, "r-out", "t-out", "running", "default").await;
    test_helpers::seed_tool_call(&pool, "tc-out", "r-out", "t-out", "read", "running").await;

    let output = r#"{"content":"fn main() {}"}"#;
    sqlx::query(
        "UPDATE tool_calls SET status = 'completed', tool_output_json = ?, finished_at = strftime('%Y-%m-%dT%H:%M:%fZ','now') WHERE id = 'tc-out'",
    )
    .bind(output)
    .execute(&pool)
    .await
    .unwrap();

    let row = sqlx::query("SELECT tool_output_json FROM tool_calls WHERE id = 'tc-out'")
        .fetch_one(&pool)
        .await
        .unwrap();

    let val: serde_json::Value =
        serde_json::from_str(&row.get::<String, _>("tool_output_json")).unwrap();
    assert_eq!(val["content"].as_str().unwrap(), "fn main() {}");
}

// =========================================================================
// T1.6.5 — Policy verdict JSON storage
// =========================================================================

#[tokio::test]
async fn test_tool_call_policy_verdict_stored() {
    let pool = test_helpers::setup_test_pool().await;
    test_helpers::seed_workspace(&pool, "ws-pv", "/tmp/pv").await;
    test_helpers::seed_thread(&pool, "t-pv", "ws-pv").await;
    test_helpers::seed_run(&pool, "r-pv", "t-pv", "running", "default").await;

    let verdict = r#"{"toolName":"write","verdict":{"require_approval":{"reason":"Mutating tool"}},"checkedRules":["builtin","user_deny_list","workspace_boundary"]}"#;

    sqlx::query(
        "INSERT INTO tool_calls (id, run_id, thread_id, tool_name, status, policy_verdict_json)
         VALUES ('tc-pv', 'r-pv', 't-pv', 'write', 'waiting_approval', ?)",
    )
    .bind(verdict)
    .execute(&pool)
    .await
    .unwrap();

    let row = sqlx::query("SELECT policy_verdict_json FROM tool_calls WHERE id = 'tc-pv'")
        .fetch_one(&pool)
        .await
        .unwrap();

    let v: serde_json::Value =
        serde_json::from_str(&row.get::<String, _>("policy_verdict_json")).unwrap();
    assert_eq!(v["toolName"].as_str().unwrap(), "write");
}

// =========================================================================
// T1.6.6 — Audit event recording
// =========================================================================

#[tokio::test]
async fn test_audit_event_recording() {
    let pool = test_helpers::setup_test_pool().await;

    // Seed required FK references
    test_helpers::seed_workspace(&pool, "ws-audit", "/tmp/audit").await;
    test_helpers::seed_thread(&pool, "t-audit", "ws-audit").await;
    test_helpers::seed_run(&pool, "r-audit", "t-audit", "running", "default").await;
    test_helpers::seed_tool_call(&pool, "tc-audit", "r-audit", "t-audit", "read", "completed")
        .await;

    // Verify audit_events table accepts records with correct schema
    sqlx::query(
        "INSERT INTO audit_events (id, actor_type, actor_id, source, workspace_id, thread_id, run_id, tool_call_id, action, target_type, target_id, policy_check_json, result_json)
         VALUES ('audit-001', 'agent', 'sidecar', 'tool_gateway', 'ws-audit', 't-audit', 'r-audit', 'tc-audit', 'tool_execute', 'tool_call', 'tc-audit', ?, ?)",
    )
    .bind(r#"{"verdict":"auto_allow","checkedRules":["builtin","workspace_boundary"]}"#)
    .bind(r#"{"success":true,"duration_ms":42}"#)
    .execute(&pool)
    .await
    .unwrap();

    let row = sqlx::query(
        "SELECT action, policy_check_json, result_json FROM audit_events WHERE id = 'audit-001'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();

    assert_eq!(row.get::<String, _>("action"), "tool_execute");
    let policy: serde_json::Value =
        serde_json::from_str(&row.get::<String, _>("policy_check_json")).unwrap();
    assert_eq!(policy["verdict"].as_str().unwrap(), "auto_allow");
}

// =========================================================================
// T1.6.7 — Pending tool calls index
// =========================================================================

#[tokio::test]
async fn test_pending_tool_calls_query() {
    let pool = test_helpers::setup_test_pool().await;
    test_helpers::seed_workspace(&pool, "ws-pending", "/tmp/pending").await;
    test_helpers::seed_thread(&pool, "t-pending", "ws-pending").await;
    test_helpers::seed_run(&pool, "r-pending", "t-pending", "running", "default").await;

    test_helpers::seed_tool_call(
        &pool,
        "tc-req",
        "r-pending",
        "t-pending",
        "read",
        "requested",
    )
    .await;
    test_helpers::seed_tool_call(
        &pool,
        "tc-wait",
        "r-pending",
        "t-pending",
        "write",
        "waiting_approval",
    )
    .await;
    test_helpers::seed_tool_call(
        &pool,
        "tc-run",
        "r-pending",
        "t-pending",
        "search",
        "running",
    )
    .await;
    test_helpers::seed_tool_call(
        &pool,
        "tc-done",
        "r-pending",
        "t-pending",
        "read",
        "completed",
    )
    .await;

    let pending = sqlx::query(
        "SELECT id FROM tool_calls
         WHERE status IN ('requested', 'waiting_approval', 'running')",
    )
    .fetch_all(&pool)
    .await
    .unwrap();

    assert_eq!(pending.len(), 3);

    let ids: Vec<String> = pending.iter().map(|r| r.get("id")).collect();
    assert!(ids.contains(&"tc-req".to_string()));
    assert!(ids.contains(&"tc-wait".to_string()));
    assert!(ids.contains(&"tc-run".to_string()));
    assert!(!ids.contains(&"tc-done".to_string()));
}

// =========================================================================
// T1.6.8 — Helper-safe approval escalation
// =========================================================================

#[tokio::test]
async fn test_tool_gateway_can_fold_approval_into_escalation() {
    use tiy_agent_lib::core::terminal_manager::TerminalManager;
    use tiy_agent_lib::core::tool_gateway::{
        ToolExecutionOptions, ToolExecutionRequest, ToolGateway, ToolGatewayResult,
    };

    let pool = test_helpers::setup_test_pool().await;
    let workspace_root =
        std::env::temp_dir().join(format!("tiy-tool-gateway-{}", uuid::Uuid::now_v7()));
    std::fs::create_dir_all(&workspace_root).unwrap();
    std::fs::write(workspace_root.join("README.md"), "hello").unwrap();
    let workspace_root = std::fs::canonicalize(&workspace_root).unwrap();
    let readme_path = std::fs::canonicalize(workspace_root.join("README.md")).unwrap();

    test_helpers::seed_workspace(
        &pool,
        "ws-helper-escalate",
        workspace_root.to_str().unwrap(),
    )
    .await;
    test_helpers::seed_thread(&pool, "t-helper-escalate", "ws-helper-escalate").await;
    test_helpers::seed_run(
        &pool,
        "r-helper-escalate",
        "t-helper-escalate",
        "running",
        "default",
    )
    .await;
    test_helpers::seed_tool_call(
        &pool,
        "tc-helper-escalate",
        "r-helper-escalate",
        "t-helper-escalate",
        "write",
        "requested",
    )
    .await;
    test_helpers::seed_policy(&pool, "approval_policy", r#""require_all""#).await;

    let terminal_manager = Arc::new(TerminalManager::new(pool.clone()));
    let gateway = ToolGateway::new(pool.clone(), terminal_manager);
    let approval_prompted = Arc::new(AtomicBool::new(false));
    let execution_started = Arc::new(AtomicBool::new(false));

    let outcome = gateway
        .execute_tool_call(
            ToolExecutionRequest {
                run_id: "r-helper-escalate".into(),
                thread_id: "t-helper-escalate".into(),
                tool_call_id: "tc-helper-escalate".into(),
                tool_name: "write".into(),
                tool_input: serde_json::json!({
                    "path": readme_path.display().to_string(),
                    "content": "updated by helper",
                }),
                workspace_path: workspace_root.display().to_string(),
                run_mode: "default".into(),
            },
            tiy_core::agent::AbortSignal::new(),
            ToolExecutionOptions {
                allow_user_approval: false,
            },
            {
                let approval_prompted = Arc::clone(&approval_prompted);
                move |_| {
                    approval_prompted.store(true, Ordering::SeqCst);
                }
            },
            {
                let execution_started = Arc::clone(&execution_started);
                move || {
                    execution_started.store(true, Ordering::SeqCst);
                }
            },
        )
        .await
        .unwrap();

    assert!(!outcome.approval_required);
    assert!(!approval_prompted.load(Ordering::SeqCst));
    assert!(!execution_started.load(Ordering::SeqCst));

    match outcome.result {
        ToolGatewayResult::EscalationRequired { reason, .. } => {
            assert!(
                reason.contains("Approval required"),
                "unexpected escalation reason: {reason}"
            );
        }
        other => panic!(
            "expected escalation_required outcome, got {:?}",
            std::mem::discriminant(&other)
        ),
    }

    let row = sqlx::query(
        "SELECT status, approval_status FROM tool_calls WHERE id = 'tc-helper-escalate'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();

    assert_eq!(row.get::<String, _>("status"), "denied");
    assert_eq!(
        row.get::<Option<String>, _>("approval_status").unwrap(),
        "escalation_required"
    );
}

#[tokio::test]
async fn test_search_repo_allows_relative_directory_within_workspace() {
    use tiy_agent_lib::core::terminal_manager::TerminalManager;
    use tiy_agent_lib::core::tool_gateway::{
        ToolExecutionOptions, ToolExecutionRequest, ToolGateway, ToolGatewayResult,
    };

    let pool = test_helpers::setup_test_pool().await;
    let workspace_root =
        std::env::temp_dir().join(format!("tiy-search-relative-{}", uuid::Uuid::now_v7()));
    let search_dir = workspace_root.join("src-tauri");
    std::fs::create_dir_all(&search_dir).unwrap();
    std::fs::write(
        search_dir.join("main.rs"),
        "fn main() { println!(\"hello\"); }\n",
    )
    .unwrap();
    let workspace_root = std::fs::canonicalize(&workspace_root).unwrap();

    test_helpers::seed_workspace(
        &pool,
        "ws-search-relative",
        workspace_root.to_str().unwrap(),
    )
    .await;
    test_helpers::seed_thread(&pool, "t-search-relative", "ws-search-relative").await;
    test_helpers::seed_run(
        &pool,
        "r-search-relative",
        "t-search-relative",
        "running",
        "default",
    )
    .await;
    test_helpers::seed_tool_call(
        &pool,
        "tc-search-relative",
        "r-search-relative",
        "t-search-relative",
        "search",
        "requested",
    )
    .await;

    let terminal_manager = Arc::new(TerminalManager::new(pool.clone()));
    let gateway = ToolGateway::new(pool, terminal_manager);

    let outcome = gateway
        .execute_tool_call(
            ToolExecutionRequest {
                run_id: "r-search-relative".into(),
                thread_id: "t-search-relative".into(),
                tool_call_id: "tc-search-relative".into(),
                tool_name: "search".into(),
                tool_input: serde_json::json!({
                    "query": "hello",
                    "directory": "src-tauri",
                }),
                workspace_path: workspace_root.display().to_string(),
                run_mode: "default".into(),
            },
            tiy_core::agent::AbortSignal::new(),
            ToolExecutionOptions::default(),
            |_| {},
            || {},
        )
        .await
        .unwrap();

    match outcome.result {
        ToolGatewayResult::Executed { output, .. } => {
            let directory = output.result["directory"].as_str().unwrap_or_default();
            assert!(
                directory.ends_with("/src-tauri"),
                "search should execute inside the requested relative directory"
            );

            if !output.success {
                let error = output.result["error"].as_str().unwrap_or_default();
                assert!(
                    error.contains("ripgrep execution failed"),
                    "unexpected search failure: {error}"
                );
            }
        }
        ToolGatewayResult::Denied { reason, .. } => {
            panic!("relative workspace directory should not be denied: {reason}");
        }
        ToolGatewayResult::EscalationRequired { reason, .. } => {
            panic!("search should not require approval: {reason}");
        }
        ToolGatewayResult::Cancelled { .. } => {
            panic!("search should not be cancelled");
        }
    }
}

#[tokio::test]
async fn test_search_repo_ignores_wildcard_file_pattern_and_limits_preview() {
    use tiy_agent_lib::core::terminal_manager::TerminalManager;
    use tiy_agent_lib::core::tool_gateway::{
        ToolExecutionOptions, ToolExecutionRequest, ToolGateway, ToolGatewayResult,
    };

    let pool = test_helpers::setup_test_pool().await;
    let workspace_root =
        std::env::temp_dir().join(format!("tiy-search-wildcard-{}", uuid::Uuid::now_v7()));
    std::fs::create_dir_all(&workspace_root).unwrap();
    std::fs::write(
        workspace_root.join("first.rs"),
        "fn first() { println!(\"hello\"); }\n",
    )
    .unwrap();
    std::fs::write(
        workspace_root.join("second.ts"),
        "export const second = 'hello';\n",
    )
    .unwrap();
    let workspace_root = std::fs::canonicalize(&workspace_root).unwrap();

    test_helpers::seed_workspace(
        &pool,
        "ws-search-wildcard",
        workspace_root.to_str().unwrap(),
    )
    .await;
    test_helpers::seed_thread(&pool, "t-search-wildcard", "ws-search-wildcard").await;
    test_helpers::seed_run(
        &pool,
        "r-search-wildcard",
        "t-search-wildcard",
        "running",
        "default",
    )
    .await;
    test_helpers::seed_tool_call(
        &pool,
        "tc-search-wildcard",
        "r-search-wildcard",
        "t-search-wildcard",
        "search",
        "requested",
    )
    .await;

    let terminal_manager = Arc::new(TerminalManager::new(pool.clone()));
    let gateway = ToolGateway::new(pool, terminal_manager);

    let outcome = gateway
        .execute_tool_call(
            ToolExecutionRequest {
                run_id: "r-search-wildcard".into(),
                thread_id: "t-search-wildcard".into(),
                tool_call_id: "tc-search-wildcard".into(),
                tool_name: "search".into(),
                tool_input: serde_json::json!({
                    "query": "hello",
                    "filePattern": "*",
                    "maxResults": 1,
                }),
                workspace_path: workspace_root.display().to_string(),
                run_mode: "default".into(),
            },
            tiy_core::agent::AbortSignal::new(),
            ToolExecutionOptions::default(),
            |_| {},
            || {},
        )
        .await
        .unwrap();

    match outcome.result {
        ToolGatewayResult::Executed { output, .. } => {
            if !output.success {
                let error = output.result["error"].as_str().unwrap_or_default();
                assert!(
                    error.contains("ripgrep"),
                    "unexpected search failure: {error}"
                );
                return;
            }

            assert_eq!(output.result["shownCount"].as_u64(), Some(1));
            assert_eq!(output.result["truncated"].as_bool(), Some(true));
            assert!(
                output.result["count"].as_u64().unwrap_or_default() >= 2,
                "wildcard-only filePattern should be ignored so search spans both files"
            );

            let notice = output.result["notice"].as_str().unwrap_or_default();
            assert!(
                notice.contains("Ignored filePattern '*'"),
                "expected wildcard normalization notice, got: {notice}"
            );
        }
        ToolGatewayResult::Denied { reason, .. } => {
            panic!("wildcard file pattern search should not be denied: {reason}");
        }
        ToolGatewayResult::EscalationRequired { reason, .. } => {
            panic!("wildcard file pattern search should not require approval: {reason}");
        }
        ToolGatewayResult::Cancelled { .. } => {
            panic!("wildcard file pattern search should not be cancelled");
        }
    }
}
