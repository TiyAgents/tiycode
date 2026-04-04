use base64::{engine::general_purpose, Engine as _};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex as StdMutex, Weak};

use sqlx::SqlitePool;
use tiy_core::agent::{
    Agent, AgentError, AgentMessage, AgentTool, AgentToolResult, ToolExecutionMode,
};
use tiy_core::thinking::ThinkingLevel;
use tiy_core::types::{
    AssistantMessage, AssistantMessageEvent, ContentBlock, Cost, ImageContent, InputType, Model,
    Provider, StopReason, TextContent, Transport, Usage, UserMessage,
};
use tokio::sync::mpsc;

use crate::core::plan_checkpoint::{
    approval_prompt_markdown, build_approval_prompt_metadata, build_plan_artifact_from_tool_input,
    build_plan_message_metadata, parse_plan_message_metadata, plan_markdown,
};
use crate::core::prompt;
use crate::core::subagent::{
    runtime_orchestration_tools, HelperAgentOrchestrator, HelperRunRequest,
    RuntimeOrchestrationTool, SubagentProfile, TERM_CLOSE_TOOL_DESCRIPTION,
    TERM_OUTPUT_TOOL_DESCRIPTION, TERM_RESTART_TOOL_DESCRIPTION, TERM_STATUS_TOOL_DESCRIPTION,
    TERM_WRITE_TOOL_DESCRIPTION,
};
use crate::core::tool_gateway::{
    ApprovalRequest, ToolExecutionOptions, ToolExecutionRequest, ToolGateway, ToolGatewayResult,
};
use crate::ipc::frontend_channels::ThreadStreamEvent;
use crate::model::errors::{AppError, ErrorSource};
use crate::model::provider::AgentProfileRecord;
use crate::model::thread::{MessageAttachmentDto, MessageRecord, RunUsageDto};
use crate::persistence::repo::{message_repo, provider_repo, tool_call_repo};

const MESSAGE_HISTORY_LIMIT: i64 = 200;
const DEFAULT_CONTEXT_WINDOW: u32 = 128_000;
const DEFAULT_MAX_OUTPUT_TOKENS: u32 = 32_000;
const DEFAULT_FULL_TOOL_PROFILE: &str = "default_full";
const PLAN_READ_ONLY_TOOL_PROFILE: &str = "plan_read_only";
const STANDARD_TOOL_TIMEOUT_SECS: u64 = 120;
const SUBAGENT_TOOL_TIMEOUT_SECS: u64 = 600;
const CLARIFY_TOOL_NAME: &str = "clarify";
const PLAN_MODE_MISSING_CHECKPOINT_ERROR: &str =
    "Plan mode requires publishing a plan with update_plan before the run can finish.";
const TEXT_ATTACHMENT_MAX_CHARS: usize = 12_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProfileResponseStyle {
    Balanced,
    Concise,
    Guide,
}

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeModelRole {
    pub provider_id: String,
    pub model_record_id: String,
    pub provider: Option<String>,
    pub provider_key: Option<String>,
    pub provider_type: String,
    pub provider_name: Option<String>,
    pub model: String,
    pub model_id: String,
    pub model_display_name: Option<String>,
    pub base_url: String,
    pub context_window: Option<String>,
    pub max_output_tokens: Option<String>,
    pub supports_image_input: Option<bool>,
    pub custom_headers: Option<HashMap<String, String>>,
    pub provider_options: Option<serde_json::Value>,
}

#[derive(Debug, Clone, serde::Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeModelPlan {
    pub profile_id: Option<String>,
    pub profile_name: Option<String>,
    pub custom_instructions: Option<String>,
    pub response_style: Option<String>,
    pub response_language: Option<String>,
    pub primary: Option<RuntimeModelRole>,
    pub auxiliary: Option<RuntimeModelRole>,
    pub lightweight: Option<RuntimeModelRole>,
    pub thinking_level: Option<String>,
    pub transport: Option<String>,
    pub tool_profile_by_mode: Option<serde_json::Value>,
}

#[derive(Debug, Clone)]
pub struct ResolvedModelRole {
    pub provider_id: String,
    pub model_record_id: String,
    pub model_id: String,
    pub model_name: String,
    pub provider_type: String,
    pub provider_name: String,
    pub api_key: Option<String>,
    pub provider_options: Option<serde_json::Value>,
    pub model: Model,
}

#[derive(Debug, Clone)]
pub struct ResolvedRuntimeModelPlan {
    pub raw: RuntimeModelPlan,
    pub primary: ResolvedModelRole,
    pub auxiliary: Option<ResolvedModelRole>,
    pub lightweight: Option<ResolvedModelRole>,
    pub thinking_level: ThinkingLevel,
    pub transport: Transport,
}

#[derive(Debug, Clone)]
pub struct AgentSessionSpec {
    pub run_id: String,
    pub thread_id: String,
    pub workspace_path: String,
    pub run_mode: String,
    pub tool_profile_name: String,
    pub system_prompt: String,
    pub history_messages: Vec<MessageRecord>,
    pub model_plan: ResolvedRuntimeModelPlan,
    pub initial_prompt: Option<String>,
}

pub async fn build_session_spec(
    pool: &SqlitePool,
    run_id: &str,
    thread_id: &str,
    workspace_path: &str,
    run_mode: &str,
    model_plan_value: &serde_json::Value,
) -> Result<AgentSessionSpec, AppError> {
    let raw_plan: RuntimeModelPlan =
        serde_json::from_value(model_plan_value.clone()).unwrap_or_default();
    let resolved_plan = resolve_model_plan(pool, raw_plan.clone()).await?;
    let recent_messages =
        message_repo::list_recent(pool, thread_id, None, MESSAGE_HISTORY_LIMIT).await?;
    let history_messages = trim_history_to_current_context(&recent_messages);
    let system_prompt = build_system_prompt(pool, &raw_plan, workspace_path, run_mode).await?;

    Ok(AgentSessionSpec {
        run_id: run_id.to_string(),
        thread_id: thread_id.to_string(),
        workspace_path: workspace_path.to_string(),
        run_mode: run_mode.to_string(),
        tool_profile_name: resolve_tool_profile_name(&raw_plan, run_mode),
        system_prompt,
        history_messages,
        model_plan: resolved_plan,
        initial_prompt: None,
    })
}

pub(crate) fn trim_history_to_current_context(messages: &[MessageRecord]) -> Vec<MessageRecord> {
    let start_index = messages
        .iter()
        .rposition(is_context_reset_marker)
        .map(|index| index + 1)
        .unwrap_or(0);

    messages[start_index..]
        .iter()
        .filter(|message| message.status != "discarded")
        .cloned()
        .collect()
}

pub struct AgentSession {
    spec: AgentSessionSpec,
    pool: SqlitePool,
    tool_gateway: Arc<ToolGateway>,
    helper_orchestrator: Arc<HelperAgentOrchestrator>,
    event_tx: mpsc::UnboundedSender<ThreadStreamEvent>,
    agent: Arc<Agent>,
    cancel_requested: Arc<AtomicBool>,
    checkpoint_requested: AtomicBool,
    abort_signal: tiy_core::agent::AbortSignal,
}

impl AgentSession {
    pub fn new(
        pool: SqlitePool,
        tool_gateway: Arc<ToolGateway>,
        helper_orchestrator: Arc<HelperAgentOrchestrator>,
        event_tx: mpsc::UnboundedSender<ThreadStreamEvent>,
        spec: AgentSessionSpec,
    ) -> Arc<Self> {
        Arc::new_cyclic(|weak_self| {
            let agent = Arc::new(Agent::with_model(spec.model_plan.primary.model.clone()));
            agent.set_max_turns(crate::desktop_agent_max_turns!());
            configure_agent(&agent, &spec, weak_self.clone());

            Self {
                spec,
                pool,
                tool_gateway,
                helper_orchestrator,
                event_tx,
                agent,
                cancel_requested: Arc::new(AtomicBool::new(false)),
                checkpoint_requested: AtomicBool::new(false),
                abort_signal: tiy_core::agent::AbortSignal::new(),
            }
        })
    }

    pub fn start(self: Arc<Self>) {
        tokio::spawn(async move {
            self.run().await;
        });
    }

    pub async fn cancel(&self) {
        self.cancel_requested.store(true, Ordering::SeqCst);
        self.abort_signal.cancel();
        self.helper_orchestrator.cancel_run(&self.spec.run_id).await;
        self.agent.abort();
    }

    async fn run(self: Arc<Self>) {
        let current_message_id = Arc::new(StdMutex::new(None::<String>));
        let last_completed_message_id = Arc::new(StdMutex::new(None::<String>));
        let current_reasoning_message_id = Arc::new(StdMutex::new(None::<String>));
        let last_usage = Arc::new(StdMutex::new(None::<Usage>));
        let reasoning_buffer = Arc::new(StdMutex::new(String::new()));
        let run_id = self.spec.run_id.clone();
        let event_tx = self.event_tx.clone();
        let context_window = self
            .spec
            .model_plan
            .primary
            .model
            .context_window
            .to_string();
        let model_display_name = self.spec.model_plan.primary.model_name.clone();

        let message_id_ref = Arc::clone(&current_message_id);
        let last_completed_message_id_ref = Arc::clone(&last_completed_message_id);
        let reasoning_message_id_ref = Arc::clone(&current_reasoning_message_id);
        let last_usage_ref = Arc::clone(&last_usage);
        let reasoning_ref = Arc::clone(&reasoning_buffer);
        let unsubscribe = self.agent.subscribe(move |event| {
            handle_agent_event(
                &run_id,
                &event_tx,
                &message_id_ref,
                &last_completed_message_id_ref,
                &reasoning_message_id_ref,
                &last_usage_ref,
                &reasoning_ref,
                &context_window,
                &model_display_name,
                event,
            );
        });

        let _ = self.event_tx.send(ThreadStreamEvent::RunStarted {
            run_id: self.spec.run_id.clone(),
            run_mode: self.spec.run_mode.clone(),
        });

        let result = if let Some(prompt) = self.spec.initial_prompt.clone() {
            self.agent.prompt(prompt).await
        } else {
            self.agent.continue_().await
        };
        unsubscribe();

        match result {
            Ok(_) => {
                if self.cancel_requested.load(Ordering::SeqCst) {
                    let _ = self.event_tx.send(ThreadStreamEvent::RunCancelled {
                        run_id: self.spec.run_id.clone(),
                    });
                } else if self.checkpoint_requested.load(Ordering::SeqCst) {
                    let _ = self.event_tx.send(ThreadStreamEvent::RunCheckpointed {
                        run_id: self.spec.run_id.clone(),
                    });
                } else if let Some(error) = plan_mode_missing_checkpoint_error(
                    &self.spec.run_mode,
                    self.checkpoint_requested.load(Ordering::SeqCst),
                ) {
                    let _ = self.event_tx.send(ThreadStreamEvent::RunFailed {
                        run_id: self.spec.run_id.clone(),
                        error: error.to_string(),
                    });
                } else {
                    let _ = self.event_tx.send(ThreadStreamEvent::RunCompleted {
                        run_id: self.spec.run_id.clone(),
                    });
                }
            }
            Err(error) => {
                if self.cancel_requested.load(Ordering::SeqCst) {
                    let _ = self.event_tx.send(ThreadStreamEvent::RunCancelled {
                        run_id: self.spec.run_id.clone(),
                    });
                } else if self.checkpoint_requested.load(Ordering::SeqCst) {
                    let _ = self.event_tx.send(ThreadStreamEvent::RunCheckpointed {
                        run_id: self.spec.run_id.clone(),
                    });
                } else if let AgentError::MaxTurnsReached(max_turns) = &error {
                    let _ = self.event_tx.send(ThreadStreamEvent::RunLimitReached {
                        run_id: self.spec.run_id.clone(),
                        error: error.to_string(),
                        max_turns: *max_turns,
                    });
                } else {
                    let _ = self.event_tx.send(ThreadStreamEvent::RunFailed {
                        run_id: self.spec.run_id.clone(),
                        error: error.to_string(),
                    });
                }
            }
        }
    }

