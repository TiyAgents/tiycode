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
