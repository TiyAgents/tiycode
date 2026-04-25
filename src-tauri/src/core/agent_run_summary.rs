use tiycore::agent::AgentMessage;
use tiycore::provider::get_provider;
use tiycore::types::{
    Context as TiyContext, Message as TiyMessage, StopReason, StreamOptions as TiyStreamOptions,
    UserMessage,
};

use crate::core::agent_session::{normalize_profile_response_language, ResolvedModelRole};
use crate::core::plan_checkpoint::{PlanApprovalAction, PlanMessageMetadata};
use crate::core::tiycode_default_headers;
use crate::core::tiycode_url_policy;
use crate::model::errors::{AppError, ErrorSource};
use crate::model::thread::MessageRecord;

use super::agent_run_manager::{
    build_provider_options_payload_hook, PRIMARY_SUMMARY_MAX_TOKENS, PRIMARY_SUMMARY_TIMEOUT,
    SUMMARY_HISTORY_MIN_CHARS,
};
use super::agent_run_title::collapse_whitespace;

pub(crate) fn parse_message_metadata<T>(message: &MessageRecord) -> Result<T, AppError>
where
    T: for<'de> serde::Deserialize<'de>,
{
    let raw = message.metadata_json.as_deref().ok_or_else(|| {
        AppError::recoverable(
            ErrorSource::Thread,
            "thread.message.metadata_missing",
            format!("Message '{}' is missing metadata.", message.id),
        )
    })?;
    serde_json::from_str::<T>(raw).map_err(|error| {
        AppError::recoverable(
            ErrorSource::Thread,
            "thread.message.metadata_invalid",
            format!("Message '{}' has invalid metadata: {error}", message.id),
        )
    })
}

pub(crate) fn extract_run_string(model_plan: &serde_json::Value, path: &[&str]) -> Option<String> {
    let mut current = model_plan;

    for segment in path {
        current = current.get(*segment)?;
    }

    current.as_str().map(ToString::to_string)
}

pub(crate) fn extract_run_model_refs(
    model_plan: &serde_json::Value,
) -> (Option<String>, Option<String>, Option<String>) {
    (
        extract_run_string(model_plan, &["profileId"]),
        extract_run_string(model_plan, &["primary", "providerId"]),
        extract_run_string(model_plan, &["primary", "modelRecordId"])
            .or_else(|| extract_run_string(model_plan, &["primary", "modelId"])),
    )
}

pub(crate) fn build_implementation_handoff_prompt(
    thread_id: &str,
    metadata: &PlanMessageMetadata,
    action: PlanApprovalAction,
) -> String {
    let action_note = match action {
        PlanApprovalAction::ApplyPlan => {
            "The user approved this plan for direct implementation."
        }
        PlanApprovalAction::ApplyPlanWithContextReset => {
            "The user approved this plan after clearing the planning conversation from the implementation context."
        }
    };
    let plan_file_note = crate::core::plan_checkpoint::plan_file_path(thread_id)
        .filter(|path| path.exists())
        .map(|path| format!("\n- Plan file on disk: {}", path.display()))
        .unwrap_or_default();
    match action {
        PlanApprovalAction::ApplyPlan => {
            let plan_markdown = crate::core::plan_checkpoint::plan_markdown(metadata);

            format!(
                "Implementation handoff:\n- {action_note}\n- Plan revision: {}{plan_file_note}\n- Treat the approved plan below as the implementation baseline.\n- If the plan turns out to be invalid or incomplete, pause and return to planning before making a different change.\n- After implementation, use agent_review with planFilePath to verify each plan step was completed.\n\nApproved plan:\n{}",
                metadata.artifact.plan_revision,
                plan_markdown
            )
        }
        PlanApprovalAction::ApplyPlanWithContextReset => format!(
            "Implementation handoff:\n- {action_note}\n- Plan revision: {}{plan_file_note}\n- The reset context already includes a historical summary and the approved plan.\n- Treat the approved plan in context as the implementation baseline.\n- If the plan turns out to be invalid or incomplete, pause and return to planning before making a different change.\n- After implementation, use agent_review with planFilePath to verify each plan step was completed.",
            metadata.artifact.plan_revision,
        ),
    }
}

/// Returns the model to use for primary summary generation.
/// Always uses the primary model to avoid context window mismatches.
pub(crate) fn primary_summary_model(
    model_plan: &crate::core::agent_session::ResolvedRuntimeModelPlan,
) -> tiycore::types::Model {
    model_plan.primary.model.clone()
}

