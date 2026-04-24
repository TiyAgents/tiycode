use chrono::Utc;
use sqlx::QueryBuilder;
use sqlx::SqlitePool;

use crate::model::errors::AppError;
use crate::model::thread::ToolCallDto;

#[derive(sqlx::FromRow)]
struct ToolCallRow {
    storage_id: String,
    tool_call_id: String,
    run_id: String,
    thread_id: String,
    helper_id: Option<String>,
    tool_name: String,
    tool_input_json: String,
    tool_output_json: Option<String>,
    status: String,
    approval_status: Option<String>,
    started_at: String,
    finished_at: Option<String>,
}

impl ToolCallRow {
    fn into_dto(self) -> ToolCallDto {
        ToolCallDto {
            id: self.tool_call_id,
            storage_id: self.storage_id,
            run_id: self.run_id,
            thread_id: self.thread_id,
            helper_id: self.helper_id,
            tool_name: self.tool_name,
            tool_input: serde_json::from_str(&self.tool_input_json)
                .unwrap_or(serde_json::Value::String(self.tool_input_json)),
            tool_output: self
                .tool_output_json
                .and_then(|value| serde_json::from_str(&value).ok()),
            status: self.status,
            approval_status: self.approval_status,
            started_at: self.started_at,
            finished_at: self.finished_at,
        }
    }
}

pub struct ToolCallInsert {
    pub id: String,
    pub tool_call_id: String,
    pub run_id: String,
    pub thread_id: String,
    pub helper_id: Option<String>,
    pub tool_name: String,
    pub tool_input_json: String,
    pub status: String,
}

