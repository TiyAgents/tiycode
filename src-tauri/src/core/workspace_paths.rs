use std::ffi::OsString;
use std::path::{Component, Path, PathBuf};

use crate::model::errors::{AppError, ErrorSource};

/// Canonicalize a workspace root and ensure it is a directory.
pub fn canonicalize_workspace_root(
    workspace_path: &str,
    source: ErrorSource,
    not_directory_code: &str,
) -> Result<PathBuf, AppError> {
    let root = canonicalize_lossy(Path::new(workspace_path));
    if !root.is_dir() {
        return Err(AppError::recoverable(
            source,
            not_directory_code,
            format!("'{}' is not a directory", workspace_path),
        ));
    }

    Ok(root)
}

/// Resolve an absolute or relative path against the workspace root and enforce the boundary.
pub fn resolve_path_within_workspace(
    workspace_root: &Path,
    raw_path: &str,
    source: ErrorSource,
    outside_code: &str,
    outside_message: impl Into<String>,
) -> Result<PathBuf, AppError> {
    let candidate = if raw_path.is_empty() {
        workspace_root.to_path_buf()
    } else {
        let path = Path::new(raw_path);
        if path.is_absolute() {
            path.to_path_buf()
        } else {
            workspace_root.join(path)
        }
    };

    let resolved = canonicalize_lossy(&candidate);
    if !resolved.starts_with(workspace_root) {
        return Err(AppError::recoverable(source, outside_code, outside_message));
    }

    Ok(resolved)
}

fn canonicalize_lossy(path: &Path) -> PathBuf {
    if let Ok(canonical) = std::fs::canonicalize(path) {
        return canonical;
    }

    let mut existing = path;
    let mut suffix = Vec::<OsString>::new();

    while !existing.exists() {
        let Some(name) = existing.file_name() else {
            return normalize_path(path);
        };
        suffix.push(name.to_os_string());

        let Some(parent) = existing.parent() else {
            return normalize_path(path);
        };

        if parent == existing {
            return normalize_path(path);
        }

        existing = parent;
    }

    let mut resolved = std::fs::canonicalize(existing).unwrap_or_else(|_| normalize_path(existing));
    for segment in suffix.iter().rev() {
        resolved.push(segment);
    }

    normalize_path(&resolved)
}

fn normalize_path(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();

    for component in path.components() {
        match component {
            Component::Prefix(prefix) => normalized.push(prefix.as_os_str()),
            Component::RootDir => normalized.push(component.as_os_str()),
            Component::CurDir => {}
            Component::ParentDir => {
                let _ = normalized.pop();
            }
            Component::Normal(part) => normalized.push(part),
        }
    }

    normalized
}

#[cfg(test)]
mod tests {
    use super::{canonicalize_workspace_root, resolve_path_within_workspace};
    use crate::model::errors::ErrorSource;

    #[test]
    fn resolves_relative_paths_inside_workspace() {
        let tmp = tempfile::tempdir().expect("should create tempdir");
        let workspace = std::fs::canonicalize(tmp.path()).expect("workspace should canonicalize");
        let nested = workspace.join("src-tauri");
        std::fs::create_dir_all(&nested).expect("should create nested dir");

        let resolved = resolve_path_within_workspace(
            &workspace,
            "src-tauri",
            ErrorSource::Tool,
            "tool.path.outside_workspace",
            "outside workspace",
        )
        .expect("relative path should resolve");

        assert_eq!(resolved, nested);
    }

    #[test]
    fn rejects_parent_traversal_outside_workspace() {
        let tmp = tempfile::tempdir().expect("should create tempdir");
        let workspace = std::fs::canonicalize(tmp.path()).expect("workspace should canonicalize");

        let error = resolve_path_within_workspace(
            &workspace,
            "../escape",
            ErrorSource::Tool,
            "tool.path.outside_workspace",
            "outside workspace",
        )
        .expect_err("parent traversal should be rejected");

        assert_eq!(error.error_code, "tool.path.outside_workspace");
    }

    #[cfg(unix)]
    #[test]
    fn rejects_missing_child_under_symlinked_parent() {
        use std::os::unix::fs::symlink;

        let tmp = tempfile::tempdir().expect("should create tempdir");
        let workspace = std::fs::canonicalize(tmp.path()).expect("workspace should canonicalize");
        let outside = tempfile::tempdir().expect("should create outside dir");
        let link_path = workspace.join("linked-outside");
        symlink(outside.path(), &link_path).expect("should create symlink");

        let error = resolve_path_within_workspace(
            &workspace,
            "linked-outside/new-file.txt",
            ErrorSource::Tool,
            "tool.path.outside_workspace",
            "outside workspace",
        )
        .expect_err("symlink escape should be rejected");

        assert_eq!(error.error_code, "tool.path.outside_workspace");
    }

    #[test]
    fn canonicalized_workspace_must_be_directory() {
        let tmp = tempfile::tempdir().expect("should create tempdir");
        let file_path = tmp.path().join("README.md");
        std::fs::write(&file_path, "hello").expect("should create file");

        let error = canonicalize_workspace_root(
            &file_path.to_string_lossy(),
            ErrorSource::Index,
            "index.path.not_directory",
        )
        .expect_err("file path should be rejected as workspace");

        assert_eq!(error.error_code, "index.path.not_directory");
    }
}
