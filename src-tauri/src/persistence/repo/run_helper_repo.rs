use chrono::Utc;
use sqlx::QueryBuilder;
use sqlx::SqlitePool;
use tiycore::types::Usage;

use crate::model::errors::AppError;
use crate::model::thread::{RunHelperDto, RunUsageDto};

#[derive(sqlx::FromRow)]
struct RunHelperRow {
    id: String,
    run_id: String,
    thread_id: String,
    helper_kind: String,
    parent_tool_call_id: Option<String>,
    status: String,
    input_summary: Option<String>,
    output_summary: Option<String>,
    error_summary: Option<String>,
    started_at: String,
    finished_at: Option<String>,
    input_tokens: i64,
    output_tokens: i64,
    cache_read_tokens: i64,
    cache_write_tokens: i64,
    total_tokens: i64,
}

impl RunHelperRow {
    fn into_dto(self) -> RunHelperDto {
        RunHelperDto {
            id: self.id,
            run_id: self.run_id,
            thread_id: self.thread_id,
            helper_kind: self.helper_kind,
            parent_tool_call_id: self.parent_tool_call_id,
            status: self.status,
            input_summary: self.input_summary,
            output_summary: self.output_summary,
            error_summary: self.error_summary,
            started_at: self.started_at,
            finished_at: self.finished_at,
            usage: RunUsageDto {
                input_tokens: self.input_tokens.max(0) as u64,
                output_tokens: self.output_tokens.max(0) as u64,
                cache_read_tokens: self.cache_read_tokens.max(0) as u64,
                cache_write_tokens: self.cache_write_tokens.max(0) as u64,
                total_tokens: self.total_tokens.max(0) as u64,
            },
        }
    }
}

pub struct RunHelperInsert {
    pub id: String,
    pub run_id: String,
    pub thread_id: String,
    pub helper_kind: String,
    pub parent_tool_call_id: Option<String>,
    pub status: String,
    pub model_role: String,
    pub provider_id: Option<String>,
    pub model_id: Option<String>,
    pub input_summary: Option<String>,
}

pub async fn insert(pool: &SqlitePool, helper: &RunHelperInsert) -> Result<String, AppError> {
    let now = Utc::now().to_rfc3339();
    sqlx::query(
        "INSERT INTO run_helpers (
            id, run_id, thread_id, helper_kind, parent_tool_call_id, status, model_role,
            provider_id, model_id, input_summary, started_at
         ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&helper.id)
    .bind(&helper.run_id)
    .bind(&helper.thread_id)
    .bind(&helper.helper_kind)
    .bind(&helper.parent_tool_call_id)
    .bind(&helper.status)
    .bind(&helper.model_role)
    .bind(&helper.provider_id)
    .bind(&helper.model_id)
    .bind(&helper.input_summary)
    .bind(&now)
    .execute(pool)
    .await?;

    Ok(now)
}

