//! Built-in agent runtime tests
//!
//! Acceptance criteria:
//! - Run state machine: Created → Dispatching → Running ⇄ WaitingApproval → Completed/Failed/Cancelled/Interrupted
//! - Crash recovery marks dangling runs as interrupted
//! - Runtime model plan resolves into executable built-in agent sessions

mod test_helpers;

use std::fs;

use sqlx::Row;
use tempfile::tempdir;
use tiycode_lib::core::thread_manager::ThreadManager;

// =========================================================================
// T1.5.1 — Run lifecycle state machine
// =========================================================================

#[tokio::test]
async fn test_run_creation_with_default_status() {
    let pool = test_helpers::setup_test_pool().await;
    test_helpers::seed_workspace(&pool, "ws-run", "/tmp/run").await;
    test_helpers::seed_thread(&pool, "t-run", "ws-run", None).await;
    test_helpers::seed_run(&pool, "r-create", "t-run", "created", "default").await;

    let row = sqlx::query("SELECT status, run_mode FROM thread_runs WHERE id = 'r-create'")
        .fetch_one(&pool)
        .await
        .unwrap();

    assert_eq!(row.get::<String, _>("status"), "created");
    assert_eq!(row.get::<String, _>("run_mode"), "default");
}

#[tokio::test]
async fn test_run_state_transitions() {
    let pool = test_helpers::setup_test_pool().await;
    test_helpers::seed_workspace(&pool, "ws-sm", "/tmp/sm").await;
    test_helpers::seed_thread(&pool, "t-sm", "ws-sm", None).await;
    test_helpers::seed_run(&pool, "r-sm", "t-sm", "created", "default").await;

    // Transition: created → dispatching
    sqlx::query("UPDATE thread_runs SET status = 'dispatching' WHERE id = 'r-sm'")
        .execute(&pool)
        .await
        .unwrap();

    // Transition: dispatching → running
    sqlx::query("UPDATE thread_runs SET status = 'running' WHERE id = 'r-sm'")
        .execute(&pool)
        .await
        .unwrap();

    // Transition: running → waiting_approval
    sqlx::query("UPDATE thread_runs SET status = 'waiting_approval' WHERE id = 'r-sm'")
        .execute(&pool)
        .await
        .unwrap();

    // Transition: waiting_approval → running (after approval)
    sqlx::query("UPDATE thread_runs SET status = 'running' WHERE id = 'r-sm'")
        .execute(&pool)
        .await
        .unwrap();

    // Transition: running → completed
    sqlx::query("UPDATE thread_runs SET status = 'completed', finished_at = strftime('%Y-%m-%dT%H:%M:%fZ','now') WHERE id = 'r-sm'")
        .execute(&pool)
        .await
        .unwrap();

    let row = sqlx::query("SELECT status, finished_at FROM thread_runs WHERE id = 'r-sm'")
        .fetch_one(&pool)
        .await
        .unwrap();

    assert_eq!(row.get::<String, _>("status"), "completed");
    assert!(row.get::<Option<String>, _>("finished_at").is_some());
}

#[tokio::test]
async fn test_run_failure_with_error() {
    let pool = test_helpers::setup_test_pool().await;
    test_helpers::seed_workspace(&pool, "ws-fail", "/tmp/fail").await;
    test_helpers::seed_thread(&pool, "t-fail", "ws-fail", None).await;
    test_helpers::seed_run(&pool, "r-fail", "t-fail", "running", "default").await;

    sqlx::query(
        "UPDATE thread_runs SET status = 'failed', error_message = 'LLM timeout', finished_at = strftime('%Y-%m-%dT%H:%M:%fZ','now') WHERE id = 'r-fail'",
    )
    .execute(&pool)
    .await
    .unwrap();

    let row = sqlx::query("SELECT status, error_message FROM thread_runs WHERE id = 'r-fail'")
        .fetch_one(&pool)
        .await
        .unwrap();

    assert_eq!(row.get::<String, _>("status"), "failed");
    assert_eq!(
        row.get::<Option<String>, _>("error_message").unwrap(),
        "LLM timeout"
    );
}

#[tokio::test]
async fn test_run_cancellation() {
    let pool = test_helpers::setup_test_pool().await;
    test_helpers::seed_workspace(&pool, "ws-cancel", "/tmp/cancel").await;
    test_helpers::seed_thread(&pool, "t-cancel", "ws-cancel", None).await;
    test_helpers::seed_run(&pool, "r-cancel", "t-cancel", "running", "default").await;

    sqlx::query("UPDATE thread_runs SET status = 'cancelled', finished_at = strftime('%Y-%m-%dT%H:%M:%fZ','now') WHERE id = 'r-cancel'")
        .execute(&pool)
        .await
        .unwrap();

    let row = sqlx::query("SELECT status FROM thread_runs WHERE id = 'r-cancel'")
        .fetch_one(&pool)
        .await
        .unwrap();

    assert_eq!(row.get::<String, _>("status"), "cancelled");
}

