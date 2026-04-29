use super::truncation::GREP_MAX_MATCHES;
use super::ToolOutput;
use crate::core::local_search::{
    extensions_for_file_type, is_noop_file_pattern, normalize_file_pattern, run_local_search,
    LocalSearchOutcome, LocalSearchRequest, SearchOutputMode, SearchQueryMode,
};
use crate::core::workspace_paths::{
    canonicalize_workspace_root, normalize_additional_roots, resolve_path_within_roots,
};
use crate::model::errors::{AppError, ErrorSource};
use std::time::Duration;

const MAX_CONTEXT_LINES: usize = 20;
const MAX_SEARCH_TIMEOUT_MS: u64 = 120_000;

/// Search workspace files with an in-process matcher.
/// Input:
/// {
///   "query": "search term",
///   "directory": "optional/path",
///   "filePattern": "*.rs",
///   "queryMode": "literal|regex",
///   "outputMode": "content|files_with_matches|count",
///   "type": "rust|ts|py|...",
///   "caseInsensitive": false,
///   "multiline": false,
///   "context": 2,
///   "beforeContext": 1,
///   "afterContext": 1,
///   "timeoutMs": 20000,
///   "offset": 0,
///   "maxResults": 100
/// }
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
    let offset = input["offset"].as_u64().unwrap_or(0) as usize;
    let normalized_file_pattern = normalize_file_pattern(input["filePattern"].as_str());
    let raw_file_type = input["type"]
        .as_str()
        .map(str::trim)
        .filter(|value| !value.is_empty());

    // Auto-resolve filePattern + type conflict: when both are set and their
    // extension sets have no overlap, drop the type filter and keep filePattern.
    let (file_type, conflict_notice) =
        resolve_filter_conflict(normalized_file_pattern, raw_file_type);
    let output_mode = SearchOutputMode::from_str(input["outputMode"].as_str());
    let query_mode = SearchQueryMode::from_str(input["queryMode"].as_str());
    let case_insensitive = input["caseInsensitive"].as_bool().unwrap_or(false);
    let multiline = input["multiline"].as_bool().unwrap_or(false);
    let shared_context = input["context"]
        .as_u64()
        .map(|value| value.min(MAX_CONTEXT_LINES as u64) as usize)
        .unwrap_or(0);
    let context_before = input["beforeContext"]
        .as_u64()
        .map(|value| value.min(MAX_CONTEXT_LINES as u64) as usize)
        .unwrap_or(shared_context);
    let context_after = input["afterContext"]
        .as_u64()
        .map(|value| value.min(MAX_CONTEXT_LINES as u64) as usize)
        .unwrap_or(shared_context);
    let timeout = input["timeoutMs"]
        .as_u64()
        .map(|value| Duration::from_millis(value.min(MAX_SEARCH_TIMEOUT_MS)));

    let outcome = match run_local_search(LocalSearchRequest {
        workspace_root: workspace_root.clone(),
        search_root: search_dir.clone(),
        query: query.to_string(),
        file_pattern: normalized_file_pattern.map(ToOwned::to_owned),
        file_type: file_type.map(ToOwned::to_owned),
        query_mode,
        output_mode,
        case_insensitive,
        multiline,
        context_before,
        context_after,
        offset,
        max_results,
        timeout,
        cancellation: None,
    })
    .await
    {
        Ok(outcome) => outcome,
        Err(error) => {
            return Ok(ToolOutput {
                success: false,
                result: serde_json::json!({
                    "error": format!("local search failed: {error}"),
                    "query": query,
                    "directory": search_dir.to_string_lossy().to_string(),
                }),
            });
        }
    };

    let LocalSearchOutcome {
        query,
        output_mode,
        results,
        files,
        file_counts,
        total_matches,
        total_files,
        shown_count,
        truncated,
        completed,
        cancelled: _,
        timed_out,
        partial,
        elapsed_ms,
        searched_files,
    } = outcome;

    let mut notices = Vec::new();
    let mut result = serde_json::json!({
        "query": query,
        "directory": search_dir.to_string_lossy().to_string(),
        "queryMode": match query_mode {
            SearchQueryMode::Literal => "literal",
            SearchQueryMode::Regex => "regex",
        },
        "outputMode": output_mode.as_str(),
        "count": total_matches,
        "shownCount": shown_count,
        "truncated": truncated,
        "completed": completed,
        "timedOut": timed_out,
        "partial": partial,
        "elapsedMs": elapsed_ms,
        "searchedFiles": searched_files,
        "totalFiles": total_files,
    });

    match output_mode {
        SearchOutputMode::Content => {
            let results = results
                .into_iter()
                .map(|search_match| {
                    serde_json::json!({
                        "path": search_match.path,
                        "absolutePath": search_match.absolute_path,
                        "lineNumber": search_match.line_number,
                        "endLineNumber": search_match.end_line_number,
                        "lineText": search_match.line_text,
                        "matchText": search_match.match_text,
                        "beforeContext": search_match.before_context.into_iter().map(|line| serde_json::json!({
                            "lineNumber": line.line_number,
                            "lineText": line.line_text,
                        })).collect::<Vec<_>>(),
                        "afterContext": search_match.after_context.into_iter().map(|line| serde_json::json!({
                            "lineNumber": line.line_number,
                            "lineText": line.line_text,
                        })).collect::<Vec<_>>(),
                    })
                })
                .collect::<Vec<_>>();

            result["results"] = serde_json::json!(results);
        }
        SearchOutputMode::FilesWithMatches => {
            result["files"] = serde_json::json!(files
                .into_iter()
                .map(|file| serde_json::json!({
                    "path": file.path,
                    "absolutePath": file.absolute_path,
                }))
                .collect::<Vec<_>>());
            result["count"] = serde_json::json!(total_files);
        }
        SearchOutputMode::Count => {
            result["fileCounts"] = serde_json::json!(file_counts
                .into_iter()
                .map(|file| serde_json::json!({
                    "path": file.path,
                    "absolutePath": file.absolute_path,
                    "count": file.count,
                }))
                .collect::<Vec<_>>());
        }
    }

    if truncated {
        let total_units = match output_mode {
            SearchOutputMode::Content => total_matches,
            SearchOutputMode::FilesWithMatches | SearchOutputMode::Count => total_files,
        };
        notices.push(format!(
            "Showing {} results starting at offset {} out of {}. Refine the query, directory, or filePattern for a narrower result set.",
            shown_count, offset, total_units
        ));
    }

    if timed_out {
        notices.push(format!(
            "Search timed out after {} ms and returned partial results from {} scanned files. Narrow the scope or refine the query to finish in one pass.",
            elapsed_ms, searched_files
        ));
    }

    // Zero-result diagnostic: help the model understand why no files matched.
    if searched_files > 0 && total_matches == 0 && total_files == 0 && !timed_out {
        match (normalized_file_pattern, file_type) {
            (Some(pat), Some(ft)) => {
                notices.push(format!(
                    "No matches found. Both filePattern '{}' and type '{}' filters are active \
                     (AND logic); their intersection may exclude all files. Try using only one.",
                    pat, ft
                ));
            }
            (None, Some(ft)) => {
                if let Some(exts) = extensions_for_file_type(ft) {
                    notices.push(format!(
                        "No matches found with type '{}' (matches extensions {:?}). \
                         Verify that the target files have one of these extensions.",
                        ft, exts
                    ));
                }
            }
            (Some(pat), None) => {
                notices.push(format!(
                    "No matches found. The filePattern '{}' may not match any files \
                     in the search directory, or no matched file contains the query.",
                    pat
                ));
            }
            (None, None) => {}
        }
    }

    if let Some(raw_pattern) = input["filePattern"].as_str() {
        if normalized_file_pattern.is_none() && is_noop_file_pattern(raw_pattern) {
            notices.push(format!(
                "Ignored filePattern '{}'; omit wildcard-only patterns because search already covers the selected directory.",
                raw_pattern.trim()
            ));
        }
    }

    if let Some(notice) = conflict_notice {
        notices.push(notice);
    }

    if let Some(raw_type) = file_type {
        result["type"] = serde_json::json!(raw_type);
    }

    if !notices.is_empty() {
        result["notice"] = serde_json::json!(notices.join(" "));
    }

    Ok(ToolOutput {
        success: true,
        result,
    })
}

