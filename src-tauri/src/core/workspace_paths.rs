use std::ffi::OsString;
use std::path::{Component, Path, PathBuf};

use crate::model::errors::{AppError, ErrorSource};

const BUILTIN_HOME_WRITABLE_ROOT_DIRS: &[&str] = &[".agents", ".tiy", ".cache"];

#[cfg(unix)]
const BUILTIN_SYSTEM_WRITABLE_ROOTS: &[&str] = &["/tmp"];

#[cfg(not(unix))]
const BUILTIN_SYSTEM_WRITABLE_ROOTS: &[&str] = &[];

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
    resolve_path_within_roots(
        workspace_root,
        &[],
        raw_path,
        source,
        outside_code,
        outside_message,
    )
}

/// Resolve an absolute or relative path against the workspace root and any
/// additional allowed roots.
pub fn resolve_path_within_roots(
    workspace_root: &Path,
    additional_roots: &[PathBuf],
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
    if !path_within_allowed_roots(&resolved, workspace_root, additional_roots) {
        return Err(AppError::recoverable(source, outside_code, outside_message));
    }

    Ok(resolved)
}

/// Parse writable root paths from the persisted JSON policy value.
///
/// The value is expected to be a JSON array of objects with a `"path"` field,
/// e.g. `[{"id":"…","path":"/Users/foo/bar"}]`.
pub fn parse_writable_roots(value_json: &str) -> Vec<String> {
    let parsed: serde_json::Value = serde_json::from_str(value_json).unwrap_or_default();
    parsed
        .as_array()
        .map(|entries| {
            entries
                .iter()
                .filter_map(|entry| entry.get("path").and_then(serde_json::Value::as_str))
                .map(str::trim)
                .filter(|path| !path.is_empty())
                .map(ToOwned::to_owned)
                .collect()
        })
        .unwrap_or_default()
}

/// Return builtin writable roots that should always be available at runtime.
pub fn builtin_writable_roots() -> Vec<String> {
    let mut roots = Vec::new();

    if let Some(home_dir) = dirs::home_dir() {
        for dir in BUILTIN_HOME_WRITABLE_ROOT_DIRS {
            push_unique_root(&mut roots, home_dir.join(dir));
        }
    }

    for root in BUILTIN_SYSTEM_WRITABLE_ROOTS {
        push_unique_root(&mut roots, PathBuf::from(root));
    }

    if let Some(tmpdir) = std::env::var_os("TMPDIR") {
        if !tmpdir.is_empty() {
            push_unique_root(&mut roots, PathBuf::from(tmpdir));
        }
    }

    roots
}

/// Merge persisted writable roots with builtin runtime roots and de-duplicate them.
pub fn merge_writable_roots(raw_roots: &[String]) -> Vec<String> {
    let mut merged = Vec::new();

    for root in raw_roots
        .iter()
        .chain(builtin_writable_roots().iter())
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
    {
        if !merged.iter().any(|existing: &String| existing == root) {
            merged.push(root.to_string());
        }
    }

    merged
}

/// Normalize persisted writable root strings into canonical path boundaries.
pub fn normalize_additional_roots(raw_roots: &[String]) -> Vec<PathBuf> {
    raw_roots
        .iter()
        .filter_map(|root| {
            let trimmed = root.trim();
            if trimmed.is_empty() {
                None
            } else {
                Some(canonicalize_lossy(Path::new(trimmed)))
            }
        })
        .collect()
}

fn path_within_allowed_roots(
    resolved: &Path,
    workspace_root: &Path,
    additional_roots: &[PathBuf],
) -> bool {
    resolved.starts_with(workspace_root)
        || additional_roots
            .iter()
            .any(|root| resolved.starts_with(root))
}

fn push_unique_root(roots: &mut Vec<String>, path: PathBuf) {
    let normalized = normalize_path(&path).to_string_lossy().into_owned();
    if !roots.iter().any(|existing| existing == &normalized) {
        roots.push(normalized);
    }
}

