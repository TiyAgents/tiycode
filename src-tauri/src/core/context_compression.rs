//! Context compression (compaction) for managing LLM context window limits.
//!
//! Provides token-budget-aware context window management via the
//! `transform_context` hook. The actual summary generation is done by
//! calling the primary LLM model (via `generate_primary_summary` in
//! `agent_run_manager`); this module handles the **mechanics** of
//! detecting when compression is needed, finding a safe cut-point, and
//! building the compressed message list.
//!
//! ## Strategy
//!
//! 1. **Token estimation**: `chars / 4` heuristic (matches pi-mono).
//! 2. **Threshold check**: Compress when `estimated_tokens > context_window - reserve_tokens`.
//! 3. **Cut-point detection**: Walk backwards from newest message, keep `keep_recent_tokens`
//!    worth of recent messages. Never cut in the middle of an assistant+tool_result pair.
//! 4. **Summary injection**: Old messages before the cut-point are replaced with a single
//!    `AgentMessage::User` containing the LLM-generated summary.
//! 5. **Tool result truncation**: Large tool results in the recent region are truncated.

use tiycore::agent::AgentMessage;
use tiycore::types::{ContentBlock, TextContent, UserMessage};
/// Reserve this many tokens for the model's response + overhead.
/// Matches pi-mono `DEFAULT_COMPACTION_SETTINGS.reserveTokens`.
const RESERVE_TOKENS: u32 = 16_384;

/// Keep at least this many tokens of recent conversation untouched.
/// With LLM-generated summaries providing rich context, we can keep a
/// smaller recent window to maximise the space saved per compression.
const KEEP_RECENT_TOKENS: u32 = 16_000;

/// Maximum characters for a tool result in the "old" region.
/// Longer results are truncated with a summary marker.
const OLD_TOOL_RESULT_MAX_CHARS: usize = 800;

/// Minimum number of messages to keep (never compress below this).
const MIN_MESSAGES_TO_KEEP: usize = 4;

/// Estimate the number of tokens for a string.
///
/// Base model is the chars/4 heuristic (matches pi-mono's `estimateTokens()`
/// and works well for English / code). For Chinese / Japanese / Korean text,
/// chars/4 substantially underestimates because each CJK character typically
/// consumes between one and two BPE tokens — we have seen real provider calls
/// return 413/context-length errors even when our estimate was well under the
/// advertised context window on CJK-heavy threads.
///
/// Strategy: count CJK characters and non-CJK bytes separately, estimate
/// `cjk_chars + non_cjk_bytes / 4`, and take the max with the plain chars/4
/// heuristic. The max keeps ASCII behaviour identical to the original (so
/// existing tests remain valid) while raising the floor for CJK input.
fn estimate_tokens(text: &str) -> u32 {
    let base = (text.len() as u32).saturating_add(3) / 4;

    // Fast path: pure-ASCII strings need no CJK accounting.
    if text.is_ascii() {
        return base;
    }

    let mut cjk_chars: u32 = 0;
    let mut non_cjk_bytes: u32 = 0;
    for ch in text.chars() {
        if is_cjk_like(ch) {
            cjk_chars = cjk_chars.saturating_add(1);
        } else {
            non_cjk_bytes = non_cjk_bytes.saturating_add(ch.len_utf8() as u32);
        }
    }
    // One token per CJK char + chars/4 for the remaining ASCII/Latin part.
    let cjk_weighted = cjk_chars.saturating_add(non_cjk_bytes.saturating_add(3) / 4);

    base.max(cjk_weighted)
}