/// When `filePattern` and `type` are both set, check whether the pattern's
/// implied extension is compatible with the type's extension list.  If they
/// have no overlap the type filter is silently dropped and a diagnostic notice
/// is returned so the caller can inform the model.
fn resolve_filter_conflict<'a>(
    file_pattern: Option<&str>,
    file_type: Option<&'a str>,
) -> (Option<&'a str>, Option<String>) {
    let (Some(pattern), Some(type_name)) = (file_pattern, file_type) else {
        return (file_type, None);
    };
    let Some(ext) = extract_extension_from_pattern(pattern) else {
        return (file_type, None);
    };
    let Some(type_extensions) = extensions_for_file_type(type_name) else {
        return (file_type, None);
    };
    if type_extensions.iter().any(|e| e.eq_ignore_ascii_case(ext)) {
        return (file_type, None);
    }
    let notice = format!(
        "Dropped type filter '{}' because it conflicts with filePattern '{}' \
         (type '{}' matches {:?} which does not include '.{}').",
        type_name, pattern, type_name, type_extensions, ext
    );
    (None, Some(notice))
}

/// Extract the file extension from a glob pattern such as `*.toml`, `Cargo.lock`,
/// or `src/**/*.ts`.  Returns `None` for wildcard extensions like `Cargo.*`.
fn extract_extension_from_pattern(pattern: &str) -> Option<&str> {
    let basename = pattern.rsplit('/').next().unwrap_or(pattern);
    let dot_pos = basename.rfind('.')?;
    let ext = &basename[dot_pos + 1..];
    if ext.is_empty() || ext.contains('*') || ext.contains('?') {
        return None;
    }
    Some(ext)
}

