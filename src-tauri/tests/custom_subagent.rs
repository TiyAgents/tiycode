//! Integration tests for custom subagent CRUD and profile access.

use sqlx::sqlite::{SqliteConnectOptions, SqlitePool, SqlitePoolOptions};
use std::str::FromStr;
use tiycode_lib::model::subagent::{CustomSubagentInput, CustomSubagentModelRole};

async fn setup_test_pool() -> SqlitePool {
    let options = SqliteConnectOptions::from_str("sqlite::memory:")
        .expect("invalid sqlite options")
        .foreign_keys(true);

    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(options)
        .await
        .expect("failed to create in-memory pool");

    sqlx::migrate!("./migrations")
        .run(&pool)
        .await
        .expect("migrations failed");

    pool
}

#[tokio::test]
async fn custom_subagent_crud_lifecycle() {
    use tiycode_lib::persistence::repo::custom_subagent_repo;

    let pool = setup_test_pool().await;

    // Create
    let input = CustomSubagentInput {
        name: "Refactor Agent".to_string(),
        slug: "refactor".to_string(),
        system_prompt: "You are a refactoring helper.".to_string(),
        invocation_description: "Use when code needs refactoring.".to_string(),
        allowed_tools: vec!["read".to_string(), "edit".to_string(), "search".to_string()],
        model_role: CustomSubagentModelRole::Primary,
        is_enabled: Some(true),
        can_delegate: None,
        max_delegation_depth: None,
    };
    let created = custom_subagent_repo::create(&pool, &input)
        .await
        .expect("create should succeed");
    assert_eq!(created.name, "Refactor Agent");
    assert_eq!(created.slug, "refactor");
    assert_eq!(created.model_role, CustomSubagentModelRole::Primary);
    assert!(created.is_enabled);

    // Get by ID
    let found = custom_subagent_repo::get_by_id(&pool, &created.id)
        .await
        .expect("get_by_id should succeed")
        .expect("should find created record");
    assert_eq!(found.slug, "refactor");
    assert_eq!(found.model_role, CustomSubagentModelRole::Primary);
    assert_eq!(found.allowed_tools_vec(), vec!["read", "edit", "search"]);

    // Get by slug
    let found_by_slug = custom_subagent_repo::get_by_slug(&pool, "refactor")
        .await
        .expect("get_by_slug should succeed")
        .expect("should find by slug");
    assert_eq!(found_by_slug.id, created.id);

    // List all
    let all = custom_subagent_repo::list_all(&pool)
        .await
        .expect("list_all should succeed");
    assert_eq!(all.len(), 1);

    // Update
    let update_input = CustomSubagentInput {
        name: "Refactor Agent V2".to_string(),
        slug: "refactor".to_string(),
        system_prompt: "You are an improved refactoring helper.".to_string(),
        invocation_description: "Use when code needs refactoring (v2).".to_string(),
        allowed_tools: vec!["read".to_string(), "edit".to_string(), "write".to_string()],
        model_role: CustomSubagentModelRole::Lightweight,
        is_enabled: Some(false),
        can_delegate: None,
        max_delegation_depth: None,
    };
    let updated = custom_subagent_repo::update(&pool, &created.id, &update_input)
        .await
        .expect("update should succeed");
    assert_eq!(updated.name, "Refactor Agent V2");
    assert_eq!(updated.model_role, CustomSubagentModelRole::Lightweight);
    assert!(!updated.is_enabled);

    // Delete
    let deleted = custom_subagent_repo::delete(&pool, &created.id)
        .await
        .expect("delete should succeed");
    assert!(deleted);

    // Verify deleted
    let not_found = custom_subagent_repo::get_by_id(&pool, &created.id)
        .await
        .expect("get_by_id after delete should succeed");
    assert!(not_found.is_none());
}

#[tokio::test]
async fn slug_uniqueness_constraint() {
    use tiycode_lib::persistence::repo::custom_subagent_repo;

    let pool = setup_test_pool().await;

    let input = CustomSubagentInput {
        name: "Agent A".to_string(),
        slug: "my-agent".to_string(),
        system_prompt: "prompt".to_string(),
        invocation_description: "desc".to_string(),
        allowed_tools: vec![],
        model_role: CustomSubagentModelRole::Auxiliary,
        is_enabled: Some(true),
        can_delegate: None,
        max_delegation_depth: None,
    };
    custom_subagent_repo::create(&pool, &input)
        .await
        .expect("first create should succeed");

    // Duplicate slug should fail
    let result = custom_subagent_repo::create(&pool, &input).await;
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(err.error_code.contains("slug_conflict"));
}

