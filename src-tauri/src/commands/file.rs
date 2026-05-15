use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
use std::path::{Path, PathBuf};
use tauri::State;

use crate::core::app_state::AppState;
use crate::model::errors::{AppError, ErrorSource};
use crate::model::file::FileContentDto;
use crate::persistence::repo::workspace_repo;

/// Maximum file size allowed for reading (5 MB).
const MAX_READ_SIZE: u64 = 5 * 1024 * 1024;

/// Image extensions that should be returned as base64-encoded content.
const IMAGE_EXTENSIONS: &[&str] = &["png", "jpg", "jpeg", "gif", "webp", "ico", "bmp"];

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Resolve workspace_id → canonical workspace root path.
async fn resolve_workspace_root(state: &AppState, workspace_id: &str) -> Result<PathBuf, AppError> {
    let workspace = workspace_repo::find_by_id(&state.pool, workspace_id)
        .await?
        .ok_or_else(|| AppError::not_found(ErrorSource::Workspace, "workspace"))?;
    Ok(PathBuf::from(&workspace.canonical_path))
}

/// Join a relative path onto the workspace root and canonicalize, rejecting
/// any traversal that escapes the root.
fn safe_resolve(root: &Path, relative: &str) -> Result<PathBuf, AppError> {
    // Reject obviously malicious inputs early.
    if relative.contains("..") {
        return Err(AppError::validation(
            ErrorSource::System,
            "Path must not contain '..' segments",
        ));
    }

    let joined = root.join(relative);

    // For existing paths we canonicalize; for new paths (create / write) we
    // resolve the parent and append the final component.
    let resolved = if joined.exists() {
        joined.canonicalize().map_err(|e| {
            AppError::internal(ErrorSource::System, format!("Failed to resolve path: {e}"))
        })?
    } else {
        // Parent must exist and resolve within root.
        let parent = joined
            .parent()
            .ok_or_else(|| AppError::validation(ErrorSource::System, "Invalid file path"))?;
        let parent_canonical = parent.canonicalize().map_err(|e| {
            AppError::internal(
                ErrorSource::System,
                format!("Parent directory does not exist: {e}"),
            )
        })?;
        let file_name = joined
            .file_name()
            .ok_or_else(|| AppError::validation(ErrorSource::System, "Missing file name"))?;
        parent_canonical.join(file_name)
    };

    if !resolved.starts_with(root) {
        return Err(AppError::validation(
            ErrorSource::System,
            "Path escapes workspace boundary",
        ));
    }

    Ok(resolved)
}

/// Returns `true` when the first `n` bytes contain a NUL byte (binary heuristic).
fn is_binary_content(buf: &[u8]) -> bool {
    let check_len = buf.len().min(8192);
    buf[..check_len].contains(&0)
}

fn is_image_extension(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| IMAGE_EXTENSIONS.contains(&ext.to_ascii_lowercase().as_str()))
        .unwrap_or(false)
}

// ---------------------------------------------------------------------------
// Commands
// ---------------------------------------------------------------------------

#[tauri::command]
pub async fn file_read(
    state: State<'_, AppState>,
    workspace_id: String,
    path: String,
) -> Result<FileContentDto, AppError> {
    let root = resolve_workspace_root(&state, &workspace_id).await?;
    let resolved = safe_resolve(&root, &path)?;

    let metadata = tokio::fs::metadata(&resolved)
        .await
        .map_err(|e| AppError::not_found(ErrorSource::System, format!("File not found: {e}")))?;

    if metadata.is_dir() {
        return Err(AppError::validation(
            ErrorSource::System,
            "Cannot read a directory as a file",
        ));
    }

    let size = metadata.len();
    if size > MAX_READ_SIZE {
        return Err(AppError::validation(
            ErrorSource::System,
            format!(
                "File too large ({:.1} MB). Maximum is {:.0} MB.",
                size as f64 / 1_048_576.0,
                MAX_READ_SIZE as f64 / 1_048_576.0,
            ),
        ));
    }

    let bytes = tokio::fs::read(&resolved).await.map_err(|e| {
        AppError::internal(ErrorSource::System, format!("Failed to read file: {e}"))
    })?;

    // Image files → base64 data URI
    if is_image_extension(&resolved) {
        let mime = match resolved
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_ascii_lowercase()
            .as_str()
        {
            "png" => "image/png",
            "jpg" | "jpeg" => "image/jpeg",
            "gif" => "image/gif",
            "webp" => "image/webp",
            "ico" => "image/x-icon",
            "bmp" => "image/bmp",
            _ => "application/octet-stream",
        };
        let b64 = BASE64.encode(&bytes);
        return Ok(FileContentDto {
            content: format!("data:{mime};base64,{b64}"),
            size_bytes: size,
            is_binary: true,
        });
    }

    // Binary detection
    if is_binary_content(&bytes) {
        return Ok(FileContentDto {
            content: String::new(),
            size_bytes: size,
            is_binary: true,
        });
    }

    let content = String::from_utf8(bytes)
        .map_err(|_| AppError::internal(ErrorSource::System, "File contains invalid UTF-8"))?;

    Ok(FileContentDto {
        content,
        size_bytes: size,
        is_binary: false,
    })
}