pub(crate) fn build_compact_summary_system_prompt(response_language: Option<&str>) -> String {
    let mut lines = vec![
        "You compress conversation state so another model can continue after context reset.".to_string(),
        "Return only one compact summary block using the exact XML-style wrapper below.".to_string(),
        String::new(),
        "Requirements:".to_string(),
        "- Preserve the user's current goal and latest requested outcome.".to_string(),
        "- Preserve important constraints, preferences, and decisions.".to_string(),
        "- List work already completed and important findings.".to_string(),
        "- List the most relevant remaining tasks, open questions, or risks.".to_string(),
        "- Mention key files, components, commands, tools, or errors only when they matter for continuation.".to_string(),
        "- Be factual and concise. Do not invent details.".to_string(),
        "- Do not address the user directly. Do not include greetings or commentary.".to_string(),
        "- Prefer short bullet lists under clear section labels.".to_string(),
        "- Keep the summary self-contained and suitable for direct insertion into future model context.".to_string(),
    ];

    if let Some(language) = normalize_profile_response_language(response_language) {
        lines.push(format!(
            "- Respond in {language} unless the user explicitly asks for a different language."
        ));
    }

    lines.extend([
        String::new(),
        "Output rules:".to_string(),
        "- Start with <context_summary> on its own line.".to_string(),
        "- End with </context_summary> on its own line.".to_string(),
        "- Do not output any text before or after the wrapper.".to_string(),
        String::new(),
        "Example output:".to_string(),
        "<context_summary>".to_string(),
        "- User goal: Stabilize /compact summary formatting.".to_string(),
        "- Completed: Checked current local summarization flow and wrapper handling.".to_string(),
        "- Remaining: Move compact rules into system prompt and keep output parsing robust."
            .to_string(),
        "</context_summary>".to_string(),
    ]);

    lines.join("\n")
}

pub(crate) fn build_compact_summary_messages(
    history: &[AgentMessage],
    instructions: Option<&str>,
    max_history_chars: usize,
) -> Vec<TiyMessage> {
    let mut messages = Vec::new();

    if let Some(instructions) = instructions
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        messages.push(TiyMessage::User(UserMessage::text(format!(
            "Additional user instructions for this compact:\n{instructions}"
        ))));
    }

    messages.push(TiyMessage::User(UserMessage::text(format!(
        "Conversation history to compact:\n{}",
        render_compact_summary_history(history, max_history_chars)
    ))));

    messages
}

/// Generate a context summary using the primary model.
///
/// Always uses the primary model (not lightweight) to ensure the summary
/// stays within the model's context window. Returns `Err` on failure
/// (no fallback), so callers can decide how to handle errors.
///
/// If `abort` is provided, the call short-circuits with a recoverable
/// cancellation error as soon as the signal fires. This is used by the
/// `transform_context` hook so that clicking Cancel during
/// "Compressing context…" doesn't have to wait out the 90s timeout.
pub(crate) async fn generate_primary_summary(
    model_role: &ResolvedModelRole,
    history: &[AgentMessage],
    instructions: Option<&str>,
    response_language: Option<&str>,
    abort: Option<tiycore::agent::AbortSignal>,
) -> Result<String, AppError> {
    let max_history_chars = summary_history_char_budget(model_role);
    execute_summary_llm_call(
        model_role,
        build_compact_summary_system_prompt(response_language),
        build_compact_summary_messages(history, instructions, max_history_chars),
        instructions,
        abort,
        "primary",
    )
    .await
}

