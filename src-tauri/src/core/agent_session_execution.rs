use std::sync::atomic::Ordering;
use std::time::Instant;

use futures::StreamExt;

use crate::core::agent_session::{
    standard_tool_timeout, ResolvedModelRole, CLARIFY_TOOL_NAME, PLAN_TOOL_NAME, RENDER_TOOL_NAME,
    STANDARD_TOOL_TIMEOUT_SECS, TASK_TOOL_NAMES,
};
use crate::core::agent_session_tools::{
    agent_error_result, agent_tool_result_from_output, resolve_helper_model_role,
    resolve_helper_profile, validate_clarify_input,
};
use crate::core::plan_checkpoint::{
    approval_prompt_markdown, build_approval_prompt_metadata, build_plan_artifact_from_tool_input,
    build_plan_message_metadata, plan_markdown, write_plan_file,
};
use crate::core::subagent::{
    extract_judge_report, extract_review_report, render_parallel_summary, HelperRunRequest,
    HelperRunResult, JudgeReport, ParallelSubagentBatchStatus, ParallelSubagentRequest,
    ParallelSubagentSummary, ParallelSubagentTask, ParallelSubagentTaskResult,
    ParallelSubagentTaskStatus, ReviewRequest, RuntimeOrchestrationTool, SubagentProfile,
};
use crate::core::tool_gateway::{
    ApprovalRequest, ToolExecutionOptions, ToolExecutionRequest, ToolGatewayResult,
};
use crate::core::workspace_paths;
use crate::ipc::frontend_channels::{ArtifactStatus, ThreadStreamEvent};
use crate::model::thread::MessageRecord;
use crate::persistence::repo::{message_repo, tool_call_repo};
use tiycore::agent::AgentToolResult;
use tiycore::types::{ContentBlock, TextContent};

use super::agent_session::AgentSession;

#[derive(Debug)]
struct HelperToolTask {
    task: String,
    review_request: Option<ReviewRequest>,
}

#[derive(Debug, Clone)]
struct ResolvedHelperDelegate {
    tool: RuntimeOrchestrationTool,
    agent_name: String,
    task: String,
    review_request: Option<ReviewRequest>,
    helper_profile: Option<SubagentProfile>,
    model_role: ResolvedModelRole,
}

/// Delegation chain depth (1-based) at which the main agent's direct delegates
/// run. The main agent is depth 1, so its immediate sub-agents are depth 2.
/// Shared by the resolve-time depth check and the `HelperRunRequest` it builds.
const MAIN_AGENT_CHILD_DEPTH: u32 = 2;

fn resolve_helper_tool_task(
    tool: RuntimeOrchestrationTool,
    tool_input: &serde_json::Value,
) -> Result<HelperToolTask, String> {
    // Shares the task/review-request extraction with the recursive subagent
    // delegation path to avoid divergent prompt construction.
    let (task, review_request) =
        crate::core::subagent::orchestrator::resolve_delegation_task(&tool, tool_input)?;
    Ok(HelperToolTask {
        task,
        review_request,
    })
}

fn parallel_task_failed_result(
    index: usize,
    agent: String,
    task: String,
    error: String,
    duration_ms: u64,
) -> ParallelSubagentTaskResult {
    ParallelSubagentTaskResult {
        index,
        agent,
        task,
        status: ParallelSubagentTaskStatus::Failed,
        summary: None,
        raw_summary: None,
        snapshot: None,
        review_request: None,
        review_report: None,
        error: Some(error),
        duration_ms,
    }
}

fn parallel_task_skipped_result(
    index: usize,
    agent: String,
    task: String,
    reason: String,
) -> ParallelSubagentTaskResult {
    ParallelSubagentTaskResult {
        index,
        agent,
        task,
        status: ParallelSubagentTaskStatus::Skipped,
        summary: None,
        raw_summary: None,
        snapshot: None,
        review_request: None,
        review_report: None,
        error: Some(reason),
        duration_ms: 0,
    }
}

fn elapsed_ms_u64(started: Instant) -> u64 {
    u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX)
}

fn validate_parallel_delegate_safety(delegate: &ResolvedHelperDelegate) -> Result<(), String> {
    let Some(SubagentProfile::Custom { allowed_tools, .. }) = &delegate.helper_profile else {
        return Ok(());
    };

    const PARALLEL_SAFE_CUSTOM_TOOLS: &[&str] = &[
        "read",
        "list",
        "find",
        "search",
        "git_status",
        "git_diff",
        "git_log",
        "term_status",
        "term_output",
        "web_search",
    ];

    let unsafe_tools = allowed_tools
        .iter()
        .filter(|tool| !PARALLEL_SAFE_CUSTOM_TOOLS.contains(&tool.as_str()))
        .cloned()
        .collect::<Vec<_>>();

    if unsafe_tools.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "Custom subagent '{}' is not allowed in agent_parallel because it has non-read-only tools: {}",
            delegate.agent_name,
            unsafe_tools.join(", ")
        ))
    }
}

fn render_chart_part(artifact_id: &str, chart_payload: &serde_json::Value) -> serde_json::Value {
    serde_json::json!({
        "type": "chart",
        "artifactId": artifact_id,
        "library": chart_payload
            .get("library")
            .cloned()
            .unwrap_or_else(|| serde_json::json!("vega-lite")),
        "spec": chart_payload
            .get("spec")
            .cloned()
            .unwrap_or_else(|| serde_json::json!({})),
        "source": chart_payload
            .get("source")
            .cloned()
            .unwrap_or(serde_json::Value::Null),
        "title": chart_payload
            .get("title")
            .cloned()
            .unwrap_or(serde_json::Value::Null),
        "caption": chart_payload
            .get("caption")
            .cloned()
            .unwrap_or(serde_json::Value::Null),
        "status": chart_payload
            .get("status")
            .cloned()
            .unwrap_or_else(|| serde_json::json!("ready")),
        "error": chart_payload
            .get("error")
            .cloned()
            .unwrap_or(serde_json::Value::Null),
    })
}

fn render_host_message_metadata(
    tool_call_id: &str,
    artifact_id: &str,
    library: &str,
) -> serde_json::Value {
    serde_json::json!({
        "kind": "render_artifact_host",
        "toolCallId": tool_call_id,
        "artifactId": artifact_id,
        "library": library,
    })
}

fn render_artifact_host_message(
    thread_id: &str,
    run_id: &str,
    tool_call_id: &str,
    message_id: &str,
    artifact_id: &str,
    library: &str,
    chart_payload: &serde_json::Value,
) -> Result<MessageRecord, serde_json::Error> {
    let chart_part = render_chart_part(artifact_id, chart_payload);
    let parts_json = serde_json::to_string(&vec![chart_part])?;
    let metadata = render_host_message_metadata(tool_call_id, artifact_id, library);

    Ok(MessageRecord {
        id: message_id.to_string(),
        thread_id: thread_id.to_string(),
        run_id: Some(run_id.to_string()),
        role: "assistant".to_string(),
        content_markdown: String::new(),
        parts_json: Some(parts_json),
        message_type: "plain_message".to_string(),
        status: "completed".to_string(),
        metadata_json: Some(metadata.to_string()),
        attachments_json: None,
        created_at: String::new(),
    })
}