#[tauri::command]
pub async fn file_write(
    state: State<'_, AppState>,
    workspace_id: String,
    path: String,
    content: String,
) -> Result<(), AppError> {
    let root = resolve_workspace_root(&state, &workspace_id).await?;
    let resolved = safe_resolve(&root, &path)?;

    tokio::fs::write(&resolved, content.as_bytes())
        .await
        .map_err(|e| {
            AppError::internal(ErrorSource::System, format!("Failed to write file: {e}"))
        })?;

    Ok(())
}

#[tauri::command]
pub async fn file_create(
    state: State<'_, AppState>,
    workspace_id: String,
    parent_path: String,
    name: String,
    is_dir: bool,
) -> Result<(), AppError> {
    let root = resolve_workspace_root(&state, &workspace_id).await?;
    let parent = safe_resolve(&root, &parent_path)?;

    if !parent.is_dir() {
        return Err(AppError::validation(
            ErrorSource::System,
            "Parent path is not a directory",
        ));
    }

    // Validate name: no path separators or traversal segments
    if name.contains('/') || name.contains('\\') || name.contains("..") {
        return Err(AppError::validation(
            ErrorSource::System,
            "Name must not contain path separators or '..' segments",
        ));
    }

    let target = parent.join(&name);
    if target.exists() {
        return Err(AppError::validation(
            ErrorSource::System,
            format!("'{}' already exists", name),
        ));
    }

    if is_dir {
        tokio::fs::create_dir_all(&target).await.map_err(|e| {
            AppError::internal(
                ErrorSource::System,
                format!("Failed to create directory: {e}"),
            )
        })?;
    } else {
        tokio::fs::write(&target, b"").await.map_err(|e| {
            AppError::internal(ErrorSource::System, format!("Failed to create file: {e}"))
        })?;
    }

    Ok(())
}

#[tauri::command]
pub async fn file_delete(
    state: State<'_, AppState>,
    workspace_id: String,
    path: String,
) -> Result<(), AppError> {
    let root = resolve_workspace_root(&state, &workspace_id).await?;
    let resolved = safe_resolve(&root, &path)?;

    if !resolved.exists() {
        return Err(AppError::not_found(
            ErrorSource::System,
            format!("Path does not exist: {path}"),
        ));
    }

    if resolved.is_dir() {
        tokio::fs::remove_dir_all(&resolved).await.map_err(|e| {
            AppError::internal(
                ErrorSource::System,
                format!("Failed to delete directory: {e}"),
            )
        })?;
    } else {
        tokio::fs::remove_file(&resolved).await.map_err(|e| {
            AppError::internal(ErrorSource::System, format!("Failed to delete file: {e}"))
        })?;
    }

    Ok(())
}

#[tauri::command]
pub async fn file_rename(
    state: State<'_, AppState>,
    workspace_id: String,
    old_path: String,
    new_name: String,
) -> Result<(), AppError> {
    let root = resolve_workspace_root(&state, &workspace_id).await?;
    let resolved_old = safe_resolve(&root, &old_path)?;

    if !resolved_old.exists() {
        return Err(AppError::not_found(
            ErrorSource::System,
            format!("Path does not exist: {old_path}"),
        ));
    }

    // new_name must be a bare filename (no path separators)
    if new_name.contains('/') || new_name.contains('\\') {
        return Err(AppError::validation(
            ErrorSource::System,
            "New name must not contain path separators",
        ));
    }

    let parent = resolved_old.parent().ok_or_else(|| {
        AppError::validation(ErrorSource::System, "Cannot determine parent directory")
    })?;
    let new_path = parent.join(&new_name);

    if new_path.exists() {
        return Err(AppError::validation(
            ErrorSource::System,
            format!("'{}' already exists", new_name),
        ));
    }

    tokio::fs::rename(&resolved_old, &new_path)
        .await
        .map_err(|e| AppError::internal(ErrorSource::System, format!("Failed to rename: {e}")))?;

    Ok(())
}
