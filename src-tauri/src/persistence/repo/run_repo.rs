use chrono::Utc;
use sqlx::SqlitePool;
use tiycore::types::Usage;

use crate::model::errors::AppError;
use crate::model::thread::{RunSummaryDto, RunUsageDto};

#[derive(sqlx::FromRow)]
struct RunRow {
    id: String,
    thread_id: String,
    run_mode: String,
    status: String,
    model_id: Option<String>,
    effective_model_plan_json: Option<String>,
    error_message: Option<String>,
    started_at: String,
    input_tokens: i64,
    output_tokens: i64,
    cache_read_tokens: i64,
    cache_write_tokens: i64,
    total_tokens: i64,
}

/// Full run record for insert.
pub struct RunInsert {
    pub id: String,
    pub thread_id: String,
    pub profile_id: Option<String>,
    pub run_mode: String,
    pub provider_id: Option<String>,
    pub model_id: Option<String>,
    pub effective_model_plan_json: Option<String>,
    pub status: String,
}

pub async fn insert(pool: &SqlitePool, r: &RunInsert) -> Result<(), AppError> {
    let now = Utc::now().to_rfc3339();
    sqlx::query(
        "INSERT INTO thread_runs (id, thread_id, profile_id, run_mode,
                provider_id, model_id, effective_model_plan_json, status, started_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&r.id)
    .bind(&r.thread_id)
    .bind(&r.profile_id)
    .bind(&r.run_mode)
    .bind(&r.provider_id)
    .bind(&r.model_id)
    .bind(&r.effective_model_plan_json)
    .bind(&r.status)
    .bind(&now)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn update_status(pool: &SqlitePool, id: &str, status: &str) -> Result<(), AppError> {
    let is_terminal = matches!(
        status,
        "completed" | "failed" | "denied" | "interrupted" | "cancelled" | "limit_reached"
    );

    if is_terminal {
        let now = Utc::now().to_rfc3339();
        sqlx::query("UPDATE thread_runs SET status = ?, finished_at = ? WHERE id = ?")
            .bind(status)
            .bind(&now)
            .bind(id)
            .execute(pool)
            .await?;
    } else {
        sqlx::query("UPDATE thread_runs SET status = ? WHERE id = ?")
            .bind(status)
            .bind(id)
            .execute(pool)
            .await?;
    }

    Ok(())
}

pub async fn update_usage(pool: &SqlitePool, id: &str, usage: &Usage) -> Result<(), AppError> {
    sqlx::query(
        "UPDATE thread_runs
         SET input_tokens = ?, output_tokens = ?, cache_read_tokens = ?,
             cache_write_tokens = ?, total_tokens = ?
         WHERE id = ?",
    )
    .bind(i64::try_from(usage.input).unwrap_or(i64::MAX))
    .bind(i64::try_from(usage.output).unwrap_or(i64::MAX))
    .bind(i64::try_from(usage.cache_read).unwrap_or(i64::MAX))
    .bind(i64::try_from(usage.cache_write).unwrap_or(i64::MAX))
    .bind(i64::try_from(usage.total_tokens).unwrap_or(i64::MAX))
    .bind(id)
    .execute(pool)
    .await?;

    Ok(())
}

pub async fn set_error_message(
    pool: &SqlitePool,
    id: &str,
    error_message: &str,
) -> Result<(), AppError> {
    sqlx::query("UPDATE thread_runs SET error_message = ? WHERE id = ?")
        .bind(error_message)
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn find_effective_model_plan_json(
    pool: &SqlitePool,
    id: &str,
) -> Result<Option<String>, AppError> {
    let value = sqlx::query_scalar::<_, Option<String>>(
        "SELECT effective_model_plan_json
         FROM thread_runs
         WHERE id = ?
         LIMIT 1",
    )
    .bind(id)
    .fetch_optional(pool)
    .await?
    .flatten();

    Ok(value)
}

/// Find the currently active (non-terminal) run for a thread.
pub async fn find_active_by_thread(
    pool: &SqlitePool,
    thread_id: &str,
) -> Result<Option<RunSummaryDto>, AppError> {
    let row = sqlx::query_as::<_, RunRow>(
        "SELECT id, thread_id, run_mode, status, model_id, effective_model_plan_json,
                error_message, started_at, input_tokens, output_tokens,
                cache_read_tokens, cache_write_tokens, total_tokens
         FROM thread_runs
         WHERE thread_id = ?
           AND status NOT IN ('completed', 'failed', 'denied', 'interrupted', 'cancelled', 'limit_reached', 'waiting_approval')
         ORDER BY started_at DESC
         LIMIT 1",
    )
    .bind(thread_id)
    .fetch_optional(pool)
    .await?;

    Ok(row.map(map_run_summary))
}

/// Find the latest run for a thread (any status), used for ThreadStatus derivation.
pub async fn find_latest_by_thread(
    pool: &SqlitePool,
    thread_id: &str,
) -> Result<Option<RunSummaryDto>, AppError> {
    let row = sqlx::query_as::<_, RunRow>(
        "SELECT id, thread_id, run_mode, status, model_id, effective_model_plan_json,
                error_message, started_at, input_tokens, output_tokens,
                cache_read_tokens, cache_write_tokens, total_tokens
         FROM thread_runs
         WHERE thread_id = ?
         ORDER BY started_at DESC
         LIMIT 1",
    )
    .bind(thread_id)
    .fetch_optional(pool)
    .await?;

    Ok(row.map(map_run_summary))
}

/// Find the latest historical run for a thread that has non-zero prompt usage
/// (`input_tokens + cache_read_tokens`), excluding the currently created run.
/// This is used to seed conservative context-token calibration for the next
/// run on the same thread.
pub async fn find_latest_with_prompt_usage_by_thread_excluding_run(
    pool: &SqlitePool,
    thread_id: &str,
    excluded_run_id: &str,
) -> Result<Option<RunSummaryDto>, AppError> {
    let row = sqlx::query_as::<_, RunRow>(
        "SELECT id, thread_id, run_mode, status, model_id, effective_model_plan_json,
                error_message, started_at, input_tokens, output_tokens,
                cache_read_tokens, cache_write_tokens, total_tokens
         FROM thread_runs
         WHERE thread_id = ?
           AND id != ?
           AND (input_tokens > 0 OR cache_read_tokens > 0)
         ORDER BY started_at DESC
         LIMIT 1",
    )
    .bind(thread_id)
    .bind(excluded_run_id)
    .fetch_optional(pool)
    .await?;

    Ok(row.map(map_run_summary))
}

pub async fn list_thread_ids_with_active_runs(pool: &SqlitePool) -> Result<Vec<String>, AppError> {
    let rows = sqlx::query_scalar::<_, String>(
        "SELECT DISTINCT thread_id
         FROM thread_runs
         WHERE status NOT IN ('completed', 'failed', 'denied', 'interrupted', 'cancelled')
           AND status != 'limit_reached'
           AND status != 'waiting_approval'
           AND finished_at IS NULL",
    )
    .fetch_all(pool)
    .await?;

    Ok(rows)
}

/// Cancel all `waiting_approval` runs for a thread, setting them to `cancelled`
/// with a `finished_at` timestamp.  Called when a pending plan approval is
/// superseded by a new run so the old run does not linger as a zombie.
pub async fn cancel_waiting_approval_by_thread(
    pool: &SqlitePool,
    thread_id: &str,
) -> Result<u64, AppError> {
    let result = sqlx::query(
        "UPDATE thread_runs
         SET status = 'cancelled',
             finished_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
         WHERE thread_id = ?
           AND status = 'waiting_approval'
           AND finished_at IS NULL",
    )
    .bind(thread_id)
    .execute(pool)
    .await?;

    Ok(result.rows_affected())
}

/// Mark all non-terminal runs for a thread as interrupted (crash recovery).
pub async fn interrupt_active_runs(pool: &SqlitePool) -> Result<u64, AppError> {
    let result = sqlx::query(
        "UPDATE thread_runs
         SET status = 'interrupted',
             error_message = COALESCE(
                 error_message,
                 'The app closed or the run was terminated before completion. Restarted in interrupted state.'
             ),
             finished_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
         WHERE status NOT IN ('completed', 'failed', 'denied', 'interrupted', 'cancelled', 'limit_reached')
           AND status != 'waiting_approval'
           AND finished_at IS NULL",
    )
    .execute(pool)
    .await?;

    Ok(result.rows_affected())
}

fn map_run_summary(row: RunRow) -> RunSummaryDto {
    let (model_display_name, context_window) =
        extract_primary_model_details(row.effective_model_plan_json.as_deref());

    RunSummaryDto {
        id: row.id,
        thread_id: row.thread_id,
        run_mode: row.run_mode,
        status: row.status,
        model_id: row.model_id,
        model_display_name,
        context_window,
        error_message: row.error_message,
        started_at: row.started_at,
        usage: RunUsageDto {
            input_tokens: row.input_tokens.max(0) as u64,
            output_tokens: row.output_tokens.max(0) as u64,
            cache_read_tokens: row.cache_read_tokens.max(0) as u64,
            cache_write_tokens: row.cache_write_tokens.max(0) as u64,
            total_tokens: row.total_tokens.max(0) as u64,
        },
    }
}

fn extract_primary_model_details(
    effective_model_plan_json: Option<&str>,
) -> (Option<String>, Option<String>) {
    let Some(raw) = effective_model_plan_json else {
        return (None, None);
    };

    let Ok(value) = serde_json::from_str::<serde_json::Value>(raw) else {
        return (None, None);
    };

    let primary = value.get("primary").and_then(|entry| entry.as_object());
    let model_display_name = primary
        .and_then(|entry| {
            entry
                .get("modelDisplayName")
                .and_then(|value| value.as_str())
        })
        .or_else(|| primary.and_then(|entry| entry.get("model").and_then(|value| value.as_str())))
        .map(str::to_string);
    let context_window = primary
        .and_then(|entry| entry.get("contextWindow").and_then(|value| value.as_str()))
        .map(str::to_string);

    (model_display_name, context_window)
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
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

        crate::persistence::sqlite::run_migrations(&pool)
            .await
            .expect("migrations failed");

        sqlx::query(
            "INSERT INTO workspaces (id, name, path, canonical_path, display_path,
                    is_default, is_git, auto_work_tree, status, created_at, updated_at)
             VALUES ('ws-1', 'ws', '/tmp', '/tmp', '/tmp', 0, 0, 0, 'ready',
                     strftime('%Y-%m-%dT%H:%M:%fZ', 'now'),
                     strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))",
        )
        .execute(&pool)
        .await
        .expect("seed workspace");

        sqlx::query(
            "INSERT INTO threads (id, workspace_id, title, status, created_at, updated_at, last_active_at)
             VALUES ('t1', 'ws-1', 't', 'idle',
                     strftime('%Y-%m-%dT%H:%M:%fZ', 'now'),
                     strftime('%Y-%m-%dT%H:%M:%fZ', 'now'),
                     strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))",
        )
        .execute(&pool)
        .await
        .expect("seed thread t1");

        sqlx::query(
            "INSERT INTO threads (id, workspace_id, title, status, created_at, updated_at, last_active_at)
             VALUES ('t2', 'ws-1', 't2', 'idle',
                     strftime('%Y-%m-%dT%H:%M:%fZ', 'now'),
                     strftime('%Y-%m-%dT%H:%M:%fZ', 'now'),
                     strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))",
        )
        .execute(&pool)
        .await
        .expect("seed thread t2");

        pool
    }

    async fn insert_run_with_started_at(
        pool: &SqlitePool,
        id: &str,
        thread_id: &str,
        started_at: &str,
        input_tokens: i64,
    ) {
        insert_run_with_usage(pool, id, thread_id, started_at, input_tokens, 0).await;
    }

    async fn insert_run_with_usage(
        pool: &SqlitePool,
        id: &str,
        thread_id: &str,
        started_at: &str,
        input_tokens: i64,
        cache_read_tokens: i64,
    ) {
        sqlx::query(
            "INSERT INTO thread_runs (
                id, thread_id, profile_id, run_mode, provider_id, model_id,
                effective_model_plan_json, status, started_at, input_tokens,
                output_tokens, cache_read_tokens, cache_write_tokens, total_tokens
             )
             VALUES (?, ?, NULL, 'default', NULL, NULL, NULL, 'completed', ?, ?, 0, ?, 0, ?)",
        )
        .bind(id)
        .bind(thread_id)
        .bind(started_at)
        .bind(input_tokens)
        .bind(cache_read_tokens)
        .bind(input_tokens.saturating_add(cache_read_tokens))
        .execute(pool)
        .await
        .expect("seed run");
    }

    #[tokio::test]
    async fn find_latest_with_prompt_usage_returns_none_when_no_matching_history_exists() {
        let pool = setup_test_pool().await;

        assert!(
            find_latest_with_prompt_usage_by_thread_excluding_run(&pool, "t1", "run-x")
                .await
                .unwrap()
                .is_none()
        );

        insert_run_with_started_at(&pool, "run-1", "t1", "2026-04-22T09:00:00Z", 0).await;

        assert!(
            find_latest_with_prompt_usage_by_thread_excluding_run(&pool, "t1", "run-x")
                .await
                .unwrap()
                .is_none()
        );
    }

    #[tokio::test]
    async fn find_latest_with_prompt_usage_filters_thread_zero_usage_and_excluded_run() {
        let pool = setup_test_pool().await;

        insert_run_with_started_at(&pool, "run-1", "t1", "2026-04-22T09:00:00Z", 10).await;
        insert_run_with_started_at(&pool, "run-2", "t1", "2026-04-22T09:05:00Z", 0).await;
        insert_run_with_started_at(&pool, "run-3", "t1", "2026-04-22T09:10:00Z", 50).await;
        insert_run_with_started_at(&pool, "run-4", "t2", "2026-04-22T09:20:00Z", 999).await;

        let latest = find_latest_with_prompt_usage_by_thread_excluding_run(&pool, "t1", "run-x")
            .await
            .unwrap()
            .expect("latest matching run for t1");
        assert_eq!(latest.id, "run-3");
        assert_eq!(latest.thread_id, "t1");
        assert_eq!(latest.usage.input_tokens, 50);

        let excluded = find_latest_with_prompt_usage_by_thread_excluding_run(&pool, "t1", "run-3")
            .await
            .unwrap()
            .expect("previous matching run after exclusion");
        assert_eq!(excluded.id, "run-1");
        assert_eq!(excluded.usage.input_tokens, 10);
    }

    #[tokio::test]
    async fn find_latest_with_prompt_usage_includes_cache_read_only_runs() {
        let pool = setup_test_pool().await;

        insert_run_with_usage(&pool, "run-1", "t1", "2026-04-22T09:00:00Z", 10, 0).await;
        insert_run_with_usage(&pool, "run-2", "t1", "2026-04-22T09:05:00Z", 0, 64).await;
        insert_run_with_usage(&pool, "run-3", "t1", "2026-04-22T09:10:00Z", 0, 0).await;

        let latest = find_latest_with_prompt_usage_by_thread_excluding_run(&pool, "t1", "run-x")
            .await
            .unwrap()
            .expect("cache-read-only run should qualify as prompt usage");
        assert_eq!(latest.id, "run-2");
        assert_eq!(latest.usage.input_tokens, 0);
        assert_eq!(latest.usage.cache_read_tokens, 64);
    }

    #[tokio::test]
    async fn insert_run_persists_fields() {
        let pool = setup_test_pool().await;
        let r = RunInsert {
            id: "run-1".into(),
            thread_id: "t1".into(),
            profile_id: Some("prof-1".into()),
            run_mode: "default".into(),
            provider_id: Some("prov-1".into()),
            model_id: Some("gpt-4".into()),
            effective_model_plan_json: Some("{}".into()),
            status: "running".into(),
        };
        insert(&pool, &r).await.unwrap();

        let found = find_active_by_thread(&pool, "t1")
            .await
            .unwrap()
            .expect("should exist");
        assert_eq!(found.id, "run-1");
        assert_eq!(found.status, "running");
    }

    #[tokio::test]
    async fn update_status_sets_finished_at_for_terminal() {
        let pool = setup_test_pool().await;
        insert_run_with_started_at(&pool, "run-1", "t1", "2026-04-22T09:00:00Z", 0).await;

        update_status(&pool, "run-1", "completed").await.unwrap();

        let found = find_latest_by_thread(&pool, "t1")
            .await
            .unwrap()
            .expect("should exist");
        assert_eq!(found.status, "completed");
    }

    #[tokio::test]
    async fn update_status_for_non_terminal_does_not_set_finished_at() {
        let pool = setup_test_pool().await;
        insert_run_with_started_at(&pool, "run-1", "t1", "2026-04-22T09:00:00Z", 0).await;

        update_status(&pool, "run-1", "waiting_approval")
            .await
            .unwrap();

        let found = find_latest_by_thread(&pool, "t1")
            .await
            .unwrap()
            .expect("should exist");
        assert_eq!(found.status, "waiting_approval");
    }

    #[tokio::test]
    async fn update_usage_persists_token_counts() {
        let pool = setup_test_pool().await;
        insert_run_with_started_at(&pool, "run-1", "t1", "2026-04-22T09:00:00Z", 0).await;

        let usage = tiycore::types::Usage {
            input: 500,
            output: 300,
            cache_read: 100,
            cache_write: 50,
            total_tokens: 950,
            cost: tiycore::types::UsageCost::default(),
        };
        update_usage(&pool, "run-1", &usage).await.unwrap();

        let found = find_latest_by_thread(&pool, "t1")
            .await
            .unwrap()
            .expect("should exist");
        assert_eq!(found.usage.input_tokens, 500);
        assert_eq!(found.usage.total_tokens, 950);
    }

    #[tokio::test]
    async fn set_error_message_persists() {
        let pool = setup_test_pool().await;
        insert_run_with_started_at(&pool, "run-1", "t1", "2026-04-22T09:00:00Z", 0).await;

        set_error_message(&pool, "run-1", "oops").await.unwrap();

        let found = find_latest_by_thread(&pool, "t1")
            .await
            .unwrap()
            .expect("should exist");
        assert_eq!(found.error_message, Some("oops".into()));
    }

    #[tokio::test]
    async fn find_effective_model_plan_json_returns_json() {
        let pool = setup_test_pool().await;
        let r = RunInsert {
            id: "run-1".into(),
            thread_id: "t1".into(),
            profile_id: None,
            run_mode: "default".into(),
            provider_id: None,
            model_id: None,
            effective_model_plan_json: Some(r#"{"primary":{"modelDisplayName":"GPT-4"}}"#.into()),
            status: "completed".into(),
        };
        insert(&pool, &r).await.unwrap();

        let json = find_effective_model_plan_json(&pool, "run-1")
            .await
            .unwrap()
            .expect("should exist");
        assert!(json.contains("GPT-4"));
    }

    #[tokio::test]
    async fn find_effective_model_plan_json_returns_none_for_missing() {
        let pool = setup_test_pool().await;
        let json = find_effective_model_plan_json(&pool, "nope").await.unwrap();
        assert!(json.is_none());
    }

    #[tokio::test]
    async fn find_active_by_thread_skips_terminal_and_waiting_approval() {
        let pool = setup_test_pool().await;
        insert_run_with_started_at(&pool, "run-1", "t1", "2026-04-22T09:00:00Z", 0).await;
        // run-1 is 'completed' status → not active
        assert!(find_active_by_thread(&pool, "t1").await.unwrap().is_none());

        let r = RunInsert {
            id: "run-2".into(),
            thread_id: "t1".into(),
            profile_id: None,
            run_mode: "default".into(),
            provider_id: None,
            model_id: None,
            effective_model_plan_json: None,
            status: "running".into(),
        };
        insert(&pool, &r).await.unwrap();
        assert!(find_active_by_thread(&pool, "t1").await.unwrap().is_some());
    }

    #[tokio::test]
    async fn list_thread_ids_with_active_runs_finds_running() {
        let pool = setup_test_pool().await;
        let r = RunInsert {
            id: "run-1".into(),
            thread_id: "t1".into(),
            profile_id: None,
            run_mode: "default".into(),
            provider_id: None,
            model_id: None,
            effective_model_plan_json: None,
            status: "running".into(),
        };
        insert(&pool, &r).await.unwrap();

        let ids = list_thread_ids_with_active_runs(&pool).await.unwrap();
        assert!(ids.contains(&"t1".to_string()));
    }

    #[tokio::test]
    async fn interrupt_active_runs_marks_non_terminal() {
        let pool = setup_test_pool().await;
        let r = RunInsert {
            id: "run-1".into(),
            thread_id: "t1".into(),
            profile_id: None,
            run_mode: "default".into(),
            provider_id: None,
            model_id: None,
            effective_model_plan_json: None,
            status: "running".into(),
        };
        insert(&pool, &r).await.unwrap();

        let affected = interrupt_active_runs(&pool).await.unwrap();
        assert_eq!(affected, 1);

        let found = find_latest_by_thread(&pool, "t1")
            .await
            .unwrap()
            .expect("should exist");
        assert_eq!(found.status, "interrupted");
    }

    #[tokio::test]
    async fn cancel_waiting_approval_by_thread_terminates_zombie_run() {
        let pool = setup_test_pool().await;
        insert_run_with_started_at(&pool, "run-1", "t1", "2026-04-22T09:00:00Z", 0).await;

        // Move run-1 to waiting_approval (non-terminal, no finished_at)
        update_status(&pool, "run-1", "waiting_approval")
            .await
            .unwrap();
        let before = find_latest_by_thread(&pool, "t1")
            .await
            .unwrap()
            .expect("should exist");
        assert_eq!(before.status, "waiting_approval");

        let affected = cancel_waiting_approval_by_thread(&pool, "t1")
            .await
            .unwrap();
        assert_eq!(affected, 1);

        let after = find_latest_by_thread(&pool, "t1")
            .await
            .unwrap()
            .expect("should exist");
        assert_eq!(after.status, "cancelled");
    }

    #[tokio::test]
    async fn cancel_waiting_approval_by_thread_ignores_other_statuses() {
        let pool = setup_test_pool().await;
        let r = RunInsert {
            id: "run-1".into(),
            thread_id: "t1".into(),
            profile_id: None,
            run_mode: "default".into(),
            provider_id: None,
            model_id: None,
            effective_model_plan_json: None,
            status: "running".into(),
        };
        insert(&pool, &r).await.unwrap();

        let affected = cancel_waiting_approval_by_thread(&pool, "t1")
            .await
            .unwrap();
        assert_eq!(affected, 0);

        let found = find_latest_by_thread(&pool, "t1")
            .await
            .unwrap()
            .expect("should exist");
        assert_eq!(found.status, "running");
    }
}
