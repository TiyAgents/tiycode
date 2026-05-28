//! Message formatting and chunking for IM platforms — placeholder for Step 4.

// TODO(step-4): Full Markdown adaptation and platform-specific formatting.

use super::traits::Platform;

/// Maximum text length per message for each platform.
pub fn max_message_length(platform: Platform) -> usize {
    match platform {
        Platform::Weixin => 2000,
        Platform::Wecom => 4000,
    }
}

/// Accumulates streamed message deltas and produces the final text.
#[derive(Debug, Default)]
pub struct MessageAccumulator {
    buffer: String,
    has_error: bool,
}

impl MessageAccumulator {
    pub fn new() -> Self {
        Self::default()
    }

    /// Append a text delta from `ThreadStreamEvent::MessageDelta`.
    pub fn push_text(&mut self, delta: &str) {
        self.buffer.push_str(delta);
    }

    /// Record an error message.
    pub fn push_error(&mut self, error: &str) {
        self.has_error = true;
        if !self.buffer.is_empty() {
            self.buffer.push_str("\n\n");
        }
        self.buffer.push_str(&format!("❌ 错误: {error}"));
    }

    /// Finalize and return the accumulated text.
    pub fn finalize(self) -> String {
        self.buffer
    }

    /// Whether the accumulated content contains an error.
    pub fn has_error(&self) -> bool {
        self.has_error
    }

    /// Whether the accumulator is empty.
    pub fn is_empty(&self) -> bool {
        self.buffer.is_empty()
    }
}

/// Normalize Markdown for IM display (WeChat/WeCom friendly).
///
/// Transforms:
/// - `# H1` → `【H1】`
/// - `## H2+` → `**H2+**`
/// - Markdown tables → key-value list
/// - Long lines (>120 chars) → wrapped
/// - Code blocks preserved as-is
pub fn normalize_markdown_for_im(text: &str) -> String {
    let mut output = String::with_capacity(text.len());
    let mut in_code_block = false;

    for line in text.lines() {
        if line.starts_with("```") {
            in_code_block = !in_code_block;
            output.push_str(line);
            output.push('\n');
            continue;
        }

        if in_code_block {
            output.push_str(line);
            output.push('\n');
            continue;
        }

        // Transform headings.
        if let Some(rest) = line.strip_prefix("# ") {
            output.push_str(&format!("【{}】\n", rest.trim()));
        } else if let Some(rest) = line.strip_prefix("## ") {
            output.push_str(&format!("**{}**\n", rest.trim()));
        } else if let Some(rest) = line.strip_prefix("### ") {
            output.push_str(&format!("**{}**\n", rest.trim()));
        } else if line.starts_with("| ") && line.ends_with(" |") {
            // Table row — check if it's a separator row.
            if line
                .chars()
                .all(|c| c == '|' || c == '-' || c == ':' || c == ' ')
            {
                // Skip separator rows.
            } else {
                // Convert table row to key-value or flat list.
                let cells: Vec<&str> = line
                    .trim_matches('|')
                    .split('|')
                    .map(|c| c.trim())
                    .collect();
                for cell in &cells {
                    if !cell.is_empty() {
                        output.push_str("  ");
                        output.push_str(cell);
                        output.push('\n');
                    }
                }
            }
        } else if line.chars().count() > 120 && !line.starts_with(' ') && !line.starts_with('\t') {
            // Wrap long lines at ~120 chars.
            wrap_line(line, 120, &mut output);
        } else {
            output.push_str(line);
            output.push('\n');
        }
    }

    // Remove trailing newline.
    if output.ends_with('\n') {
        output.pop();
    }
    output
}

/// Wrap a long line at approximately `width` characters.
fn wrap_line(line: &str, width: usize, output: &mut String) {
    let mut current_len = 0;
    for word in line.split_whitespace() {
        let word_len = word.chars().count();
        if current_len > 0 && current_len + 1 + word_len > width {
            output.push('\n');
            current_len = 0;
        }
        if current_len > 0 {
            output.push(' ');
            current_len += 1;
        }
        output.push_str(word);
        current_len += word_len;
    }
    output.push('\n');
}