/// Shared implementation for primary- and merge-summary LLM calls.
///
/// Both call paths share the same provider setup, reasoning-aware
/// `max_tokens` budget, stream options, and result-normalization logic.
/// Extracting the shared tail prevents behavioural drift between the two
/// public entry points when stream options or error handling change.
///
/// `kind` is a short label (e.g. "primary" / "merge") used only for error
/// messages so a failure can be traced back to the originating call path.
pub(crate) async fn execute_summary_llm_call(
    model_role: &ResolvedModelRole,
    system_prompt: String,
    messages: Vec<TiyMessage>,
    instructions: Option<&str>,
    abort: Option<tiycore::agent::AbortSignal>,
    kind: &str,
) -> Result<String, AppError> {
    // Summary generation does not benefit from reasoning/thinking tokens.
    // Disable reasoning so the protocol layer omits thinking/reasoning parameters,
    // preventing reasoning tokens from consuming the PRIMARY_SUMMARY_MAX_TOKENS budget.
    let mut model_role = model_role.clone();
    let was_reasoning = model_role.model.reasoning;
    model_role.model.reasoning = false;

    let provider = get_provider(&model_role.model.provider).ok_or_else(|| {
        AppError::recoverable(
            ErrorSource::Settings,
            "settings.primary_summary.provider_missing",
            format!(
                "Provider type '{:?}' is not registered for {} summary generation.",
                model_role.model.provider, kind
            ),
        )
    })?;

    let context = TiyContext {
        system_prompt: Some(system_prompt),
        messages,
        tools: None,
    };

    let max_tokens = if was_reasoning {
        // Bump for reasoning-only models that ignore the disable
        PRIMARY_SUMMARY_MAX_TOKENS * 2
    } else {
        PRIMARY_SUMMARY_MAX_TOKENS
    };

    let options = TiyStreamOptions {
        api_key: model_role.api_key.clone(),
        max_tokens: Some(max_tokens),
        headers: Some(tiycode_default_headers()),
        on_payload: build_provider_options_payload_hook(model_role.provider_options.clone()),
        security: Some(tiycore::types::SecurityConfig::default().with_url(tiycode_url_policy())),
        ..TiyStreamOptions::default()
    };

    let stream = provider.stream(&model_role.model, &context, options);
    let stream_fut = stream.try_result(PRIMARY_SUMMARY_TIMEOUT);
    let completion = await_summary_with_abort(stream_fut, abort).await?;

    let message = match completion {
        Some(message) => message,
        None => {
            return Err(AppError::recoverable(
                ErrorSource::System,
                "runtime.context_compression.empty_result",
                format!("{} summary generation returned empty result", kind),
            ));
        }
    };

    if message.stop_reason == StopReason::Error {
        let detail = message
            .error_message
            .clone()
            .unwrap_or_else(|| format!("{} summary generation failed", kind));
        return Err(AppError::recoverable(
            ErrorSource::System,
            "runtime.context_compression.failed",
            detail,
        ));
    }

    let summary = normalize_compact_summary(message.text_content(), instructions);
    match summary {
        Some(s) => Ok(s),
        None => Err(AppError::recoverable(
            ErrorSource::System,
            "runtime.context_compression.empty_result",
            format!("{} summary generation produced no usable content", kind),
        )),
    }
}

/// Await a summary-generation future while also watching an optional
/// `AbortSignal`. Returns `Err(cancelled)` as soon as the signal fires,
/// allowing the caller to drop the stream future (and its in-flight HTTP
/// connection) rather than wait for the provider timeout.
pub(crate) async fn await_summary_with_abort<T>(
    future: impl std::future::Future<Output = T>,
    abort: Option<tiycore::agent::AbortSignal>,
) -> Result<T, AppError> {
    match abort {
        Some(signal) if signal.is_cancelled() => Err(cancellation_error()),
        Some(signal) => {
            tokio::select! {
                // Bias towards the primary future: if both branches are
                // simultaneously ready, we prefer returning the summary
                // result over a spurious cancel. (Note: this select does
                // NOT re-check cancellation after the future wins — if the
                // future completes at the exact same instant the signal
                // fires, the summary is kept. That is acceptable because
                // the caller will then be free to use the value; we'd only
                // be throwing away work the user can still benefit from.)
                biased;
                value = future => Ok(value),
                _ = signal.cancelled() => Err(cancellation_error()),
            }
        }
        None => Ok(future.await),
    }
}

pub(crate) fn cancellation_error() -> AppError {
    AppError::recoverable(
        ErrorSource::System,
        "runtime.context_compression.cancelled",
        "Context compression was cancelled".to_string(),
    )
}

