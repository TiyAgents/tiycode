//! M1.3 — Settings & configuration system tests
//!
//! Acceptance criteria:
//! - Settings persist across restarts (no localStorage dependency)
//! - Provider API Key stored (encrypted in production, stored in DB)
//! - Profile three-layer model mapping configurable

mod test_helpers;

use sqlx::Row;

// =========================================================================
// T1.3.1 — Settings CRUD
// =========================================================================

#[tokio::test]
async fn test_settings_insert_and_get() {
    let pool = test_helpers::setup_test_pool().await;

    test_helpers::seed_setting(&pool, "ui.theme", r#""dark""#).await;

    let row = sqlx::query("SELECT value_json FROM settings WHERE key = ?")
        .bind("ui.theme")
        .fetch_one(&pool)
        .await
        .unwrap();

    assert_eq!(row.get::<String, _>("value_json"), r#""dark""#);
}

#[tokio::test]
async fn test_settings_upsert_overwrites() {
    let pool = test_helpers::setup_test_pool().await;

    test_helpers::seed_setting(&pool, "ui.theme", r#""dark""#).await;
    test_helpers::seed_setting(&pool, "ui.theme", r#""light""#).await;

    let row = sqlx::query("SELECT value_json FROM settings WHERE key = ?")
        .bind("ui.theme")
        .fetch_one(&pool)
        .await
        .unwrap();

    assert_eq!(row.get::<String, _>("value_json"), r#""light""#);
}

#[tokio::test]
async fn test_settings_get_all() {
    let pool = test_helpers::setup_test_pool().await;

    // Count any pre-seeded settings from migrations
    let baseline: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM settings")
        .fetch_one(&pool)
        .await
        .unwrap();

    test_helpers::seed_setting(&pool, "ui.theme", r#""dark""#).await;
    test_helpers::seed_setting(&pool, "ui.language", r#""zh-CN""#).await;
    test_helpers::seed_setting(&pool, "editor.fontSize", "14").await;

    let rows = sqlx::query("SELECT key FROM settings")
        .fetch_all(&pool)
        .await
        .unwrap();

    assert_eq!(rows.len() as i64, baseline + 3, "Should have 3 additional settings beyond baseline");
}

// =========================================================================
// T1.3.2 — Policies CRUD
// =========================================================================

#[tokio::test]
async fn test_policies_insert_and_get() {
    let pool = test_helpers::setup_test_pool().await;

    let deny_list = r#"[{"tool":"run_command","pattern":"rm -rf"}]"#;
    test_helpers::seed_policy(&pool, "deny_list", deny_list).await;

    let row = sqlx::query("SELECT value_json FROM policies WHERE key = ?")
        .bind("deny_list")
        .fetch_one(&pool)
        .await
        .unwrap();

    let val: serde_json::Value = serde_json::from_str(&row.get::<String, _>("value_json")).unwrap();
    assert!(val.is_array());
    assert_eq!(val[0]["tool"].as_str().unwrap(), "run_command");
}

// =========================================================================
// T1.3.3 — Provider CRUD
// =========================================================================

#[tokio::test]
async fn test_provider_create_and_list() {
    let pool = test_helpers::setup_test_pool().await;

    sqlx::query(
        "INSERT INTO providers (id, name, protocol_type, base_url, api_key_encrypted, enabled)
         VALUES (?, ?, ?, ?, ?, ?)",
    )
    .bind("prov-001")
    .bind("OpenAI")
    .bind("openai")
    .bind("https://api.openai.com/v1")
    .bind("encrypted_key_placeholder")
    .bind(1)
    .execute(&pool)
    .await
    .unwrap();

    let rows = sqlx::query("SELECT id, name FROM providers")
        .fetch_all(&pool)
        .await
        .unwrap();

    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].get::<String, _>("name"), "OpenAI");
}

#[tokio::test]
async fn test_provider_update() {
    let pool = test_helpers::setup_test_pool().await;

    sqlx::query(
        "INSERT INTO providers (id, name, protocol_type, base_url, enabled)
         VALUES ('prov-u', 'Old Name', 'openai', 'https://old.api', 1)",
    )
    .execute(&pool)
    .await
    .unwrap();

    sqlx::query("UPDATE providers SET name = 'New Name', base_url = 'https://new.api' WHERE id = 'prov-u'")
        .execute(&pool)
        .await
        .unwrap();

    let row = sqlx::query("SELECT name, base_url FROM providers WHERE id = 'prov-u'")
        .fetch_one(&pool)
        .await
        .unwrap();

    assert_eq!(row.get::<String, _>("name"), "New Name");
    assert_eq!(row.get::<String, _>("base_url"), "https://new.api");
}

#[tokio::test]
async fn test_provider_delete_cascades_models() {
    let pool = test_helpers::setup_test_pool().await;

    sqlx::query(
        "INSERT INTO providers (id, name, protocol_type, base_url, enabled)
         VALUES ('prov-cas', 'CascadeTest', 'openai', 'https://api', 1)",
    )
    .execute(&pool)
    .await
    .unwrap();

    sqlx::query(
        "INSERT INTO provider_models (id, provider_id, model_name, enabled)
         VALUES ('model-1', 'prov-cas', 'gpt-4', 1)",
    )
    .execute(&pool)
    .await
    .unwrap();

    // Delete provider — should cascade to models
    sqlx::query("DELETE FROM providers WHERE id = 'prov-cas'")
        .execute(&pool)
        .await
        .unwrap();

    let models = sqlx::query("SELECT id FROM provider_models WHERE provider_id = 'prov-cas'")
        .fetch_all(&pool)
        .await
        .unwrap();

    assert_eq!(models.len(), 0, "Models should be cascade-deleted with provider");
}

// =========================================================================
// T1.3.4 — Provider models
// =========================================================================

#[tokio::test]
async fn test_provider_model_unique_constraint() {
    let pool = test_helpers::setup_test_pool().await;

    sqlx::query(
        "INSERT INTO providers (id, name, protocol_type, base_url, enabled)
         VALUES ('prov-uniq', 'Unique', 'openai', 'https://api', 1)",
    )
    .execute(&pool)
    .await
    .unwrap();

    sqlx::query(
        "INSERT INTO provider_models (id, provider_id, model_name, enabled)
         VALUES ('m1', 'prov-uniq', 'gpt-4', 1)",
    )
    .execute(&pool)
    .await
    .unwrap();

    // Duplicate (provider_id, model_name) should fail
    let result = sqlx::query(
        "INSERT INTO provider_models (id, provider_id, model_name, enabled)
         VALUES ('m2', 'prov-uniq', 'gpt-4', 1)",
    )
    .execute(&pool)
    .await;

    assert!(result.is_err(), "Duplicate provider+model_name should be rejected");
}

// =========================================================================
// T1.3.5 — Agent profiles with three-layer model mapping
// =========================================================================

#[tokio::test]
async fn test_profile_three_layer_model() {
    let pool = test_helpers::setup_test_pool().await;

    sqlx::query(
        "INSERT INTO agent_profiles (id, name,
            primary_provider_id, primary_model_id,
            auxiliary_provider_id, auxiliary_model_id,
            lightweight_provider_id, lightweight_model_id,
            is_default)
         VALUES ('prof-1', 'My Profile',
            'prov-a', 'model-gpt4',
            'prov-b', 'model-claude',
            'prov-a', 'model-gpt35',
            1)",
    )
    .execute(&pool)
    .await
    .unwrap();

    let row = sqlx::query(
        "SELECT primary_provider_id, auxiliary_model_id, lightweight_model_id
         FROM agent_profiles WHERE id = 'prof-1'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();

    assert_eq!(row.get::<Option<String>, _>("primary_provider_id").unwrap(), "prov-a");
    assert_eq!(row.get::<Option<String>, _>("auxiliary_model_id").unwrap(), "model-claude");
    assert_eq!(row.get::<Option<String>, _>("lightweight_model_id").unwrap(), "model-gpt35");
}
