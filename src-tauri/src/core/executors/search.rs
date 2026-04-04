use super::truncation::{truncate_line, GREP_MAX_LINE_LENGTH, GREP_MAX_MATCHES};
use super::ToolOutput;
use crate::core::ripgrep::run_rg_in;
use crate::core::workspace_paths::{
    canonicalize_workspace_root, normalize_additional_roots, resolve_path_within_roots,
};
use crate::model::errors::{AppError, ErrorSource};

/// Search workspace files using ripgrep.
/// Input: { "query": "search term", "directory": "optional/path", "filePattern": "*.rs" }
pub async fn search_repo(
    input: &serde_json::Value,
    workspace_path: &str,
    writable_roots: &[String],
) -> Result<ToolOutput, AppError> {
    let query = input["query"].as_str().unwrap_or("").trim();
    if query.is_empty() {
        return Ok(ToolOutput {
            success: false,
            result: serde_json::json!({"error": "Missing 'query' field"}),
        });
    }

    let workspace_root = canonicalize_workspace_root(
        workspace_path,
        ErrorSource::Tool,
        "tool.workspace.not_directory",
    )?;
    let additional_roots = normalize_additional_roots(writable_roots);

    let search_dir = match input["directory"].as_str() {
        Some(raw) => resolve_path_within_roots(
            &workspace_root,
            &additional_roots,
            raw,
            ErrorSource::Tool,
            "tool.path.outside_workspace",
            format!("Path '{}' is outside workspace boundary", raw),
        )?,
        None => workspace_root.clone(),
    };

    let max_results = input["maxResults"]
        .as_u64()
        .map(|value| value.clamp(1, GREP_MAX_MATCHES as u64) as usize)
        .unwrap_or(GREP_MAX_MATCHES);
    let normalized_file_pattern = normalize_file_pattern(input["filePattern"].as_str());

    let mut args = vec![
        "--json".into(),
        format!("--max-count={}", max_results).into(),
        "--max-filesize=1M".into(),
        query.into(),
        search_dir.as_os_str().to_os_string(),
    ];
    if let Some(pattern) = normalized_file_pattern {
        args.push("--glob".into());
        args.push(pattern.into());
    }

    match run_rg_in(args, Some(workspace_root.as_path())).await {
        Ok(output) => {
            if !output.status.success() && output.status.code() != Some(1) {
                let stderr = String::from_utf8_lossy(&output.stderr);
                let message = stderr.trim();

                return Ok(ToolOutput {
                    success: false,
                    result: serde_json::json!({
                        "error": if message.is_empty() {
                            format!("ripgrep search failed with status {}", output.status)
                        } else {
                            format!("ripgrep search failed: {message}")
                        },
                        "query": query,
                        "directory": search_dir.to_string_lossy().to_string(),
                    }),
                });
            }

            let stdout = String::from_utf8_lossy(&output.stdout);
            let parsed = parse_rg_json(&stdout, &workspace_root, max_results);
            let shown_count = parsed.results.len();
            let mut notices = Vec::new();

            let mut result = serde_json::json!({
                "query": query,
                "directory": search_dir.to_string_lossy().to_string(),
                "results": parsed.results,
                "count": parsed.total_count,
                "shownCount": shown_count,
                "truncated": parsed.truncated,
            });

            if parsed.truncated {
                notices.push(format!(
                    "Showing first {} of {} matches. Refine the query, directory, or filePattern for a narrower result set.",
                    shown_count, parsed.total_count
                ));
            }

            if let Some(raw_pattern) = input["filePattern"].as_str() {
                if normalized_file_pattern.is_none() && is_noop_file_pattern(raw_pattern) {
                    notices.push(format!(
                        "Ignored filePattern '{}'; omit wildcard-only patterns because search already covers the selected directory.",
                        raw_pattern.trim()
                    ));
                }
            }

            if !notices.is_empty() {
                result["notice"] = serde_json::json!(notices.join(" "));
            }

            Ok(ToolOutput {
                success: true,
                result,
            })
        }
        Err(e) => Ok(ToolOutput {
            success: false,
            result: serde_json::json!({
                "error": format!("ripgrep execution failed: {e}"),
                "hint": "Ensure 'rg' (ripgrep) is installed, reachable from a login shell, or bundled with the app",
            }),
        }),
    }
}