impl AgentSession {
    pub(crate) async fn execute_tool_call(
        &self,
        tool_name: &str,
        tool_call_id: &str,
        tool_input: &serde_json::Value,
    ) -> AgentToolResult {
        // Record tool call for goal evaluation (idle/completion detection).
        {
            let mut guard = self.goal_runtime.lock().unwrap_or_else(|poisoned| {
                tracing::warn!("execute_tool_call: goal_runtime mutex poisoned, recovering");
                poisoned.into_inner()
            });
            guard
                .thread_tool_calls
                .entry(self.spec.thread_id.clone())
                .or_default()
                .push(tool_name.to_string());
        } // Drop guard before crossing await boundary

        if tool_name == PLAN_TOOL_NAME {
            return match tokio::time::timeout(
                standard_tool_timeout(),
                self.execute_plan_checkpoint(tool_input),
            )
            .await
            {
                Ok(result) => result,
                Err(_) => agent_error_result(format!(
                    "Tool '{tool_name}' timed out after {STANDARD_TOOL_TIMEOUT_SECS}s"
                )),
            };
        }

        if TASK_TOOL_NAMES.contains(&tool_name) {
            return match tokio::time::timeout(
                standard_tool_timeout(),
                self.execute_task_tool(tool_name, tool_call_id, tool_input),
            )
            .await
            {
                Ok(result) => result,
                Err(_) => agent_error_result(format!(
                    "Tool '{tool_name}' timed out after {STANDARD_TOOL_TIMEOUT_SECS}s"
                )),
            };
        }

        if tool_name == RENDER_TOOL_NAME {
            return match tokio::time::timeout(
                standard_tool_timeout(),
                self.execute_render(tool_call_id, tool_input),
            )
            .await
            {
                Ok(result) => result,
                Err(_) => agent_error_result(format!(
                    "Tool '{tool_name}' timed out after {STANDARD_TOOL_TIMEOUT_SECS}s"
                )),
            };
        }

        if tool_name == CLARIFY_TOOL_NAME {
            return self
                .execute_clarify_request(tool_name, tool_call_id, tool_input)
                .await;
        }

        let tool_call_storage_id = uuid::Uuid::now_v7().to_string();
        let insert_result = tool_call_repo::insert(
            &self.pool,
            &tool_call_repo::ToolCallInsert {
                id: tool_call_storage_id.clone(),
                tool_call_id: tool_call_id.to_string(),
                run_id: self.spec.run_id.clone(),
                thread_id: self.spec.thread_id.clone(),
                helper_id: None,
                tool_name: tool_name.to_string(),
                tool_input_json: tool_input.to_string(),
                status: "requested".to_string(),
            },
        )
        .await;

        if let Err(error) = insert_result {
            return agent_error_result(format!("failed to persist tool call: {error}"));
        }

        if let Some(tool) = RuntimeOrchestrationTool::parse(tool_name) {
            return match tool {
                RuntimeOrchestrationTool::Parallel => {
                    self.execute_parallel_helper_tool(
                        tool_call_id,
                        &tool_call_storage_id,
                        tool_input,
                    )
                    .await
                }
                RuntimeOrchestrationTool::Judge => {
                    self.execute_judge_tool(tool_call_id, &tool_call_storage_id, tool_input)
                        .await
                }
                _ => {
                    self.execute_helper_tool(tool, tool_call_id, &tool_call_storage_id, tool_input)
                        .await
                }
            };
        }

        let _ = self.event_tx.send(ThreadStreamEvent::ToolRequested {
            run_id: self.spec.run_id.clone(),
            tool_call_id: tool_call_id.to_string(),
            tool_name: tool_name.to_string(),
            tool_input: tool_input.clone(),
        });

        let request = ToolExecutionRequest {
            run_id: self.spec.run_id.clone(),
            thread_id: self.spec.thread_id.clone(),
            tool_call_id: tool_call_id.to_string(),
            tool_call_storage_id: tool_call_storage_id.clone(),
            tool_name: tool_name.to_string(),
            tool_input: tool_input.clone(),
            workspace_path: self.spec.workspace_path.clone(),
            run_mode: self.spec.run_mode.clone(),
        };

        let event_tx = self.event_tx.clone();
        let run_id = self.spec.run_id.clone();
        let tool_call_id_owned = tool_call_id.to_string();
        let outcome = self
            .tool_gateway
            .execute_tool_call(
                request,
                self.abort_signal.clone(),
                ToolExecutionOptions {
                    allow_user_approval: true,
                    execution_timeout: Some(standard_tool_timeout()),
                },
                move |approval: ApprovalRequest| {
                    let _ = event_tx.send(ThreadStreamEvent::ApprovalRequired {
                        run_id: approval.run_id,
                        tool_call_id: approval.tool_call_id,
                        tool_name: approval.tool_name,
                        tool_input: approval.tool_input,
                        reason: approval.reason,
                    });
                },
                {
                    let event_tx = self.event_tx.clone();
                    let run_id = run_id.clone();
                    let tool_call_id = tool_call_id_owned.clone();
                    move || {
                        let _ = event_tx.send(ThreadStreamEvent::ToolRunning {
                            run_id: run_id.clone(),
                            tool_call_id: tool_call_id.clone(),
                        });
                    }
                },
            )
            .await;

        match outcome {
            Ok(outcome) => {
                if outcome.approval_required {
                    let approved = matches!(outcome.result, ToolGatewayResult::Executed { .. });
                    let _ = self.event_tx.send(ThreadStreamEvent::ApprovalResolved {
                        run_id: self.spec.run_id.clone(),
                        tool_call_id: tool_call_id.to_string(),
                        approved,
                    });
                }

                match outcome.result {
                    ToolGatewayResult::Executed { output, .. } => {
                        let _ = self.event_tx.send(ThreadStreamEvent::ToolCompleted {
                            run_id: self.spec.run_id.clone(),
                            tool_call_id: tool_call_id.to_string(),
                            tool_name: tool_name.to_string(),
                            result: output.result.clone(),
                        });
                        agent_tool_result_from_output(output)
                    }
                    ToolGatewayResult::Denied { reason, .. } => {
                        let _ = self.event_tx.send(ThreadStreamEvent::ToolFailed {
                            run_id: self.spec.run_id.clone(),
                            tool_call_id: tool_call_id.to_string(),
                            error: reason.clone(),
                        });
                        agent_error_result(reason)
                    }
                    ToolGatewayResult::EscalationRequired { reason, .. } => {
                        let _ = self.event_tx.send(ThreadStreamEvent::ToolFailed {
                            run_id: self.spec.run_id.clone(),
                            tool_call_id: tool_call_id.to_string(),
                            error: reason.clone(),
                        });
                        agent_error_result(reason)
                    }
                    ToolGatewayResult::Cancelled { .. } => {
                        let message = "Tool execution cancelled".to_string();
                        let _ = self.event_tx.send(ThreadStreamEvent::ToolFailed {
                            run_id: self.spec.run_id.clone(),
                            tool_call_id: tool_call_id.to_string(),
                            error: message.clone(),
                        });
                        agent_error_result(message)
                    }
                    ToolGatewayResult::TimedOut { timeout_secs, .. } => {
                        let message =
                            format!("Tool '{}' timed out after {}s", tool_name, timeout_secs);
                        let result = serde_json::json!({ "error": message.clone() });
                        tool_call_repo::update_result(
                            &self.pool,
                            &tool_call_storage_id,
                            &result.to_string(),
                            "failed",
                        )
                        .await
                        .ok();

                        let _ = self.event_tx.send(ThreadStreamEvent::ToolFailed {
                            run_id: self.spec.run_id.clone(),
                            tool_call_id: tool_call_id.to_string(),
                            error: message.clone(),
                        });
                        agent_error_result(message)
                    }
                }
            }
            Err(error) => {
                let _ = self.event_tx.send(ThreadStreamEvent::ToolFailed {
                    run_id: self.spec.run_id.clone(),
                    tool_call_id: tool_call_id.to_string(),
                    error: error.to_string(),
                });
                agent_error_result(error.to_string())
            }
        }
    }

    async fn execute_clarify_request(
        &self,
        tool_name: &str,
        tool_call_id: &str,
        tool_input: &serde_json::Value,
    ) -> AgentToolResult {
        if let Err(error) = validate_clarify_input(tool_input) {
            let _ = self.event_tx.send(ThreadStreamEvent::ToolFailed {
                run_id: self.spec.run_id.clone(),
                tool_call_id: tool_call_id.to_string(),
                error: error.clone(),
            });
            return agent_error_result(error);
        }

        let tool_call_storage_id = uuid::Uuid::now_v7().to_string();
        if let Err(error) = tool_call_repo::insert(
            &self.pool,
            &tool_call_repo::ToolCallInsert {
                id: tool_call_storage_id.clone(),
                tool_call_id: tool_call_id.to_string(),
                run_id: self.spec.run_id.clone(),
                thread_id: self.spec.thread_id.clone(),
                helper_id: None,
                tool_name: tool_name.to_string(),
                tool_input_json: tool_input.to_string(),
                status: "requested".to_string(),
            },
        )
        .await
        {
            return agent_error_result(format!("failed to persist tool call: {error}"));
        }

        let _ = self.event_tx.send(ThreadStreamEvent::ToolRequested {
            run_id: self.spec.run_id.clone(),
            tool_call_id: tool_call_id.to_string(),
            tool_name: tool_name.to_string(),
            tool_input: tool_input.clone(),
        });

        let request = ToolExecutionRequest {
            run_id: self.spec.run_id.clone(),
            thread_id: self.spec.thread_id.clone(),
            tool_call_id: tool_call_id.to_string(),
            tool_call_storage_id: tool_call_storage_id.clone(),
            tool_name: tool_name.to_string(),
            tool_input: tool_input.clone(),
            workspace_path: self.spec.workspace_path.clone(),
            run_mode: self.spec.run_mode.clone(),
        };

        match self
            .tool_gateway
            .request_clarification(request, self.abort_signal.clone(), {
                let event_tx = self.event_tx.clone();
                let run_id = self.spec.run_id.clone();
                let tool_call_id = tool_call_id.to_string();
                let tool_name = tool_name.to_string();
                let tool_input = tool_input.clone();
                move || {
                    let _ = event_tx.send(ThreadStreamEvent::ClarifyRequired {
                        run_id: run_id.clone(),
                        tool_call_id: tool_call_id.clone(),
                        tool_name: tool_name.clone(),
                        tool_input: tool_input.clone(),
                    });
                }
            })
            .await
        {
            Ok(output) => {
                let _ = self.event_tx.send(ThreadStreamEvent::ClarifyResolved {
                    run_id: self.spec.run_id.clone(),
                    tool_call_id: tool_call_id.to_string(),
                    response: output.result.clone(),
                });
                let _ = self.event_tx.send(ThreadStreamEvent::ToolCompleted {
                    run_id: self.spec.run_id.clone(),
                    tool_call_id: tool_call_id.to_string(),
                    tool_name: tool_name.to_string(),
                    result: output.result.clone(),
                });
                agent_tool_result_from_output(output)
            }
            Err(error) => {
                let _ = self.event_tx.send(ThreadStreamEvent::ToolFailed {
                    run_id: self.spec.run_id.clone(),
                    tool_call_id: tool_call_id.to_string(),
                    error: error.to_string(),
                });
                agent_error_result(error.to_string())
            }
        }
    }

    async fn execute_helper_tool(
        &self,
        tool: RuntimeOrchestrationTool,
        tool_call_id: &str,
        tool_call_storage_id: &str,
        tool_input: &serde_json::Value,
    ) -> AgentToolResult {
        let delegate = match self.resolve_helper_delegate(tool, tool_input).await {
            Ok(delegate) => delegate,
            Err(error) => {
                tool_call_repo::update_result(
                    &self.pool,
                    tool_call_storage_id,
                    &serde_json::json!({ "error": error }).to_string(),
                    "failed",
                )
                .await
                .ok();
                return agent_error_result(error);
            }
        };

        let result = self.run_helper_for_delegate(&delegate, tool_call_id).await;

        match result {
            Ok(summary) => {
                let review_report = if delegate.tool == RuntimeOrchestrationTool::Review {
                    summary
                        .raw_summary
                        .as_deref()
                        .and_then(extract_review_report)
                } else {
                    None
                };

                let result = serde_json::json!({
                    "summary": summary.summary.clone(),
                    "rawSummary": summary.raw_summary.clone(),
                    "snapshot": summary.snapshot,
                    "reviewRequest": delegate.review_request,
                    "reviewReport": review_report,
                });
                tool_call_repo::update_result(
                    &self.pool,
                    tool_call_storage_id,
                    &result.to_string(),
                    "completed",
                )
                .await
                .ok();

                AgentToolResult {
                    content: vec![ContentBlock::Text(TextContent::new(summary.summary))],
                    details: Some(result),
                }
            }
            Err(error) => {
                tool_call_repo::update_result(
                    &self.pool,
                    tool_call_storage_id,
                    &serde_json::json!({ "error": error.to_string() }).to_string(),
                    "failed",
                )
                .await
                .ok();

                agent_error_result(error.to_string())
            }
        }
    }

