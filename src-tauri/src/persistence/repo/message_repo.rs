use sqlx::SqlitePool;

use crate::model::errors::AppError;
use crate::model::thread::MessageRecord;

#[derive(sqlx::FromRow)]
struct MessageRow {
    id: String,
    thread_id: String,
    run_id: Option<String>,
    role: String,
    content_markdown: String,
    message_type: String,
    status: String,
    metadata_json: Option<String>,
    attachments_json: Option<String>,
    created_at: String,
}

impl MessageRow {
    fn into_record(self) -> MessageRecord {
        MessageRecord {
            id: self.id,
            thread_id: self.thread_id,
            run_id: self.run_id,
            role: self.role,
            content_markdown: self.content_markdown,
            message_type: self.message_type,
            status: self.status,
            metadata_json: self.metadata_json,
            attachments_json: self.attachments_json,
            created_at: self.created_at,
        }
    }
}

pub async fn find_by_id(pool: &SqlitePool, id: &str) -> Result<Option<MessageRecord>, AppError> {
    let row = sqlx::query_as::<_, MessageRow>(
        "SELECT id, thread_id, run_id, role, content_markdown, message_type,
                status, metadata_json, attachments_json, created_at
         FROM messages
         WHERE id = ?
         LIMIT 1",
    )
    .bind(id)
    .fetch_optional(pool)
    .await?;

    Ok(row.map(MessageRow::into_record))
}

/// Load recent messages for a thread using cursor-based pagination.
/// `before_id` is the UUID v7 cursor — returns messages older than this ID.
pub async fn list_recent(
    pool: &SqlitePool,
    thread_id: &str,
    before_id: Option<&str>,
    limit: i64,
) -> Result<Vec<MessageRecord>, AppError> {
    let rows = if let Some(cursor) = before_id {
        sqlx::query_as::<_, MessageRow>(
            "SELECT id, thread_id, run_id, role, content_markdown, message_type,
                    status, metadata_json, attachments_json, created_at
             FROM messages
             WHERE thread_id = ? AND id < ?
             ORDER BY id DESC
             LIMIT ?",
        )
        .bind(thread_id)
        .bind(cursor)
        .bind(limit)
        .fetch_all(pool)
        .await?
    } else {
        sqlx::query_as::<_, MessageRow>(
            "SELECT id, thread_id, run_id, role, content_markdown, message_type,
                    status, metadata_json, attachments_json, created_at
             FROM messages
             WHERE thread_id = ?
             ORDER BY id DESC
             LIMIT ?",
        )
        .bind(thread_id)
        .bind(limit)
        .fetch_all(pool)
        .await?
    };

    // Reverse to chronological order (oldest first)
    let mut records: Vec<MessageRecord> = rows.into_iter().map(|r| r.into_record()).collect();
    records.reverse();
    Ok(records)
}

/// Load all messages since the last context reset marker for a thread.
///
/// A context reset marker is a `summary_marker` message with `kind = "context_reset"`
/// in its metadata JSON.
///
/// Strategy:
/// 1. Query the DB for the most recent context_reset marker (uses the partial index
///    `idx_messages_type` on `(thread_id, message_type)` for summary_marker rows).
/// 2. If the reset marker's metadata contains `boundaryMessageId`, use it as the
///    lower bound. This handles auto-compression where the reset marker is written
///    *after* the recent messages it meant to preserve (UUID v7 timing issue) —
///    the boundary id points at the first DB message we want to keep, regardless
///    of the reset marker's own id.
/// 3. Otherwise, load all messages whose id >= reset.id (legacy `/clear` + `/compact`
///    behavior; those writes happen while the thread is idle so timing is safe).
/// 4. If no reset marker exists, load the most recent messages (up to `SAFETY_LIMIT`).
/// 5. Discarded messages (`status = "discarded"`) are filtered out.
pub async fn list_since_last_reset(
    pool: &SqlitePool,
    thread_id: &str,
) -> Result<Vec<MessageRecord>, AppError> {
    const SAFETY_LIMIT: i64 = 2000;

    let reset = find_last_context_reset(pool, thread_id).await?;

    let mut rows = match reset.as_ref() {
        Some(marker) => {
            // Prefer boundaryMessageId if present (auto-compression case);
            // else use the reset marker's own id (legacy /clear, /compact).
            let cursor = marker
                .boundary_message_id
                .clone()
                .unwrap_or_else(|| marker.id.clone());
            sqlx::query_as::<_, MessageRow>(
                "SELECT id, thread_id, run_id, role, content_markdown, message_type,
                        status, metadata_json, attachments_json, created_at
                 FROM messages
                 WHERE thread_id = ? AND id >= ? AND status != 'discarded'
                 ORDER BY id DESC
                 LIMIT ?",
            )
            .bind(thread_id)
            .bind(cursor)
            .bind(SAFETY_LIMIT)
            .fetch_all(pool)
            .await?
        }
        None => {
            sqlx::query_as::<_, MessageRow>(
                "SELECT id, thread_id, run_id, role, content_markdown, message_type,
                        status, metadata_json, attachments_json, created_at
                 FROM messages
                 WHERE thread_id = ? AND status != 'discarded'
                 ORDER BY id DESC
                 LIMIT ?",
            )
            .bind(thread_id)
            .bind(SAFETY_LIMIT)
            .fetch_all(pool)
            .await?
        }
    };

    // Reverse to restore chronological (ASC) order after the DESC fetch.
    rows.reverse();

    Ok(rows.into_iter().map(|r| r.into_record()).collect())
}