/// Parse ripgrep JSON output into structured results.
fn parse_rg_json(
    output: &str,
    workspace_root: &std::path::Path,
    max_results: usize,
) -> ParsedSearchResults {
    let mut results = Vec::new();
    let mut total_count = 0usize;

    for line in output.lines() {
        if let Ok(entry) = serde_json::from_str::<serde_json::Value>(line) {
            if entry["type"].as_str() == Some("match") {
                total_count += 1;

                let data = &entry["data"];
                let path = data["path"]["text"].as_str().unwrap_or("");
                let line_number = data["line_number"].as_u64().unwrap_or(0);
                let raw_line_text = data["lines"]["text"].as_str().unwrap_or("").trim();

                // Truncate long match lines (pi-mono style)
                let line_text = truncate_line(raw_line_text, GREP_MAX_LINE_LENGTH);

                // Make path relative to workspace for display
                let display_path = std::path::Path::new(path)
                    .strip_prefix(workspace_root)
                    .map(|p| p.to_string_lossy().to_string())
                    .unwrap_or_else(|_| path.to_string());

                if results.len() < max_results {
                    results.push(serde_json::json!({
                        "path": display_path,
                        "absolutePath": path,
                        "lineNumber": line_number,
                        "lineText": line_text,
                    }));
                }
            }
        }
    }

    ParsedSearchResults {
        truncated: total_count > results.len(),
        total_count,
        results,
    }
}

fn normalize_file_pattern(pattern: Option<&str>) -> Option<&str> {
    let trimmed = pattern?.trim();
    if trimmed.is_empty() || is_noop_file_pattern(trimmed) {
        None
    } else {
        Some(trimmed)
    }
}

fn is_noop_file_pattern(pattern: &str) -> bool {
    matches!(pattern.trim(), "*" | "**" | "**/*" | "./*" | "./**/*")
}

struct ParsedSearchResults {
    truncated: bool,
    total_count: usize,
    results: Vec<serde_json::Value>,
}

#[cfg(test)]
mod tests {
    use super::{is_noop_file_pattern, normalize_file_pattern, parse_rg_json};
    use std::path::Path;

    #[test]
    fn normalize_file_pattern_drops_wildcard_only_values() {
        assert_eq!(normalize_file_pattern(Some("*")), None);
        assert_eq!(normalize_file_pattern(Some(" **/* ")), None);
        assert_eq!(normalize_file_pattern(Some("")), None);
        assert_eq!(normalize_file_pattern(Some("*.rs")), Some("*.rs"));
    }

    #[test]
    fn wildcard_only_pattern_detection_is_narrow() {
        assert!(is_noop_file_pattern("*"));
        assert!(is_noop_file_pattern("./**/*"));
        assert!(!is_noop_file_pattern("*.ts"));
        assert!(!is_noop_file_pattern("src/**/*.rs"));
    }

    #[test]
    fn parse_rg_json_caps_preview_but_preserves_total_count() {
        let output = r#"{"type":"match","data":{"path":{"text":"/workspace/src/a.rs"},"line_number":3,"lines":{"text":"let tauri = true;\n"}}}
{"type":"match","data":{"path":{"text":"/workspace/src/b.rs"},"line_number":8,"lines":{"text":"tauri::Builder::default();\n"}}}"#;

        let parsed = parse_rg_json(output, Path::new("/workspace"), 1);

        assert_eq!(parsed.total_count, 2);
        assert_eq!(parsed.results.len(), 1);
        assert!(parsed.truncated);
        assert_eq!(parsed.results[0]["path"].as_str(), Some("src/a.rs"));
    }
}
