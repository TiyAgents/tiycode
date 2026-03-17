//! Workspace file tree cache and ripgrep text search.
//!
//! Phase 2 scope:
//! - Shallow tree scan optimized for first paint
//! - On-demand child loading for expandable directories
//! - Workspace-wide file manifest for complete path filtering
//! - Ripgrep subprocess for text search

use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::process::Command;
use tokio::sync::RwLock;

use crate::model::errors::{AppError, ErrorSource};

/// Entries never shown in the tree or file manifest.
const ALWAYS_SKIPPED: &[&str] = &[".git", ".DS_Store", "thumbs.db"];

/// Heavy directories excluded from full-path filtering and deep tree expansion.
const HEAVY_EXCLUDED_DIRS: &[&str] = &[
    "node_modules",
    ".next",
    "target",
    "dist",
    "build",
    ".cache",
    "__pycache__",
];

/// Load root + first two visible levels eagerly. Deeper levels load on demand.
const INITIAL_LOADED_DEPTH: usize = 2;

/// Cache workspace file manifests briefly so repeated filter input stays fast.
const MANIFEST_TTL: Duration = Duration::from_secs(2);

/// Max file filter results returned to the UI in one request.
const DEFAULT_FILTER_LIMIT: usize = 200;

// ---------------------------------------------------------------------------
// File tree types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileTreeNode {
    pub name: String,
    pub path: String,
    pub is_dir: bool,
    pub is_expandable: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub git_state: Option<GitFileState>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub children: Option<Vec<FileTreeNode>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum GitFileState {
    Tracked,
    Untracked,
    Ignored,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileTreeResponse {
    pub repo_available: bool,
    pub tree: FileTreeNode,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileFilterMatch {
    pub name: String,
    pub path: String,
    pub parent_path: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileFilterResponse {
    pub query: String,
    pub results: Vec<FileFilterMatch>,
    pub count: usize,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchResult {
    pub path: String,
    pub absolute_path: String,
    pub line_number: u64,
    pub line_text: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchResponse {
    pub query: String,
    pub results: Vec<SearchResult>,
    pub count: usize,
}

// ---------------------------------------------------------------------------
// IndexManager
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
struct ManifestCacheEntry {
    built_at: Instant,
    files: Arc<Vec<String>>,
}

#[derive(Clone)]
pub struct IndexManager {
    manifest_cache: Arc<RwLock<HashMap<String, ManifestCacheEntry>>>,
}

impl FileTreeNode {
    pub fn apply_git_overlay(&mut self, states: &HashMap<String, GitFileState>) {
        annotate_git_state(self, states);
    }
}

impl IndexManager {
    pub fn new() -> Self {
        Self {
            manifest_cache: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Scan workspace directory and return a shallow, expandable file tree.
    pub async fn get_tree(&self, workspace_path: &str) -> Result<FileTreeNode, AppError> {
        let root = canonicalize_workspace(workspace_path)?;

        tokio::task::spawn_blocking(move || build_initial_tree(&root))
            .await
            .map_err(|error| {
                AppError::internal(
                    ErrorSource::Index,
                    format!("Initial tree task failed: {error}"),
                )
            })?
    }

    /// Load a directory's direct children on demand.
    pub async fn get_children(
        &self,
        workspace_path: &str,
        directory_path: &str,
    ) -> Result<Vec<FileTreeNode>, AppError> {
        let root = canonicalize_workspace(workspace_path)?;
        let target = resolve_workspace_directory(&root, directory_path)?;

        tokio::task::spawn_blocking(move || load_directory_children(&root, &target))
            .await
            .map_err(|error| {
                AppError::internal(
                    ErrorSource::Index,
                    format!("Directory children task failed: {error}"),
                )
            })?
    }

    /// Filter all visible files in the workspace using the cached manifest.
    pub async fn filter_files(
        &self,
        workspace_path: &str,
        query: &str,
        max_results: Option<usize>,
    ) -> Result<FileFilterResponse, AppError> {
        let normalized_query = query.trim().to_lowercase();
        if normalized_query.is_empty() {
            return Err(AppError::recoverable(
                ErrorSource::Index,
                "index.filter.empty_query",
                "File filter query cannot be empty",
            ));
        }

        let root = canonicalize_workspace(workspace_path)?;
        let manifest = self.get_or_build_manifest(&root).await?;
        let limit = max_results.unwrap_or(DEFAULT_FILTER_LIMIT);

        let mut results = Vec::new();

        for path in manifest.iter() {
            if !path.to_lowercase().contains(&normalized_query) {
                continue;
            }

            let name = Path::new(path)
                .file_name()
                .and_then(|value| value.to_str())
                .unwrap_or(path)
                .to_string();
            let parent_path = Path::new(path)
                .parent()
                .and_then(|value| value.to_str())
                .unwrap_or("")
                .to_string();

            results.push(FileFilterMatch {
                name,
                path: path.clone(),
                parent_path,
            });

            if results.len() >= limit {
                break;
            }
        }

        Ok(FileFilterResponse {
            query: query.to_string(),
            count: results.len(),
            results,
        })
    }

    /// Search workspace files using ripgrep.
    pub async fn search(
        &self,
        workspace_path: &str,
        query: &str,
        file_pattern: Option<&str>,
        max_results: Option<usize>,
    ) -> Result<SearchResponse, AppError> {
        if query.is_empty() {
            return Err(AppError::recoverable(
                ErrorSource::Index,
                "index.search.empty_query",
                "Search query cannot be empty",
            ));
        }

        let limit = max_results.unwrap_or(50);

        let mut cmd = Command::new("rg");
        cmd.arg("--json")
            .arg("--max-count")
            .arg(limit.to_string())
            .arg("--max-filesize=1M")
            .arg(query)
            .arg(workspace_path)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());

        if let Some(pattern) = file_pattern {
            cmd.arg("--glob").arg(pattern);
        }

        let output = cmd.output().await.map_err(|e| {
            AppError::recoverable(
                ErrorSource::Index,
                "index.search.rg_failed",
                format!("ripgrep failed: {e}. Ensure 'rg' is installed."),
            )
        })?;

        let stdout = String::from_utf8_lossy(&output.stdout);
        let results = parse_rg_json(&stdout, workspace_path);
        let count = results.len();

        Ok(SearchResponse {
            query: query.to_string(),
            results,
            count,
        })
    }

    async fn get_or_build_manifest(
        &self,
        workspace_root: &Path,
    ) -> Result<Arc<Vec<String>>, AppError> {
        let cache_key = workspace_root.to_string_lossy().to_string();

        if let Some(cached) = self.manifest_cache.read().await.get(&cache_key) {
            if cached.built_at.elapsed() < MANIFEST_TTL {
                return Ok(Arc::clone(&cached.files));
            }
        }

        let root = workspace_root.to_path_buf();
        let files = tokio::task::spawn_blocking(move || build_file_manifest(&root))
            .await
            .map_err(|error| {
                AppError::internal(
                    ErrorSource::Index,
                    format!("File manifest task failed: {error}"),
                )
            })??;

        let files = Arc::new(files);

        self.manifest_cache.write().await.insert(
            cache_key,
            ManifestCacheEntry {
                built_at: Instant::now(),
                files: Arc::clone(&files),
            },
        );

        Ok(files)
    }
}

// ---------------------------------------------------------------------------
// Tree loading
// ---------------------------------------------------------------------------

fn canonicalize_workspace(workspace_path: &str) -> Result<PathBuf, AppError> {
    let root =
        std::fs::canonicalize(workspace_path).unwrap_or_else(|_| PathBuf::from(workspace_path));
    if !root.is_dir() {
        return Err(AppError::recoverable(
            ErrorSource::Index,
            "index.path.not_directory",
            format!("'{}' is not a directory", workspace_path),
        ));
    }

    Ok(root)
}

fn resolve_workspace_directory(root: &Path, directory_path: &str) -> Result<PathBuf, AppError> {
    let candidate = if directory_path.is_empty() {
        root.to_path_buf()
    } else {
        root.join(directory_path)
    };

    let canonical = std::fs::canonicalize(&candidate).unwrap_or(candidate);
    if !canonical.starts_with(root) {
        return Err(AppError::recoverable(
            ErrorSource::Index,
            "index.path.out_of_workspace",
            "Requested directory is outside the workspace boundary",
        ));
    }

    if !canonical.is_dir() {
        return Err(AppError::recoverable(
            ErrorSource::Index,
            "index.path.not_directory",
            format!("'{}' is not a directory", directory_path),
        ));
    }

    Ok(canonical)
}

fn build_initial_tree(root: &Path) -> Result<FileTreeNode, AppError> {
    let skipped: HashSet<&str> = ALWAYS_SKIPPED.iter().copied().collect();
    let excluded: HashSet<&str> = HEAVY_EXCLUDED_DIRS.iter().copied().collect();

    scan_tree_node(root, root, &skipped, &excluded, 0, INITIAL_LOADED_DEPTH)
}

fn load_directory_children(root: &Path, target: &Path) -> Result<Vec<FileTreeNode>, AppError> {
    let skipped: HashSet<&str> = ALWAYS_SKIPPED.iter().copied().collect();
    let excluded: HashSet<&str> = HEAVY_EXCLUDED_DIRS.iter().copied().collect();

    read_directory_entries(root, target, &skipped, &excluded, true)
}

fn scan_tree_node(
    path: &Path,
    root: &Path,
    skipped: &HashSet<&str>,
    excluded: &HashSet<&str>,
    depth: usize,
    preload_depth: usize,
) -> Result<FileTreeNode, AppError> {
    if !path.is_dir() {
        return Ok(make_file_node(path, root));
    }

    let children = read_directory_entries(root, path, skipped, excluded, depth < preload_depth)?;

    Ok(FileTreeNode {
        name: node_name(path, root),
        path: relative_path(path, root),
        is_dir: true,
        is_expandable: !children.is_empty(),
        git_state: None,
        children: Some(children),
    })
}

fn read_directory_entries(
    root: &Path,
    directory: &Path,
    skipped: &HashSet<&str>,
    excluded: &HashSet<&str>,
    load_child_directories: bool,
) -> Result<Vec<FileTreeNode>, AppError> {
    let mut entries = Vec::new();

    for entry in fs::read_dir(directory).map_err(|error| {
        AppError::internal(ErrorSource::Index, format!("Failed to read dir: {error}"))
    })? {
        let entry = entry.map_err(|error| {
            AppError::internal(
                ErrorSource::Index,
                format!("Failed to read dir entry: {error}"),
            )
        })?;
        let entry_path = entry.path();
        let entry_name = entry.file_name().to_string_lossy().to_string();

        if skipped.contains(entry_name.as_str()) {
            continue;
        }

        let file_type = entry.file_type().map_err(|error| {
            AppError::internal(
                ErrorSource::Index,
                format!("Failed to inspect dir entry: {error}"),
            )
        })?;

        if file_type.is_dir() {
            if excluded.contains(entry_name.as_str()) {
                entries.push(FileTreeNode {
                    name: entry_name,
                    path: relative_path(&entry_path, root),
                    is_dir: true,
                    is_expandable: false,
                    git_state: None,
                    children: None,
                });
                continue;
            }

            if load_child_directories {
                entries.push(make_lazy_directory_node(
                    &entry_path,
                    root,
                    skipped,
                    excluded,
                )?);
            } else {
                entries.push(make_directory_placeholder(
                    &entry_path,
                    root,
                    skipped,
                    excluded,
                )?);
            }
        } else if file_type.is_file() {
            entries.push(make_file_node(&entry_path, root));
        }
    }

    sort_tree_nodes(&mut entries);
    Ok(entries)
}

fn make_lazy_directory_node(
    path: &Path,
    root: &Path,
    skipped: &HashSet<&str>,
    excluded: &HashSet<&str>,
) -> Result<FileTreeNode, AppError> {
    let children = read_directory_entries(root, path, skipped, excluded, false)?;

    Ok(FileTreeNode {
        name: node_name(path, root),
        path: relative_path(path, root),
        is_dir: true,
        is_expandable: !children.is_empty(),
        git_state: None,
        children: Some(children),
    })
}

fn make_directory_placeholder(
    path: &Path,
    root: &Path,
    skipped: &HashSet<&str>,
    excluded: &HashSet<&str>,
) -> Result<FileTreeNode, AppError> {
    Ok(FileTreeNode {
        name: node_name(path, root),
        path: relative_path(path, root),
        is_dir: true,
        is_expandable: directory_has_visible_entries(path, skipped, excluded)?,
        git_state: None,
        children: None,
    })
}

fn make_file_node(path: &Path, root: &Path) -> FileTreeNode {
    FileTreeNode {
        name: node_name(path, root),
        path: relative_path(path, root),
        is_dir: false,
        is_expandable: false,
        git_state: None,
        children: None,
    }
}

fn directory_has_visible_entries(
    path: &Path,
    skipped: &HashSet<&str>,
    _excluded: &HashSet<&str>,
) -> Result<bool, AppError> {
    for entry in fs::read_dir(path).map_err(|error| {
        AppError::internal(ErrorSource::Index, format!("Failed to read dir: {error}"))
    })? {
        let entry = entry.map_err(|error| {
            AppError::internal(
                ErrorSource::Index,
                format!("Failed to read dir entry: {error}"),
            )
        })?;
        let entry_name = entry.file_name().to_string_lossy().to_string();
        if skipped.contains(entry_name.as_str()) {
            continue;
        }

        return Ok(true);
    }

    Ok(false)
}

fn sort_tree_nodes(nodes: &mut [FileTreeNode]) {
    nodes.sort_by(|left, right| match (left.is_dir, right.is_dir) {
        (true, false) => std::cmp::Ordering::Less,
        (false, true) => std::cmp::Ordering::Greater,
        _ => left.name.cmp(&right.name),
    });
}

fn node_name(path: &Path, root: &Path) -> String {
    path.file_name()
        .and_then(|value| value.to_str())
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| {
            root.file_name()
                .and_then(|value| value.to_str())
                .unwrap_or("")
                .to_string()
        })
}

fn relative_path(path: &Path, root: &Path) -> String {
    path.strip_prefix(root)
        .map(|value| value.to_string_lossy().to_string())
        .unwrap_or_default()
}

fn build_file_manifest(root: &Path) -> Result<Vec<String>, AppError> {
    let skipped: HashSet<&str> = ALWAYS_SKIPPED.iter().copied().collect();
    let excluded: HashSet<&str> = HEAVY_EXCLUDED_DIRS.iter().copied().collect();
    let mut files = Vec::new();

    collect_manifest_paths(root, root, &skipped, &excluded, &mut files)?;
    files.sort();

    Ok(files)
}

fn collect_manifest_paths(
    root: &Path,
    directory: &Path,
    skipped: &HashSet<&str>,
    excluded: &HashSet<&str>,
    files: &mut Vec<String>,
) -> Result<(), AppError> {
    for entry in fs::read_dir(directory).map_err(|error| {
        AppError::internal(ErrorSource::Index, format!("Failed to read dir: {error}"))
    })? {
        let entry = entry.map_err(|error| {
            AppError::internal(
                ErrorSource::Index,
                format!("Failed to read dir entry: {error}"),
            )
        })?;
        let entry_path = entry.path();
        let entry_name = entry.file_name().to_string_lossy().to_string();
        let file_type = entry.file_type().map_err(|error| {
            AppError::internal(
                ErrorSource::Index,
                format!("Failed to inspect dir entry: {error}"),
            )
        })?;

        if skipped.contains(entry_name.as_str()) {
            continue;
        }

        if file_type.is_dir() {
            if excluded.contains(entry_name.as_str()) {
                continue;
            }

            collect_manifest_paths(root, &entry_path, skipped, excluded, files)?;
            continue;
        }

        if file_type.is_file() {
            files.push(relative_path(&entry_path, root));
        }
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Git overlay helpers
// ---------------------------------------------------------------------------

fn annotate_git_state(
    node: &mut FileTreeNode,
    states: &HashMap<String, GitFileState>,
) -> Option<GitFileState> {
    let child_state = node.children.as_mut().and_then(|children| {
        let mut aggregate = None;

        for child in children {
            if let Some(state) = annotate_git_state(child, states) {
                aggregate = Some(match aggregate {
                    Some(current) => strongest_git_state(current, state),
                    None => state,
                });
            }
        }

        aggregate
    });

    let direct_state = if node.path.is_empty() {
        None
    } else {
        states.get(&node.path).copied()
    };

    let resolved = match (direct_state, child_state) {
        (Some(direct), Some(child)) => Some(strongest_git_state(direct, child)),
        (Some(direct), None) => Some(direct),
        (None, Some(child)) => Some(child),
        (None, None) => None,
    };

    node.git_state = resolved;
    resolved
}

fn strongest_git_state(left: GitFileState, right: GitFileState) -> GitFileState {
    if git_state_priority(left) >= git_state_priority(right) {
        left
    } else {
        right
    }
}

fn git_state_priority(state: GitFileState) -> u8 {
    match state {
        GitFileState::Ignored => 1,
        GitFileState::Tracked => 2,
        GitFileState::Untracked => 3,
    }
}

// ---------------------------------------------------------------------------
// Ripgrep JSON output parser
// ---------------------------------------------------------------------------

fn parse_rg_json(output: &str, workspace_path: &str) -> Vec<SearchResult> {
    let mut results = Vec::new();

    for line in output.lines() {
        if let Ok(entry) = serde_json::from_str::<serde_json::Value>(line) {
            if entry["type"].as_str() == Some("match") {
                let data = &entry["data"];
                let abs_path = data["path"]["text"].as_str().unwrap_or("");
                let line_number = data["line_number"].as_u64().unwrap_or(0);
                let line_text = data["lines"]["text"]
                    .as_str()
                    .unwrap_or("")
                    .trim()
                    .to_string();

                let rel_path = Path::new(abs_path)
                    .strip_prefix(workspace_path)
                    .map(|p| p.to_string_lossy().to_string())
                    .unwrap_or_else(|_| abs_path.to_string());

                results.push(SearchResult {
                    path: rel_path,
                    absolute_path: abs_path.to_string(),
                    line_number,
                    line_text,
                });
            }
        }
    }

    results
}
