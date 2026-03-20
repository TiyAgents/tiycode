//! Context compression (compaction) for managing LLM context window limits.
//!
//! Inspired by pi-mono's compaction system, this module provides token-budget-aware
//! context window management via the `transform_context` hook.
//!
//! ## Strategy
//!
//! 1. **Token estimation**: `chars / 4` heuristic (matches pi-mono).
//! 2. **Threshold check**: Compress when `estimated_tokens > context_window - reserve_tokens`.
//! 3. **Cut-point detection**: Walk backwards from newest message, keep `keep_recent_tokens`
//!    worth of recent messages. Never cut in the middle of an assistant+tool_result pair.
//! 4. **Tool result truncation**: Older tool results beyond the keep-window are summarized
//!    to a short descriptor: `[Tool result: {tool_name}, truncated]`.
//! 5. **Summary injection**: Old messages before the cut-point are replaced with a single
//!    `AgentMessage::User` containing a structured summary of what was discarded.

use tiy_core::agent::AgentMessage;
use tiy_core::types::{ContentBlock, TextContent, UserMessage};

/// Reserve this many tokens for the model's response + overhead.
/// Matches pi-mono `DEFAULT_COMPACTION_SETTINGS.reserveTokens`.
const RESERVE_TOKENS: u32 = 16_384;

/// Keep at least this many tokens of recent conversation untouched.
/// Matches pi-mono `DEFAULT_COMPACTION_SETTINGS.keepRecentTokens`.
const KEEP_RECENT_TOKENS: u32 = 20_000;

/// Maximum characters for a tool result in the "old" region.
/// Longer results are truncated with a summary marker.
const OLD_TOOL_RESULT_MAX_CHARS: usize = 800;

/// Minimum number of messages to keep (never compress below this).
const MIN_MESSAGES_TO_KEEP: usize = 4;

/// Estimate the number of tokens for a string using the chars/4 heuristic.
///
/// This matches pi-mono's `estimateTokens()` and is a reasonable approximation
/// for most LLM tokenizers (GPT, Claude, etc.).
fn estimate_tokens(text: &str) -> u32 {
    // For CJK-heavy text, chars/2 might be more accurate, but chars/4 is
    // the industry standard heuristic that pi-mono uses.
    (text.len() as u32).saturating_add(3) / 4
}