    async fn execute_tool_call(
        &self,
        tool_name: &str,
        tool_call_id: &str,
        tool_input: &serde_json::Value,
    ) -> AgentToolResult {
        if tool_name == "update_plan" {
            return self.execute_plan_checkpoint(tool_input).await;
        }

        if tool_name == "create_task" || tool_name == "update_task" {
            return self
                .execute_task_tool(tool_name, tool_call_id, tool_input)
                .await;
        }

        if tool_name == CLARIFY_TOOL_NAME {
            return self
                .execute_clarify_request(tool_name, tool_call_id, tool_input)
                .await;
        }

        let insert_result = tool_call_repo::insert(
            &self.pool,
            &tool_call_repo::ToolCallInsert {
                id: tool_call_id.to_string(),
                run_id: self.spec.run_id.clone(),
                thread_id: self.spec.thread_id.clone(),
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
                .execute_helper_tool(tool, tool_call_id, tool_input)
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
            tool_name: tool_name.to_string(),
            tool_input: tool_input.clone(),
            workspace_path: self.spec.workspace_path.clone(),
            run_mode: self.spec.run_mode.clone(),
        };

        let event_tx = self.event_tx.clone();
        let run_id = self.spec.run_id.clone();
        let tool_call_id_owned = tool_call_id.to_string();
        let tool_timeout = standard_tool_timeout();
        let outcome = tokio::time::timeout(
            tool_timeout,
            self.tool_gateway.execute_tool_call(
                request,
                self.abort_signal.clone(),
                ToolExecutionOptions::default(),
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
            ),
        )
        .await;

        let outcome = match outcome {
            Ok(outcome) => outcome,
            Err(_) => {
                let message = format!(
                    "Tool '{}' timed out after {}s",
                    tool_name, STANDARD_TOOL_TIMEOUT_SECS
                );
                let result = serde_json::json!({ "error": message.clone() });
                tool_call_repo::update_result(
                    &self.pool,
                    tool_call_id,
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

                return agent_error_result(message);
            }
        };

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

        if let Err(error) = tool_call_repo::insert(
            &self.pool,
            &tool_call_repo::ToolCallInsert {
                id: tool_call_id.to_string(),
                run_id: self.spec.run_id.clone(),
                thread_id: self.spec.thread_id.clone(),
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
        tool_input: &serde_json::Value,
    ) -> AgentToolResult {
        let task = tool_input
            .get("task")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .trim()
            .to_string();

        if task.is_empty() {
            tool_call_repo::update_result(
                &self.pool,
                tool_call_id,
                &serde_json::json!({ "error": "missing helper task" }).to_string(),
                "failed",
            )
            .await
            .ok();
            return agent_error_result("missing helper task");
        }

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
            })
            .await;

        match result {
            Ok(summary) => {
                let result = serde_json::json!({
                    "summary": summary.summary.clone(),
                    "snapshot": summary.snapshot,
                });
                tool_call_repo::update_result(
                    &self.pool,
                    tool_call_id,
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
                    tool_call_id,
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

        self.checkpoint_requested.store(true, Ordering::SeqCst);
        self.abort_signal.cancel();
        self.agent.abort();

        AgentToolResult {
            content: vec![ContentBlock::Text(TextContent::new(
                "Implementation plan published. Waiting for approval before execution.",
            ))],
            details: Some(serde_json::json!({
                "planMessageId": plan_message_id,
                "approvalMessageId": approval_message_id,
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
        use crate::model::task_board::{CreateTaskInput, UpdateTaskInput};

        // Persist the tool call record
        if let Err(error) = tool_call_repo::insert(
            &self.pool,
            &tool_call_repo::ToolCallInsert {
                id: tool_call_id.to_string(),
                run_id: self.spec.run_id.clone(),
                thread_id: self.spec.thread_id.clone(),
                tool_name: tool_name.to_string(),
                tool_input_json: tool_input.to_string(),
                status: "running".to_string(),
            },
        )
        .await
        {
            return agent_error_result(format!("failed to persist tool call: {error}"));
        }

        let _ = self.event_tx.send(ThreadStreamEvent::ToolRunning {
            run_id: self.spec.run_id.clone(),
            tool_call_id: tool_call_id.to_string(),
        });

        let result = if tool_name == "create_task" {
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
                            Ok(dto)
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
                            Ok(dto)
                        }
                        Err(e) => Err(e.to_string()),
                    }
                }
                Err(e) => Err(format!("Invalid update_task input: {}", e)),
            }
        } else {
            Err(format!("Unknown task tool: {}", tool_name))
        };

        match result {
            Ok(dto) => {
                let result_json = serde_json::to_value(&dto).unwrap_or(serde_json::json!({}));
                tool_call_repo::update_result(
                    &self.pool,
                    tool_call_id,
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
                        serde_json::to_string(&dto)
                            .unwrap_or_else(|_| "Task updated successfully".to_string()),
                    ))],
                    details: Some(result_json),
                }
            }
            Err(error) => {
                let error_json = serde_json::json!({ "error": &error });
                tool_call_repo::update_result(
                    &self.pool,
                    tool_call_id,
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

    async fn next_plan_revision(&self) -> Result<u32, AppError> {
        let messages =
            message_repo::list_recent(&self.pool, &self.spec.thread_id, None, 256).await?;
        let next = messages
            .iter()
            .filter(|message| message.message_type == "plan")
            .filter_map(|message| {
                message
                    .metadata_json
                    .as_deref()
                    .and_then(|raw| serde_json::from_str::<serde_json::Value>(raw).ok())
                    .and_then(|value| {
                        value
                            .get("planRevision")
                            .and_then(serde_json::Value::as_u64)
                            .and_then(|value| u32::try_from(value).ok())
                    })
            })
            .max()
            .unwrap_or(0);
        Ok(next.saturating_add(1))
    }
}

fn configure_agent(agent: &Arc<Agent>, spec: &AgentSessionSpec, weak_self: Weak<AgentSession>) {
    agent.set_system_prompt(spec.system_prompt.clone());
    agent.replace_messages(convert_history_messages(
        &spec.history_messages,
        &spec.model_plan.primary.model,
    ));
    agent.set_tools(runtime_tools_for_profile(&spec.tool_profile_name));
    agent.set_tool_execution(ToolExecutionMode::Sequential);
    agent.set_thinking_level(spec.model_plan.thinking_level);
    agent.set_transport(spec.model_plan.transport);
    agent.set_security_config(runtime_security_config());

    // Context compression: automatically trim messages to fit the context window.
    let compression_settings = crate::core::context_compression::CompressionSettings::new(
        spec.model_plan.primary.model.context_window,
    );
    agent.set_transform_context(move |messages| {
        let settings = compression_settings.clone();
        async move { crate::core::context_compression::compress_context(messages, &settings) }
    });

    if let Some(api_key) = spec.model_plan.primary.api_key.clone() {
        agent.set_api_key(api_key);
    }

    if let Some(provider_options) = spec.model_plan.primary.provider_options.clone() {
        agent.set_on_payload(move |payload, _model| {
            let provider_options = provider_options.clone();
            Box::pin(async move { Some(merge_payload(payload, &provider_options)) })
        });
    }

    agent.set_tool_executor(move |tool_name, tool_call_id, tool_input, _update_cb| {
        let weak_self = weak_self.clone();
        let tool_name = tool_name.to_string();
        let tool_call_id = tool_call_id.to_string();
        let tool_input = tool_input.clone();

        async move {
            match weak_self.upgrade() {
                Some(session) => {
                    session
                        .execute_tool_call(&tool_name, &tool_call_id, &tool_input)
                        .await
                }
                None => agent_error_result("agent session unavailable"),
            }
        }
    });
}

fn handle_agent_event(
    run_id: &str,
    event_tx: &mpsc::UnboundedSender<ThreadStreamEvent>,
    current_message_id: &StdMutex<Option<String>>,
    last_completed_message_id: &StdMutex<Option<String>>,
    current_reasoning_message_id: &StdMutex<Option<String>>,
    last_usage: &StdMutex<Option<Usage>>,
    reasoning_buffer: &StdMutex<String>,
    context_window: &str,
    model_display_name: &str,
    event: &tiy_core::agent::AgentEvent,
) {
    match event {
        tiy_core::agent::AgentEvent::TurnRetrying {
            attempt,
            max_attempts,
            delay_ms,
            reason,
        } => {
            let _ = event_tx.send(ThreadStreamEvent::RunRetrying {
                run_id: run_id.to_string(),
                attempt: *attempt,
                max_attempts: *max_attempts,
                delay_ms: *delay_ms,
                reason: reason.clone(),
            });
        }
        tiy_core::agent::AgentEvent::MessageUpdate {
            assistant_event, ..
        } => {
            match assistant_event.as_ref() {
                AssistantMessageEvent::TextDelta { delta, .. } => {
                    let message_id = ensure_message_id(current_message_id);
                    let _ = event_tx.send(ThreadStreamEvent::MessageDelta {
                        run_id: run_id.to_string(),
                        message_id,
                        delta: delta.clone(),
                    });
                }
                AssistantMessageEvent::ThinkingStart { .. } => {
                    reset_reasoning_state(current_reasoning_message_id, reasoning_buffer);
                }
                AssistantMessageEvent::ThinkingDelta { delta, .. } => {
                    if let Ok(mut buffer) = reasoning_buffer.lock() {
                        buffer.push_str(delta);
                        let message_id = ensure_message_id(current_reasoning_message_id);
                        let _ = event_tx.send(ThreadStreamEvent::ReasoningUpdated {
                            run_id: run_id.to_string(),
                            message_id,
                            reasoning: buffer.clone(),
                        });
                    }
                }
                AssistantMessageEvent::ThinkingEnd { content, .. } => {
                    let reasoning = if let Ok(mut buffer) = reasoning_buffer.lock() {
                        buffer.clear();
                        buffer.push_str(content);
                        buffer.clone()
                    } else {
                        content.clone()
                    };

                    if reasoning.trim().is_empty() {
                        reset_reasoning_state(current_reasoning_message_id, reasoning_buffer);
                        return;
                    }

                    let message_id = ensure_message_id(current_reasoning_message_id);
                    let _ = event_tx.send(ThreadStreamEvent::ReasoningUpdated {
                        run_id: run_id.to_string(),
                        message_id,
                        reasoning,
                    });
                    reset_reasoning_state(current_reasoning_message_id, reasoning_buffer);
                }
                _ => {}
            }

            if let Some(partial) = assistant_event.partial_message() {
                emit_usage_update_if_changed(
                    run_id,
                    event_tx,
                    last_usage,
                    &partial.usage,
                    context_window,
                    model_display_name,
                );
            }
        }
        tiy_core::agent::AgentEvent::MessageEnd { message } => {
            if let AgentMessage::Assistant(assistant) = message {
                let content = assistant.text_content();
                if content.is_empty() && assistant.has_tool_calls() {
                    emit_usage_update_if_changed(
                        run_id,
                        event_tx,
                        last_usage,
                        &assistant.usage,
                        context_window,
                        model_display_name,
                    );
                    reset_message_id(current_message_id);
                    reset_reasoning_state(current_reasoning_message_id, reasoning_buffer);
                    return;
                }

                emit_usage_update_if_changed(
                    run_id,
                    event_tx,
                    last_usage,
                    &assistant.usage,
                    context_window,
                    model_display_name,
                );
                let message_id = take_or_create_message_id(current_message_id);
                set_last_completed_message_id(last_completed_message_id, Some(message_id.clone()));
                let _ = event_tx.send(ThreadStreamEvent::MessageCompleted {
                    run_id: run_id.to_string(),
                    message_id,
                    content,
                });
            }

            reset_reasoning_state(current_reasoning_message_id, reasoning_buffer);
        }
        tiy_core::agent::AgentEvent::MessageDiscarded { reason, .. } => {
            if let Some(message_id) = read_last_completed_message_id(last_completed_message_id) {
                let _ = event_tx.send(ThreadStreamEvent::MessageDiscarded {
                    run_id: run_id.to_string(),
                    message_id,
                    reason: reason.clone(),
                });
            }
        }
        _ => {}
    }
}

fn emit_usage_update_if_changed(
    run_id: &str,
    event_tx: &mpsc::UnboundedSender<ThreadStreamEvent>,
    last_usage: &StdMutex<Option<Usage>>,
    usage: &Usage,
    context_window: &str,
    model_display_name: &str,
) {
    let should_emit = if let Ok(mut previous_usage) = last_usage.lock() {
        if previous_usage.as_ref() == Some(usage) {
            return;
        }

        if usage.total_tokens == 0
            && usage.input == 0
            && usage.output == 0
            && usage.cache_read == 0
            && usage.cache_write == 0
        {
            return;
        }

        *previous_usage = Some(*usage);
        true
    } else {
        usage.total_tokens > 0
            || usage.input > 0
            || usage.output > 0
            || usage.cache_read > 0
            || usage.cache_write > 0
    };

    if !should_emit {
        return;
    }

    let _ = event_tx.send(ThreadStreamEvent::ThreadUsageUpdated {
        run_id: run_id.to_string(),
        model_display_name: Some(model_display_name.to_string()),
        context_window: Some(context_window.to_string()),
        usage: RunUsageDto::from(*usage),
    });
}

fn ensure_message_id(current_message_id: &StdMutex<Option<String>>) -> String {
    if let Ok(mut guard) = current_message_id.lock() {
        if let Some(existing) = guard.clone() {
            return existing;
        }

        let message_id = uuid::Uuid::now_v7().to_string();
        *guard = Some(message_id.clone());
        return message_id;
    }

    uuid::Uuid::now_v7().to_string()
}

fn take_or_create_message_id(current_message_id: &StdMutex<Option<String>>) -> String {
    if let Ok(mut guard) = current_message_id.lock() {
        if let Some(existing) = guard.take() {
            return existing;
        }
    }

    uuid::Uuid::now_v7().to_string()
}

fn reset_message_id(current_message_id: &StdMutex<Option<String>>) {
    if let Ok(mut guard) = current_message_id.lock() {
        *guard = None;
    }
}

fn set_last_completed_message_id(
    last_completed_message_id: &StdMutex<Option<String>>,
    value: Option<String>,
) {
    if let Ok(mut guard) = last_completed_message_id.lock() {
        *guard = value;
    }
}

fn read_last_completed_message_id(
    last_completed_message_id: &StdMutex<Option<String>>,
) -> Option<String> {
    last_completed_message_id
        .lock()
        .ok()
        .and_then(|guard| guard.clone())
}

fn reset_reasoning_state(
    current_reasoning_message_id: &StdMutex<Option<String>>,
    reasoning_buffer: &StdMutex<String>,
) {
    reset_message_id(current_reasoning_message_id);
    if let Ok(mut buffer) = reasoning_buffer.lock() {
        buffer.clear();
    }
}

fn runtime_tools_for_profile(profile_name: &str) -> Vec<AgentTool> {
    let mut tools = vec![
        AgentTool::new(
            "read",
            "Read File",
            "Read a file inside the current workspace. Supports optional offset/limit windowing for large files and returns a truncated preview when the selected range exceeds safety limits.",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string" },
                    "offset": {
                        "type": "integer",
                        "description": "Optional 1-indexed line number to start reading from."
                    },
                    "limit": {
                        "type": "integer",
                        "description": "Optional maximum number of lines to read from the offset."
                    }
                },
                "required": ["path"]
            }),
        ),
        AgentTool::new(
            "list",
            "List Directory",
            "List files and folders inside the current workspace. Supports an optional preview limit for large directories.",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string" },
                    "limit": {
                        "type": "integer",
                        "description": "Optional maximum number of entries to return. Defaults to 500 and is capped for safety."
                    }
                }
            }),
        ),
        AgentTool::new(
            "search",
            "Search Repo",
            "Search the current workspace with ripgrep. Results are preview-limited for safety; omit wildcard-only filePattern values like '*' or '**/*'.",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "query": {
                        "type": "string",
                        "description": "Search term or regex."
                    },
                    "directory": {
                        "type": "string",
                        "description": "Directory to search in (default: workspace root)."
                    },
                    "filePattern": {
                        "type": "string",
                        "description": "Optional glob filter such as '*.rs' or 'src/**/*.ts'. Omit it to search all files; do not pass '*' or '**/*'."
                    },
                    "maxResults": {
                        "type": "integer",
                        "description": "Optional preview limit for returned matches. Defaults to 100 and is capped for context safety."
                    }
                },
                "required": ["query"]
            }),
        ),
        AgentTool::new(
            "find",
            "Find Files",
            "Search for files by glob pattern. Returns matching file paths relative to the workspace. Respects common ignore patterns (.git, node_modules, target). Supports an optional preview limit and truncates output to 1000 results or 100KB.",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "pattern": {
                        "type": "string",
                        "description": "Glob pattern to match files, e.g. '*.ts', '*.json', '*.spec.ts'"
                    },
                    "path": {
                        "type": "string",
                        "description": "Directory to search in (default: workspace root)"
                    },
                    "limit": {
                        "type": "integer",
                        "description": "Optional maximum number of matches to preview. Defaults to 1000 and is capped for safety."
                    }
                },
                "required": ["pattern"]
            }),
        ),
        AgentTool::new(
            "term_status",
            "Terminal Status",
            TERM_STATUS_TOOL_DESCRIPTION,
            serde_json::json!({
                "type": "object",
                "properties": {}
            }),
        ),
        AgentTool::new(
            "term_output",
            "Terminal Output",
            TERM_OUTPUT_TOOL_DESCRIPTION,
            serde_json::json!({
                "type": "object",
                "properties": {}
            }),
        ),
        AgentTool::new(
            CLARIFY_TOOL_NAME,
            "Clarify",
            "Ask the user one concise question when they need to choose between reasonable options, confirm a preference, approve a risky action, define scope, or provide missing requirements before you continue. Prefer this tool over guessing when multiple valid paths exist. Offer 2-5 short options when possible, mark the recommended option, and keep the wording brief.",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "header": {
                        "type": "string",
                        "description": "Optional short label for the UI, ideally 12 characters or fewer."
                    },
                    "question": {
                        "type": "string",
                        "description": "A single concise question for the user."
                    },
                    "options": {
                        "type": "array",
                        "minItems": 2,
                        "maxItems": 5,
                        "items": {
                            "type": "object",
                            "properties": {
                                "id": { "type": "string" },
                                "label": { "type": "string" },
                                "description": { "type": "string" },
                                "recommended": { "type": "boolean" }
                            },
                            "required": ["label", "description"]
                        }
                    }
                },
                "required": ["question", "options"]
            }),
        ),
        AgentTool::new(
            "update_plan",
            "Update Plan",
            "Publish the current implementation plan and pause before execution. Use this when the main agent has enough context to present a concrete pre-implementation plan for user approval.",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "title": { "type": "string" },
                    "summary": { "type": "string" },
                    "context": {
                        "oneOf": [
                            { "type": "string" },
                            {
                                "type": "array",
                                "items": { "type": "string" }
                            }
                        ]
                    },
                    "design": {
                        "oneOf": [
                            { "type": "string" },
                            {
                                "type": "array",
                                "items": { "type": "string" }
                            }
                        ]
                    },
                    "keyImplementation": {
                        "oneOf": [
                            { "type": "string" },
                            {
                                "type": "array",
                                "items": { "type": "string" }
                            }
                        ]
                    },
                    "steps": {
                        "type": "array",
                        "items": {
                            "oneOf": [
                                { "type": "string" },
                                {
                                    "type": "object",
                                    "properties": {
                                        "id": { "type": "string" },
                                        "title": { "type": "string" },
                                        "description": { "type": "string" },
                                        "status": { "type": "string" },
                                        "files": {
                                            "type": "array",
                                            "items": { "type": "string" }
                                        }
                                    }
                                }
                            ]
                        }
                    },
                    "verification": {
                        "oneOf": [
                            { "type": "string" },
                            {
                                "type": "array",
                                "items": { "type": "string" }
                            }
                        ]
                    },
                    "risks": {
                        "type": "array",
                        "items": { "type": "string" }
                    },
                    "assumptions": {
                        "oneOf": [
                            { "type": "string" },
                            {
                                "type": "array",
                                "items": { "type": "string" }
                            }
                        ]
                    },
                    "needsContextResetOption": { "type": "boolean" },
                    "plan": {
                        "type": "object",
                        "description": "Optional nested plan payload. If provided, the runtime reads planning fields from this object.",
                        "properties": {
                            "title": { "type": "string" },
                            "summary": { "type": "string" },
                            "context": {
                                "oneOf": [
                                    { "type": "string" },
                                    {
                                        "type": "array",
                                        "items": { "type": "string" }
                                    }
                                ]
                            },
                            "design": {
                                "oneOf": [
                                    { "type": "string" },
                                    {
                                        "type": "array",
                                        "items": { "type": "string" }
                                    }
                                ]
                            },
                            "keyImplementation": {
                                "oneOf": [
                                    { "type": "string" },
                                    {
                                        "type": "array",
                                        "items": { "type": "string" }
                                    }
                                ]
                            },
                            "steps": {
                                "type": "array",
                                "items": {
                                    "oneOf": [
                                        { "type": "string" },
                                        {
                                            "type": "object",
                                            "properties": {
                                                "id": { "type": "string" },
                                                "title": { "type": "string" },
                                                "description": { "type": "string" },
                                                "status": { "type": "string" },
                                                "files": {
                                                    "type": "array",
                                                    "items": { "type": "string" }
                                                }
                                            }
                                        }
                                    ]
                                }
                            },
                            "verification": {
                                "oneOf": [
                                    { "type": "string" },
                                    {
                                        "type": "array",
                                        "items": { "type": "string" }
                                    }
                                ]
                            },
                            "risks": {
                                "type": "array",
                                "items": { "type": "string" }
                            },
                            "assumptions": {
                                "oneOf": [
                                    { "type": "string" },
                                    {
                                        "type": "array",
                                        "items": { "type": "string" }
                                    }
                                ]
                            },
                            "needsContextResetOption": { "type": "boolean" }
                        }
                    }
                }
            }),
        ),
    ];
    tools.extend(runtime_orchestration_tools());

    if profile_name == DEFAULT_FULL_TOOL_PROFILE {
        tools.push(AgentTool::new(
            "edit",
            "Edit File",
            "Make a targeted edit to a file by specifying the exact text to find and its replacement. \
             The old_string must uniquely identify the text to replace (appear exactly once in the file). \
             Include enough surrounding context in old_string to make it unique. \
             If old_string is empty, a new file will be created with new_string as content. \
             Supports fuzzy matching for trailing whitespace and Unicode quote/dash differences.",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "Absolute path to the file to edit"
                    },
                    "old_string": {
                        "type": "string",
                        "description": "The exact text to find and replace. Must match exactly once in the file. Use empty string to create a new file."
                    },
                    "new_string": {
                        "type": "string",
                        "description": "The replacement text"
                    }
                },
                "required": ["path", "old_string", "new_string"]
            }),
        ));
        tools.push(AgentTool::new(
            "write",
            "Write File",
            "Write or overwrite a file inside the current workspace.",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string" },
                    "content": { "type": "string" }
                },
                "required": ["path", "content"]
            }),
        ));
        tools.push(AgentTool::new(
            "shell",
            "Run Command",
            "Run a non-interactive shell command inside the current workspace.",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "command": { "type": "string" },
                    "cwd": { "type": "string" },
                    "timeout": { "type": "number" }
                },
                "required": ["command"]
            }),
        ));
        tools.push(AgentTool::new(
            "term_write",
            "Terminal Write",
            TERM_WRITE_TOOL_DESCRIPTION,
            serde_json::json!({
                "type": "object",
                "properties": {
                    "data": {
                        "type": "string",
                        "description": "Input to send to the current thread's Terminal panel session."
                    }
                },
                "required": ["data"]
            }),
        ));
        tools.push(AgentTool::new(
            "term_restart",
            "Terminal Restart",
            TERM_RESTART_TOOL_DESCRIPTION,
            serde_json::json!({
                "type": "object",
                "properties": {
                    "cols": {
                        "type": "integer",
                        "description": "Optional terminal width in columns for the restarted Terminal panel session."
                    },
                    "rows": {
                        "type": "integer",
                        "description": "Optional terminal height in rows for the restarted Terminal panel session."
                    }
                }
            }),
        ));
        tools.push(AgentTool::new(
            "term_close",
            "Terminal Close",
            TERM_CLOSE_TOOL_DESCRIPTION,
            serde_json::json!({
                "type": "object",
                "properties": {}
            }),
        ));
    }

    // Task tracking tools (always available)
    tools.push(AgentTool::new(
        "create_task",
        "Create Task",
        "Create a new task board with steps to track implementation progress. Use this when starting a complex multi-step implementation. After creating a board, keep it current while you work instead of waiting until the very end to update statuses.",
        serde_json::json!({
            "type": "object",
            "properties": {
                "title": {
                    "type": "string",
                    "description": "Human-readable title for the task board."
                },
                "steps": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "properties": {
                            "description": { "type": "string" }
                        },
                        "required": ["description"]
                    },
                    "description": "Ordered list of task steps."
                }
            },
            "required": ["title", "steps"]
        }),
    ));
    tools.push(AgentTool::new(
        "update_task",
        "Update Task",
        "Update a task board or its steps. Keep task state aligned with the real implementation lifecycle: update progress as each step finishes, prefer `advance_step` or `complete_step` to move work forward, and make sure the board is fully reconciled before the run ends.",
        serde_json::json!({
            "type": "object",
            "properties": {
                "taskBoardId": {
                    "type": "string",
                    "description": "ID of the task board to update."
                },
                "action": {
                    "type": "string",
                    "enum": ["start_step", "advance_step", "complete_step", "fail_step", "complete_board", "abandon_board"],
                    "description": "The action to perform. `advance_step` is the recommended default for normal progress updates because it completes the current in-progress step and automatically starts the next step or completes the board."
                },
                "stepId": {
                    "type": "string",
                    "description": "ID of the step (required for start_step, complete_step, fail_step; optional for advance_step, which falls back to the board's current active step)."
                },
                "errorDetail": {
                    "type": "string",
                    "description": "Error description (required for fail_step)."
                },
                "reason": {
                    "type": "string",
                    "description": "Optional reason for abandoning the board."
                }
            },
            "required": ["taskBoardId", "action"]
        }),
    ));

    tools
}

