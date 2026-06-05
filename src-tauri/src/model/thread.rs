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
// RunStatus — status of an individual thread run
// ---------------------------------------------------------------------------

/// Authoritative run-level status enum. Serializes to/from the same
/// lowercase snake_case strings stored in SQLite, so no migration is needed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunStatus {
    Created,
    Dispatching,
    Running,
    WaitingApproval,
    NeedsReply,
    WaitingToolResult,
    Cancelling,
    Completed,
    LimitReached,
    Failed,
    Denied,
    Interrupted,
    Cancelled,
}

impl RunStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Created => "created",
            Self::Dispatching => "dispatching",
            Self::Running => "running",
            Self::WaitingApproval => "waiting_approval",
            Self::NeedsReply => "needs_reply",
            Self::WaitingToolResult => "waiting_tool_result",
            Self::Cancelling => "cancelling",
            Self::Completed => "completed",
            Self::LimitReached => "limit_reached",
            Self::Failed => "failed",
            Self::Denied => "denied",
            Self::Interrupted => "interrupted",
            Self::Cancelled => "cancelled",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "created" => Some(Self::Created),
            "dispatching" => Some(Self::Dispatching),
            "running" => Some(Self::Running),
            "waiting_approval" => Some(Self::WaitingApproval),
            "needs_reply" => Some(Self::NeedsReply),
            "waiting_tool_result" => Some(Self::WaitingToolResult),
            "cancelling" => Some(Self::Cancelling),
            "completed" => Some(Self::Completed),
            "limit_reached" => Some(Self::LimitReached),
            "failed" => Some(Self::Failed),
            "denied" => Some(Self::Denied),
            "interrupted" => Some(Self::Interrupted),
            "cancelled" => Some(Self::Cancelled),
            _ => None,
        }
    }

    /// Whether this run has reached a terminal state and will not transition further.
    pub fn is_terminal(&self) -> bool {
        matches!(
            self,
            Self::Completed
                | Self::Failed
                | Self::Denied
                | Self::Interrupted
                | Self::Cancelled
                | Self::LimitReached
        )
    }

    /// Whether this run is actively executing (consuming compute resources).
    pub fn is_active(&self) -> bool {
        matches!(
            self,
            Self::Created
                | Self::Dispatching
                | Self::Running
                | Self::WaitingToolResult
                | Self::Cancelling
        )
    }

    /// Whether this run is paused waiting for user action.
    pub fn is_needs_user_action(&self) -> bool {
        matches!(self, Self::WaitingApproval | Self::NeedsReply)
    }

    /// Derive the thread-level status from this run status.
    pub fn to_thread_status(&self) -> ThreadStatus {
        match self {
            Self::Created
            | Self::Dispatching
            | Self::Running
            | Self::WaitingToolResult
            | Self::Cancelling => ThreadStatus::Running,
            Self::WaitingApproval => ThreadStatus::WaitingApproval,
            Self::NeedsReply | Self::LimitReached => ThreadStatus::NeedsReply,
            Self::Interrupted => ThreadStatus::Interrupted,
            Self::Failed | Self::Denied => ThreadStatus::Failed,
            Self::Completed | Self::Cancelled => ThreadStatus::Idle,
        }
    }

    /// SQL `IN(...)` clause containing all terminal status strings.
    pub fn terminal_sql_in_clause() -> &'static str {
        "('completed','failed','denied','interrupted','cancelled','limit_reached')"
    }

    /// SQL `IN(...)` clause containing all non-progressing statuses
    /// (terminal + needs-user-action). Used to find "truly active" runs.
    pub fn non_progressing_sql_in_clause() -> &'static str {
        "('completed','failed','denied','interrupted','cancelled','limit_reached','waiting_approval','needs_reply')"
    }
}

impl std::fmt::Display for RunStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

// ---------------------------------------------------------------------------
// ThreadRecord
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct ThreadRecord {
    pub id: String,
    pub workspace_id: String,
    pub profile_id: Option<String>,
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
    pub profile_id: Option<String>,
    pub title: String,
    pub status: ThreadStatus,
    pub last_active_at: String,
    pub created_at: String,
    /// Wall-clock start time (Unix ms) of the thread's currently active run,
    /// if any.  Retained as metadata; the workbench header elapsed timer is
    /// driven by `active_run_elapsed_seconds` which excludes pauses.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub active_run_started_at_ms: Option<i64>,
    /// Thread-level cumulative active running seconds used to seed the
    /// workbench header timer. This includes historical completed or
    /// interrupted runs and, when a run is currently running, its in-flight
    /// `running_since` segment. The legacy field name is kept for API
    /// compatibility even though the value is no longer active-run-only.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub active_run_elapsed_seconds: Option<i64>,
}

