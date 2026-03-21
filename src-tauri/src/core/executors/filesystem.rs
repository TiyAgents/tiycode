use std::path::Path;

use tokio::fs;

use crate::core::workspace_paths::{canonicalize_workspace_root, resolve_path_within_workspace};
use crate::model::errors::{AppError, ErrorSource};

use super::edit::{count_diff_line_changes, generate_diff, generate_diff_new_file};
use super::truncation::{
    self, format_size, truncate_head, LIST_DIR_MAX_ENTRIES, READ_MAX_BYTES, READ_MAX_LINES,
};
use super::ToolOutput;

/// Supported image extensions (matching pi-mono's read tool).
const IMAGE_EXTENSIONS: &[&str] = &["jpg", "jpeg", "png", "gif", "webp"];

/// Maximum image file size to read (10 MB).
const IMAGE_MAX_BYTES: usize = 10 * 1024 * 1024;

/// Read a file's content.
///
/// For text files: truncates from the head (keeps the beginning), capped by
/// whichever limit is hit first: READ_MAX_BYTES or READ_MAX_LINES.
///
/// For image files (jpg/jpeg/png/gif/webp): reads binary content and returns
/// a base64-encoded data URL, matching pi-mono's image support.
///
/// Input: { "path": "/absolute/path" }
pub async fn read_file(
    input: &serde_json::Value,
    workspace_path: &str,
) -> Result<ToolOutput, AppError> {
    let path = resolve_required_path(input, workspace_path)?;

    // Check if this is an image file
    if is_image_file(&path) {
        return read_image_file(&path).await;
    }

    match fs::read_to_string(&path).await {
        Ok(content) => {
            let total_lines = content.lines().count();
            let total_bytes = content.len();

            let (output_content, truncated) =
                truncate_head(&content, READ_MAX_BYTES, READ_MAX_LINES);

            let mut result = serde_json::json!({
                "path": path.to_string_lossy().to_string(),
                "content": output_content,
                "lineCount": total_lines,
                "truncated": truncated,
            });

            if truncated {
                let shown_lines = output_content.lines().count();
                result["totalBytes"] = serde_json::json!(total_bytes);
                result["shownLines"] = serde_json::json!(shown_lines);
                result["notice"] = serde_json::json!(format!(
                    "[Showing first {} lines of {}. Total size: {}. File was truncated.]",
                    shown_lines,
                    total_lines,
                    format_size(total_bytes)
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
                "error": format!("Failed to read file: {e}"),
                "path": path.to_string_lossy().to_string(),
            }),
        }),
    }
}

/// Check if a path points to a supported image file.
fn is_image_file(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| IMAGE_EXTENSIONS.contains(&ext.to_lowercase().as_str()))
        .unwrap_or(false)
}