/// Classify a character as "CJK-like" for the purposes of token estimation.
///
/// Covers the ranges commonly used in real threads: CJK Unified Ideographs
/// (basic + Extension A), Hiragana, Katakana, Hangul Syllables, plus CJK
/// Symbols & Punctuation and Fullwidth forms. This deliberately overshoots
/// (some of these, like fullwidth punctuation, may tokenise as a single token)
/// because the intent is a conservative upper bound, not a precise count.
fn is_cjk_like(ch: char) -> bool {
    matches!(
        ch as u32,
        0x3000..=0x303F      // CJK Symbols and Punctuation
        | 0x3040..=0x309F   // Hiragana
        | 0x30A0..=0x30FF   // Katakana
        | 0x3400..=0x4DBF   // CJK Extension A
        | 0x4E00..=0x9FFF   // CJK Unified Ideographs
        | 0xAC00..=0xD7AF   // Hangul Syllables
        | 0xF900..=0xFAFF   // CJK Compatibility Ideographs
        | 0xFF00..=0xFFEF   // Halfwidth and Fullwidth Forms
    )
}

/// Estimate tokens for a single `AgentMessage`.
pub fn estimate_message_tokens(message: &AgentMessage) -> u32 {
    match message {
        AgentMessage::User(user_msg) => {
            let text = match &user_msg.content {
                tiycore::types::UserContent::Text(t) => t.as_str(),
                tiycore::types::UserContent::Blocks(blocks) => {
                    let mut total = 0u32;
                    for block in blocks {
                        total += match block {
                            ContentBlock::Text(tc) => estimate_tokens(&tc.text),
                            ContentBlock::Image(_) => 1000,
                            _ => 10,
                        };
                    }
                    return total + 4;
                }
            };
            estimate_tokens(text) + 4
        }
        AgentMessage::Assistant(assistant_msg) => {
            let mut total = 0u32;
            for block in &assistant_msg.content {
                total += match block {
                    ContentBlock::Text(tc) => estimate_tokens(&tc.text),
                    ContentBlock::Thinking(tc) => estimate_tokens(&tc.thinking),
                    ContentBlock::ToolCall(tc) => {
                        estimate_tokens(&tc.name) + estimate_tokens(&tc.arguments.to_string()) + 20
                    }
                    ContentBlock::Image(_) => 1000,
                };
            }
            total + 4
        }
        AgentMessage::ToolResult(tool_result) => {
            let mut total = 0u32;
            for block in &tool_result.content {
                total += match block {
                    ContentBlock::Text(tc) => estimate_tokens(&tc.text),
                    _ => 10,
                };
            }
            total + 10
        }
        AgentMessage::Custom { data, .. } => estimate_tokens(&data.to_string()) + 10,
    }
}

/// Settings for context compression.
#[derive(Debug, Clone)]
pub struct CompressionSettings {
    /// Total context window size in tokens.
    pub context_window: u32,
    /// Tokens reserved for model output + overhead.
    pub reserve_tokens: u32,
    /// Minimum tokens of recent conversation to preserve.
    pub keep_recent_tokens: u32,
}

impl CompressionSettings {
    pub fn new(context_window: u32) -> Self {
        Self {
            context_window,
            reserve_tokens: RESERVE_TOKENS,
            keep_recent_tokens: KEEP_RECENT_TOKENS,
        }
    }

    /// The maximum tokens we can use for input context.
    pub fn budget(&self) -> u32 {
        self.context_window.saturating_sub(self.reserve_tokens)
    }
}

// ---------------------------------------------------------------------------
// Public API: should_compress, find_cut_point, build_compressed_messages
// ---------------------------------------------------------------------------

/// Estimate total tokens across all messages.
pub fn estimate_total_tokens(messages: &[AgentMessage]) -> u32 {
    messages.iter().map(estimate_message_tokens).sum()
}

/// Check whether compression is needed for the given messages and settings.
pub fn should_compress(messages: &[AgentMessage], settings: &CompressionSettings) -> bool {
    if messages.is_empty() {
        return false;
    }
    let total_tokens = estimate_total_tokens(messages);
    total_tokens > settings.budget()
}

