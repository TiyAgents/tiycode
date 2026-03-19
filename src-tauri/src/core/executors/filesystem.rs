use tokio::fs;

use crate::core::workspace_paths::{canonicalize_workspace_root, resolve_path_within_workspace};
use crate::model::errors::{AppError, ErrorSource};

use super::ToolOutput;

/// Read a file's content.
/// Input: { "path": "/absolute/path" }
pub async fn read_file(
    input: &serde_json::Value,
    workspace_path: &str,
) -> Result<ToolOutput, AppError> {
    let path = resolve_required_path(input, workspace_path)?;

    match fs::read_to_string(&path).await {
        Ok(content) => {
            let line_count = content.lines().count();
            // Truncate very large outputs
            let (content, truncated) = if content.len() > 512_000 {
                (content[..512_000].to_string(), true)
            } else {
                (content, false)
            };

            Ok(ToolOutput {
                success: true,
                result: serde_json::json!({
                    "path": path.to_string_lossy().to_string(),
                    "content": content,
                    "lineCount": line_count,
                    "truncated": truncated,
                }),
            })
        }
        Err(e) => Ok(ToolOutput {
            success: false,
            result: serde_json::json!({
                "error": format!("Failed to read file: {e}"),
                "path": path.to_string_lossy().to_string(),
            }),
        }),
    }
}

/// Write content to a file (create or overwrite).
/// Input: { "path": "/absolute/path", "content": "..." }
pub async fn write_file(
    input: &serde_json::Value,
    workspace_path: &str,
) -> Result<ToolOutput, AppError> {
    let path = resolve_required_path(input, workspace_path)?;
    let content = input["content"].as_str().ok_or_else(|| {
        AppError::recoverable(
            ErrorSource::Tool,
            "tool.input.missing",
            "Missing 'content' field",
        )
    })?;

    // Ensure parent directory exists
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).await.map_err(|e| {
            AppError::internal(
                ErrorSource::Tool,
                format!("Failed to create directory: {e}"),
            )
        })?;
    }

    match fs::write(&path, content).await {
        Ok(()) => Ok(ToolOutput {
            success: true,
            result: serde_json::json!({
                "path": path.to_string_lossy().to_string(),
                "bytesWritten": content.len(),
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

/// List directory contents.
/// Input: { "path": "/absolute/path" }
pub async fn list_dir(
    input: &serde_json::Value,
    workspace_path: &str,
) -> Result<ToolOutput, AppError> {
    let path = resolve_path_or_workspace_root(input, workspace_path)?;

    match fs::read_dir(&path).await {
        Ok(mut entries) => {
            let mut items = Vec::new();
            while let Ok(Some(entry)) = entries.next_entry().await {
                let name = entry.file_name().to_string_lossy().to_string();
                let file_type = entry.file_type().await.ok();
                let is_dir = file_type.as_ref().map(|t| t.is_dir()).unwrap_or(false);

                items.push(serde_json::json!({
                    "name": name,
                    "isDir": is_dir,
                }));

                if items.len() >= 1000 {
                    break;
                }
            }

            Ok(ToolOutput {
                success: true,
                result: serde_json::json!({
                    "path": path.to_string_lossy().to_string(),
                    "items": items,
                    "count": items.len(),
                }),
            })
        }
        Err(e) => Ok(ToolOutput {
            success: false,
            result: serde_json::json!({
                "error": format!("Failed to list directory: {e}"),
                "path": path.to_string_lossy().to_string(),
            }),
        }),
    }
}

fn resolve_required_path(
    input: &serde_json::Value,
    workspace_path: &str,
) -> Result<std::path::PathBuf, AppError> {
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

    resolve_path_within_workspace(
        &workspace_root,
        raw,
        ErrorSource::Tool,
        "tool.path.outside_workspace",
        format!("Path '{}' is outside workspace boundary", raw),
    )
}

fn resolve_path_or_workspace_root(
    input: &serde_json::Value,
    workspace_path: &str,
) -> Result<std::path::PathBuf, AppError> {
    let workspace_root = canonicalize_workspace_root(
        workspace_path,
        ErrorSource::Tool,
        "tool.workspace.not_directory",
    )?;

    match input["path"].as_str() {
        Some(raw) => resolve_path_within_workspace(
            &workspace_root,
            raw,
            ErrorSource::Tool,
            "tool.path.outside_workspace",
            format!("Path '{}' is outside workspace boundary", raw),
        ),
        None => Ok(workspace_root),
    }
}
