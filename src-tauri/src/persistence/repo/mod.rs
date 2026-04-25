pub mod audit_repo;
pub mod message_repo;
pub mod profile_repo;
pub mod provider_repo;
pub mod run_helper_repo;
pub mod run_repo;
pub mod settings_repo;
pub mod task_board_repo;
pub mod task_item_repo;
pub mod terminal_session_repo;
pub mod thread_repo;
pub mod tool_call_repo;
pub mod workspace_repo;

#[cfg(test)]
mod coverage_tests {
    use super::{audit_repo, profile_repo, settings_repo};
    use crate::model::provider::AgentProfileRecord;
    use sqlx::sqlite::{SqliteConnectOptions, SqlitePool, SqlitePoolOptions};
    use std::str::FromStr;

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

    fn profile_record(id: &str, name: &str, is_default: bool) -> AgentProfileRecord {
        AgentProfileRecord {
            id: id.to_string(),
            name: name.to_string(),
            custom_instructions: Some(format!("instructions for {name}")),
            commit_message_prompt: Some("Use conventional commits".to_string()),
            response_style: Some("concise".to_string()),
            response_language: Some("zh-CN".to_string()),
            commit_message_language: Some("en-US".to_string()),
            thinking_level: Some("medium".to_string()),
            primary_provider_id: Some("primary-provider".to_string()),
            primary_model_id: Some("primary-model".to_string()),
            auxiliary_provider_id: Some("aux-provider".to_string()),
            auxiliary_model_id: Some("aux-model".to_string()),
            lightweight_provider_id: Some("light-provider".to_string()),
            lightweight_model_id: Some("light-model".to_string()),
            is_default,
            created_at: "ignored-on-insert".to_string(),
            updated_at: "ignored-on-insert".to_string(),
        }
    }

    fn audit_insert(
        source: &str,
        action: &str,
        result_json: Option<&str>,
    ) -> audit_repo::AuditInsert {
        audit_repo::AuditInsert {
            actor_type: "test".to_string(),
            actor_id: Some("actor-1".to_string()),
            source: source.to_string(),
            workspace_id: None,
            thread_id: None,
            run_id: None,
            tool_call_id: None,
            action: action.to_string(),
            target_type: Some("target-kind".to_string()),
            target_id: Some(format!("target-{action}")),
            policy_check_json: None,
            result_json: result_json.map(ToString::to_string),
        }
    }

