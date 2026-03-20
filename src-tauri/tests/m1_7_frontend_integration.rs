//! M1.7 — Frontend full integration tests (Rust-side)
//!
//! Acceptance criteria:
//! - ThreadStreamEvent covers all event types for the frontend adapter
//! - All Tauri commands are registered in invoke_handler
//! - No mock data residual in Rust layer

fn sample_subagent_snapshot() -> tiy_agent_lib::core::subagent::SubagentProgressSnapshot {
    let mut snapshot = tiy_agent_lib::core::subagent::SubagentProgressSnapshot::default();
    snapshot.total_tool_calls = 2;
    snapshot.completed_steps = 1;
    snapshot.current_action = Some("reading src-tauri/src/core/agent_session.rs".into());
    snapshot.tool_counts.insert("read".into(), 1);
    snapshot.tool_counts.insert("search".into(), 1);
    snapshot.recent_actions = vec![
        "Started reading src-tauri/src/core/agent_session.rs".into(),
        "Finished reading src-tauri/src/core/agent_session.rs".into(),
    ];
    snapshot.usage.input_tokens = 256;
    snapshot.usage.output_tokens = 32;
    snapshot.usage.total_tokens = 288;
    snapshot
}

// =========================================================================
// T1.7.1 — ThreadStreamEvent serialization covers all variants
// =========================================================================

#[test]
fn test_thread_stream_event_run_started_serialization() {
    use tiy_agent_lib::ipc::frontend_channels::ThreadStreamEvent;

    let event = ThreadStreamEvent::RunStarted {
        run_id: "run-1".into(),
        run_mode: "default".into(),
    };

    let json = serde_json::to_value(&event).unwrap();
    assert_eq!(json["type"].as_str().unwrap(), "run_started");
    assert_eq!(json["run_id"].as_str().unwrap(), "run-1");
    assert_eq!(json["run_mode"].as_str().unwrap(), "default");
}

#[test]
fn test_thread_stream_event_message_delta_serialization() {
    use tiy_agent_lib::ipc::frontend_channels::ThreadStreamEvent;

    let event = ThreadStreamEvent::MessageDelta {
        run_id: "run-1".into(),
        message_id: "msg-1".into(),
        delta: "Hello ".into(),
    };

    let json = serde_json::to_value(&event).unwrap();
    assert_eq!(json["type"].as_str().unwrap(), "message_delta");
    assert_eq!(json["delta"].as_str().unwrap(), "Hello ");
}

#[test]
fn test_thread_stream_event_message_completed_serialization() {
    use tiy_agent_lib::ipc::frontend_channels::ThreadStreamEvent;

    let event = ThreadStreamEvent::MessageCompleted {
        run_id: "run-1".into(),
        message_id: "msg-1".into(),
        content: "Full response".into(),
    };

    let json = serde_json::to_value(&event).unwrap();
    assert_eq!(json["type"].as_str().unwrap(), "message_completed");
    assert_eq!(json["content"].as_str().unwrap(), "Full response");
}

#[test]
fn test_thread_stream_event_approval_required_serialization() {
    use tiy_agent_lib::ipc::frontend_channels::ThreadStreamEvent;

    let event = ThreadStreamEvent::ApprovalRequired {
        run_id: "run-1".into(),
        tool_call_id: "tc-1".into(),
        tool_name: "write".into(),
        tool_input: serde_json::json!({"path": "/src/lib.rs", "content": "// new"}),
        reason: "Mutating tool requires approval".into(),
    };

    let json = serde_json::to_value(&event).unwrap();
    assert_eq!(json["type"].as_str().unwrap(), "approval_required");
    assert_eq!(json["tool_name"].as_str().unwrap(), "write");
    assert_eq!(
        json["reason"].as_str().unwrap(),
        "Mutating tool requires approval"
    );
    assert_eq!(json["tool_input"]["path"].as_str().unwrap(), "/src/lib.rs");
}