#[tokio::test]
async fn profile_subagent_access_set_and_get() {
    use tiycode_lib::model::provider::AgentProfileRecord;
    use tiycode_lib::persistence::repo::{custom_subagent_repo, profile_repo};

    let pool = setup_test_pool().await;

    // Create a profile
    let profile = AgentProfileRecord {
        id: "test-profile-1".to_string(),
        name: "Test Profile".to_string(),
        custom_instructions: None,
        commit_message_prompt: None,
        response_style: Some("concise".to_string()),
        response_language: None,
        commit_message_language: None,
        thinking_level: None,
        primary_provider_id: None,
        primary_model_id: None,
        auxiliary_provider_id: None,
        auxiliary_model_id: None,
        lightweight_provider_id: None,
        lightweight_model_id: None,
        is_default: true,
        created_at: "".to_string(),
        updated_at: "".to_string(),
    };
    profile_repo::insert(&pool, &profile)
        .await
        .expect("insert profile should succeed");

    // Create subagents
    let input_a = CustomSubagentInput {
        name: "Agent A".to_string(),
        slug: "agent-a".to_string(),
        system_prompt: "A".to_string(),
        invocation_description: "A".to_string(),
        allowed_tools: vec!["read".to_string()],
        model_role: CustomSubagentModelRole::Auxiliary,
        is_enabled: Some(true),
        can_delegate: None,
        max_delegation_depth: None,
    };
    let input_b = CustomSubagentInput {
        name: "Agent B".to_string(),
        slug: "agent-b".to_string(),
        system_prompt: "B".to_string(),
        invocation_description: "B".to_string(),
        allowed_tools: vec!["read".to_string()],
        model_role: CustomSubagentModelRole::Auxiliary,
        is_enabled: Some(true),
        can_delegate: None,
        max_delegation_depth: None,
    };
    let a = custom_subagent_repo::create(&pool, &input_a).await.unwrap();
    let b = custom_subagent_repo::create(&pool, &input_b).await.unwrap();

    // Set access
    custom_subagent_repo::set_profile_access(
        &pool,
        "test-profile-1",
        &[a.id.clone(), b.id.clone()],
    )
    .await
    .expect("set_profile_access should succeed");

    // Get access
    let access = custom_subagent_repo::get_profile_access(&pool, "test-profile-1")
        .await
        .expect("get_profile_access should succeed");
    assert_eq!(access.len(), 2);
    assert!(access.contains(&a.id));
    assert!(access.contains(&b.id));

    // List for profile
    let for_profile = custom_subagent_repo::list_for_profile(&pool, "test-profile-1")
        .await
        .expect("list_for_profile should succeed");
    assert_eq!(for_profile.len(), 2);
    assert!(for_profile
        .iter()
        .all(|record| record.model_role == CustomSubagentModelRole::Auxiliary));

    // Update access (remove one)
    custom_subagent_repo::set_profile_access(&pool, "test-profile-1", &[a.id.clone()])
        .await
        .expect("update access should succeed");
    let access_after = custom_subagent_repo::get_profile_access(&pool, "test-profile-1")
        .await
        .unwrap();
    assert_eq!(access_after.len(), 1);
    assert_eq!(access_after[0], a.id);
}

#[tokio::test]
async fn cascade_delete_subagent_removes_access() {
    use tiycode_lib::model::provider::AgentProfileRecord;
    use tiycode_lib::persistence::repo::{custom_subagent_repo, profile_repo};

    let pool = setup_test_pool().await;

    // Setup
    let profile = AgentProfileRecord {
        id: "cascade-profile".to_string(),
        name: "Cascade Test".to_string(),
        custom_instructions: None,
        commit_message_prompt: None,
        response_style: None,
        response_language: None,
        commit_message_language: None,
        thinking_level: None,
        primary_provider_id: None,
        primary_model_id: None,
        auxiliary_provider_id: None,
        auxiliary_model_id: None,
        lightweight_provider_id: None,
        lightweight_model_id: None,
        is_default: false,
        created_at: "".to_string(),
        updated_at: "".to_string(),
    };
    profile_repo::insert(&pool, &profile).await.unwrap();

    let input = CustomSubagentInput {
        name: "Temp Agent".to_string(),
        slug: "temp-agent".to_string(),
        system_prompt: "temp".to_string(),
        invocation_description: "temp".to_string(),
        allowed_tools: vec![],
        model_role: CustomSubagentModelRole::Auxiliary,
        is_enabled: Some(true),
        can_delegate: None,
        max_delegation_depth: None,
    };
    let agent = custom_subagent_repo::create(&pool, &input).await.unwrap();
    custom_subagent_repo::set_profile_access(&pool, "cascade-profile", &[agent.id.clone()])
        .await
        .unwrap();

    // Delete the subagent — should cascade
    custom_subagent_repo::delete(&pool, &agent.id)
        .await
        .unwrap();

    // Access should be empty now
    let access = custom_subagent_repo::get_profile_access(&pool, "cascade-profile")
        .await
        .unwrap();
    assert!(access.is_empty());
}

