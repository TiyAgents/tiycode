use std::sync::atomic::Ordering;

use tiycore::agent::AgentToolResult;
use tiycore::types::{ContentBlock, TextContent};

use crate::core::agent_session_tools::{
    agent_error_result, agent_tool_result_from_output, resolve_helper_model_role,
    resolve_helper_profile, validate_clarify_input,
};
use crate::core::plan_checkpoint::{
    approval_prompt_markdown, build_approval_prompt_metadata, build_plan_artifact_from_tool_input,
    build_plan_message_metadata, plan_markdown, write_plan_file,
};
use crate::core::subagent::{
    extract_review_report, HelperRunRequest, ReviewRequest, RuntimeOrchestrationTool,
};
use crate::core::tool_gateway::{
    ApprovalRequest, ToolExecutionOptions, ToolExecutionRequest, ToolGatewayResult,
};
use crate::ipc::frontend_channels::ThreadStreamEvent;
use crate::model::thread::MessageRecord;
use crate::persistence::repo::{message_repo, tool_call_repo};

use super::agent_session::{standard_tool_timeout, AgentSession, CLARIFY_TOOL_NAME};

#[derive(Debug)]
struct HelperToolTask {
    task: String,
    review_request: Option<ReviewRequest>,
}

fn resolve_helper_tool_task(
    tool: RuntimeOrchestrationTool,
    tool_input: &serde_json::Value,
) -> Result<HelperToolTask, String> {
    if tool == RuntimeOrchestrationTool::Review {
        let request = ReviewRequest::from_tool_input(tool_input)?;
        return Ok(HelperToolTask {
            task: request.to_helper_prompt(),
            review_request: Some(request),
        });
    }

    let task = tool_input
        .get("task")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default()
        .trim()
        .to_string();

    if task.is_empty() {
        return Err("missing helper task".to_string());
    }

    Ok(HelperToolTask {
        task,
        review_request: None,
    })
}

impl AgentSession {
    pub(crate) async fn execute_tool_call(
        &self,
        tool_name: &str,
        tool_call_id: &str,
        tool_input: &serde_json::Value,
    ) -> AgentToolResult {
        if tool_name == "update_plan" {
            return self.execute_plan_checkpoint(tool_input).await;
        }

        if tool_name == "create_task" || tool_name == "update_task" || tool_name == "query_task" {
            return self
                .execute_task_tool(tool_name, tool_call_id, tool_input)
                .await;
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
            return self
                .execute_helper_tool(tool, tool_call_id, &tool_call_storage_id, tool_input)
                .await;
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
        let HelperToolTask {
            task,
            review_request,
        } = match resolve_helper_tool_task(tool, tool_input) {
            Ok(resolved) => resolved,
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

        let helper_role = resolve_helper_model_role(&self.spec.model_plan, tool);
        let helper_profile = resolve_helper_profile(tool);

        let result = self
            .helper_orchestrator
            .run_helper(HelperRunRequest {
                run_id: self.spec.run_id.clone(),
                thread_id: self.spec.thread_id.clone(),
                tool,
                helper_profile: Some(helper_profile),
                parent_tool_call_id: Some(tool_call_id.to_string()),
                task: task.clone(),
                model_role: helper_role,
                system_prompt: self.spec.system_prompt.clone(),
                workspace_path: self.spec.workspace_path.clone(),
                run_mode: self.spec.run_mode.clone(),
                event_tx: self.event_tx.clone(),
                session_abort_signal: self.abort_signal.clone(),
                thinking_level: self.spec.model_plan.thinking_level,
            })
            .await;

        match result {
            Ok(summary) => {
                let review_report = if tool == RuntimeOrchestrationTool::Review {
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
                    "reviewRequest": review_request,
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
                "update_plan requires a non-empty title that describes the change (not the default placeholder).",
            );
        }
        if artifact.context.trim().is_empty() {
            return agent_error_result(
                "update_plan requires a non-empty context section. Describe the current state of the relevant code with inspected file paths and confirmed facts.",
            );
        }
        if artifact.design.trim().is_empty() {
            return agent_error_result(
                "update_plan requires a non-empty design section. Explain the recommended approach, architecture changes, data flow, and tradeoffs.",
            );
        }

        let plan_message_id = uuid::Uuid::now_v7().to_string();
        let approval_message_id = uuid::Uuid::now_v7().to_string();
        let plan_metadata =
            build_plan_message_metadata(artifact.clone(), &self.spec.run_id, &self.spec.run_mode);
        let approval_metadata =
            build_approval_prompt_metadata(artifact.plan_revision, &plan_message_id);

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
        self.abort_signal.cancel();
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
}

#[cfg(test)]
mod tests {
    use super::{resolve_helper_tool_task, RuntimeOrchestrationTool};
    use crate::core::subagent::review_contract::{GlobalScanMode, ReviewScope, ReviewTarget};

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
}