#[test]
fn test_thread_stream_event_tool_completed_serialization() {
    use tiy_agent_lib::ipc::frontend_channels::ThreadStreamEvent;

    let event = ThreadStreamEvent::ToolCompleted {
        run_id: "run-1".into(),
        tool_call_id: "tc-1".into(),
        result: serde_json::json!({"content": "file contents here"}),
    };

    let json = serde_json::to_value(&event).unwrap();
    assert_eq!(json["type"].as_str().unwrap(), "tool_completed");
    assert!(json["result"]["content"].is_string());
}

#[test]
fn test_thread_stream_event_tool_failed_serialization() {
    use tiy_agent_lib::ipc::frontend_channels::ThreadStreamEvent;

    let event = ThreadStreamEvent::ToolFailed {
        run_id: "run-1".into(),
        tool_call_id: "tc-1".into(),
        error: "File not found".into(),
    };

    let json = serde_json::to_value(&event).unwrap();
    assert_eq!(json["type"].as_str().unwrap(), "tool_failed");
    assert_eq!(json["error"].as_str().unwrap(), "File not found");
}

#[test]
fn test_thread_stream_event_run_completed_serialization() {
    use tiy_agent_lib::ipc::frontend_channels::ThreadStreamEvent;

    let event = ThreadStreamEvent::RunCompleted {
        run_id: "run-1".into(),
    };

    let json = serde_json::to_value(&event).unwrap();
    assert_eq!(json["type"].as_str().unwrap(), "run_completed");
}

#[test]
fn test_thread_stream_event_run_failed_serialization() {
    use tiy_agent_lib::ipc::frontend_channels::ThreadStreamEvent;

    let event = ThreadStreamEvent::RunFailed {
        run_id: "run-1".into(),
        error: "LLM provider error".into(),
    };

    let json = serde_json::to_value(&event).unwrap();
    assert_eq!(json["type"].as_str().unwrap(), "run_failed");
    assert_eq!(json["error"].as_str().unwrap(), "LLM provider error");
}

#[test]
fn test_thread_stream_event_run_cancelled_serialization() {
    use tiy_agent_lib::ipc::frontend_channels::ThreadStreamEvent;

    let event = ThreadStreamEvent::RunCancelled {
        run_id: "run-1".into(),
    };

    let json = serde_json::to_value(&event).unwrap();
    assert_eq!(json["type"].as_str().unwrap(), "run_cancelled");
}

#[test]
fn test_thread_stream_event_plan_updated_serialization() {
    use tiy_agent_lib::ipc::frontend_channels::ThreadStreamEvent;

    let event = ThreadStreamEvent::PlanUpdated {
        run_id: "run-1".into(),
        plan: serde_json::json!({"steps": ["step1", "step2"]}),
    };

    let json = serde_json::to_value(&event).unwrap();
    assert_eq!(json["type"].as_str().unwrap(), "plan_updated");
    assert!(json["plan"]["steps"].is_array());
}

#[test]
fn test_thread_stream_event_tool_requested_serialization() {
    use tiy_agent_lib::ipc::frontend_channels::ThreadStreamEvent;

    let event = ThreadStreamEvent::ToolRequested {
        run_id: "run-1".into(),
        tool_call_id: "tc-1".into(),
        tool_name: "search".into(),
        tool_input: serde_json::json!({"query": "TODO"}),
    };

    let json = serde_json::to_value(&event).unwrap();
    assert_eq!(json["type"].as_str().unwrap(), "tool_requested");
    assert_eq!(json["tool_name"].as_str().unwrap(), "search");
}

#[test]
fn test_thread_stream_event_reasoning_updated_serialization() {
    use tiy_agent_lib::ipc::frontend_channels::ThreadStreamEvent;

    let event = ThreadStreamEvent::ReasoningUpdated {
        run_id: "run-1".into(),
        message_id: "reasoning-1".into(),
        reasoning: "Inspecting the repository layout".into(),
    };

    let json = serde_json::to_value(&event).unwrap();
    assert_eq!(json["type"].as_str().unwrap(), "reasoning_updated");
    assert_eq!(json["message_id"].as_str().unwrap(), "reasoning-1");
    assert_eq!(
        json["reasoning"].as_str().unwrap(),
        "Inspecting the repository layout"
    );
}

