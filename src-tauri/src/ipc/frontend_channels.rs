//! Typed events pushed from Rust Core to Frontend via Tauri channels.

use serde::Serialize;

use crate::core::index_manager::{SearchBatchResponse, SearchResponse};
use crate::core::subagent::{SubagentActivityStatus, SubagentProgressSnapshot};
use crate::model::git::GitSnapshotDto;
use crate::model::task_board::TaskBoardDto;
use crate::model::terminal::{TerminalSessionDto, TerminalSessionStatus};
use crate::model::thread::RunUsageDto;

/// Events sent to the frontend for a specific thread.
/// Consumed by the ThreadStream adapter which maps them to AI Elements.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ThreadStreamEvent {
    RunStarted {
        run_id: String,
        run_mode: String,
    },
    StreamResyncRequired {
        run_id: String,
        dropped_events: u64,
    },
    RunRetrying {
        run_id: String,
        attempt: usize,
        max_attempts: usize,
        delay_ms: u64,
        reason: String,
    },
    MessageDelta {
        run_id: String,
        message_id: String,
        delta: String,
    },
    MessageCompleted {
        run_id: String,
        message_id: String,
        content: String,
    },
    MessageDiscarded {
        run_id: String,
        message_id: String,
        reason: String,
    },
    PlanUpdated {
        run_id: String,
        plan: serde_json::Value,
    },
    ReasoningUpdated {
        run_id: String,
        message_id: String,
        reasoning: String,
    },
    QueueUpdated {
        run_id: String,
        queue: serde_json::Value,
    },
    SubagentStarted {
        run_id: String,
        subtask_id: String,
        helper_kind: String,
        started_at: String,
        snapshot: SubagentProgressSnapshot,
    },
    SubagentProgress {
        run_id: String,
        subtask_id: String,
        helper_kind: String,
        started_at: String,
        activity: SubagentActivityStatus,
        message: String,
        snapshot: SubagentProgressSnapshot,
    },
    SubagentCompleted {
        run_id: String,
        subtask_id: String,
        helper_kind: String,
        started_at: String,
        summary: Option<String>,
        snapshot: SubagentProgressSnapshot,
    },
    SubagentFailed {
        run_id: String,
        subtask_id: String,
        helper_kind: String,
        started_at: String,
        error: String,
        snapshot: SubagentProgressSnapshot,
    },
    ToolRequested {
        run_id: String,
        tool_call_id: String,
        tool_name: String,
        tool_input: serde_json::Value,
    },
    ApprovalRequired {
        run_id: String,
        tool_call_id: String,
        tool_name: String,
        tool_input: serde_json::Value,
        reason: String,
    },
    ClarifyRequired {
        run_id: String,
        tool_call_id: String,
        tool_name: String,
        tool_input: serde_json::Value,
    },
    ApprovalResolved {
        run_id: String,
        tool_call_id: String,
        approved: bool,
    },
    ClarifyResolved {
        run_id: String,
        tool_call_id: String,
        response: serde_json::Value,
    },
    ToolRunning {
        run_id: String,
        tool_call_id: String,
    },
    ToolCompleted {
        run_id: String,
        tool_call_id: String,
        result: serde_json::Value,
    },
    ToolFailed {
        run_id: String,
        tool_call_id: String,
        error: String,
    },
    ThreadTitleUpdated {
        run_id: String,
        thread_id: String,
        title: String,
    },
    ThreadUsageUpdated {
        run_id: String,
        model_display_name: Option<String>,
        context_window: Option<String>,
        usage: RunUsageDto,
    },
    RunCheckpointed {
        run_id: String,
    },
    /// Emitted when context compression is in progress (LLM generating summary).
    /// The frontend should show a "Compressing context…" placeholder.
    ContextCompressing {
        run_id: String,
    },
    RunCompleted {
        run_id: String,
    },
    RunLimitReached {
        run_id: String,
        error: String,
        max_turns: usize,
    },
    RunFailed {
        run_id: String,
        error: String,
    },
    RunCancelled {
        run_id: String,
    },
    RunInterrupted {
        run_id: String,
    },
    TaskBoardUpdated {
        run_id: String,
        task_board: TaskBoardDto,
    },
}

/// Events sent to the frontend terminal layer for a specific thread terminal.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type")]
pub enum TerminalStreamEvent {
    #[serde(rename = "session_created")]
    SessionCreated {
        #[serde(rename = "threadId")]
        thread_id: String,
        session: TerminalSessionDto,
    },
    #[serde(rename = "stdout_chunk")]
    StdoutChunk {
        #[serde(rename = "threadId")]
        thread_id: String,
        data: String,
    },
    #[serde(rename = "stderr_chunk")]
    StderrChunk {
        #[serde(rename = "threadId")]
        thread_id: String,
        data: String,
    },
    #[serde(rename = "status_changed")]
    StatusChanged {
        #[serde(rename = "threadId")]
        thread_id: String,
        status: TerminalSessionStatus,
    },
    #[serde(rename = "session_exited")]
    SessionExited {
        #[serde(rename = "threadId")]
        thread_id: String,
        #[serde(rename = "exitCode")]
        exit_code: Option<i32>,
    },
}

/// Events sent to the frontend Git drawer for refresh lifecycle updates.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum GitStreamEvent {
    RefreshStarted {
        #[serde(rename = "workspaceId")]
        workspace_id: String,
    },
    SnapshotUpdated {
        #[serde(rename = "workspaceId")]
        workspace_id: String,
        snapshot: GitSnapshotDto,
    },
    RefreshCompleted {
        #[serde(rename = "workspaceId")]
        workspace_id: String,
    },
}

/// Events sent to the frontend index search surface for progressive results.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SearchStreamEvent {
    Started {
        #[serde(rename = "workspaceId")]
        workspace_id: String,
        query: String,
    },
    Batch {
        #[serde(rename = "workspaceId")]
        workspace_id: String,
        batch: SearchBatchResponse,
    },
    Completed {
        #[serde(rename = "workspaceId")]
        workspace_id: String,
        response: SearchResponse,
    },
    Failed {
        #[serde(rename = "workspaceId")]
        workspace_id: String,
        query: String,
        error: String,
    },
}