    #[tokio::test]
    async fn settings_repo_crud_orders_keys_and_policy_repo_reads_seeded_table() {
        let pool = setup_test_pool().await;

        settings_repo::set(&pool, "coverage.zeta", r#"{"enabled":true}"#)
            .await
            .unwrap();
        settings_repo::set(&pool, "coverage.alpha", r#""first""#)
            .await
            .unwrap();

        let alpha = settings_repo::get(&pool, "coverage.alpha")
            .await
            .unwrap()
            .expect("inserted setting should exist");
        assert_eq!(alpha.key, "coverage.alpha");
        assert_eq!(alpha.value_json, r#""first""#);
        assert!(!alpha.updated_at.is_empty());

        settings_repo::set(&pool, "coverage.alpha", r#""updated""#)
            .await
            .unwrap();
        let updated = settings_repo::get(&pool, "coverage.alpha")
            .await
            .unwrap()
            .expect("updated setting should exist");
        assert_eq!(updated.value_json, r#""updated""#);

        let all = settings_repo::get_all(&pool).await.unwrap();
        let keys: Vec<_> = all.iter().map(|record| record.key.as_str()).collect();
        let mut sorted_keys = keys.clone();
        sorted_keys.sort_unstable();
        assert_eq!(keys, sorted_keys, "settings should be ordered by key");
        assert!(keys.contains(&"coverage.alpha"));
        assert!(keys.contains(&"coverage.zeta"));

        assert!(settings_repo::delete(&pool, "coverage.zeta").await.unwrap());
        assert!(!settings_repo::delete(&pool, "coverage.missing")
            .await
            .unwrap());
        assert!(settings_repo::get(&pool, "coverage.zeta")
            .await
            .unwrap()
            .is_none());

        settings_repo::policy_set(&pool, "coverage.policy.b", r#"{"allow":false}"#)
            .await
            .unwrap();
        settings_repo::policy_set(&pool, "coverage.policy.a", r#"{"allow":true}"#)
            .await
            .unwrap();

        let policy = settings_repo::policy_get(&pool, "coverage.policy.a")
            .await
            .unwrap()
            .expect("inserted policy should exist");
        assert_eq!(policy.value_json, r#"{"allow":true}"#);

        let policies = settings_repo::policy_get_all(&pool).await.unwrap();
        let policy_keys: Vec<_> = policies.iter().map(|record| record.key.as_str()).collect();
        let mut sorted_policy_keys = policy_keys.clone();
        sorted_policy_keys.sort_unstable();
        assert_eq!(
            policy_keys, sorted_policy_keys,
            "policies should be ordered by key"
        );
        assert!(policy_keys.contains(&"coverage.policy.a"));
        assert!(policy_keys.contains(&"coverage.policy.b"));
    }

    #[tokio::test]
    async fn audit_repo_lists_only_extension_activity_with_json_parse_fallback() {
        let pool = setup_test_pool().await;

        for (source, action, result_json, created_at) in [
            (
                "extensions",
                "coverage_extensions",
                Some(r#"{"ok":true}"#),
                "2026-04-24T00:00:01Z",
            ),
            (
                "plugin:demo",
                "coverage_plugin",
                Some("not-json"),
                "2026-04-24T00:00:02Z",
            ),
            (
                "mcp:filesystem",
                "coverage_mcp",
                Some(r#"{"count":2}"#),
                "2026-04-24T00:00:03Z",
            ),
            (
                "app",
                "coverage_app",
                Some(r#"{"ignored":true}"#),
                "2026-04-24T00:00:04Z",
            ),
        ] {
            audit_repo::insert(&pool, &audit_insert(source, action, result_json))
                .await
                .unwrap();
            sqlx::query("UPDATE audit_events SET created_at = ? WHERE action = ?")
                .bind(created_at)
                .bind(action)
                .execute(&pool)
                .await
                .unwrap();
        }

        let rows = audit_repo::list_extension_activity(&pool, 10)
            .await
            .unwrap();
        let actions: Vec<_> = rows.iter().map(|row| row.action.as_str()).collect();
        assert_eq!(
            actions,
            vec!["coverage_mcp", "coverage_plugin", "coverage_extensions"]
        );
        assert!(!actions.contains(&"coverage_app"));

        assert_eq!(rows[0].source, "mcp:filesystem");
        assert_eq!(rows[0].target_type.as_deref(), Some("target-kind"));
        assert_eq!(rows[0].target_id.as_deref(), Some("target-coverage_mcp"));
        assert_eq!(rows[0].result, Some(serde_json::json!({ "count": 2 })));
        assert_eq!(rows[1].source, "plugin:demo");
        assert_eq!(
            rows[1].result, None,
            "invalid result_json should be ignored"
        );
        assert_eq!(rows[2].result, Some(serde_json::json!({ "ok": true })));

        let limited = audit_repo::list_extension_activity(&pool, 2).await.unwrap();
        assert_eq!(limited.len(), 2);
        assert_eq!(limited[0].action, "coverage_mcp");
        assert_eq!(limited[1].action, "coverage_plugin");
    }

    #[tokio::test]
    async fn profile_repo_crud_sets_default_and_reports_missing_profiles() {
        let pool = setup_test_pool().await;
        let alpha = profile_record("coverage-profile-alpha", "Alpha", false);
        let beta = profile_record("coverage-profile-beta", "Beta", true);

        profile_repo::insert(&pool, &alpha).await.unwrap();
        profile_repo::insert(&pool, &beta).await.unwrap();

        let profiles = profile_repo::list_all(&pool).await.unwrap();
        assert_eq!(profiles.len(), 2);
        assert_eq!(profiles[0].id, "coverage-profile-beta");
        assert!(profiles[0].is_default);
        assert_eq!(profiles[1].id, "coverage-profile-alpha");

        let found = profile_repo::find_by_id(&pool, "coverage-profile-alpha")
            .await
            .unwrap()
            .expect("inserted profile should exist");
        assert_eq!(found.name, "Alpha");
        assert_eq!(found.response_language.as_deref(), Some("zh-CN"));
        assert_eq!(found.thinking_level.as_deref(), Some("medium"));

        let mut updated_alpha = found.clone();
        updated_alpha.name = "Alpha Updated".to_string();
        updated_alpha.is_default = false;
        updated_alpha.primary_model_id = Some("primary-model-updated".to_string());
        profile_repo::update(&pool, &updated_alpha).await.unwrap();

        let updated = profile_repo::find_by_id(&pool, "coverage-profile-alpha")
            .await
            .unwrap()
            .expect("updated profile should exist");
        assert_eq!(updated.name, "Alpha Updated");
        assert_eq!(
            updated.primary_model_id.as_deref(),
            Some("primary-model-updated")
        );

        profile_repo::set_default(&pool, "coverage-profile-alpha")
            .await
            .unwrap();
        let after_default_change = profile_repo::list_all(&pool).await.unwrap();
        assert_eq!(after_default_change[0].id, "coverage-profile-alpha");
        assert!(after_default_change[0].is_default);
        assert!(!after_default_change[1].is_default);

        let missing_update =
            profile_repo::update(&pool, &profile_record("missing-profile", "Missing", false))
                .await
                .unwrap_err();
        assert_eq!(missing_update.error_code, "settings.not_found");
        assert_eq!(missing_update.user_message, "agent profile not found");

        let missing_default = profile_repo::set_default(&pool, "missing-profile")
            .await
            .unwrap_err();
        assert_eq!(missing_default.error_code, "settings.not_found");

        assert!(profile_repo::delete(&pool, "coverage-profile-beta")
            .await
            .unwrap());
        assert!(!profile_repo::delete(&pool, "coverage-profile-beta")
            .await
            .unwrap());
        assert!(profile_repo::find_by_id(&pool, "coverage-profile-beta")
            .await
            .unwrap()
            .is_none());
    }
}
