use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex as StdMutex, Weak};

use sqlx::SqlitePool;
use tiycore::agent::{Agent, AgentError, ToolExecutionMode};
use tiycore::thinking::ThinkingLevel;
use tiycore::types::{Cost, InputType, Model, Provider, Usage};
use tokio::sync::mpsc;

use crate::core::prompt;
use crate::core::subagent::HelperAgentOrchestrator;
use crate::core::tool_gateway::ToolGateway;
use crate::extensions::ExtensionsManager;
use crate::ipc::frontend_channels::ThreadStreamEvent;
use crate::model::errors::{AppError, ErrorSource};
use crate::persistence::repo::{message_repo, provider_repo, run_repo, tool_call_repo};

/// Deprecated: previously used as the hard limit for `message_repo::list_recent` in
/// `build_session_spec`.  Replaced by `message_repo::list_since_last_reset()` which
/// queries the DB for the reset boundary directly.  Retained for reference only.
#[allow(dead_code)]
const MESSAGE_HISTORY_LIMIT: i64 = 200;
pub(crate) const DEFAULT_CONTEXT_WINDOW: u32 = 128_000;
const DEFAULT_MAX_OUTPUT_TOKENS: u32 = 32_000;
pub(crate) const DEFAULT_FULL_TOOL_PROFILE: &str = "default_full";
pub(crate) const PLAN_READ_ONLY_TOOL_PROFILE: &str = "plan_read_only";
const STANDARD_TOOL_TIMEOUT_SECS: u64 = 120;
const SUBAGENT_TOOL_TIMEOUT_SECS: u64 = 600;
/// Main agent timeout is effectively unlimited (24 h) because user-interactive
/// tools like `clarify` and approval prompts must wait for human input without
/// being killed by the outer tiycore timeout.  The per-tool execution timeout
/// inside `tool_gateway` (120 s) still guards against runaway non-interactive
/// tool calls.
const MAIN_AGENT_TOOL_TIMEOUT_SECS: u64 = 86_400;
pub(crate) const CLARIFY_TOOL_NAME: &str = "clarify";
pub(crate) const PLAN_MODE_MISSING_CHECKPOINT_ERROR: &str =
    "Plan mode requires publishing a plan with update_plan before the run can finish.";
pub(crate) const TEXT_ATTACHMENT_MAX_CHARS: usize = 12_000;

use crate::core::agent_session_compression::{
    build_initial_context_token_calibration, current_context_token_calibration,
    persist_compression_markers_to_pool, record_pending_prompt_estimate, run_auto_compression,
    ContextCompressionRuntimeState,
};
use crate::core::agent_session_events::handle_agent_event;
pub(crate) use crate::core::agent_session_history::*;
pub(crate) use crate::core::agent_session_tools::*;
pub use crate::core::agent_session_types::*;

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
    let tool_profile_name = resolve_tool_profile_name(&raw_plan, run_mode);
    // Load all messages since the last context reset marker (or all messages
    // if no reset exists).  The new `list_since_last_reset` queries the DB
    // directly for the reset boundary, removing the former hard-coded 200-row
    // limit (`MESSAGE_HISTORY_LIMIT`) that could silently discard older
    // context_reset markers.
    let history_messages = message_repo::list_since_last_reset(pool, thread_id).await?;

    // Load tool calls from all runs referenced by the history messages so that
    // convert_history_messages can reconstruct assistant-tool-call / tool-result
    // pairs that are not stored in the messages table.
    let history_run_ids: Vec<String> = history_messages
        .iter()
        .filter_map(|m| m.run_id.clone())
        .collect::<std::collections::HashSet<_>>()
        .into_iter()
        .collect();
    let history_tool_calls =
        tool_call_repo::list_parent_visible_by_run_ids(pool, &history_run_ids).await?;
    let latest_historical_run =
        run_repo::find_latest_with_prompt_usage_by_thread_excluding_run(pool, thread_id, run_id)
            .await?;

    let system_prompt = build_system_prompt(pool, &raw_plan, workspace_path, run_mode).await?;
    let extension_tools = ExtensionsManager::new(pool.clone())
        .list_runtime_agent_tools(Some(workspace_path))
        .await?;
    let initial_context_calibration = build_initial_context_token_calibration(
        latest_historical_run.as_ref(),
        &history_messages,
        &history_tool_calls,
        &resolved_plan.primary,
        &system_prompt,
    );

    Ok(AgentSessionSpec {
        run_id: run_id.to_string(),
        thread_id: thread_id.to_string(),
        workspace_path: workspace_path.to_string(),
        run_mode: run_mode.to_string(),
        tool_profile_name: tool_profile_name.clone(),
        runtime_tools: runtime_tools_for_profile_with_extensions(
            &tool_profile_name,
            extension_tools,
        ),
        system_prompt,
        history_messages,
        history_tool_calls,
        model_plan: resolved_plan,
        initial_prompt: None,
        initial_context_calibration,
    })
}