impl From<ThreadRecord> for ThreadSummaryDto {
    fn from(r: ThreadRecord) -> Self {
        Self {
            id: r.id,
            workspace_id: r.workspace_id,
            profile_id: r.profile_id,
            title: r.title,
            status: r.status,
            last_active_at: r.last_active_at,
            created_at: r.created_at,
            active_run_started_at_ms: None,
            active_run_elapsed_seconds: None,
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
    pub parts_json: Option<String>,
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
    pub parts: Option<Vec<serde_json::Value>>,
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
            parts: r.parts_json.and_then(|s| serde_json::from_str(&s).ok()),
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

impl From<tiycore::types::Usage> for RunUsageDto {
    fn from(value: tiycore::types::Usage) -> Self {
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
    pub storage_id: String,
    pub run_id: String,
    pub thread_id: String,
    pub helper_id: Option<String>,
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

#[cfg(test)]
mod tests {
    use super::{RunStatus, ThreadStatus};

    fn all_run_status_cases() -> Vec<(RunStatus, &'static str, ThreadStatus)> {
        vec![
            (RunStatus::Created, "created", ThreadStatus::Running),
            (RunStatus::Dispatching, "dispatching", ThreadStatus::Running),
            (RunStatus::Running, "running", ThreadStatus::Running),
            (
                RunStatus::WaitingApproval,
                "waiting_approval",
                ThreadStatus::WaitingApproval,
            ),
            (
                RunStatus::NeedsReply,
                "needs_reply",
                ThreadStatus::NeedsReply,
            ),
            (
                RunStatus::WaitingToolResult,
                "waiting_tool_result",
                ThreadStatus::Running,
            ),
            (RunStatus::Cancelling, "cancelling", ThreadStatus::Running),
            (RunStatus::Completed, "completed", ThreadStatus::Idle),
            (
                RunStatus::LimitReached,
                "limit_reached",
                ThreadStatus::NeedsReply,
            ),
            (RunStatus::Failed, "failed", ThreadStatus::Failed),
            (RunStatus::Denied, "denied", ThreadStatus::Failed),
            (
                RunStatus::Interrupted,
                "interrupted",
                ThreadStatus::Interrupted,
            ),
            (RunStatus::Cancelled, "cancelled", ThreadStatus::Idle),
        ]
    }

    #[test]
    fn run_status_string_conversions_round_trip_for_every_variant() {
        for (status, expected, _) in all_run_status_cases() {
            assert_eq!(status.as_str(), expected);
            assert_eq!(RunStatus::from_str(expected), Some(status));
            assert_eq!(status.to_string(), expected);
        }

        assert_eq!(RunStatus::from_str("unknown"), None);
        assert_eq!(RunStatus::from_str(""), None);
    }

    #[test]
    fn run_status_thread_status_mapping_is_explicit_for_every_variant() {
        for (status, _, expected_thread_status) in all_run_status_cases() {
            assert_eq!(status.to_thread_status(), expected_thread_status);
        }
    }

    #[test]
    fn run_status_classification_matches_run_lifecycle_semantics() {
        let terminal = [
            RunStatus::Completed,
            RunStatus::Failed,
            RunStatus::Denied,
            RunStatus::Interrupted,
            RunStatus::Cancelled,
            RunStatus::LimitReached,
        ];
        let active = [
            RunStatus::Created,
            RunStatus::Dispatching,
            RunStatus::Running,
            RunStatus::WaitingToolResult,
            RunStatus::Cancelling,
        ];
        let needs_user_action = [RunStatus::WaitingApproval, RunStatus::NeedsReply];

        for (status, _, _) in all_run_status_cases() {
            assert_eq!(status.is_terminal(), terminal.contains(&status));
            assert_eq!(status.is_active(), active.contains(&status));
            assert_eq!(
                status.is_needs_user_action(),
                needs_user_action.contains(&status)
            );
        }
    }

    #[test]
    fn run_status_sql_clauses_match_status_sets() {
        assert_eq!(
            RunStatus::terminal_sql_in_clause(),
            "('completed','failed','denied','interrupted','cancelled','limit_reached')"
        );
        assert_eq!(
            RunStatus::non_progressing_sql_in_clause(),
            "('completed','failed','denied','interrupted','cancelled','limit_reached','waiting_approval','needs_reply')"
        );
    }
}