pub(crate) fn build_merge_summary_system_prompt(response_language: Option<&str>) -> String {
    let mut lines = vec![
        "You maintain a rolling context summary for another model to continue after context reset."
            .to_string(),
        "You will be given the PRIOR summary (already in <context_summary> form) and a DELTA of conversation"
            .to_string(),
        "that happened after that summary was last produced. Produce a SINGLE updated <context_summary>"
            .to_string(),
        "that merges both — keeping still-relevant facts from the prior summary and folding in new information"
            .to_string(),
        "from the delta. Treat the prior summary as authoritative for anything it covers and do not drop"
            .to_string(),
        "details that remain pertinent.".to_string(),
        String::new(),
        "Requirements:".to_string(),
        "- Preserve the user's current goal and most recent requested outcome.".to_string(),
        "- Retain important constraints, preferences, and decisions from the prior summary unless the delta"
            .to_string(),
        "  explicitly supersedes them.".to_string(),
        "- Fold newly completed work, findings, key files/commands, and remaining tasks from the delta in."
            .to_string(),
        "- Drop items the delta marks resolved; add items the delta newly raises.".to_string(),
        "- Be factual and concise. Do not invent details. Do not address the user.".to_string(),
        "- Prefer short bullet lists under clear section labels.".to_string(),
    ];

    if let Some(language) = normalize_profile_response_language(response_language) {
        lines.push(format!(
            "- Respond in {language} unless the user explicitly asks for a different language."
        ));
    }

    lines.extend([
        String::new(),
        "Output rules:".to_string(),
        "- Start with <context_summary> on its own line.".to_string(),
        "- End with </context_summary> on its own line.".to_string(),
        "- Do not output any text before or after the wrapper.".to_string(),
    ]);

    lines.join("\n")
}

pub(crate) fn build_merge_summary_messages(
    prior_summary: &str,
    delta_history: &[AgentMessage],
    instructions: Option<&str>,
    max_history_chars: usize,
) -> Vec<TiyMessage> {
    let mut messages = Vec::new();

    if let Some(instructions) = instructions
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        messages.push(TiyMessage::User(UserMessage::text(format!(
            "Additional user instructions for this compact:\n{instructions}"
        ))));
    }

    messages.push(TiyMessage::User(UserMessage::text(format!(
        "Prior summary (authoritative for anything it covers):\n{}",
        prior_summary.trim()
    ))));

    messages.push(TiyMessage::User(UserMessage::text(format!(
        "New conversation delta (happened after the prior summary):\n{}",
        render_compact_summary_history(delta_history, max_history_chars)
    ))));

    messages
}

/// Generate an updated summary by merging a prior `<context_summary>` block with
/// a delta of conversation history.
///
/// Used by auto-compression when the previous compression already left a summary
/// in the in-memory context; merging avoids the "summary-of-summary" quality
/// decay that would happen if we re-summarised the already-summarised prefix.
///
/// `abort` mirrors `generate_primary_summary`: the call short-circuits with a
/// recoverable cancellation error when the signal fires.
pub(crate) async fn generate_merge_summary(
    model_role: &ResolvedModelRole,
    prior_summary: &str,
    delta_history: &[AgentMessage],
    instructions: Option<&str>,
    response_language: Option<&str>,
    abort: Option<tiycore::agent::AbortSignal>,
) -> Result<String, AppError> {
    let max_history_chars = summary_history_char_budget(model_role);
    execute_summary_llm_call(
        model_role,
        build_merge_summary_system_prompt(response_language),
        build_merge_summary_messages(
            prior_summary,
            delta_history,
            instructions,
            max_history_chars,
        ),
        instructions,
        abort,
        "merge",
    )
    .await
}

/// Detect whether the head of `messages` contains a previously injected
/// `<context_summary>` block (produced by an earlier auto-compression pass).
///
/// Returns `Some((prior_summary_text, consumed_prefix_len))` when found — the
/// caller should treat the first `consumed_prefix_len` messages as a pinned
/// prefix (the old summary) and summarise the **rest** as a delta.
///
/// Only the **first** user message is inspected: previous compression always
/// places the summary as the new head of the context.
pub(crate) fn detect_prior_summary(messages: &[AgentMessage]) -> Option<(String, usize)> {
    let first = messages.first()?;
    let user = match first {
        AgentMessage::User(user) => user,
        _ => return None,
    };
    let text = match &user.content {
        tiycore::types::UserContent::Text(t) => t.as_str(),
        tiycore::types::UserContent::Blocks(blocks) => {
            // Accept only a single text block for detection.
            blocks
                .iter()
                .find_map(|block| match block {
                    tiycore::types::ContentBlock::Text(t) => Some(t.text.as_str()),
                    _ => None,
                })
                .unwrap_or("")
        }
    };

    let trimmed = text.trim_start();
    if !trimmed.starts_with("<context_summary>") {
        return None;
    }
    // Require a closing wrapper too; a truncated block means the message
    // isn't a well-formed prior summary and re-summarisation is safer.
    if !trimmed.contains("</context_summary>") {
        return None;
    }

    Some((text.to_string(), 1))
}

