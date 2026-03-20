use super::truncation::{truncate_line, GREP_MAX_LINE_LENGTH, GREP_MAX_MATCHES};
use super::ToolOutput;
use crate::core::ripgrep::run_rg;
use crate::core::workspace_paths::{canonicalize_workspace_root, resolve_path_within_workspace};
use crate::model::errors::{AppError, ErrorSource};

/// Search workspace files using ripgrep.
/// Input: { "query": "search term", "directory": "optional/path", "filePattern": "*.rs" }
pub async fn search_repo(
    input: &serde_json::Value,
    workspace_path: &str,
) -> Result<ToolOutput, AppError> {
    let query = input["query"].as_str().unwrap_or("");
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

    let search_dir = match input["directory"].as_str() {
        Some(raw) => resolve_path_within_workspace(
            &workspace_root,
            raw,
            ErrorSource::Tool,
            "tool.path.outside_workspace",
            format!("Path '{}' is outside workspace boundary", raw),
        )?,
        None => workspace_root.clone(),
    };

    let mut args = vec![
        "--json".into(),
        format!("--max-count={}", GREP_MAX_MATCHES).into(),
        "--max-filesize=1M".into(),
        query.into(),
        search_dir.as_os_str().to_os_string(),
    ];
    if let Some(pattern) = input["filePattern"].as_str() {
        args.push("--glob".into());
        args.push(pattern.into());
    }

    match run_rg(args).await {
        Ok(output) => {
            let stdout = String::from_utf8_lossy(&output.stdout);
            let results = parse_rg_json(&stdout, &workspace_root);

            let mut result = serde_json::json!({
                "query": query,
                "directory": search_dir.to_string_lossy().to_string(),
                "results": results,
                "count": results.len(),
            });

            if results.len() >= GREP_MAX_MATCHES {
                result["notice"] = serde_json::json!(format!(
                    "[{} matches limit reached. Refine your query for more specific results.]",
                    GREP_MAX_MATCHES
                ));
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
fn parse_rg_json(output: &str, workspace_root: &std::path::Path) -> Vec<serde_json::Value> {
    let mut results = Vec::new();

    for line in output.lines() {
        if let Ok(entry) = serde_json::from_str::<serde_json::Value>(line) {
            if entry["type"].as_str() == Some("match") {
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

                results.push(serde_json::json!({
                    "path": display_path,
                    "absolutePath": path,
                    "lineNumber": line_number,
                    "lineText": line_text,
                }));
            }
        }
    }

    results
}