fn canonicalize_lossy(path: &Path) -> PathBuf {
    // Use dunce::canonicalize to avoid Windows UNC path prefix (\\?\).
    if let Ok(canonical) = dunce::canonicalize(path) {
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

    let mut resolved =
        dunce::canonicalize(existing).unwrap_or_else(|_| normalize_path(existing));
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
    use super::{
        builtin_writable_roots, canonicalize_workspace_root, merge_writable_roots,
        normalize_additional_roots, normalize_path, parse_writable_roots,
        resolve_path_within_roots, resolve_path_within_workspace,
    };
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

    #[test]
    fn resolves_absolute_paths_inside_additional_writable_root() {
        let workspace = tempfile::tempdir().expect("workspace");
        let writable_root = tempfile::tempdir().expect("writable root");
        let workspace_root =
            std::fs::canonicalize(workspace.path()).expect("workspace should canonicalize");
        let writable_root_path =
            std::fs::canonicalize(writable_root.path()).expect("writable root canonicalize");

        let resolved = resolve_path_within_roots(
            &workspace_root,
            std::slice::from_ref(&writable_root_path),
            &writable_root_path.join("notes.txt").to_string_lossy(),
            ErrorSource::Tool,
            "tool.path.outside_workspace",
            "outside workspace",
        )
        .expect("absolute path inside writable root should resolve");

        assert_eq!(resolved, writable_root_path.join("notes.txt"));
    }

    #[test]
    fn rejects_paths_outside_workspace_and_additional_writable_roots() {
        let workspace = tempfile::tempdir().expect("workspace");
        let writable_root = tempfile::tempdir().expect("writable root");
        let outside = tempfile::tempdir().expect("outside");
        let workspace_root =
            std::fs::canonicalize(workspace.path()).expect("workspace should canonicalize");
        let writable_root_path =
            std::fs::canonicalize(writable_root.path()).expect("writable root canonicalize");
        let outside_path = outside.path().join("escape.txt");

        let error = resolve_path_within_roots(
            &workspace_root,
            std::slice::from_ref(&writable_root_path),
            &outside_path.to_string_lossy(),
            ErrorSource::Tool,
            "tool.path.outside_workspace",
            "outside workspace",
        )
        .expect_err("path outside allowed roots should be rejected");

        assert_eq!(error.error_code, "tool.path.outside_workspace");
    }

    #[test]
    fn builtin_writable_roots_include_builtin_entries() {
        let roots = builtin_writable_roots();

        if let Some(home_dir) = dirs::home_dir() {
            let agents = home_dir.join(".agents").to_string_lossy().into_owned();
            let tiy = home_dir.join(".tiy").to_string_lossy().into_owned();
            let cache = home_dir.join(".cache").to_string_lossy().into_owned();
            assert!(roots.contains(&agents));
            assert!(roots.contains(&tiy));
            assert!(roots.contains(&cache));
        }

        #[cfg(unix)]
        assert!(roots.contains(&"/tmp".to_string()));

        if let Some(tmpdir) = std::env::var_os("TMPDIR") {
            if !tmpdir.is_empty() {
                let normalized = normalize_path(&std::path::PathBuf::from(tmpdir))
                    .to_string_lossy()
                    .into_owned();
                assert!(roots.contains(&normalized));
            }
        }
    }

    #[test]
    fn merge_writable_roots_adds_builtin_roots_without_duplicates() {
        let mut input = vec!["/tmp/custom-root".to_string()];
        let builtin = builtin_writable_roots();
        if let Some(first_builtin) = builtin.first() {
            input.push(first_builtin.clone());
        }

        let merged = merge_writable_roots(&input);

        assert!(merged.contains(&"/tmp/custom-root".to_string()));
        for builtin_root in builtin {
            assert!(merged.contains(&builtin_root));
            assert_eq!(
                merged.iter().filter(|root| *root == &builtin_root).count(),
                1,
                "builtin root should not be duplicated"
            );
        }
    }

    #[test]
    fn normalizes_additional_roots_and_drops_empty_entries() {
        let normalized = normalize_additional_roots(&[
            "".to_string(),
            "   ".to_string(),
            "/tmp/example".to_string(),
        ]);

        assert_eq!(normalized.len(), 1);
        assert!(normalized[0].ends_with("example"));
    }

    #[test]
    fn parses_writable_roots_from_valid_json() {
        let json = r#"[{"id":"abc","path":"/Users/foo/bar"},{"id":"def","path":"/tmp/workspace"}]"#;
        let roots = parse_writable_roots(json);
        assert_eq!(roots, vec!["/Users/foo/bar", "/tmp/workspace"]);
    }

    #[test]
    fn parses_writable_roots_trims_whitespace_and_skips_empty() {
        let json =
            r#"[{"id":"1","path":"  /Users/foo  "},{"id":"2","path":""},{"id":"3","path":"  "}]"#;
        let roots = parse_writable_roots(json);
        assert_eq!(roots, vec!["/Users/foo"]);
    }

    #[test]
    fn parses_writable_roots_returns_empty_for_invalid_json() {
        assert!(parse_writable_roots("not json").is_empty());
        assert!(parse_writable_roots("").is_empty());
        assert!(parse_writable_roots("null").is_empty());
        assert!(parse_writable_roots("42").is_empty());
    }

    #[test]
    fn parses_writable_roots_returns_empty_for_empty_array() {
        assert!(parse_writable_roots("[]").is_empty());
    }

    #[test]
    fn parses_writable_roots_skips_entries_without_path() {
        let json = r#"[{"id":"1"},{"id":"2","path":"/valid"}]"#;
        let roots = parse_writable_roots(json);
        assert_eq!(roots, vec!["/valid"]);
    }
}