#[cfg(test)]
mod tests {
    use super::search_repo;

    #[tokio::test]
    async fn content_mode_includes_context_lines() {
        let workspace = tempfile::tempdir().unwrap();
        std::fs::write(
            workspace.path().join("example.rs"),
            "line one\nwarn!(\"hello\")\nline three\n",
        )
        .unwrap();

        let output = search_repo(
            &serde_json::json!({
                "query": "warn!(",
                "context": 1,
            }),
            workspace.path().to_str().unwrap(),
            &[],
        )
        .await
        .unwrap();

        assert!(output.success);
        let first = &output.result["results"][0];
        assert_eq!(first["beforeContext"][0]["lineNumber"].as_u64(), Some(1));
        assert_eq!(first["afterContext"][0]["lineNumber"].as_u64(), Some(3));
    }

    #[tokio::test]
    async fn files_mode_returns_unique_matching_files() {
        let workspace = tempfile::tempdir().unwrap();
        std::fs::write(workspace.path().join("a.ts"), "hello\nhello\n").unwrap();
        std::fs::write(workspace.path().join("b.ts"), "hello\n").unwrap();

        let output = search_repo(
            &serde_json::json!({
                "query": "hello",
                "outputMode": "files_with_matches",
            }),
            workspace.path().to_str().unwrap(),
            &[],
        )
        .await
        .unwrap();

        assert!(output.success);
        assert_eq!(output.result["count"].as_u64(), Some(2));
        assert_eq!(output.result["files"].as_array().unwrap().len(), 2);
    }

    #[tokio::test]
    async fn type_filter_limits_results() {
        let workspace = tempfile::tempdir().unwrap();
        std::fs::write(workspace.path().join("a.ts"), "hello\n").unwrap();
        std::fs::write(workspace.path().join("b.rs"), "hello\n").unwrap();

        let output = search_repo(
            &serde_json::json!({
                "query": "hello",
                "type": "rust",
                "outputMode": "files_with_matches",
            }),
            workspace.path().to_str().unwrap(),
            &[],
        )
        .await
        .unwrap();

        assert!(output.success);
        assert_eq!(output.result["count"].as_u64(), Some(1));
        assert_eq!(output.result["files"][0]["path"].as_str(), Some("b.rs"));
    }

    #[tokio::test]
    async fn context_is_capped_to_keep_output_bounded() {
        let workspace = tempfile::tempdir().unwrap();
        let content = (1..=60)
            .map(|index| {
                if index == 31 {
                    "needle".to_string()
                } else {
                    format!("line {index}")
                }
            })
            .collect::<Vec<_>>()
            .join("\n");
        std::fs::write(workspace.path().join("example.txt"), content).unwrap();

        let output = search_repo(
            &serde_json::json!({
                "query": "needle",
                "context": 999,
            }),
            workspace.path().to_str().unwrap(),
            &[],
        )
        .await
        .unwrap();

        assert!(output.success);
        assert_eq!(
            output.result["results"][0]["beforeContext"]
                .as_array()
                .unwrap()
                .len(),
            20
        );
        assert_eq!(
            output.result["results"][0]["afterContext"]
                .as_array()
                .unwrap()
                .len(),
            20
        );
    }

    #[tokio::test]
    async fn regex_mode_supports_regular_expressions() {
        let workspace = tempfile::tempdir().unwrap();
        std::fs::write(workspace.path().join("a.ts"), "const name = 'hello';\n").unwrap();

        let output = search_repo(
            &serde_json::json!({
                "query": "name\\s*=\\s*'hello'",
                "queryMode": "regex",
            }),
            workspace.path().to_str().unwrap(),
            &[],
        )
        .await
        .unwrap();

        assert!(output.success);
        assert_eq!(output.result["count"].as_u64(), Some(1));
    }