pub struct AgentSession {
    pub(crate) spec: AgentSessionSpec,
    pub(crate) pool: SqlitePool,
    pub(crate) tool_gateway: Arc<ToolGateway>,
    pub(crate) helper_orchestrator: Arc<HelperAgentOrchestrator>,
    pub(crate) event_tx: mpsc::UnboundedSender<ThreadStreamEvent>,
    pub(crate) agent: Arc<Agent>,
    cancel_requested: Arc<AtomicBool>,
    pub(crate) checkpoint_requested: AtomicBool,
    pub(crate) abort_signal: tiycore::agent::AbortSignal,
    context_compression_state: Arc<StdMutex<ContextCompressionRuntimeState>>,
}

impl AgentSession {
    pub fn new(
        pool: SqlitePool,
        tool_gateway: Arc<ToolGateway>,
        helper_orchestrator: Arc<HelperAgentOrchestrator>,
        event_tx: mpsc::UnboundedSender<ThreadStreamEvent>,
        spec: AgentSessionSpec,
        max_turns: usize,
    ) -> Arc<Self> {
        Arc::new_cyclic(|weak_self| {
            let agent = Arc::new(Agent::with_model(spec.model_plan.primary.model.clone()));
            let context_compression_state = Arc::new(StdMutex::new(
                ContextCompressionRuntimeState::new(spec.initial_context_calibration),
            ));
            agent.set_max_turns(max_turns);
            configure_agent(
                &agent,
                &spec,
                weak_self.clone(),
                Arc::clone(&context_compression_state),
            );

            Self {
                spec,
                pool,
                tool_gateway,
                helper_orchestrator,
                event_tx,
                agent,
                cancel_requested: Arc::new(AtomicBool::new(false)),
                checkpoint_requested: AtomicBool::new(false),
                abort_signal: tiycore::agent::AbortSignal::new(),
                context_compression_state,
            }
        })
    }

    pub fn start(self: Arc<Self>) -> tokio::task::JoinHandle<()> {
        tokio::spawn(async move {
            self.run().await;
        })
    }