    async fn execute_parallel_helper_tool(
        &self,
        tool_call_id: &str,
        tool_call_storage_id: &str,
        tool_input: &serde_json::Value,
    ) -> AgentToolResult {
        let batch_started = Instant::now();
        let request = match ParallelSubagentRequest::from_tool_input(tool_input) {
            Ok(request) => request,
            Err(error) => {
                tool_call_repo::update_result(
                    &self.pool,
                    tool_call_storage_id,
                    &serde_json::json!({ "error": &error }).to_string(),
                    "failed",
                )
                .await
                .ok();
                return agent_error_result(error);
            }
        };

        let max_concurrency = request.effective_max_concurrency();
        let mut immediate_results = Vec::new();
        let mut delegates = Vec::new();

        for (index, task) in request.tasks.iter().cloned().enumerate() {
            let task_started = Instant::now();
            let agent_name = task.agent.trim().to_string();
            let Some(tool) = RuntimeOrchestrationTool::parse(&agent_name) else {
                immediate_results.push(parallel_task_failed_result(
                    index,
                    agent_name,
                    task.task,
                    "Unknown subagent tool".to_string(),
                    elapsed_ms_u64(task_started),
                ));
                continue;
            };

            if tool == RuntimeOrchestrationTool::Parallel {
                immediate_results.push(parallel_task_failed_result(
                    index,
                    agent_name,
                    task.task,
                    "agent_parallel cannot delegate to itself".to_string(),
                    elapsed_ms_u64(task_started),
                ));
                continue;
            }

            match self
                .resolve_helper_delegate(tool, &task.to_tool_input())
                .await
            {
                Ok(delegate) => match validate_parallel_delegate_safety(&delegate) {
                    Ok(()) => delegates.push((index, task, delegate)),
                    Err(error) => immediate_results.push(parallel_task_failed_result(
                        index,
                        agent_name,
                        task.task,
                        error,
                        elapsed_ms_u64(task_started),
                    )),
                },
                Err(error) => immediate_results.push(parallel_task_failed_result(
                    index,
                    agent_name,
                    task.task,
                    error,
                    elapsed_ms_u64(task_started),
                )),
            }
        }

        let mut results = immediate_results;
        let mut queued: std::collections::VecDeque<_> = delegates.into_iter().collect();
        let mut active = futures::stream::FuturesUnordered::new();
        let mut stop_reason = if self.abort_signal.is_cancelled() {
            Some(
                "Skipped because the parent run was cancelled before this subagent could start"
                    .to_string(),
            )
        } else if request.fail_fast
            && results
                .iter()
                .any(|result| result.status == ParallelSubagentTaskStatus::Failed)
        {
            Some("Skipped because failFast stopped scheduling after an earlier failure".to_string())
        } else {
            None
        };

        while stop_reason.is_none() && active.len() < max_concurrency {
            if self.abort_signal.is_cancelled() {
                stop_reason = Some(
                    "Skipped because the parent run was cancelled before this subagent could start"
                        .to_string(),
                );
                break;
            }
            if let Some((index, task, delegate)) = queued.pop_front() {
                active.push(self.run_parallel_delegate_task(
                    index,
                    task,
                    delegate,
                    tool_call_id.to_string(),
                ));
            } else {
                break;
            }
        }

        while let Some(result) = active.next().await {
            if self.abort_signal.is_cancelled() {
                stop_reason.get_or_insert_with(|| {
                    "Skipped because the parent run was cancelled before this subagent could start"
                        .to_string()
                });
            } else if result.status == ParallelSubagentTaskStatus::Failed && request.fail_fast {
                stop_reason.get_or_insert_with(|| {
                    "Skipped because failFast stopped scheduling after an earlier failure"
                        .to_string()
                });
            }
            results.push(result);

            while stop_reason.is_none() && active.len() < max_concurrency {
                if self.abort_signal.is_cancelled() {
                    stop_reason = Some(
                        "Skipped because the parent run was cancelled before this subagent could start"
                            .to_string(),
                    );
                    break;
                }
                if let Some((index, task, delegate)) = queued.pop_front() {
                    active.push(self.run_parallel_delegate_task(
                        index,
                        task,
                        delegate,
                        tool_call_id.to_string(),
                    ));
                } else {
                    break;
                }
            }
        }

        if let Some(reason) = stop_reason {
            for (index, task, _delegate) in queued {
                results.push(parallel_task_skipped_result(
                    index,
                    task.agent,
                    task.task,
                    reason.clone(),
                ));
            }
        }

        results.sort_by_key(|result| result.index);
        let completed = results
            .iter()
            .filter(|result| result.status == ParallelSubagentTaskStatus::Completed)
            .count();
        let failed = results
            .iter()
            .filter(|result| result.status == ParallelSubagentTaskStatus::Failed)
            .count();
        let skipped = results
            .iter()
            .filter(|result| result.status == ParallelSubagentTaskStatus::Skipped)
            .count();
        let batch_status = if completed == request.tasks.len() {
            ParallelSubagentBatchStatus::Completed
        } else if completed == 0 {
            ParallelSubagentBatchStatus::Failed
        } else {
            ParallelSubagentBatchStatus::PartialFailure
        };
        let mut aggregate = ParallelSubagentSummary {
            summary: String::new(),
            mode: request.mode,
            result_format: request.result_format,
            batch_status,
            max_concurrency,
            total: request.tasks.len(),
            completed,
            failed,
            skipped,
            duration_ms: elapsed_ms_u64(batch_started),
            results,
        };
        aggregate.summary = render_parallel_summary(&aggregate);
        let details = serde_json::to_value(&aggregate).unwrap_or_else(|_| {
            serde_json::json!({
                "summary": aggregate.summary,
                "error": "failed to serialize parallel subagent result"
            })
        });

        let storage_status = if aggregate.batch_status == ParallelSubagentBatchStatus::Failed {
            "failed"
        } else {
            "completed"
        };
        tool_call_repo::update_result(
            &self.pool,
            tool_call_storage_id,
            &details.to_string(),
            storage_status,
        )
        .await
        .ok();

        AgentToolResult {
            content: vec![ContentBlock::Text(TextContent::new(aggregate.summary))],
            details: Some(details),
        }
    }

    async fn resolve_helper_delegate(
        &self,
        tool: RuntimeOrchestrationTool,
        tool_input: &serde_json::Value,
    ) -> Result<ResolvedHelperDelegate, String> {
        if tool == RuntimeOrchestrationTool::Parallel {
            return Err("agent_parallel cannot be used as an individual helper".to_string());
        }

        if tool == RuntimeOrchestrationTool::Judge {
            // agent_judge is a main-agent-only acceptance tool: it must not be
            // reachable as a generic helper delegate or as an agent_parallel
            // batch target.
            return Err(
                "agent_judge can only be called directly by the main agent for the current goal"
                    .to_string(),
            );
        }

        let HelperToolTask {
            task,
            review_request,
        } = resolve_helper_tool_task(tool.clone(), tool_input)?;

        let helper_profile = resolve_helper_profile(&tool);
        let resolved_profile = if let RuntimeOrchestrationTool::Custom(ref slug) = tool {
            match self.resolve_custom_subagent_profile(slug).await {
                Some(profile) => Some(profile),
                None => {
                    return Err(format!(
                        "Custom subagent 'agent_{slug}' not found or not enabled"
                    ));
                }
            }
        } else {
            helper_profile
        };
        let model_role =
            resolve_helper_model_role(&self.spec.model_plan, &tool, resolved_profile.as_ref())
                .ok_or_else(|| {
                    format!("No helper profile resolved for tool '{}'", tool.tool_name())
                })?;

        // The main agent is depth 1, so its direct delegates run at depth 2.
        // Reject targets whose configured max delegation depth cannot accommodate
        // being delegated to at depth 2 (mirrors the recursive subagent path's
        // enforcement in HelperDelegationContext::resolve_delegation_sync).
        if let Some(profile) = resolved_profile.as_ref() {
            let max_depth = profile.max_delegation_depth();
            if MAIN_AGENT_CHILD_DEPTH > max_depth {
                return Err(format!(
                    "{} cannot be delegated at depth {MAIN_AGENT_CHILD_DEPTH} (its max delegation depth is {max_depth})",
                    tool.tool_name()
                ));
            }
        }

        Ok(ResolvedHelperDelegate {
            agent_name: tool.tool_name(),
            tool,
            task,
            review_request,
            helper_profile: resolved_profile,
            model_role,
        })
    }

    async fn run_helper_for_delegate(
        &self,
        delegate: &ResolvedHelperDelegate,
        parent_tool_call_id: &str,
    ) -> Result<HelperRunResult, crate::model::errors::AppError> {
        // Custom subagents accessible from the active profile, so a delegated
        // helper that is allowed to delegate further can inject and resolve
        // `agent_{slug}` delegation tools without re-reading the active profile.
        let custom_delegation_targets =
            crate::core::agent_session_tools::list_custom_delegation_targets_from_pool(&self.pool)
                .await;
        self.helper_orchestrator
            .run_helper(HelperRunRequest {
                run_id: self.spec.run_id.clone(),
                thread_id: self.spec.thread_id.clone(),
                tool: delegate.tool.clone(),
                helper_profile: delegate.helper_profile.clone(),
                parent_tool_call_id: Some(parent_tool_call_id.to_string()),
                task: delegate.task.clone(),
                model_role: delegate.model_role.clone(),
                workspace_path: self.spec.workspace_path.clone(),
                run_mode: self.spec.run_mode.clone(),
                event_tx: self.event_tx.clone(),
                session_abort_signal: self.abort_signal.clone(),
                thinking_level: self.spec.model_plan.thinking_level,
                delegation_depth: MAIN_AGENT_CHILD_DEPTH,
                model_plan: self.spec.model_plan.clone(),
                custom_delegation_targets,
            })
            .await
    }

    async fn run_parallel_delegate_task(
        &self,
        index: usize,
        task: ParallelSubagentTask,
        delegate: ResolvedHelperDelegate,
        parent_tool_call_id: String,
    ) -> ParallelSubagentTaskResult {
        let started = Instant::now();
        let review_request = delegate.review_request.clone();
        let agent_name = delegate.agent_name.clone();
        let task_summary = task.task.clone();
        let tool = delegate.tool.clone();

        match self
            .run_helper_for_delegate(&delegate, &parent_tool_call_id)
            .await
        {
            Ok(summary) => ParallelSubagentTaskResult {
                index,
                agent: agent_name,
                task: task_summary,
                status: ParallelSubagentTaskStatus::Completed,
                summary: Some(summary.summary),
                raw_summary: summary.raw_summary.clone(),
                snapshot: Some(summary.snapshot),
                review_request,
                review_report: if tool == RuntimeOrchestrationTool::Review {
                    summary
                        .raw_summary
                        .as_deref()
                        .and_then(extract_review_report)
                } else {
                    None
                },
                error: None,
                duration_ms: elapsed_ms_u64(started),
            },
            Err(error) => parallel_task_failed_result(
                index,
                agent_name,
                task_summary,
                error.to_string(),
                elapsed_ms_u64(started),
            ),
        }
    }