/// Derive how many characters of conversation history we can afford to send
/// to the summary LLM for a given model role.
///
/// The formula reserves room for:
/// - The system prompt + instructions + wrapper text (~2,000 tokens)
/// - The model's own output budget (`PRIMARY_SUMMARY_MAX_TOKENS`, doubled
///   for reasoning-only models since reasoning tokens share the output slot)
/// - A safety margin (1,000 tokens) for off-by-one token vs char estimation
///
/// The remaining tokens are multiplied by 4 (the chars-per-token heuristic
/// used elsewhere in the codebase) to produce a char budget. We floor the
/// result at `SUMMARY_HISTORY_MIN_CHARS` so a missing or degenerate
/// `context_window` cannot collapse the budget to zero, but we **do not**
/// impose an upper cap — modern 1M/2M-token models need their full
/// advertised window to compress long CJK-heavy threads without silent
/// information loss. Provider limits (payload size, rate limits) are
/// enforced downstream; here we trust the model's advertised capacity.
pub(crate) fn summary_history_char_budget(model_role: &ResolvedModelRole) -> usize {
    let context_window = model_role.model.context_window as usize;
    let output_tokens = if model_role.model.reasoning {
        // Reasoning models share the output slot with thinking tokens, so
        // we must assume the doubled allowance to avoid collisions.
        (PRIMARY_SUMMARY_MAX_TOKENS as usize).saturating_mul(2)
    } else {
        PRIMARY_SUMMARY_MAX_TOKENS as usize
    };
    // Non-history overhead: system prompt, instructions wrapper, safety margin.
    let overhead_tokens: usize = 3_000;

    let tokens_for_history = context_window
        .saturating_sub(output_tokens)
        .saturating_sub(overhead_tokens);
    let chars_for_history = tokens_for_history.saturating_mul(4);

    chars_for_history.max(SUMMARY_HISTORY_MIN_CHARS)
}

/// Render conversation history for the summary model.
///
/// Strategy: pack **full** messages from newest to oldest within the char
/// budget, then reverse so the model reads them in chronological order.
/// Individual items are only truncated when a single item is itself larger
/// than the remaining budget — in which case we prefer to keep the most
/// recent portion of that item. Older messages that don't fit are dropped
/// entirely rather than half-truncated, because the older end of the
/// conversation is the least load-bearing for continuing the task.
///
/// This is a substantial behavioural change from the previous version
/// (which pre-truncated every item to 300–1,500 chars and capped the whole
/// payload at 18K chars). The previous formula was tight enough to drop
/// most of a real compact call's context; the new formula preserves full
/// content for typical threads and only activates the fallback on genuinely
/// oversized payloads.
/// Per-tool-result budget cap inside `render_compact_summary_history`.
///
/// Tool results can be very large (file reads, command output). Letting a
/// single one consume the entire remaining budget would crowd out other
/// messages that provide better summarisation signal. This cap limits any
/// single tool result body (the text portion, before the header) so the
/// budget is distributed more evenly across the conversation.
pub(crate) const SUMMARY_TOOL_RESULT_MAX_CHARS: usize = 6_000;