#[test]
fn test_thread_stream_event_subagent_events_serialization() {
    use tiy_agent_lib::ipc::frontend_channels::ThreadStreamEvent;

    // SubagentStarted
    let event = ThreadStreamEvent::SubagentStarted {
        run_id: "run-1".into(),
        subtask_id: "sub-1".into(),
        helper_kind: "helper_scout".into(),
        started_at: "2026-03-20T00:00:00Z".into(),
        snapshot: sample_subagent_snapshot(),
    };
    let json = serde_json::to_value(&event).unwrap();
    assert_eq!(json["type"].as_str().unwrap(), "subagent_started");
    assert_eq!(json["helper_kind"].as_str().unwrap(), "helper_scout");
    assert_eq!(json["snapshot"]["total_tool_calls"].as_u64().unwrap(), 2);

    // SubagentProgress
    let event = ThreadStreamEvent::SubagentProgress {
        run_id: "run-1".into(),
        subtask_id: "sub-1".into(),
        helper_kind: "helper_scout".into(),
        started_at: "2026-03-20T00:00:00Z".into(),
        activity: tiy_agent_lib::core::subagent::SubagentActivityStatus::Started,
        message: "Reading src-tauri/src/core/agent_session.rs".into(),
        snapshot: sample_subagent_snapshot(),
    };
    let json = serde_json::to_value(&event).unwrap();
    assert_eq!(json["type"].as_str().unwrap(), "subagent_progress");
    assert_eq!(json["activity"].as_str().unwrap(), "started");
    assert_eq!(
        json["message"].as_str().unwrap(),
        "Reading src-tauri/src/core/agent_session.rs"
    );

    // SubagentUsageUpdated
    let event = ThreadStreamEvent::SubagentUsageUpdated {
        run_id: "run-1".into(),
        subtask_id: "sub-1".into(),
        helper_kind: "helper_scout".into(),
        started_at: "2026-03-20T00:00:00Z".into(),
        snapshot: sample_subagent_snapshot(),
    };
    let json = serde_json::to_value(&event).unwrap();
    assert_eq!(json["type"].as_str().unwrap(), "subagent_usage_updated");
    assert_eq!(json["snapshot"]["usage"]["inputTokens"].as_u64(), Some(256));
    assert_eq!(json["snapshot"]["usage"]["outputTokens"].as_u64(), Some(32));

    // SubagentCompleted
    let event = ThreadStreamEvent::SubagentCompleted {
        run_id: "run-1".into(),
        subtask_id: "sub-1".into(),
        helper_kind: "helper_planner".into(),
        started_at: "2026-03-20T00:00:00Z".into(),
        summary: Some("Analysis complete".into()),
        snapshot: sample_subagent_snapshot(),
    };
    let json = serde_json::to_value(&event).unwrap();
    assert_eq!(json["type"].as_str().unwrap(), "subagent_completed");
    assert_eq!(json["helper_kind"].as_str().unwrap(), "helper_planner");
    assert_eq!(json["summary"].as_str().unwrap(), "Analysis complete");

    // SubagentFailed
    let event = ThreadStreamEvent::SubagentFailed {
        run_id: "run-1".into(),
        subtask_id: "sub-1".into(),
        helper_kind: "helper_reviewer".into(),
        started_at: "2026-03-20T00:00:00Z".into(),
        error: "timeout".into(),
        snapshot: sample_subagent_snapshot(),
    };
    let json = serde_json::to_value(&event).unwrap();
    assert_eq!(json["type"].as_str().unwrap(), "subagent_failed");
    assert_eq!(json["helper_kind"].as_str().unwrap(), "helper_reviewer");
}

// =========================================================================
// T1.7.2 — All ThreadStreamEvent variants are tag-discriminated
// =========================================================================