#[tokio::test]
async fn slug_validation_rejects_reserved_and_invalid() {
    use tiycode_lib::model::subagent::validate_slug;

    assert!(validate_slug("explore").is_err());
    assert!(validate_slug("review").is_err());
    assert!(validate_slug("").is_err());
    assert!(validate_slug("123abc").is_err()); // starts with digit
    assert!(validate_slug("my agent").is_err()); // space
    assert!(validate_slug("My-Agent").is_err()); // uppercase

    assert!(validate_slug("refactor").is_ok());
    assert!(validate_slug("code-review").is_ok());
    assert!(validate_slug("a123").is_ok());
}

#[tokio::test]
async fn custom_subagent_delegation_fields_persist_and_clamp() {
    use tiycode_lib::persistence::repo::custom_subagent_repo;

    let pool = setup_test_pool().await;

    // Default (None) → can_delegate=false, max_delegation_depth=3.
    let default_input = CustomSubagentInput {
        name: "Default Agent".to_string(),
        slug: "default-agent".to_string(),
        system_prompt: "prompt".to_string(),
        invocation_description: "desc".to_string(),
        allowed_tools: vec!["read".to_string()],
        model_role: CustomSubagentModelRole::Auxiliary,
        is_enabled: Some(true),
        can_delegate: None,
        max_delegation_depth: None,
    };
    let created = custom_subagent_repo::create(&pool, &default_input)
        .await
        .expect("create should succeed");
    assert!(!created.can_delegate);
    assert_eq!(created.max_delegation_depth, 3);

    // Explicit can_delegate=true and an out-of-range depth that must clamp to 5.
    let delegating_input = CustomSubagentInput {
        name: "Delegating Agent".to_string(),
        slug: "delegating-agent".to_string(),
        system_prompt: "prompt".to_string(),
        invocation_description: "desc".to_string(),
        allowed_tools: vec!["read".to_string()],
        model_role: CustomSubagentModelRole::Auxiliary,
        is_enabled: Some(true),
        can_delegate: Some(true),
        max_delegation_depth: Some(99),
    };
    let delegating = custom_subagent_repo::create(&pool, &delegating_input)
        .await
        .expect("create should succeed");
    assert!(delegating.can_delegate);
    assert_eq!(delegating.max_delegation_depth, 5, "depth must clamp to 5");

    // Reload from DB to confirm persistence.
    let reloaded = custom_subagent_repo::get_by_slug(&pool, "delegating-agent")
        .await
        .expect("get_by_slug should succeed")
        .expect("should find record");
    assert!(reloaded.can_delegate);
    assert_eq!(reloaded.max_delegation_depth, 5);

    // Update can lower the depth and toggle can_delegate off; a too-low value clamps to 1.
    let update_input = CustomSubagentInput {
        name: "Delegating Agent".to_string(),
        slug: "delegating-agent".to_string(),
        system_prompt: "prompt".to_string(),
        invocation_description: "desc".to_string(),
        allowed_tools: vec!["read".to_string()],
        model_role: CustomSubagentModelRole::Auxiliary,
        is_enabled: Some(true),
        can_delegate: Some(false),
        max_delegation_depth: Some(0),
    };
    let updated = custom_subagent_repo::update(&pool, &delegating.id, &update_input)
        .await
        .expect("update should succeed");
    assert!(!updated.can_delegate);
    assert_eq!(updated.max_delegation_depth, 1, "depth must clamp to 1");
}

#[tokio::test]
async fn db_check_rejects_out_of_range_delegation_values() {
    // The repo layer clamps inputs, so the DB CHECK constraints are normally
    // unreachable through the public API. This test bypasses the repo with raw
    // SQL to assert the schema-level guards still reject illegal values, guarding
    // against future code paths that write the columns directly.
    let pool = setup_test_pool().await;

    let insert = |id: &str, can_delegate: i64, depth: i64| {
        let sql = "INSERT INTO custom_subagents \
             (id, name, slug, system_prompt, invocation_description, allowed_tools, model_role, is_enabled, can_delegate, max_delegation_depth, created_at, updated_at) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)";
        sqlx::query(sql)
            .bind(id.to_string())
            .bind("Raw")
            .bind(id.to_string())
            .bind("prompt")
            .bind("desc")
            .bind("[]")
            .bind("auxiliary")
            .bind(1_i64)
            .bind(can_delegate)
            .bind(depth)
            .bind("now")
            .bind("now")
            .execute(&pool)
    };

    // depth = 0 violates max_delegation_depth >= 1.
    assert!(
        insert("too-low", 0, 0).await.is_err(),
        "DB CHECK must reject max_delegation_depth = 0"
    );
    // depth = 6 violates max_delegation_depth <= 5.
    assert!(
        insert("too-high", 0, 6).await.is_err(),
        "DB CHECK must reject max_delegation_depth = 6"
    );
    // can_delegate = 2 violates can_delegate IN (0, 1).
    assert!(
        insert("bad-flag", 2, 3).await.is_err(),
        "DB CHECK must reject can_delegate = 2"
    );
    // A legal row still inserts.
    assert!(
        insert("ok-row", 1, 5).await.is_ok(),
        "valid delegation values must insert successfully"
    );
}