/// Find the cut-point index: messages before this index are "old" (to be
/// discarded), messages from this index onward are "recent" (to be kept).
///
/// Walks backwards from the end, accumulating token estimates.
/// Never cuts between an assistant message with tool calls and its
/// corresponding tool results.
pub fn find_cut_point(
    messages: &[AgentMessage],
    token_estimates: &[u32],
    keep_recent_tokens: u32,
) -> usize {
    let mut accumulated = 0u32;
    let mut cut = messages.len();

    for i in (0..messages.len()).rev() {
        accumulated += token_estimates[i];

        if accumulated >= keep_recent_tokens {
            cut = i;
            break;
        }
        cut = i;
    }

    // Adjust cut point: never cut between an assistant (with tool calls) and its tool results.
    while cut > 0 && cut < messages.len() {
        match &messages[cut] {
            AgentMessage::ToolResult(_) => {
                // This tool result belongs to the previous assistant message — include it
                cut -= 1;
            }
            AgentMessage::Assistant(assistant) if assistant.has_tool_calls() => {
                if cut + 1 < messages.len() {
                    if matches!(&messages[cut + 1], AgentMessage::ToolResult(_)) {
                        cut -= 1;
                        continue;
                    }
                }
                break;
            }
            _ => break,
        }
    }

    cut
}

/// Build the compressed message list given a summary string and the recent
/// messages to keep.
///
/// The summary is injected as the first `AgentMessage::User` message, followed
/// by the recent messages (with large tool results truncated).
pub fn build_compressed_messages(
    summary: &str,
    recent_messages: &[AgentMessage],
) -> Vec<AgentMessage> {
    let summary_message = AgentMessage::User(UserMessage::text(summary.to_string()));

    let mut result = Vec::with_capacity(1 + recent_messages.len());
    result.push(summary_message);

    for msg in recent_messages {
        result.push(maybe_truncate_tool_result(msg.clone(), false));
    }

    result
}

// ---------------------------------------------------------------------------
// Legacy / fallback: compress_context (pure truncation, no LLM summary)
// ---------------------------------------------------------------------------

/// Compress the message list using pure truncation with a **heuristic**
/// summary (no LLM call).
///
/// This is the safety-net path used when the LLM summary generation fails
/// or is cancelled. Rather than silently dropping all old messages — which
/// would leave the user with no trace of earlier context — we synthesize a
/// best-effort structural summary (user topics, tools used, assistant
/// actions) via `generate_discard_summary` and inject it as the first
/// message, preserving at least a skeleton of earlier conversation.
///
/// For the primary compression path (with LLM summary), use
/// `should_compress` → `find_cut_point` → `build_compressed_messages` instead.
pub fn compress_context_fallback(
    messages: Vec<AgentMessage>,
    settings: &CompressionSettings,
) -> Vec<AgentMessage> {
    if messages.is_empty() {
        return messages;
    }

    let budget = settings.budget();

    let token_estimates: Vec<u32> = messages.iter().map(estimate_message_tokens).collect();
    let total_tokens: u32 = token_estimates.iter().sum();

    if total_tokens <= budget {
        return messages;
    }

    tracing::warn!(
        total_tokens,
        budget,
        message_count = messages.len(),
        "Context compression fallback triggered (LLM summary failed)"
    );

    let cut_index = find_cut_point(&messages, &token_estimates, settings.keep_recent_tokens);

    // Not enough room for a summary+recent split: at least truncate old tool
    // results in-place so the context shrinks, but leave the structural
    // ordering untouched.
    if cut_index == 0 || messages.len() - cut_index < MIN_MESSAGES_TO_KEEP {
        return truncate_old_tool_results(messages, budget, &token_estimates);
    }

    let (old_messages, recent_messages) = messages.split_at(cut_index);
    let heuristic_summary = generate_discard_summary(old_messages);

    let mut result = Vec::with_capacity(1 + recent_messages.len());
    result.push(AgentMessage::User(UserMessage::text(heuristic_summary)));
    for msg in recent_messages {
        result.push(maybe_truncate_tool_result(msg.clone(), false));
    }

    tracing::info!(
        discarded = cut_index,
        kept = result.len(),
        "Context compression fallback completed (heuristic summary injected)"
    );

    result
}