#[tokio::test]
async fn test_limit_reached_run_syncs_thread_to_needs_reply() {
    let pool = test_helpers::setup_test_pool().await;
    test_helpers::seed_workspace(&pool, "ws-limit", "/tmp/limit").await;
    test_helpers::seed_thread(&pool, "t-limit", "ws-limit", None).await;
    test_helpers::seed_run(&pool, "r-limit", "t-limit", "running", "default").await;

    sqlx::query(
        "UPDATE thread_runs
         SET status = 'limit_reached',
             error_message = 'Agent reached the maximum turn limit (25) before producing a final response',
             finished_at = strftime('%Y-%m-%dT%H:%M:%fZ','now')
         WHERE id = 'r-limit'",
    )
    .execute(&pool)
    .await
    .unwrap();

    let manager = ThreadManager::new(pool.clone());
    manager.sync_status("t-limit").await.unwrap();

    let row = sqlx::query("SELECT status FROM threads WHERE id = 't-limit'")
        .fetch_one(&pool)
        .await
        .unwrap();

    assert_eq!(row.get::<String, _>("status"), "needs_reply");
}

// =========================================================================
// T1.5.2 — Crash recovery (interrupted runs)
// =========================================================================

#[tokio::test]
async fn test_recover_interrupted_runs() {
    let pool = test_helpers::setup_test_pool().await;
    test_helpers::seed_workspace(&pool, "ws-rec", "/tmp/rec").await;
    test_helpers::seed_thread(&pool, "t-rec", "ws-rec", None).await;

    // Create "dangling" runs that were in-progress when app crashed
    test_helpers::seed_run(&pool, "r-dangling-1", "t-rec", "running", "default").await;
    test_helpers::seed_run(&pool, "r-dangling-2", "t-rec", "dispatching", "default").await;
    test_helpers::seed_run(&pool, "r-ok", "t-rec", "completed", "default").await;

    // Create dangling tool calls and run helpers
    test_helpers::seed_tool_call(
        &pool,
        "tc-running",
        "r-dangling-1",
        "t-rec",
        "read",
        "running",
    )
    .await;
    test_helpers::seed_tool_call(
        &pool,
        "tc-waiting",
        "r-dangling-1",
        "t-rec",
        "write",
        "waiting_approval",
    )
    .await;
    test_helpers::seed_tool_call(&pool, "tc-completed", "r-ok", "t-rec", "read", "completed").await;
    test_helpers::seed_run_helper(
        &pool,
        "h-running",
        "r-dangling-1",
        "t-rec",
        "helper_explore",
        "running",
    )
    .await;
    test_helpers::seed_run_helper(
        &pool,
        "h-completed",
        "r-ok",
        "t-rec",
        "helper_explore",
        "completed",
    )
    .await;

    let manager = ThreadManager::new(pool.clone());
    manager.recover_interrupted_runs().await.unwrap();

    // Verify dangling runs are now interrupted
    let dangling1 =
        sqlx::query("SELECT status, error_message FROM thread_runs WHERE id = 'r-dangling-1'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(dangling1.get::<String, _>("status"), "interrupted");
    assert_eq!(
        dangling1.get::<Option<String>, _>("error_message").as_deref(),
        Some("The app closed or the run was terminated before completion. Restarted in interrupted state.")
    );

    let dangling2 =
        sqlx::query("SELECT status, error_message FROM thread_runs WHERE id = 'r-dangling-2'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(dangling2.get::<String, _>("status"), "interrupted");
    assert_eq!(
        dangling2.get::<Option<String>, _>("error_message").as_deref(),
        Some("The app closed or the run was terminated before completion. Restarted in interrupted state.")
    );

    // Verify completed run is untouched
    let ok = sqlx::query("SELECT status FROM thread_runs WHERE id = 'r-ok'")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(ok.get::<String, _>("status"), "completed");

    // Verify dangling tool calls are now cancelled
    let tc_running =
        sqlx::query("SELECT status, finished_at FROM tool_calls WHERE id = 'tc-running'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(tc_running.get::<String, _>("status"), "cancelled");
    assert!(tc_running.get::<Option<String>, _>("finished_at").is_some());

    let tc_waiting =
        sqlx::query("SELECT status, finished_at FROM tool_calls WHERE id = 'tc-waiting'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(tc_waiting.get::<String, _>("status"), "cancelled");
    assert!(tc_waiting.get::<Option<String>, _>("finished_at").is_some());

    // Verify completed tool call is untouched
    let tc_ok = sqlx::query("SELECT status FROM tool_calls WHERE id = 'tc-completed'")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(tc_ok.get::<String, _>("status"), "completed");

    // Verify dangling run helper is now interrupted
    let h_running = sqlx::query(
        "SELECT status, error_summary, finished_at FROM run_helpers WHERE id = 'h-running'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(h_running.get::<String, _>("status"), "interrupted");
    assert_eq!(
        h_running
            .get::<Option<String>, _>("error_summary")
            .as_deref(),
        Some("The app closed before this helper finished. Marked as interrupted on restart.")
    );
    assert!(h_running.get::<Option<String>, _>("finished_at").is_some());

    // Verify completed run helper is untouched
    let h_ok = sqlx::query("SELECT status FROM run_helpers WHERE id = 'h-completed'")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(h_ok.get::<String, _>("status"), "completed");
}

// =========================================================================
// T1.5.3 — Active runs index (only non-terminal runs)
// =========================================================================

#[tokio::test]
async fn test_active_runs_index() {
    let pool = test_helpers::setup_test_pool().await;
    test_helpers::seed_workspace(&pool, "ws-idx", "/tmp/idx").await;
    test_helpers::seed_thread(&pool, "t-idx", "ws-idx", None).await;

    test_helpers::seed_run(&pool, "r-active", "t-idx", "running", "default").await;
    test_helpers::seed_run(&pool, "r-done", "t-idx", "completed", "default").await;
    test_helpers::seed_run(&pool, "r-wait", "t-idx", "waiting_approval", "default").await;

    // Query active runs using the partial index pattern
    let rows = sqlx::query(
        "SELECT id FROM thread_runs WHERE thread_id = 't-idx'
         AND status NOT IN ('completed', 'failed', 'denied', 'interrupted', 'cancelled')",
    )
    .fetch_all(&pool)
    .await
    .unwrap();

    let ids: Vec<String> = rows.iter().map(|r| r.get("id")).collect();
    assert!(ids.contains(&"r-active".to_string()));
    assert!(ids.contains(&"r-wait".to_string()));
    assert!(!ids.contains(&"r-done".to_string()));
}

// =========================================================================
// T1.5.4 — One active run per thread constraint
// =========================================================================

#[tokio::test]
async fn test_only_one_active_run_per_thread_check() {
    let pool = test_helpers::setup_test_pool().await;
    test_helpers::seed_workspace(&pool, "ws-1run", "/tmp/1run").await;
    test_helpers::seed_thread(&pool, "t-1run", "ws-1run", None).await;
    test_helpers::seed_run(&pool, "r-existing", "t-1run", "running", "default").await;

    // Application-level check: count active runs before starting a new one
    let active_count = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM thread_runs WHERE thread_id = 't-1run'
         AND status NOT IN ('completed', 'failed', 'denied', 'interrupted', 'cancelled')",
    )
    .fetch_one(&pool)
    .await
    .unwrap();

    assert_eq!(active_count, 1, "Should have exactly 1 active run");
    // Application code should reject starting a new run when active_count > 0
}

// =========================================================================
// T1.5.5 — Effective model plan freeze
// =========================================================================

#[tokio::test]
async fn test_effective_model_plan_stored() {
    let pool = test_helpers::setup_test_pool().await;
    test_helpers::seed_workspace(&pool, "ws-mp", "/tmp/mp").await;
    test_helpers::seed_thread(&pool, "t-mp", "ws-mp", None).await;

    let model_plan = r#"{"primary":{"provider":"openai","model":"gpt-4"},"auxiliary":{"provider":"anthropic","model":"claude-3"}}"#;

    sqlx::query(
        "INSERT INTO thread_runs (id, thread_id, run_mode, status, effective_model_plan_json)
         VALUES ('r-mp', 't-mp', 'default', 'running', ?)",
    )
    .bind(model_plan)
    .execute(&pool)
    .await
    .unwrap();

    let row = sqlx::query("SELECT effective_model_plan_json FROM thread_runs WHERE id = 'r-mp'")
        .fetch_one(&pool)
        .await
        .unwrap();

    let plan: serde_json::Value =
        serde_json::from_str(&row.get::<String, _>("effective_model_plan_json")).unwrap();
    assert_eq!(plan["primary"]["model"].as_str().unwrap(), "gpt-4");
}

// =========================================================================
// T1.5.6 — Built-in runtime session configuration
// =========================================================================

#[tokio::test]
async fn test_build_session_spec_resolves_primary_model_and_profile_prompt() {
    use tiycode_lib::core::agent_session::build_session_spec;

    let pool = test_helpers::setup_test_pool().await;
    test_helpers::seed_workspace(&pool, "ws-runtime", "/tmp/runtime").await;
    test_helpers::seed_thread(&pool, "t-runtime", "ws-runtime", None).await;
    test_helpers::seed_message(
        &pool,
        "m-runtime",
        "t-runtime",
        "user",
        "Explain this project",
    )
    .await;

    sqlx::query(
        "INSERT INTO providers (
            id, provider_kind, provider_key, name, protocol_type, base_url,
            api_key_encrypted, enabled, mapping_locked
         ) VALUES ('prov-runtime', 'builtin', 'openai', 'OpenAI', 'openai',
                   'https://api.openai.com/v1', 'sk-test', 1, 1)",
    )
    .execute(&pool)
    .await
    .unwrap();

    sqlx::query(
        "INSERT INTO agent_profiles (id, name, custom_instructions, primary_provider_id, primary_model_id, is_default)
         VALUES ('profile-runtime', 'Runtime Profile', 'Always answer in concise engineering prose.', 'prov-runtime', 'model-record-runtime', 1)",
    )
    .execute(&pool)
    .await
    .unwrap();

    let model_plan = serde_json::json!({
        "profileId": "profile-runtime",
        "primary": {
            "providerId": "prov-runtime",
            "modelRecordId": "model-record-runtime",
            "providerType": "openai",
            "providerName": "OpenAI",
            "model": "gpt-4.1",
            "modelId": "gpt-4.1",
            "modelDisplayName": "GPT-4.1",
            "baseUrl": "https://api.openai.com/v1",
            "contextWindow": "128000",
            "maxOutputTokens": "16384"
        }
    });

    let spec = build_session_spec(
        &pool,
        "run-runtime",
        "t-runtime",
        "/tmp/runtime",
        "default",
        &model_plan,
    )
    .await
    .unwrap();

    assert_eq!(spec.run_id, "run-runtime");
    assert_eq!(spec.model_plan.primary.model.id, "gpt-4.1");
    assert_eq!(spec.model_plan.primary.provider_id, "prov-runtime");
    assert_eq!(spec.model_plan.primary.api_key.as_deref(), Some("sk-test"));
    assert_eq!(spec.tool_profile_name, "default_full");
    assert!(spec
        .system_prompt
        .contains("Always answer in concise engineering prose."));
    assert!(spec.system_prompt.contains("Use agent_explore"));
    assert!(spec
        .system_prompt
        .contains("Use update_plan to publish the current implementation plan"));
    assert!(spec
        .system_prompt
        .contains("Do not use update_plan for pure analysis"));
    assert_eq!(spec.history_messages.len(), 1);
}

#[tokio::test]
async fn test_build_session_spec_uses_runtime_custom_instructions_when_profile_lookup_misses() {
    use tiycode_lib::core::agent_session::build_session_spec;

    let pool = test_helpers::setup_test_pool().await;
    test_helpers::seed_workspace(&pool, "ws-runtime-inline", "/tmp/runtime-inline").await;
    test_helpers::seed_thread(&pool, "t-runtime-inline", "ws-runtime-inline", None).await;

    sqlx::query(
        "INSERT INTO providers (
            id, provider_kind, provider_key, name, protocol_type, base_url,
            api_key_encrypted, enabled, mapping_locked
         ) VALUES ('prov-runtime-inline', 'builtin', 'openai', 'OpenAI', 'openai',
                   'https://api.openai.com/v1', 'sk-test', 1, 1)",
    )
    .execute(&pool)
    .await
    .unwrap();

    let model_plan = serde_json::json!({
        "profileId": "missing-profile",
        "profileName": "Missing Profile",
        "customInstructions": "Always include the user's runtime custom instructions.",
        "responseLanguage": "简体中文",
        "responseStyle": "concise",
        "primary": {
            "providerId": "prov-runtime-inline",
            "modelRecordId": "model-record-runtime-inline",
            "providerType": "openai",
            "providerName": "OpenAI",
            "model": "gpt-4.1",
            "modelId": "gpt-4.1",
            "modelDisplayName": "GPT-4.1",
            "baseUrl": "https://api.openai.com/v1"
        }
    });

    let spec = build_session_spec(
        &pool,
        "run-runtime-inline",
        "t-runtime-inline",
        "/tmp/runtime-inline",
        "default",
        &model_plan,
    )
    .await
    .unwrap();

    assert!(spec.system_prompt.contains("## Profile Instructions"));
    assert!(spec
        .system_prompt
        .contains("Always include the user's runtime custom instructions."));
    assert!(spec
        .system_prompt
        .contains("Respond in 简体中文 unless the user explicitly asks for a different language."));
    assert!(spec.system_prompt.contains("Response style: concise."));
}

#[tokio::test]
async fn test_build_session_spec_adds_plan_mode_guardrails() {
    use tiycode_lib::core::agent_session::build_session_spec;

    let pool = test_helpers::setup_test_pool().await;
    test_helpers::seed_workspace(&pool, "ws-plan", "/tmp/plan").await;
    test_helpers::seed_thread(&pool, "t-plan", "ws-plan", None).await;
    test_helpers::seed_message(
        &pool,
        "m-plan",
        "t-plan",
        "user",
        "Draft an implementation plan",
    )
    .await;

    sqlx::query(
        "INSERT INTO providers (
            id, provider_kind, provider_key, name, protocol_type, base_url,
            api_key_encrypted, enabled, mapping_locked
         ) VALUES ('prov-plan', 'builtin', 'openai', 'OpenAI', 'openai',
                   'https://api.openai.com/v1', 'sk-test', 1, 1)",
    )
    .execute(&pool)
    .await
    .unwrap();

    let model_plan = serde_json::json!({
        "primary": {
            "providerId": "prov-plan",
            "modelRecordId": "model-record-plan",
            "providerType": "openai",
            "providerName": "OpenAI",
            "model": "gpt-4.1-mini",
            "modelId": "gpt-4.1-mini",
            "modelDisplayName": "GPT-4.1 Mini",
            "baseUrl": "https://api.openai.com/v1"
        }
    });

    let spec = build_session_spec(
        &pool,
        "run-plan",
        "t-plan",
        "/tmp/plan",
        "plan",
        &model_plan,
    )
    .await
    .unwrap();

    assert_eq!(spec.tool_profile_name, "plan_read_only");
    assert!(spec.system_prompt.contains("Plan mode is active."));
    // Phase 4 instructs the model to call update_plan to publish the plan
    assert!(spec
        .system_prompt
        .contains("Call update_plan to publish the formal implementation plan"));
    // Quality contract covers key sections
    assert!(spec
        .system_prompt
        .contains("`summary`: State what is being changed"));
    assert!(spec
        .system_prompt
        .contains("Write a thorough narrative of confirmed facts"));
    assert!(spec.system_prompt.contains(
        "Write a connected prose description of the specific files, modules, interfaces, data flows, or state transitions"
    ));
    assert!(spec.system_prompt.contains("pauses for user approval"));
}

#[tokio::test]
async fn test_build_session_spec_keeps_reasoning_disabled_when_thinking_level_is_off() {
    use tiycode_lib::core::agent_session::build_session_spec;
    use tiycore::thinking::ThinkingLevel;

    let pool = test_helpers::setup_test_pool().await;
    test_helpers::seed_workspace(&pool, "ws-thinking-off", "/tmp/thinking-off").await;
    test_helpers::seed_thread(&pool, "t-thinking-off", "ws-thinking-off", None).await;

    sqlx::query(
        "INSERT INTO providers (
            id, provider_kind, provider_key, name, protocol_type, base_url,
            api_key_encrypted, enabled, mapping_locked
         ) VALUES ('prov-thinking-off', 'builtin', 'openai', 'OpenAI', 'openai',
                   'https://api.openai.com/v1', 'sk-test', 1, 1)",
    )
    .execute(&pool)
    .await
    .unwrap();

    let model_plan = serde_json::json!({
        "thinkingLevel": "off",
        "primary": {
            "providerId": "prov-thinking-off",
            "modelRecordId": "model-record-thinking-off",
            "providerType": "openai",
            "providerName": "OpenAI",
            "model": "gpt-5",
            "modelId": "gpt-5",
            "modelDisplayName": "GPT-5",
            "baseUrl": "https://api.openai.com/v1"
        }
    });

    let spec = build_session_spec(
        &pool,
        "run-thinking-off",
        "t-thinking-off",
        "/tmp/thinking-off",
        "default",
        &model_plan,
    )
    .await
    .unwrap();

    assert_eq!(spec.model_plan.thinking_level, ThinkingLevel::Off);
    assert!(!spec.model_plan.primary.model.reasoning);
}

