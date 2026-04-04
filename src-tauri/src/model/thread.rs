use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// ThreadStatus — derived from latest run state
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ThreadStatus {
    Idle,
    Running,
    WaitingApproval,
    NeedsReply,
    Interrupted,
    Failed,
    Archived,
}

impl ThreadStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Idle => "idle",
            Self::Running => "running",
            Self::WaitingApproval => "waiting_approval",
            Self::NeedsReply => "needs_reply",
            Self::Interrupted => "interrupted",
            Self::Failed => "failed",
            Self::Archived => "archived",
        }
    }

    pub fn from_str(s: &str) -> Self {
        match s {
            "idle" => Self::Idle,
            "running" => Self::Running,
            "waiting_approval" => Self::WaitingApproval,
            "needs_reply" => Self::NeedsReply,
            "interrupted" => Self::Interrupted,
            "failed" => Self::Failed,
            "archived" => Self::Archived,
            _ => Self::Idle,
        }
    }
}

// ---------------------------------------------------------------------------
// ThreadRecord
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct ThreadRecord {
    pub id: String,
    pub workspace_id: String,
    pub title: String,
    pub status: ThreadStatus,
    pub summary: Option<String>,
    pub last_active_at: String,
    pub created_at: String,
    pub updated_at: String,
}

/// Lightweight DTO for thread list (sidebar).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreadSummaryDto {
    pub id: String,
    pub workspace_id: String,
    pub title: String,
    pub status: ThreadStatus,
    pub last_active_at: String,
    pub created_at: String,
}

impl From<ThreadRecord> for ThreadSummaryDto {
    fn from(r: ThreadRecord) -> Self {
        Self {
            id: r.id,
            workspace_id: r.workspace_id,
            title: r.title,
            status: r.status,
            last_active_at: r.last_active_at,
            created_at: r.created_at,
        }
    }
}

// ---------------------------------------------------------------------------
// MessageRecord
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MessageAttachmentDto {
    pub id: String,
    pub name: String,
    pub media_type: Option<String>,
    pub url: Option<String>,
}

#[derive(Debug, Clone)]
pub struct MessageRecord {
    pub id: String,
    pub thread_id: String,
    pub run_id: Option<String>,
    pub role: String,
    pub content_markdown: String,
    pub message_type: String,
    pub status: String,
    pub metadata_json: Option<String>,
    pub attachments_json: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MessageDto {
    pub id: String,
    pub thread_id: String,
    pub run_id: Option<String>,
    pub role: String,
    pub content_markdown: String,
    pub message_type: String,
    pub status: String,
    pub metadata: Option<serde_json::Value>,
    pub attachments: Vec<MessageAttachmentDto>,
    pub created_at: String,
}

impl From<MessageRecord> for MessageDto {
    fn from(r: MessageRecord) -> Self {
        Self {
            id: r.id,
            thread_id: r.thread_id,
            run_id: r.run_id,
            role: r.role,
            content_markdown: r.content_markdown,
            message_type: r.message_type,
            status: r.status,
            metadata: r.metadata_json.and_then(|s| serde_json::from_str(&s).ok()),
            attachments: r
                .attachments_json
                .and_then(|s| serde_json::from_str(&s).ok())
                .unwrap_or_default(),
            created_at: r.created_at,
        }
    }
}

// ---------------------------------------------------------------------------
// RunSummary — lightweight run info for snapshots
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RunUsageDto {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_read_tokens: u64,
    pub cache_write_tokens: u64,
    pub total_tokens: u64,
}

impl From<tiy_core::types::Usage> for RunUsageDto {
    fn from(value: tiy_core::types::Usage) -> Self {
        Self {
            input_tokens: value.input,
            output_tokens: value.output,
            cache_read_tokens: value.cache_read,
            cache_write_tokens: value.cache_write,
            total_tokens: value.total_tokens,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RunSummaryDto {
    pub id: String,
    pub thread_id: String,
    pub run_mode: String,
    pub status: String,
    pub model_id: Option<String>,
    pub model_display_name: Option<String>,
    pub context_window: Option<String>,
    pub error_message: Option<String>,
    pub started_at: String,
    pub usage: RunUsageDto,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolCallDto {
    pub id: String,
    pub run_id: String,
    pub thread_id: String,
    pub tool_name: String,
    pub tool_input: serde_json::Value,
    pub tool_output: Option<serde_json::Value>,
    pub status: String,
    pub approval_status: Option<String>,
    pub started_at: String,
    pub finished_at: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RunHelperDto {
    pub id: String,
    pub run_id: String,
    pub thread_id: String,
    pub helper_kind: String,
    pub parent_tool_call_id: Option<String>,
    pub status: String,
    pub input_summary: Option<String>,
    pub output_summary: Option<String>,
    pub error_summary: Option<String>,
    pub started_at: String,
    pub finished_at: Option<String>,
    pub usage: RunUsageDto,
}

// ---------------------------------------------------------------------------
// ThreadSnapshot — full snapshot for UI recovery and run startup
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreadSnapshotDto {
    pub thread: ThreadSummaryDto,
    pub messages: Vec<MessageDto>,
    pub has_more_messages: bool,
    pub active_run: Option<RunSummaryDto>,
    pub latest_run: Option<RunSummaryDto>,
    pub tool_calls: Vec<ToolCallDto>,
    pub helpers: Vec<RunHelperDto>,
    pub task_boards: Vec<super::task_board::TaskBoardDto>,
    pub active_task_board_id: Option<String>,
}

// ---------------------------------------------------------------------------
// Input types
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AddMessageInput {
    pub role: String,
    pub content: String,
    pub message_type: Option<String>,
    pub metadata: Option<serde_json::Value>,
}
