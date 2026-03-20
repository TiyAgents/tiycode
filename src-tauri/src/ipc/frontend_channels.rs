//! Typed events pushed from Rust Core to Frontend via Tauri channels.

use serde::Serialize;

use crate::core::subagent::{SubagentActivityStatus, SubagentProgressSnapshot};
use crate::model::git::GitSnapshotDto;
use crate::model::terminal::{TerminalSessionDto, TerminalSessionStatus};

/// Events sent to the frontend for a specific thread.
/// Consumed by the ThreadStream adapter which maps them to AI Elements.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ThreadStreamEvent {
    RunStarted {
        run_id: String,
        run_mode: String,
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
    ApprovalResolved {
        run_id: String,
        tool_call_id: String,
        approved: bool,
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
    RunCompleted {
        run_id: String,
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
