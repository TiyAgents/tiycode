//! Edit tool — precise text replacement within files.
//!
//! Mirrors pi-mono's `edit.ts` + `edit-diff.ts`:
//! - Fuzzy matching: trailing whitespace normalization, Unicode quote/dash
//!   normalization when exact match fails.
//! - Uniqueness validation: the old_string must match exactly once.
//! - Unified diff generation for the LLM to understand what changed.
//! - BOM (byte-order mark) handling.

use std::path::{Path, PathBuf};

use tokio::fs;

use crate::core::workspace_paths::{
    canonicalize_workspace_root, normalize_additional_roots, resolve_path_within_roots,
};
use crate::model::errors::{AppError, ErrorSource};

use super::ToolOutput;

/// Execute an edit (find-and-replace) on a single file.
///
/// Input:
/// ```json
/// {
///   "path": "/absolute/path/to/file",
///   "old_string": "text to find",
///   "new_string": "replacement text"
/// }
/// ```
///
/// Behaviour:
/// 1. Read the file content (handling BOM).
/// 2. Find `old_string` in the content. If not found, try fuzzy matching.
/// 3. Validate that the match is unique (exactly one occurrence).
/// 4. Replace the matched text with `new_string`.
/// 5. Write the file back.
/// 6. Return a unified diff snippet.
pub async fn edit_file(
    input: &serde_json::Value,
    workspace_path: &str,
    writable_roots: &[String],
) -> Result<ToolOutput, AppError> {
    let path = resolve_required_path(input, workspace_path, writable_roots)?;
    let old_string = input["old_string"].as_str().ok_or_else(|| {
        AppError::recoverable(
            ErrorSource::Tool,
            "tool.input.missing",
            "Missing 'old_string' field",
        )
    })?;
    let new_string = input["new_string"].as_str().ok_or_else(|| {
        AppError::recoverable(
            ErrorSource::Tool,
            "tool.input.missing",
            "Missing 'new_string' field",
        )
    })?;

    // Special case: empty old_string means create a new file
    if old_string.is_empty() {
        return create_new_file(&path, new_string).await;
    }

    // Read file content
    let raw_content = match fs::read_to_string(&path).await {
        Ok(c) => c,
        Err(e) => {
            return Ok(ToolOutput {
                success: false,
                result: serde_json::json!({
                    "error": format!("Failed to read file: {e}"),
                    "path": path.to_string_lossy().to_string(),
                }),
            });
        }
    };

    // Strip BOM if present
    let content = strip_bom(&raw_content);

    // Try to find the old_string
    let find_result = find_text(content, old_string);

    match find_result {
        FindResult::ExactUnique(offset) => {
            apply_edit(&path, content, old_string, new_string, offset).await
        }
        FindResult::FuzzyUnique {
            offset,
            matched_text,
        } => apply_edit(&path, content, &matched_text, new_string, offset).await,
        FindResult::NotFound => Ok(ToolOutput {
            success: false,
            result: serde_json::json!({
                "error": "old_string not found in file",
                "path": path.to_string_lossy().to_string(),
                "hint": "The exact text was not found. Check for whitespace differences, encoding, or Unicode characters.",
            }),
        }),
        FindResult::MultipleMatches(count) => Ok(ToolOutput {
            success: false,
            result: serde_json::json!({
                "error": format!("old_string matches {count} locations in the file — it must be unique. Add more surrounding context to narrow down to a single match."),
                "path": path.to_string_lossy().to_string(),
                "matchCount": count,
            }),
        }),
    }
}

// ---------------------------------------------------------------------------
// Find logic with fuzzy fallback
// ---------------------------------------------------------------------------

enum FindResult {
    ExactUnique(usize),
    FuzzyUnique { offset: usize, matched_text: String },
    NotFound,
    MultipleMatches(usize),
}

