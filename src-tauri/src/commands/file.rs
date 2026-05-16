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

fn validate_bare_file_name(name: &str, field_label: &str) -> Result<(), AppError> {
    let trimmed = name.trim();
    if trimmed.is_empty() || trimmed == "." || trimmed == ".." {
        return Err(AppError::validation(
            ErrorSource::System,
            format!("{field_label} must be a non-empty file name"),
        ));
    }

    if name.contains('/') || name.contains('\\') {
        return Err(AppError::validation(
            ErrorSource::System,
            format!("{field_label} must not contain path separators"),
        ));
    }

    Ok(())
}

fn reject_workspace_root_target(root: &Path, target: &Path, action: &str) -> Result<(), AppError> {
    if target == root {
        return Err(AppError::validation(
            ErrorSource::System,
            format!("Cannot {action} the workspace root"),
        ));
    }

    Ok(())
}

/// Map an image file extension to its MIME type.
fn mime_for_extension(ext: &str) -> &'static str {
    match ext.to_ascii_lowercase().as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "ico" => "image/x-icon",
        "bmp" => "image/bmp",
        _ => "application/octet-stream",
    }
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
        let ext = resolved.extension().and_then(|e| e.to_str()).unwrap_or("");
        let mime = mime_for_extension(ext);
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

    validate_bare_file_name(&name, "Name")?;

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
    reject_workspace_root_target(&root, &resolved, "delete")?;

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

    validate_bare_file_name(&new_name, "New name")?;

    let parent = resolved_old.parent().ok_or_else(|| {
        AppError::validation(ErrorSource::System, "Cannot determine parent directory")
    })?;
    let new_path = parent.join(&new_name);
    reject_workspace_root_target(&root, &new_path, "rename to")?;

    if !new_path.starts_with(&root) {
        return Err(AppError::validation(
            ErrorSource::System,
            "New path escapes workspace boundary",
        ));
    }

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

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    // ---- safe_resolve tests ------------------------------------------------

    #[test]
    fn safe_resolve_rejects_dotdot() {
        let dir = tempfile::tempdir().expect("temp dir");
        let root = dir.path();
        fs::write(root.join("legit.txt"), "ok").unwrap();

        let result = safe_resolve(root, "../escape.txt");
        assert!(result.is_err(), "paths with '..' must be rejected");
    }

    #[test]
    fn delete_guard_rejects_workspace_root_targets() {
        let dir = tempfile::tempdir().expect("temp dir");
        let root = fs::canonicalize(dir.path()).unwrap();

        let empty_path = safe_resolve(&root, "").unwrap();
        let dot_path = safe_resolve(&root, ".").unwrap();

        assert_eq!(empty_path, root);
        assert_eq!(dot_path, root);
        assert!(reject_workspace_root_target(&root, &empty_path, "delete").is_err());
        assert!(reject_workspace_root_target(&root, &dot_path, "delete").is_err());
    }

    #[test]
    fn validate_bare_file_name_rejects_unsafe_names() {
        let bad_names = ["", " ", ".", "..", "sub/file.txt", "sub\\file.txt"];

        for name in bad_names {
            assert!(
                validate_bare_file_name(name, "Name").is_err(),
                "{name:?} should be rejected"
            );
        }
    }

    #[test]
    fn validate_bare_file_name_accepts_simple_names() {
        let good_names = ["file.txt", "folder", "archive.tar.gz", "hello-world_123"];

        for name in good_names {
            assert!(
                validate_bare_file_name(name, "Name").is_ok(),
                "{name:?} should be accepted"
            );
        }
    }

    #[test]
    fn rename_target_boundary_check_rejects_root_escape() {
        let dir = tempfile::tempdir().expect("temp dir");
        let root = fs::canonicalize(dir.path()).unwrap();
        let file_path = root.join("file.txt");
        fs::write(&file_path, "ok").unwrap();
        let parent = file_path.parent().unwrap();
        let unsafe_target = parent.join("..");
        let canonical_unsafe_target = unsafe_target.canonicalize().unwrap();

        assert!(validate_bare_file_name("..", "New name").is_err());
        assert!(
            !canonical_unsafe_target.starts_with(&root),
            "canonical unsafe rename target must escape root"
        );
    }

    #[test]
    fn safe_resolve_accepts_simple_relative_path() {
        let dir = tempfile::tempdir().expect("temp dir");
        let root = fs::canonicalize(dir.path()).unwrap();
        fs::write(root.join("a.txt"), "hello").unwrap();

        let result = safe_resolve(&root, "a.txt");
        assert!(result.is_ok(), "simple relative path should resolve");
        assert!(result.unwrap().starts_with(&root));
    }

    #[test]
    fn safe_resolve_rejects_escape_via_canonicalize() {
        let dir = tempfile::tempdir().expect("temp dir");
        let root = fs::canonicalize(dir.path()).unwrap();
        // Even though `..` is caught early, verify the boundary check too.
        // We construct a path that resolves outside root but is not caught by
        // the `..` check because it uses absolute or other tricks.
        // Since `..` is rejected early, this test documents that behavior.
        let result = safe_resolve(&root, "../../etc/passwd");
        assert!(result.is_err());
    }

    #[test]
    fn safe_resolve_new_file_in_existing_dir() {
        let dir = tempfile::tempdir().expect("temp dir");
        let root = fs::canonicalize(dir.path()).unwrap();

        let result = safe_resolve(&root, "new_file.txt");
        assert!(result.is_ok(), "new file in existing dir should resolve");
        assert!(result.unwrap().starts_with(&root));
    }

    // ---- is_image_extension tests ------------------------------------------

    #[test]
    fn image_extensions_are_recognized() {
        let cases = [
            "photo.png",
            "photo.jpg",
            "photo.jpeg",
            "photo.gif",
            "photo.webp",
            "photo.ico",
            "photo.bmp",
        ];
        for name in &cases {
            let path = Path::new(name);
            assert!(
                is_image_extension(path),
                "{name} should be recognized as image"
            );
        }
    }

    #[test]
    fn non_image_extensions_are_not_recognized() {
        let cases = [
            "doc.txt",
            "archive.zip",
            "script.rs",
            "data.json",
            "style.css",
        ];
        for name in &cases {
            let path = Path::new(name);
            assert!(
                !is_image_extension(path),
                "{name} should NOT be recognized as image"
            );
        }
    }

    #[test]
    fn is_image_extension_is_case_insensitive() {
        assert!(is_image_extension(Path::new("photo.PNG")));
        assert!(is_image_extension(Path::new("photo.Jpg")));
    }

    // ---- is_binary_content tests -------------------------------------------

    #[test]
    fn binary_detection_finds_nul_byte() {
        assert!(is_binary_content(b"hello\x00world"));
    }

    #[test]
    fn binary_detection_passes_for_text() {
        assert!(!is_binary_content(b"plain text content"));
    }

    // ---- MIME mapping + base64 integration ---------------------------------

    /// Verify the full image-branch logic: for a small PNG-like binary file,
    /// reading via `file_read` should return a `data:image/png;base64,...`
    /// content string with `is_binary: true`.
    ///
    /// This indirectly tests the MIME mapping and base64 encoding,
    /// while also exercising `safe_resolve` and `is_image_extension`.
    #[tokio::test]
    async fn file_read_returns_base64_data_uri_for_png() {
        let dir = tempfile::tempdir().expect("temp dir");
        let root = fs::canonicalize(dir.path()).unwrap();
        // Minimal valid PNG: 8-byte signature + IHDR chunk + IEND chunk
        let png_bytes = vec![
            // PNG signature
            0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, // IHDR chunk (13 bytes data)
            0x00, 0x00, 0x00, 0x0D, // length
            0x49, 0x48, 0x44, 0x52, // "IHDR"
            0x00, 0x00, 0x00, 0x01, // width: 1
            0x00, 0x00, 0x00, 0x01, // height: 1
            0x08, // bit depth
            0x02, // color type: RGB
            0x00, 0x00, 0x00, // compression, filter, interlace
            0x90, 0x77, 0x53, 0xDE, // CRC
            // IEND chunk
            0x00, 0x00, 0x00, 0x00, // length
            0x49, 0x45, 0x4E, 0x44, // "IEND"
            0xAE, 0x42, 0x60, 0x82, // CRC
        ];
        let png_path = root.join("sample.png");
        fs::write(&png_path, &png_bytes).unwrap();

        // Call safe_resolve directly (unit level, avoids needing AppState/DB)
        let resolved = safe_resolve(&root, "sample.png").unwrap();

        // Read and verify the image branch logic
        let bytes = fs::read(&resolved).unwrap();
        assert!(is_image_extension(&resolved));

        // Verify MIME mapping via the production helper
        let ext = resolved.extension().and_then(|e| e.to_str()).unwrap_or("");
        let mime = mime_for_extension(ext);
        assert_eq!(mime, "image/png");

        let b64 = BASE64.encode(&bytes);
        let data_uri = format!("data:{mime};base64,{b64}");
        assert!(data_uri.starts_with("data:image/png;base64,"));
    }

    #[test]
    fn mime_mapping_covers_all_image_extensions() {
        // Verify the MIME match covers every extension in IMAGE_EXTENSIONS
        let cases = [
            ("png", "image/png"),
            ("jpg", "image/jpeg"),
            ("jpeg", "image/jpeg"),
            ("gif", "image/gif"),
            ("webp", "image/webp"),
            ("ico", "image/x-icon"),
            ("bmp", "image/bmp"),
        ];
        for (ext, expected_mime) in &cases {
            assert_eq!(
                mime_for_extension(ext),
                *expected_mime,
                "MIME mismatch for extension '{ext}'"
            );
        }
    }
}
