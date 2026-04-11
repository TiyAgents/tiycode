//! Workspace file tree cache and local text search.
//!
//! Phase 2 scope:
//! - Shallow tree scan optimized for first paint
//! - On-demand child loading for expandable directories
//! - Workspace-wide file manifest for complete path filtering
//! - Shared in-process search engine for text search

use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{Mutex, RwLock};

use crate::core::local_search::{
    normalize_file_pattern as normalize_search_file_pattern, run_local_search, stream_local_search,
    LocalSearchBatch, LocalSearchCancellation, LocalSearchOutcome, LocalSearchRequest,
    SearchFileCount as LocalSearchFileCount, SearchFileMatch as LocalSearchFileMatch,
    SearchOutputMode, SearchQueryMode,
};
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

/// Keep first paint to the root listing; child directories load when expanded.
const INITIAL_LOADED_DEPTH: usize = 0;

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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub end_line_number: Option<u64>,
    pub line_text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub match_text: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchFileMatch {
    pub path: String,
    pub absolute_path: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchFileCount {
    pub path: String,
    pub absolute_path: String,
    pub count: usize,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchResponse {
    pub query: String,
    pub query_mode: String,
    pub output_mode: String,
    pub results: Vec<SearchResult>,
    pub files: Vec<SearchFileMatch>,
    pub file_counts: Vec<SearchFileCount>,
    pub count: usize,
    pub total_count: usize,
    pub total_files: usize,
    pub completed: bool,
    pub cancelled: bool,
    pub timed_out: bool,
    pub partial: bool,
    pub elapsed_ms: u64,
    pub searched_files: usize,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchBatchResponse {
    pub query: String,
    pub output_mode: String,
    pub results: Vec<SearchResult>,
    pub files: Vec<SearchFileMatch>,
    pub file_counts: Vec<SearchFileCount>,
    pub count: usize,
    pub total_count: usize,
    pub total_files: usize,
    pub searched_files: usize,
}

#[derive(Debug, Clone)]
pub struct SearchOptions {
    pub file_pattern: Option<String>,
    pub file_type: Option<String>,
    pub max_results: Option<usize>,
    pub query_mode: SearchQueryMode,
    pub output_mode: SearchOutputMode,
    pub case_insensitive: bool,
    pub multiline: bool,
    pub timeout: Option<Duration>,
    pub cancellation: Option<LocalSearchCancellation>,
}

impl Default for SearchOptions {
    fn default() -> Self {
        Self {
            file_pattern: None,
            file_type: None,
            max_results: None,
            query_mode: SearchQueryMode::Literal,
            output_mode: SearchOutputMode::Content,
            case_insensitive: false,
            multiline: false,
            timeout: None,
            cancellation: None,
        }
    }
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
    canonical_cache: Arc<RwLock<HashMap<String, PathBuf>>>,
    stream_cancellations: Arc<Mutex<HashMap<u32, LocalSearchCancellation>>>,
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
            canonical_cache: Arc::new(RwLock::new(HashMap::new())),
            stream_cancellations: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub async fn register_stream_search(&self, search_id: u32) -> LocalSearchCancellation {
        let cancellation = LocalSearchCancellation::new();
        let mut searches = self.stream_cancellations.lock().await;
        searches.insert(search_id, cancellation.clone());
        cancellation
    }

    pub async fn cancel_stream_search(&self, search_id: u32) {
        if let Some(cancellation) = self
            .stream_cancellations
            .lock()
            .await
            .get(&search_id)
            .cloned()
        {
            cancellation.cancel();
        }
    }

    pub async fn finish_stream_search(&self, search_id: u32) {
        self.stream_cancellations.lock().await.remove(&search_id);
    }

    /// Canonicalize a workspace path with in-memory caching to avoid repeated
    /// `dunce::canonicalize` syscalls on Windows.
    async fn cached_canonicalize(&self, workspace_path: &str) -> Result<PathBuf, AppError> {
        {
            let cache = self.canonical_cache.read().await;
            if let Some(cached) = cache.get(workspace_path) {
                return Ok(cached.clone());
            }
        }

        let canonical = canonicalize_workspace(workspace_path)?;

        self.canonical_cache
            .write()
            .await
            .insert(workspace_path.to_string(), canonical.clone());

        Ok(canonical)
    }

    /// Scan workspace directory and return a shallow, expandable file tree.
    pub async fn get_tree(&self, workspace_path: &str) -> Result<FileTreeNode, AppError> {
        let root = self.cached_canonicalize(workspace_path).await?;

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
        let root = self.cached_canonicalize(workspace_path).await?;
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

        let root = self.cached_canonicalize(workspace_path).await?;
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

        let root = self.cached_canonicalize(workspace_path).await?;
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

    /// Search workspace files using the shared in-process search engine.
    pub async fn search(
        &self,
        workspace_path: &str,
        query: &str,
        options: SearchOptions,
    ) -> Result<SearchResponse, AppError> {
        if query.is_empty() {
            return Err(AppError::recoverable(
                ErrorSource::Index,
                "index.search.empty_query",
                "Search query cannot be empty",
            ));
        }

        let workspace_root = self.cached_canonicalize(workspace_path).await?;
        let query_mode = options.query_mode;
        let request = build_search_request(&workspace_root, query, options);
        let outcome = run_local_search(request).await.map_err(|e| {
            AppError::recoverable(
                ErrorSource::Index,
                "index.search.failed",
                format!("local search failed: {e}"),
            )
        })?;

        Ok(search_response_from_outcome(query_mode, outcome))
    }

    pub async fn search_stream<F>(
        &self,
        workspace_path: &str,
        query: &str,
        options: SearchOptions,
        mut on_batch: F,
    ) -> Result<SearchResponse, AppError>
    where
        F: FnMut(SearchBatchResponse) -> Result<(), AppError>,
    {
        if query.is_empty() {
            return Err(AppError::recoverable(
                ErrorSource::Index,
                "index.search.empty_query",
                "Search query cannot be empty",
            ));
        }

        let workspace_root = self.cached_canonicalize(workspace_path).await?;
        let query_mode = options.query_mode;
        let request = build_search_request(&workspace_root, query, options);
        let mut stream = stream_local_search(request);

        while let Some(batch) = stream.receiver.recv().await {
            on_batch(search_batch_response_from_batch(query, batch))?;
        }

        let outcome = stream.finish().await.map_err(|e| {
            AppError::recoverable(
                ErrorSource::Index,
                "index.search.failed",
                format!("local search failed: {e}"),
            )
        })?;

        Ok(search_response_from_outcome(query_mode, outcome))
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

fn build_search_request(
    workspace_root: &Path,
    query: &str,
    options: SearchOptions,
) -> LocalSearchRequest {
    let limit = options.max_results.unwrap_or(50).max(1);
    let query_mode = options.query_mode;
    let output_mode = options.output_mode;

    LocalSearchRequest {
        workspace_root: workspace_root.to_path_buf(),
        search_root: workspace_root.to_path_buf(),
        query: query.to_string(),
        file_pattern: normalize_search_file_pattern(options.file_pattern.as_deref())
            .map(str::to_string),
        file_type: options.file_type,
        query_mode,
        output_mode,
        case_insensitive: options.case_insensitive,
        multiline: options.multiline,
        context_before: 0,
        context_after: 0,
        offset: 0,
        max_results: limit,
        timeout: options.timeout,
        cancellation: options.cancellation,
    }
}

fn search_result_from_match(search_match: crate::core::local_search::SearchMatch) -> SearchResult {
    SearchResult {
        path: search_match.path,
        absolute_path: search_match.absolute_path,
        line_number: search_match.line_number,
        end_line_number: search_match.end_line_number,
        line_text: search_match.line_text,
        match_text: search_match.match_text,
    }
}

fn search_file_from_match(file: LocalSearchFileMatch) -> SearchFileMatch {
    SearchFileMatch {
        path: file.path,
        absolute_path: file.absolute_path,
    }
}

fn search_file_count_from_match(file_count: LocalSearchFileCount) -> SearchFileCount {
    SearchFileCount {
        path: file_count.path,
        absolute_path: file_count.absolute_path,
        count: file_count.count,
    }
}

fn search_batch_response_from_batch(query: &str, batch: LocalSearchBatch) -> SearchBatchResponse {
    let total_count = match batch.output_mode {
        SearchOutputMode::Content => batch.total_matches,
        SearchOutputMode::FilesWithMatches | SearchOutputMode::Count => batch.total_files,
    };

    SearchBatchResponse {
        query: query.to_string(),
        output_mode: batch.output_mode.as_str().to_string(),
        results: batch
            .results
            .into_iter()
            .map(search_result_from_match)
            .collect(),
        files: batch
            .files
            .into_iter()
            .map(search_file_from_match)
            .collect(),
        file_counts: batch
            .file_counts
            .into_iter()
            .map(search_file_count_from_match)
            .collect(),
        count: batch.count,
        total_count,
        total_files: batch.total_files,
        searched_files: batch.searched_files,
    }
}

fn search_response_from_outcome(
    query_mode: SearchQueryMode,
    outcome: LocalSearchOutcome,
) -> SearchResponse {
    let total_count = match outcome.output_mode {
        SearchOutputMode::Content => outcome.total_matches,
        SearchOutputMode::FilesWithMatches | SearchOutputMode::Count => outcome.total_files,
    };

    SearchResponse {
        query: outcome.query,
        query_mode: match query_mode {
            SearchQueryMode::Literal => "literal".to_string(),
            SearchQueryMode::Regex => "regex".to_string(),
        },
        output_mode: outcome.output_mode.as_str().to_string(),
        results: outcome
            .results
            .into_iter()
            .map(search_result_from_match)
            .collect(),
        files: outcome
            .files
            .into_iter()
            .map(search_file_from_match)
            .collect(),
        file_counts: outcome
            .file_counts
            .into_iter()
            .map(search_file_count_from_match)
            .collect(),
        count: outcome.shown_count,
        total_count,
        total_files: outcome.total_files,
        completed: outcome.completed,
        cancelled: outcome.cancelled,
        timed_out: outcome.timed_out,
        partial: outcome.partial,
        elapsed_ms: outcome.elapsed_ms,
        searched_files: outcome.searched_files,
    }
}

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
                nodes.push(make_directory_placeholder(&entry.path, root));
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

fn make_directory_placeholder(path: &Path, root: &Path) -> FileTreeNode {
    FileTreeNode {
        name: node_name(path, root),
        path: relative_path(path, root),
        is_dir: true,
        is_expandable: true,
        children_has_more: false,
        children_next_offset: None,
        git_state: None,
        children: None,
    }
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
        .map(|value| value.to_string_lossy().replace('\\', "/"))
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
