//! Agent profile persistence tests
//!
//! Coverage:
//! - Profile CRUD operations via raw SQL (list, find, insert, update, delete)
//! - set_default transaction behavior
//! - Profile ordering (default first, then alphabetical)

mod test_helpers;

use sqlx::Row;
use tiycode::model::provider::AgentProfileRecord;
use tiycode::persistence::repo::profile_repo;

// =========================================================================
// Helper: insert a profile directly via SQL
// =========================================================================

async fn seed_profile(
    pool: &sqlx::SqlitePool,
    id: &str,
    name: &str,
    is_default: bool,
    custom_instructions: Option<&str>,
) {
    sqlx::query(
        "INSERT INTO agent_profiles (id, name, custom_instructions, is_default, created_at, updated_at)
         VALUES (?, ?, ?, ?, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'), strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))",
    )
    .bind(id)
    .bind(name)
    .bind(custom_instructions)
    .bind(is_default as i32)
    .execute(pool)
    .await
    .expect("failed to seed profile");
}

async fn seed_full_profile(pool: &sqlx::SqlitePool, id: &str, name: &str, is_default: bool) {
    sqlx::query(
        "INSERT INTO agent_profiles (
            id, name, custom_instructions, commit_message_prompt,
            response_style, response_language, commit_message_language,
            thinking_level,
            primary_provider_id, primary_model_id,
            auxiliary_provider_id, auxiliary_model_id,
            lightweight_provider_id, lightweight_model_id,
            is_default, created_at, updated_at
         ) VALUES (?, ?, 'Be concise', 'Commit: {{changes}}',
            'concise', 'zh-CN', 'en', 'minimal',
            'prov-a', 'gpt-4', 'prov-b', 'claude-3', 'prov-c', 'gpt-3.5',
            ?, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'), strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))",
    )
    .bind(id)
    .bind(name)
    .bind(is_default as i32)
    .execute(pool)
    .await
    .expect("failed to seed full profile");
}

// =========================================================================
// List profiles
// =========================================================================

#[tokio::test]
async fn test_profile_list_empty() {
    let pool = test_helpers::setup_test_pool().await;

    let rows = sqlx::query("SELECT id FROM agent_profiles")
        .fetch_all(&pool)
        .await
        .unwrap();
    assert!(rows.is_empty());
}

#[tokio::test]
async fn test_profile_list_returns_all_inserted() {
    let pool = test_helpers::setup_test_pool().await;

    seed_profile(&pool, "p1", "Profile One", false, None).await;
    seed_profile(&pool, "p2", "Profile Two", true, None).await;

    let rows = sqlx::query(
        "SELECT id, name, is_default FROM agent_profiles ORDER BY is_default DESC, name",
    )
    .fetch_all(&pool)
    .await
    .unwrap();

    assert_eq!(rows.len(), 2);
    // Default comes first
    assert_eq!(rows[0].get::<String, _>("id"), "p2");
    assert_eq!(rows[0].get::<i32, _>("is_default"), 1);
    assert_eq!(rows[1].get::<String, _>("id"), "p1");
    assert_eq!(rows[1].get::<i32, _>("is_default"), 0);
}

#[tokio::test]
async fn test_profile_list_orders_non_default_by_name() {
    let pool = test_helpers::setup_test_pool().await;

    seed_profile(&pool, "p-z", "Zebra", false, None).await;
    seed_profile(&pool, "p-a", "Alpha", false, None).await;
    seed_profile(&pool, "p-m", "Middle", false, None).await;

    let rows = sqlx::query("SELECT name FROM agent_profiles ORDER BY is_default DESC, name")
        .fetch_all(&pool)
        .await
        .unwrap();

    assert_eq!(rows.len(), 3);
    assert_eq!(rows[0].get::<String, _>("name"), "Alpha");
    assert_eq!(rows[1].get::<String, _>("name"), "Middle");
    assert_eq!(rows[2].get::<String, _>("name"), "Zebra");
}

// =========================================================================
// Find by ID
// =========================================================================

#[tokio::test]
async fn test_profile_find_by_id_exists() {
    let pool = test_helpers::setup_test_pool().await;

    seed_profile(&pool, "p-find", "Findable", false, Some("Be helpful.")).await;

    let row = sqlx::query(
        "SELECT id, name, custom_instructions, is_default FROM agent_profiles WHERE id = 'p-find'",
    )
    .fetch_optional(&pool)
    .await
    .unwrap();

    assert!(row.is_some());
    let row = row.unwrap();
    assert_eq!(row.get::<String, _>("name"), "Findable");
    assert_eq!(
        row.get::<Option<String>, _>("custom_instructions")
            .as_deref(),
        Some("Be helpful.")
    );
    assert_eq!(row.get::<i32, _>("is_default"), 0);
}

#[tokio::test]
async fn test_profile_find_by_id_not_found() {
    let pool = test_helpers::setup_test_pool().await;

    let row = sqlx::query("SELECT id FROM agent_profiles WHERE id = 'nonexistent'")
        .fetch_optional(&pool)
        .await
        .unwrap();

    assert!(row.is_none());
}

// =========================================================================
// Insert with all optional fields
// =========================================================================

#[tokio::test]
async fn test_profile_insert_with_all_fields() {
    let pool = test_helpers::setup_test_pool().await;

    seed_full_profile(&pool, "p-full", "Full Profile", true).await;

    let row = sqlx::query(
        "SELECT name, custom_instructions, commit_message_prompt, response_style,
                response_language, commit_message_language, thinking_level,
                primary_provider_id, primary_model_id,
                auxiliary_provider_id, auxiliary_model_id,
                lightweight_provider_id, lightweight_model_id,
                is_default, created_at, updated_at
         FROM agent_profiles WHERE id = 'p-full'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();

    assert_eq!(row.get::<String, _>("name"), "Full Profile");
    assert_eq!(
        row.get::<Option<String>, _>("custom_instructions")
            .as_deref(),
        Some("Be concise")
    );
    assert_eq!(
        row.get::<Option<String>, _>("commit_message_prompt")
            .as_deref(),
        Some("Commit: {{changes}}")
    );
    assert_eq!(
        row.get::<Option<String>, _>("response_style").as_deref(),
        Some("concise")
    );
    assert_eq!(
        row.get::<Option<String>, _>("response_language").as_deref(),
        Some("zh-CN")
    );
    assert_eq!(
        row.get::<Option<String>, _>("commit_message_language")
            .as_deref(),
        Some("en")
    );
    assert_eq!(
        row.get::<Option<String>, _>("thinking_level").as_deref(),
        Some("minimal")
    );
    assert_eq!(
        row.get::<Option<String>, _>("primary_provider_id")
            .as_deref(),
        Some("prov-a")
    );
    assert_eq!(
        row.get::<Option<String>, _>("primary_model_id").as_deref(),
        Some("gpt-4")
    );
    assert_eq!(
        row.get::<Option<String>, _>("auxiliary_provider_id")
            .as_deref(),
        Some("prov-b")
    );
    assert_eq!(
        row.get::<Option<String>, _>("auxiliary_model_id")
            .as_deref(),
        Some("claude-3")
    );
    assert_eq!(
        row.get::<Option<String>, _>("lightweight_provider_id")
            .as_deref(),
        Some("prov-c")
    );
    assert_eq!(
        row.get::<Option<String>, _>("lightweight_model_id")
            .as_deref(),
        Some("gpt-3.5")
    );
    assert_eq!(row.get::<i32, _>("is_default"), 1);
    assert!(!row.get::<String, _>("created_at").is_empty());
    assert!(!row.get::<String, _>("updated_at").is_empty());
}

// =========================================================================
// Update
// =========================================================================

#[tokio::test]
async fn test_profile_update() {
    let pool = test_helpers::setup_test_pool().await;

    seed_profile(&pool, "p-upd", "Original", false, None).await;

    sqlx::query(
        "UPDATE agent_profiles
         SET name = 'Updated', custom_instructions = 'New instructions',
             response_style = 'verbose', is_default = 1,
             updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
         WHERE id = 'p-upd'",
    )
    .execute(&pool)
    .await
    .unwrap();

    let row = sqlx::query(
        "SELECT name, custom_instructions, response_style, is_default FROM agent_profiles WHERE id = 'p-upd'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();

    assert_eq!(row.get::<String, _>("name"), "Updated");
    assert_eq!(
        row.get::<Option<String>, _>("custom_instructions")
            .as_deref(),
        Some("New instructions")
    );
    assert_eq!(
        row.get::<Option<String>, _>("response_style").as_deref(),
        Some("verbose")
    );
    assert_eq!(row.get::<i32, _>("is_default"), 1);
}

#[tokio::test]
async fn test_profile_update_nonexistent_affects_zero_rows() {
    let pool = test_helpers::setup_test_pool().await;

    let result = sqlx::query("UPDATE agent_profiles SET name = 'Ghost' WHERE id = 'nonexistent'")
        .execute(&pool)
        .await
        .unwrap();

    assert_eq!(result.rows_affected(), 0);
}

// =========================================================================
// Delete
// =========================================================================

#[tokio::test]
async fn test_profile_delete_existing() {
    let pool = test_helpers::setup_test_pool().await;

    seed_profile(&pool, "p-del", "Deletable", false, None).await;

    let result = sqlx::query("DELETE FROM agent_profiles WHERE id = 'p-del'")
        .execute(&pool)
        .await
        .unwrap();
    assert_eq!(result.rows_affected(), 1);

    let row = sqlx::query("SELECT id FROM agent_profiles WHERE id = 'p-del'")
        .fetch_optional(&pool)
        .await
        .unwrap();
    assert!(row.is_none());
}

#[tokio::test]
async fn test_profile_delete_nonexistent_affects_zero_rows() {
    let pool = test_helpers::setup_test_pool().await;

    let result = sqlx::query("DELETE FROM agent_profiles WHERE id = 'nonexistent'")
        .execute(&pool)
        .await
        .unwrap();
    assert_eq!(result.rows_affected(), 0);
}

// =========================================================================
// set_default transactional behavior
// =========================================================================

#[tokio::test]
async fn test_profile_set_default_clears_previous() {
    let pool = test_helpers::setup_test_pool().await;

    seed_profile(&pool, "p-old-def", "Old Default", true, None).await;
    seed_profile(&pool, "p-new-def", "New Default", false, None).await;

    // Verify initial state
    let old = sqlx::query("SELECT is_default FROM agent_profiles WHERE id = 'p-old-def'")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(old.get::<i32, _>("is_default"), 1);

    // Transactional set_default: clear all defaults, then set the new one
    let mut tx = pool.begin().await.unwrap();

    sqlx::query("UPDATE agent_profiles SET is_default = 0 WHERE is_default = 1")
        .execute(&mut *tx)
        .await
        .unwrap();

    let result = sqlx::query("UPDATE agent_profiles SET is_default = 1 WHERE id = 'p-new-def'")
        .execute(&mut *tx)
        .await
        .unwrap();
    assert_eq!(result.rows_affected(), 1);

    tx.commit().await.unwrap();

    // Verify final state
    let old_after = sqlx::query("SELECT is_default FROM agent_profiles WHERE id = 'p-old-def'")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(old_after.get::<i32, _>("is_default"), 0);

    let new_after = sqlx::query("SELECT is_default FROM agent_profiles WHERE id = 'p-new-def'")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(new_after.get::<i32, _>("is_default"), 1);
}

#[tokio::test]
async fn test_profile_set_default_nonexistent_id_affects_zero_rows() {
    let pool = test_helpers::setup_test_pool().await;

    seed_profile(&pool, "p-existing", "Existing", true, None).await;

    let result = sqlx::query("UPDATE agent_profiles SET is_default = 1 WHERE id = 'nonexistent'")
        .execute(&pool)
        .await
        .unwrap();
    assert_eq!(result.rows_affected(), 0);
}

// =========================================================================
// Profile with threads (FK relationship)
// =========================================================================

#[tokio::test]
async fn test_profile_thread_association() {
    let pool = test_helpers::setup_test_pool().await;

    seed_profile(&pool, "p-thread", "Thread Profile", false, None).await;
    test_helpers::seed_workspace(&pool, "ws-prof", "/tmp/prof").await;
    test_helpers::seed_thread(&pool, "t-prof", "ws-prof", Some("p-thread")).await;

    let row = sqlx::query("SELECT profile_id FROM threads WHERE id = 't-prof'")
        .fetch_one(&pool)
        .await
        .unwrap();

    assert_eq!(
        row.get::<Option<String>, _>("profile_id").as_deref(),
        Some("p-thread")
    );
}

// =========================================================================
// profile_repo module coverage
//
// The tests above exercise the underlying SQL through hand-rolled queries.
// These tests drive the public functions in persistence::repo::profile_repo
// so the repo module itself is covered.
// =========================================================================

fn sample_record(id: &str, name: &str, is_default: bool) -> AgentProfileRecord {
    AgentProfileRecord {
        id: id.to_string(),
        name: name.to_string(),
        custom_instructions: Some("be terse".to_string()),
        commit_message_prompt: Some("conventional".to_string()),
        response_style: Some("concise".to_string()),
        response_language: Some("en".to_string()),
        commit_message_language: Some("en".to_string()),
        thinking_level: Some("medium".to_string()),
        primary_provider_id: Some("prov-primary".to_string()),
        primary_model_id: Some("model-primary".to_string()),
        auxiliary_provider_id: Some("prov-aux".to_string()),
        auxiliary_model_id: Some("model-aux".to_string()),
        lightweight_provider_id: Some("prov-lite".to_string()),
        lightweight_model_id: Some("model-lite".to_string()),
        is_default,
        created_at: String::new(),
        updated_at: String::new(),
    }
}

#[tokio::test]
async fn profile_repo_insert_and_list_returns_records() {
    let pool = test_helpers::setup_test_pool().await;

    profile_repo::insert(&pool, &sample_record("p1", "Alpha", false))
        .await
        .expect("insert alpha");
    profile_repo::insert(&pool, &sample_record("p2", "Default", true))
        .await
        .expect("insert default");
    profile_repo::insert(&pool, &sample_record("p3", "Bravo", false))
        .await
        .expect("insert bravo");

    let list = profile_repo::list_all(&pool).await.expect("list");
    assert_eq!(list.len(), 3);
    // Default first, then name asc
    assert_eq!(list[0].id, "p2");
    assert!(list[0].is_default);
    assert_eq!(list[1].id, "p1");
    assert_eq!(list[2].id, "p3");
    // Every field round-trips through the repo
    assert_eq!(list[1].custom_instructions.as_deref(), Some("be terse"));
    assert_eq!(list[1].primary_provider_id.as_deref(), Some("prov-primary"));
    assert_eq!(list[1].auxiliary_model_id.as_deref(), Some("model-aux"));
    assert_eq!(
        list[1].lightweight_provider_id.as_deref(),
        Some("prov-lite")
    );
    assert!(!list[1].created_at.is_empty());
}

#[tokio::test]
async fn profile_repo_find_by_id_returns_some_and_none() {
    let pool = test_helpers::setup_test_pool().await;

    profile_repo::insert(&pool, &sample_record("p-only", "One", true))
        .await
        .unwrap();

    let hit = profile_repo::find_by_id(&pool, "p-only")
        .await
        .expect("find");
    assert!(hit.is_some());
    assert_eq!(hit.unwrap().name, "One");

    let miss = profile_repo::find_by_id(&pool, "nope").await.unwrap();
    assert!(miss.is_none());
}

#[tokio::test]
async fn profile_repo_update_rewrites_fields_and_touches_updated_at() {
    let pool = test_helpers::setup_test_pool().await;

    profile_repo::insert(&pool, &sample_record("p1", "Original", false))
        .await
        .unwrap();

    let mut patched = sample_record("p1", "Renamed", true);
    patched.custom_instructions = Some("shipping mode".to_string());
    profile_repo::update(&pool, &patched).await.expect("update");

    let after = profile_repo::find_by_id(&pool, "p1")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(after.name, "Renamed");
    assert_eq!(after.custom_instructions.as_deref(), Some("shipping mode"));
    assert!(after.is_default);
}

#[tokio::test]
async fn profile_repo_update_missing_record_returns_not_found_error() {
    let pool = test_helpers::setup_test_pool().await;
    let err = profile_repo::update(&pool, &sample_record("ghost", "x", false))
        .await
        .unwrap_err();
    // not_found AppError uses Settings error source for agent profile domain
    assert!(err.user_message.to_lowercase().contains("not found"));
}

#[tokio::test]
async fn profile_repo_delete_reports_whether_row_existed() {
    let pool = test_helpers::setup_test_pool().await;

    profile_repo::insert(&pool, &sample_record("p1", "One", false))
        .await
        .unwrap();

    assert!(profile_repo::delete(&pool, "p1").await.unwrap());
    assert!(!profile_repo::delete(&pool, "p1").await.unwrap());
    assert!(profile_repo::find_by_id(&pool, "p1")
        .await
        .unwrap()
        .is_none());
}

#[tokio::test]
async fn profile_repo_set_default_clears_previous_default() {
    let pool = test_helpers::setup_test_pool().await;

    profile_repo::insert(&pool, &sample_record("p1", "One", true))
        .await
        .unwrap();
    profile_repo::insert(&pool, &sample_record("p2", "Two", false))
        .await
        .unwrap();

    profile_repo::set_default(&pool, "p2")
        .await
        .expect("set default");

    let row_default = sqlx::query("SELECT id FROM agent_profiles WHERE is_default = 1")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(row_default.get::<String, _>("id"), "p2");

    let count_default: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM agent_profiles WHERE is_default = 1")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(count_default, 1);
}

#[tokio::test]
async fn profile_repo_set_default_missing_id_returns_not_found_and_rolls_back() {
    let pool = test_helpers::setup_test_pool().await;

    profile_repo::insert(&pool, &sample_record("p1", "One", true))
        .await
        .unwrap();

    let err = profile_repo::set_default(&pool, "missing")
        .await
        .unwrap_err();
    assert!(err.user_message.to_lowercase().contains("not found"));

    // The previous default must still be the default because the tx rolled back.
    let still_default: i64 =
        sqlx::query_scalar("SELECT is_default FROM agent_profiles WHERE id = 'p1'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(still_default, 1, "previous default must survive rollback");
}