/// Estimate tokens for a single `AgentMessage`.
fn estimate_message_tokens(message: &AgentMessage) -> u32 {
    match message {
        AgentMessage::User(user_msg) => {
            let text = match &user_msg.content {
                tiy_core::types::UserContent::Text(t) => t.as_str(),
                tiy_core::types::UserContent::Blocks(blocks) => {
                    // Sum all text blocks; images contribute a fixed overhead
                    let mut total = 0u32;
                    for block in blocks {
                        total += match block {
                            ContentBlock::Text(tc) => estimate_tokens(&tc.text),
                            ContentBlock::Image(_) => 1000, // rough image token estimate
                            _ => 10,
                        };
                    }
                    return total + 4; // message overhead
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
                        // tool name + JSON args
                        estimate_tokens(&tc.name) + estimate_tokens(&tc.arguments.to_string()) + 20
                        // overhead for tool call framing
                    }
                    ContentBlock::Image(_) => 1000,
                };
            }
            total + 4 // message overhead
        }
        AgentMessage::ToolResult(tool_result) => {
            let mut total = 0u32;
            for block in &tool_result.content {
                total += match block {
                    ContentBlock::Text(tc) => estimate_tokens(&tc.text),
                    _ => 10,
                };
            }
            total + 10 // overhead for tool_call_id, tool_name, etc.
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
    fn budget(&self) -> u32 {
        self.context_window.saturating_sub(self.reserve_tokens)
    }
}

/// Compress the message list to fit within the context window budget.
///
/// This is designed to be called from `Agent::set_transform_context()`.
///
/// ## Algorithm
///
/// 1. Estimate total tokens across all messages.
/// 2. If within budget, return messages unchanged.
/// 3. Find a "cut point" that preserves the most recent `keep_recent_tokens`.
/// 4. Truncate old tool results in the discarded region.
/// 5. Replace discarded messages with a summary marker.
pub fn compress_context(
    messages: Vec<AgentMessage>,
    settings: &CompressionSettings,
) -> Vec<AgentMessage> {
    if messages.is_empty() {
        return messages;
    }

    let budget = settings.budget();

    // Phase 1: Estimate total tokens
    let token_estimates: Vec<u32> = messages.iter().map(estimate_message_tokens).collect();
    let total_tokens: u32 = token_estimates.iter().sum();

    // If within budget, no compression needed
    if total_tokens <= budget {
        return messages;
    }

    tracing::info!(
        total_tokens,
        budget,
        message_count = messages.len(),
        "Context compression triggered"
    );

    // Phase 2: Find the cut point
    // Walk backwards from the end, accumulating tokens until we reach keep_recent_tokens.
    let cut_index = find_cut_point(&messages, &token_estimates, settings.keep_recent_tokens);

    // Safety: never discard everything
    if cut_index == 0 || messages.len() - cut_index < MIN_MESSAGES_TO_KEEP {
        // Can't compress meaningfully — try just truncating old tool results
        return truncate_old_tool_results(messages, budget, &token_estimates);
    }

    // Phase 3: Build the compressed message list
    let old_messages = &messages[..cut_index];
    let recent_messages = &messages[cut_index..];

    // Generate a summary of discarded messages
    let summary = generate_discard_summary(old_messages);
    let summary_message = AgentMessage::User(UserMessage::text(summary));

    let mut result = Vec::with_capacity(1 + recent_messages.len());
    result.push(summary_message);

    // Phase 4: For recent messages, truncate large tool results that are still too big
    for msg in recent_messages {
        result.push(maybe_truncate_tool_result(msg.clone(), false));
    }

    tracing::info!(
        discarded = cut_index,
        kept = result.len(),
        "Context compression completed"
    );

    result
}

/// Find the cut point index: the first message index where we start keeping messages.
///
/// Walks backwards from the end, accumulating token estimates.
/// Never cuts between an assistant message with tool calls and its corresponding tool results.
fn find_cut_point(
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
    // Walk backwards from cut to find a safe boundary.
    // A safe cut point is:
    //   - Before a User message, OR
    //   - At position 0
    while cut > 0 && cut < messages.len() {
        match &messages[cut] {
            AgentMessage::ToolResult(_) => {
                // This tool result belongs to the previous assistant message — include it
                cut -= 1;
            }
            AgentMessage::Assistant(assistant) if assistant.has_tool_calls() => {
                // Don't cut right before an assistant with tool calls if its results follow
                // Check if next message is a tool result
                if cut + 1 < messages.len() {
                    if matches!(&messages[cut + 1], AgentMessage::ToolResult(_)) {
                        // The tool results are already in the "keep" region — we need to
                        // include this assistant message too
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

/// When we can't find a good cut point, at least truncate old tool results.
fn truncate_old_tool_results(
    messages: Vec<AgentMessage>,
    _budget: u32,
    _token_estimates: &[u32],
) -> Vec<AgentMessage> {
    // Truncate tool results in the first 2/3 of messages (the "old" region)
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
                // Truncate the text content
                tool_result.content = tool_result
                    .content
                    .into_iter()
                    .map(|block| match block {
                        ContentBlock::Text(tc) if tc.text.len() > max_chars => {
                            let mut truncated = tc.text;
                            truncated.truncate(max_chars);
                            // Ensure valid UTF-8 boundary
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

/// Generate a structured summary of discarded messages.
///
/// This follows pi-mono's summary format with key sections.
fn generate_discard_summary(messages: &[AgentMessage]) -> String {
    let mut user_topics = Vec::new();
    let mut tool_names = std::collections::HashSet::new();
    let mut assistant_actions = Vec::new();

    for msg in messages {
        match msg {
            AgentMessage::User(user_msg) => {
                let text = match &user_msg.content {
                    tiy_core::types::UserContent::Text(t) => t.clone(),
                    tiy_core::types::UserContent::Blocks(blocks) => blocks
                        .iter()
                        .filter_map(|b| b.as_text())
                        .map(|t| t.text.as_str())
                        .collect::<Vec<_>>()
                        .join(" "),
                };
                // Take first 200 chars as a topic summary
                let summary = if text.len() > 200 {
                    format!(
                        "{}...",
                        &text[..text.char_indices().nth(200).map_or(text.len(), |(i, _)| i)]
                    )
                } else {
                    text
                };
                if !summary.trim().is_empty() {
                    user_topics.push(summary);
                }
            }
            AgentMessage::Assistant(assistant_msg) => {
                // Track tool calls made
                for tc in assistant_msg.tool_calls() {
                    tool_names.insert(tc.name.clone());
                }
                // Track brief text responses
                let text = assistant_msg.text_content();
                if !text.is_empty() {
                    let brief = if text.len() > 150 {
                        format!(
                            "{}...",
                            &text[..text.char_indices().nth(150).map_or(text.len(), |(i, _)| i)]
                        )
                    } else {
                        text
                    };
                    assistant_actions.push(brief);
                }
            }
            AgentMessage::ToolResult(tool_result) => {
                tool_names.insert(tool_result.tool_name.clone());
            }
            _ => {}
        }
    }

    let mut summary = String::from("<context_summary>\n");
    summary.push_str("The following is a summary of earlier conversation that was compressed to fit the context window.\n\n");

    if !user_topics.is_empty() {
        summary.push_str("## User Requests\n");
        for (i, topic) in user_topics.iter().enumerate().take(5) {
            summary.push_str(&format!("{}. {}\n", i + 1, topic));
        }
        if user_topics.len() > 5 {
            summary.push_str(&format!(
                "... and {} more requests\n",
                user_topics.len() - 5
            ));
        }
        summary.push('\n');
    }

    if !tool_names.is_empty() {
        summary.push_str("## Tools Used\n");
        let mut sorted_tools: Vec<_> = tool_names.into_iter().collect();
        sorted_tools.sort();
        summary.push_str(&sorted_tools.join(", "));
        summary.push_str("\n\n");
    }

    if !assistant_actions.is_empty() {
        summary.push_str("## Key Actions\n");
        for (i, action) in assistant_actions.iter().enumerate().take(5) {
            summary.push_str(&format!("{}. {}\n", i + 1, action));
        }
        if assistant_actions.len() > 5 {
            summary.push_str(&format!(
                "... and {} more actions\n",
                assistant_actions.len() - 5
            ));
        }
        summary.push('\n');
    }

    summary.push_str(&format!("Total messages compressed: {}\n", messages.len()));
    summary.push_str("</context_summary>");

    summary
}

#[cfg(test)]
mod tests {
    use super::*;
    use tiy_core::types::{
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
        assert_eq!(estimate_tokens(""), 0); // (0 + 3) / 4 = 0 with integer division
        assert_eq!(estimate_tokens("hello world"), 3); // 11 chars -> (11+3)/4 = 3
        assert_eq!(estimate_tokens("a"), 1); // (1+3)/4 = 1
        assert_eq!(estimate_tokens("abcdefgh"), 2); // (8+3)/4 = 2
    }

    #[test]
    fn test_no_compression_when_within_budget() {
        let messages = vec![make_user("Hello"), make_assistant("Hi there!")];
        let s = settings(128_000);
        let result = compress_context(messages.clone(), &s);
        assert_eq!(result.len(), messages.len());
    }

    #[test]
    fn test_compression_triggers_when_over_budget() {
        // Create a very small context window to force compression
        let mut messages = Vec::new();
        for i in 0..20 {
            messages.push(make_user(&format!("Question {}: {}", i, "x".repeat(500))));
            messages.push(make_assistant(&format!(
                "Answer {}: {}",
                i,
                "y".repeat(500)
            )));
        }

        // With a tiny budget, compression should kick in
        let s = CompressionSettings {
            context_window: 2000,
            reserve_tokens: 500,
            keep_recent_tokens: 500,
        };

        let result = compress_context(messages.clone(), &s);
        // Should have fewer messages than original
        assert!(result.len() < messages.len());
        // First message should be a summary
        assert!(matches!(&result[0], AgentMessage::User(_)));
    }

    #[test]
    fn test_cut_point_respects_tool_result_boundary() {
        let messages = vec![
            make_user("Do something"),
            make_assistant_with_tool_call("read_file"),
            make_tool_result("read_file", "file contents here"),
            make_user("Now do something else"),
            make_assistant("OK, done"),
        ];

        let token_estimates: Vec<u32> = messages.iter().map(estimate_message_tokens).collect();

        // With very low keep_recent_tokens, it should still not cut between
        // assistant+tool_call and tool_result
        let cut = find_cut_point(&messages, &token_estimates, 10);

        // The cut should be at 0 or 3 (before the second user message),
        // but never at 1 or 2 (splitting assistant+tool_result pair)
        assert!(
            cut == 0 || cut == 3,
            "Cut point was {}, expected 0 or 3",
            cut
        );
    }

    #[test]
    fn test_tool_result_truncation() {
        let big_content = "x".repeat(10_000);
        let msg = make_tool_result("read_file", &big_content);

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
    fn test_discard_summary_format() {
        let messages = vec![
            make_user("Help me fix a bug in my code"),
            make_assistant_with_tool_call("read_file"),
            make_tool_result("read_file", "contents..."),
            make_assistant("I found the issue. Let me fix it."),
        ];

        let summary = generate_discard_summary(&messages);
        assert!(summary.contains("<context_summary>"));
        assert!(summary.contains("</context_summary>"));
        assert!(summary.contains("User Requests"));
        assert!(summary.contains("Tools Used"));
        assert!(summary.contains("read_file"));
    }
}