/// Return the id of the Nth most-recent non-discarded message in a thread.
///
/// Used by auto-compression to resolve a DB-backed boundary id for the
/// `boundaryMessageId` metadata field (see `list_since_last_reset`).
///
/// `n_from_end` is 1-based; `n_from_end == 1` returns the newest message id.
/// Returns `Ok(None)` if the thread has fewer than `n_from_end` non-discarded
/// messages.
pub async fn find_nth_from_end_id(
    pool: &SqlitePool,
    thread_id: &str,
    n_from_end: usize,
) -> Result<Option<String>, AppError> {
    if n_from_end == 0 {
        return Ok(None);
    }

    let offset = (n_from_end as i64).saturating_sub(1);
    let row: Option<(String,)> = sqlx::query_as(
        "SELECT id FROM messages
         WHERE thread_id = ? AND status != 'discarded'
         ORDER BY id DESC
         LIMIT 1 OFFSET ?",
    )
    .bind(thread_id)
    .bind(offset)
    .fetch_optional(pool)
    .await?;

    Ok(row.map(|(id,)| id))
}

/// Lightweight row for the context-reset-marker lookup (only the columns we need).
#[derive(sqlx::FromRow)]
struct MarkerRow {
    id: String,
    metadata_json: Option<String>,
}

/// Context-reset marker details used by `list_since_last_reset`.
struct ContextResetMarker {
    id: String,
    boundary_message_id: Option<String>,
}

/// Find the most recent context_reset summary_marker in a thread, including
/// any `boundaryMessageId` hint embedded in its metadata.
///
/// Uses the partial index on `(thread_id, message_type)` to efficiently scan
/// only `summary_marker` rows, then checks metadata in Rust for the `context_reset`
/// kind (consistent with the existing application-layer check pattern).
/// Discarded markers are skipped.
async fn find_last_context_reset(
    pool: &SqlitePool,
    thread_id: &str,
) -> Result<Option<ContextResetMarker>, AppError> {
    // Load the most recent summary_marker messages; typically very few exist.
    let rows = sqlx::query_as::<_, MarkerRow>(
        "SELECT id, metadata_json
         FROM messages
         WHERE thread_id = ? AND message_type = 'summary_marker' AND status != 'discarded'
         ORDER BY id DESC
         LIMIT 50",
    )
    .bind(thread_id)
    .fetch_all(pool)
    .await?;

    for row in rows {
        if metadata_kind_matches(row.metadata_json.as_deref(), "context_reset") {
            let boundary_message_id = extract_boundary_message_id(row.metadata_json.as_deref());
            return Ok(Some(ContextResetMarker {
                id: row.id,
                boundary_message_id,
            }));
        }
    }
    Ok(None)
}

/// Extract the optional `boundaryMessageId` string from a reset marker's metadata.
fn extract_boundary_message_id(raw: Option<&str>) -> Option<String> {
    let value: serde_json::Value = serde_json::from_str(raw?).ok()?;
    let id = value.get("boundaryMessageId")?.as_str()?;
    if id.is_empty() {
        None
    } else {
        Some(id.to_string())
    }
}

/// Insert a new message (append-only).
pub async fn insert(pool: &SqlitePool, record: &MessageRecord) -> Result<(), AppError> {
    sqlx::query(
        "INSERT INTO messages (id, thread_id, run_id, role, content_markdown,
                message_type, status, metadata_json, attachments_json, created_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))",
    )
    .bind(&record.id)
    .bind(&record.thread_id)
    .bind(&record.run_id)
    .bind(&record.role)
    .bind(&record.content_markdown)
    .bind(&record.message_type)
    .bind(&record.status)
    .bind(&record.metadata_json)
    .bind(&record.attachments_json)
    .execute(pool)
    .await?;

    Ok(())
}

/// Update message status (e.g. streaming → completed).
pub async fn update_status(pool: &SqlitePool, id: &str, status: &str) -> Result<(), AppError> {
    sqlx::query("UPDATE messages SET status = ? WHERE id = ?")
        .bind(status)
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}