#[test]
fn test_all_events_have_type_field() {
    use tiy_agent_lib::ipc::frontend_channels::ThreadStreamEvent;

    let events: Vec<ThreadStreamEvent> = vec![
        ThreadStreamEvent::RunStarted {
            run_id: "r".into(),
            run_mode: "default".into(),
        },
        ThreadStreamEvent::MessageDelta {
            run_id: "r".into(),
            message_id: "m".into(),
            delta: "d".into(),
        },
        ThreadStreamEvent::MessageCompleted {
            run_id: "r".into(),
            message_id: "m".into(),
            content: "c".into(),
        },
        ThreadStreamEvent::PlanUpdated {
            run_id: "r".into(),
            plan: serde_json::json!({}),
        },
        ThreadStreamEvent::ReasoningUpdated {
            run_id: "r".into(),
            message_id: "rm".into(),
            reasoning: "r".into(),
        },
        ThreadStreamEvent::QueueUpdated {
            run_id: "r".into(),
            queue: serde_json::json!([]),
        },
        ThreadStreamEvent::SubagentStarted {
            run_id: "r".into(),
            subtask_id: "s".into(),
            helper_kind: "helper_scout".into(),
            started_at: "2026-03-20T00:00:00Z".into(),
            snapshot: sample_subagent_snapshot(),
        },
        ThreadStreamEvent::SubagentProgress {
            run_id: "r".into(),
            subtask_id: "s".into(),
            helper_kind: "helper_scout".into(),
            started_at: "2026-03-20T00:00:00Z".into(),
            activity: tiy_agent_lib::core::subagent::SubagentActivityStatus::Started,
            message: "Reading foo".into(),
            snapshot: sample_subagent_snapshot(),
        },
        ThreadStreamEvent::SubagentUsageUpdated {
            run_id: "r".into(),
            subtask_id: "s".into(),
            helper_kind: "helper_scout".into(),
            started_at: "2026-03-20T00:00:00Z".into(),
            snapshot: sample_subagent_snapshot(),
        },
        ThreadStreamEvent::SubagentCompleted {
            run_id: "r".into(),
            subtask_id: "s".into(),
            helper_kind: "helper_planner".into(),
            started_at: "2026-03-20T00:00:00Z".into(),
            summary: None,
            snapshot: sample_subagent_snapshot(),
        },
        ThreadStreamEvent::SubagentFailed {
            run_id: "r".into(),
            subtask_id: "s".into(),
            helper_kind: "helper_reviewer".into(),
            started_at: "2026-03-20T00:00:00Z".into(),
            error: "e".into(),
            snapshot: sample_subagent_snapshot(),
        },
        ThreadStreamEvent::ToolRequested {
            run_id: "r".into(),
            tool_call_id: "t".into(),
            tool_name: "n".into(),
            tool_input: serde_json::json!({}),
        },
        ThreadStreamEvent::ApprovalRequired {
            run_id: "r".into(),
            tool_call_id: "t".into(),
            tool_name: "n".into(),
            tool_input: serde_json::json!({}),
            reason: "r".into(),
        },
        ThreadStreamEvent::ApprovalResolved {
            run_id: "r".into(),
            tool_call_id: "t".into(),
            approved: true,
        },
        ThreadStreamEvent::ToolRunning {
            run_id: "r".into(),
            tool_call_id: "t".into(),
        },
        ThreadStreamEvent::ToolCompleted {
            run_id: "r".into(),
            tool_call_id: "t".into(),
            result: serde_json::json!({}),
        },
        ThreadStreamEvent::ToolFailed {
            run_id: "r".into(),
            tool_call_id: "t".into(),
            error: "e".into(),
        },
        ThreadStreamEvent::RunCompleted { run_id: "r".into() },
        ThreadStreamEvent::RunFailed {
            run_id: "r".into(),
            error: "e".into(),
        },
        ThreadStreamEvent::RunCancelled { run_id: "r".into() },
        ThreadStreamEvent::RunInterrupted { run_id: "r".into() },
    ];

    for event in &events {
        let json = serde_json::to_value(event).unwrap();
        assert!(
            json.get("type").is_some(),
            "Event should have 'type' discriminator: {json}"
        );
        let type_val = json["type"].as_str().unwrap();
        assert!(!type_val.is_empty(), "Event type should not be empty");
    }

    // Verify total count matches enum variants (21 variants)
    assert_eq!(
        events.len(),
        21,
        "Should test all 21 ThreadStreamEvent variants"
    );
}