    #[tokio::test]
    async fn multiline_mode_returns_match_metadata() {
        let workspace = tempfile::tempdir().unwrap();
        std::fs::write(
            workspace.path().join("query.sql"),
            "SELECT *\nFROM users\nWHERE active = true;\n",
        )
        .unwrap();

        let output = search_repo(
            &serde_json::json!({
                "query": "SELECT \\*\\nFROM users",
                "queryMode": "regex",
                "multiline": true,
            }),
            workspace.path().to_str().unwrap(),
            &[],
        )
        .await
        .unwrap();

        assert!(output.success);
        assert_eq!(output.result["results"][0]["lineNumber"].as_u64(), Some(1));
        assert_eq!(
            output.result["results"][0]["endLineNumber"].as_u64(),
            Some(2)
        );
        assert_eq!(
            output.result["results"][0]["matchText"].as_str(),
            Some("SELECT *\nFROM users")
        );
    }

    #[test]
    fn extract_extension_from_common_patterns() {
        use super::extract_extension_from_pattern;
        assert_eq!(extract_extension_from_pattern("*.toml"), Some("toml"));
        assert_eq!(extract_extension_from_pattern("Cargo.lock"), Some("lock"));
        assert_eq!(extract_extension_from_pattern("src/**/*.ts"), Some("ts"));
        assert_eq!(extract_extension_from_pattern("Cargo.*"), None);
        assert_eq!(extract_extension_from_pattern("Makefile"), None);
        assert_eq!(extract_extension_from_pattern("*."), None);
    }

    #[test]
    fn resolve_filter_conflict_drops_incompatible_type() {
        use super::resolve_filter_conflict;
        let (ft, notice) = resolve_filter_conflict(Some("*.toml"), Some("rust"));
        assert!(ft.is_none(), "type should be dropped");
        assert!(notice.is_some());
        let msg = notice.unwrap();
        assert!(msg.contains("Dropped type filter"));
        assert!(msg.contains("toml"));
    }

    #[test]
    fn resolve_filter_conflict_keeps_compatible_type() {
        use super::resolve_filter_conflict;
        let (ft, notice) = resolve_filter_conflict(Some("*.rs"), Some("rust"));
        assert_eq!(ft, Some("rust"));
        assert!(notice.is_none());
    }

    #[test]
    fn resolve_filter_conflict_noop_without_both() {
        use super::resolve_filter_conflict;
        let (ft, notice) = resolve_filter_conflict(None, Some("rust"));
        assert_eq!(ft, Some("rust"));
        assert!(notice.is_none());

        let (ft2, notice2) = resolve_filter_conflict(Some("*.toml"), None);
        assert!(ft2.is_none());
        assert!(notice2.is_none());
    }

    #[tokio::test]
    async fn conflict_resolution_drops_type_and_adds_notice() {
        let workspace = tempfile::tempdir().unwrap();
        std::fs::write(workspace.path().join("config.toml"), "key = \"needle\"\n").unwrap();

        let output = search_repo(
            &serde_json::json!({
                "query": "needle",
                "filePattern": "*.toml",
                "type": "rust",
            }),
            workspace.path().to_str().unwrap(),
            &[],
        )
        .await
        .unwrap();

        assert!(output.success);
        // The conflict should be auto-resolved: type dropped, toml file found.
        assert_eq!(output.result["count"].as_u64(), Some(1));
        assert!(
            output.result["type"].is_null(),
            "type should not appear in result"
        );
        let notice = output.result["notice"].as_str().unwrap_or_default();
        assert!(
            notice.contains("Dropped type filter"),
            "notice should explain conflict: {notice}"
        );
    }

    #[tokio::test]
    async fn zero_result_diagnostic_for_type_only() {
        let workspace = tempfile::tempdir().unwrap();
        std::fs::write(workspace.path().join("data.toml"), "key = 1\n").unwrap();

        let output = search_repo(
            &serde_json::json!({
                "query": "key",
                "type": "rust",
            }),
            workspace.path().to_str().unwrap(),
            &[],
        )
        .await
        .unwrap();

        assert!(output.success);
        assert_eq!(output.result["count"].as_u64(), Some(0));
        let notice = output.result["notice"].as_str().unwrap_or_default();
        assert!(
            notice.contains("No matches found with type"),
            "notice should diagnose type filter: {notice}"
        );
    }
}
