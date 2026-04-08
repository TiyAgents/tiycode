use std::path::Path;

use tokio::fs;

use crate::core::windows_process::configure_background_tokio_command;
use crate::core::workspace_paths::{
    canonicalize_workspace_root, normalize_additional_roots, resolve_path_within_roots,
};
use crate::model::errors::{AppError, ErrorSource};

use super::edit::{count_diff_line_changes, generate_diff, generate_diff_new_file};
use super::truncation::{
    self, format_size, truncate_head, FIND_MAX_RESULTS, LIST_DIR_MAX_ENTRIES, READ_MAX_BYTES,
    READ_MAX_LINES,
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
/// Input: {
///   "path": "/absolute/path",
///   "offset": 1,   // optional, 1-indexed line offset
///   "limit": 100   // optional, max lines to read from offset
/// }
pub async fn read_file(
    input: &serde_json::Value,
    workspace_path: &str,
    writable_roots: &[String],
) -> Result<ToolOutput, AppError> {
    let path = resolve_required_path(input, workspace_path, writable_roots)?;
    let offset = read_positive_integer(input, "offset").unwrap_or(1);
    let limit = read_positive_integer(input, "limit");

    // Check if this is an image file
    if is_image_file(&path) {
        return read_image_file(&path).await;
    }

    match fs::read_to_string(&path).await {
        Ok(content) => {
            let all_lines: Vec<&str> = content.split('\n').collect();
            let total_lines = all_lines.len();
            let total_bytes = content.len();
            let start_line_index = offset.saturating_sub(1);

            if start_line_index >= total_lines && total_lines > 0 {
                return Ok(ToolOutput {
                    success: false,
                    result: serde_json::json!({
                        "error": format!(
                            "Offset {} is beyond end of file ({} lines total)",
                            offset, total_lines
                        ),
                        "path": path.to_string_lossy().to_string(),
                    }),
                });
            }

            let end_line_index = limit
                .map(|line_limit| start_line_index.saturating_add(line_limit).min(total_lines))
                .unwrap_or(total_lines);
            let selected_content = if start_line_index >= total_lines {
                String::new()
            } else {
                all_lines[start_line_index..end_line_index].join("\n")
            };

            let (output_content, truncated) =
                truncate_head(&selected_content, READ_MAX_BYTES, READ_MAX_LINES);
            let shown_lines = output_content.lines().count();
            let start_line_display = if total_lines == 0 {
                0
            } else {
                start_line_index + 1
            };
            let end_line_display = if shown_lines == 0 {
                start_line_display.saturating_sub(1)
            } else {
                start_line_index + shown_lines
            };

            let mut result = serde_json::json!({
                "path": path.to_string_lossy().to_string(),
                "content": output_content,
                "lineCount": total_lines,
                "truncated": truncated,
                "offset": start_line_display,
            });

            if let Some(line_limit) = limit {
                result["limit"] = serde_json::json!(line_limit);
            }

            if truncated {
                result["totalBytes"] = serde_json::json!(total_bytes);
                result["shownLines"] = serde_json::json!(shown_lines);
                let next_offset = end_line_display.saturating_add(1);
                let notice = if shown_lines == 0 && !selected_content.is_empty() {
                    format!(
                        "[The selected range starts with a line that exceeds {}. Narrow the read window or use shell for a byte-level slice.]",
                        format_size(READ_MAX_BYTES)
                    )
                } else {
                    format!(
                        "[Showing lines {}-{} of {}. Total size: {}. Use offset={} to continue.]",
                        start_line_display,
                        end_line_display,
                        total_lines,
                        format_size(total_bytes),
                        next_offset
                    )
                };
                result["notice"] = serde_json::json!(notice);
            } else if let Some(line_limit) = limit {
                if end_line_index < total_lines {
                    let remaining = total_lines - end_line_index;
                    result["shownLines"] = serde_json::json!(shown_lines);
                    result["notice"] = serde_json::json!(format!(
                        "[{} more lines in file. Use offset={} to continue.]",
                        remaining,
                        end_line_index + 1
                    ));
                } else {
                    result["shownLines"] = serde_json::json!(shown_lines);
                    result["limit"] = serde_json::json!(line_limit);
                }
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
    writable_roots: &[String],
) -> Result<ToolOutput, AppError> {
    let path = resolve_required_path(input, workspace_path, writable_roots)?;
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
                    Some(ref old_content) => {
                        (false, generate_diff(path.as_path(), old_content, content))
                    }
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
/// Input: { "path": "/absolute/path", "limit": 100 }
pub async fn list_dir(
    input: &serde_json::Value,
    workspace_path: &str,
    writable_roots: &[String],
) -> Result<ToolOutput, AppError> {
    let path = resolve_path_or_workspace_root(input, workspace_path, writable_roots)?;
    let effective_limit = read_positive_integer(input, "limit")
        .unwrap_or(LIST_DIR_MAX_ENTRIES)
        .min(LIST_DIR_MAX_ENTRIES);

    match fs::read_dir(&path).await {
        Ok(mut entries) => {
            let mut items = Vec::new();
            let mut truncated = false;
            while let Ok(Some(entry)) = entries.next_entry().await {
                if items.len() >= effective_limit {
                    truncated = true;
                    break;
                }

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
                "limit": effective_limit,
            });

            if truncated {
                let notice = if effective_limit < LIST_DIR_MAX_ENTRIES {
                    format!(
                        "[Showing first {} entries. Use limit={} for more, or use find for more targeted search.]",
                        effective_limit,
                        (effective_limit * 2).min(LIST_DIR_MAX_ENTRIES)
                    )
                } else {
                    format!(
                        "[Showing first {} entries. Use find for more targeted search.]",
                        LIST_DIR_MAX_ENTRIES
                    )
                };
                result["notice"] = serde_json::json!(notice);
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
/// Input: { "pattern": "*.rs", "path": "optional/subdir", "limit": 100 }
pub async fn find_files(
    input: &serde_json::Value,
    workspace_path: &str,
    writable_roots: &[String],
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
    let additional_roots = normalize_additional_roots(writable_roots);
    let effective_limit = read_positive_integer(input, "limit")
        .unwrap_or(FIND_MAX_RESULTS)
        .min(FIND_MAX_RESULTS);

    let search_dir = match input["path"].as_str() {
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

    // Build a platform-appropriate find command
    let result = {
        #[cfg(target_os = "windows")]
        {
            // On Windows, use cmd.exe /C with `where` style or `dir /S /B` for file search.
            // We use PowerShell for reliable glob + exclusion support.
            let search_dir_str = search_dir.to_string_lossy().replace('\'', "''");
            let ps_command = format!(
                "Get-ChildItem -Path '{}' -Recurse -Filter '{}' -ErrorAction SilentlyContinue \
                 | Where-Object {{ \
                     $_.FullName -notmatch '[\\\\/]\\.git[\\\\/]' -and \
                     $_.FullName -notmatch '[\\\\/]node_modules[\\\\/]' -and \
                     $_.FullName -notmatch '[\\\\/]target[\\\\/]' -and \
                     $_.FullName -notmatch '[\\\\/]__pycache__[\\\\/]' -and \
                     $_.FullName -notmatch '[\\\\/]\\.next[\\\\/]' -and \
                     $_.FullName -notmatch '[\\\\/]dist[\\\\/]' \
                 }} \
                 | Select-Object -First {} -ExpandProperty FullName",
                search_dir_str,
                pattern.replace('\'', "''"),
                effective_limit.saturating_add(1),
            );

            let mut command = tokio::process::Command::new("powershell.exe");
            configure_background_tokio_command(&mut command);
            let future = command
                .args(["-NoProfile", "-NonInteractive", "-Command", &ps_command])
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped())
                .output();

            tokio::time::timeout(std::time::Duration::from_secs(30), future).await
        }
        #[cfg(not(target_os = "windows"))]
        {
            let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_string());
            let quoted_dir = shell_quote(&search_dir.to_string_lossy());
            let quoted_pattern = shell_quote(pattern);

            // Build a find command that respects .gitignore-like patterns
            let find_command = format!(
                "find {} -name {} \
                 -not -path '*/.git/*' \
                 -not -path '*/node_modules/*' \
                 -not -path '*/target/*' \
                 -not -path '*/__pycache__/*' \
                 -not -path '*/.next/*' \
                 -not -path '*/dist/*' \
                 -not -path '*/.DS_Store' \
                 2>/dev/null | head -n {}",
                quoted_dir,
                quoted_pattern,
                effective_limit.saturating_add(1),
            );

            tokio::time::timeout(
                std::time::Duration::from_secs(30),
                tokio::process::Command::new(&shell)
                    .arg("-c")
                    .arg(&find_command)
                    .stdout(std::process::Stdio::piped())
                    .stderr(std::process::Stdio::piped())
                    .output(),
            )
            .await
        }
    };

    match result {
        Ok(Ok(output)) => {
            let stdout = String::from_utf8_lossy(&output.stdout);
            let raw_paths: Vec<&str> = stdout
                .lines()
                .filter(|line| !line.trim().is_empty())
                .collect();

            // Make paths relative to workspace root
            let workspace_prefix = workspace_root.to_string_lossy();
            let mut relative_paths: Vec<String> = raw_paths
                .iter()
                .map(|p| {
                    let trimmed = p.trim();
                    if let Some(rel) = trimmed.strip_prefix(workspace_prefix.as_ref()) {
                        rel.trim_start_matches(['/', '\\']).to_string()
                    } else {
                        trimmed.to_string()
                    }
                })
                .collect();

            let result_limit_reached = relative_paths.len() > effective_limit;
            if result_limit_reached {
                relative_paths.truncate(effective_limit);
            }

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
                "limit": effective_limit,
            });

            let mut notices = Vec::new();
            if result_limit_reached {
                let notice = if effective_limit < FIND_MAX_RESULTS {
                    format!(
                        "{} results limit reached. Use limit={} for more, or refine your pattern.",
                        effective_limit,
                        (effective_limit * 2).min(FIND_MAX_RESULTS)
                    )
                } else {
                    format!(
                        "{} results limit reached. Refine your pattern for more specific results.",
                        FIND_MAX_RESULTS
                    )
                };
                notices.push(notice);
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
    writable_roots: &[String],
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

fn resolve_path_or_workspace_root(
    input: &serde_json::Value,
    workspace_path: &str,
    writable_roots: &[String],
) -> Result<std::path::PathBuf, AppError> {
    let workspace_root = canonicalize_workspace_root(
        workspace_path,
        ErrorSource::Tool,
        "tool.workspace.not_directory",
    )?;
    let additional_roots = normalize_additional_roots(writable_roots);

    match input["path"].as_str() {
        Some(raw) => resolve_path_within_roots(
            &workspace_root,
            &additional_roots,
            raw,
            ErrorSource::Tool,
            "tool.path.outside_workspace",
            format!("Path '{}' is outside workspace boundary", raw),
        ),
        None => Ok(workspace_root),
    }
}

fn read_positive_integer(input: &serde_json::Value, key: &str) -> Option<usize> {
    input[key].as_i64().map(|value| value.max(1) as usize)
}

#[cfg(not(target_os = "windows"))]
fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

#[cfg(test)]
mod tests {
    use super::{find_files, list_dir, read_file, write_file};
    use tempfile::tempdir;

    #[tokio::test]
    async fn read_file_supports_offset_and_limit_windows() {
        let temp_dir = tempdir().expect("temp dir");
        let workspace = temp_dir.path();
        let file_path = workspace.join("notes.txt");
        let content = (1..=6)
            .map(|index| format!("Line {index}"))
            .collect::<Vec<_>>()
            .join("\n");
        std::fs::write(&file_path, content).expect("write file");

        let output = read_file(
            &serde_json::json!({
                "path": "notes.txt",
                "offset": 3,
                "limit": 2,
            }),
            workspace.to_string_lossy().as_ref(),
            &[],
        )
        .await
        .expect("read file");

        assert!(output.success);
        assert_eq!(output.result["content"].as_str(), Some("Line 3\nLine 4"));
        assert_eq!(output.result["offset"].as_u64(), Some(3));
        assert_eq!(output.result["limit"].as_u64(), Some(2));
        let notice = output.result["notice"].as_str().unwrap_or_default();
        assert!(notice.contains("2 more lines"));
        assert!(notice.contains("offset=5"));
    }

    #[tokio::test]
    async fn list_dir_respects_limit_parameter() {
        let temp_dir = tempdir().expect("temp dir");
        let workspace = temp_dir.path();
        std::fs::write(workspace.join("a.txt"), "a").expect("write a");
        std::fs::write(workspace.join("b.txt"), "b").expect("write b");
        std::fs::write(workspace.join("c.txt"), "c").expect("write c");

        let output = list_dir(
            &serde_json::json!({
                "limit": 2,
            }),
            workspace.to_string_lossy().as_ref(),
            &[],
        )
        .await
        .expect("list dir");

        assert!(output.success);
        assert_eq!(output.result["count"].as_u64(), Some(2));
        assert_eq!(output.result["limit"].as_u64(), Some(2));
        assert_eq!(output.result["truncated"].as_bool(), Some(true));
        let notice = output.result["notice"].as_str().unwrap_or_default();
        assert!(notice.contains("limit=4") || notice.contains("limit=500"));
    }

    #[tokio::test]
    async fn find_files_respects_limit_parameter() {
        let temp_dir = tempdir().expect("temp dir");
        let workspace = temp_dir.path();
        std::fs::write(workspace.join("a.rs"), "fn a() {}\n").expect("write a");
        std::fs::write(workspace.join("b.rs"), "fn b() {}\n").expect("write b");
        std::fs::write(workspace.join("c.rs"), "fn c() {}\n").expect("write c");

        let output = find_files(
            &serde_json::json!({
                "pattern": "*.rs",
                "limit": 2,
            }),
            workspace.to_string_lossy().as_ref(),
            &[],
        )
        .await
        .expect("find files");

        assert!(output.success);
        assert_eq!(output.result["count"].as_u64(), Some(2));
        assert_eq!(output.result["limit"].as_u64(), Some(2));
        assert_eq!(output.result["resultLimitReached"].as_bool(), Some(true));
        let results = output.result["results"].as_str().unwrap_or_default();
        assert_eq!(results.lines().count(), 2);
    }

    #[tokio::test]
    async fn write_file_allows_paths_in_configured_writable_root() {
        let workspace = tempdir().expect("workspace");
        let writable_root = tempdir().expect("writable root");
        let target_path = writable_root.path().join("notes.txt");

        let output = write_file(
            &serde_json::json!({
                "path": target_path.to_string_lossy().to_string(),
                "content": "hello writable root",
            }),
            workspace.path().to_string_lossy().as_ref(),
            &[writable_root.path().to_string_lossy().to_string()],
        )
        .await
        .expect("write file");

        assert!(output.success);
        let written = tokio::fs::read_to_string(&target_path)
            .await
            .expect("written content");
        assert_eq!(written, "hello writable root");
    }
}