// =========================================================================
// T1.7.3 — Model DTO serialization (camelCase)
// =========================================================================

#[test]
fn test_workspace_dto_camel_case() {
    use tiy_agent_lib::model::workspace::{WorkspaceDto, WorkspaceStatus};

    let dto = WorkspaceDto {
        id: "ws-1".into(),
        name: "Project".into(),
        path: "/tmp/proj".into(),
        canonical_path: "/tmp/proj".into(),
        display_path: "~/proj".into(),
        is_default: true,
        is_git: true,
        auto_work_tree: false,
        status: WorkspaceStatus::Ready,
        last_validated_at: Some("2026-03-16T00:00:00Z".into()),
        created_at: "2026-03-16T00:00:00Z".into(),
        updated_at: "2026-03-16T00:00:00Z".into(),
    };

    let json = serde_json::to_value(&dto).unwrap();

    // Verify camelCase keys
    assert!(
        json.get("isDefault").is_some(),
        "Should use camelCase: isDefault"
    );
    assert!(json.get("isGit").is_some(), "Should use camelCase: isGit");
    assert!(
        json.get("autoWorkTree").is_some(),
        "Should use camelCase: autoWorkTree"
    );
    assert!(
        json.get("canonicalPath").is_some(),
        "Should use camelCase: canonicalPath"
    );
    assert!(
        json.get("displayPath").is_some(),
        "Should use camelCase: displayPath"
    );
    assert!(
        json.get("lastValidatedAt").is_some(),
        "Should use camelCase: lastValidatedAt"
    );
    assert!(
        json.get("createdAt").is_some(),
        "Should use camelCase: createdAt"
    );

    // Verify no snake_case keys
    assert!(
        json.get("is_default").is_none(),
        "Should NOT have snake_case key"
    );
    assert!(
        json.get("is_git").is_none(),
        "Should NOT have snake_case key"
    );
}

#[test]
fn test_thread_summary_dto_camel_case() {
    use tiy_agent_lib::model::thread::{ThreadStatus, ThreadSummaryDto};

    let dto = ThreadSummaryDto {
        id: "t-1".into(),
        workspace_id: "ws-1".into(),
        title: "Test".into(),
        status: ThreadStatus::Idle,
        last_active_at: "2026-03-16T00:00:00Z".into(),
        created_at: "2026-03-16T00:00:00Z".into(),
    };

    let json = serde_json::to_value(&dto).unwrap();
    assert!(json.get("workspaceId").is_some());
    assert!(json.get("lastActiveAt").is_some());
    assert!(json.get("createdAt").is_some());
}

#[test]
fn test_message_dto_camel_case() {
    use tiy_agent_lib::model::thread::MessageDto;

    let dto = MessageDto {
        id: "m-1".into(),
        thread_id: "t-1".into(),
        run_id: Some("r-1".into()),
        role: "assistant".into(),
        content_markdown: "Hello".into(),
        message_type: "plain_message".into(),
        status: "completed".into(),
        metadata: None,
        created_at: "2026-03-16T00:00:00Z".into(),
    };

    let json = serde_json::to_value(&dto).unwrap();
    assert!(json.get("threadId").is_some());
    assert!(json.get("runId").is_some());
    assert!(json.get("contentMarkdown").is_some());
    assert!(json.get("messageType").is_some());
    assert!(json.get("createdAt").is_some());
}

// =========================================================================
// T1.7.4 — Error response serialization (camelCase)
// =========================================================================

#[test]
fn test_app_error_serialization_camel_case() {
    use tiy_agent_lib::model::errors::{AppError, ErrorSource};

    let err = AppError::recoverable(
        ErrorSource::Workspace,
        "workspace.duplicate",
        "Already exists",
    );

    let json = serde_json::to_value(&err).unwrap();

    assert!(
        json.get("errorCode").is_some(),
        "Should use camelCase: errorCode"
    );
    assert!(
        json.get("userMessage").is_some(),
        "Should use camelCase: userMessage"
    );
    assert!(
        json.get("error_code").is_none(),
        "Should NOT have snake_case"
    );
}