fn resolve_tool_profile_name(raw_plan: &RuntimeModelPlan, run_mode: &str) -> String {
    if let Some(profile_name) = raw_plan
        .tool_profile_by_mode
        .as_ref()
        .and_then(|value| value.get(run_mode))
        .and_then(serde_json::Value::as_str)
        .filter(|value| !value.trim().is_empty())
    {
        return profile_name.to_string();
    }

    match run_mode {
        "plan" => PLAN_READ_ONLY_TOOL_PROFILE.to_string(),
        _ => DEFAULT_FULL_TOOL_PROFILE.to_string(),
    }
}

fn resolve_helper_profile(tool: RuntimeOrchestrationTool) -> SubagentProfile {
    match tool {
        RuntimeOrchestrationTool::Explore => SubagentProfile::Explore,
        RuntimeOrchestrationTool::Review => SubagentProfile::Review,
    }
}

fn resolve_helper_model_role(
    model_plan: &ResolvedRuntimeModelPlan,
    tool: RuntimeOrchestrationTool,
) -> ResolvedModelRole {
    match tool {
        RuntimeOrchestrationTool::Explore | RuntimeOrchestrationTool::Review => model_plan
            .auxiliary
            .clone()
            .unwrap_or_else(|| model_plan.primary.clone()),
    }
}