pub(crate) fn render_compact_summary_history(history: &[AgentMessage], max_chars: usize) -> String {
    // Rendered chunks in **reverse** order (newest first) for budget packing.
    let mut chunks_reversed: Vec<String> = Vec::new();
    let mut remaining = max_chars;

    for message in history.iter().rev() {
        if remaining == 0 {
            break;
        }

        let chunk = match message {
            AgentMessage::User(user) => {
                let text = user_message_to_text(user);
                if text.is_empty() {
                    continue;
                }
                format!("[user]\n{text}\n\n")
            }
            AgentMessage::Assistant(assistant) => {
                let text = assistant_message_to_text(assistant);
                if text.is_empty() {
                    continue;
                }
                format!("[assistant]\n{text}\n\n")
            }
            AgentMessage::ToolResult(tool_result) => {
                let raw_text = tool_result_to_text(tool_result);
                if raw_text.is_empty() {
                    continue;
                }
                let header = if tool_result.tool_name.is_empty() {
                    "[tool_result]".to_string()
                } else {
                    format!("[tool_result] {}", tool_result.tool_name)
                };
                // Apply per-item smart truncation: head+tail with overlap
                // detection. This keeps the beginning (structure, headers,
                // imports) and the end (errors, final output) of large tool
                // results while compressing the less-informative middle.
                let text = truncate_tool_result_head_tail(
                    &raw_text,
                    SUMMARY_TOOL_RESULT_MAX_CHARS.min(remaining),
                );
                format!("{header}\n{text}\n\n")
            }
            AgentMessage::Custom { data, .. } => {
                let text = collapse_whitespace(&data.to_string());
                if text.is_empty() {
                    continue;
                }
                format!("[custom]\n{text}\n\n")
            }
        };

        let chunk_len = chunk.chars().count();
        if chunk_len <= remaining {
            remaining -= chunk_len;
            chunks_reversed.push(chunk);
        } else {
            // This single item is larger than the remaining budget. Keep
            // the TAIL of it (more recent tokens tend to matter more for
            // continuation), prefixed with an ellipsis marker so the
            // model knows the head was elided.
            let truncated = truncate_chars_keep_tail(&chunk, remaining);
            if !truncated.is_empty() {
                chunks_reversed.push(truncated);
            }
            break;
        }
    }

    chunks_reversed.reverse();
    chunks_reversed.concat()
}

/// Smart head+tail truncation for tool result text.
///
/// When the text fits within `max_chars`, returns it as-is. Otherwise keeps
/// the first 2/3 and last 1/3 of the budget (minus the elision marker),
/// preserving both the beginning (structure, headers, imports) and the end
/// (errors, final output) of large tool results. When the omitted middle
/// section is very small (< 50 chars), a simple head truncation is used
/// instead to avoid a gap marker that hides barely any content.
pub(crate) fn truncate_tool_result_head_tail(text: &str, max_chars: usize) -> String {
    let total = text.chars().count();
    if total <= max_chars {
        return text.to_string();
    }

    const MARKER: &str = "\n[… middle content omitted …]\n";
    let marker_len = MARKER.chars().count();

    // Budget too small for marker + meaningful head/tail — just hard-truncate.
    if max_chars <= marker_len + 2 {
        return text.chars().take(max_chars).collect();
    }

    let content_budget = max_chars - marker_len;
    let head_budget = content_budget * 2 / 3;
    let tail_budget = content_budget - head_budget;

    let omitted = total - head_budget - tail_budget;

    // If the omitted section is tiny, a gap marker that hides just a few
    // chars would be misleading — just do a simple head truncation instead.
    if omitted < 50 {
        let head: String = text.chars().take(max_chars - marker_len).collect();
        return format!("{head}{MARKER}");
    }

    let tail_start = total - tail_budget;
    let head: String = text.chars().take(head_budget).collect();
    let tail: String = text.chars().skip(tail_start).collect();

    format!("{head}\n[… {omitted} chars omitted …]\n{tail}")
}

/// Keep the tail `max_chars` of a string, prefixed with an ellipsis marker
/// when truncation occurs. Char-boundary safe (walks by `char`, not byte).
pub(crate) fn truncate_chars_keep_tail(text: &str, max_chars: usize) -> String {
    let total = text.chars().count();
    if total <= max_chars {
        return text.to_string();
    }
    // Reserve a few chars for the elision marker so the resulting string
    // fits within max_chars total.
    const MARKER: &str = "[…earlier content truncated…]\n";
    let marker_len = MARKER.chars().count();
    if max_chars <= marker_len {
        // Budget too small for a marker — just return the tail without one.
        let skip = total - max_chars;
        return text.chars().skip(skip).collect();
    }
    let tail_len = max_chars - marker_len;
    let skip = total - tail_len;
    let tail: String = text.chars().skip(skip).collect();
    format!("{MARKER}{tail}")
}

