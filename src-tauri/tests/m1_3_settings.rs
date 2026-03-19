//! M1.3 — Settings & configuration system tests
//!
//! Acceptance criteria:
//! - Settings persist across restarts (no localStorage dependency)
//! - Provider API Key stored (encrypted in production, stored in DB)
//! - Profile three-layer model mapping configurable

mod test_helpers;

use sqlx::Row;
use tiy_agent_lib::core::settings_manager::SettingsManager;
use tiy_agent_lib::model::provider::{CustomProviderCreateInput, ProviderModelInput};

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

    assert_eq!(
        rows.len() as i64,
        baseline + 3,
        "Should have 3 additional settings beyond baseline"
    );
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

    sqlx::query(
        "UPDATE providers SET name = 'New Name', base_url = 'https://new.api' WHERE id = 'prov-u'",
    )
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

    assert_eq!(
        models.len(),
        0,
        "Models should be cascade-deleted with provider"
    );
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

    assert!(
        result.is_err(),
        "Duplicate provider+model_name should be rejected"
    );
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

    assert_eq!(
        row.get::<Option<String>, _>("primary_provider_id").unwrap(),
        "prov-a"
    );
    assert_eq!(
        row.get::<Option<String>, _>("auxiliary_model_id").unwrap(),
        "model-claude"
    );
    assert_eq!(
        row.get::<Option<String>, _>("lightweight_model_id")
            .unwrap(),
        "model-gpt35"
    );
}

// =========================================================================
// T1.3.6 — tiy-core-backed provider settings
// =========================================================================

#[tokio::test]
async fn test_provider_settings_seed_builtin_catalog() {
    let pool = test_helpers::setup_test_pool().await;
    let manager = SettingsManager::new(pool);

    let providers = manager.get_all_provider_settings().await.unwrap();
    let catalog = manager.list_provider_catalog().await.unwrap();

    assert!(
        providers
            .iter()
            .any(|provider| provider.provider_key == "openai"),
        "Expected OpenAI to be present in the built-in provider catalog"
    );
    assert!(
        providers
            .iter()
            .any(|provider| provider.provider_key == "zenmux"),
        "Expected Zenmux to be present in the built-in provider catalog"
    );

    let openai = providers
        .iter()
        .find(|provider| provider.provider_key == "openai")
        .unwrap();
    assert_eq!(openai.kind, "builtin");
    assert!(openai.locked_mapping);
    assert!(
        openai.models.is_empty(),
        "Built-in providers should start with an empty model list until Fetch or manual add"
    );

    assert!(
        !providers
            .iter()
            .any(|provider| provider.provider_key == "openai-compatible"),
        "OpenAI Compatible should not appear in the built-in provider settings list"
    );
    assert!(
        catalog
            .iter()
            .any(|entry| entry.provider_type == "openai-compatible"
                && !entry.builtin
                && entry.supports_custom),
        "OpenAI Compatible should still be available as a custom provider type"
    );
}

#[tokio::test]
async fn test_provider_settings_create_custom_and_protect_builtins() {
    let pool = test_helpers::setup_test_pool().await;
    let manager = SettingsManager::new(pool);

    let custom = manager
        .create_custom_provider(CustomProviderCreateInput {
            display_name: "My Gateway".to_string(),
            provider_type: "openai-compatible".to_string(),
            base_url: "https://example.com/v1".to_string(),
            api_key: Some("sk-test".to_string()),
            enabled: Some(true),
            custom_headers: None,
            models: None,
        })
        .await
        .unwrap();

    assert_eq!(custom.kind, "custom");
    assert_eq!(custom.provider_type, "openai-compatible");

    let builtin = manager
        .get_all_provider_settings()
        .await
        .unwrap()
        .into_iter()
        .find(|provider| provider.provider_key == "openai")
        .unwrap();

    let delete_builtin = manager.delete_custom_provider(&builtin.id).await;
    assert!(
        delete_builtin.is_err(),
        "Built-in providers should not be deletable through the custom delete path"
    );
}

#[tokio::test]
async fn test_provider_model_connection_test_returns_unsupported_for_embedding_models() {
    let pool = test_helpers::setup_test_pool().await;
    let manager = SettingsManager::new(pool);

    let custom = manager
        .create_custom_provider(CustomProviderCreateInput {
            display_name: "Embedding Gateway".to_string(),
            provider_type: "openai-compatible".to_string(),
            base_url: "https://example.com/v1".to_string(),
            api_key: Some("sk-test".to_string()),
            enabled: Some(true),
            custom_headers: None,
            models: Some(vec![ProviderModelInput {
                id: None,
                model_id: "text-embedding-3-small".to_string(),
                display_name: Some("Text Embedding 3 Small".to_string()),
                enabled: Some(true),
                context_window: None,
                max_output_tokens: None,
                capability_overrides: Some(serde_json::json!({ "embedding": true })),
                provider_options: None,
                is_manual: Some(true),
            }]),
        })
        .await
        .unwrap();

    let result = manager
        .test_provider_model_connection(&custom.id, &custom.models[0].id)
        .await
        .unwrap();

    assert!(!result.success);
    assert!(result.unsupported);
    assert_eq!(
        result.message,
        "Embedding model test is not supported yet."
    );
}

#[tokio::test]
async fn test_provider_model_connection_test_returns_not_found_for_missing_provider() {
    let pool = test_helpers::setup_test_pool().await;
    let manager = SettingsManager::new(pool);

    let error = manager
        .test_provider_model_connection("missing-provider", "missing-model")
        .await
        .unwrap_err();

    assert_eq!(error.error_code, "settings.not_found");
    assert_eq!(error.user_message, "provider not found");
}

#[tokio::test]
async fn test_provider_model_connection_test_returns_not_found_for_missing_model() {
    let pool = test_helpers::setup_test_pool().await;
    let manager = SettingsManager::new(pool);
    let provider = manager
        .get_all_provider_settings()
        .await
        .unwrap()
        .into_iter()
        .find(|entry| entry.provider_key == "openai")
        .unwrap();

    let error = manager
        .test_provider_model_connection(&provider.id, "missing-model")
        .await
        .unwrap_err();

    assert_eq!(error.error_code, "settings.not_found");
    assert_eq!(error.user_message, "provider model not found");
}
