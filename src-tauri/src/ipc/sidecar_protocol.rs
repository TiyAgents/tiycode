//! JSON-RPC style protocol for Rust <-> TS Agent Sidecar communication.
//!
//! Transport: stdio (stdin/stdout) with NDJSON framing.
//! Each line is one complete JSON message.

use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Envelope types (wire format)
// ---------------------------------------------------------------------------

/// Outgoing message from Rust to Sidecar.
#[derive(Debug, Serialize)]
#[serde(tag = "type")]
pub enum RustToSidecar {
    #[serde(rename = "request")]
    Request {
        id: String,
        method: String,
        payload: serde_json::Value,
    },
}

/// Incoming message from Sidecar to Rust.
#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
pub enum SidecarToRust {
    #[serde(rename = "response")]
    Response {
        id: String,
        ok: bool,
        payload: serde_json::Value,
    },
    #[serde(rename = "event")]
    Event {
        event: String,
        payload: serde_json::Value,
    },
}

// ---------------------------------------------------------------------------
// Rust -> Sidecar request payloads
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RunStartPayload {
    pub run_id: String,
    pub thread_id: String,
    pub run_mode: String,
    pub prompt: String,
    pub model_plan: serde_json::Value,
    pub thread_snapshot: serde_json::Value,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RunCancelPayload {
    pub run_id: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolResultPayload {
    pub tool_call_id: String,
    pub run_id: String,
    pub result: serde_json::Value,
    pub success: bool,
}

// ---------------------------------------------------------------------------
// Sidecar -> Rust event payloads
// ---------------------------------------------------------------------------

/// Parsed sidecar event with typed payload.
#[derive(Debug, Clone)]
pub enum SidecarEvent {
    RunStarted {
        run_id: String,
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
    RunCompleted {
        run_id: String,
    },
    RunFailed {
        run_id: String,
        error: String,
    },
}

impl SidecarEvent {
    /// Parse a raw sidecar event name + payload into a typed event.
    pub fn parse(event: &str, payload: serde_json::Value) -> Option<Self> {
        match event {
            "agent.run.started" => Some(Self::RunStarted {
                run_id: payload["runId"].as_str()?.to_string(),
            }),
            "agent.message.delta" => Some(Self::MessageDelta {
                run_id: payload["runId"].as_str()?.to_string(),
                message_id: payload["messageId"].as_str().unwrap_or("").to_string(),
                delta: payload["delta"].as_str()?.to_string(),
            }),
            "agent.message.completed" => Some(Self::MessageCompleted {
                run_id: payload["runId"].as_str()?.to_string(),
                message_id: payload["messageId"].as_str().unwrap_or("").to_string(),
                content: payload["content"].as_str().unwrap_or("").to_string(),
            }),
            "agent.plan.updated" => Some(Self::PlanUpdated {
                run_id: payload["runId"].as_str()?.to_string(),
                plan: payload["plan"].clone(),
            }),
            "agent.reasoning.updated" => Some(Self::ReasoningUpdated {
                run_id: payload["runId"].as_str()?.to_string(),
                reasoning: payload["reasoning"].as_str()?.to_string(),
            }),
            "agent.queue.updated" => Some(Self::QueueUpdated {
                run_id: payload["runId"].as_str()?.to_string(),
                queue: payload["queue"].clone(),
            }),
            "agent.subagent.started" => Some(Self::SubagentStarted {
                run_id: payload["runId"].as_str()?.to_string(),
                subtask_id: payload["subtaskId"].as_str()?.to_string(),
            }),
            "agent.subagent.completed" => Some(Self::SubagentCompleted {
                run_id: payload["runId"].as_str()?.to_string(),
                subtask_id: payload["subtaskId"].as_str()?.to_string(),
                summary: payload["summary"].as_str().map(|s| s.to_string()),
            }),
            "agent.subagent.failed" => Some(Self::SubagentFailed {
                run_id: payload["runId"].as_str()?.to_string(),
                subtask_id: payload["subtaskId"].as_str()?.to_string(),
                error: payload["error"].as_str().unwrap_or("unknown").to_string(),
            }),
            "agent.tool.requested" => Some(Self::ToolRequested {
                run_id: payload["runId"].as_str()?.to_string(),
                tool_call_id: payload["toolCallId"].as_str()?.to_string(),
                tool_name: payload["toolName"].as_str()?.to_string(),
                tool_input: payload["toolInput"].clone(),
            }),
            "agent.run.completed" => Some(Self::RunCompleted {
                run_id: payload["runId"].as_str()?.to_string(),
            }),
            "agent.run.failed" => Some(Self::RunFailed {
                run_id: payload["runId"].as_str()?.to_string(),
                error: payload["error"].as_str().unwrap_or("unknown").to_string(),
            }),
            _ => {
                tracing::warn!(event, "unknown sidecar event");
                None
            }
        }
    }

    pub fn run_id(&self) -> &str {
        match self {
            Self::RunStarted { run_id }
            | Self::MessageDelta { run_id, .. }
            | Self::MessageCompleted { run_id, .. }
            | Self::PlanUpdated { run_id, .. }
            | Self::ReasoningUpdated { run_id, .. }
            | Self::QueueUpdated { run_id, .. }
            | Self::SubagentStarted { run_id, .. }
            | Self::SubagentCompleted { run_id, .. }
            | Self::SubagentFailed { run_id, .. }
            | Self::ToolRequested { run_id, .. }
            | Self::RunCompleted { run_id }
            | Self::RunFailed { run_id, .. } => run_id,
        }
    }
}