/// Find `needle` in `haystack`. First try exact, then fuzzy.
fn find_text(haystack: &str, needle: &str) -> FindResult {
    // 1. Exact match
    let exact_matches: Vec<usize> = find_all_occurrences(haystack, needle);

    match exact_matches.len() {
        1 => return FindResult::ExactUnique(exact_matches[0]),
        n if n > 1 => return FindResult::MultipleMatches(n),
        _ => {} // 0 — try fuzzy
    }

    // 2. Fuzzy match: normalize both and search
    let normalized_haystack = normalize_for_fuzzy(haystack);
    let normalized_needle = normalize_for_fuzzy(needle);

    let fuzzy_matches = find_all_occurrences(&normalized_haystack, &normalized_needle);

    match fuzzy_matches.len() {
        1 => {
            // Map normalized offset back to original text offset
            let norm_offset = fuzzy_matches[0];
            let norm_end = norm_offset + normalized_needle.len();

            // Find the corresponding range in the original text
            if let Some((orig_start, orig_end)) = map_normalized_range_to_original(
                haystack,
                &normalized_haystack,
                norm_offset,
                norm_end,
            ) {
                let matched_text = haystack[orig_start..orig_end].to_string();
                FindResult::FuzzyUnique {
                    offset: orig_start,
                    matched_text,
                }
            } else {
                FindResult::NotFound
            }
        }
        n if n > 1 => FindResult::MultipleMatches(n),
        _ => FindResult::NotFound,
    }
}

/// Find all byte-offset occurrences of `needle` in `haystack`.
fn find_all_occurrences(haystack: &str, needle: &str) -> Vec<usize> {
    let mut results = Vec::new();
    let mut start = 0;
    while let Some(pos) = haystack[start..].find(needle) {
        results.push(start + pos);
        // Advance past the current match start by at least one character,
        // ensuring we land on a valid UTF-8 char boundary.
        start += pos + 1;
        while start < haystack.len() && !haystack.is_char_boundary(start) {
            start += 1;
        }
        if start >= haystack.len() {
            break;
        }
    }
    results
}

/// Normalize text for fuzzy matching:
/// - Strip trailing whitespace from each line
/// - Normalize Unicode fancy quotes to ASCII quotes
/// - Normalize Unicode dashes to ASCII hyphens
fn normalize_for_fuzzy(text: &str) -> String {
    let mut result = String::with_capacity(text.len());
    for (i, line) in text.lines().enumerate() {
        if i > 0 {
            result.push('\n');
        }
        result.push_str(line.trim_end());
    }
    // If original ended with newline, preserve it
    if text.ends_with('\n') {
        result.push('\n');
    }

    // Unicode normalization
    normalize_unicode_chars(&mut result);
    result
}

/// Normalize Unicode quotes and dashes in-place.
fn normalize_unicode_chars(text: &mut String) {
    // Smart quotes → ASCII
    let replacements: &[(&str, &str)] = &[
        ("\u{201C}", "\""), // left double quotation mark
        ("\u{201D}", "\""), // right double quotation mark
        ("\u{2018}", "'"),  // left single quotation mark
        ("\u{2019}", "'"),  // right single quotation mark
        ("\u{2013}", "-"),  // en dash
        ("\u{2014}", "-"),  // em dash
        ("\u{2015}", "-"),  // horizontal bar
        ("\u{2212}", "-"),  // minus sign
    ];

    for (from, to) in replacements {
        if text.contains(from) {
            *text = text.replace(from, to);
        }
    }
}