/// Format agent output for a specific platform and split into chunks.
pub fn format_and_split(text: &str, platform: Platform) -> Vec<String> {
    let normalized = normalize_markdown_for_im(text);
    let max_chars = max_message_length(platform);
    if normalized.is_empty() {
        return vec![];
    }
    if normalized.chars().count() <= max_chars {
        return vec![normalized];
    }

    // Split by paragraph boundaries (double newline), falling back to hard split.
    let mut chunks = Vec::new();
    let mut current = String::new();

    for paragraph in normalized.split("\n\n") {
        let candidate = if current.is_empty() {
            paragraph.to_string()
        } else {
            format!("{current}\n\n{paragraph}")
        };

        if candidate.chars().count() <= max_chars {
            current = candidate;
        } else if current.is_empty() {
            // Single paragraph exceeds max — hard split by char count.
            let mut remaining: &str = paragraph;
            while !remaining.is_empty() {
                let split_at = char_boundary_at_count(remaining, max_chars);
                chunks.push(remaining[..split_at].to_string());
                remaining = &remaining[split_at..];
            }
        } else {
            chunks.push(current);
            current = paragraph.to_string();
        }
    }
    if !current.is_empty() {
        chunks.push(current);
    }

    chunks
}

/// Find the byte offset of the character boundary after `max_chars` characters.
fn char_boundary_at_count(s: &str, max_chars: usize) -> usize {
    s.char_indices()
        .nth(max_chars)
        .map(|(idx, _)| idx)
        .unwrap_or(s.len())
}

/// Render a numbered workspace list for IM display.
pub fn render_workspace_list(
    workspaces: &[crate::model::workspace::WorkspaceRecord],
    current_id: Option<&str>,
) -> String {
    if workspaces.is_empty() {
        return "📁 暂无 workspace\n\n发送 /ws add <路径> 添加".to_string();
    }

    let mut out = String::from("📁 Workspace 列表:\n");
    for (i, ws) in workspaces.iter().enumerate() {
        let marker = if Some(ws.id.as_str()) == current_id {
            " ★"
        } else {
            ""
        };
        let default_marker = if ws.is_default { " [默认]" } else { "" };
        out.push_str(&format!(
            "  {}. {}{}{} ({})\n",
            i + 1,
            ws.name,
            default_marker,
            marker,
            ws.display_path
        ));
    }
    out.push_str("\n回复编号切换，/ws add <路径> 添加新 workspace");
    out
}

/// Render a numbered thread list for IM display.
pub fn render_thread_list(
    threads: &[crate::model::thread::ThreadSummaryDto],
    current_id: Option<&str>,
) -> String {
    if threads.is_empty() {
        return "💬 暂无会话\n\n发送 /new 创建新会话".to_string();
    }

    let mut out = String::from("💬 会话列表:\n");
    for (i, t) in threads.iter().enumerate() {
        let marker = if Some(t.id.as_str()) == current_id {
            " ★"
        } else {
            ""
        };
        let status = match t.status.as_str() {
            "running" => " [运行中]",
            "failed" => " [失败]",
            "interrupted" => " [中断]",
            "waiting_approval" => " [待审批]",
            "needs_reply" => " [需回复]",
            _ => "",
        };
        let title = if t.title.is_empty() {
            "(无标题)"
        } else {
            &t.title
        };
        out.push_str(&format!("  {}. {}{}{}\n", i + 1, title, status, marker));
    }
    out.push_str("\n回复编号进入会话，/new 创建新会话");
    out
}

/// Render a numbered profile list for IM display.
pub fn render_profile_list(
    profiles: &[crate::model::provider::AgentProfileRecord],
    current_profile_id: Option<&str>,
) -> String {
    if profiles.is_empty() {
        return "🔧 暂无 Profile\n\n请在 TiyCode 设置中配置".to_string();
    }

    let mut out = String::from("🔧 Profile 列表:\n");
    for (i, p) in profiles.iter().enumerate() {
        let marker = if Some(p.id.as_str()) == current_profile_id {
            " ★"
        } else {
            ""
        };
        let default_tag = if p.is_default { " [默认]" } else { "" };
        let model_info = match p.primary_model_id.as_deref() {
            Some(id) if !id.is_empty() => format!(" ({})", id),
            _ => String::new(),
        };
        out.push_str(&format!(
            "  {}. {}{}{}{}\n",
            i + 1,
            p.name,
            model_info,
            default_tag,
            marker
        ));
    }
    out.push_str("\n/profile <编号> 切换当前会话的 Profile");
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_short_message() {
        let chunks = format_and_split("hello", Platform::Weixin);
        assert_eq!(chunks, vec!["hello"]);
    }

    #[test]
    fn split_empty_message() {
        let chunks = format_and_split("", Platform::Weixin);
        assert!(chunks.is_empty());
    }

    #[test]
    fn split_by_paragraph() {
        let text = "a".repeat(1500) + "\n\n" + &"b".repeat(1500);
        let chunks = format_and_split(&text, Platform::Weixin);
        assert_eq!(chunks.len(), 2);
        assert!(chunks[0].len() <= 2000);
        assert!(chunks[1].len() <= 2000);
    }
}