/// Append content to an existing streaming message.
pub async fn append_content(pool: &SqlitePool, id: &str, delta: &str) -> Result<(), AppError> {
    sqlx::query("UPDATE messages SET content_markdown = content_markdown || ? WHERE id = ?")
        .bind(delta)
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}

/// Replace the full content of an existing message.
pub async fn replace_content(pool: &SqlitePool, id: &str, content: &str) -> Result<(), AppError> {
    sqlx::query("UPDATE messages SET content_markdown = ? WHERE id = ?")
        .bind(content)
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn update_metadata(
    pool: &SqlitePool,
    id: &str,
    metadata_json: Option<&str>,
) -> Result<(), AppError> {
    sqlx::query("UPDATE messages SET metadata_json = ? WHERE id = ?")
        .bind(metadata_json)
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}

/// Discard reasoning messages left in streaming state (from interrupted runs).
/// Called during startup recovery to clean up orphaned reasoning.
pub async fn discard_dangling_reasoning(pool: &SqlitePool) -> Result<u64, AppError> {
    let result = sqlx::query(
        "UPDATE messages SET status = 'discarded' \
         WHERE status = 'streaming' AND message_type = 'reasoning'",
    )
    .execute(pool)
    .await?;
    Ok(result.rows_affected())
}

/// Helper: Check if metadata contains the expected kind value.
fn metadata_kind_matches(raw: Option<&str>, expected: &str) -> bool {
    if let Some(json_str) = raw {
        if let Ok(value) = serde_json::from_str::<serde_json::Value>(json_str) {
            if let Some(kind_str) = value.get("kind").and_then(serde_json::Value::as_str) {
                return kind_str == expected;
            }
        }
    }
    false
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
        .expect("seed thread");

        pool
    }

    fn msg(id: &str, role: &str, content: &str) -> MessageRecord {
        MessageRecord {
            id: id.to_string(),
            thread_id: "t1".to_string(),
            run_id: None,
            role: role.to_string(),
            content_markdown: content.to_string(),
            message_type: "plain_message".to_string(),
            status: "completed".to_string(),
            metadata_json: None,
            attachments_json: None,
            created_at: String::new(),
        }
    }

    fn reset_marker(id: &str, metadata: serde_json::Value) -> MessageRecord {
        MessageRecord {
            id: id.to_string(),
            thread_id: "t1".to_string(),
            run_id: None,
            role: "system".to_string(),
            content_markdown: "Context is now reset".to_string(),
            message_type: "summary_marker".to_string(),
            status: "completed".to_string(),
            metadata_json: Some(metadata.to_string()),
            attachments_json: None,
            created_at: String::new(),
        }
    }

    #[tokio::test]
    async fn find_nth_from_end_id_returns_expected_positions() {
        let pool = setup_test_pool().await;

        // UUID v7 ids are sortable; use monotonically increasing ids for the test.
        insert(&pool, &msg("01", "user", "a")).await.unwrap();
        insert(&pool, &msg("02", "assistant", "b")).await.unwrap();
        insert(&pool, &msg("03", "user", "c")).await.unwrap();
        insert(&pool, &msg("04", "assistant", "d")).await.unwrap();

        assert_eq!(
            find_nth_from_end_id(&pool, "t1", 1)
                .await
                .unwrap()
                .as_deref(),
            Some("04")
        );
        assert_eq!(
            find_nth_from_end_id(&pool, "t1", 2)
                .await
                .unwrap()
                .as_deref(),
            Some("03")
        );
        assert_eq!(
            find_nth_from_end_id(&pool, "t1", 4)
                .await
                .unwrap()
                .as_deref(),
            Some("01")
        );
        // Overshooting returns None rather than erroring.
        assert!(find_nth_from_end_id(&pool, "t1", 99)
            .await
            .unwrap()
            .is_none());
    }

    #[tokio::test]
    async fn list_since_last_reset_honors_boundary_message_id_even_when_reset_id_is_larger() {
        // Simulates the UUID v7 timing issue: the run persists user + assistant
        // messages first, and only *afterwards* the auto-compression writes its
        // reset marker. The reset marker's own id is therefore LARGER than the
        // messages we want to preserve. Without `boundaryMessageId`, those
        // earlier messages would be excluded by `id >= reset_id`.
        let pool = setup_test_pool().await;

        // Pretend these are already-persisted current-run messages:
        insert(&pool, &msg("10", "user", "ask for help"))
            .await
            .unwrap();
        insert(&pool, &msg("11", "assistant", "here is part 1"))
            .await
            .unwrap();
        insert(&pool, &msg("12", "assistant", "here is part 2"))
            .await
            .unwrap();
        // Then auto-compression writes its reset + summary markers LATER:
        insert(
            &pool,
            &reset_marker(
                "20",
                serde_json::json!({
                    "kind": "context_reset",
                    "source": "auto",
                    "boundaryMessageId": "10",
                }),
            ),
        )
        .await
        .unwrap();
        insert(
            &pool,
            &MessageRecord {
                id: "21".to_string(),
                thread_id: "t1".to_string(),
                run_id: None,
                role: "system".to_string(),
                content_markdown: "<context_summary>state</context_summary>".to_string(),
                message_type: "summary_marker".to_string(),
                status: "completed".to_string(),
                metadata_json: Some(serde_json::json!({"kind": "context_summary"}).to_string()),
                attachments_json: None,
                created_at: String::new(),
            },
        )
        .await
        .unwrap();

        let messages = list_since_last_reset(&pool, "t1").await.unwrap();
        // The earlier user + assistant messages must still appear because the
        // boundaryMessageId hint points at them, even though their ids are
        // smaller than the reset marker's id.
        let ids: Vec<&str> = messages.iter().map(|m| m.id.as_str()).collect();
        assert_eq!(ids, vec!["10", "11", "12", "20", "21"]);
    }

    #[tokio::test]
    async fn list_since_last_reset_falls_back_to_reset_id_when_boundary_absent() {
        // Legacy /clear, /compact path: reset markers don't carry
        // boundaryMessageId because they are written while the thread is idle,
        // and the reset marker's own id is a safe lower bound.
        let pool = setup_test_pool().await;
        insert(&pool, &msg("10", "user", "ancient")).await.unwrap();
        insert(
            &pool,
            &reset_marker(
                "50",
                serde_json::json!({"kind": "context_reset", "source": "clear"}),
            ),
        )
        .await
        .unwrap();
        insert(&pool, &msg("60", "user", "after reset"))
            .await
            .unwrap();

        let messages = list_since_last_reset(&pool, "t1").await.unwrap();
        let ids: Vec<&str> = messages.iter().map(|m| m.id.as_str()).collect();
        assert_eq!(ids, vec!["50", "60"]);
    }

    #[tokio::test]
    async fn list_since_last_reset_supports_manual_title_tail_windowing_after_boundary() {
        let pool = setup_test_pool().await;

        for index in 0..140 {
            let id = format!("{:03}", index + 1);
            let role = if index % 2 == 0 { "user" } else { "assistant" };
            insert(&pool, &msg(&id, role, &format!("message-{index}")))
                .await
                .unwrap();
        }

        insert(
            &pool,
            &reset_marker(
                "200",
                serde_json::json!({
                    "kind": "context_reset",
                    "source": "auto",
                    "boundaryMessageId": "050",
                }),
            ),
        )
        .await
        .unwrap();
        insert(
            &pool,
            &MessageRecord {
                id: "201".to_string(),
                thread_id: "t1".to_string(),
                run_id: None,
                role: "system".to_string(),
                content_markdown: "<context_summary>state</context_summary>".to_string(),
                message_type: "summary_marker".to_string(),
                status: "completed".to_string(),
                metadata_json: Some(serde_json::json!({"kind": "context_summary"}).to_string()),
                attachments_json: None,
                created_at: String::new(),
            },
        )
        .await
        .unwrap();

        let messages = list_since_last_reset(&pool, "t1").await.unwrap();
        let tail_start = messages.len().saturating_sub(128);
        let recent_tail = &messages[tail_start..];
        let relevant: Vec<&MessageRecord> = recent_tail
            .iter()
            .filter(|message| {
                message.message_type == "plain_message"
                    && (message.role == "user" || message.role == "assistant")
            })
            .collect();
        let relevant_start = relevant.len().saturating_sub(24);
        let recent_relevant: Vec<&MessageRecord> = relevant[relevant_start..].to_vec();
        let ids: Vec<&str> = recent_relevant
            .iter()
            .map(|message| message.id.as_str())
            .collect();

        assert_eq!(ids.len(), 24);
        assert_eq!(ids.first().copied(), Some("117"));
        assert_eq!(ids.last().copied(), Some("140"));
    }

    #[test]
    fn extract_boundary_message_id_is_tolerant_of_malformed_metadata() {
        assert_eq!(extract_boundary_message_id(None), None);
        assert_eq!(extract_boundary_message_id(Some("")), None);
        assert_eq!(extract_boundary_message_id(Some("not-json")), None);
        // Empty string values are treated as "no boundary".
        assert_eq!(
            extract_boundary_message_id(Some(r#"{"boundaryMessageId":""}"#)),
            None
        );
        // Wrong type — boundary is numeric instead of string.
        assert_eq!(
            extract_boundary_message_id(Some(r#"{"boundaryMessageId":123}"#)),
            None
        );
        assert_eq!(
            extract_boundary_message_id(Some(
                r#"{"kind":"context_reset","boundaryMessageId":"msg-42"}"#
            ))
            .as_deref(),
            Some("msg-42")
        );
    }
}
