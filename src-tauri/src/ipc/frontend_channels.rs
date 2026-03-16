//! Typed events pushed from Rust Core to Frontend via Tauri channels.

use serde::Serialize;

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
        reasoning: String,
    },
    QueueUpdated {
        run_id: String,
        queue: serde_json::Value,
    },
    SubagentStarted {
        run_id: String,
        subtask_id: String,
    },
    SubagentCompleted {
        run_id: String,
        subtask_id: String,
        summary: Option<String>,
    },
    SubagentFailed {
        run_id: String,
        subtask_id: String,
        error: String,
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
    RunCompleted {
        run_id: String,
    },
    RunFailed {
        run_id: String,
        error: String,
    },
    RunInterrupted {
        run_id: String,
    },
}