pub async fn insert(pool: &SqlitePool, r: &ToolCallInsert) -> Result<(), AppError> {
    let now = Utc::now().to_rfc3339();
    sqlx::query(
        "INSERT INTO tool_calls (id, run_id, thread_id, helper_id, tool_name, tool_input_json, status, started_at, tool_call_id)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&r.id)
    .bind(&r.run_id)
    .bind(&r.thread_id)
    .bind(&r.helper_id)
    .bind(&r.tool_name)
    .bind(&r.tool_input_json)
    .bind(&r.status)
    .bind(&now)
    .bind(&r.tool_call_id)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn update_status(
    pool: &SqlitePool,
    storage_id: &str,
    status: &str,
) -> Result<(), AppError> {
    let is_terminal = matches!(status, "completed" | "failed" | "denied" | "cancelled");

    if is_terminal {
        let now = Utc::now().to_rfc3339();
        sqlx::query("UPDATE tool_calls SET status = ?, finished_at = ? WHERE id = ?")
            .bind(status)
            .bind(&now)
            .bind(storage_id)
            .execute(pool)
            .await?;
    } else {
        sqlx::query("UPDATE tool_calls SET status = ? WHERE id = ?")
            .bind(status)
            .bind(storage_id)
            .execute(pool)
            .await?;
    }
    Ok(())
}

pub async fn update_result(
    pool: &SqlitePool,
    storage_id: &str,
    output_json: &str,
    status: &str,
) -> Result<(), AppError> {
    let now = Utc::now().to_rfc3339();
    sqlx::query(
        "UPDATE tool_calls SET tool_output_json = ?, status = ?, finished_at = ? WHERE id = ?",
    )
    .bind(output_json)
    .bind(status)
    .bind(&now)
    .bind(storage_id)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn update_approval(
    pool: &SqlitePool,
    storage_id: &str,
    approval_status: &str,
    tool_status: &str,
) -> Result<(), AppError> {
    sqlx::query("UPDATE tool_calls SET approval_status = ?, status = ? WHERE id = ?")
        .bind(approval_status)
        .bind(tool_status)
        .bind(storage_id)
        .execute(pool)
        .await?;
    Ok(())
}

/// Mark all non-terminal tool calls as cancelled (crash recovery).
pub async fn interrupt_active_tool_calls(pool: &SqlitePool) -> Result<u64, AppError> {
    let result = sqlx::query(
        "UPDATE tool_calls
         SET status = 'cancelled',
             finished_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
         WHERE status NOT IN ('completed', 'failed', 'denied', 'cancelled')
           AND finished_at IS NULL",
    )
    .execute(pool)
    .await?;

    Ok(result.rows_affected())
}

pub async fn list_by_run_ids(
    pool: &SqlitePool,
    run_ids: &[String],
) -> Result<Vec<ToolCallDto>, AppError> {
    list_by_run_ids_with_visibility(pool, run_ids, ToolCallVisibility::All).await
}

pub async fn list_parent_visible_by_run_ids(
    pool: &SqlitePool,
    run_ids: &[String],
) -> Result<Vec<ToolCallDto>, AppError> {
    list_by_run_ids_with_visibility(pool, run_ids, ToolCallVisibility::ParentVisible).await
}

enum ToolCallVisibility {
    All,
    ParentVisible,
}

async fn list_by_run_ids_with_visibility(
    pool: &SqlitePool,
    run_ids: &[String],
    visibility: ToolCallVisibility,
) -> Result<Vec<ToolCallDto>, AppError> {
    if run_ids.is_empty() {
        return Ok(Vec::new());
    }

    let mut query = QueryBuilder::new(
        "SELECT id AS storage_id, COALESCE(tool_call_id, id) AS tool_call_id, run_id, thread_id, helper_id, tool_name, tool_input_json, tool_output_json,
                status, approval_status, started_at, finished_at
         FROM tool_calls
         WHERE run_id IN (",
    );
    {
        let mut separated = query.separated(", ");
        for run_id in run_ids {
            separated.push_bind(run_id);
        }
    }
    query.push(")");
    if matches!(visibility, ToolCallVisibility::ParentVisible) {
        // `helper_id IS NULL` is the durable schema marker for top-level model tool calls.
        // The colon check is a legacy-compatibility guard for helper-internal rows
        // persisted before `helper_id` existed, whose ids were stored as
        // `{helper-id-prefix}:{provider-tool-call-id}`. New helper rows should be
        // excluded by `helper_id`, not by depending on provider id formatting.
        query.push(" AND helper_id IS NULL AND instr(COALESCE(tool_call_id, id), ':') = 0");
    }
    query.push(" ORDER BY started_at ASC, id ASC");

    let rows = query
        .build_query_as::<ToolCallRow>()
        .fetch_all(pool)
        .await?;

    Ok(rows.into_iter().map(ToolCallRow::into_dto).collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
    use sqlx::Row;
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

        for thread_id in ["t1", "t2"] {
            sqlx::query(
                "INSERT INTO threads (id, workspace_id, title, status, created_at, updated_at, last_active_at)
                 VALUES (?, 'ws-1', 't', 'idle',
                         strftime('%Y-%m-%dT%H:%M:%fZ', 'now'),
                         strftime('%Y-%m-%dT%H:%M:%fZ', 'now'),
                         strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))",
            )
            .bind(thread_id)
            .execute(&pool)
            .await
            .expect("seed thread");
        }

        for (run_id, thread_id) in [("r1", "t1"), ("r2", "t2")] {
            sqlx::query(
                "INSERT INTO thread_runs (id, thread_id, run_mode, status, started_at)
                 VALUES (?, ?, 'default', 'running', strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))",
            )
            .bind(run_id)
            .bind(thread_id)
            .execute(&pool)
            .await
            .expect("seed run");
        }

        pool
    }

    fn insert_record(
        storage_id: &str,
        raw_id: &str,
        run_id: &str,
        thread_id: &str,
    ) -> ToolCallInsert {
        insert_record_with_helper(storage_id, raw_id, run_id, thread_id, None)
    }

    fn insert_record_with_helper(
        storage_id: &str,
        raw_id: &str,
        run_id: &str,
        thread_id: &str,
        helper_id: Option<&str>,
    ) -> ToolCallInsert {
        ToolCallInsert {
            id: storage_id.to_string(),
            tool_call_id: raw_id.to_string(),
            run_id: run_id.to_string(),
            thread_id: thread_id.to_string(),
            helper_id: helper_id.map(str::to_string),
            tool_name: "shell".to_string(),
            tool_input_json: serde_json::json!({ "command": "git status" }).to_string(),
            status: "requested".to_string(),
        }
    }

    #[tokio::test]
    async fn duplicate_runtime_tool_call_ids_are_allowed_across_runs() {
        let pool = setup_test_pool().await;

        insert(&pool, &insert_record("storage-1", "shell:3", "r1", "t1"))
            .await
            .expect("insert first tool call");
        insert(&pool, &insert_record("storage-2", "shell:3", "r2", "t2"))
            .await
            .expect("insert second tool call with same runtime id");

        update_result(
            &pool,
            "storage-2",
            &serde_json::json!({ "ok": true }).to_string(),
            "completed",
        )
        .await
        .expect("update by storage id");

        let calls = list_by_run_ids(&pool, &["r1".to_string(), "r2".to_string()])
            .await
            .expect("list tool calls");
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[0].id, "shell:3");
        assert_eq!(calls[0].storage_id, "storage-1");
        assert_eq!(calls[0].status, "requested");
        assert_eq!(calls[1].id, "shell:3");
        assert_eq!(calls[1].storage_id, "storage-2");
        assert_eq!(calls[1].status, "completed");

        let first_status: String =
            sqlx::query("SELECT status FROM tool_calls WHERE id = 'storage-1'")
                .fetch_one(&pool)
                .await
                .expect("first row")
                .get("status");
        assert_eq!(first_status, "requested");
    }

    #[tokio::test]
    async fn duplicate_runtime_tool_call_ids_are_rejected_within_same_run() {
        let pool = setup_test_pool().await;

        insert(&pool, &insert_record("storage-1", "shell:3", "r1", "t1"))
            .await
            .expect("insert first tool call");

        let duplicate = insert(&pool, &insert_record("storage-2", "shell:3", "r1", "t1")).await;
        assert!(
            duplicate.is_err(),
            "same run should not reuse a runtime tool call id"
        );
    }

    #[tokio::test]
    async fn list_by_run_ids_falls_back_to_storage_id_for_legacy_rows() {
        let pool = setup_test_pool().await;

        sqlx::query(
            "INSERT INTO tool_calls (id, run_id, thread_id, tool_name, tool_input_json, status, started_at, tool_call_id)
             VALUES ('legacy-storage-1', 'r1', 't1', 'shell', '{}', 'completed', strftime('%Y-%m-%dT%H:%M:%fZ', 'now'), NULL)",
        )
        .execute(&pool)
        .await
        .expect("insert legacy tool call");

        let calls = list_by_run_ids(&pool, &["r1".to_string()])
            .await
            .expect("list tool calls");

        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].id, "legacy-storage-1");
        assert_eq!(calls[0].storage_id, "legacy-storage-1");
    }

    #[tokio::test]
    async fn list_parent_visible_by_run_ids_excludes_helper_internal_calls() {
        let pool = setup_test_pool().await;

        insert(
            &pool,
            &insert_record("storage-parent", "call_parent", "r1", "t1"),
        )
        .await
        .expect("insert parent tool call");
        insert(
            &pool,
            &insert_record_with_helper(
                "storage-helper",
                "019dbed5:call_helper",
                "r1",
                "t1",
                Some("helper-1"),
            ),
        )
        .await
        .expect("insert helper tool call");
        insert(
            &pool,
            &insert_record("storage-legacy-helper", "019dbed5:call_legacy", "r1", "t1"),
        )
        .await
        .expect("insert legacy helper tool call");

        let all_calls = list_by_run_ids(&pool, &["r1".to_string()])
            .await
            .expect("list all tool calls");
        assert_eq!(all_calls.len(), 3);
        assert_eq!(all_calls[1].helper_id.as_deref(), Some("helper-1"));

        let parent_visible = list_parent_visible_by_run_ids(&pool, &["r1".to_string()])
            .await
            .expect("list parent-visible tool calls");

        assert_eq!(parent_visible.len(), 1);
        assert_eq!(parent_visible[0].id, "call_parent");
        assert!(parent_visible[0].helper_id.is_none());
    }

    #[tokio::test]
    async fn update_operations_target_rows_by_storage_id_when_runtime_ids_overlap() {
        let pool = setup_test_pool().await;

        insert(&pool, &insert_record("storage-1", "shell:3", "r1", "t1"))
            .await
            .expect("insert first tool call");
        insert(&pool, &insert_record("storage-2", "shell:3", "r2", "t2"))
            .await
            .expect("insert second tool call with same runtime id");

        update_status(&pool, "storage-1", "running")
            .await
            .expect("update first status by storage id");
        update_approval(&pool, "storage-2", "approved", "approved")
            .await
            .expect("update second approval by storage id");
        update_result(
            &pool,
            "storage-2",
            &serde_json::json!({ "ok": true }).to_string(),
            "completed",
        )
        .await
        .expect("update second result by storage id");

        let first = sqlx::query(
            "SELECT status, approval_status, tool_output_json FROM tool_calls WHERE id = 'storage-1'",
        )
        .fetch_one(&pool)
        .await
        .expect("first row");
        assert_eq!(first.get::<String, _>("status"), "running");
        assert!(first.get::<Option<String>, _>("approval_status").is_none());
        assert!(first.get::<Option<String>, _>("tool_output_json").is_none());

        let second = sqlx::query(
            "SELECT status, approval_status, tool_output_json FROM tool_calls WHERE id = 'storage-2'",
        )
        .fetch_one(&pool)
        .await
        .expect("second row");
        assert_eq!(second.get::<String, _>("status"), "completed");
        assert_eq!(
            second
                .get::<Option<String>, _>("approval_status")
                .as_deref(),
            Some("approved")
        );
        assert_eq!(
            second
                .get::<Option<String>, _>("tool_output_json")
                .as_deref(),
            Some(r#"{"ok":true}"#)
        );
    }
}
