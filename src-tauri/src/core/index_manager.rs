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
use tokio::sync::RwLock;

use crate::core::ripgrep::run_rg;
use crate::core::workspace_paths::{canonicalize_workspace_root, resolve_path_within_workspace};
use crate::model::errors::{AppError, ErrorSource};
use crate::model::git::GitFileState;

/// Entries never shown in the tree or file manifest.
const ALWAYS_SKIPPED: &[&str] = &[".git", ".DS_Store", "thumbs.db"];

/// Heavy directories excluded from the workspace-wide file filter manifest.
const FILTER_EXCLUDED_DIRS: &[&str] = &[
    "node_modules",
    ".next",
    "target",
    "dist",
    "build",
    ".cache",
    "__pycache__",
];

/// Large directories stay visible in TreeView, but skip eager child preloading.
const TREE_LAZY_ONLY_DIRS: &[&str] = &[
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

/// Max children loaded for a directory page in TreeView.
const DIRECTORY_PAGE_SIZE: usize = 200;

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
    pub children_has_more: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub children_next_offset: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub git_state: Option<GitFileState>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub children: Option<Vec<FileTreeNode>>,
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
pub struct DirectoryChildrenResponse {
    pub children: Vec<FileTreeNode>,
    pub has_more: bool,
    pub next_offset: Option<usize>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RevealPathSegment {
    pub directory_path: String,
    pub children: Vec<FileTreeNode>,
    pub has_more: bool,
    pub next_offset: Option<usize>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RevealPathResponse {
    pub target_path: String,
    pub segments: Vec<RevealPathSegment>,
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

#[derive(Debug, Clone)]
struct DirectoryPage {
    children: Vec<FileTreeNode>,
    has_more: bool,
    next_offset: Option<usize>,
}

#[derive(Debug)]
struct DirectoryEntry {
    name: String,
    path: PathBuf,
    is_dir: bool,
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
        offset: Option<usize>,
        max_results: Option<usize>,
    ) -> Result<DirectoryChildrenResponse, AppError> {
        let root = canonicalize_workspace(workspace_path)?;
        let target = resolve_workspace_directory(&root, directory_path)?;
        let page_offset = offset.unwrap_or(0);
        let page_size = max_results.unwrap_or(DIRECTORY_PAGE_SIZE);

        tokio::task::spawn_blocking(move || {
            load_directory_children(&root, &target, page_offset, page_size)
        })
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

    /// Materialize the directory chain needed to reveal a target path in the tree.
    pub async fn reveal_path(
        &self,
        workspace_path: &str,
        target_path: &str,
    ) -> Result<RevealPathResponse, AppError> {
        let normalized_target = target_path.trim().trim_matches('/');
        if normalized_target.is_empty() {
            return Err(AppError::recoverable(
                ErrorSource::Index,
                "index.reveal.empty_path",
                "Reveal target path cannot be empty",
            ));
        }

        let root = canonicalize_workspace(workspace_path)?;
        let target = resolve_workspace_entry(&root, normalized_target)?;
        let relative_target = relative_path(&target, &root);
        let components = relative_target
            .split('/')
            .filter(|component| !component.is_empty())
            .collect::<Vec<_>>();

        if components.is_empty() {
            return Err(AppError::recoverable(
                ErrorSource::Index,
                "index.reveal.invalid_path",
                "Reveal target path must resolve inside the workspace",
            ));
        }

        let skipped: HashSet<&str> = ALWAYS_SKIPPED.iter().copied().collect();
        let tree_lazy_only: HashSet<&str> = TREE_LAZY_ONLY_DIRS.iter().copied().collect();
        let mut segments = Vec::with_capacity(components.len());

        for component_index in 0..components.len() {
            let directory_path = if component_index == 0 {
                String::new()
            } else {
                components[..component_index].join("/")
            };
            let child_path = components[..=component_index].join("/");

            let page = tokio::task::spawn_blocking({
                let root = root.clone();
                let directory_path = directory_path.clone();
                let child_path = child_path.clone();
                let skipped = skipped.clone();
                let tree_lazy_only = tree_lazy_only.clone();

                move || {
                    load_directory_entries_until_contains(
                        &root,
                        &directory_path,
                        &child_path,
                        &skipped,
                        &tree_lazy_only,
                    )
                }
            })
            .await
            .map_err(|error| {
                AppError::internal(
                    ErrorSource::Index,
                    format!("Reveal path task failed: {error}"),
                )
            })??;

            segments.push(RevealPathSegment {
                directory_path,
                children: page.children,
                has_more: page.has_more,
                next_offset: page.next_offset,
            });
        }

        Ok(RevealPathResponse {
            target_path: relative_target,
            segments,
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

        let limit = max_results.unwrap_or(50).max(1);
        let normalized_file_pattern = normalize_search_file_pattern(file_pattern);

        let mut args = vec![
            "--json".into(),
            "--max-count".into(),
            limit.to_string().into(),
            "--max-filesize=1M".into(),
            query.into(),
            workspace_path.into(),
        ];
        if let Some(pattern) = normalized_file_pattern {
            args.push("--glob".into());
            args.push(pattern.into());
        }

        let output = run_rg(args).await.map_err(|e| {
            AppError::recoverable(
                ErrorSource::Index,
                "index.search.rg_failed",
                format!(
                    "ripgrep failed: {e}. Ensure 'rg' is installed, reachable from a login shell, or bundled with the app."
                ),
            )
        })?;

        if !output.status.success() && output.status.code() != Some(1) {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let message = stderr.trim();

            return Err(AppError::recoverable(
                ErrorSource::Index,
                "index.search.rg_failed",
                if message.is_empty() {
                    format!("ripgrep search failed with status {}", output.status)
                } else {
                    format!("ripgrep search failed: {message}")
                },
            ));
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        let mut results = parse_rg_json(&stdout, workspace_path);
        if results.len() > limit {
            results.truncate(limit);
        }
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
    canonicalize_workspace_root(
        workspace_path,
        ErrorSource::Index,
        "index.path.not_directory",
    )
}

fn resolve_workspace_directory(root: &Path, directory_path: &str) -> Result<PathBuf, AppError> {
    let canonical = resolve_path_within_workspace(
        root,
        directory_path,
        ErrorSource::Index,
        "index.path.out_of_workspace",
        "Requested directory is outside the workspace boundary",
    )?;

    if !canonical.is_dir() {
        return Err(AppError::recoverable(
            ErrorSource::Index,
            "index.path.not_directory",
            format!("'{}' is not a directory", directory_path),
        ));
    }

    Ok(canonical)
}

fn resolve_workspace_entry(root: &Path, entry_path: &str) -> Result<PathBuf, AppError> {
    let canonical = resolve_path_within_workspace(
        root,
        entry_path,
        ErrorSource::Index,
        "index.path.out_of_workspace",
        "Requested path is outside the workspace boundary",
    )?;

    if !canonical.exists() {
        return Err(AppError::recoverable(
            ErrorSource::Index,
            "index.path.not_found",
            format!("'{}' no longer exists", entry_path),
        ));
    }

    Ok(canonical)
}

fn build_initial_tree(root: &Path) -> Result<FileTreeNode, AppError> {
    let skipped: HashSet<&str> = ALWAYS_SKIPPED.iter().copied().collect();
    let tree_lazy_only: HashSet<&str> = TREE_LAZY_ONLY_DIRS.iter().copied().collect();

    scan_tree_node(
        root,
        root,
        &skipped,
        &tree_lazy_only,
        0,
        INITIAL_LOADED_DEPTH,
    )
}

fn load_directory_children(
    root: &Path,
    target: &Path,
    offset: usize,
    max_results: usize,
) -> Result<DirectoryChildrenResponse, AppError> {
    let skipped: HashSet<&str> = ALWAYS_SKIPPED.iter().copied().collect();
    let tree_lazy_only: HashSet<&str> = TREE_LAZY_ONLY_DIRS.iter().copied().collect();
    let page = read_directory_entries_page(
        root,
        target,
        &skipped,
        &tree_lazy_only,
        false,
        offset,
        max_results,
    )?;

    Ok(DirectoryChildrenResponse {
        children: page.children,
        has_more: page.has_more,
        next_offset: page.next_offset,
    })
}

fn load_directory_entries_until_contains(
    root: &Path,
    directory_path: &str,
    child_path: &str,
    skipped: &HashSet<&str>,
    tree_lazy_only: &HashSet<&str>,
) -> Result<DirectoryPage, AppError> {
    let directory = resolve_workspace_directory(root, directory_path)?;
    let mut offset = 0usize;
    let mut merged_children = Vec::new();

    loop {
        let page = read_directory_entries_page(
            root,
            &directory,
            skipped,
            tree_lazy_only,
            false,
            offset,
            DIRECTORY_PAGE_SIZE,
        )?;
        let contains_child = page.children.iter().any(|child| child.path == child_path);

        merged_children.extend(page.children);

        if contains_child {
            return Ok(DirectoryPage {
                children: merged_children,
                has_more: page.has_more,
                next_offset: page.next_offset,
            });
        }

        if !page.has_more {
            return Err(AppError::recoverable(
                ErrorSource::Index,
                "index.reveal.target_not_found",
                format!(
                    "Could not materialize '{}' from directory '{}'",
                    child_path, directory_path
                ),
            ));
        }

        offset = page.next_offset.unwrap_or(offset + DIRECTORY_PAGE_SIZE);
    }
}

fn scan_tree_node(
    path: &Path,
    root: &Path,
    skipped: &HashSet<&str>,
    tree_lazy_only: &HashSet<&str>,
    depth: usize,
    preload_depth: usize,
) -> Result<FileTreeNode, AppError> {
    if !path.is_dir() {
        return Ok(make_file_node(path, root));
    }

    let page = if depth == 0 {
        read_directory_entries_page(
            root,
            path,
            skipped,
            tree_lazy_only,
            depth < preload_depth,
            0,
            usize::MAX,
        )?
    } else {
        read_directory_entries_page(
            root,
            path,
            skipped,
            tree_lazy_only,
            depth < preload_depth,
            0,
            DIRECTORY_PAGE_SIZE,
        )?
    };

    Ok(FileTreeNode {
        name: node_name(path, root),
        path: relative_path(path, root),
        is_dir: true,
        is_expandable: !page.children.is_empty() || page.has_more,
        children_has_more: page.has_more,
        children_next_offset: page.next_offset,
        git_state: None,
        children: Some(page.children),
    })
}

fn read_directory_entries_page(
    root: &Path,
    directory: &Path,
    skipped: &HashSet<&str>,
    tree_lazy_only: &HashSet<&str>,
    preload_child_directories: bool,
    offset: usize,
    limit: usize,
) -> Result<DirectoryPage, AppError> {
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

        entries.push(DirectoryEntry {
            name: entry_name,
            path: entry_path,
            is_dir: file_type.is_dir(),
        });
    }

    sort_directory_entries(&mut entries);

    if offset >= entries.len() {
        return Ok(DirectoryPage {
            children: Vec::new(),
            has_more: false,
            next_offset: None,
        });
    }

    let end = offset.saturating_add(limit).min(entries.len());
    let has_more = end < entries.len();
    let next_offset = has_more.then_some(end);
    let mut nodes = Vec::new();

    for entry in entries.into_iter().skip(offset).take(end - offset) {
        if entry.is_dir {
            if preload_child_directories && !tree_lazy_only.contains(entry.name.as_str()) {
                nodes.push(make_preloaded_directory_node(
                    &entry.path,
                    root,
                    skipped,
                    tree_lazy_only,
                )?);
            } else {
                nodes.push(make_directory_placeholder(
                    &entry.path,
                    root,
                    skipped,
                    tree_lazy_only,
                )?);
            }
        } else {
            nodes.push(make_file_node(&entry.path, root));
        }
    }

    Ok(DirectoryPage {
        children: nodes,
        has_more,
        next_offset,
    })
}

fn make_preloaded_directory_node(
    path: &Path,
    root: &Path,
    skipped: &HashSet<&str>,
    tree_lazy_only: &HashSet<&str>,
) -> Result<FileTreeNode, AppError> {
    let page = read_directory_entries_page(
        root,
        path,
        skipped,
        tree_lazy_only,
        false,
        0,
        DIRECTORY_PAGE_SIZE,
    )?;

    Ok(FileTreeNode {
        name: node_name(path, root),
        path: relative_path(path, root),
        is_dir: true,
        is_expandable: !page.children.is_empty() || page.has_more,
        children_has_more: page.has_more,
        children_next_offset: page.next_offset,
        git_state: None,
        children: Some(page.children),
    })
}

fn make_directory_placeholder(
    path: &Path,
    root: &Path,
    skipped: &HashSet<&str>,
    tree_lazy_only: &HashSet<&str>,
) -> Result<FileTreeNode, AppError> {
    Ok(FileTreeNode {
        name: node_name(path, root),
        path: relative_path(path, root),
        is_dir: true,
        is_expandable: directory_has_visible_entries(path, skipped, tree_lazy_only)?,
        children_has_more: false,
        children_next_offset: None,
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
        children_has_more: false,
        children_next_offset: None,
        git_state: None,
        children: None,
    }
}

fn directory_has_visible_entries(
    path: &Path,
    skipped: &HashSet<&str>,
    _tree_lazy_only: &HashSet<&str>,
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

fn sort_directory_entries(nodes: &mut [DirectoryEntry]) {
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
    let excluded: HashSet<&str> = FILTER_EXCLUDED_DIRS.iter().copied().collect();
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
        GitFileState::Modified => 3,
        GitFileState::Untracked => 4,
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

fn normalize_search_file_pattern(file_pattern: Option<&str>) -> Option<&str> {
    let trimmed = file_pattern?.trim();
    if trimmed.is_empty() || matches!(trimmed, "*" | "**" | "**/*" | "./*" | "./**/*") {
        None
    } else {
        Some(trimmed)
    }
}