#[tokio::test]
async fn test_build_session_spec_enables_reasoning_when_thinking_level_is_set() {
    use tiycode_lib::core::agent_session::build_session_spec;
    use tiycore::thinking::ThinkingLevel;

    let pool = test_helpers::setup_test_pool().await;
    test_helpers::seed_workspace(&pool, "ws-thinking-on", "/tmp/thinking-on").await;
    test_helpers::seed_thread(&pool, "t-thinking-on", "ws-thinking-on", None).await;

    sqlx::query(
        "INSERT INTO providers (
            id, provider_kind, provider_key, name, protocol_type, base_url,
            api_key_encrypted, enabled, mapping_locked
         ) VALUES ('prov-thinking-on', 'builtin', 'openai', 'OpenAI', 'openai',
                   'https://api.openai.com/v1', 'sk-test', 1, 1)",
    )
    .execute(&pool)
    .await
    .unwrap();

    let model_plan = serde_json::json!({
        "thinkingLevel": "minimal",
        "primary": {
            "providerId": "prov-thinking-on",
            "modelRecordId": "model-record-thinking-on",
            "providerType": "openai",
            "providerName": "OpenAI",
            "model": "gpt-5",
            "modelId": "gpt-5",
            "modelDisplayName": "GPT-5",
            "baseUrl": "https://api.openai.com/v1",
            "supportsReasoning": true
        }
    });

    let spec = build_session_spec(
        &pool,
        "run-thinking-on",
        "t-thinking-on",
        "/tmp/thinking-on",
        "default",
        &model_plan,
    )
    .await
    .unwrap();

    assert_eq!(spec.model_plan.thinking_level, ThinkingLevel::Minimal);
    assert!(spec.model_plan.primary.model.reasoning);
}