/// Map a range in the normalized string back to a range in the original string.
/// We normalize line by line (strip trailing whitespace + Unicode normalization)
/// and track cumulative byte offsets to build the mapping.
fn map_normalized_range_to_original(
    original: &str,
    _normalized: &str,
    norm_start: usize,
    norm_end: usize,
) -> Option<(usize, usize)> {
    // Build parallel offset arrays: for each normalized byte position,
    // track the corresponding original byte position.
    let mut norm_to_orig: Vec<usize> = Vec::new();

    let orig_lines: Vec<&str> = original.lines().collect();
    let orig_ends_with_newline = original.ends_with('\n');
    let mut orig_byte = 0usize;

    for (line_idx, &orig_line) in orig_lines.iter().enumerate() {
        let trimmed = orig_line.trim_end();

        // Process each character: some Unicode chars get replaced with shorter ASCII
        let mut line_orig_offset = 0usize;
        for ch in trimmed.chars() {
            let ch_orig_len = ch.len_utf8();
            match get_normalized_replacement_str(ch) {
                Some(replacement) => {
                    // Unicode char replaced with shorter ASCII
                    for _ in 0..replacement.len() {
                        norm_to_orig.push(orig_byte + line_orig_offset);
                    }
                }
                None => {
                    // Regular character: each byte maps 1:1
                    for byte_i in 0..ch_orig_len {
                        norm_to_orig.push(orig_byte + line_orig_offset + byte_i);
                    }
                }
            }
            line_orig_offset += ch_orig_len;
        }
        // Skip trailing whitespace in original (we don't emit it in normalized)
        // but advance orig_byte past the entire original line
        orig_byte += orig_line.len();

        // Account for newline separator
        if line_idx < orig_lines.len() - 1 || orig_ends_with_newline {
            norm_to_orig.push(orig_byte); // the \n byte
            orig_byte += 1;
        }
    }

    // Sentinel for end-of-string mapping
    norm_to_orig.push(orig_byte);

    if norm_start >= norm_to_orig.len() || norm_end > norm_to_orig.len() {
        return None;
    }

    let mapped_start = norm_to_orig[norm_start];
    let mapped_end = if norm_end < norm_to_orig.len() {
        norm_to_orig[norm_end]
    } else {
        *norm_to_orig.last().unwrap_or(&orig_byte)
    };

    Some((mapped_start, mapped_end))
}