    /// Resolve a custom subagent slug into a SubagentProfile by loading from the database.
    /// Validates both that the subagent is enabled and that it is accessible from the
    /// active agent profile via the `profile_subagent_access` table.
    async fn resolve_custom_subagent_profile(&self, slug: &str) -> Option<SubagentProfile> {
        crate::core::agent_session_tools::resolve_custom_subagent_profile_from_pool(
            &self.pool, slug,
        )
        .await
    }

    async fn execute_plan_checkpoint(&self, tool_input: &serde_json::Value) -> AgentToolResult {
        let plan_revision = match self.next_plan_revision().await {
            Ok(revision) => revision,
            Err(error) => return agent_error_result(error.to_string()),
        };
        let artifact = build_plan_artifact_from_tool_input(tool_input, plan_revision);

        // Require title, context, and design to be non-empty so that the plan
        // meets the quality contract enforced by the system prompt.
        if artifact.title == "Implementation Plan" {
            return agent_error_result(
                "update_plan failed: missing a real \"title\". Pass all required fields as non-empty top-level arguments: title, summary, context, design, keyImplementation, steps, verification, risks. Minimal example: {\"title\": \"Add X to Y\", \"summary\": \"...\", \"context\": \"...\", \"design\": \"...\", \"keyImplementation\": \"...\", \"steps\": [\"...\"], \"verification\": \"...\", \"risks\": [\"...\"]}.",
            );
        }
        if artifact.context.trim().is_empty() {
            return agent_error_result(
                "update_plan failed: \"context\" is empty. Provide a non-empty top-level \"context\" string describing the current state of the relevant code with inspected file paths and confirmed facts.",
            );
        }
        if artifact.design.trim().is_empty() {
            return agent_error_result(
                "update_plan failed: \"design\" is empty. Provide a non-empty top-level \"design\" string explaining the recommended approach, architecture changes, data flow, and tradeoffs.",
            );
        }

        let plan_message_id = uuid::Uuid::now_v7().to_string();
        let approval_message_id = uuid::Uuid::now_v7().to_string();
        let plan_metadata =
            build_plan_message_metadata(artifact.clone(), &self.spec.run_id, &self.spec.run_mode);
        let approval_metadata = build_approval_prompt_metadata(
            &self.pool,
            &self.spec.thread_id,
            artifact.plan_revision,
            &plan_message_id,
        )
        .await;

        let plan_message = MessageRecord {
            id: plan_message_id.clone(),
            thread_id: self.spec.thread_id.clone(),
            run_id: Some(self.spec.run_id.clone()),
            role: "assistant".to_string(),
            content_markdown: plan_markdown(&plan_metadata),
            parts_json: None,
            message_type: "plan".to_string(),
            status: "completed".to_string(),
            metadata_json: serde_json::to_string(&plan_metadata).ok(),
            attachments_json: None,
            created_at: String::new(),
        };
        let approval_message = MessageRecord {
            id: approval_message_id.clone(),
            thread_id: self.spec.thread_id.clone(),
            run_id: Some(self.spec.run_id.clone()),
            role: "assistant".to_string(),
            content_markdown: approval_prompt_markdown(&artifact),
            parts_json: None,
            message_type: "approval_prompt".to_string(),
            status: "completed".to_string(),
            metadata_json: serde_json::to_string(&approval_metadata).ok(),
            attachments_json: None,
            created_at: String::new(),
        };

        if let Err(error) = message_repo::insert(&self.pool, &plan_message).await {
            return agent_error_result(format!("failed to persist plan message: {error}"));
        }
        if let Err(error) = message_repo::insert(&self.pool, &approval_message).await {
            return agent_error_result(format!("failed to persist approval prompt: {error}"));
        }

        let _ = self.event_tx.send(ThreadStreamEvent::PlanUpdated {
            run_id: self.spec.run_id.clone(),
            plan: serde_json::to_value(&artifact).unwrap_or_else(|_| serde_json::json!({})),
        });

        // Persist plan markdown to ~/.tiy/plans/{thread_id}.md for incremental
        // plan refinement and downstream review verification.
        let plan_file_path = write_plan_file(&self.spec.thread_id, &plan_message.content_markdown)
            .ok()
            .map(|path| path.to_string_lossy().to_string());

        if let Some(ref path) = plan_file_path {
            tracing::info!(
                thread_id = %self.spec.thread_id,
                plan_revision = plan_revision,
                path = %path,
                "plan file written to disk"
            );
        }

        self.checkpoint_requested.store(true, Ordering::SeqCst);
        // Mirror AgentSession::cancel ordering: cancel in-flight tool calls,
        // abort any in-flight subagents for this run (so a helper mid-LLM-turn
        // stops rather than only exiting on its next poll), then abort the main
        // agent. abort_signal child tokens already cascade into tool executions,
        // but cancel_run is needed to stop subagent Agent run loops promptly.
        self.abort_signal.cancel();
        self.helper_orchestrator.cancel_run(&self.spec.run_id).await;
        self.agent.abort();

        let result_message = match &plan_file_path {
            Some(path) => format!(
                "Implementation plan published and saved to {path}. Waiting for approval before execution."
            ),
            None => "Implementation plan published. Waiting for approval before execution.".to_string(),
        };

        AgentToolResult {
            content: vec![ContentBlock::Text(TextContent::new(result_message))],
            details: Some(serde_json::json!({
                "planMessageId": plan_message_id,
                "approvalMessageId": approval_message_id,
                "planFilePath": plan_file_path,
                "plan": artifact,
            })),
        }
    }

    async fn execute_render(
        &self,
        tool_call_id: &str,
        tool_input: &serde_json::Value,
    ) -> AgentToolResult {
        let tool_call_storage_id = uuid::Uuid::now_v7().to_string();
        let artifact_id = uuid::Uuid::now_v7().to_string();

        // Persist the tool call
        if let Err(error) = tool_call_repo::insert(
            &self.pool,
            &tool_call_repo::ToolCallInsert {
                id: tool_call_storage_id.clone(),
                tool_call_id: tool_call_id.to_string(),
                run_id: self.spec.run_id.clone(),
                thread_id: self.spec.thread_id.clone(),
                helper_id: None,
                tool_name: "render".to_string(),
                tool_input_json: tool_input.to_string(),
                status: "requested".to_string(),
            },
        )
        .await
        {
            return agent_error_result(format!("failed to persist tool call: {error}"));
        }

        let _ = self.event_tx.send(ThreadStreamEvent::ToolRequested {
            run_id: self.spec.run_id.clone(),
            tool_call_id: tool_call_id.to_string(),
            tool_name: "render".to_string(),
            tool_input: tool_input.clone(),
        });

        let title = tool_input
            .get("title")
            .and_then(serde_json::Value::as_str)
            .map(String::from);
        let caption = tool_input
            .get("caption")
            .and_then(serde_json::Value::as_str)
            .map(String::from);
        let library = tool_input
            .get("library")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("vega-lite")
            .to_string();

        // Validate input based on library type
        let chart_payload = match library.as_str() {
            "html" | "svg" => {
                // Resolve source: from inline 'source' or from file 'path'
                let source = if let Some(inline) = tool_input.get("source").and_then(|s| s.as_str())
                {
                    if inline.is_empty() {
                        None
                    } else {
                        Some(inline.to_string())
                    }
                } else if let Some(raw_path) = tool_input.get("path").and_then(|s| s.as_str()) {
                    // Read from workspace file
                    let workspace_root = std::path::Path::new(&self.spec.workspace_path);
                    let resolved = workspace_paths::resolve_path_within_roots(
                        workspace_root,
                        &[],
                        raw_path,
                        crate::model::errors::ErrorSource::Tool,
                        "render.path.outside_workspace",
                        "render path is outside the workspace boundary",
                    );
                    match resolved {
                        Ok(path) => match tokio::fs::read_to_string(&path).await {
                            Ok(content) if !content.is_empty() => Some(content),
                            Ok(_) => None,
                            Err(e) => {
                                let error = format!("failed to read file '{}': {}", raw_path, e);
                                let error_json = serde_json::json!({ "error": &error });
                                tool_call_repo::update_result(
                                    &self.pool,
                                    &tool_call_storage_id,
                                    &error_json.to_string(),
                                    "failed",
                                )
                                .await
                                .ok();
                                let _ = self.event_tx.send(ThreadStreamEvent::ToolFailed {
                                    run_id: self.spec.run_id.clone(),
                                    tool_call_id: tool_call_id.to_string(),
                                    error: error.clone(),
                                });
                                return agent_error_result(error);
                            }
                        },
                        Err(e) => {
                            let error = format!("invalid path '{}': {}", raw_path, e);
                            let error_json = serde_json::json!({ "error": &error });
                            tool_call_repo::update_result(
                                &self.pool,
                                &tool_call_storage_id,
                                &error_json.to_string(),
                                "failed",
                            )
                            .await
                            .ok();
                            let _ = self.event_tx.send(ThreadStreamEvent::ToolFailed {
                                run_id: self.spec.run_id.clone(),
                                tool_call_id: tool_call_id.to_string(),
                                error: error.to_string(),
                            });
                            return agent_error_result(error);
                        }
                    }
                } else {
                    None
                };

                let source = match source {
                    Some(s) => s,
                    None => {
                        let error = format!(
                            "render with library '{}' requires a non-empty 'source' string or a valid 'path'",
                            library
                        );
                        let error_json = serde_json::json!({ "error": &error });
                        tool_call_repo::update_result(
                            &self.pool,
                            &tool_call_storage_id,
                            &error_json.to_string(),
                            "failed",
                        )
                        .await
                        .ok();
                        let _ = self.event_tx.send(ThreadStreamEvent::ToolFailed {
                            run_id: self.spec.run_id.clone(),
                            tool_call_id: tool_call_id.to_string(),
                            error: error.clone(),
                        });
                        return agent_error_result(error);
                    }
                };

                // Enforce 1MB size limit
                if source.len() > 1_048_576 {
                    let error = "render source exceeds maximum size of 1MB".to_string();
                    let error_json = serde_json::json!({ "error": &error });
                    tool_call_repo::update_result(
                        &self.pool,
                        &tool_call_storage_id,
                        &error_json.to_string(),
                        "failed",
                    )
                    .await
                    .ok();
                    let _ = self.event_tx.send(ThreadStreamEvent::ToolFailed {
                        run_id: self.spec.run_id.clone(),
                        tool_call_id: tool_call_id.to_string(),
                        error: error.clone(),
                    });
                    return agent_error_result(error);
                }
                serde_json::json!({
                    "library": library,
                    "source": source,
                    "title": title,
                    "caption": caption,
                    "status": "ready",
                })
            }
            _ => {
                // vega-lite (default): validate spec field
                let spec = match tool_input.get("spec") {
                    Some(s) if s.is_object() || s.is_array() => s.clone(),
                    _ => {
                        let error =
                            "render with library 'vega-lite' requires a valid 'spec' object"
                                .to_string();
                        let error_json = serde_json::json!({ "error": &error });
                        tool_call_repo::update_result(
                            &self.pool,
                            &tool_call_storage_id,
                            &error_json.to_string(),
                            "failed",
                        )
                        .await
                        .ok();
                        let _ = self.event_tx.send(ThreadStreamEvent::ToolFailed {
                            run_id: self.spec.run_id.clone(),
                            tool_call_id: tool_call_id.to_string(),
                            error: error.clone(),
                        });
                        return agent_error_result(error);
                    }
                };
                serde_json::json!({
                    "library": library,
                    "spec": spec,
                    "title": title,
                    "caption": caption,
                    "status": "ready",
                })
            }
        };

        let host_message_id = uuid::Uuid::now_v7().to_string();
        let host_message = match render_artifact_host_message(
            &self.spec.thread_id,
            &self.spec.run_id,
            tool_call_id,
            &host_message_id,
            &artifact_id,
            &library,
            &chart_payload,
        ) {
            Ok(host_message) => host_message,
            Err(error) => {
                let error = format!("failed to serialize render artifact message parts: {error}");
                let error_json = serde_json::json!({ "error": &error });
                tool_call_repo::update_result(
                    &self.pool,
                    &tool_call_storage_id,
                    &error_json.to_string(),
                    "failed",
                )
                .await
                .ok();
                let _ = self.event_tx.send(ThreadStreamEvent::ToolFailed {
                    run_id: self.spec.run_id.clone(),
                    tool_call_id: tool_call_id.to_string(),
                    error: error.clone(),
                });
                return agent_error_result(error);
            }
        };

        if let Err(error) = message_repo::insert(&self.pool, &host_message).await {
            let error = format!("failed to persist render artifact host message: {error}");
            let error_json = serde_json::json!({ "error": &error });
            tool_call_repo::update_result(
                &self.pool,
                &tool_call_storage_id,
                &error_json.to_string(),
                "failed",
            )
            .await
            .ok();
            let _ = self.event_tx.send(ThreadStreamEvent::ToolFailed {
                run_id: self.spec.run_id.clone(),
                tool_call_id: tool_call_id.to_string(),
                error: error.clone(),
            });
            return agent_error_result(error);
        }

        // Emit started event
        let _ = self.event_tx.send(ThreadStreamEvent::ArtifactUpdated {
            run_id: self.spec.run_id.clone(),
            message_id: host_message_id.clone(),
            artifact_id: artifact_id.clone(),
            artifact_type: "chart".to_string(),
            status: ArtifactStatus::Started,
            payload: Some(chart_payload.clone()),
            error: None,
        });

        // Emit completed event
        let _ = self.event_tx.send(ThreadStreamEvent::ArtifactUpdated {
            run_id: self.spec.run_id.clone(),
            message_id: host_message_id.clone(),
            artifact_id: artifact_id.clone(),
            artifact_type: "chart".to_string(),
            status: ArtifactStatus::Completed,
            payload: Some(chart_payload.clone()),
            error: None,
        });

        // Mark tool call as completed
        let result_json = serde_json::json!({
            "success": true,
            "artifactId": artifact_id,
            "messageId": host_message_id,
            "library": library,
            "title": title,
            "caption": caption,
        });
        tool_call_repo::update_result(
            &self.pool,
            &tool_call_storage_id,
            &result_json.to_string(),
            "completed",
        )
        .await
        .ok();

        let _ = self.event_tx.send(ThreadStreamEvent::ToolCompleted {
            run_id: self.spec.run_id.clone(),
            tool_call_id: tool_call_id.to_string(),
            tool_name: "render".to_string(),
            result: result_json.clone(),
        });

        AgentToolResult {
            content: vec![ContentBlock::Text(TextContent::new(
                serde_json::to_string(&result_json)
                    .unwrap_or_else(|_| "Artifact rendered successfully".to_string()),
            ))],
            details: Some(result_json),
        }
    }