/// Generate a structural summary of discarded messages when no LLM is
/// available.
///
/// Produces a best-effort `<context_summary>` block with:
/// - User requests (first 200 chars per turn, max 5 turns)
/// - Tools used (unique, sorted)
/// - Assistant actions (first 150 chars per turn, max 5 turns)
/// - Compressed-message count
///
/// This is **deliberately mechanical** — it does not attempt to understand
/// intent, only to leave a structural skeleton so the model can tell what
/// kind of conversation happened before the reset. The block is wrapped in
/// `<context_summary>…</context_summary>` so downstream code (e.g.
/// `detect_prior_summary`) can treat it identically to an LLM-generated
/// summary.
pub fn generate_discard_summary(messages: &[AgentMessage]) -> String {
    let mut user_topics: Vec<String> = Vec::new();
    let mut tool_names: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut assistant_actions: Vec<String> = Vec::new();

    for msg in messages {
        match msg {
            AgentMessage::User(user_msg) => {
                let text = match &user_msg.content {
                    tiycore::types::UserContent::Text(t) => t.clone(),
                    tiycore::types::UserContent::Blocks(blocks) => blocks
                        .iter()
                        .filter_map(|b| match b {
                            ContentBlock::Text(tc) => Some(tc.text.as_str()),
                            _ => None,
                        })
                        .collect::<Vec<_>>()
                        .join(" "),
                };
                let topic = truncate_text_chars(text.trim(), 200);
                if !topic.is_empty() {
                    user_topics.push(topic);
                }
            }
            AgentMessage::Assistant(assistant_msg) => {
                for block in &assistant_msg.content {
                    if let ContentBlock::ToolCall(tc) = block {
                        tool_names.insert(tc.name.clone());
                    }
                }
                let text = assistant_msg
                    .content
                    .iter()
                    .filter_map(|b| match b {
                        ContentBlock::Text(tc) => Some(tc.text.as_str()),
                        _ => None,
                    })
                    .collect::<Vec<_>>()
                    .join(" ");
                let brief = truncate_text_chars(text.trim(), 150);
                if !brief.is_empty() {
                    assistant_actions.push(brief);
                }
            }
            AgentMessage::ToolResult(tool_result) => {
                tool_names.insert(tool_result.tool_name.clone());
            }
            AgentMessage::Custom { .. } => {}
        }
    }

    let mut out = String::from("<context_summary>\n");
    out.push_str(
        "Heuristic summary (LLM summary unavailable) of earlier conversation that was compressed to fit the context window.\n\n",
    );

    if !user_topics.is_empty() {
        out.push_str("## User Requests\n");
        for (i, topic) in user_topics.iter().enumerate().take(5) {
            out.push_str(&format!("{}. {}\n", i + 1, topic));
        }
        if user_topics.len() > 5 {
            out.push_str(&format!(
                "... and {} more requests\n",
                user_topics.len() - 5
            ));
        }
        out.push('\n');
    }

    if !tool_names.is_empty() {
        out.push_str("## Tools Used\n");
        let mut sorted_tools: Vec<_> = tool_names.into_iter().collect();
        sorted_tools.sort();
        out.push_str(&sorted_tools.join(", "));
        out.push_str("\n\n");
    }

    if !assistant_actions.is_empty() {
        out.push_str("## Key Actions\n");
        for (i, action) in assistant_actions.iter().enumerate().take(5) {
            out.push_str(&format!("{}. {}\n", i + 1, action));
        }
        if assistant_actions.len() > 5 {
            out.push_str(&format!(
                "... and {} more actions\n",
                assistant_actions.len() - 5
            ));
        }
        out.push('\n');
    }

    out.push_str(&format!("Total messages compressed: {}\n", messages.len()));
    out.push_str("</context_summary>");

    out
}

