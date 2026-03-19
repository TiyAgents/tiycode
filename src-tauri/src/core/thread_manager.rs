use sqlx::SqlitePool;

use crate::model::errors::{AppError, ErrorSource};
use crate::model::thread::{
    AddMessageInput, MessageDto, MessageRecord, RunSummaryDto, ThreadRecord, ThreadSnapshotDto,
    ThreadStatus, ThreadSummaryDto,
};
use crate::persistence::repo::{
    message_repo, run_helper_repo, run_repo, thread_repo, tool_call_repo,
};

const DEFAULT_MESSAGE_PAGE_SIZE: i64 = 50;

pub struct ThreadManager {
    pool: SqlitePool,
}

impl ThreadManager {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    // -----------------------------------------------------------------------
    // Thread CRUD
    // -----------------------------------------------------------------------

    /// List threads for a workspace (sidebar query).
    pub async fn list(
        &self,
        workspace_id: &str,
        limit: Option<i64>,
        offset: Option<i64>,
    ) -> Result<Vec<ThreadSummaryDto>, AppError> {
        let records = thread_repo::list_by_workspace(
            &self.pool,
            workspace_id,
            limit.unwrap_or(100),
            offset.unwrap_or(0),
        )
        .await?;

        Ok(records.into_iter().map(ThreadSummaryDto::from).collect())
    }

    /// Create a new thread under a workspace.
    pub async fn create(
        &self,
        workspace_id: &str,
        title: Option<String>,
    ) -> Result<ThreadSummaryDto, AppError> {
        let record = ThreadRecord {
            id: uuid::Uuid::now_v7().to_string(),
            workspace_id: workspace_id.to_string(),
            title: title.unwrap_or_default(),
            status: ThreadStatus::Idle,
            summary: None,
            last_active_at: String::new(),
            created_at: String::new(),
            updated_at: String::new(),
        };

        thread_repo::insert(&self.pool, &record).await?;
        tracing::info!(thread_id = %record.id, workspace_id = %workspace_id, "thread created");

        // Re-fetch for server-set timestamps
        let saved = thread_repo::find_by_id(&self.pool, &record.id)
            .await?
            .ok_or_else(|| AppError::internal(ErrorSource::Thread, "failed to read back thread"))?;

        Ok(ThreadSummaryDto::from(saved))
    }

    /// Load a full thread snapshot (for UI recovery and run startup).
    pub async fn load(
        &self,
        id: &str,
        message_cursor: Option<String>,
        message_limit: Option<i64>,
    ) -> Result<ThreadSnapshotDto, AppError> {
        let thread = thread_repo::find_by_id(&self.pool, id)
            .await?
            .ok_or_else(|| AppError::not_found(ErrorSource::Thread, "thread"))?;

        let limit = message_limit.unwrap_or(DEFAULT_MESSAGE_PAGE_SIZE);

        let messages = message_repo::list_recent(
            &self.pool,
            id,
            message_cursor.as_deref(),
            limit + 1, // fetch one extra to detect has_more
        )
        .await?;

        let has_more = messages.len() as i64 > limit;
        let messages: Vec<MessageDto> = messages
            .into_iter()
            .take(limit as usize)
            .map(MessageDto::from)
            .collect();

        let active_run = run_repo::find_active_by_thread(&self.pool, id).await?;
        let latest_run = run_repo::find_latest_by_thread(&self.pool, id).await?;
        let mut run_ids: Vec<String> = messages
            .iter()
            .filter_map(|message| message.run_id.clone())
            .collect();
        if let Some(run) = active_run.as_ref() {
            if !run_ids.iter().any(|candidate| candidate == &run.id) {
                run_ids.push(run.id.clone());
            }
        }
        if let Some(run) = latest_run.as_ref() {
            if !run_ids.iter().any(|candidate| candidate == &run.id) {
                run_ids.push(run.id.clone());
            }
        }
        let tool_calls = tool_call_repo::list_by_run_ids(&self.pool, &run_ids).await?;
        let helpers = run_helper_repo::list_by_run_ids(&self.pool, &run_ids).await?;

        Ok(ThreadSnapshotDto {
            thread: ThreadSummaryDto::from(thread),
            messages,
            has_more_messages: has_more,
            active_run,
            latest_run,
            tool_calls,
            helpers,
        })
    }