    async fn execute_task_tool(
        &self,
        tool_name: &str,
        tool_call_id: &str,
        tool_input: &serde_json::Value,
    ) -> AgentToolResult {
        use crate::core::task_board_manager;
        use crate::model::task_board::{CreateTaskInput, QueryTaskInput, UpdateTaskInput};

        // Persist the tool call record
        let tool_call_storage_id = uuid::Uuid::now_v7().to_string();
        if let Err(error) = tool_call_repo::insert(
            &self.pool,
            &tool_call_repo::ToolCallInsert {
                id: tool_call_storage_id.clone(),
                tool_call_id: tool_call_id.to_string(),
                run_id: self.spec.run_id.clone(),
                thread_id: self.spec.thread_id.clone(),
                helper_id: None,
                tool_name: tool_name.to_string(),
                tool_input_json: tool_input.to_string(),
                status: "requested".to_string(),
            },
        )
        .await
        {
            return agent_error_result(format!("failed to persist tool call: {error}"));
        }

        let _ = self.event_tx.send(ThreadStreamEvent::ToolRequested {
            run_id: self.spec.run_id.clone(),
            tool_call_id: tool_call_id.to_string(),
            tool_name: tool_name.to_string(),
            tool_input: tool_input.clone(),
        });

        if let Err(error) =
            tool_call_repo::update_status(&self.pool, &tool_call_storage_id, "running").await
        {
            let message = format!("failed to mark task tool call as running: {error}");
            let _ = self.event_tx.send(ThreadStreamEvent::ToolFailed {
                run_id: self.spec.run_id.clone(),
                tool_call_id: tool_call_id.to_string(),
                error: message.clone(),
            });
            return agent_error_result(message);
        }

        let _ = self.event_tx.send(ThreadStreamEvent::ToolRunning {
            run_id: self.spec.run_id.clone(),
            tool_call_id: tool_call_id.to_string(),
        });

        let result: Result<serde_json::Value, String> = if tool_name == "create_task" {
            match serde_json::from_value::<CreateTaskInput>(tool_input.clone()) {
                Ok(input) => {
                    match task_board_manager::create_task_board(
                        &self.pool,
                        &self.spec.thread_id,
                        &input,
                    )
                    .await
                    {
                        Ok(dto) => {
                            let _ = self.event_tx.send(ThreadStreamEvent::TaskBoardUpdated {
                                run_id: self.spec.run_id.clone(),
                                task_board: dto.clone(),
                            });
                            serde_json::to_value(&dto).map_err(|error| {
                                format!("Failed to serialize create_task result: {error}")
                            })
                        }
                        Err(e) => Err(e.to_string()),
                    }
                }
                Err(e) => Err(format!("Invalid create_task input: {}", e)),
            }
        } else if tool_name == "update_task" {
            match serde_json::from_value::<UpdateTaskInput>(tool_input.clone()) {
                Ok(input) => {
                    match task_board_manager::update_task_board(
                        &self.pool,
                        &self.spec.thread_id,
                        &input,
                    )
                    .await
                    {
                        Ok(dto) => {
                            let _ = self.event_tx.send(ThreadStreamEvent::TaskBoardUpdated {
                                run_id: self.spec.run_id.clone(),
                                task_board: dto.clone(),
                            });
                            serde_json::to_value(&dto).map_err(|error| {
                                format!("Failed to serialize update_task result: {error}")
                            })
                        }
                        Err(e) => Err(e.to_string()),
                    }
                }
                Err(e) => Err(format!("Invalid update_task input: {}", e)),
            }
        } else if tool_name == "query_task" {
            match serde_json::from_value::<QueryTaskInput>(tool_input.clone()) {
                Ok(input) => task_board_manager::query_thread_task_boards(
                    &self.pool,
                    &self.spec.thread_id,
                    input.scope,
                )
                .await
                .map_err(|error| error.to_string())
                .and_then(|result| {
                    serde_json::to_value(&result)
                        .map_err(|error| format!("Failed to serialize query_task result: {error}"))
                }),
                Err(e) => Err(format!("Invalid query_task input: {}", e)),
            }
        } else {
            Err(format!("Unknown task tool: {}", tool_name))
        };

        match result {
            Ok(result_json) => {
                tool_call_repo::update_result(
                    &self.pool,
                    &tool_call_storage_id,
                    &result_json.to_string(),
                    "completed",
                )
                .await
                .ok();

                let _ = self.event_tx.send(ThreadStreamEvent::ToolCompleted {
                    run_id: self.spec.run_id.clone(),
                    tool_call_id: tool_call_id.to_string(),
                    tool_name: tool_name.to_string(),
                    result: result_json.clone(),
                });

                AgentToolResult {
                    content: vec![ContentBlock::Text(TextContent::new(
                        serde_json::to_string(&result_json)
                            .unwrap_or_else(|_| "Task updated successfully".to_string()),
                    ))],
                    details: Some(result_json),
                }
            }
            Err(error) => {
                let error_json = serde_json::json!({ "error": &error });
                tool_call_repo::update_result(
                    &self.pool,
                    &tool_call_storage_id,
                    &error_json.to_string(),
                    "failed",
                )
                .await
                .ok();

                let _ = self.event_tx.send(ThreadStreamEvent::ToolFailed {
                    run_id: self.spec.run_id.clone(),
                    tool_call_id: tool_call_id.to_string(),
                    error: error.clone(),
                });

                agent_error_result(error)
            }
        }
    }

    // ── Goal acceptance Judge handler ──