#[tokio::test]
async fn test_build_session_spec_defaults_openai_compatible_to_system_role_compat() {
    use tiycode_lib::core::agent_session::build_session_spec;

    let pool = test_helpers::setup_test_pool().await;
    test_helpers::seed_workspace(&pool, "ws-openai-compat", "/tmp/openai-compat").await;
    test_helpers::seed_thread(&pool, "t-openai-compat", "ws-openai-compat", None).await;

    sqlx::query(
        "INSERT INTO providers (
            id, provider_kind, provider_key, name, protocol_type, base_url,
            api_key_encrypted, enabled, mapping_locked
         ) VALUES ('prov-openai-compat', 'custom', 'custom-openai-compat', 'My Gateway', 'openai-compatible',
                   'https://gateway.example.com/v1', 'sk-test', 1, 0)",
    )
    .execute(&pool)
    .await
    .unwrap();

    let model_plan = serde_json::json!({
        "thinkingLevel": "minimal",
        "primary": {
            "providerId": "prov-openai-compat",
            "modelRecordId": "model-record-openai-compat",
            "providerType": "openai-compatible",
            "providerName": "My Gateway",
            "model": "gpt-5-compatible",
            "modelId": "gpt-5-compatible",
            "modelDisplayName": "GPT-5 Compatible",
            "baseUrl": "https://gateway.example.com/v1",
            "supportsReasoning": true
        }
    });

    let spec = build_session_spec(
        &pool,
        "run-openai-compat",
        "t-openai-compat",
        "/tmp/openai-compat",
        "default",
        &model_plan,
    )
    .await
    .unwrap();

    let compat = spec
        .model_plan
        .primary
        .model
        .compat
        .as_ref()
        .expect("openai-compatible models should set explicit compat defaults");

    assert!(spec.model_plan.primary.model.reasoning);
    assert!(!compat.supports_developer_role);
}