pub(crate) fn convert_history_messages(
    messages: &[MessageRecord],
    model: &Model,
) -> Vec<AgentMessage> {
    messages
        .iter()
        .filter_map(|message| match message.message_type.as_str() {
            "plain_message" => match message.role.as_str() {
                "user" => Some(AgentMessage::User(history_user_message(message, model))),
                "assistant" => Some(AgentMessage::Assistant(assistant_message_from_text(
                    &message.content_markdown,
                    model,
                ))),
                _ => None,
            },
            "plan" if message.role == "assistant" => Some(AgentMessage::Assistant(
                assistant_message_from_text(&format_plan_history_message(message), model),
            )),
            "summary_marker" if is_context_summary_marker(message) => {
                let summary = message.content_markdown.trim();
                if summary.is_empty() {
                    None
                } else {
                    Some(AgentMessage::User(UserMessage::text(summary.to_string())))
                }
            }
            _ => None,
        })
        .collect()
}

fn history_user_message(message: &MessageRecord, model: &Model) -> UserMessage {
    let text = history_user_message_text(message);
    let attachments = history_message_attachments(message);

    if attachments.is_empty() {
        return UserMessage::text(text);
    }

    let mut blocks = Vec::new();
    let trimmed = text.trim();
    if !trimmed.is_empty() {
        blocks.push(ContentBlock::Text(TextContent::new(trimmed)));
    }

    blocks.extend(attachment_blocks(&attachments, model));

    if blocks.is_empty() {
        UserMessage::text(text)
    } else {
        UserMessage::blocks(blocks)
    }
}

fn history_user_message_text(message: &MessageRecord) -> String {
    message
        .metadata_json
        .as_deref()
        .and_then(command_effective_prompt_from_metadata)
        .unwrap_or_else(|| message.content_markdown.clone())
}

fn command_effective_prompt_from_metadata(raw: &str) -> Option<String> {
    let metadata = serde_json::from_str::<serde_json::Value>(raw).ok()?;
    let composer = metadata.get("composer")?;

    if composer.get("kind")?.as_str()? != "command" {
        return None;
    }

    composer
        .get("effectivePrompt")?
        .as_str()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
}

fn history_message_attachments(message: &MessageRecord) -> Vec<MessageAttachmentDto> {
    message
        .attachments_json
        .as_deref()
        .and_then(|raw| serde_json::from_str::<Vec<MessageAttachmentDto>>(raw).ok())
        .unwrap_or_default()
}

fn attachment_blocks(attachments: &[MessageAttachmentDto], model: &Model) -> Vec<ContentBlock> {
    attachments
        .iter()
        .filter_map(|attachment| attachment_block(attachment, model))
        .collect()
}

fn attachment_block(attachment: &MessageAttachmentDto, model: &Model) -> Option<ContentBlock> {
    if is_image_attachment(attachment) {
        return Some(image_attachment_block(attachment, model));
    }

    if is_text_attachment(attachment) {
        return Some(text_attachment_block(attachment));
    }

    None
}

fn image_attachment_block(attachment: &MessageAttachmentDto, model: &Model) -> ContentBlock {
    let label = attachment_label(attachment);

    if !model.supports_image() {
        return ContentBlock::Text(TextContent::new(format!("[Image attachment: {label}]")));
    }

    let Some(parsed) = attachment
        .url
        .as_deref()
        .and_then(parse_data_url)
        .filter(|parsed| parsed.is_base64)
    else {
        return ContentBlock::Text(TextContent::new(format!(
            "[Image attachment could not be loaded: {label}]"
        )));
    };

    let mime_type = attachment
        .media_type
        .clone()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or(parsed.mime_type);

    ContentBlock::Image(ImageContent::new(parsed.payload, mime_type))
}

fn text_attachment_block(attachment: &MessageAttachmentDto) -> ContentBlock {
    let label = attachment_label(attachment);

    match attachment
        .url
        .as_deref()
        .and_then(decode_data_url_text)
        .map(|text| render_text_attachment(&label, &text))
    {
        Some(content) => ContentBlock::Text(TextContent::new(content)),
        None => ContentBlock::Text(TextContent::new(format!(
            "[Text attachment could not be decoded: {label}]"
        ))),
    }
}

fn attachment_label(attachment: &MessageAttachmentDto) -> String {
    let trimmed = attachment.name.trim();
    if trimmed.is_empty() {
        "unnamed attachment".to_string()
    } else {
        trimmed.to_string()
    }
}

fn is_image_attachment(attachment: &MessageAttachmentDto) -> bool {
    attachment
        .media_type
        .as_deref()
        .map(|value| value.starts_with("image/"))
        .unwrap_or(false)
        || matches!(
            file_extension(&attachment.name).as_deref(),
            Some("png" | "jpg" | "jpeg" | "gif" | "webp" | "bmp" | "svg")
        )
}

fn is_text_attachment(attachment: &MessageAttachmentDto) -> bool {
    attachment
        .media_type
        .as_deref()
        .map(is_text_media_type)
        .unwrap_or(false)
        || matches!(
            file_extension(&attachment.name).as_deref(),
            Some(
                "txt"
                    | "md"
                    | "markdown"
                    | "json"
                    | "js"
                    | "jsx"
                    | "ts"
                    | "tsx"
                    | "rs"
                    | "py"
                    | "java"
                    | "go"
                    | "css"
                    | "scss"
                    | "less"
                    | "html"
                    | "xml"
                    | "yaml"
                    | "yml"
                    | "toml"
                    | "ini"
                    | "sh"
                    | "bash"
                    | "zsh"
                    | "sql"
                    | "c"
                    | "cc"
                    | "cpp"
                    | "h"
                    | "hpp"
                    | "swift"
                    | "kt"
                    | "rb"
                    | "php"
                    | "vue"
                    | "svelte"
                    | "astro"
            )
        )
}

fn is_text_media_type(media_type: &str) -> bool {
    media_type.starts_with("text/")
        || matches!(
            media_type,
            "application/json"
                | "application/xml"
                | "application/yaml"
                | "application/x-yaml"
                | "application/toml"
                | "application/javascript"
                | "application/x-javascript"
                | "application/typescript"
        )
}

fn file_extension(name: &str) -> Option<String> {
    let (_, ext) = name.rsplit_once('.')?;
    let trimmed = ext.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_ascii_lowercase())
    }
}

fn render_text_attachment(name: &str, content: &str) -> String {
    let trimmed = content.trim();
    let truncated = truncate_for_attachment(trimmed, TEXT_ATTACHMENT_MAX_CHARS);
    let language = fence_language(name);
    let suffix = if truncated.chars().count() < trimmed.chars().count() {
        "\n[attachment content truncated]"
    } else {
        ""
    };

    format!("[Text attachment: {name}]\n~~~{language}\n{truncated}\n~~~{suffix}")
}

fn fence_language(name: &str) -> &'static str {
    match file_extension(name).as_deref() {
        Some("md" | "markdown") => "markdown",
        Some("txt") => "text",
        Some("json") => "json",
        Some("js" | "jsx") => "javascript",
        Some("ts" | "tsx") => "typescript",
        Some("rs") => "rust",
        Some("py") => "python",
        Some("java") => "java",
        Some("go") => "go",
        Some("css" | "scss" | "less") => "css",
        Some("html") => "html",
        Some("xml") => "xml",
        Some("yaml" | "yml") => "yaml",
        Some("toml") => "toml",
        Some("sh" | "bash" | "zsh") => "bash",
        Some("sql") => "sql",
        Some("c" | "cc" | "cpp" | "h" | "hpp") => "cpp",
        Some("swift") => "swift",
        Some("kt") => "kotlin",
        Some("rb") => "ruby",
        Some("php") => "php",
        Some("vue") => "vue",
        Some("svelte") => "svelte",
        Some("astro") => "astro",
        _ => "text",
    }
}

fn truncate_for_attachment(value: &str, max_chars: usize) -> String {
    value.chars().take(max_chars).collect()
}

fn decode_data_url_text(url: &str) -> Option<String> {
    let parsed = parse_data_url(url)?;
    let bytes = if parsed.is_base64 {
        general_purpose::STANDARD
            .decode(parsed.payload.as_bytes())
            .ok()?
    } else {
        percent_decode(parsed.payload.as_bytes())?
    };

    Some(String::from_utf8_lossy(&bytes).into_owned())
}

struct ParsedDataUrl {
    mime_type: String,
    payload: String,
    is_base64: bool,
}

fn parse_data_url(url: &str) -> Option<ParsedDataUrl> {
    let payload = url.strip_prefix("data:")?;
    let (meta, data) = payload.split_once(',')?;
    let mut meta_parts = meta.split(';');
    let mime_type = meta_parts
        .next()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or("text/plain")
        .to_string();
    let is_base64 = meta_parts.any(|part| part.eq_ignore_ascii_case("base64"));

    Some(ParsedDataUrl {
        mime_type,
        payload: data.to_string(),
        is_base64,
    })
}

fn percent_decode(bytes: &[u8]) -> Option<Vec<u8>> {
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;

    while index < bytes.len() {
        if bytes[index] == b'%' {
            if index + 2 >= bytes.len() {
                return None;
            }
            let high = decode_hex_digit(bytes[index + 1])?;
            let low = decode_hex_digit(bytes[index + 2])?;
            decoded.push((high << 4) | low);
            index += 3;
            continue;
        }

        decoded.push(bytes[index]);
        index += 1;
    }

    Some(decoded)
}

fn decode_hex_digit(value: u8) -> Option<u8> {
    match value {
        b'0'..=b'9' => Some(value - b'0'),
        b'a'..=b'f' => Some(value - b'a' + 10),
        b'A'..=b'F' => Some(value - b'A' + 10),
        _ => None,
    }
}

fn is_context_reset_marker(message: &MessageRecord) -> bool {
    message.message_type == "summary_marker"
        && metadata_kind_matches(message.metadata_json.as_deref(), "context_reset")
}

fn is_context_summary_marker(message: &MessageRecord) -> bool {
    message.message_type == "summary_marker"
        && metadata_kind_matches(message.metadata_json.as_deref(), "context_summary")
}

fn metadata_kind_matches(raw: Option<&str>, expected: &str) -> bool {
    raw.and_then(|value| serde_json::from_str::<serde_json::Value>(value).ok())
        .and_then(|value| {
            value
                .get("kind")
                .and_then(serde_json::Value::as_str)
                .map(str::to_string)
        })
        .as_deref()
        == Some(expected)
}

fn format_plan_history_message(message: &MessageRecord) -> String {
    let metadata = message
        .metadata_json
        .as_deref()
        .and_then(|raw| serde_json::from_str::<serde_json::Value>(raw).ok())
        .and_then(|value| parse_plan_message_metadata(&value));

    let Some(metadata) = metadata else {
        return format!(
            "Implementation plan checkpoint:\n{}",
            message.content_markdown.trim()
        );
    };

    format!(
        "Implementation plan checkpoint (revision {}, approval state: {}):\n{}",
        metadata.artifact.plan_revision,
        metadata.approval_state,
        plan_markdown(&metadata)
    )
}

fn assistant_message_from_text(content: &str, model: &Model) -> AssistantMessage {
    AssistantMessage::builder()
        .content(vec![ContentBlock::Text(TextContent::new(content))])
        .api(effective_api_for_model(model))
        .provider(model.provider.clone())
        .model(model.id.clone())
        .usage(Usage::default())
        .stop_reason(StopReason::Stop)
        .build()
        .expect("assistant history message should always build")
}