    /// Run the main-agent-only `agent_judge` acceptance flow: build a Judge task
    /// with the current goal injected, run the Judge helper, parse its structured
    /// verdict, persist it, and (on pass) flip the goal to verified/complete.
    async fn execute_judge_tool(
        &self,
        tool_call_id: &str,
        tool_call_storage_id: &str,
        tool_input: &serde_json::Value,
    ) -> AgentToolResult {
        // Validate the tool input shape. The main agent's optional note is
        // not injected into the Judge prompt — the Judge evaluates the
        // project state independently against the goal. Parsing is retained
        // to reject malformed input (e.g. non-string `task` values) that
        // would violate the tool JSON schema. A missing `task` is acceptable
        // and falls back to a neutral default.
        if let Err(error) = crate::core::subagent::JudgeRequest::from_tool_input(tool_input) {
            tool_call_repo::update_result(
                &self.pool,
                tool_call_storage_id,
                &serde_json::json!({ "error": &error }).to_string(),
                "failed",
            )
            .await
            .ok();
            return agent_error_result(error);
        }

        // Backstop: re-query goal state. agent_judge is injected only when an
        // un-verified goal exists, but a stale tool set or a direct call must be
        // rejected here too.
        let goal = match crate::persistence::repo::goal_repo::find_by_thread_id(
            &self.pool,
            &self.spec.thread_id,
        )
        .await
        {
            Ok(Some(goal)) => goal,
            Ok(None) => {
                let err_msg =
                    "agent_judge cannot run: no goal exists for this thread. Create one with the /goal command first.";
                tool_call_repo::update_result(
                    &self.pool,
                    tool_call_storage_id,
                    &serde_json::json!({ "error": err_msg }).to_string(),
                    "failed",
                )
                .await
                .ok();
                return agent_error_result(err_msg);
            }
            Err(e) => {
                let err_msg = format!("Failed to load goal: {e}");
                tool_call_repo::update_result(
                    &self.pool,
                    tool_call_storage_id,
                    &serde_json::json!({ "error": &err_msg }).to_string(),
                    "failed",
                )
                .await
                .ok();
                return agent_error_result(err_msg);
            }
        };

        if goal.status == crate::model::goal::GoalStatus::Complete && goal.judge_passed {
            let err_msg =
                "The goal has already passed acceptance. No further verification is needed.";
            tool_call_repo::update_result(
                &self.pool,
                tool_call_storage_id,
                &serde_json::json!({ "error": err_msg }).to_string(),
                "failed",
            )
            .await
            .ok();
            return agent_error_result(err_msg);
        }

        // Build the Judge task: inject the goal objective, the task board
        // state, (when applicable) process compliance evidence, and (when
        // this is a re-verification) the previous Judge verdict. The Judge
        // receives no input from the main agent — it evaluates the project
        // state independently against the goal. The previous verdict is
        // included only as objective context so the Judge can confirm prior
        // findings have actually been addressed, not as a starting point
        // for a self-assessment.

        // Query task board state for cross-reference.
        let task_board_summary = build_task_board_summary(&self.pool, &goal.thread_id).await;

        // Conditionally include process compliance layer for goals that
        // require reviews or phase-by-phase verification.
        let process_compliance = if has_process_requirements(&goal.objective) {
            build_process_compliance_summary(&self.pool, &goal.thread_id).await
        } else {
            String::new()
        };

        // On re-verification, surface the prior Judge verdict as objective
        // context so the Judge can confirm each prior finding has been
        // genuinely resolved. This is read from the goal record (not from
        // the main agent) and is empty on the first verification.
        let prior_verdict = if goal.judge_evaluated_run_id.is_some() {
            let mut section = String::new();
            if let Some(summary) = goal.judge_summary.as_deref() {
                if !summary.trim().is_empty() {
                    section.push_str(&format!("\nPrevious Judge summary: {summary}"));
                }
            }
            if let Some(findings_json) = goal.judge_findings.as_deref() {
                if let Ok(findings) = serde_json::from_str::<Vec<String>>(findings_json) {
                    if !findings.is_empty() {
                        section.push_str("\nPrevious Judge findings:");
                        for finding in findings {
                            section.push_str(&format!("\n- {finding}"));
                        }
                    }
                }
            }
            section
        } else {
            String::new()
        };

        let judge_task = format!(
            "You are verifying acceptance of the following goal for the current project.\n\n\
Goal objective:\n{objective}\n\n\
{task_board_summary}\
{process_compliance}\
{prior_verdict}\n\
Independently inspect the project's current state and decide whether it satisfies the goal. \
You must verify ALL requirements in the goal, not just those that seem to have been worked on. \
Cross-reference the task board state above with your file-system findings. \
If this is a re-verification, confirm that every prior finding has been genuinely \
resolved (do NOT accept claims of fix without verifying the actual change). \
Return your structured JudgeReport verdict.",
            objective = goal.objective,
            task_board_summary = task_board_summary,
            process_compliance = process_compliance,
            prior_verdict = prior_verdict,
        );

        // Build a Judge delegate (depth 2, primary model) and run it.
        let tool = RuntimeOrchestrationTool::Judge;
        let helper_profile = resolve_helper_profile(&tool);
        let model_role = match resolve_helper_model_role(
            &self.spec.model_plan,
            &tool,
            helper_profile.as_ref(),
        ) {
            Some(role) => role,
            None => {
                let err_msg = "Failed to resolve a model for agent_judge.".to_string();
                tool_call_repo::update_result(
                    &self.pool,
                    tool_call_storage_id,
                    &serde_json::json!({ "error": &err_msg }).to_string(),
                    "failed",
                )
                .await
                .ok();
                return agent_error_result(err_msg);
            }
        };

        let delegate = ResolvedHelperDelegate {
            tool: tool.clone(),
            agent_name: tool.tool_name(),
            task: judge_task,
            review_request: None,
            helper_profile,
            model_role,
        };

        let report: JudgeReport = match self.run_helper_for_delegate(&delegate, tool_call_id).await
        {
            Ok(summary) => extract_judge_report(
                summary
                    .raw_summary
                    .as_deref()
                    .unwrap_or(summary.summary.as_str()),
            ),
            Err(error) => {
                let err_msg = format!("agent_judge failed to run: {error}");
                tool_call_repo::update_result(
                    &self.pool,
                    tool_call_storage_id,
                    &serde_json::json!({ "error": &err_msg }).to_string(),
                    "failed",
                )
                .await
                .ok();
                return agent_error_result(err_msg);
            }
        };

        // Persist the verdict (atomically flips to complete + judge_passed on pass).
        let findings_json =
            serde_json::to_string(&report.findings).unwrap_or_else(|_| "[]".to_string());
        let recorded = crate::persistence::repo::goal_repo::record_judge_verdict(
            &self.pool,
            &goal.id,
            &self.spec.run_id,
            report.passed,
            report.completeness_pct as i64,
            &findings_json,
            &report.summary,
        )
        .await;

        if let Err(e) = recorded {
            let err_msg = format!("Failed to persist Judge verdict: {e}");
            tool_call_repo::update_result(
                &self.pool,
                tool_call_storage_id,
                &serde_json::json!({ "error": &err_msg }).to_string(),
                "failed",
            )
            .await
            .ok();
            return agent_error_result(err_msg);
        }

        // Emit goal events with the freshly updated record.
        if let Ok(Some(record)) =
            crate::persistence::repo::goal_repo::find_by_thread_id(&self.pool, &self.spec.thread_id)
                .await
        {
            let payload = crate::core::goal_manager::GoalManager::to_payload(&record);
            if report.passed {
                let _ = self.event_tx.send(ThreadStreamEvent::GoalCompleted {
                    thread_id: record.thread_id.clone(),
                    evidence: record.evidence.clone().unwrap_or_default(),
                });
            }
            let _ = self.event_tx.send(ThreadStreamEvent::GoalStateUpdated {
                thread_id: record.thread_id.clone(),
                goal: Some(payload),
            });
        }

        let result_text = crate::core::subagent::judge_contract::render_parent_summary(&report);
        tool_call_repo::update_result(
            &self.pool,
            tool_call_storage_id,
            &serde_json::json!({ "output": &result_text, "passed": report.passed }).to_string(),
            "completed",
        )
        .await
        .ok();

        AgentToolResult {
            content: vec![ContentBlock::Text(TextContent::new(result_text))],
            details: Some(serde_json::json!({
                "passed": report.passed,
                "completenessPct": report.completeness_pct,
                "findings": report.findings,
                "summary": report.summary,
            })),
        }
    }
}

/// Build a human-readable summary of the task board state for the Judge.
/// Returns a string describing each step and its stage, or a note that no
/// task board exists.
async fn build_task_board_summary(pool: &sqlx::SqlitePool, thread_id: &str) -> String {
    use crate::persistence::repo::{task_board_repo, task_item_repo};

    let boards = match task_board_repo::list_by_thread(pool, thread_id).await {
        Ok(boards) => boards,
        Err(_) => return "(No task board data available.)\n".to_string(),
    };

    if boards.is_empty() {
        return "(No task board exists for this goal. Verify entirely from file system and goal text.)\n"
            .to_string();
    }

    let mut summary = String::from("## Associated task board state\n\n");
    for board in &boards {
        // Skip abandoned boards — they are not relevant to the current goal.
        if board.status.as_str() == "abandoned" {
            continue;
        }
        summary.push_str(&format!(
            "**{}** (status: {}):\n",
            board.title,
            board.status.as_str()
        ));

        let items = match task_item_repo::list_by_task_board(pool, &board.id).await {
            Ok(items) => items,
            Err(_) => {
                summary.push_str("  (Could not load task items.)\n");
                continue;
            }
        };

        if items.is_empty() {
            summary.push_str("  (No task items.)\n");
            continue;
        }

        for item in &items {
            summary.push_str(&format!(
                "  - [{}] {}\n",
                item.stage.as_str(),
                item.description
            ));
        }
    }

    summary.push_str(
        "\n**Important**: Any step above that is not `completed` and maps to a goal \
         requirement is evidence of incomplete work. Report these as findings.\n",
    );
    summary
}

/// Check whether the goal objective contains process requirements (e.g.,
/// "review each phase", "每阶段验收"). When true, the Judge prompt will
/// include a process compliance layer showing the thread's review call history.
fn has_process_requirements(objective: &str) -> bool {
    let lower = objective.to_lowercase();
    let keywords = [
        "review",
        "验收",
        "检查",
        "verify each",
        "verify every",
        "per phase",
        "每个阶段",
        "每一阶段",
        "每轮",
        "阶段完成",
    ];
    keywords.iter().any(|kw| lower.contains(&kw.to_lowercase()))
}

