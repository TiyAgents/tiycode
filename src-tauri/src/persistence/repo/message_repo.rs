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
/// 2. If found, load all messages whose id >= that marker (UUID v7 ordering).
/// 3. If no reset marker exists, load the most recent messages (up to `SAFETY_LIMIT`).
/// 4. Discarded messages (`status = "discarded"`) are filtered out.
pub async fn list_since_last_reset(
    pool: &SqlitePool,
    thread_id: &str,
) -> Result<Vec<MessageRecord>, AppError> {
    const SAFETY_LIMIT: i64 = 2000;

    let reset_id = find_last_context_reset_id(pool, thread_id).await?;

    let mut rows = match reset_id.as_deref() {
        Some(cursor) => {
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

/// Lightweight row for the context-reset-marker lookup (only the columns we need).
#[derive(sqlx::FromRow)]
struct MarkerRow {
    id: String,
    metadata_json: Option<String>,
}

/// Find the ID of the last context_reset summary_marker in a thread.
///
/// Uses the partial index on `(thread_id, message_type)` to efficiently scan
/// only `summary_marker` rows, then checks metadata in Rust for the `context_reset`
/// kind (consistent with the existing application-layer check pattern).
/// Discarded markers are skipped.
async fn find_last_context_reset_id(
    pool: &SqlitePool,
    thread_id: &str,
) -> Result<Option<String>, AppError> {
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
            return Ok(Some(row.id));
        }
    }
    Ok(None)
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
