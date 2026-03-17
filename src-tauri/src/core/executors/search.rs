use std::path::Path;
use tokio::process::Command;

use super::ToolOutput;
use crate::model::errors::AppError;

/// Search workspace files using ripgrep.
/// Input: { "query": "search term", "directory": "/optional/path", "filePattern": "*.rs" }
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

    let search_dir = input["directory"]
        .as_str()
        .map(|d| {
            let p = Path::new(d);
            if p.is_absolute() {
                d.to_string()
            } else {
                Path::new(workspace_path)
                    .join(d)
                    .to_string_lossy()
                    .to_string()
            }
        })
        .unwrap_or_else(|| workspace_path.to_string());

    let mut cmd = Command::new("rg");
    cmd.arg("--json")
        .arg("--max-count=50")
        .arg("--max-filesize=1M")
        .arg(query)
        .arg(&search_dir)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());

    if let Some(pattern) = input["filePattern"].as_str() {
        cmd.arg("--glob").arg(pattern);
    }

    match cmd.output().await {
        Ok(output) => {
            let stdout = String::from_utf8_lossy(&output.stdout);
            let results = parse_rg_json(&stdout, workspace_path);

            Ok(ToolOutput {
                success: true,
                result: serde_json::json!({
                    "query": query,
                    "directory": search_dir,
                    "results": results,
                    "count": results.len(),
                }),
            })
        }
        Err(e) => Ok(ToolOutput {
            success: false,
            result: serde_json::json!({
                "error": format!("ripgrep execution failed: {e}"),
                "hint": "Ensure 'rg' (ripgrep) is installed and in PATH",
            }),
        }),
    }
}

/// Parse ripgrep JSON output into structured results.
fn parse_rg_json(output: &str, workspace_path: &str) -> Vec<serde_json::Value> {
    let mut results = Vec::new();

    for line in output.lines() {
        if let Ok(entry) = serde_json::from_str::<serde_json::Value>(line) {
            if entry["type"].as_str() == Some("match") {
                let data = &entry["data"];
                let path = data["path"]["text"].as_str().unwrap_or("");
                let line_number = data["line_number"].as_u64().unwrap_or(0);
                let line_text = data["lines"]["text"].as_str().unwrap_or("").trim();

                // Make path relative to workspace for display
                let display_path = Path::new(path)
                    .strip_prefix(workspace_path)
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