/// Build a process compliance summary from the thread's run_helper history.
/// Lists all review-related helper calls chronologically with their input
/// summaries and status. Only meaningful when the goal objective contains
/// process requirements (e.g., "each phase must have a review").
async fn build_process_compliance_summary(pool: &sqlx::SqlitePool, thread_id: &str) -> String {
    use crate::persistence::repo::run_helper_repo;

    let helpers = match run_helper_repo::list_by_thread_id(pool, thread_id).await {
        Ok(h) => h,
        Err(_) => return String::new(),
    };

    // Filter for review-related calls: agent_review, helper_review
    let reviews: Vec<_> = helpers
        .iter()
        .filter(|h| h.helper_kind.contains("review"))
        .collect();

    if reviews.is_empty() {
        return format!(
            "## Process compliance\n\n\
            No review calls found in thread history. \
            If the goal requires reviews, this is evidence of non-compliance.\n\n"
        );
    }

    let mut summary = String::from("## Process compliance\n\n");
    summary.push_str("The following review calls were recorded during this goal:\n\n");

    for (i, review) in reviews.iter().enumerate() {
        let status_label = match review.status.as_str() {
            "completed" => "✓ completed",
            "failed" => "✗ failed",
            "interrupted" => "⚠ interrupted",
            _ => &review.status,
        };

        let input_preview = review
            .input_summary
            .as_deref()
            .map(|s| {
                // Truncate to first 200 chars for readability (character-safe,
                // avoids panicking on multi-byte UTF-8 sequences).
                if s.chars().count() > 200 {
                    format!("{}...", s.chars().take(200).collect::<String>())
                } else {
                    s.to_string()
                }
            })
            .unwrap_or_else(|| "(no task description)".to_string());

        summary.push_str(&format!(
            "{}. `{}` called at {} (status: {})\n   Scope: {}\n",
            i + 1,
            review.helper_kind,
            // Truncate to first 19 chars (RFC3339 timestamp prefix) using
            // char-aware slicing to avoid panicking on multi-byte UTF-8
            // boundaries — mirrors the 200-char limit used on
            // `input_preview` above.
            review.started_at.chars().take(19).collect::<String>(),
            status_label,
            input_preview,
        ));
    }

    summary.push_str(
        "\n**Guidance**: If the goal requires reviews at specific milestones \
         (e.g., \"after each phase\"), verify that the review calls above \
         cover all required milestones. Missing or failed reviews are findings.\n\n",
    );
    summary
}

#[cfg(test)]
mod tests {
    use super::{
        render_artifact_host_message, render_chart_part, render_host_message_metadata,
        resolve_helper_tool_task, validate_parallel_delegate_safety, ResolvedHelperDelegate,
        RuntimeOrchestrationTool,
    };
    use crate::core::agent_session::ResolvedModelRole;
    use crate::core::subagent::review_contract::{GlobalScanMode, ReviewScope, ReviewTarget};
    use crate::core::subagent::SubagentProfile;
    use crate::model::subagent::CustomSubagentModelRole;
    use tiycore::types::{Cost, InputType, Model, Provider};

    #[test]
    fn render_chart_part_maps_vega_payload_fields() {
        let part = render_chart_part(
            "artifact-1",
            &serde_json::json!({
                "library": "vega-lite",
                "spec": { "mark": "bar" },
                "title": "Sales",
                "caption": "Quarterly sales",
                "status": "ready"
            }),
        );

        assert_eq!(
            part.get("type").and_then(serde_json::Value::as_str),
            Some("chart")
        );
        assert_eq!(
            part.get("artifactId").and_then(serde_json::Value::as_str),
            Some("artifact-1")
        );
        assert_eq!(
            part.pointer("/spec/mark")
                .and_then(serde_json::Value::as_str),
            Some("bar")
        );
        assert_eq!(part.get("source"), Some(&serde_json::Value::Null));
        assert_eq!(part.get("error"), Some(&serde_json::Value::Null));
    }

    #[test]
    fn render_chart_part_maps_html_source_fields() {
        let part = render_chart_part(
            "artifact-html",
            &serde_json::json!({
                "library": "html",
                "source": "<div>Hello</div>",
                "title": "HTML preview",
                "caption": null,
                "status": "ready"
            }),
        );

        assert_eq!(
            part.get("library").and_then(serde_json::Value::as_str),
            Some("html")
        );
        assert_eq!(
            part.get("source").and_then(serde_json::Value::as_str),
            Some("<div>Hello</div>")
        );
        assert_eq!(part.get("spec"), Some(&serde_json::json!({})));
    }

    #[test]
    fn render_host_message_metadata_marks_render_artifact_hosts() {
        let metadata = render_host_message_metadata("tool-call-1", "artifact-1", "svg");

        assert_eq!(
            metadata.get("kind").and_then(serde_json::Value::as_str),
            Some("render_artifact_host")
        );
        assert_eq!(
            metadata
                .get("toolCallId")
                .and_then(serde_json::Value::as_str),
            Some("tool-call-1")
        );
        assert_eq!(
            metadata
                .get("artifactId")
                .and_then(serde_json::Value::as_str),
            Some("artifact-1")
        );
        assert_eq!(
            metadata.get("library").and_then(serde_json::Value::as_str),
            Some("svg")
        );
    }

    #[test]
    fn render_artifact_host_message_contains_persistable_chart_part() {
        let message = render_artifact_host_message(
            "thread-1",
            "run-1",
            "tool-call-1",
            "host-msg-1",
            "artifact-1",
            "vega-lite",
            &serde_json::json!({
                "library": "vega-lite",
                "spec": { "mark": "line" },
                "title": "Trend",
                "caption": "A trend chart",
                "status": "ready"
            }),
        )
        .expect("render host message should serialize");

        assert_eq!(message.id, "host-msg-1");
        assert_eq!(message.thread_id, "thread-1");
        assert_eq!(message.run_id.as_deref(), Some("run-1"));
        assert_eq!(message.role, "assistant");
        assert_eq!(message.message_type, "plain_message");
        assert_eq!(message.status, "completed");
        assert!(message.content_markdown.is_empty());

        let parts: Vec<serde_json::Value> = serde_json::from_str(
            message
                .parts_json
                .as_deref()
                .expect("host should include parts_json"),
        )
        .expect("parts_json should parse");
        assert_eq!(parts.len(), 1);
        assert_eq!(
            parts[0]
                .get("artifactId")
                .and_then(serde_json::Value::as_str),
            Some("artifact-1")
        );
        assert_eq!(
            parts[0]
                .pointer("/spec/mark")
                .and_then(serde_json::Value::as_str),
            Some("line")
        );

        let metadata: serde_json::Value = serde_json::from_str(
            message
                .metadata_json
                .as_deref()
                .expect("host should include metadata_json"),
        )
        .expect("metadata_json should parse");
        assert_eq!(
            metadata.get("kind").and_then(serde_json::Value::as_str),
            Some("render_artifact_host")
        );
        assert_eq!(
            metadata
                .get("toolCallId")
                .and_then(serde_json::Value::as_str),
            Some("tool-call-1")
        );
    }

    #[test]
    fn resolve_helper_tool_task_trims_explore_task() {
        let resolved = resolve_helper_tool_task(
            RuntimeOrchestrationTool::Explore,
            &serde_json::json!({ "task": "  map the runtime files  " }),
        )
        .expect("explore task should resolve");

        assert_eq!(resolved.task, "map the runtime files");
        assert!(resolved.review_request.is_none());
    }

    #[test]
    fn resolve_helper_tool_task_rejects_missing_or_blank_explore_task() {
        let missing = resolve_helper_tool_task(
            RuntimeOrchestrationTool::Explore,
            &serde_json::json!({ "changedFiles": [] }),
        )
        .expect_err("missing task should be rejected");
        assert_eq!(missing, "missing helper task");

        let blank = resolve_helper_tool_task(
            RuntimeOrchestrationTool::Explore,
            &serde_json::json!({ "task": "   \n\t  " }),
        )
        .expect_err("blank task should be rejected");
        assert_eq!(blank, "missing helper task");
    }

    #[test]
    fn resolve_helper_tool_task_rejects_invalid_review_request() {
        let error = resolve_helper_tool_task(
            RuntimeOrchestrationTool::Review,
            &serde_json::json!({
                "task": "review the diff",
                "target": "not-a-target",
                "reviewScope": "local",
                "globalScanMode": "off"
            }),
        )
        .expect_err("invalid review target should be rejected");

        assert!(error.contains("invalid review target"));
    }

    #[test]
    fn resolve_helper_tool_task_builds_review_prompt_and_request() {
        let resolved = resolve_helper_tool_task(
            RuntimeOrchestrationTool::Review,
            &serde_json::json!({
                "task": "  review implemented tests  ",
                "target": "code",
                "reviewScope": "local",
                "globalScanMode": "off",
                "changedFiles": ["src-tauri/src/core/agent_session_execution.rs"],
                "preferredChecks": ["cargo test --manifest-path src-tauri/Cargo.toml agent_session_execution"],
                "riskHints": ["tests"],
                "planFilePath": " /tmp/plan.md "
            }),
        )
        .expect("review request should resolve");

        assert!(resolved.task.contains("Review request:"));
        assert!(resolved.task.contains("- task: review implemented tests"));
        assert!(resolved.task.contains("- plan_file_path: /tmp/plan.md"));
        let request = resolved
            .review_request
            .expect("review request metadata should be retained");
        assert_eq!(request.task, "review implemented tests");
        assert_eq!(request.target, ReviewTarget::Code);
        assert_eq!(request.review_scope, ReviewScope::Local);
        assert_eq!(request.global_scan_mode, GlobalScanMode::Off);
        assert_eq!(
            request.changed_files,
            vec!["src-tauri/src/core/agent_session_execution.rs".to_string()]
        );
        assert_eq!(request.risk_hints, vec!["tests".to_string()]);
        assert_eq!(request.plan_file_path.as_deref(), Some("/tmp/plan.md"));
    }

    #[test]
    fn parallel_delegate_safety_allows_read_only_custom_tools() {
        let delegate = test_custom_delegate(vec!["read", "search", "term_output"]);

        assert!(validate_parallel_delegate_safety(&delegate).is_ok());
    }

    #[test]
    fn parallel_delegate_safety_rejects_mutating_custom_tools() {
        let delegate = test_custom_delegate(vec!["read", "edit", "shell"]);
        let error = validate_parallel_delegate_safety(&delegate)
            .expect_err("mutating custom helper should be rejected");

        assert!(error.contains("non-read-only tools"));
        assert!(error.contains("edit"));
        assert!(error.contains("shell"));
    }

    fn test_custom_delegate(allowed_tools: Vec<&str>) -> ResolvedHelperDelegate {
        ResolvedHelperDelegate {
            tool: RuntimeOrchestrationTool::Custom("custom".to_string()),
            agent_name: "agent_custom".to_string(),
            task: "custom task".to_string(),
            review_request: None,
            helper_profile: Some(SubagentProfile::Custom {
                slug: "custom".to_string(),
                name: "Custom".to_string(),
                invocation_description: "custom agent".to_string(),
                system_prompt: "custom prompt".to_string(),
                allowed_tools: allowed_tools.into_iter().map(str::to_string).collect(),
                model_role: CustomSubagentModelRole::Auxiliary,
                can_delegate: false,
                max_delegation_depth: 3,
            }),
            model_role: test_model_role(),
        }
    }

