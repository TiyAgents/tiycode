//! Workspace file tree cache and ripgrep text search.
//!
//! Phase 1 scope:
//! - In-memory file tree scan with configurable ignores
//! - Ripgrep subprocess for text search
//! Phase 2+ will add persistent index, FTS5, and semantic search.

use serde::Serialize;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use tokio::fs;
use tokio::process::Command;

use crate::model::errors::{AppError, ErrorSource};

/// Default patterns to ignore during tree scan.
const DEFAULT_IGNORES: &[&str] = &[
    ".git",
    "node_modules",
    ".next",
    "target",
    "dist",
    "build",
    ".cache",
    "__pycache__",
    ".DS_Store",
    "thumbs.db",
];

/// Max depth for tree scan to avoid runaway recursion.
const MAX_DEPTH: usize = 12;

/// Max entries returned in a single tree scan.
const MAX_ENTRIES: usize = 5000;

// ---------------------------------------------------------------------------
// File tree types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileTreeNode {
    pub name: String,
    pub path: String,
    pub is_dir: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub children: Option<Vec<FileTreeNode>>,
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

pub struct IndexManager;

impl IndexManager {
    pub fn new() -> Self {
        Self
    }

    /// Scan workspace directory and return a file tree.
    pub async fn get_tree(&self, workspace_path: &str) -> Result<FileTreeNode, AppError> {
        let root = PathBuf::from(workspace_path);
        if !root.is_dir() {
            return Err(AppError::recoverable(
                ErrorSource::Index,
                "index.path.not_directory",
                format!("'{}' is not a directory", workspace_path),
            ));
        }

        let ignores: HashSet<&str> = DEFAULT_IGNORES.iter().copied().collect();
        let mut entry_count = 0;

        let tree = scan_dir(&root, &root, &ignores, 0, &mut entry_count).await?;

        tracing::debug!(
            path = %workspace_path,
            entries = entry_count,
            "file tree scanned"
        );

        Ok(tree)
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
}

// ---------------------------------------------------------------------------
// Recursive directory scanner
// ---------------------------------------------------------------------------

async fn scan_dir(
    path: &Path,
    root: &Path,
    ignores: &HashSet<&str>,
    depth: usize,
    entry_count: &mut usize,
) -> Result<FileTreeNode, AppError> {
    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("")
        .to_string();

    let relative = path
        .strip_prefix(root)
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_default();

    if !path.is_dir() || depth > MAX_DEPTH || *entry_count >= MAX_ENTRIES {
        return Ok(FileTreeNode {
            name,
            path: relative,
            is_dir: path.is_dir(),
            children: None,
        });
    }

    let mut entries = fs::read_dir(path)
        .await
        .map_err(|e| AppError::internal(ErrorSource::Index, format!("Failed to read dir: {e}")))?;

    let mut children = Vec::new();
    let mut dirs = Vec::new();
    let mut files = Vec::new();

    while let Ok(Some(entry)) = entries.next_entry().await {
        if *entry_count >= MAX_ENTRIES {
            break;
        }

        let entry_name = entry.file_name().to_string_lossy().to_string();

        // Skip ignored patterns
        if ignores.contains(entry_name.as_str()) {
            continue;
        }
        // Skip hidden files/dirs (except at root level for things like .gitignore)
        if entry_name.starts_with('.') && depth > 0 {
            continue;
        }

        let entry_path = entry.path();
        let is_dir = entry.file_type().await.map(|t| t.is_dir()).unwrap_or(false);

        *entry_count += 1;

        if is_dir {
            dirs.push((entry_name, entry_path));
        } else {
            let rel = entry_path
                .strip_prefix(root)
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_default();
            files.push(FileTreeNode {
                name: entry_name,
                path: rel,
                is_dir: false,
                children: None,
            });
        }
    }

    // Sort: directories first (alphabetical), then files (alphabetical)
    dirs.sort_by(|a, b| a.0.cmp(&b.0));
    files.sort_by(|a, b| a.name.cmp(&b.name));

    // Recurse into directories
    for (dir_name, dir_path) in dirs {
        let child = Box::pin(scan_dir(&dir_path, root, ignores, depth + 1, entry_count)).await?;
        children.push(child);
    }

    children.extend(files);

    Ok(FileTreeNode {
        name,
        path: relative,
        is_dir: true,
        children: Some(children),
    })
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