    pub async fn cancel(&self) {
        self.cancel_requested.store(true, Ordering::SeqCst);
        // 1. Cascade-cancel all in-flight tool_gateway calls (including subagent ones
        //    via child tokens) so they return Cancelled immediately.
        self.abort_signal.cancel();
        // 2. Abort subagent Agent instances so their LLM streaming / run loops stop.
        self.helper_orchestrator.cancel_run(&self.spec.run_id).await;
        // 3. Yield to let already-aborted subagents finish cleanup and emit
        //    SubagentFailed before the main agent terminates.
        tokio::task::yield_now().await;
        // 4. Finally abort the main agent.
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
        let context_compression_state_ref = Arc::clone(&self.context_compression_state);
        let current_turn_index: Arc<StdMutex<Option<usize>>> = Arc::new(StdMutex::new(None));
        let turn_index_ref = Arc::clone(&current_turn_index);
        let unsubscribe = self.agent.subscribe(move |event| {
            handle_agent_event(
                &run_id,
                &event_tx,
                &message_id_ref,
                &last_completed_message_id_ref,
                &reasoning_message_id_ref,
                &last_usage_ref,
                &context_compression_state_ref,
                &reasoning_ref,
                &turn_index_ref,
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

    /// Persist context_summary and context_reset markers to the DB after
    /// auto-compression.
    ///
    /// `boundary_message_id` is the id of the **first DB-backed message we
    /// intend to keep** in the next run's loaded history. It is stored inside
    /// the reset marker's metadata as `boundaryMessageId` so that
    /// `list_since_last_reset` can use it as the lower bound — this is the
    /// only way to keep the DB view aligned with the in-memory `cut_point`,
    /// because UUID v7 timing means the reset marker itself is written *after*
    /// some messages that the cut_point means to preserve.
    ///
    /// The reset marker is written first, then the summary, so both rows have
    /// `id > boundary_message_id` and the next `list_since_last_reset(…)` call
    /// will return `[boundary_message, …, reset_marker, summary_marker]` in
    /// chronological order.
    pub async fn persist_compression_markers(
        &self,
        thread_id: &str,
        summary: &str,
        source: &str,
        boundary_message_id: Option<&str>,
    ) -> Result<(), crate::model::errors::AppError> {
        persist_compression_markers_to_pool(
            &self.pool,
            thread_id,
            summary,
            source,
            boundary_message_id,
        )
        .await
    }

    pub(super) async fn next_plan_revision(&self) -> Result<u32, AppError> {
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

fn configure_agent(
    agent: &Arc<Agent>,
    spec: &AgentSessionSpec,
    weak_self: Weak<AgentSession>,
    context_compression_state: Arc<StdMutex<ContextCompressionRuntimeState>>,
) {
    agent.set_system_prompt(spec.system_prompt.clone());
    agent.replace_messages(convert_history_messages(
        &spec.history_messages,
        &spec.history_tool_calls,
        &spec.model_plan.primary.model,
    ));
    agent.set_tools(spec.runtime_tools.clone());
    agent.set_tool_execution(ToolExecutionMode::Sequential);
    agent.set_thinking_level(spec.model_plan.thinking_level);
    agent.set_transport(spec.model_plan.transport);
    agent.set_security_config(main_agent_security_config());

    // Context compression: when messages exceed the token budget, generate
    // a summary with the primary model, persist markers to DB, and keep only
    // recent messages. Falls back to pure truncation if the LLM call fails.
    //
    // Two correctness hazards addressed here:
    //
    // 1. UUID v7 timing. The reset marker we write to DB uses `now_v7()`, but
    //    `cut_point` in-memory points at a slice that includes messages the
    //    current run persisted EARLIER than this call. A naive
    //    `list_since_last_reset WHERE id >= reset_id` would exclude those
    //    earlier messages and effectively "lose" the current user prompt on
    //    the next reload. We therefore resolve a DB-backed boundary id
    //    conservatively covering all recent messages and attach it to the
    //    reset marker's metadata.
    //
    // 2. Summary-of-summary decay. If a previous auto-compression already
    //    injected a `<context_summary>` as the head of `messages`, naively
    //    re-summarising `old_messages` would re-summarise an already-
    //    summarised prefix, losing detail each pass. Instead we detect the
    //    prior summary, treat it as a pinned prefix, and ask the model to
    //    **merge** the prior summary with the delta of messages since then.
    let compression_settings = crate::core::context_compression::CompressionSettings::new(
        spec.model_plan.primary.model.context_window,
    );
    let primary_model_role = spec.model_plan.primary.clone();
    let compression_weak_self = weak_self.clone();
    let compression_thread_id = spec.thread_id.clone();
    let compression_run_id = spec.run_id.clone();
    let compression_response_language = spec.model_plan.raw.response_language.clone();
    let compression_state = Arc::clone(&context_compression_state);
    // Pre-compute the system prompt's estimated token count so the
    // compression check includes fixed overhead that the provider counts
    // against the context window but `estimate_total_tokens(messages)`
    // does not see.  This narrows the gap the calibration ratio has to
    // cover, making the trigger point more predictable.
    let system_prompt_estimated_tokens =
        crate::core::context_compression::estimate_tokens(&spec.system_prompt);
    agent.set_transform_context(move |messages| {
        // Cheap pass-through check first: only clone the heavy captured state
        // (ResolvedModelRole, String ids, Weak) when compression will actually
        // run. For long sessions with many turns this avoids per-turn heap
        // allocations when the thread is still well under budget.
        let settings = compression_settings.clone();
        let raw_estimated_tokens =
            crate::core::context_compression::estimate_total_tokens(&messages)
                .saturating_add(system_prompt_estimated_tokens);
        let calibration = current_context_token_calibration(&compression_state);
        let calibrated_total_tokens = crate::core::context_compression::calibrate_total_tokens(
            raw_estimated_tokens,
            Some(calibration),
        );
        let needs_compression = !messages.is_empty()
            && crate::core::context_compression::should_compress_total_tokens(
                calibrated_total_tokens,
                &settings,
            );

        let model_role = if needs_compression {
            Some(primary_model_role.clone())
        } else {
            None
        };
        let weak = if needs_compression {
            Some(compression_weak_self.clone())
        } else {
            None
        };
        let thread_id = if needs_compression {
            Some(compression_thread_id.clone())
        } else {
            None
        };
        let run_id = if needs_compression {
            Some(compression_run_id.clone())
        } else {
            None
        };
        let response_language = if needs_compression {
            Some(compression_response_language.clone())
        } else {
            None
        };
        let compression_state = Arc::clone(&compression_state);

        async move {
            let transformed_messages = if !needs_compression {
                messages
            } else {
                // Unwraps are sound: all `Some(_)` are populated together under
                // `needs_compression`, so either all four are `Some` (compression
                // path) or we returned above.
                run_auto_compression(
                    messages,
                    settings,
                    model_role.expect("model_role populated when compressing"),
                    weak.expect("weak populated when compressing"),
                    thread_id.expect("thread_id populated when compressing"),
                    run_id.expect("run_id populated when compressing"),
                    response_language.expect("response_language populated when compressing"),
                )
                .await
            };
            let sent_estimated_tokens =
                crate::core::context_compression::estimate_total_tokens(&transformed_messages)
                    .saturating_add(system_prompt_estimated_tokens);
            record_pending_prompt_estimate(&compression_state, sent_estimated_tokens);
            transformed_messages
        }
    });

    if let Some(api_key) = spec.model_plan.primary.api_key.clone() {
        agent.set_api_key(api_key);
    }

    // Set session ID for prompt caching (used as prompt_cache_key in OpenAI Responses API).
    // Using thread_id ensures the same conversation thread shares a cache across runs.
    agent.set_session_id(&spec.thread_id);

    // Inject default TiyCode identification headers for all LLM API requests.
    agent.set_custom_headers(crate::core::tiycode_default_headers());

    // Always set the payload hook so the DeepSeek thinking normalizer runs
    // even when there are no provider_options to merge.
    {
        let provider_options = spec.model_plan.primary.provider_options.clone();
        let provider_type = spec.model_plan.primary.provider_type.clone();
        let base_url = spec
            .model_plan
            .primary
            .model
            .base_url
            .clone()
            .unwrap_or_default();
        let thinking_enabled = spec.model_plan.thinking_level != ThinkingLevel::Off;
        agent.set_on_payload(move |payload, _model| {
            let provider_options = provider_options.clone();
            let provider_type = provider_type.clone();
            let base_url = base_url.clone();
            Box::pin(async move {
                let mut p = payload;
                if let Some(ref opts) = provider_options {
                    p = merge_payload(p, opts);
                }
                let is_ds = is_deepseek_provider(&provider_type, &base_url);
                p = normalize_deepseek_thinking_payload(p, is_ds, thinking_enabled);
                Some(p)
            })
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

    let mut primary = resolve_runtime_model_role(pool, primary).await?;
    let auxiliary = match raw_plan.auxiliary.clone() {
        Some(role) => Some(resolve_runtime_model_role(pool, role).await?),
        None => None,
    };
    let lightweight = match raw_plan.lightweight.clone() {
        Some(role) => Some(resolve_runtime_model_role(pool, role).await?),
        None => None,
    };

    let thinking_level = raw_plan
        .thinking_level
        .as_deref()
        .map(ThinkingLevel::from)
        .unwrap_or(ThinkingLevel::Off);

    // When the user has selected a thinking level, ensure the primary model
    // has its `reasoning` flag enabled so that the protocol layer actually
    // includes the reasoning parameters in the API request.  Without this
    // the protocol guard (`if !model.reasoning { return None; }`) silently
    // drops the thinking configuration.
    if thinking_level != ThinkingLevel::Off && !primary.model.reasoning {
        primary.model.reasoning = true;
    }

    Ok(ResolvedRuntimeModelPlan {
        thinking_level,
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
        .reasoning(false)
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

    {
        let mut headers = crate::core::tiycode_default_headers();
        if let Some(user_headers) = role.custom_headers.clone() {
            headers.extend(user_headers);
        }
        builder = builder.headers(headers);
    }

    if let Some(compat) = default_openai_compatible_compat(&role.provider_type) {
        builder = builder.compat(compat);
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

/// Security config for the **main** agent.  Uses a very large tool timeout so
/// that user-interactive tools (clarify, approval) are never killed by the
/// outer tiycore `tokio::select!` timeout.
fn main_agent_security_config() -> tiycore::types::SecurityConfig {
    let mut security = tiycore::types::SecurityConfig::default();
    security.agent.tool_execution_timeout_secs = MAIN_AGENT_TOOL_TIMEOUT_SECS;
    security.url = crate::core::tiycode_url_policy();
    security
}

/// Security config for **sub-agents** (helpers).  Keeps the tighter 600 s
/// timeout because sub-agents never surface user-interactive tools.
pub(crate) fn runtime_security_config() -> tiycore::types::SecurityConfig {
    let mut security = tiycore::types::SecurityConfig::default();
    security.agent.tool_execution_timeout_secs = SUBAGENT_TOOL_TIMEOUT_SECS;
    security.url = crate::core::tiycode_url_policy();
    security
}

pub(crate) fn standard_tool_timeout() -> std::time::Duration {
    std::time::Duration::from_secs(STANDARD_TOOL_TIMEOUT_SECS)
}

pub(crate) fn merge_payload(
    mut base: serde_json::Value,
    patch: &serde_json::Value,
) -> serde_json::Value {
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

/// Returns `true` when the provider should be treated as a DeepSeek endpoint.
pub(crate) fn is_deepseek_provider(provider_type: &str, base_url: &str) -> bool {
    provider_type.eq_ignore_ascii_case("deepseek") || base_url.contains("api.deepseek.com")
}

/// Final-gate normalizer that sanitizes a DeepSeek-bound JSON payload so that
/// every assistant message satisfies the API's `reasoning_content` constraints.
///
/// * **thinking-enabled**: carries forward the most-recent non-empty
///   `reasoning_content` to any assistant message that is missing
///   `reasoning_content` — regardless of whether it contains `tool_calls` or
///   is a text-only reply.  DeepSeek requires the field on every assistant
///   message when thinking mode is active.  Also ensures every assistant
///   message has a `content` field that is at least an empty string (DeepSeek
///   rejects `content: null`).
/// * **thinking-disabled**: strips `reasoning_content` from all assistant
///   messages so the API does not receive it when the session has thinking off.
///
/// When `is_deepseek` is `false` the payload is returned unmodified.
pub(crate) fn normalize_deepseek_thinking_payload(
    mut payload: serde_json::Value,
    is_deepseek: bool,
    thinking_enabled: bool,
) -> serde_json::Value {
    if !is_deepseek {
        return payload;
    }

    let messages = match payload.get_mut("messages").and_then(|v| v.as_array_mut()) {
        Some(arr) => arr,
        None => return payload,
    };

    if thinking_enabled {
        let mut last_reasoning: Option<String> = None;

        for msg in messages.iter_mut() {
            if msg.get("role").and_then(|v| v.as_str()) != Some("assistant") {
                continue;
            }

            // Track the latest non-empty reasoning_content.
            if let Some(rc) = msg.get("reasoning_content").and_then(|v| v.as_str()) {
                if !rc.is_empty() {
                    last_reasoning = Some(rc.to_string());
                }
            }

            // Ensure `content` is at least an empty string (never null).
            match msg.get("content") {
                None | Some(serde_json::Value::Null) => {
                    msg.as_object_mut()
                        .map(|o| o.insert("content".to_string(), serde_json::json!("")));
                }
                _ => {}
            }

            // If the message lacks reasoning_content, fill it from the
            // most-recent reasoning we saw.  This covers both tool-call-only
            // and text-only assistant messages that lost their thinking block
            // during history reconstruction (Phase 4 mis-allocation).
            //
            // KNOWN-LIMITATION: `last_reasoning` is not reset at user message
            // boundaries — it carries forward across turns.  Per DeepSeek spec,
            // reasoning_content from previous turns is ignored by the API when
            // passed back, so this is functionally harmless but not precise.
            //
            // KNOWN-LIMITATION: If the very first assistant message in the
            // conversation has no reasoning (last_reasoning is None), it will
            // not receive a backfilled reasoning_content.  In thinking mode the
            // first API response always includes reasoning, so this edge case
            // does not occur in practice.
            let missing_reasoning = msg.get("reasoning_content").map_or(true, |v| {
                v.is_null() || !v.is_string() || v.as_str().map_or(true, str::is_empty)
            });

            if missing_reasoning {
                if let Some(ref rc) = last_reasoning {
                    msg.as_object_mut()
                        .map(|o| o.insert("reasoning_content".to_string(), serde_json::json!(rc)));
                }
            }
        }
    } else {
        // Thinking disabled: strip reasoning_content from all assistant messages.
        for msg in messages.iter_mut() {
            if msg.get("role").and_then(|v| v.as_str()) != Some("assistant") {
                continue;
            }
            if let Some(obj) = msg.as_object_mut() {
                obj.remove("reasoning_content");
            }
        }
    }

    payload
}

#[cfg(test)]
#[path = "agent_session_tests.rs"]
mod agent_session_tests;