fn effective_api_for_model(model: &Model) -> tiy_core::types::Api {
    if let Some(api) = model.api.clone() {
        return api;
    }

    match &model.provider {
        Provider::OpenAI | Provider::OpenAIResponses | Provider::AzureOpenAIResponses => {
            tiy_core::types::Api::OpenAIResponses
        }
        Provider::Anthropic | Provider::MiniMax | Provider::MiniMaxCN | Provider::KimiCoding => {
            tiy_core::types::Api::AnthropicMessages
        }
        Provider::Google | Provider::GoogleGeminiCli | Provider::GoogleAntigravity => {
            tiy_core::types::Api::GoogleGenerativeAi
        }
        Provider::GoogleVertex => tiy_core::types::Api::GoogleVertex,
        Provider::Ollama => tiy_core::types::Api::Ollama,
        Provider::XAI
        | Provider::Groq
        | Provider::OpenRouter
        | Provider::OpenAICompatible
        | Provider::OpenAICodex
        | Provider::GitHubCopilot
        | Provider::Cerebras
        | Provider::VercelAiGateway
        | Provider::ZAI
        | Provider::Mistral
        | Provider::HuggingFace
        | Provider::OpenCode
        | Provider::OpenCodeGo
        | Provider::DeepSeek
        | Provider::Zenmux => tiy_core::types::Api::OpenAICompletions,
        Provider::AmazonBedrock => tiy_core::types::Api::BedrockConverseStream,
        Provider::Custom(name) => tiy_core::types::Api::Custom(name.clone()),
    }
}

/// Maximum size for a single tool result sent to the LLM (8 MB).
/// OpenAI Responses API enforces a 10 MB limit per `input[n].output` field;
/// this leaves headroom for protocol overhead and JSON escaping.
const MAX_TOOL_RESULT_SIZE: usize = 8_000_000;

fn agent_tool_result_from_output(output: crate::core::executors::ToolOutput) -> AgentToolResult {
    // Use compact JSON (no pretty-print) to reduce whitespace overhead.
    let mut rendered =
        serde_json::to_string(&output.result).unwrap_or_else(|_| output.result.to_string());

    // Hard safety cap — truncate if the serialized result is still too large.
    if rendered.len() > MAX_TOOL_RESULT_SIZE {
        rendered.truncate(MAX_TOOL_RESULT_SIZE);
        // Ensure we don't cut in the middle of a multi-byte UTF-8 char
        while !rendered.is_char_boundary(rendered.len()) {
            rendered.pop();
        }
        rendered.push_str("\n\n[Tool output truncated: exceeded 8MB limit]");
    }

    if output.success {
        AgentToolResult {
            content: vec![ContentBlock::Text(TextContent::new(rendered))],
            details: Some(output.result),
        }
    } else {
        AgentToolResult {
            content: vec![ContentBlock::Text(TextContent::new(format!(
                "Error: {rendered}"
            )))],
            details: Some(output.result),
        }
    }
}

fn agent_error_result(message: impl Into<String>) -> AgentToolResult {
    AgentToolResult {
        content: vec![ContentBlock::Text(TextContent::new(format!(
            "Error: {}",
            message.into()
        )))],
        details: None,
    }
}

fn validate_clarify_input(value: &serde_json::Value) -> Result<(), String> {
    let Some(question) = value.get("question").and_then(serde_json::Value::as_str) else {
        return Err("clarify requires a non-empty question".to_string());
    };

    if question.trim().is_empty() {
        return Err("clarify requires a non-empty question".to_string());
    }

    let Some(options) = value.get("options").and_then(serde_json::Value::as_array) else {
        return Err("clarify requires 2 to 5 options".to_string());
    };

    if !(2..=5).contains(&options.len()) {
        return Err("clarify requires 2 to 5 options".to_string());
    }

    let recommended_count = options
        .iter()
        .filter(|option| {
            option
                .get("recommended")
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(false)
        })
        .count();

    if recommended_count > 1 {
        return Err("clarify may mark at most one option as recommended".to_string());
    }

    for option in options {
        let label = option
            .get("label")
            .and_then(serde_json::Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty());
        let description = option
            .get("description")
            .and_then(serde_json::Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty());

        if label.is_none() || description.is_none() {
            return Err("clarify options must include non-empty label and description".to_string());
        }
    }

    Ok(())
}

async fn resolve_model_plan(
    pool: &SqlitePool,
    raw_plan: RuntimeModelPlan,
) -> Result<ResolvedRuntimeModelPlan, AppError> {
    let primary = raw_plan.primary.clone().ok_or_else(|| {
        AppError::recoverable(
            ErrorSource::Settings,
            "settings.model_plan.primary_missing",
            "Run model plan is missing the primary model",
        )
    })?;

    let primary = resolve_runtime_model_role(pool, primary).await?;
    let auxiliary = match raw_plan.auxiliary.clone() {
        Some(role) => Some(resolve_runtime_model_role(pool, role).await?),
        None => None,
    };
    let lightweight = match raw_plan.lightweight.clone() {
        Some(role) => Some(resolve_runtime_model_role(pool, role).await?),
        None => None,
    };

    Ok(ResolvedRuntimeModelPlan {
        thinking_level: raw_plan
            .thinking_level
            .as_deref()
            .map(ThinkingLevel::from)
            .unwrap_or(ThinkingLevel::Off),
        transport: parse_transport(raw_plan.transport.as_deref()),
        raw: raw_plan,
        primary,
        auxiliary,
        lightweight,
    })
}

pub async fn resolve_runtime_model_role(
    pool: &SqlitePool,
    role: RuntimeModelRole,
) -> Result<ResolvedModelRole, AppError> {
    let provider = provider_repo::find_by_id(pool, &role.provider_id)
        .await?
        .ok_or_else(|| AppError::not_found(ErrorSource::Settings, "provider"))?;

    let provider_name = role
        .provider_name
        .clone()
        .unwrap_or_else(|| provider.display_name.clone());
    let model_name = role
        .model_display_name
        .clone()
        .unwrap_or_else(|| role.model.clone());
    let context_window = parse_positive_u32(role.context_window.as_deref(), DEFAULT_CONTEXT_WINDOW);
    let max_output_tokens =
        parse_positive_u32(role.max_output_tokens.as_deref(), DEFAULT_MAX_OUTPUT_TOKENS);

    let mut builder = Model::builder()
        .id(&role.model)
        .name(&model_name)
        .provider(Provider::from(role.provider_type.clone()))
        .base_url(&role.base_url)
        .context_window(context_window)
        .max_tokens(max_output_tokens)
        .input({
            let mut input = vec![InputType::Text];
            if role.supports_image_input.unwrap_or(false) {
                input.push(InputType::Image);
            }
            input
        })
        .cost(Cost::default());

    if let Some(headers) = role.custom_headers.clone() {
        if !headers.is_empty() {
            builder = builder.headers(headers);
        }
    }

    let model = builder.build().map_err(|error| {
        AppError::internal(
            ErrorSource::Settings,
            format!("failed to build runtime model: {error}"),
        )
    })?;

    Ok(ResolvedModelRole {
        provider_id: role.provider_id,
        model_record_id: role.model_record_id,
        model_id: role.model_id,
        model_name,
        provider_type: role.provider_type,
        provider_name,
        api_key: provider.api_key_encrypted,
        provider_options: normalize_provider_options(role.provider_options),
        model,
    })
}

async fn build_system_prompt(
    pool: &SqlitePool,
    raw_plan: &RuntimeModelPlan,
    workspace_path: &str,
    run_mode: &str,
) -> Result<String, AppError> {
    prompt::build_system_prompt(pool, raw_plan, workspace_path, run_mode).await
}

fn plan_mode_missing_checkpoint_error(
    run_mode: &str,
    checkpoint_requested: bool,
) -> Option<&'static str> {
    if run_mode == "plan" && !checkpoint_requested {
        Some(PLAN_MODE_MISSING_CHECKPOINT_ERROR)
    } else {
        None
    }
}

pub fn build_profile_response_prompt_parts(profile: &AgentProfileRecord) -> Vec<String> {
    build_profile_response_prompt_parts_from_runtime(
        profile.response_language.as_deref(),
        profile.response_style.as_deref(),
    )
}

fn build_profile_response_prompt_parts_from_runtime(
    response_language: Option<&str>,
    response_style: Option<&str>,
) -> Vec<String> {
    let mut parts = Vec::new();

    if let Some(language) = normalize_profile_response_language(response_language) {
        parts.push(format!(
            "Respond in {language} unless the user explicitly asks for a different language."
        ));
    }

    parts.push(
        response_style_system_instruction(normalize_profile_response_style(response_style))
            .to_string(),
    );

    parts
}

pub fn normalize_profile_response_language(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
}

pub fn normalize_profile_response_style(value: Option<&str>) -> ProfileResponseStyle {
    match value.unwrap_or("balanced").trim().to_lowercase().as_str() {
        "concise" => ProfileResponseStyle::Concise,
        "guide" | "guided" => ProfileResponseStyle::Guide,
        _ => ProfileResponseStyle::Balanced,
    }
}

pub fn response_style_system_instruction(style: ProfileResponseStyle) -> &'static str {
    match style {
        ProfileResponseStyle::Balanced => {
            "Response style: balanced. Default to a compact but complete answer. Lead with the answer or outcome first. Use a short paragraph or a short flat list when that makes the reply clearer. Add explanation when it materially helps understanding, but avoid over-explaining routine details. Each point should be a complete thought expressed in a full sentence, not a bare noun phrase or keyword fragment. When multiple points share a single theme, consolidate them into one paragraph rather than scattering them across separate bullets."
        }
        ProfileResponseStyle::Concise => {
            "Response style: concise. Treat brevity as a hard default. Lead with the answer, result, or next action immediately. Keep the final response to 1-3 short sentences or a very short flat list unless the user explicitly asks for more detail. Do not include background, reasoning, summaries, or pleasantries unless they are required for correctness. Prefer code, commands, and direct facts over prose."
        }
        ProfileResponseStyle::Guide => {
            "Response style: guided. Lead with the answer, then explain the reasoning, tradeoffs, and recommended next steps clearly. Be intentionally explanatory when that helps the user learn or make a decision. Surface relevant alternatives, caveats, or examples when useful."
        }
    }
}