/// Read an image file and return its content as a base64-encoded data URL.
/// Mirrors pi-mono's read tool image handling.
async fn read_image_file(path: &Path) -> Result<ToolOutput, AppError> {
    let metadata = match fs::metadata(path).await {
        Ok(m) => m,
        Err(e) => {
            return Ok(ToolOutput {
                success: false,
                result: serde_json::json!({
                    "error": format!("Failed to read image: {e}"),
                    "path": path.to_string_lossy().to_string(),
                }),
            });
        }
    };

    if metadata.len() as usize > IMAGE_MAX_BYTES {
        return Ok(ToolOutput {
            success: false,
            result: serde_json::json!({
                "error": format!(
                    "Image file too large: {} (max {})",
                    format_size(metadata.len() as usize),
                    format_size(IMAGE_MAX_BYTES)
                ),
                "path": path.to_string_lossy().to_string(),
            }),
        });
    }

    let bytes = match fs::read(path).await {
        Ok(b) => b,
        Err(e) => {
            return Ok(ToolOutput {
                success: false,
                result: serde_json::json!({
                    "error": format!("Failed to read image: {e}"),
                    "path": path.to_string_lossy().to_string(),
                }),
            });
        }
    };

    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("png")
        .to_lowercase();
    let mime_type = match ext.as_str() {
        "jpg" | "jpeg" => "image/jpeg",
        "png" => "image/png",
        "gif" => "image/gif",
        "webp" => "image/webp",
        _ => "image/png",
    };

    use base64::Engine;
    let encoded = base64::engine::general_purpose::STANDARD.encode(&bytes);
    let data_url = format!("data:{};base64,{}", mime_type, encoded);

    Ok(ToolOutput {
        success: true,
        result: serde_json::json!({
            "path": path.to_string_lossy().to_string(),
            "type": "image",
            "mimeType": mime_type,
            "dataUrl": data_url,
            "sizeBytes": bytes.len(),
        }),
    })
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

    let previous_content = fs::read_to_string(&path).await.ok();

    match fs::write(&path, content).await {
        Ok(()) => Ok(ToolOutput {
            success: true,
            result: {
                let (created, diff) = match previous_content {
                    Some(ref old_content) => (false, generate_diff(path.as_path(), old_content, content)),
                    None => (true, generate_diff_new_file(path.as_path(), content)),
                };
                let (lines_added, lines_removed) = count_diff_line_changes(&diff);

                serde_json::json!({
                    "path": path.to_string_lossy().to_string(),
                    "bytesWritten": content.len(),
                    "created": created,
                    "diff": diff,
                    "linesAdded": lines_added,
                    "linesRemoved": lines_removed,
                })
            },
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
            let mut truncated = false;
            while let Ok(Some(entry)) = entries.next_entry().await {
                let name = entry.file_name().to_string_lossy().to_string();
                let file_type = entry.file_type().await.ok();
                let is_dir = file_type.as_ref().map(|t| t.is_dir()).unwrap_or(false);

                // Append "/" suffix for directories (matching pi-mono's ls tool)
                let display_name = if is_dir {
                    format!("{}/", name)
                } else {
                    name.clone()
                };

                items.push(serde_json::json!({
                    "name": display_name,
                    "isDir": is_dir,
                }));

                if items.len() >= LIST_DIR_MAX_ENTRIES {
                    truncated = true;
                    break;
                }
            }

            // Sort alphabetically (matching pi-mono's ls tool)
            items.sort_by(|a, b| {
                let a_name = a["name"].as_str().unwrap_or("");
                let b_name = b["name"].as_str().unwrap_or("");
                a_name.cmp(b_name)
            });

            let mut result = serde_json::json!({
                "path": path.to_string_lossy().to_string(),
                "items": items,
                "count": items.len(),
                "truncated": truncated,
            });

            if truncated {
                result["notice"] = serde_json::json!(format!(
                    "[Showing first {} entries. Use find for more targeted search.]",
                    LIST_DIR_MAX_ENTRIES
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
                "error": format!("Failed to list directory: {e}"),
                "path": path.to_string_lossy().to_string(),
            }),
        }),
    }
}

/// Find files by glob pattern using the `find` command.
/// Input: { "pattern": "*.rs", "path": "optional/subdir" }
pub async fn find_files(
    input: &serde_json::Value,
    workspace_path: &str,
) -> Result<ToolOutput, AppError> {
    let pattern = input["pattern"].as_str().ok_or_else(|| {
        AppError::recoverable(
            ErrorSource::Tool,
            "tool.input.missing",
            "Missing 'pattern' field",
        )
    })?;

    let workspace_root = canonicalize_workspace_root(
        workspace_path,
        ErrorSource::Tool,
        "tool.workspace.not_directory",
    )?;

    let search_dir = match input["path"].as_str() {
        Some(raw) => resolve_path_within_workspace(
            &workspace_root,
            raw,
            ErrorSource::Tool,
            "tool.path.outside_workspace",
            format!("Path '{}' is outside workspace boundary", raw),
        )?,
        None => workspace_root.clone(),
    };

    // Use `find` on Unix or fall back to walkdir-style approach via shell
    let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_string());

    // Build a find command that respects .gitignore-like patterns
    // We use `find` with `-name` for the glob pattern, excluding common directories
    let find_command = format!(
        "find {} -name '{}' \
         -not -path '*/.git/*' \
         -not -path '*/node_modules/*' \
         -not -path '*/target/*' \
         -not -path '*/__pycache__/*' \
         -not -path '*/.next/*' \
         -not -path '*/dist/*' \
         -not -path '*/.DS_Store' \
         2>/dev/null | head -n 1000",
        search_dir.to_string_lossy(),
        pattern,
    );

    let result = tokio::time::timeout(
        std::time::Duration::from_secs(30),
        tokio::process::Command::new(&shell)
            .arg("-c")
            .arg(&find_command)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .output(),
    )
    .await;

    match result {
        Ok(Ok(output)) => {
            let stdout = String::from_utf8_lossy(&output.stdout);
            let raw_paths: Vec<&str> = stdout
                .lines()
                .filter(|line| !line.trim().is_empty())
                .collect();

            // Make paths relative to workspace root
            let workspace_prefix = workspace_root.to_string_lossy();
            let relative_paths: Vec<String> = raw_paths
                .iter()
                .map(|p| {
                    let trimmed = p.trim();
                    if let Some(rel) = trimmed.strip_prefix(workspace_prefix.as_ref()) {
                        rel.trim_start_matches('/').to_string()
                    } else {
                        trimmed.to_string()
                    }
                })
                .collect();

            let total_count = relative_paths.len();
            let result_text = relative_paths.join("\n");

            // Apply head truncation (keep first N bytes/lines)
            let (output_text, truncated_by_size) =
                truncate_head(&result_text, truncation::READ_MAX_BYTES, usize::MAX);

            let mut result = serde_json::json!({
                "pattern": pattern,
                "directory": search_dir.to_string_lossy().to_string(),
                "results": output_text,
                "count": total_count,
            });

            let mut notices = Vec::new();
            if total_count >= truncation::FIND_MAX_RESULTS {
                notices.push(
                    "1000 results limit reached. Refine your pattern for more specific results."
                        .to_string(),
                );
                result["resultLimitReached"] = serde_json::json!(true);
            }
            if truncated_by_size {
                notices.push(format!(
                    "Output truncated to {}.",
                    format_size(truncation::READ_MAX_BYTES)
                ));
            }
            if !notices.is_empty() {
                result["notice"] = serde_json::json!(notices.join(" "));
            }

            Ok(ToolOutput {
                success: true,
                result,
            })
        }
        Ok(Err(e)) => Ok(ToolOutput {
            success: false,
            result: serde_json::json!({
                "error": format!("Find command failed: {e}"),
                "pattern": pattern,
            }),
        }),
        Err(_) => Ok(ToolOutput {
            success: false,
            result: serde_json::json!({
                "error": "Find command timed out after 30s",
                "pattern": pattern,
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
