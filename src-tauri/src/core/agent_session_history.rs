use base64::{engine::general_purpose, Engine as _};

use tiycore::agent::AgentMessage;
use tiycore::types::{
    AssistantMessage, ContentBlock, ImageContent, Model, StopReason, TextContent, ThinkingContent,
    ToolCall, ToolResultMessage, Usage, UserMessage,
};

use crate::core::agent_session_tools::{
    assistant_message_with_blocks, effective_api_for_model, HISTORY_TOOL_RESULT_MAX_CHARS,
};
use crate::core::plan_checkpoint::{parse_plan_message_metadata, plan_markdown};
use crate::model::thread::{MessageAttachmentDto, MessageRecord, ToolCallDto};

use super::agent_session::TEXT_ATTACHMENT_MAX_CHARS;

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

pub(crate) fn convert_history_messages(
    messages: &[MessageRecord],
    tool_calls: &[ToolCallDto],
    model: &Model,
) -> Vec<AgentMessage> {
    // Timeline entries: either a real message or a pending-thinking placeholder
    // that will be merged into the next assistant message after sorting.
    enum TimelineEntry {
        Msg(AgentMessage),
        PendingThinking(ContentBlock),
    }

    // Phase 1: Convert message records into (sort_key, TimelineEntry) pairs.
    // Messages arrive from the DB in chronological order (sorted by UUID v7 id).
    // We use a zero-padded positional index as the primary sort key to preserve
    // this order exactly, then interleave tool calls using their `started_at`
    // timestamp mapped into the same key space.
    //
    // Reasoning messages are placed into the timeline as PendingThinking
    // placeholders instead of being eagerly drained into the next assistant
    // text message.  This allows the post-sort pass (Phase 4) to attach
    // thinking blocks to whichever assistant message follows them — whether
    // that is a plain text reply or a tool-call assistant message inserted by
    // Phase 2.
    let mut timeline: Vec<(SortKey, TimelineEntry)> = Vec::new();

    for (pos, message) in messages.iter().enumerate() {
        let key = SortKey::positional(pos);
        match message.message_type.as_str() {
            "reasoning" if message.role == "assistant" => {
                let signature = message
                    .metadata_json
                    .as_deref()
                    .and_then(|json| serde_json::from_str::<serde_json::Value>(json).ok())
                    .and_then(|v| v.get("thinking_signature")?.as_str().map(String::from));
                timeline.push((
                    key,
                    TimelineEntry::PendingThinking(ContentBlock::Thinking(ThinkingContent {
                        thinking: message.content_markdown.clone(),
                        thinking_signature: signature,
                        redacted: false,
                    })),
                ));
            }
            "plain_message" => match message.role.as_str() {
                "user" => {
                    timeline.push((
                        key,
                        TimelineEntry::Msg(AgentMessage::User(history_user_message(
                            message, model,
                        ))),
                    ));
                }
                "assistant" => {
                    // Skip empty assistant plain_messages — these are left
                    // behind when a provider error interrupts the stream
                    // before any text is generated.  An empty Text("") block
                    // serialises to `content: null` in tiycore, which causes
                    // DeepSeek to reject the request (content or tool_calls
                    // must be set).
                    if message.content_markdown.trim().is_empty() {
                        continue;
                    }
                    let blocks = vec![ContentBlock::Text(TextContent::new(
                        &message.content_markdown,
                    ))];
                    timeline.push((
                        key,
                        TimelineEntry::Msg(AgentMessage::Assistant(assistant_message_with_blocks(
                            blocks, model,
                        ))),
                    ));
                }
                _ => {}
            },
            "plan" if message.role == "assistant" => {
                let blocks = vec![ContentBlock::Text(TextContent::new(
                    &format_plan_history_message(message),
                ))];
                timeline.push((
                    key,
                    TimelineEntry::Msg(AgentMessage::Assistant(assistant_message_with_blocks(
                        blocks, model,
                    ))),
                ));
            }
            "summary_marker" if is_context_summary_marker(message) => {
                let summary = message.content_markdown.trim();
                if !summary.is_empty() {
                    timeline.push((
                        key,
                        TimelineEntry::Msg(AgentMessage::User(UserMessage::text(
                            summary.to_string(),
                        ))),
                    ));
                }
            }
            _ => {}
        }
    }

    // Phase 2: Interleave completed tool calls from the tool_calls table.
    //
    // When the preceding assistant text message in the same run exists in the
    // timeline, the tool call is **merged** into that message rather than
    // creating a separate assistant message.  This is critical for providers
    // like DeepSeek that require `reasoning_content` on every assistant message
    // that originally contained it — the original API response has
    // `{reasoning_content, content, tool_calls}` as a single message, and we
    // must reconstruct it the same way.
    //
    // When no preceding assistant text exists (e.g. reasoning → tool_call with
    // no intermediate text), a standalone assistant message is created so that
    // Phase 4 can attach the pending thinking block to it.
    if !tool_calls.is_empty() {
        // Build a lookup: for each message, record (run_id, created_at, position)
        // so we can find where tool calls slot in.
        let msg_positions: Vec<(Option<&str>, &str, usize)> = messages
            .iter()
            .enumerate()
            .map(|(i, m)| (m.run_id.as_deref(), m.created_at.as_str(), i))
            .collect();

        // Build an index from message position → timeline index so we can
        // find and modify existing assistant entries for merging.
        let mut pos_to_timeline_idx: std::collections::HashMap<usize, usize> =
            std::collections::HashMap::new();
        for (tl_idx, (key, _)) in timeline.iter().enumerate() {
            if key.sub == 2 {
                // sub == 2 means a positional message (not a tool call)
                pos_to_timeline_idx.insert(key.position, tl_idx);
            }
        }

        for (tc_idx, tc) in tool_calls.iter().enumerate() {
            if tc.status != "completed" {
                continue;
            }

            // Find the position for this tool call: the position of the first
            // message in the same run whose created_at >= started_at, minus a
            // small delta so the tool call lands just before it.  If no match
            // is found, place it at the end.
            let insert_pos = msg_positions
                .iter()
                .find(|(run_id, created_at, _)| {
                    run_id.map_or(false, |r| r == tc.run_id)
                        && *created_at >= tc.started_at.as_str()
                })
                .map(|(_, _, pos)| *pos)
                .unwrap_or(messages.len());

            // Try to find the preceding assistant text message in the same run
            // to merge this tool call into.  We search backwards from
            // insert_pos to find the last plain_message assistant in the same
            // run.  This handles the common case where a single API response
            // contains reasoning_content + content + tool_calls.
            //
            // We only merge when there is **no** reasoning message between the
            // candidate text message and insert_pos — a reasoning message in
            // between indicates the tool call came from a different model
            // response and must remain a separate assistant message.
            let merge_target_pos = msg_positions[..insert_pos]
                .iter()
                .rev()
                .find(|(run_id, _, pos)| {
                    run_id.map_or(false, |r| r == tc.run_id) && {
                        let m = &messages[*pos];
                        m.role == "assistant" && m.message_type == "plain_message"
                    }
                })
                .map(|(_, _, pos)| *pos)
                .filter(|&merge_pos| {
                    // Reject when any reasoning message from the same run sits
                    // between the merge target and the insert position.
                    !msg_positions[merge_pos + 1..insert_pos]
                        .iter()
                        .any(|(run_id, _, pos)| {
                            run_id.map_or(false, |r| r == tc.run_id)
                                && messages[*pos].message_type == "reasoning"
                        })
                });

            let tool_call_block =
                ContentBlock::ToolCall(ToolCall::new(&tc.id, &tc.tool_name, tc.tool_input.clone()));

            if let Some(merge_pos) = merge_target_pos {
                if let Some(&tl_idx) = pos_to_timeline_idx.get(&merge_pos) {
                    // Merge the tool call into the existing assistant message.
                    if let (_, TimelineEntry::Msg(AgentMessage::Assistant(ref mut assistant))) =
                        &mut timeline[tl_idx]
                    {
                        assistant.content.push(tool_call_block);
                        assistant.stop_reason = StopReason::ToolUse;
                    }

                    // Place the tool result right after the merged message but
                    // before the next positional entry.
                    let result_text = tc
                        .tool_output
                        .as_ref()
                        .map(|v| {
                            truncate_tool_result_text(&v.to_string(), HISTORY_TOOL_RESULT_MAX_CHARS)
                        })
                        .unwrap_or_else(|| "[no output]".to_string());
                    let tool_result =
                        ToolResultMessage::text(&tc.id, &tc.tool_name, result_text, false);
                    // Use the merge_pos + 1 so the result sorts right after
                    // the merged assistant message (sub=0 < sub=2).
                    timeline.push((
                        SortKey::before_position(merge_pos + 1, tc_idx * 2 + 1),
                        TimelineEntry::Msg(AgentMessage::ToolResult(tool_result)),
                    ));

                    continue;
                }
            }

            // No preceding assistant text to merge into — create a standalone
            // assistant message (Phase 4 will attach any pending thinking).
            //
            // However, if a previous standalone tool-call assistant was already
            // inserted at the same insert_pos (meaning the two tool calls come
            // from the same API response), merge into that message instead of
            // creating yet another standalone.  This avoids the problem where
            // only the first standalone gets reasoning_content from Phase 4.
            let merged_into_prev_standalone = timeline
                .iter()
                .rposition(|(key, entry)| {
                    key.position == insert_pos
                        && key.sub == 3
                        && matches!(entry, TimelineEntry::Msg(AgentMessage::Assistant(_)))
                })
                .and_then(|tl_idx| {
                    if let (_, TimelineEntry::Msg(AgentMessage::Assistant(ref mut prev))) =
                        &mut timeline[tl_idx]
                    {
                        prev.content.push(tool_call_block.clone());
                        Some(tl_idx)
                    } else {
                        None
                    }
                })
                .is_some();

            if !merged_into_prev_standalone {
                let assistant = AssistantMessage::builder()
                    .content(vec![tool_call_block])
                    .api(effective_api_for_model(model))
                    .provider(model.provider.clone())
                    .model(model.id.clone())
                    .usage(Usage::default())
                    .stop_reason(StopReason::ToolUse)
                    .build()
                    .expect("history tool-call assistant message should always build");
                timeline.push((
                    SortKey::after_position(insert_pos, tc_idx * 2),
                    TimelineEntry::Msg(AgentMessage::Assistant(assistant)),
                ));
            }

            // Build the tool result message (truncated to avoid blowing up context).
            let result_text = tc
                .tool_output
                .as_ref()
                .map(|v| truncate_tool_result_text(&v.to_string(), HISTORY_TOOL_RESULT_MAX_CHARS))
                .unwrap_or_else(|| "[no output]".to_string());

            let tool_result = ToolResultMessage::text(&tc.id, &tc.tool_name, result_text, false);
            // Standalone and merged-into-standalone tool results use
            // after_position so they sort after reasoning PendingThinking
            // at the same position (matching the standalone assistant key).
            timeline.push((
                SortKey::after_position(insert_pos, tc_idx * 2 + 1),
                TimelineEntry::Msg(AgentMessage::ToolResult(tool_result)),
            ));
        }
    }

    // Phase 3: Sort by the sort key to produce a chronological sequence.
    timeline.sort_by(|a, b| a.0.cmp(&b.0));

    // Phase 4: Post-sort pass — attach pending thinking blocks to the next
    // assistant message.  This correctly associates reasoning with both plain
    // text replies and tool-call assistant messages that were interleaved by
    // Phase 2.
    let mut result: Vec<AgentMessage> = Vec::new();
    let mut pending_thinking: Vec<ContentBlock> = Vec::new();

    for (_, entry) in timeline {
        match entry {
            TimelineEntry::PendingThinking(block) => {
                pending_thinking.push(block);
            }
            TimelineEntry::Msg(AgentMessage::Assistant(mut assistant)) => {
                if !pending_thinking.is_empty() {
                    // Prepend accumulated thinking blocks before the existing content.
                    let mut merged = pending_thinking.drain(..).collect::<Vec<_>>();
                    merged.append(&mut assistant.content);
                    assistant.content = merged;
                }
                result.push(AgentMessage::Assistant(assistant));
            }
            TimelineEntry::Msg(msg @ AgentMessage::User(_)) => {
                // Discard any pending thinking that precedes a user message
                // (should not happen in normal flow, but be defensive).
                pending_thinking.clear();
                result.push(msg);
            }
            TimelineEntry::Msg(msg) => {
                result.push(msg);
            }
        }
    }
    // Any remaining pending_thinking blocks (orphan reasoning at the end with
    // no following assistant message) are silently dropped.

    result
}