    fn test_model_role() -> ResolvedModelRole {
        let model = Model::builder()
            .id("test-model")
            .name("Test Model")
            .provider(Provider::OpenAI)
            .base_url("https://example.com")
            .input(vec![InputType::Text])
            .context_window(128_000)
            .max_tokens(32_000)
            .cost(Cost::default())
            .build()
            .expect("test model should build");

        ResolvedModelRole {
            provider_id: "provider".to_string(),
            model_record_id: "model-record".to_string(),
            model_id: "model".to_string(),
            model_name: "Test Model".to_string(),
            provider_type: "openai".to_string(),
            provider_name: "Provider".to_string(),
            api_key: None,
            provider_options: None,
            model,
        }
    }
}

#[cfg(test)]
mod has_process_requirements_tests {
    use super::has_process_requirements;

    #[test]
    fn detects_english_keywords() {
        for objective in [
            "Each phase needs a code review before merge.",
            "Verify every change against the spec.",
            "Run a per phase smoke test.",
        ] {
            assert!(
                has_process_requirements(objective),
                "expected keyword match in: {objective}"
            );
        }
    }

    #[test]
    fn detects_cjk_keywords() {
        for objective in [
            "每个阶段都需要验收",
            "每一阶段检查通过",
            "需要你每轮 review",
            "完成所有阶段完成的任务",
        ] {
            assert!(
                has_process_requirements(objective),
                "expected CJK keyword match in: {objective}"
            );
        }
    }

    #[test]
    fn records_substring_match_semantics() {
        // The implementation is a plain case-insensitive substring match
        // over a fixed keyword list. These cases pin down the current
        // behaviour, including the substring-match quirk where "review"
        // hits inside "preview". They are not assertions about an ideal
        // matcher — they are regression guards against accidental keyword
        // list changes. If the keyword list is later tightened, update
        // this test alongside it.
        assert!(
            has_process_requirements("Preview the rendered HTML before shipping."),
            "current implementation matches 'review' inside 'preview' (substring match)"
        );
        assert!(
            !has_process_requirements("Forward-looking design without explicit verify step."),
            "no keyword substring present"
        );
        // "审阅" (look over) is intentionally NOT a keyword — only
        // "验收" (formal acceptance) and "检查" (check) are, so this
        // should be rejected.
        assert!(
            !has_process_requirements("请仔细审阅代码风格。"),
            "审阅 does not contain any current keyword"
        );
        assert!(
            !has_process_requirements("Survey users about preferences."),
            "no keyword substring present"
        );
    }

    #[test]
    fn empty_and_whitespace_objectives_return_false() {
        assert!(!has_process_requirements(""));
        assert!(!has_process_requirements("   \n\t  "));
    }

    #[test]
    fn keyword_match_is_case_insensitive() {
        assert!(has_process_requirements("Final REVIEW before release."));
        assert!(has_process_requirements("Need a Verify Each phase step."));
    }
}

#[cfg(test)]
mod judge_summary_tests {
    //! Integration tests for the two Judge-prompt context builders:
    //!
    //! * [`super::build_task_board_summary`] — surfaces the active task board
    //!   (and its items) for the Judge to cross-check against the goal.
    //! * [`super::build_process_compliance_summary`] — surfaces review
    //!   helper history so the Judge can verify the agent followed the
    //!   process-requirements contract.
    //!
    //! These functions are private and would otherwise only be exercised
    //! through the Judge tool pipeline. The tests use raw SQL seeding on
    //! a SQLite in-memory pool with migrations applied, matching the
    //! pattern used by [`crate::goal_lifecycle`] tests.

    use sqlx::sqlite::{SqliteConnectOptions, SqlitePool, SqlitePoolOptions};
    use std::str::FromStr;

    use super::{build_process_compliance_summary, build_task_board_summary};

    async fn setup_pool() -> SqlitePool {
        let options = SqliteConnectOptions::from_str("sqlite::memory:")
            .unwrap()
            .foreign_keys(true);
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(options)
            .await
            .unwrap();
        crate::persistence::sqlite::run_migrations(&pool)
            .await
            .unwrap();

        let now = chrono::Utc::now().to_rfc3339();
        sqlx::query(
            "INSERT INTO workspaces (id, name, path, canonical_path, display_path,
                    is_default, is_git, auto_work_tree, status, created_at, updated_at)
             VALUES ('ws-test', 'Test Workspace', '/tmp/test', '/tmp/test', '/tmp/test',
                     0, 0, 0, 'ready', ?, ?)",
        )
        .bind(&now)
        .bind(&now)
        .execute(&pool)
        .await
        .expect("failed to seed workspace");

        sqlx::query(
            "INSERT INTO threads (id, workspace_id, title, status, last_active_at, created_at, updated_at)
             VALUES ('thread-1', 'ws-test', 'Test Thread', 'idle', ?, ?, ?)",
        )
        .bind(&now)
        .bind(&now)
        .bind(&now)
        .execute(&pool)
        .await
        .expect("failed to seed thread");

        // run_helpers has a FK to thread_runs; seed one run to satisfy it.
        sqlx::query(
            "INSERT INTO thread_runs (id, thread_id, run_mode, status, started_at, finished_at)
             VALUES ('run-1', 'thread-1', 'default', 'completed', ?, ?)",
        )
        .bind(&now)
        .bind(&now)
        .execute(&pool)
        .await
        .expect("failed to seed run");

        pool
    }

    #[tokio::test]
    async fn task_board_summary_reports_when_no_board_exists() {
        let pool = setup_pool().await;

        let summary = build_task_board_summary(&pool, "thread-1").await;

        assert!(
            summary.contains("No task board exists"),
            "expected absent-board message, got: {summary}"
        );
        assert!(
            summary.contains("file system and goal text"),
            "absent-board summary should nudge Judge to file-system + goal verification, got: {summary}"
        );
    }

    #[tokio::test]
    async fn task_board_summary_renders_active_items_and_skips_abandoned() {
        let pool = setup_pool().await;

        // Two active boards (one with items, one empty) + one abandoned.
        sqlx::query(
            "INSERT INTO task_boards (id, thread_id, title, status, created_at, updated_at)
             VALUES ('board-1', 'thread-1', 'Implement feature', 'active', '2026-01-01T00:00:00.000Z', '2026-01-01T00:00:00.000Z'),
                    ('board-2', 'thread-1', 'Write docs', 'active', '2026-01-02T00:00:00.000Z', '2026-01-02T00:00:00.000Z'),
                    ('board-3', 'thread-1', 'Old attempt', 'abandoned', '2026-01-03T00:00:00.000Z', '2026-01-03T00:00:00.000Z')",
        )
        .execute(&pool)
        .await
        .unwrap();

        sqlx::query(
            "INSERT INTO task_items (id, task_board_id, description, stage, sort_order, created_at, updated_at)
             VALUES ('item-1', 'board-1', 'add API endpoint', 'completed', 0, '2026-01-01T00:00:00.000Z', '2026-01-01T00:00:00.000Z'),
                    ('item-2', 'board-1', 'add CLI wiring', 'in_progress', 1, '2026-01-01T00:00:00.000Z', '2026-01-01T00:00:00.000Z')",
        )
        .execute(&pool)
        .await
        .unwrap();

        let summary = build_task_board_summary(&pool, "thread-1").await;

        // Active boards appear with status.
        assert!(summary.contains("**Implement feature** (status: active)"));
        assert!(summary.contains("**Write docs** (status: active)"));
        // Abandoned boards are filtered out before rendering.
        assert!(
            !summary.contains("Old attempt"),
            "abandoned board should be skipped, got: {summary}"
        );
        // Items render in the requested `stage / description` shape.
        assert!(summary.contains("- [completed] add API endpoint"));
        assert!(summary.contains("- [in_progress] add CLI wiring"));
        // Empty board gets a placeholder.
        assert!(summary.contains("(No task items.)"));
        // Footer reminds the Judge to report non-completed steps as findings.
        assert!(summary.contains("Report these as findings"));
    }

    #[tokio::test]
    async fn process_compliance_summary_reports_when_no_reviews_exist() {
        let pool = setup_pool().await;

        let summary = build_process_compliance_summary(&pool, "thread-1").await;

        assert!(
            summary.contains("No review calls found"),
            "expected absent-review message, got: {summary}"
        );
        assert!(summary.contains("non-compliance"));
    }

    #[tokio::test]
    async fn process_compliance_summary_filters_to_review_helpers_only() {
        let pool = setup_pool().await;

        // Three helpers: one review, one explore, one review with a long
        // input_summary to exercise the 200-char truncation path.
        let long_input = "x".repeat(450);
        sqlx::query(
            "INSERT INTO run_helpers
                (id, run_id, thread_id, helper_kind, status, input_summary, started_at, finished_at)
             VALUES
                ('rh-1', 'run-1', 'thread-1', 'agent_review', 'completed', 'review the diff', '2026-01-01T00:00:00.000Z', '2026-01-01T00:01:00.000Z'),
                ('rh-2', 'run-1', 'thread-1', 'agent_explore', 'completed', 'scan code', '2026-01-01T00:02:00.000Z', '2026-01-01T00:03:00.000Z'),
                ('rh-3', 'run-1', 'thread-1', 'helper_review', 'failed', ?1, '2026-01-01T00:04:00.000Z', '2026-01-01T00:05:00.000Z')",
        )
        .bind(&long_input)
        .execute(&pool)
        .await
        .unwrap();

        let summary = build_process_compliance_summary(&pool, "thread-1").await;

        // Both review helpers are listed.
        assert!(summary.contains("`agent_review`"));
        assert!(summary.contains("`helper_review`"));
        // Non-review helpers are filtered out.
        assert!(
            !summary.contains("`agent_explore`"),
            "non-review helper should be filtered, got: {summary}"
        );
        // Status symbols are mapped to a human label.
        assert!(summary.contains("✓ completed"));
        assert!(summary.contains("✗ failed"));
        // Long input is truncated to 200 chars + ellipsis. We assert the
        // exact trailing shape rather than digging for a specific line:
        // the input was 450 'x' characters, so the rendered Scope line
        // must end with 200 'x' chars followed by "...".
        assert!(
            summary.contains(&format!("{}...", "x".repeat(200))),
            "long input should be truncated to 200 chars + '...'"
        );
    }
}
