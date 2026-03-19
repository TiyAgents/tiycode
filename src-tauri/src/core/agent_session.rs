use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex as StdMutex, Weak};

use sqlx::SqlitePool;
use tokio::sync::mpsc;
use tiy_core::agent::{Agent, AgentMessage, AgentTool, AgentToolResult, ToolExecutionMode};
use tiy_core::thinking::ThinkingLevel;
use tiy_core::types::{
    AssistantMessage, AssistantMessageEvent, ContentBlock, Cost, InputType, Model, Provider,
    StopReason, TextContent, Transport, Usage, UserMessage,
};

use crate::core::helper_agent_orchestrator::{HelperAgentOrchestrator, HelperRunRequest};
use crate::core::tool_gateway::{
    ApprovalRequest, ToolExecutionOptions, ToolExecutionRequest, ToolGateway, ToolGatewayResult,
};
use crate::ipc::frontend_channels::ThreadStreamEvent;
use crate::model::errors::{AppError, ErrorSource};
use crate::model::thread::MessageRecord;
use crate::persistence::repo::{message_repo, profile_repo, provider_repo, tool_call_repo};

const MESSAGE_HISTORY_LIMIT: i64 = 200;
const DEFAULT_CONTEXT_WINDOW: u32 = 128_000;
const DEFAULT_MAX_OUTPUT_TOKENS: u32 = 16_384;
const DEFAULT_FULL_TOOL_PROFILE: &str = "default_full";
const PLAN_READ_ONLY_TOOL_PROFILE: &str = "plan_read_only";

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
    pub custom_headers: Option<HashMap<String, String>>,
    pub provider_options: Option<serde_json::Value>,
}

#[derive(Debug, Clone, serde::Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeModelPlan {
    pub profile_id: Option<String>,
    pub profile_name: Option<String>,
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
    let history_messages = message_repo::list_recent(pool, thread_id, None, MESSAGE_HISTORY_LIMIT).await?;
    let system_prompt = build_system_prompt(pool, &raw_plan, run_mode).await?;

    Ok(AgentSessionSpec {
        run_id: run_id.to_string(),
        thread_id: thread_id.to_string(),
        workspace_path: workspace_path.to_string(),
        run_mode: run_mode.to_string(),
        tool_profile_name: resolve_tool_profile_name(&raw_plan, run_mode),
        system_prompt,
        history_messages,
        model_plan: resolved_plan,
    })
}

pub struct AgentSession {
    spec: AgentSessionSpec,
    pool: SqlitePool,
    tool_gateway: Arc<ToolGateway>,
    helper_orchestrator: Arc<HelperAgentOrchestrator>,
    event_tx: mpsc::UnboundedSender<ThreadStreamEvent>,
    agent: Arc<Agent>,
    cancel_requested: Arc<AtomicBool>,
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
            configure_agent(&agent, &spec, weak_self.clone());

