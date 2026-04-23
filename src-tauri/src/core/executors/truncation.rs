//! Unified truncation utilities for tool output.
//!
//! Mirrors pi-mono's `truncate.ts` — provides a single, shared module that all
//! executors use instead of each implementing its own truncation logic.

use super::output_sanitizer::sanitize_terminal_output;

/// Default limit for file-read style output (~100 KB).
pub const READ_MAX_BYTES: usize = 100_000;
/// Default max lines for file-read style output.
pub const READ_MAX_LINES: usize = 2_000;

/// Default limit for command/bash output (~50 KB).
pub const COMMAND_MAX_BYTES: usize = 50_000;
/// Default max lines for command/bash output.
pub const COMMAND_MAX_LINES: usize = 2_000;

/// Maximum characters per grep/search match line.
pub const GREP_MAX_LINE_LENGTH: usize = 500;

/// Maximum entries for directory listings.
pub const LIST_DIR_MAX_ENTRIES: usize = 500;

/// Maximum grep/search match results.
pub const GREP_MAX_MATCHES: usize = 100;

/// Maximum find results.
pub const FIND_MAX_RESULTS: usize = 1_000;

// ---------------------------------------------------------------------------
// truncate_head — keep the *first* N bytes / N lines
// ---------------------------------------------------------------------------

/// Truncate content from the head: keep the first `max_bytes` or `max_lines`
/// (whichever limit is hit first). Never cuts in the middle of a line — the
/// output is always a sequence of complete lines.
///
/// Returns `(truncated_content, was_truncated)`.
pub fn truncate_head(content: &str, max_bytes: usize, max_lines: usize) -> (String, bool) {
    let total_bytes = content.len();
    let total_lines = content.lines().count();

    if total_bytes <= max_bytes && total_lines <= max_lines {
        return (content.to_string(), false);
    }

    let mut output = String::new();
    let mut line_count = 0usize;

    for line in content.lines() {
        if line_count >= max_lines {
            break;
        }

        let needed = if output.is_empty() {
            line.len()
        } else {
            1 + line.len() // +1 for the newline separator
        };

        if output.len() + needed > max_bytes {
            break;
        }

        if !output.is_empty() {
            output.push('\n');
        }
        output.push_str(line);
        line_count += 1;
    }

    (output, true)
}

// ---------------------------------------------------------------------------
// truncate_tail — keep the *last* N bytes / N lines
// ---------------------------------------------------------------------------

/// Truncate content from the tail: keep the *last* `max_bytes` or `max_lines`
/// (whichever limit is hit first). For command output the most recent content
/// (errors, final results) is the most useful part.
///
/// Returns `(truncated_content, was_truncated)`.
pub fn truncate_tail(content: &str, max_bytes: usize, max_lines: usize) -> (String, bool) {
    let total_bytes = content.len();
    let total_lines = content.lines().count();

    if total_bytes <= max_bytes && total_lines <= max_lines {
        return (content.to_string(), false);
    }

    let lines: Vec<&str> = content.lines().collect();
    let mut output_lines: Vec<&str> = Vec::new();
    let mut output_bytes = 0usize;

    for &line in lines.iter().rev() {
        if output_lines.len() >= max_lines {
            break;
        }
        let needed = if output_lines.is_empty() {
            line.len()
        } else {
            1 + line.len()
        };
        if output_bytes + needed > max_bytes {
            break;
        }
        output_lines.push(line);
        output_bytes += needed;
    }

    output_lines.reverse();
    let kept_lines = output_lines.len();
    let mut result = output_lines.join("\n");

    // Prepend truncation notice
    let notice = format!(
        "[Output truncated: showing last {} of {} lines, {} of {} bytes]\n",
        kept_lines, total_lines, output_bytes, total_bytes
    );
    result.insert_str(0, &notice);

    (result, true)
}

/// Convenience wrapper that accepts raw bytes (e.g. from process stdout/stderr)
/// and returns a truncated-tail string after sanitizing ANSI/control sequences.
pub fn truncate_tail_bytes(bytes: &[u8], max_bytes: usize, max_lines: usize) -> (String, bool) {
    let s = String::from_utf8_lossy(bytes);
    let sanitized = sanitize_terminal_output(&s);
    truncate_tail(&sanitized, max_bytes, max_lines)
}