#[tokio::test]
async fn test_build_session_spec_includes_structured_runtime_context_sections() {
    use tiycode_lib::core::agent_session::build_session_spec;

    let pool = test_helpers::setup_test_pool().await;
    let temp_dir = tempdir().unwrap();
    let workspace_path = temp_dir.path().to_string_lossy().to_string();

    fs::write(temp_dir.path().join("CLAUDE.md"), "Claude instructions").unwrap();
    fs::write(temp_dir.path().join("AGENTS.md"), "Agents instructions").unwrap();

    test_helpers::seed_workspace(&pool, "ws-ctx", &workspace_path).await;
    test_helpers::seed_thread(&pool, "t-ctx", "ws-ctx", None).await;
    test_helpers::seed_message(&pool, "m-ctx", "t-ctx", "user", "Inspect the setup").await;
    test_helpers::seed_policy(&pool, "approval_policy", r#""require_all""#).await;

    sqlx::query(
        "INSERT INTO providers (
            id, provider_kind, provider_key, name, protocol_type, base_url,
            api_key_encrypted, enabled, mapping_locked
         ) VALUES ('prov-ctx', 'builtin', 'openai', 'OpenAI', 'openai',
                   'https://api.openai.com/v1', 'sk-test', 1, 1)",
    )
    .execute(&pool)
    .await
    .unwrap();

    let model_plan = serde_json::json!({
        "primary": {
            "providerId": "prov-ctx",
            "modelRecordId": "model-record-ctx",
            "providerType": "openai",
            "providerName": "OpenAI",
            "model": "gpt-4.1-mini",
            "modelId": "gpt-4.1-mini",
            "modelDisplayName": "GPT-4.1 Mini",
            "baseUrl": "https://api.openai.com/v1"
        }
    });

    let spec = build_session_spec(
        &pool,
        "run-ctx",
        "t-ctx",
        &workspace_path,
        "default",
        &model_plan,
    )
    .await
    .unwrap();

    assert!(spec
        .system_prompt
        .contains("## Project Context (workspace instructions)"));
    assert!(spec.system_prompt.contains("### AGENTS.md"));
    assert!(spec
        .system_prompt
        .contains("### AGENTS.md\n```md\nAgents instructions"));
    assert!(!spec.system_prompt.contains("```md\n### AGENTS.md"));
    assert!(spec.system_prompt.contains("Agents instructions"));
    assert!(!spec.system_prompt.contains("Claude instructions"));
    assert!(spec.system_prompt.contains(
        "Before taking tool actions or making substantive changes, send a brief, friendly reply"
    ));
    assert!(spec.system_prompt.contains("Read files before editing."));
    assert!(spec.system_prompt.contains("Use `read` to inspect files"));
    assert!(spec
        .system_prompt
        .contains("instead of shell commands such as `cat`, `sed`, or `head`"));
    assert!(spec
        .system_prompt
        .contains("Use `search` to find content and `find` to locate files"));
    assert!(spec
        .system_prompt
        .contains("Use `shell` for one-shot non-interactive commands"));
    assert!(spec
        .system_prompt
        .contains("terminal panel tools only for their dedicated session workflow"));
    assert!(spec
        .system_prompt
        .contains("Flag risks, destructive operations, or ambiguity before acting."));
    assert!(spec
        .system_prompt
        .contains("Do not rerun the same verification commands yourself unless the helper explicitly could not run them"));
    assert!(spec.system_prompt.contains("When the user's goal is clear"));
    assert!(spec
        .system_prompt
        .contains("low-risk, local, and reversible"));
    assert!(spec
        .system_prompt
        .contains("move forward without unnecessary clarification"));
    assert!(spec
        .system_prompt
        .contains("Do not use clarify to offload work"));
    assert!(spec
        .system_prompt
        .contains("Use update_plan before implementation"));
    assert!(spec.system_prompt.contains("complex, cross-file, or risky"));
    assert!(spec
        .system_prompt
        .contains("scope decision is still unresolved"));
    assert!(spec.system_prompt.contains("before publishing update_plan"));
    assert!(spec.system_prompt.contains("## System Environment"));
    assert!(spec.system_prompt.contains("## Sandbox & Permissions"));
    assert!(spec.system_prompt.contains("Approval policy: require_all."));
    assert!(spec.system_prompt.contains("## Shell Tooling Guide"));
    assert!(spec
        .system_prompt
        .contains("This section is a shell command selection and boundary guide."));
    assert!(spec
        .system_prompt
        .contains("Use `term_status`, `term_output`, `term_write`"));
    assert!(spec
        .system_prompt
        .contains("do not replace one-shot `shell` execution"));
    assert!(spec
        .system_prompt
        .contains(&format!("Workspace path: {workspace_path}")));
}