            Self {
                spec,
                pool,
                tool_gateway,
                helper_orchestrator,
                event_tx,
                agent,
                cancel_requested: Arc::new(AtomicBool::new(false)),
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
        let reasoning_buffer = Arc::new(StdMutex::new(String::new()));
        let run_id = self.spec.run_id.clone();
        let event_tx = self.event_tx.clone();

        let message_id_ref = Arc::clone(&current_message_id);
        let reasoning_ref = Arc::clone(&reasoning_buffer);
        let unsubscribe = self.agent.subscribe(move |event| {
            handle_agent_event(
                &run_id,
                &event_tx,
                &message_id_ref,
                &reasoning_ref,
                event,
            );
        });

        let _ = self.event_tx.send(ThreadStreamEvent::RunStarted {
            run_id: self.spec.run_id.clone(),
            run_mode: self.spec.run_mode.clone(),
        });

        let result = self.agent.continue_().await;
        unsubscribe();

        match result {
            Ok(_) => {
                if self.cancel_requested.load(Ordering::SeqCst) {
                    let _ = self.event_tx.send(ThreadStreamEvent::RunCancelled {
                        run_id: self.spec.run_id.clone(),
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

        let _ = self.event_tx.send(ThreadStreamEvent::ToolRequested {
            run_id: self.spec.run_id.clone(),
            tool_call_id: tool_call_id.to_string(),
            tool_name: tool_name.to_string(),
            tool_input: tool_input.clone(),
        });

        if is_helper_tool(tool_name) {
            return self
                .execute_helper_tool(tool_name, tool_call_id, tool_input)
                .await;
        }

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
        let outcome = self
            .tool_gateway
            .execute_tool_call(
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

    async fn execute_helper_tool(
        &self,
        tool_name: &str,
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
            return agent_error_result("missing helper task");
        }

        let _ = self.event_tx.send(ThreadStreamEvent::ToolRunning {
            run_id: self.spec.run_id.clone(),
            tool_call_id: tool_call_id.to_string(),
        });

        let helper_role = self
            .spec
            .model_plan
            .auxiliary
            .clone()
            .unwrap_or_else(|| self.spec.model_plan.primary.clone());

        let result = self
            .helper_orchestrator
            .run_helper(HelperRunRequest {
                run_id: self.spec.run_id.clone(),
                thread_id: self.spec.thread_id.clone(),
                helper_kind: tool_name.to_string(),
                parent_tool_call_id: Some(tool_call_id.to_string()),
                task,
                model_role: helper_role,
                system_prompt: self.spec.system_prompt.clone(),
                workspace_path: self.spec.workspace_path.clone(),
                run_mode: self.spec.run_mode.clone(),
                event_tx: self.event_tx.clone(),
            })
            .await;

        match result {
            Ok(summary) => {
                let result = serde_json::json!({ "summary": summary.summary.clone() });
                tool_call_repo::update_result(
                    &self.pool,
                    tool_call_id,
                    &result.to_string(),
                    "completed",
                )
                .await
                .ok();

                let _ = self.event_tx.send(ThreadStreamEvent::ToolCompleted {
                    run_id: self.spec.run_id.clone(),
                    tool_call_id: tool_call_id.to_string(),
                    result: result.clone(),
                });

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

                let _ = self.event_tx.send(ThreadStreamEvent::ToolFailed {
                    run_id: self.spec.run_id.clone(),
                    tool_call_id: tool_call_id.to_string(),
                    error: error.to_string(),
                });

                agent_error_result(error.to_string())
            }
        }
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
    reasoning_buffer: &StdMutex<String>,
    event: &tiy_core::agent::AgentEvent,
) {
    match event {
        tiy_core::agent::AgentEvent::MessageUpdate {
            assistant_event, ..
        } => match assistant_event.as_ref() {
            AssistantMessageEvent::TextDelta { delta, .. } => {
                let message_id = ensure_message_id(current_message_id);
                let _ = event_tx.send(ThreadStreamEvent::MessageDelta {
                    run_id: run_id.to_string(),
                    message_id,
                    delta: delta.clone(),
                });
            }
            AssistantMessageEvent::ThinkingDelta { delta, .. } => {
                if let Ok(mut buffer) = reasoning_buffer.lock() {
                    buffer.push_str(delta);
                    let _ = event_tx.send(ThreadStreamEvent::ReasoningUpdated {
                        run_id: run_id.to_string(),
                        reasoning: buffer.clone(),
                    });
                }
            }
            _ => {}
        },
        tiy_core::agent::AgentEvent::MessageEnd { message } => {
            if let AgentMessage::Assistant(assistant) = message {
                let content = assistant.text_content();
                if content.is_empty() && assistant.has_tool_calls() {
                    reset_message_id(current_message_id);
                    return;
                }

                let message_id = take_or_create_message_id(current_message_id);
                let _ = event_tx.send(ThreadStreamEvent::MessageCompleted {
                    run_id: run_id.to_string(),
                    message_id,
                    content,
                });
            }
        }
        _ => {}
    }
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

fn is_helper_tool(tool_name: &str) -> bool {
    matches!(
        tool_name,
        "delegate_research" | "delegate_plan_review" | "delegate_code_review"
    )
}

fn runtime_tools_for_profile(profile_name: &str) -> Vec<AgentTool> {
    let mut tools = vec![
        AgentTool::new(
            "read_file",
            "Read File",
            "Read a file inside the current workspace.",
            serde_json::json!({
                "type": "object",
                "properties": { "path": { "type": "string" } },
                "required": ["path"]
            }),
        ),
        AgentTool::new(
            "list_dir",
            "List Directory",
            "List files and folders inside the current workspace.",
            serde_json::json!({
                "type": "object",
                "properties": { "path": { "type": "string" } }
            }),
        ),
        AgentTool::new(
            "search_repo",
            "Search Repo",
            "Search the current workspace with ripgrep.",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "query": { "type": "string" },
                    "directory": { "type": "string" },
                    "filePattern": { "type": "string" }
                },
                "required": ["query"]
            }),
        ),
        AgentTool::new(
            "terminal_get_status",
            "Terminal Status",
            "Inspect the current thread terminal status without mutating it.",
            serde_json::json!({
                "type": "object",
                "properties": {}
            }),
        ),
        AgentTool::new(
            "terminal_get_recent_output",
            "Terminal Output",
            "Read the recent terminal output for the current thread.",
            serde_json::json!({
                "type": "object",
                "properties": {}
            }),
        ),
        AgentTool::new(
            "delegate_research",
            "Delegate Research",
            "Run a scoped helper agent to investigate a question and return a summary.",
            serde_json::json!({
                "type": "object",
                "properties": { "task": { "type": "string" } },
                "required": ["task"]
            }),
        ),
        AgentTool::new(
            "delegate_plan_review",
            "Delegate Plan Review",
            "Run a scoped helper agent to review a plan and return a summary.",
            serde_json::json!({
                "type": "object",
                "properties": { "task": { "type": "string" } },
                "required": ["task"]
            }),
        ),
        AgentTool::new(
            "delegate_code_review",
            "Delegate Code Review",
            "Run a scoped helper agent to review code and return a summary.",
            serde_json::json!({
                "type": "object",
                "properties": { "task": { "type": "string" } },
                "required": ["task"]
            }),
        ),
    ];

    if profile_name == DEFAULT_FULL_TOOL_PROFILE {
        tools.push(AgentTool::new(
            "write_file",
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
            "run_command",
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
    }

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

fn convert_history_messages(messages: &[MessageRecord], model: &Model) -> Vec<AgentMessage> {
    messages
        .iter()
        .filter_map(|message| match message.role.as_str() {
            "user" => Some(AgentMessage::User(UserMessage::text(
                message.content_markdown.clone(),
            ))),
            "assistant" => Some(AgentMessage::Assistant(assistant_message_from_text(
                &message.content_markdown,
                model,
            ))),
            _ => None,
        })
        .collect()
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

fn agent_tool_result_from_output(output: crate::core::executors::ToolOutput) -> AgentToolResult {
    let rendered = serde_json::to_string_pretty(&output.result).unwrap_or_else(|_| output.result.to_string());

    if output.success {
        AgentToolResult {
            content: vec![ContentBlock::Text(TextContent::new(rendered))],
            details: Some(output.result),
        }
    } else {
        AgentToolResult {
            content: vec![ContentBlock::Text(TextContent::new(format!("Error: {rendered}")))],
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

    let primary = resolve_model_role(pool, primary).await?;
    let auxiliary = match raw_plan.auxiliary.clone() {
        Some(role) => Some(resolve_model_role(pool, role).await?),
        None => None,
    };
    let lightweight = match raw_plan.lightweight.clone() {
        Some(role) => Some(resolve_model_role(pool, role).await?),
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

async fn resolve_model_role(
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
    let max_output_tokens = parse_positive_u32(
        role.max_output_tokens.as_deref(),
        DEFAULT_MAX_OUTPUT_TOKENS,
    );

    let mut builder = Model::builder()
        .id(&role.model)
        .name(&model_name)
        .provider(Provider::from(role.provider_type.clone()))
        .base_url(&role.base_url)
        .context_window(context_window)
        .max_tokens(max_output_tokens)
        .input(vec![InputType::Text])
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
    run_mode: &str,
) -> Result<String, AppError> {
    let mut parts = vec![
        "You are Tiy Agent, a coding assistant embedded in the user's desktop workspace."
            .to_string(),
    ];

    if let Some(profile_id) = raw_plan.profile_id.as_deref() {
        if let Some(profile) = profile_repo::find_by_id(pool, profile_id).await? {
            if let Some(custom_instructions) = profile.custom_instructions {
                if !custom_instructions.trim().is_empty() {
                    parts.push(custom_instructions);
                }
            }
        }
    }

    if run_mode == "plan" {
        parts.push(
            "Plan mode is active. Prefer analysis, explanation, and read-only tooling. Do not use mutating tools unless the user explicitly starts a new execution run."
                .to_string(),
        );
    }

    Ok(parts.join("\n\n"))
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