    /// Update thread title.
    pub async fn update_title(&self, id: &str, title: &str) -> Result<(), AppError> {
        thread_repo::update_title(&self.pool, id, title).await
    }

    /// Delete a thread and all its messages.
    pub async fn delete(&self, id: &str) -> Result<(), AppError> {
        let deleted = thread_repo::delete(&self.pool, id).await?;
        if !deleted {
            return Err(AppError::not_found(ErrorSource::Thread, "thread"));
        }
        tracing::info!(thread_id = %id, "thread deleted");
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Messages
    // -----------------------------------------------------------------------

    /// Add a user message to a thread.
    pub async fn add_message(
        &self,
        thread_id: &str,
        input: AddMessageInput,
    ) -> Result<MessageDto, AppError> {
        // Verify thread exists
        thread_repo::find_by_id(&self.pool, thread_id)
            .await?
            .ok_or_else(|| AppError::not_found(ErrorSource::Thread, "thread"))?;

        let record = MessageRecord {
            id: uuid::Uuid::now_v7().to_string(),
            thread_id: thread_id.to_string(),
            run_id: None,
            role: input.role,
            content_markdown: input.content,
            message_type: input
                .message_type
                .unwrap_or_else(|| "plain_message".to_string()),
            status: "completed".to_string(),
            metadata_json: input.metadata.map(|v| v.to_string()),
            created_at: String::new(),
        };

        message_repo::insert(&self.pool, &record).await?;

        // Touch thread activity timestamp
        thread_repo::touch_active(&self.pool, thread_id).await?;

        // Re-read to get server timestamp
        let messages = message_repo::list_recent(&self.pool, thread_id, None, 1).await?;
        let msg = messages.into_iter().last().ok_or_else(|| {
            AppError::internal(ErrorSource::Thread, "failed to read back message")
        })?;

        Ok(MessageDto::from(msg))
    }

    // -----------------------------------------------------------------------
    // Thread status derivation
    // -----------------------------------------------------------------------

    /// Derive and update thread status from the latest run.
    pub async fn sync_status(&self, thread_id: &str) -> Result<ThreadStatus, AppError> {
        let latest_run = run_repo::find_latest_by_thread(&self.pool, thread_id).await?;
        let status = derive_thread_status(latest_run.as_ref());
        thread_repo::update_status(&self.pool, thread_id, &status).await?;
        Ok(status)
    }

    // -----------------------------------------------------------------------
    // Crash recovery
    // -----------------------------------------------------------------------

    /// On app startup, mark any dangling active runs as interrupted,
    /// then sync affected thread statuses.
    pub async fn recover_interrupted_runs(&self) -> Result<(), AppError> {
        let count = run_repo::interrupt_active_runs(&self.pool).await?;
        if count > 0 {
            tracing::warn!(count, "interrupted dangling runs on startup");
        }
        Ok(())
    }
}

/// Derive ThreadStatus from the latest run's status.
fn derive_thread_status(latest_run: Option<&RunSummaryDto>) -> ThreadStatus {
    match latest_run {
        None => ThreadStatus::Idle,
        Some(run) => match run.status.as_str() {
            "created" | "dispatching" | "running" | "waiting_tool_result" | "cancelling" => {
                ThreadStatus::Running
            }
            "waiting_approval" => ThreadStatus::WaitingApproval,
            "interrupted" => ThreadStatus::Interrupted,
            "failed" | "denied" => ThreadStatus::Failed,
            _ => ThreadStatus::Idle, // completed, cancelled → idle
        },
    }
}