/// Get the normalized replacement string for a character, if any.
fn get_normalized_replacement_str(ch: char) -> Option<&'static str> {
    match ch {
        '\u{201C}' | '\u{201D}' => Some("\""),
        '\u{2018}' | '\u{2019}' => Some("'"),
        '\u{2013}' | '\u{2014}' | '\u{2015}' | '\u{2212}' => Some("-"),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Apply edit + diff generation
// ---------------------------------------------------------------------------

/// Apply the replacement and write back.
async fn apply_edit(
    path: &PathBuf,
    content: &str,
    matched_text: &str,
    new_string: &str,
    offset: usize,
) -> Result<ToolOutput, AppError> {
    let end = offset + matched_text.len();
    let new_content = format!("{}{}{}", &content[..offset], new_string, &content[end..]);

    // Generate diff before writing
    let diff = generate_diff(path, content, &new_content);
    let (lines_added, lines_removed) = count_diff_line_changes(&diff);

    // Write the file
    match fs::write(path, &new_content).await {
        Ok(()) => Ok(ToolOutput {
            success: true,
            result: serde_json::json!({
                "path": path.to_string_lossy().to_string(),
                "diff": diff,
                "linesRemoved": lines_removed,
                "linesAdded": lines_added,
            }),
        }),
        Err(e) => Ok(ToolOutput {
            success: false,
            result: serde_json::json!({
                "error": format!("Failed to write file: {e}"),
                "path": path.to_string_lossy().to_string(),
            }),
        }),
    }
}

/// Create a new file with the given content (when old_string is empty).
async fn create_new_file(path: &PathBuf, content: &str) -> Result<ToolOutput, AppError> {
    // Check if file already exists
    if path.exists() {
        return Ok(ToolOutput {
            success: false,
            result: serde_json::json!({
                "error": "File already exists. Use a non-empty old_string to edit, or use write to overwrite.",
                "path": path.to_string_lossy().to_string(),
            }),
        });
    }

    // Ensure parent directory exists
    if let Some(parent) = path.parent() {
        if let Err(e) = fs::create_dir_all(parent).await {
            return Ok(ToolOutput {
                success: false,
                result: serde_json::json!({
                    "error": format!("Failed to create directory: {e}"),
                    "path": path.to_string_lossy().to_string(),
                }),
            });
        }
    }

    match fs::write(path, content).await {
        Ok(()) => {
            let diff = generate_diff_new_file(path, content);
            Ok(ToolOutput {
                success: true,
                result: serde_json::json!({
                    "path": path.to_string_lossy().to_string(),
                    "diff": diff,
                    "created": true,
                    "linesAdded": content.lines().count(),
                }),
            })
        }
        Err(e) => Ok(ToolOutput {
            success: false,
            result: serde_json::json!({
                "error": format!("Failed to create file: {e}"),
                "path": path.to_string_lossy().to_string(),
            }),
        }),
    }
}

// ---------------------------------------------------------------------------
// Diff generation (unified diff format)
// ---------------------------------------------------------------------------

/// Generate a unified diff string between old and new content.
pub(super) fn generate_diff(path: &Path, old_content: &str, new_content: &str) -> String {
    let path_str = path.to_string_lossy();
    let old_lines: Vec<&str> = old_content.lines().collect();
    let new_lines: Vec<&str> = new_content.lines().collect();

    // Find the first and last differing lines
    let common_prefix = old_lines
        .iter()
        .zip(new_lines.iter())
        .take_while(|(a, b)| a == b)
        .count();

    let common_suffix = old_lines
        .iter()
        .rev()
        .zip(new_lines.iter().rev())
        .take_while(|(a, b)| a == b)
        .count()
        .min(old_lines.len() - common_prefix)
        .min(new_lines.len() - common_prefix);

    let old_changed_start = common_prefix;
    let old_changed_end = old_lines.len() - common_suffix;
    let new_changed_start = common_prefix;
    let new_changed_end = new_lines.len() - common_suffix;
    let old_changed_len = old_changed_end - old_changed_start;
    let new_changed_len = new_changed_end - new_changed_start;

    // Context lines around the change (3 lines, matching standard unified diff)
    let context = 3;
    let display_start = old_changed_start.saturating_sub(context);
    let display_old_end = (old_changed_end + context).min(old_lines.len());
    let display_new_end = (new_changed_end + context).min(new_lines.len());
    let context_before_len = old_changed_start - display_start;
    let old_context_after_len = display_old_end - old_changed_end;
    let new_context_after_len = display_new_end - new_changed_end;
    let old_hunk_len = context_before_len + old_changed_len + old_context_after_len;
    let new_hunk_len = context_before_len + new_changed_len + new_context_after_len;

    let mut diff = String::new();
    diff.push_str(&format!("--- {path_str}\n"));
    diff.push_str(&format!("+++ {path_str}\n"));
    diff.push_str(&format!(
        "@@ -{},{} +{},{} @@\n",
        display_start + 1,
        old_hunk_len,
        display_start + 1,
        new_hunk_len,
    ));

    // Context before
    for i in display_start..old_changed_start {
        diff.push_str(&format!(" {}\n", old_lines[i]));
    }

    // Removed lines
    for i in old_changed_start..old_changed_end {
        diff.push_str(&format!("-{}\n", old_lines[i]));
    }

    // Added lines
    for i in new_changed_start..new_changed_end {
        diff.push_str(&format!("+{}\n", new_lines[i]));
    }

    // Context after
    for i in old_changed_end..display_old_end {
        diff.push_str(&format!(" {}\n", old_lines[i]));
    }

    diff
}

/// Generate a diff for a newly created file.
pub(super) fn generate_diff_new_file(path: &Path, content: &str) -> String {
    let path_str = path.to_string_lossy();
    let lines: Vec<&str> = content.lines().collect();
    let mut diff = String::new();
    diff.push_str(&format!("--- /dev/null\n"));
    diff.push_str(&format!("+++ {path_str}\n"));
    diff.push_str(&format!("@@ -0,0 +1,{} @@\n", lines.len()));
    for line in &lines {
        diff.push_str(&format!("+{line}\n"));
    }
    diff
}

pub(super) fn count_diff_line_changes(diff: &str) -> (usize, usize) {
    let mut lines_added = 0usize;
    let mut lines_removed = 0usize;

    for line in diff.lines() {
        if line.starts_with("+++ ") || line.starts_with("--- ") || line.starts_with("@@ ") {
            continue;
        }

        if line.starts_with('+') {
            lines_added += 1;
            continue;
        }

        if line.starts_with('-') {
            lines_removed += 1;
        }
    }

    (lines_added, lines_removed)
}

// ---------------------------------------------------------------------------
// BOM handling
// ---------------------------------------------------------------------------

/// Strip UTF-8 BOM if present at the start of content.
fn strip_bom(content: &str) -> &str {
    content.strip_prefix('\u{FEFF}').unwrap_or(content)
}

// ---------------------------------------------------------------------------
// Path resolution
// ---------------------------------------------------------------------------

fn resolve_required_path(
    input: &serde_json::Value,
    workspace_path: &str,
    writable_roots: &[String],
) -> Result<PathBuf, AppError> {
    let raw = input["path"].as_str().ok_or_else(|| {
        AppError::recoverable(
            ErrorSource::Tool,
            "tool.input.missing",
            "Missing 'path' field",
        )
    })?;

    let workspace_root = canonicalize_workspace_root(
        workspace_path,
        ErrorSource::Tool,
        "tool.workspace.not_directory",
    )?;
    let additional_roots = normalize_additional_roots(writable_roots);

    resolve_path_within_roots(
        &workspace_root,
        &additional_roots,
        raw,
        ErrorSource::Tool,
        "tool.path.outside_workspace",
        format!("Path '{}' is outside workspace boundary", raw),
    )
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_find_exact_unique() {
        let content = "hello world\nfoo bar\nbaz qux";
        match find_text(content, "foo bar") {
            FindResult::ExactUnique(offset) => {
                assert_eq!(&content[offset..offset + 7], "foo bar");
            }
            _ => panic!("Expected ExactUnique"),
        }
    }

    #[test]
    fn test_find_multiple_matches() {
        let content = "abc abc abc";
        match find_text(content, "abc") {
            FindResult::MultipleMatches(3) => {}
            _ => panic!("Expected MultipleMatches(3)"),
        }
    }

    #[test]
    fn test_find_not_found() {
        let content = "hello world";
        match find_text(content, "xyz") {
            FindResult::NotFound => {}
            _ => panic!("Expected NotFound"),
        }
    }

    #[test]
    fn test_fuzzy_trailing_whitespace() {
        let content = "hello   \nworld";
        // Searching for "hello\nworld" should fuzzy-match
        match find_text(content, "hello\nworld") {
            FindResult::FuzzyUnique { matched_text, .. } => {
                assert_eq!(matched_text, "hello   \nworld");
            }
            _ => panic!("Expected FuzzyUnique for trailing whitespace"),
        }
    }

    #[test]
    fn test_normalize_unicode_quotes() {
        let mut text = String::from("He said \u{201C}hello\u{201D}");
        normalize_unicode_chars(&mut text);
        assert_eq!(text, "He said \"hello\"");
    }

    #[test]
    fn test_strip_bom() {
        let with_bom = "\u{FEFF}hello";
        assert_eq!(strip_bom(with_bom), "hello");

        let without_bom = "hello";
        assert_eq!(strip_bom(without_bom), "hello");
    }

    #[test]
    fn test_generate_diff() {
        let path = PathBuf::from("/test/file.rs");
        let old = "line1\nline2\nline3\nline4";
        let new = "line1\nline2_modified\nline3\nline4";
        let diff = generate_diff(&path, old, new);
        assert!(diff.contains("-line2"));
        assert!(diff.contains("+line2_modified"));
    }

    #[test]
    fn test_count_diff_line_changes() {
        let diff = "\
--- /test/file.rs
+++ /test/file.rs
@@ -1,2 +1,2 @@
 line1
-line2
+line2_updated";

        let (lines_added, lines_removed) = count_diff_line_changes(diff);

        assert_eq!(lines_added, 1);
        assert_eq!(lines_removed, 1);
    }

    #[test]
    fn test_diff_counts_track_actual_changes_not_replacement_block_size() {
        let path = PathBuf::from("/test/file.rs");
        let old = "line1\nline2\nline3\nline4";
        let new = "line1\nline2_updated\nline3\nline4";
        let diff = generate_diff(&path, old, new);

        let (lines_added, lines_removed) = count_diff_line_changes(&diff);

        assert_eq!(lines_added, 1);
        assert_eq!(lines_removed, 1);
    }

    #[test]
    fn test_generate_diff_handles_line_deletions_without_underflow() {
        let path = PathBuf::from("/test/file.rs");
        let old = "line1\nline2\nline3";
        let new = "line1";
        let diff = generate_diff(&path, old, new);

        assert!(diff.contains("@@ -1,3 +1,1 @@"));
        assert!(diff.contains(" line1"));
        assert!(diff.contains("-line2"));
        assert!(diff.contains("-line3"));
    }
}