// ---------------------------------------------------------------------------
// truncate_line — single line truncation
// ---------------------------------------------------------------------------

/// Truncate a single line to `max_chars` characters, appending `… [truncated]`
/// if the line was cut.
pub fn truncate_line(line: &str, max_chars: usize) -> String {
    if line.len() <= max_chars {
        line.to_string()
    } else {
        let truncated: String = line.chars().take(max_chars).collect();
        format!("{}... [truncated]", truncated)
    }
}

// ---------------------------------------------------------------------------
// format_size — human-readable byte size
// ---------------------------------------------------------------------------

/// Format a byte count as a human-readable string (e.g. "1.5 KB", "3.2 MB").
pub fn format_size(bytes: usize) -> String {
    if bytes < 1024 {
        format!("{} B", bytes)
    } else if bytes < 1024 * 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncate_head_no_truncation() {
        let content = "line1\nline2\nline3";
        let (result, truncated) = truncate_head(content, 1000, 100);
        assert_eq!(result, content);
        assert!(!truncated);
    }

    #[test]
    fn truncate_head_by_lines() {
        let content = "a\nb\nc\nd\ne";
        let (result, truncated) = truncate_head(content, 10000, 3);
        assert_eq!(result, "a\nb\nc");
        assert!(truncated);
    }

    #[test]
    fn truncate_head_by_bytes() {
        let content = "aaaa\nbbbb\ncccc";
        // 4 (aaaa) + 1 (\n) + 4 (bbbb) = 9
        let (result, truncated) = truncate_head(content, 9, 100);
        assert_eq!(result, "aaaa\nbbbb");
        assert!(truncated);
    }

    #[test]
    fn truncate_tail_no_truncation() {
        let content = "line1\nline2\nline3";
        let (result, truncated) = truncate_tail(content, 1000, 100);
        assert_eq!(result, content);
        assert!(!truncated);
    }

    #[test]
    fn truncate_tail_by_lines() {
        let content = "a\nb\nc\nd\ne";
        let (result, truncated) = truncate_tail(content, 10000, 2);
        assert!(truncated);
        assert!(result.contains("d\ne"));
        assert!(result.contains("[Output truncated"));
    }

    #[test]
    fn truncate_tail_bytes_sanitizes_ansi_sequences() {
        let bytes = b"\x1b[31m-red\x1b[0m\n\x1b[32m+green\x1b[0m\r\n";

        let (result, truncated) = truncate_tail_bytes(bytes, 10_000, 100);

        assert_eq!(result, "-red\n+green\n");
        assert!(!truncated);
    }

    #[test]
    fn truncate_tail_bytes_sanitizes_before_truncating() {
        let bytes = concat!(
            "keep\n",
            "\u{1b}[31mremove-me\u{1b}[0m\n",
            "\u{1b}]0;title\u{7}",
            "abc\u{8}Z\n",
            "tail\n"
        )
        .as_bytes();

        let (result, truncated) = truncate_tail_bytes(bytes, 16, 2);

        assert!(truncated);
        assert!(result.contains("showing last 2 of 4 lines"));
        assert!(result.contains("abZ\ntail"));
        assert!(!result.contains("\u{1b}"));
        assert!(!result.contains("remove-me"));
    }

    #[test]
    fn truncate_line_no_truncation() {
        assert_eq!(truncate_line("short", 100), "short");
    }

    #[test]
    fn truncate_line_with_truncation() {
        let result = truncate_line("abcdefghij", 5);
        assert!(result.starts_with("abcde"));
        assert!(result.contains("[truncated]"));
    }

    #[test]
    fn format_size_works() {
        assert_eq!(format_size(512), "512 B");
        assert_eq!(format_size(1024), "1.0 KB");
        assert_eq!(format_size(1024 * 1024 + 512 * 1024), "1.5 MB");
    }
}