pub async fn mark_completed(
    pool: &SqlitePool,
    id: &str,
    output_summary: &str,
    usage: &Usage,
) -> Result<(), AppError> {
    let now = Utc::now().to_rfc3339();
    sqlx::query(
        "UPDATE run_helpers
         SET status = 'completed', output_summary = ?, finished_at = ?,
             input_tokens = ?, output_tokens = ?, cache_read_tokens = ?,
             cache_write_tokens = ?, total_tokens = ?
         WHERE id = ?",
    )
    .bind(output_summary)
    .bind(&now)
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

pub async fn mark_failed(
    pool: &SqlitePool,
    id: &str,
    error_summary: &str,
    interrupted: bool,
    usage: &Usage,
) -> Result<(), AppError> {
    let now = Utc::now().to_rfc3339();
    let status = if interrupted { "interrupted" } else { "failed" };

    sqlx::query(
        "UPDATE run_helpers
         SET status = ?, error_summary = ?, finished_at = ?,
             input_tokens = ?, output_tokens = ?, cache_read_tokens = ?,
             cache_write_tokens = ?, total_tokens = ?
         WHERE id = ?",
    )
    .bind(status)
    .bind(error_summary)
    .bind(&now)
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

/// Mark all non-terminal run helpers as interrupted (crash recovery).
pub async fn interrupt_active_helpers(pool: &SqlitePool) -> Result<u64, AppError> {
    let result = sqlx::query(
        "UPDATE run_helpers
         SET status = 'interrupted',
             error_summary = COALESCE(
                 error_summary,
                 'The app closed before this helper finished. Marked as interrupted on restart.'
             ),
             finished_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
         WHERE status NOT IN ('completed', 'failed', 'interrupted', 'cancelled')
           AND finished_at IS NULL",
    )
    .execute(pool)
    .await?;

    Ok(result.rows_affected())
}

pub async fn list_by_run_ids(
    pool: &SqlitePool,
    run_ids: &[String],
) -> Result<Vec<RunHelperDto>, AppError> {
    if run_ids.is_empty() {
        return Ok(Vec::new());
    }

    let mut query = QueryBuilder::new(
        "SELECT id, run_id, thread_id, helper_kind, parent_tool_call_id, status,
                input_summary, output_summary, error_summary, started_at, finished_at,
                input_tokens, output_tokens, cache_read_tokens, cache_write_tokens, total_tokens
         FROM run_helpers
         WHERE run_id IN (",
    );
    {
        let mut separated = query.separated(", ");
        for run_id in run_ids {
            separated.push_bind(run_id);
        }
    }
    query.push(") ORDER BY started_at ASC, id ASC");

    let rows = query
        .build_query_as::<RunHelperRow>()
        .fetch_all(pool)
        .await?;

    Ok(rows.into_iter().map(RunHelperRow::into_dto).collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
    use sqlx::Row;
    use std::str::FromStr;
    use tiycore::types::Usage;

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

        // Seed parent records for FK constraints
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
        .expect("seed thread");

        sqlx::query(
            "INSERT INTO thread_runs (id, thread_id, run_mode, status, started_at)
             VALUES ('run-1', 't1', 'default', 'running',
                     strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))",
        )
        .execute(&pool)
        .await
        .expect("seed run");

        sqlx::query(
            "INSERT INTO thread_runs (id, thread_id, run_mode, status, started_at)
             VALUES ('run-2', 't1', 'default', 'running',
                     strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))",
        )
        .execute(&pool)
        .await
        .expect("seed run-2");

        pool
    }

    fn default_usage() -> Usage {
        Usage {
            input: 100,
            output: 200,
            cache_read: 50,
            cache_write: 30,
            total_tokens: 380,
            cost: tiycore::types::UsageCost::default(),
        }
    }

    #[tokio::test]
    async fn insert_run_helper_returns_started_at() {
        let pool = setup_test_pool().await;
        let helper = RunHelperInsert {
            id: "h-1".into(),
            run_id: "run-1".into(),
            thread_id: "t1".into(),
            helper_kind: "review".into(),
            parent_tool_call_id: None,
            status: "running".into(),
            model_role: "auxiliary".into(),
            provider_id: None,
            model_id: None,
            input_summary: Some("Reviewing PR".into()),
        };
        let started_at = insert(&pool, &helper).await.unwrap();
        assert!(!started_at.is_empty());

        let row = sqlx::query("SELECT status, helper_kind FROM run_helpers WHERE id = 'h-1'")
            .fetch_one(&pool)
            .await
            .unwrap();
        let status: String = row.get(0);
        let kind: String = row.get(1);
        assert_eq!(status, "running");
        assert_eq!(kind, "review");
    }

    #[tokio::test]
    async fn mark_completed_updates_status_and_usage() {
        let pool = setup_test_pool().await;
        let helper = RunHelperInsert {
            id: "h-1".into(),
            run_id: "run-1".into(),
            thread_id: "t1".into(),
            helper_kind: "explore".into(),
            parent_tool_call_id: None,
            status: "running".into(),
            model_role: "auxiliary".into(),
            provider_id: None,
            model_id: None,
            input_summary: None,
        };
        insert(&pool, &helper).await.unwrap();

        let usage = default_usage();
        mark_completed(&pool, "h-1", "All good", &usage)
            .await
            .unwrap();

        let row = sqlx::query(
            "SELECT status, output_summary, input_tokens, output_tokens, total_tokens, finished_at
             FROM run_helpers WHERE id = 'h-1'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        let status: String = row.get(0);
        let summary: String = row.get(1);
        let in_tok: i64 = row.get(2);
        let out_tok: i64 = row.get(3);
        let total: i64 = row.get(4);
        let finished: Option<String> = row.get(5);

        assert_eq!(status, "completed");
        assert_eq!(summary, "All good");
        assert_eq!(in_tok, 100);
        assert_eq!(out_tok, 200);
        assert_eq!(total, 380);
        assert!(finished.is_some());
    }

    #[tokio::test]
    async fn mark_failed_with_interrupted_false_sets_status_failed() {
        let pool = setup_test_pool().await;
        let helper = RunHelperInsert {
            id: "h-1".into(),
            run_id: "run-1".into(),
            thread_id: "t1".into(),
            helper_kind: "review".into(),
            parent_tool_call_id: None,
            status: "running".into(),
            model_role: "auxiliary".into(),
            provider_id: None,
            model_id: None,
            input_summary: None,
        };
        insert(&pool, &helper).await.unwrap();

        let usage = default_usage();
        mark_failed(&pool, "h-1", "Something broke", false, &usage)
            .await
            .unwrap();

        let row = sqlx::query(
            "SELECT status, error_summary, finished_at FROM run_helpers WHERE id = 'h-1'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        let status: String = row.get(0);
        let error: String = row.get(1);
        let finished: Option<String> = row.get(2);

        assert_eq!(status, "failed");
        assert_eq!(error, "Something broke");
        assert!(finished.is_some());
    }

    #[tokio::test]
    async fn mark_failed_with_interrupted_true_sets_status_interrupted() {
        let pool = setup_test_pool().await;
        let helper = RunHelperInsert {
            id: "h-1".into(),
            run_id: "run-1".into(),
            thread_id: "t1".into(),
            helper_kind: "explore".into(),
            parent_tool_call_id: None,
            status: "running".into(),
            model_role: "auxiliary".into(),
            provider_id: None,
            model_id: None,
            input_summary: None,
        };
        insert(&pool, &helper).await.unwrap();

        let usage = default_usage();
        mark_failed(&pool, "h-1", "App closed", true, &usage)
            .await
            .unwrap();

        let row = sqlx::query("SELECT status FROM run_helpers WHERE id = 'h-1'")
            .fetch_one(&pool)
            .await
            .unwrap();
        let status: String = row.get(0);
        assert_eq!(status, "interrupted");
    }

    #[tokio::test]
    async fn interrupt_active_helpers_only_affects_non_terminal() {
        let pool = setup_test_pool().await;

        // Insert helpers in various states
        for (id, status) in &[
            ("h-running", "running"),
            ("h-completed", "completed"),
            ("h-failed", "failed"),
            ("h-interrupted", "interrupted"),
            ("h-cancelled", "cancelled"),
        ] {
            let helper = RunHelperInsert {
                id: id.to_string(),
                run_id: "run-1".into(),
                thread_id: "t1".into(),
                helper_kind: "review".into(),
                parent_tool_call_id: None,
                status: status.to_string(),
                model_role: "auxiliary".into(),
                provider_id: None,
                model_id: None,
                input_summary: None,
            };
            insert(&pool, &helper).await.unwrap();
        }

        let affected = interrupt_active_helpers(&pool).await.unwrap();
        // Only "running" should be interrupted (1 row)
        assert_eq!(affected, 1);

        let status: String = sqlx::query("SELECT status FROM run_helpers WHERE id = 'h-running'")
            .fetch_one(&pool)
            .await
            .unwrap()
            .get(0);
        assert_eq!(status, "interrupted");

        // Completed/failed/interrupted/cancelled should be untouched
        let status: String = sqlx::query("SELECT status FROM run_helpers WHERE id = 'h-completed'")
            .fetch_one(&pool)
            .await
            .unwrap()
            .get(0);
        assert_eq!(status, "completed");
    }

    #[tokio::test]
    async fn list_by_run_ids_returns_helpers_for_given_runs() {
        let pool = setup_test_pool().await;

        for (id, run_id, kind) in &[
            ("h-1", "run-1", "review"),
            ("h-2", "run-1", "explore"),
            ("h-3", "run-2", "review"),
        ] {
            let helper = RunHelperInsert {
                id: id.to_string(),
                run_id: run_id.to_string(),
                thread_id: "t1".into(),
                helper_kind: kind.to_string(),
                parent_tool_call_id: None,
                status: "completed".into(),
                model_role: "auxiliary".into(),
                provider_id: None,
                model_id: None,
                input_summary: None,
            };
            insert(&pool, &helper).await.unwrap();
        }

        let result = list_by_run_ids(&pool, &["run-1".into()]).await.unwrap();
        assert_eq!(result.len(), 2);
        let kinds: Vec<String> = result.iter().map(|h| h.helper_kind.clone()).collect();
        assert!(kinds.contains(&"review".into()));
        assert!(kinds.contains(&"explore".into()));
    }

    #[tokio::test]
    async fn list_by_run_ids_returns_empty_for_empty_input() {
        let pool = setup_test_pool().await;
        let result = list_by_run_ids(&pool, &[]).await.unwrap();
        assert!(result.is_empty());
    }
}