/// Truncate a string to `max_chars` (counted in `char`s, not bytes) and
/// append an ellipsis if truncation occurred. Guarantees char-boundary
/// safety for CJK / multi-byte input.
fn truncate_text_chars(text: &str, max_chars: usize) -> String {
    let mut iter = text.char_indices();
    match iter.nth(max_chars) {
        Some((byte_idx, _)) => format!("{}...", &text[..byte_idx]),
        None => text.to_string(),
    }
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// When we can't find a good cut point, at least truncate old tool results.
fn truncate_old_tool_results(
    messages: Vec<AgentMessage>,
    _budget: u32,
    _token_estimates: &[u32],
) -> Vec<AgentMessage> {
    let old_boundary = messages.len() * 2 / 3;

    messages
        .into_iter()
        .enumerate()
        .map(|(i, msg)| {
            if i < old_boundary {
                maybe_truncate_tool_result(msg, true)
            } else {
                msg
            }
        })
        .collect()
}

/// Truncate a tool result message if it exceeds the limit.
fn maybe_truncate_tool_result(message: AgentMessage, aggressive: bool) -> AgentMessage {
    match message {
        AgentMessage::ToolResult(mut tool_result) => {
            let max_chars = if aggressive {
                OLD_TOOL_RESULT_MAX_CHARS
            } else {
                OLD_TOOL_RESULT_MAX_CHARS * 4 // 3200 chars for recent messages
            };

            let total_chars: usize = tool_result
                .content
                .iter()
                .map(|block| match block {
                    ContentBlock::Text(tc) => tc.text.len(),
                    _ => 0,
                })
                .sum();

            if total_chars > max_chars {
                tool_result.content = tool_result
                    .content
                    .into_iter()
                    .map(|block| match block {
                        ContentBlock::Text(tc) if tc.text.len() > max_chars => {
                            let mut truncated = tc.text;
                            truncated.truncate(max_chars);
                            while !truncated.is_char_boundary(truncated.len()) {
                                truncated.pop();
                            }
                            ContentBlock::Text(TextContent::new(format!(
                                "{}\n\n[Tool output truncated: {} chars → {} chars]",
                                truncated, total_chars, max_chars
                            )))
                        }
                        other => other,
                    })
                    .collect();
            }

            AgentMessage::ToolResult(tool_result)
        }
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tiycore::types::{
        Api, AssistantMessage, ContentBlock, Provider, StopReason, TextContent, ToolCall,
        ToolResultMessage, Usage, UserMessage,
    };

    fn make_user(text: &str) -> AgentMessage {
        AgentMessage::User(UserMessage::text(text))
    }

    fn make_assistant(text: &str) -> AgentMessage {
        AgentMessage::Assistant(
            AssistantMessage::builder()
                .content(vec![ContentBlock::Text(TextContent::new(text))])
                .api(Api::OpenAICompletions)
                .provider(Provider::OpenAI)
                .model("test")
                .usage(Usage::default())
                .stop_reason(StopReason::Stop)
                .build()
                .unwrap(),
        )
    }

    fn make_assistant_with_tool_call(tool_name: &str) -> AgentMessage {
        AgentMessage::Assistant(
            AssistantMessage::builder()
                .content(vec![ContentBlock::ToolCall(ToolCall::new(
                    "tc-1",
                    tool_name,
                    serde_json::json!({}),
                ))])
                .api(Api::OpenAICompletions)
                .provider(Provider::OpenAI)
                .model("test")
                .usage(Usage::default())
                .stop_reason(StopReason::ToolUse)
                .build()
                .unwrap(),
        )
    }

    fn make_tool_result(tool_name: &str, content: &str) -> AgentMessage {
        AgentMessage::ToolResult(ToolResultMessage::text("tc-1", tool_name, content, false))
    }

    fn settings(context_window: u32) -> CompressionSettings {
        CompressionSettings::new(context_window)
    }

    #[test]
    fn test_estimate_tokens() {
        assert_eq!(estimate_tokens(""), 0);
        assert_eq!(estimate_tokens("hello world"), 3);
        assert_eq!(estimate_tokens("a"), 1);
        assert_eq!(estimate_tokens("abcdefgh"), 2);
    }

    #[test]
    fn estimate_tokens_weights_cjk_higher_than_ascii_heuristic() {
        // 10 Chinese chars = 30 UTF-8 bytes. Plain chars/4 would estimate
        // (30+3)/4 = 8 tokens, which is a serious underestimate for real
        // tokenisers. The CJK-aware estimate should return approximately one
        // token per CJK char (i.e. ≥ 10).
        let cjk = "你好世界你好世界你好";
        assert_eq!(cjk.chars().count(), 10);
        let estimate = estimate_tokens(cjk);
        assert!(
            estimate >= 10,
            "CJK estimate should be at least one per char, got {}",
            estimate
        );
    }

    #[test]
    fn estimate_tokens_preserves_ascii_only_heuristic() {
        // The ASCII fast path must not change behaviour — the original chars/4
        // formula is still the right answer for English/code and any existing
        // token-budget thresholds depend on it.
        for text in &[
            "",
            "a",
            "hello world",
            "abcdefgh",
            "The quick brown fox jumps over the lazy dog",
        ] {
            let base = (text.len() as u32).div_ceil(4);
            assert_eq!(
                estimate_tokens(text),
                base,
                "ASCII text '{}' should still match chars/4",
                text
            );
        }
    }

    #[test]
    fn estimate_tokens_handles_mixed_cjk_and_ascii() {
        // Japanese/Chinese mixed with Latin punctuation — the two populations
        // are estimated separately and summed.
        let mixed = "Hello, 世界! 这是一个测试.";
        let est = estimate_tokens(mixed);
        let cjk_count = mixed.chars().filter(|c| is_cjk_like(*c)).count() as u32;
        assert!(
            est >= cjk_count,
            "Mixed text should be at least cjk_count ({}) tokens, got {}",
            cjk_count,
            est
        );
    }

    #[test]
    fn test_no_compression_when_within_budget() {
        let messages = vec![make_user("Hello"), make_assistant("Hi there!")];
        let s = settings(128_000);
        assert!(!should_compress(&messages, &s));
    }

    #[test]
    fn test_compression_triggers_when_over_budget() {
        let mut messages = Vec::new();
        for i in 0..20 {
            messages.push(make_user(&format!("Question {}: {}", i, "x".repeat(500))));
            messages.push(make_assistant(&format!(
                "Answer {}: {}",
                i,
                "y".repeat(500)
            )));
        }

        let s = CompressionSettings {
            context_window: 2000,
            reserve_tokens: 500,
            keep_recent_tokens: 500,
        };

        assert!(should_compress(&messages, &s));

        let result = compress_context_fallback(messages.clone(), &s);
        assert!(result.len() < messages.len());
    }

    #[test]
    fn test_cut_point_respects_tool_result_boundary() {
        let messages = vec![
            make_user("Do something"),
            make_assistant_with_tool_call("read"),
            make_tool_result("read", "file contents here"),
            make_user("Now do something else"),
            make_assistant("OK, done"),
        ];

        let token_estimates: Vec<u32> = messages.iter().map(estimate_message_tokens).collect();

        let cut = find_cut_point(&messages, &token_estimates, 10);
        assert!(
            cut == 0 || cut == 3,
            "Cut point was {}, expected 0 or 3",
            cut
        );
    }

    #[test]
    fn test_tool_result_truncation() {
        let big_content = "x".repeat(10_000);
        let msg = make_tool_result("read", &big_content);

        let truncated = maybe_truncate_tool_result(msg, true);
        if let AgentMessage::ToolResult(tr) = &truncated {
            let text = tr.text_content();
            assert!(text.len() < 10_000, "Should be truncated");
            assert!(
                text.contains("[Tool output truncated:"),
                "Should have truncation marker"
            );
        } else {
            panic!("Expected ToolResult");
        }
    }

    #[test]
    fn test_build_compressed_messages() {
        let recent = vec![make_user("What next?"), make_assistant("Let me check.")];
        let result = build_compressed_messages("Summary of earlier conversation", &recent);
        assert_eq!(result.len(), 3); // 1 summary + 2 recent
                                     // First message is the summary
        if let AgentMessage::User(u) = &result[0] {
            let text = match &u.content {
                tiycore::types::UserContent::Text(t) => t.as_str(),
                _ => panic!("Expected text content"),
            };
            assert_eq!(text, "Summary of earlier conversation");
        } else {
            panic!("Expected User message");
        }
    }

    #[test]
    fn test_estimate_total_tokens() {
        let messages = vec![make_user("Hello"), make_assistant("Hi there!")];
        let total = estimate_total_tokens(&messages);
        assert!(total > 0);
        // Should equal sum of individual estimates
        let sum: u32 = messages.iter().map(estimate_message_tokens).sum();
        assert_eq!(total, sum);
    }

    #[test]
    fn generate_discard_summary_wraps_with_context_summary_tags() {
        let messages = vec![
            make_user("Please refactor the context compression module."),
            make_assistant_with_tool_call("read"),
            make_tool_result("read", "<file contents>"),
            make_assistant("I read the file and will refactor."),
        ];
        let summary = generate_discard_summary(&messages);
        assert!(summary.starts_with("<context_summary>"));
        assert!(summary.trim_end().ends_with("</context_summary>"));
        assert!(summary.contains("User Requests"));
        assert!(summary.contains("Tools Used"));
        assert!(summary.contains("read"));
        assert!(summary.contains("Total messages compressed: 4"));
    }

    #[test]
    fn generate_discard_summary_handles_cjk_without_panicking() {
        // Heuristic truncation must be char-boundary safe for multibyte input.
        let long_cjk: String = "这是一条很长的中文用户请求".repeat(40);
        let messages = vec![make_user(&long_cjk)];
        let summary = generate_discard_summary(&messages);
        assert!(summary.contains("User Requests"));
        // No panic = char-boundary safe.
    }

    #[test]
    fn compress_context_fallback_injects_heuristic_summary_when_over_budget() {
        // Build a thread large enough to force compression, with a clear
        // assistant+user boundary so find_cut_point can pick a sane split.
        let mut messages = Vec::new();
        for i in 0..20 {
            messages.push(make_user(&format!("Question {}: {}", i, "x".repeat(400))));
            messages.push(make_assistant(&format!(
                "Answer {}: {}",
                i,
                "y".repeat(400)
            )));
        }

        let s = CompressionSettings {
            context_window: 2000,
            reserve_tokens: 500,
            keep_recent_tokens: 500,
        };
        assert!(should_compress(&messages, &s));

        let result = compress_context_fallback(messages.clone(), &s);
        assert!(result.len() < messages.len());

        // The first message should be our heuristic <context_summary> so the
        // user never fully loses the skeleton of earlier context.
        match &result[0] {
            AgentMessage::User(u) => {
                let text = match &u.content {
                    tiycore::types::UserContent::Text(t) => t.as_str(),
                    _ => panic!("expected text user content"),
                };
                assert!(
                    text.starts_with("<context_summary>"),
                    "fallback should inject a heuristic summary, got: {}",
                    text
                );
            }
            other => panic!("expected heuristic summary at head, got {:?}", other),
        }
    }
}