#[tokio::test]
async fn test_build_session_spec_reads_object_style_approval_policy() {
    use tiycode_lib::core::agent_session::build_session_spec;

    let pool = test_helpers::setup_test_pool().await;
    let temp_dir = tempdir().unwrap();
    let workspace_path = temp_dir.path().to_string_lossy().to_string();

    test_helpers::seed_workspace(&pool, "ws-ctx-object", &workspace_path).await;
    test_helpers::seed_thread(&pool, "t-ctx-object", "ws-ctx-object", None).await;
    test_helpers::seed_policy(&pool, "approval_policy", r#"{"mode":"require_all"}"#).await;

    sqlx::query(
        "INSERT INTO providers (
            id, provider_kind, provider_key, name, protocol_type, base_url,
            api_key_encrypted, enabled, mapping_locked
         ) VALUES ('prov-ctx-object', 'builtin', 'openai', 'OpenAI', 'openai',
                   'https://api.openai.com/v1', 'sk-test', 1, 1)",
    )
    .execute(&pool)
    .await
    .unwrap();

    let model_plan = serde_json::json!({
        "primary": {
            "providerId": "prov-ctx-object",
            "modelRecordId": "model-record-ctx-object",
            "providerType": "openai",
            "providerName": "OpenAI",
            "model": "gpt-4.1-mini",
            "modelId": "gpt-4.1-mini",
            "modelDisplayName": "GPT-4.1 Mini",
            "baseUrl": "https://api.openai.com/v1"
        }
    });

    let spec = build_session_spec(
        &pool,
        "run-ctx-object",
        "t-ctx-object",
        &workspace_path,
        "default",
        &model_plan,
    )
    .await
    .unwrap();

    assert!(spec.system_prompt.contains("Approval policy: require_all."));
}