pub(crate) fn user_message_to_text(user: &UserMessage) -> String {
    // Per-item truncation was removed so render_compact_summary_history can
    // make a holistic budget decision. Trimming is still applied because we
    // don't want leading/trailing whitespace polluting the rendered block.
    match &user.content {
        tiycore::types::UserContent::Text(text) => text.trim().to_string(),
        tiycore::types::UserContent::Blocks(blocks) => {
            let mut parts = Vec::new();
            for block in blocks {
                match block {
                    tiycore::types::ContentBlock::Text(text) => {
                        let trimmed = text.text.trim();
                        if !trimmed.is_empty() {
                            parts.push(trimmed.to_string());
                        }
                    }
                    tiycore::types::ContentBlock::Image(_) => parts.push("[image]".to_string()),
                    _ => {}
                }
            }
            parts.join("\n")
        }
    }
}

pub(crate) fn assistant_message_to_text(assistant: &tiycore::types::AssistantMessage) -> String {
    // No per-item char caps: the caller (render_compact_summary_history)
    // applies a single holistic budget, so the message can keep its full
    // thinking blocks and tool-call arguments. That restores fidelity for
    // long technical threads that the old 1,500-char cap silently clipped.
    let mut parts = Vec::new();
    for block in &assistant.content {
        match block {
            tiycore::types::ContentBlock::Text(text) => {
                let trimmed = text.text.trim();
                if !trimmed.is_empty() {
                    parts.push(trimmed.to_string());
                }
            }
            tiycore::types::ContentBlock::Thinking(thinking) => {
                let trimmed = thinking.thinking.trim();
                if !trimmed.is_empty() {
                    parts.push(format!("[thinking] {trimmed}"));
                }
            }
            tiycore::types::ContentBlock::ToolCall(tool_call) => {
                parts.push(format!(
                    "[tool_call] {} {}",
                    tool_call.name,
                    collapse_whitespace(&tool_call.arguments.to_string())
                ));
            }
            tiycore::types::ContentBlock::Image(_) => parts.push("[image]".to_string()),
        }
    }
    parts.join("\n")
}

pub(crate) fn tool_result_to_text(tool_result: &tiycore::types::ToolResultMessage) -> String {
    // Unbounded: the holistic budget in render_compact_summary_history
    // decides whether this item fits wholesale or must be tail-truncated.
    let mut parts = Vec::new();
    for block in &tool_result.content {
        if let tiycore::types::ContentBlock::Text(text) = block {
            let trimmed = text.text.trim();
            if !trimmed.is_empty() {
                parts.push(trimmed.to_string());
            }
        }
    }
    parts.join("\n")
}

pub(crate) fn normalize_compact_summary(raw: String, instructions: Option<&str>) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }

    let summary = extract_context_summary_block(trimmed).unwrap_or_else(|| {
        let normalized_body =
            extract_context_summary_body(trimmed).unwrap_or_else(|| trimmed.to_string());
        format!(
            "<context_summary>\n{}\n</context_summary>",
            normalized_body.trim()
        )
    });

    Some(append_compact_instructions(summary, instructions))
}

pub(crate) fn extract_context_summary_block(raw: &str) -> Option<String> {
    let start_tag = "<context_summary>";
    let end_tag = "</context_summary>";
    let start = raw.find(start_tag)?;
    let content_start = start + start_tag.len();
    let relative_end = raw[content_start..].find(end_tag)?;
    let end = content_start + relative_end + end_tag.len();
    let candidate = raw[start..end].trim();

    if candidate.is_empty() {
        return None;
    }

    Some(candidate.to_string())
}

pub(crate) fn extract_context_summary_body(raw: &str) -> Option<String> {
    let start_tag = "<context_summary>";
    let end_tag = "</context_summary>";

    if let Some(block) = extract_context_summary_block(raw) {
        let content = block
            .trim_start_matches(start_tag)
            .trim_end_matches(end_tag)
            .trim();
        return if content.is_empty() {
            None
        } else {
            Some(content.to_string())
        };
    }

    if let Some(start) = raw.find(start_tag) {
        let content = raw[start + start_tag.len()..].trim();
        return if content.is_empty() {
            None
        } else {
            Some(content.to_string())
        };
    }

    if let Some(end) = raw.find(end_tag) {
        let content = raw[..end].trim();
        return if content.is_empty() {
            None
        } else {
            Some(content.to_string())
        };
    }

    None
}

pub(crate) fn append_compact_instructions(
    base_summary: String,
    instructions: Option<&str>,
) -> String {
    let Some(extra) = instructions
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        return base_summary;
    };

    format!(
        "{base_summary}\n\n<extra_instructions>\n{}\n</extra_instructions>",
        extra
    )
}