/// Sort key for interleaving messages and tool calls chronologically.
///
/// Messages get a whole-number position (their index in the original list).
/// Tool calls are placed relative to a specific position using sub-keys.
///
/// Sub-key ordering (ascending):
///   0 = merged tool-call result placed *before* a positional message
///   2 = positional message (Phase 1 text / reasoning / plan entries)
///   3 = standalone tool-call assistant + result placed *after* positional
///       messages so that Phase 4 can attach preceding PendingThinking
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct SortKey {
    /// Main position (message index).
    position: usize,
    /// Determines ordering within the same position — see doc above.
    sub: u8,
    /// Tiebreaker for multiple entries at the same (position, sub).
    seq: usize,
}

impl SortKey {
    pub(crate) fn positional(pos: usize) -> Self {
        Self {
            position: pos,
            sub: 2,
            seq: 0,
        }
    }

    pub(crate) fn before_position(pos: usize, seq: usize) -> Self {
        Self {
            position: pos,
            sub: 0,
            seq,
        }
    }

    /// Place a standalone tool-call assistant (or its result) **after** all
    /// positional entries at the same position.  This ensures Phase 4's
    /// `PendingThinking` from reasoning messages at this position is
    /// accumulated *before* the standalone is processed, so the standalone
    /// receives the correct `reasoning_content`.
    pub(crate) fn after_position(pos: usize, seq: usize) -> Self {
        Self {
            position: pos,
            sub: 3,
            seq,
        }
    }
}

/// Truncate a tool result string to at most `max_chars` **characters**, appending a marker.
fn truncate_tool_result_text(text: &str, max_chars: usize) -> String {
    let total_chars = text.chars().count();
    if total_chars <= max_chars {
        return text.to_string();
    }
    // Collect the byte offset after the first `max_chars` characters so we
    // slice on a valid UTF-8 boundary (no panic on multi-byte chars).
    let byte_end = text
        .char_indices()
        .nth(max_chars)
        .map(|(idx, _)| idx)
        .unwrap_or(text.len());
    let truncated = &text[..byte_end];
    format!(
        "{}\n\n[Tool output truncated: {} chars → {} chars]",
        truncated, total_chars, max_chars
    )
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