#[tokio::test]
async fn test_run_helpers_table_persists_collapsed_helper_summary() {
    let pool = test_helpers::setup_test_pool().await;
    test_helpers::seed_workspace(&pool, "ws-helper", "/tmp/helper").await;
    test_helpers::seed_thread(&pool, "t-helper", "ws-helper", None).await;
    test_helpers::seed_run(&pool, "r-helper", "t-helper", "running", "default").await;

    sqlx::query(
        "INSERT INTO run_helpers (
            id, run_id, thread_id, helper_kind, status, model_role, provider_id, model_id,
            input_summary, output_summary
         ) VALUES (
            'helper-1', 'r-helper', 't-helper', 'helper_explore', 'completed', 'assistant',
            'prov-helper', 'gpt-4.1-mini', 'Inspect the repository layout', 'Repository layout summarized'
         )",
    )
    .execute(&pool)
    .await
    .unwrap();

    let row = sqlx::query(
        "SELECT helper_kind, status, output_summary FROM run_helpers WHERE id = 'helper-1'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();

    assert_eq!(row.get::<String, _>("helper_kind"), "helper_explore");
    assert_eq!(row.get::<String, _>("status"), "completed");
    assert_eq!(
        row.get::<Option<String>, _>("output_summary").unwrap(),
        "Repository layout summarized"
    );
}

// =========================================================================
// T1.5.10 — render tool execution integration test
// =========================================================================