fn parse_positive_u32(value: Option<&str>, fallback: u32) -> u32 {
    value
        .and_then(|value| value.trim().parse::<u32>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(fallback)
}

fn parse_transport(value: Option<&str>) -> Transport {
    match value.unwrap_or("sse").trim().to_lowercase().as_str() {
        "websocket" | "ws" => Transport::WebSocket,
        "auto" => Transport::Auto,
        _ => Transport::Sse,
    }
}

fn normalize_provider_options(value: Option<serde_json::Value>) -> Option<serde_json::Value> {
    value.and_then(|value| match value {
        serde_json::Value::Object(map) if map.is_empty() => None,
        serde_json::Value::Object(_) => Some(value),
        _ => None,
    })
}

fn runtime_security_config() -> tiy_core::types::SecurityConfig {
    let mut security = tiy_core::types::SecurityConfig::default();
    security.agent.tool_execution_timeout_secs = SUBAGENT_TOOL_TIMEOUT_SECS;
    security
}

fn standard_tool_timeout() -> std::time::Duration {
    std::time::Duration::from_secs(STANDARD_TOOL_TIMEOUT_SECS)
}

fn merge_payload(mut base: serde_json::Value, patch: &serde_json::Value) -> serde_json::Value {
    merge_json_value(&mut base, patch);
    base
}

fn merge_json_value(base: &mut serde_json::Value, patch: &serde_json::Value) {
    match (base, patch) {
        (serde_json::Value::Object(base_map), serde_json::Value::Object(patch_map)) => {
            for (key, patch_value) in patch_map {
                if let Some(base_value) = base_map.get_mut(key) {
                    merge_json_value(base_value, patch_value);
                } else {
                    base_map.insert(key.clone(), patch_value.clone());
                }
            }
        }
        (base_value, patch_value) => {
            *base_value = patch_value.clone();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        build_profile_response_prompt_parts, build_system_prompt, convert_history_messages,
        handle_agent_event, normalize_profile_response_language, normalize_profile_response_style,
        plan_mode_missing_checkpoint_error, resolve_helper_model_role, resolve_helper_profile,
        response_style_system_instruction, runtime_security_config, runtime_tools_for_profile,
        standard_tool_timeout, trim_history_to_current_context, ProfileResponseStyle,
        ResolvedModelRole, ResolvedRuntimeModelPlan, RuntimeModelPlan, DEFAULT_FULL_TOOL_PROFILE,
        PLAN_MODE_MISSING_CHECKPOINT_ERROR, PLAN_READ_ONLY_TOOL_PROFILE,
        STANDARD_TOOL_TIMEOUT_SECS, SUBAGENT_TOOL_TIMEOUT_SECS,
    };
    use std::fs;
    use std::sync::Mutex as StdMutex;

    use tempfile::tempdir;
    use tiy_core::agent::{AgentEvent, AgentMessage};
    use tiy_core::thinking::ThinkingLevel;
    use tiy_core::types::{Api, AssistantMessage, AssistantMessageEvent, ContentBlock, Provider};
    use tokio::sync::mpsc;

    use crate::core::plan_checkpoint::{
        build_plan_artifact_from_tool_input, build_plan_message_metadata,
    };
    use crate::core::prompt::providers::{
        final_response_structure_system_instruction, run_mode_prompt_body,
    };
    use crate::core::subagent::{RuntimeOrchestrationTool, SubagentProfile};
    use crate::ipc::frontend_channels::ThreadStreamEvent;
    use crate::model::provider::AgentProfileRecord;
    use crate::model::thread::MessageRecord;
    use crate::persistence::init_database;

    const TEST_CONTEXT_WINDOW: &str = "128000";
    const TEST_MODEL_DISPLAY_NAME: &str = "GPT Test";

    fn sample_profile() -> AgentProfileRecord {
        AgentProfileRecord {
            id: "profile-1".to_string(),
            name: "Default".to_string(),
            custom_instructions: None,
            commit_message_prompt: None,
            response_style: Some("balanced".to_string()),
            response_language: Some("English".to_string()),
            commit_message_language: Some("English".to_string()),
            primary_provider_id: None,
            primary_model_id: None,
            auxiliary_provider_id: None,
            auxiliary_model_id: None,
            lightweight_provider_id: None,
            lightweight_model_id: None,
            is_default: true,
            created_at: String::new(),
            updated_at: String::new(),
        }
    }

    fn sample_partial_assistant_message() -> AssistantMessage {
        AssistantMessage::builder()
            .api(Api::OpenAICompletions)
            .provider(Provider::OpenAI)
            .model("gpt-test")
            .build()
            .expect("partial assistant message")
    }

    fn sample_resolved_model_role_with_inputs(
        model_id: &str,
        input: Vec<tiy_core::types::InputType>,
    ) -> ResolvedModelRole {
        let model = tiy_core::types::Model::builder()
            .id(model_id)
            .name(model_id)
            .provider(Provider::OpenAI)
            .base_url("https://api.openai.com/v1")
            .context_window(128_000)
            .max_tokens(32_000)
            .input(input)
            .cost(tiy_core::types::Cost::default())
            .build()
            .expect("sample resolved model");

        ResolvedModelRole {
            provider_id: format!("provider-{model_id}"),
            model_record_id: format!("record-{model_id}"),
            model_id: model_id.to_string(),
            model_name: model_id.to_string(),
            provider_type: "openai".to_string(),
            provider_name: "OpenAI".to_string(),
            api_key: Some("sk-test".to_string()),
            provider_options: None,
            model,
        }
    }

    fn sample_resolved_model_role(model_id: &str) -> ResolvedModelRole {
        sample_resolved_model_role_with_inputs(model_id, vec![tiy_core::types::InputType::Text])
    }

    fn sample_resolved_runtime_model_plan(
        auxiliary: Option<ResolvedModelRole>,
    ) -> ResolvedRuntimeModelPlan {
        ResolvedRuntimeModelPlan {
            raw: RuntimeModelPlan::default(),
            primary: sample_resolved_model_role("primary-model"),
            auxiliary,
            lightweight: None,
            thinking_level: ThinkingLevel::Off,
            transport: tiy_core::types::Transport::Sse,
        }
    }

    fn message_text(message: &AgentMessage) -> String {
        match message {
            AgentMessage::User(user) => match &user.content {
                tiy_core::types::UserContent::Text(text) => text.clone(),
                tiy_core::types::UserContent::Blocks(blocks) => blocks
                    .iter()
                    .filter_map(|block| match block {
                        tiy_core::types::ContentBlock::Text(text) => Some(text.text.as_str()),
                        _ => None,
                    })
                    .collect::<Vec<_>>()
                    .join("\n"),
            },
            AgentMessage::Assistant(assistant) => assistant.text_content(),
            _ => String::new(),
        }
    }

    fn user_blocks(message: &AgentMessage) -> &[ContentBlock] {
        match message {
            AgentMessage::User(user) => match &user.content {
                tiy_core::types::UserContent::Blocks(blocks) => blocks,
                _ => panic!("expected block-based user message"),
            },
            _ => panic!("expected user message"),
        }
    }

    fn handle_test_agent_event(
        run_id: &str,
        event_tx: &mpsc::UnboundedSender<ThreadStreamEvent>,
        current_message_id: &StdMutex<Option<String>>,
        current_reasoning_message_id: &StdMutex<Option<String>>,
        last_usage: &StdMutex<Option<tiy_core::types::Usage>>,
        reasoning_buffer: &StdMutex<String>,
        event: &AgentEvent,
    ) {
        let last_completed_message_id = StdMutex::new(None::<String>);
        handle_agent_event(
            run_id,
            event_tx,
            current_message_id,
            &last_completed_message_id,
            current_reasoning_message_id,
            last_usage,
            reasoning_buffer,
            TEST_CONTEXT_WINDOW,
            TEST_MODEL_DISPLAY_NAME,
            event,
        );
    }

    #[test]
    fn test_runtime_security_config_extends_helper_tool_timeout() {
        let security = runtime_security_config();

        assert_eq!(
            security.agent.tool_execution_timeout_secs,
            SUBAGENT_TOOL_TIMEOUT_SECS
        );
    }

    #[test]
    fn test_standard_tool_timeout_remains_120_seconds() {
        assert_eq!(
            standard_tool_timeout().as_secs(),
            STANDARD_TOOL_TIMEOUT_SECS
        );
    }

    #[test]
    fn profile_response_language_is_trimmed() {
        assert_eq!(
            normalize_profile_response_language(Some("  简体中文  ")).as_deref(),
            Some("简体中文")
        );
        assert_eq!(normalize_profile_response_language(Some("   ")), None);
    }

    #[test]
    fn profile_response_style_defaults_to_balanced() {
        assert_eq!(
            normalize_profile_response_style(Some("guide")),
            ProfileResponseStyle::Guide
        );
        assert_eq!(
            normalize_profile_response_style(Some("concise")),
            ProfileResponseStyle::Concise
        );
        assert_eq!(
            normalize_profile_response_style(Some("unknown")),
            ProfileResponseStyle::Balanced
        );
    }

    #[test]
    fn profile_prompt_parts_include_language_and_style() {
        let mut profile = sample_profile();
        profile.response_language = Some("Japanese".to_string());
        profile.response_style = Some("concise".to_string());

        let parts = build_profile_response_prompt_parts(&profile);

        assert_eq!(parts.len(), 2);
        assert!(parts[0].contains("Japanese"));
        assert_eq!(
            parts[1],
            response_style_system_instruction(ProfileResponseStyle::Concise)
        );
    }

    #[test]
    fn response_style_instructions_have_stronger_behavioral_separation() {
        let balanced = response_style_system_instruction(ProfileResponseStyle::Balanced);
        let concise = response_style_system_instruction(ProfileResponseStyle::Concise);
        let guide = response_style_system_instruction(ProfileResponseStyle::Guide);

        assert!(balanced.contains("compact but complete answer"));
        assert!(concise.contains("1-3 short sentences"));
        assert!(concise.contains("hard default"));
        assert!(guide.contains("tradeoffs"));
        assert!(guide.contains("recommended next steps"));
    }

    #[test]
    fn final_response_structure_instruction_matches_task_types_and_markdown_hierarchy() {
        let instruction = final_response_structure_system_instruction();

        assert!(instruction.contains("at most two heading levels"));
        assert!(instruction.contains("avoid turning every sub-point into its own heading"));
        assert!(instruction.contains("Debug or problem analysis"));
        assert!(instruction.contains("Code change or result report"));
        assert!(instruction.contains("Comparison or decision support"));
        assert!(instruction.contains("Direct explanation or question answering"));
        assert!(instruction.contains("structured Markdown presentation"));
        assert!(instruction.contains("Do not overload the reply with inline code formatting"));
    }

    #[test]
    fn final_response_structure_section_is_distinct_from_response_style_rules() {
        let section = format!(
            "## Final Response Structure\n{}",
            final_response_structure_system_instruction()
        );
        let balanced = response_style_system_instruction(ProfileResponseStyle::Balanced);

        assert!(section.starts_with("## Final Response Structure"));
        assert!(section.contains("For simple tasks, you may compress the structure"));
        assert!(balanced.contains("compact but complete answer"));
        assert!(!balanced.contains("reason 1, 2, and 3"));
    }

    #[test]
    fn run_mode_prompt_clarifies_terminal_panel_scope() {
        let plan_prompt = run_mode_prompt_body("plan");
        let default_prompt = run_mode_prompt_body("default");

        assert!(plan_prompt.contains("embedded Terminal panel"));
        assert!(plan_prompt.contains("update_plan"));
        assert!(plan_prompt.contains("pause for user approval"));
        assert!(plan_prompt.contains("do not inspect your own runtime"));
        assert!(default_prompt.contains("embedded Terminal panel"));
        assert!(default_prompt.contains("do not inspect your own runtime"));
    }

    #[test]
    fn default_full_profile_exposes_mutating_terminal_tools() {
        let tools = runtime_tools_for_profile(DEFAULT_FULL_TOOL_PROFILE);
        let tool_names: Vec<&str> = tools.iter().map(|tool| tool.name.as_str()).collect();

        assert!(tool_names.contains(&"term_write"));
        assert!(tool_names.contains(&"term_restart"));
        assert!(tool_names.contains(&"term_close"));
    }

    #[test]
    fn plan_read_only_profile_does_not_expose_mutating_terminal_tools() {
        let tools = runtime_tools_for_profile(PLAN_READ_ONLY_TOOL_PROFILE);
        let tool_names: Vec<&str> = tools.iter().map(|tool| tool.name.as_str()).collect();

        assert!(!tool_names.contains(&"term_write"));
        assert!(!tool_names.contains(&"term_restart"));
        assert!(!tool_names.contains(&"term_close"));
    }

    #[test]
    fn runtime_file_tools_expose_window_and_limit_parameters() {
        let tools = runtime_tools_for_profile(DEFAULT_FULL_TOOL_PROFILE);

        let read_tool = tools
            .iter()
            .find(|tool| tool.name == "read")
            .expect("read tool should exist");
        let list_tool = tools
            .iter()
            .find(|tool| tool.name == "list")
            .expect("list tool should exist");
        let find_tool = tools
            .iter()
            .find(|tool| tool.name == "find")
            .expect("find tool should exist");

        let read_properties = read_tool.parameters["properties"]
            .as_object()
            .expect("read properties should be object");
        let list_properties = list_tool.parameters["properties"]
            .as_object()
            .expect("list properties should be object");
        let find_properties = find_tool.parameters["properties"]
            .as_object()
            .expect("find properties should be object");

        assert!(read_properties.contains_key("offset"));
        assert!(read_properties.contains_key("limit"));
        assert!(list_properties.contains_key("limit"));
        assert!(find_properties.contains_key("limit"));
    }

    #[tokio::test]
    async fn system_prompt_delegates_post_implementation_verification_to_review_helper() {
        let temp_dir = tempdir().expect("temp dir");
        let workspace_root = temp_dir.path().join("workspace");
        fs::create_dir(&workspace_root).expect("workspace dir");

        let db_path = temp_dir.path().join("test.db");
        let pool = init_database(&db_path).await.expect("database");

        let prompt = build_system_prompt(
            &pool,
            &RuntimeModelPlan::default(),
            workspace_root.to_string_lossy().as_ref(),
            "default",
        )
        .await
        .expect("system prompt");

        assert!(prompt.contains(
            "review helper is responsible for running the necessary type-check and test commands"
        ));
        assert!(prompt.contains(
            "Do not rerun the same verification commands yourself unless the helper explicitly could not run them"
        ));
    }

    #[test]
    fn reasoning_blocks_reset_message_id_between_thought_segments() {
        let (event_tx, mut event_rx) = mpsc::unbounded_channel();
        let current_message_id = StdMutex::new(None::<String>);
        let current_reasoning_message_id = StdMutex::new(None::<String>);
        let last_usage = StdMutex::new(None::<tiy_core::types::Usage>);
        let reasoning_buffer = StdMutex::new(String::new());
        let partial = sample_partial_assistant_message();

        handle_test_agent_event(
            "run-1",
            &event_tx,
            &current_message_id,
            &current_reasoning_message_id,
            &last_usage,
            &reasoning_buffer,
            &AgentEvent::MessageUpdate {
                message: AgentMessage::Assistant(partial.clone()),
                assistant_event: Box::new(AssistantMessageEvent::ThinkingStart {
                    content_index: 0,
                    partial: partial.clone(),
                }),
            },
        );
        handle_test_agent_event(
            "run-1",
            &event_tx,
            &current_message_id,
            &current_reasoning_message_id,
            &last_usage,
            &reasoning_buffer,
            &AgentEvent::MessageUpdate {
                message: AgentMessage::Assistant(partial.clone()),
                assistant_event: Box::new(AssistantMessageEvent::ThinkingDelta {
                    content_index: 0,
                    delta: "first thought".to_string(),
                    partial: partial.clone(),
                }),
            },
        );
        handle_test_agent_event(
            "run-1",
            &event_tx,
            &current_message_id,
            &current_reasoning_message_id,
            &last_usage,
            &reasoning_buffer,
            &AgentEvent::MessageUpdate {
                message: AgentMessage::Assistant(partial.clone()),
                assistant_event: Box::new(AssistantMessageEvent::ThinkingEnd {
                    content_index: 0,
                    content: "first thought".to_string(),
                    partial: partial.clone(),
                }),
            },
        );
        handle_test_agent_event(
            "run-1",
            &event_tx,
            &current_message_id,
            &current_reasoning_message_id,
            &last_usage,
            &reasoning_buffer,
            &AgentEvent::MessageUpdate {
                message: AgentMessage::Assistant(partial.clone()),
                assistant_event: Box::new(AssistantMessageEvent::ThinkingStart {
                    content_index: 1,
                    partial: partial.clone(),
                }),
            },
        );
        handle_test_agent_event(
            "run-1",
            &event_tx,
            &current_message_id,
            &current_reasoning_message_id,
            &last_usage,
            &reasoning_buffer,
            &AgentEvent::MessageUpdate {
                message: AgentMessage::Assistant(partial.clone()),
                assistant_event: Box::new(AssistantMessageEvent::ThinkingDelta {
                    content_index: 1,
                    delta: "second thought".to_string(),
                    partial,
                }),
            },
        );

        let events = std::iter::from_fn(|| event_rx.try_recv().ok()).collect::<Vec<_>>();
        let reasoning_events = events
            .into_iter()
            .filter_map(|event| match event {
                ThreadStreamEvent::ReasoningUpdated {
                    message_id,
                    reasoning,
                    ..
                } => Some((message_id, reasoning)),
                _ => None,
            })
            .collect::<Vec<_>>();

        assert_eq!(reasoning_events.len(), 3);
        assert_eq!(reasoning_events[0].1, "first thought");
        assert_eq!(reasoning_events[1].1, "first thought");
        assert_eq!(reasoning_events[2].1, "second thought");
        assert_eq!(reasoning_events[0].0, reasoning_events[1].0);
        assert_ne!(reasoning_events[0].0, reasoning_events[2].0);
    }

    #[test]
    fn empty_reasoning_blocks_do_not_emit_reasoning_events() {
        let (event_tx, mut event_rx) = mpsc::unbounded_channel();
        let current_message_id = StdMutex::new(None::<String>);
        let current_reasoning_message_id = StdMutex::new(None::<String>);
        let last_usage = StdMutex::new(None::<tiy_core::types::Usage>);
        let reasoning_buffer = StdMutex::new(String::new());
        let partial = sample_partial_assistant_message();

        handle_test_agent_event(
            "run-1",
            &event_tx,
            &current_message_id,
            &current_reasoning_message_id,
            &last_usage,
            &reasoning_buffer,
            &AgentEvent::MessageUpdate {
                message: AgentMessage::Assistant(partial.clone()),
                assistant_event: Box::new(AssistantMessageEvent::ThinkingStart {
                    content_index: 0,
                    partial: partial.clone(),
                }),
            },
        );
        handle_test_agent_event(
            "run-1",
            &event_tx,
            &current_message_id,
            &current_reasoning_message_id,
            &last_usage,
            &reasoning_buffer,
            &AgentEvent::MessageUpdate {
                message: AgentMessage::Assistant(partial),
                assistant_event: Box::new(AssistantMessageEvent::ThinkingEnd {
                    content_index: 0,
                    content: String::new(),
                    partial: sample_partial_assistant_message(),
                }),
            },
        );

        let reasoning_events = std::iter::from_fn(|| event_rx.try_recv().ok())
            .filter(|event| matches!(event, ThreadStreamEvent::ReasoningUpdated { .. }))
            .collect::<Vec<_>>();

        assert!(reasoning_events.is_empty());
    }

    #[test]
    fn message_end_emits_usage_updates_once_per_change() {
        let (event_tx, mut event_rx) = mpsc::unbounded_channel();
        let current_message_id = StdMutex::new(None::<String>);
        let current_reasoning_message_id = StdMutex::new(None::<String>);
        let last_usage = StdMutex::new(None::<tiy_core::types::Usage>);
        let reasoning_buffer = StdMutex::new(String::new());
        let assistant = AssistantMessage::builder()
            .api(Api::OpenAICompletions)
            .provider(Provider::OpenAI)
            .model("gpt-test")
            .usage(tiy_core::types::Usage::from_tokens(256, 32))
            .build()
            .expect("assistant message with usage");

        handle_test_agent_event(
            "run-usage",
            &event_tx,
            &current_message_id,
            &current_reasoning_message_id,
            &last_usage,
            &reasoning_buffer,
            &AgentEvent::MessageEnd {
                message: AgentMessage::Assistant(assistant.clone()),
            },
        );
        handle_test_agent_event(
            "run-usage",
            &event_tx,
            &current_message_id,
            &current_reasoning_message_id,
            &last_usage,
            &reasoning_buffer,
            &AgentEvent::MessageEnd {
                message: AgentMessage::Assistant(assistant),
            },
        );

        let usage_events = std::iter::from_fn(|| event_rx.try_recv().ok())
            .filter_map(|event| match event {
                ThreadStreamEvent::ThreadUsageUpdated { usage, .. } => Some(usage),
                _ => None,
            })
            .collect::<Vec<_>>();

        assert_eq!(usage_events.len(), 1);
        assert_eq!(usage_events[0].input_tokens, 256);
        assert_eq!(usage_events[0].output_tokens, 32);
        assert_eq!(usage_events[0].total_tokens, 288);
    }

    #[test]
    fn turn_retrying_event_emits_runtime_retry_notice() {
        let (event_tx, mut event_rx) = mpsc::unbounded_channel();
        let current_message_id = StdMutex::new(None::<String>);
        let last_completed_message_id = StdMutex::new(None::<String>);
        let current_reasoning_message_id = StdMutex::new(None::<String>);
        let last_usage = StdMutex::new(None::<tiy_core::types::Usage>);
        let reasoning_buffer = StdMutex::new(String::new());

        handle_agent_event(
            "run-retry",
            &event_tx,
            &current_message_id,
            &last_completed_message_id,
            &current_reasoning_message_id,
            &last_usage,
            &reasoning_buffer,
            TEST_CONTEXT_WINDOW,
            TEST_MODEL_DISPLAY_NAME,
            &AgentEvent::TurnRetrying {
                attempt: 1,
                max_attempts: 3,
                delay_ms: 1_000,
                reason: "Incomplete anthropic stream: missing message_stop".to_string(),
            },
        );

        let events = std::iter::from_fn(|| event_rx.try_recv().ok()).collect::<Vec<_>>();
        assert!(matches!(
            events.as_slice(),
            [ThreadStreamEvent::RunRetrying {
                run_id,
                attempt: 1,
                max_attempts: 3,
                delay_ms: 1_000,
                reason,
            }] if run_id == "run-retry" && reason.contains("Incomplete anthropic stream")
        ));
    }

    #[test]
    fn message_discarded_reuses_last_completed_assistant_message_id() {
        let (event_tx, mut event_rx) = mpsc::unbounded_channel();
        let current_message_id = StdMutex::new(None::<String>);
        let last_completed_message_id = StdMutex::new(None::<String>);
        let current_reasoning_message_id = StdMutex::new(None::<String>);
        let last_usage = StdMutex::new(None::<tiy_core::types::Usage>);
        let reasoning_buffer = StdMutex::new(String::new());
        let assistant = sample_partial_assistant_message();

        handle_agent_event(
            "run-discard",
            &event_tx,
            &current_message_id,
            &last_completed_message_id,
            &current_reasoning_message_id,
            &last_usage,
            &reasoning_buffer,
            TEST_CONTEXT_WINDOW,
            TEST_MODEL_DISPLAY_NAME,
            &AgentEvent::MessageEnd {
                message: AgentMessage::Assistant(assistant.clone()),
            },
        );
        handle_agent_event(
            "run-discard",
            &event_tx,
            &current_message_id,
            &last_completed_message_id,
            &current_reasoning_message_id,
            &last_usage,
            &reasoning_buffer,
            TEST_CONTEXT_WINDOW,
            TEST_MODEL_DISPLAY_NAME,
            &AgentEvent::MessageDiscarded {
                message: AgentMessage::Assistant(assistant),
                reason: "Incomplete anthropic stream: missing message_stop".to_string(),
            },
        );

        let events = std::iter::from_fn(|| event_rx.try_recv().ok()).collect::<Vec<_>>();
        let completed_message_id = events.iter().find_map(|event| match event {
            ThreadStreamEvent::MessageCompleted { message_id, .. } => Some(message_id.clone()),
            _ => None,
        });
        let discarded_message_id = events.iter().find_map(|event| match event {
            ThreadStreamEvent::MessageDiscarded { message_id, .. } => Some(message_id.clone()),
            _ => None,
        });

        assert!(completed_message_id.is_some());
        assert_eq!(completed_message_id, discarded_message_id);
    }

    #[test]
    fn helper_profiles_match_explore_and_review_tools() {
        assert_eq!(
            resolve_helper_profile(RuntimeOrchestrationTool::Explore),
            SubagentProfile::Explore,
        );
        assert_eq!(
            resolve_helper_profile(RuntimeOrchestrationTool::Review),
            SubagentProfile::Review,
        );
    }

    #[test]
    fn update_plan_tool_is_available_in_both_runtime_profiles() {
        for profile in [DEFAULT_FULL_TOOL_PROFILE, PLAN_READ_ONLY_TOOL_PROFILE] {
            let tools = runtime_tools_for_profile(profile);
            let tool_names: Vec<&str> = tools.iter().map(|tool| tool.name.as_str()).collect();

            assert!(tool_names.contains(&"update_plan"));
        }
    }

    #[test]
    fn clarify_tool_is_available_in_both_runtime_profiles() {
        for profile in [DEFAULT_FULL_TOOL_PROFILE, PLAN_READ_ONLY_TOOL_PROFILE] {
            let tools = runtime_tools_for_profile(profile);
            let tool_names: Vec<&str> = tools.iter().map(|tool| tool.name.as_str()).collect();

            assert!(tool_names.contains(&"clarify"));
        }
    }

    #[test]
    fn update_plan_tool_schema_no_longer_exposes_open_questions() {
        let tools = runtime_tools_for_profile(DEFAULT_FULL_TOOL_PROFILE);
        let update_plan = tools
            .iter()
            .find(|tool| tool.name == "update_plan")
            .expect("update_plan tool should exist");
        let properties = update_plan.parameters["properties"]
            .as_object()
            .expect("update_plan properties should be object");

        assert!(!properties.contains_key("openQuestions"));
    }

    #[test]
    fn update_plan_tool_schema_exposes_structured_plan_sections() {
        let tools = runtime_tools_for_profile(DEFAULT_FULL_TOOL_PROFILE);
        let update_plan = tools
            .iter()
            .find(|tool| tool.name == "update_plan")
            .expect("update_plan tool should exist");
        let properties = update_plan.parameters["properties"]
            .as_object()
            .expect("update_plan properties should be object");

        assert!(properties.contains_key("context"));
        assert!(properties.contains_key("design"));
        assert!(properties.contains_key("keyImplementation"));
        assert!(properties.contains_key("verification"));
        assert!(properties.contains_key("assumptions"));

        let nested_plan_properties = update_plan.parameters["properties"]["plan"]["properties"]
            .as_object()
            .expect("nested plan properties should be object");
        assert!(nested_plan_properties.contains_key("context"));
        assert!(nested_plan_properties.contains_key("design"));
        assert!(nested_plan_properties.contains_key("keyImplementation"));
        assert!(nested_plan_properties.contains_key("verification"));
        assert!(nested_plan_properties.contains_key("assumptions"));
    }

    #[test]
    fn explore_and_review_use_auxiliary_model_when_available() {
        let model_plan =
            sample_resolved_runtime_model_plan(Some(sample_resolved_model_role("assistant-model")));

        let explore_role =
            resolve_helper_model_role(&model_plan, RuntimeOrchestrationTool::Explore);
        let review_role = resolve_helper_model_role(&model_plan, RuntimeOrchestrationTool::Review);

        assert_eq!(explore_role.model_id, "assistant-model");
        assert_eq!(review_role.model_id, "assistant-model");
    }

    #[test]
    fn explore_and_review_fallback_to_primary_without_auxiliary_model() {
        let model_plan = sample_resolved_runtime_model_plan(None);

        let explore_role =
            resolve_helper_model_role(&model_plan, RuntimeOrchestrationTool::Explore);
        let review_role = resolve_helper_model_role(&model_plan, RuntimeOrchestrationTool::Review);

        assert_eq!(explore_role.model_id, "primary-model");
        assert_eq!(review_role.model_id, "primary-model");
    }

    #[test]
    fn plan_mode_prompt_mentions_waiting_for_approval_after_update_plan() {
        let prompt = run_mode_prompt_body("plan");

        assert!(prompt.contains("clarify"));
        assert!(prompt.contains("a prose answer alone does not complete the run"));
        assert!(prompt.contains("must call update_plan"));
        assert!(prompt.contains("Do not include unresolved questions"));
        assert!(prompt.contains("Once you publish a plan with update_plan"));
        assert!(prompt.contains(
            "Structure the plan as: summary, context, design, keyImplementation, ordered steps, verification, and risks."
        ));
        assert!(prompt.contains("In `design`, describe the recommended approach"));
        assert!(prompt.contains("In `verification`, include how the change will be validated"));
        assert!(prompt.contains("pause for user approval"));
    }

    #[test]
    fn default_mode_prompt_mentions_clarify_for_missing_information() {
        let prompt = run_mode_prompt_body("default");

        assert!(prompt.contains("use clarify instead of guessing"));
        assert!(prompt.contains("multiple reasonable approaches"));
        assert!(prompt.contains("approve a risky action"));
    }

    #[test]
    fn plan_mode_requires_checkpoint_before_successful_completion() {
        assert_eq!(
            plan_mode_missing_checkpoint_error("plan", false),
            Some(PLAN_MODE_MISSING_CHECKPOINT_ERROR)
        );
        assert_eq!(plan_mode_missing_checkpoint_error("plan", true), None);
        assert_eq!(plan_mode_missing_checkpoint_error("default", false), None);
    }

    #[test]
    fn build_plan_artifact_extracts_numbered_steps() {
        let artifact = build_plan_artifact_from_tool_input(
            &serde_json::json!({
                "title": "Implementation Plan",
                "summary": "Produce the implementation plan.",
                "steps": [
                    "Update runtime-thread-surface.",
                    { "title": "Validate typecheck." }
                ]
            }),
            3,
        );

        assert_eq!(artifact.title, "Implementation Plan");
        assert_eq!(artifact.plan_revision, 3);
        assert_eq!(artifact.steps[0].title, "Update runtime-thread-surface.");
        assert_eq!(artifact.steps[1].title, "Validate typecheck.");
    }

    #[test]
    fn convert_history_messages_keeps_plan_checkpoints_but_skips_approval_prompts() {
        let artifact = build_plan_artifact_from_tool_input(
            &serde_json::json!({
                "title": "Plan title",
                "summary": "Carry the previous plan forward.",
                "steps": ["Keep the plan in follow-up context."]
            }),
            2,
        );
        let plan_metadata = build_plan_message_metadata(artifact, "run-plan", "plan");
        let messages = vec![
            MessageRecord {
                id: "msg-user".to_string(),
                thread_id: "thread-1".to_string(),
                run_id: None,
                role: "user".to_string(),
                content_markdown: "Please refine the plan.".to_string(),
                message_type: "plain_message".to_string(),
                status: "completed".to_string(),
                metadata_json: None,
                attachments_json: None,
                created_at: String::new(),
            },
            MessageRecord {
                id: "msg-plan".to_string(),
                thread_id: "thread-1".to_string(),
                run_id: Some("run-plan".to_string()),
                role: "assistant".to_string(),
                content_markdown: "stale plan body".to_string(),
                message_type: "plan".to_string(),
                status: "completed".to_string(),
                metadata_json: Some(
                    serde_json::to_string(&plan_metadata).expect("serialize plan metadata"),
                ),
                attachments_json: None,
                created_at: String::new(),
            },
            MessageRecord {
                id: "msg-approval".to_string(),
                thread_id: "thread-1".to_string(),
                run_id: Some("run-plan".to_string()),
                role: "assistant".to_string(),
                content_markdown: "Review and approve the plan.".to_string(),
                message_type: "approval_prompt".to_string(),
                status: "completed".to_string(),
                metadata_json: None,
                attachments_json: None,
                created_at: String::new(),
            },
        ];

        let history =
            convert_history_messages(&messages, &sample_resolved_model_role("primary").model);

        assert_eq!(history.len(), 2);
        assert_eq!(message_text(&history[0]), "Please refine the plan.");
        assert!(message_text(&history[1])
            .contains("Implementation plan checkpoint (revision 2, approval state: pending):"));
        assert!(message_text(&history[1]).contains("# Plan title"));
        assert!(message_text(&history[1]).contains("Keep the plan in follow-up context."));
    }

    #[test]
    fn convert_history_messages_uses_effective_prompt_for_command_messages() {
        let messages = vec![MessageRecord {
            id: "msg-command".to_string(),
            thread_id: "thread-1".to_string(),
            run_id: None,
            role: "user".to_string(),
            content_markdown: "/init".to_string(),
            message_type: "plain_message".to_string(),
            status: "completed".to_string(),
            metadata_json: Some(
                serde_json::json!({
                    "composer": {
                        "kind": "command",
                        "displayText": "/init",
                        "effectivePrompt": "Generate or update a file named AGENTS.md."
                    }
                })
                .to_string(),
            ),
            attachments_json: None,
            created_at: String::new(),
        }];

        let history =
            convert_history_messages(&messages, &sample_resolved_model_role("primary").model);

        assert_eq!(history.len(), 1);
        assert_eq!(
            message_text(&history[0]),
            "Generate or update a file named AGENTS.md."
        );
    }

    #[test]
    fn convert_history_messages_includes_image_and_text_attachments() {
        let messages = vec![MessageRecord {
            id: "msg-attachment".to_string(),
            thread_id: "thread-1".to_string(),
            run_id: None,
            role: "user".to_string(),
            content_markdown: "Please inspect these files.".to_string(),
            message_type: "plain_message".to_string(),
            status: "completed".to_string(),
            metadata_json: None,
            attachments_json: Some(
                serde_json::json!([
                    {
                        "id": "image-1",
                        "name": "diagram.png",
                        "mediaType": "image/png",
                        "url": "data:image/png;base64,aGVsbG8="
                    },
                    {
                        "id": "text-1",
                        "name": "notes.md",
                        "mediaType": "text/markdown",
                        "url": "data:text/markdown;base64,IyBIZWFkZXIKCkJvZHkgbGluZS4="
                    }
                ])
                .to_string(),
            ),
            created_at: String::new(),
        }];

        let history = convert_history_messages(
            &messages,
            &sample_resolved_model_role_with_inputs(
                "vision-model",
                vec![
                    tiy_core::types::InputType::Text,
                    tiy_core::types::InputType::Image,
                ],
            )
            .model,
        );

        assert_eq!(history.len(), 1);
        let blocks = user_blocks(&history[0]);
        assert_eq!(blocks.len(), 3);

        match &blocks[0] {
            ContentBlock::Text(text) => assert_eq!(text.text, "Please inspect these files."),
            _ => panic!("expected prompt text block"),
        }

        match &blocks[1] {
            ContentBlock::Image(image) => {
                assert_eq!(image.mime_type, "image/png");
                assert_eq!(image.data, "aGVsbG8=");
            }
            _ => panic!("expected image block"),
        }

        match &blocks[2] {
            ContentBlock::Text(text) => {
                assert!(text.text.contains("[Text attachment: notes.md]"));
                assert!(text.text.contains("~~~markdown"));
                assert!(text.text.contains("# Header"));
                assert!(text.text.contains("Body line."));
            }
            _ => panic!("expected text attachment block"),
        }
    }

    #[test]
    fn convert_history_messages_falls_back_to_text_for_unsupported_image_models() {
        let messages = vec![MessageRecord {
            id: "msg-image".to_string(),
            thread_id: "thread-1".to_string(),
            run_id: None,
            role: "user".to_string(),
            content_markdown: "Describe this image.".to_string(),
            message_type: "plain_message".to_string(),
            status: "completed".to_string(),
            metadata_json: None,
            attachments_json: Some(
                serde_json::json!([
                    {
                        "id": "image-1",
                        "name": "photo.png",
                        "mediaType": "image/png",
                        "url": "data:image/png;base64,aGVsbG8="
                    }
                ])
                .to_string(),
            ),
            created_at: String::new(),
        }];

        let history =
            convert_history_messages(&messages, &sample_resolved_model_role("primary").model);

        assert_eq!(history.len(), 1);
        let blocks = user_blocks(&history[0]);
        assert_eq!(blocks.len(), 2);

        match &blocks[1] {
            ContentBlock::Text(text) => {
                assert_eq!(text.text, "[Image attachment: photo.png]");
            }
            _ => panic!("expected text fallback block"),
        }
    }

    #[test]
    fn trim_history_to_current_context_keeps_only_messages_after_latest_reset() {
        let messages = vec![
            MessageRecord {
                id: "msg-before".to_string(),
                thread_id: "thread-1".to_string(),
                run_id: Some("run-before".to_string()),
                role: "assistant".to_string(),
                content_markdown: "Old assistant reply".to_string(),
                message_type: "plain_message".to_string(),
                status: "completed".to_string(),
                metadata_json: None,
                attachments_json: None,
                created_at: String::new(),
            },
            MessageRecord {
                id: "msg-reset".to_string(),
                thread_id: "thread-1".to_string(),
                run_id: None,
                role: "system".to_string(),
                content_markdown: "Context is now reset".to_string(),
                message_type: "summary_marker".to_string(),
                status: "completed".to_string(),
                metadata_json: Some(
                    serde_json::json!({
                        "kind": "context_reset",
                    })
                    .to_string(),
                ),
                attachments_json: None,
                created_at: String::new(),
            },
            MessageRecord {
                id: "msg-summary".to_string(),
                thread_id: "thread-1".to_string(),
                run_id: None,
                role: "system".to_string(),
                content_markdown: "<context_summary>\nCarry this forward.\n</context_summary>"
                    .to_string(),
                message_type: "summary_marker".to_string(),
                status: "completed".to_string(),
                metadata_json: Some(
                    serde_json::json!({
                        "kind": "context_summary",
                    })
                    .to_string(),
                ),
                attachments_json: None,
                created_at: String::new(),
            },
            MessageRecord {
                id: "msg-after".to_string(),
                thread_id: "thread-1".to_string(),
                run_id: None,
                role: "user".to_string(),
                content_markdown: "New request".to_string(),
                message_type: "plain_message".to_string(),
                status: "completed".to_string(),
                metadata_json: None,
                attachments_json: None,
                created_at: String::new(),
            },
        ];

        let trimmed = trim_history_to_current_context(&messages);

        assert_eq!(trimmed.len(), 2);
        assert_eq!(trimmed[0].id, "msg-summary");
        assert_eq!(trimmed[1].id, "msg-after");
    }

    #[test]
    fn convert_history_messages_keeps_context_summary_but_skips_reset_markers() {
        let messages = vec![
            MessageRecord {
                id: "msg-reset".to_string(),
                thread_id: "thread-1".to_string(),
                run_id: None,
                role: "system".to_string(),
                content_markdown: "Context is now reset".to_string(),
                message_type: "summary_marker".to_string(),
                status: "completed".to_string(),
                metadata_json: Some(
                    serde_json::json!({
                        "kind": "context_reset",
                    })
                    .to_string(),
                ),
                attachments_json: None,
                created_at: String::new(),
            },
            MessageRecord {
                id: "msg-summary".to_string(),
                thread_id: "thread-1".to_string(),
                run_id: None,
                role: "system".to_string(),
                content_markdown: "<context_summary>\nCarry this forward.\n</context_summary>"
                    .to_string(),
                message_type: "summary_marker".to_string(),
                status: "completed".to_string(),
                metadata_json: Some(
                    serde_json::json!({
                        "kind": "context_summary",
                    })
                    .to_string(),
                ),
                attachments_json: None,
                created_at: String::new(),
            },
        ];

        let history =
            convert_history_messages(&messages, &sample_resolved_model_role("primary").model);

        assert_eq!(history.len(), 1);
        assert_eq!(
            message_text(&history[0]),
            "<context_summary>\nCarry this forward.\n</context_summary>"
        );
    }
}