/// Validates the complete render data flow: tool_call persistence,
/// chart artifact merge into message parts, and expected DB state.
#[tokio::test]
async fn test_render_tool_persists_tool_call_and_chart_artifact() {
    let pool = test_helpers::setup_test_pool().await;
    test_helpers::seed_workspace(&pool, "ws-chart", "/tmp/chart").await;
    test_helpers::seed_thread(&pool, "t-chart", "ws-chart", None).await;
    test_helpers::seed_run(&pool, "r-chart", "t-chart", "running", "default").await;
    test_helpers::seed_message(
        &pool,
        "msg-chart-target",
        "t-chart",
        "assistant",
        "Here is the analysis.",
    )
    .await;

    // Simulate the render tool execution flow:
    // Step 1: Insert tool call (matches execute_render logic)
    let tool_call_id = "tc-chart-001";
    let tool_input = serde_json::json!({
        "spec": { "mark": "bar", "encoding": { "x": { "field": "category" }, "y": { "field": "value" } } },
        "title": "Revenue by Category",
        "caption": "Q1 2026 data",
        "library": "vega-lite"
    });

    tiycode_lib::persistence::repo::tool_call_repo::insert(
        &pool,
        &tiycode_lib::persistence::repo::tool_call_repo::ToolCallInsert {
            id: "tc-store-chart-001".to_string(),
            tool_call_id: tool_call_id.to_string(),
            run_id: "r-chart".to_string(),
            thread_id: "t-chart".to_string(),
            helper_id: None,
            tool_name: "render".to_string(),
            tool_input_json: tool_input.to_string(),
            status: "requested".to_string(),
        },
    )
    .await
    .expect("tool_call insert should succeed");

    // Step 2: Merge chart artifact into message parts
    let artifact_id = "art-chart-001";
    let chart_payload = serde_json::json!({
        "library": "vega-lite",
        "spec": tool_input["spec"],
        "title": "Revenue by Category",
        "caption": "Q1 2026 data",
        "status": "ready",
    });

    tiycode_lib::persistence::repo::message_repo::merge_chart_artifact_part(
        &pool,
        "msg-chart-target",
        artifact_id,
        chart_payload,
    )
    .await
    .expect("merge_chart_artifact_part should succeed");

    // Step 3: Mark tool call as completed
    let result_json = serde_json::json!({
        "success": true,
        "artifactId": artifact_id,
        "messageId": "msg-chart-target",
        "library": "vega-lite",
        "title": "Revenue by Category",
        "caption": "Q1 2026 data",
    });
    tiycode_lib::persistence::repo::tool_call_repo::update_result(
        &pool,
        "tc-store-chart-001",
        &result_json.to_string(),
        "completed",
    )
    .await
    .expect("tool_call update should succeed");

    // Verify: tool call is persisted correctly
    let tc_row = sqlx::query(
        "SELECT tool_name, status, tool_output_json FROM tool_calls WHERE id = 'tc-store-chart-001'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(tc_row.get::<String, _>("tool_name"), "render");
    assert_eq!(tc_row.get::<String, _>("status"), "completed");
    let stored_result: serde_json::Value =
        serde_json::from_str(&tc_row.get::<String, _>("tool_output_json")).unwrap();
    assert_eq!(stored_result["success"].as_bool().unwrap(), true);
    assert_eq!(
        stored_result["artifactId"].as_str().unwrap(),
        "art-chart-001"
    );

    // Verify: message parts contain the chart artifact
    let msg_row = sqlx::query(
        "SELECT parts_json, content_markdown FROM messages WHERE id = 'msg-chart-target'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();

    let parts: Vec<serde_json::Value> =
        serde_json::from_str(&msg_row.get::<String, _>("parts_json")).unwrap();

    assert_eq!(parts.len(), 2, "should have text + chart parts");
    assert_eq!(parts[0]["type"].as_str().unwrap(), "text");
    assert_eq!(parts[0]["text"].as_str().unwrap(), "Here is the analysis.");
    assert_eq!(parts[1]["type"].as_str().unwrap(), "chart");
    assert_eq!(parts[1]["artifactId"].as_str().unwrap(), "art-chart-001");
    assert_eq!(parts[1]["library"].as_str().unwrap(), "vega-lite");
    assert_eq!(parts[1]["title"].as_str().unwrap(), "Revenue by Category");
    assert_eq!(parts[1]["caption"].as_str().unwrap(), "Q1 2026 data");
    assert_eq!(parts[1]["status"].as_str().unwrap(), "ready");
}

/// Validates that render validation failure still persists the tool call
/// with failed status, matching the execute_render error path.
#[tokio::test]
async fn test_render_validation_failure_persists_failed_tool_call() {
    let pool = test_helpers::setup_test_pool().await;
    test_helpers::seed_workspace(&pool, "ws-chart-fail", "/tmp/chart-fail").await;
    test_helpers::seed_thread(&pool, "t-chart-fail", "ws-chart-fail", None).await;
    test_helpers::seed_run(&pool, "r-chart-fail", "t-chart-fail", "running", "default").await;

    // Simulate: tool call persisted first (before validation)
    let tool_input = serde_json::json!({
        "spec": "not an object"
    });

    tiycode_lib::persistence::repo::tool_call_repo::insert(
        &pool,
        &tiycode_lib::persistence::repo::tool_call_repo::ToolCallInsert {
            id: "tc-store-chart-fail".to_string(),
            tool_call_id: "tc-chart-fail".to_string(),
            run_id: "r-chart-fail".to_string(),
            thread_id: "t-chart-fail".to_string(),
            helper_id: None,
            tool_name: "render".to_string(),
            tool_input_json: tool_input.to_string(),
            status: "requested".to_string(),
        },
    )
    .await
    .expect("tool_call insert should succeed");

    // Validation fails → update to failed status
    let error = "render with library 'vega-lite' requires a valid 'spec' object";
    let error_json = serde_json::json!({ "error": error });
    tiycode_lib::persistence::repo::tool_call_repo::update_result(
        &pool,
        "tc-store-chart-fail",
        &error_json.to_string(),
        "failed",
    )
    .await
    .expect("tool_call update_result should succeed");

    // Verify: tool call marked as failed
    let row = sqlx::query(
        "SELECT status, tool_output_json FROM tool_calls WHERE id = 'tc-store-chart-fail'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(row.get::<String, _>("status"), "failed");
    let result: serde_json::Value =
        serde_json::from_str(&row.get::<String, _>("tool_output_json")).unwrap();
    assert!(result["error"]
        .as_str()
        .unwrap()
        .contains("valid 'spec' object"));
}
