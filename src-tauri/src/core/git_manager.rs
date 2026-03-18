use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use chrono::Utc;
use git2::{
    BranchType, Delta, Diff, DiffFindOptions, DiffOptions, Patch, Repository, Sort, Status,
    StatusOptions,
};
use tokio::sync::{broadcast, Mutex};

use crate::ipc::frontend_channels::GitStreamEvent;
use crate::model::errors::{AppError, ErrorSource};
use crate::model::git::{
    GitChangeKind, GitCommitSummaryDto, GitDiffDto, GitDiffHunkDto, GitDiffLineDto,
    GitDiffLineKind, GitFileChangeDto, GitFileState, GitFileStatusDto, GitRepoCapabilitiesDto,
    GitSnapshotDto,
};

const DEFAULT_HISTORY_LIMIT: usize = 24;
const MAX_DIFF_LINES: usize = 1200;
const GIT_STREAM_BUFFER: usize = 32;

#[derive(Debug, Clone)]
pub struct WorkspaceGitOverlay {
    pub repo_available: bool,
    pub states: HashMap<String, GitFileState>,
}

#[derive(Debug, Clone)]
struct SnapshotParts {
    repo_root: PathBuf,
    head_ref: Option<String>,
    head_oid: Option<String>,
    is_detached: bool,
    ahead_count: u32,
    behind_count: u32,
    staged_files: Vec<GitFileChangeDto>,
    unstaged_files: Vec<GitFileChangeDto>,
    untracked_files: Vec<GitFileChangeDto>,
    recent_commits: Vec<GitCommitSummaryDto>,
}

#[derive(Clone)]
pub struct GitManager {
    streams: Arc<Mutex<HashMap<String, broadcast::Sender<GitStreamEvent>>>>,
}

impl GitManager {
    pub fn new() -> Self {
        Self {
            streams: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub async fn subscribe(&self, workspace_id: &str) -> broadcast::Receiver<GitStreamEvent> {
        let sender = self.get_or_create_sender(workspace_id).await;
        sender.subscribe()
    }

    pub async fn refresh(
        &self,
        workspace_id: &str,
        workspace_path: &str,
    ) -> Result<GitSnapshotDto, AppError> {
        let sender = self.get_or_create_sender(workspace_id).await;
        let _ = sender.send(GitStreamEvent::RefreshStarted {
            workspace_id: workspace_id.to_string(),
        });

        let snapshot = self.get_snapshot(workspace_id, workspace_path).await?;
        let _ = sender.send(GitStreamEvent::SnapshotUpdated {
            workspace_id: workspace_id.to_string(),
            snapshot: snapshot.clone(),
        });
        let _ = sender.send(GitStreamEvent::RefreshCompleted {
            workspace_id: workspace_id.to_string(),
        });

        Ok(snapshot)
    }

    pub async fn get_workspace_overlay(
        &self,
        workspace_path: &str,
    ) -> Result<WorkspaceGitOverlay, AppError> {
        let workspace_root = canonicalize_workspace(workspace_path);

        tokio::task::spawn_blocking(move || collect_workspace_overlay(&workspace_root))
            .await
            .map_err(|error| {
                AppError::internal(
                    ErrorSource::Git,
                    format!("Git overlay task failed: {error}"),
                )
            })?
    }

    pub async fn get_snapshot(
        &self,
        workspace_id: &str,
        workspace_path: &str,
    ) -> Result<GitSnapshotDto, AppError> {
        let workspace_root = canonicalize_workspace(workspace_path);
        let workspace_id = workspace_id.to_string();

        tokio::task::spawn_blocking(move || {
            let capabilities = GitRepoCapabilitiesDto {
                repo_available: is_repo_available(&workspace_root)?,
                git_cli_available: is_git_cli_available(),
            };

            if !capabilities.repo_available {
                return Ok(empty_snapshot(workspace_id, capabilities));
            }

            let snapshot = build_snapshot(&workspace_id, &workspace_root, capabilities)?;
            Ok(snapshot)
        })
        .await
        .map_err(|error| {
            AppError::internal(
                ErrorSource::Git,
                format!("Git snapshot task failed: {error}"),
            )
        })?
    }

    pub async fn get_history(
        &self,
        workspace_path: &str,
        limit: Option<usize>,
    ) -> Result<Vec<GitCommitSummaryDto>, AppError> {
        let workspace_root = canonicalize_workspace(workspace_path);
        let history_limit = limit.unwrap_or(DEFAULT_HISTORY_LIMIT);

        tokio::task::spawn_blocking(move || {
            let repo = open_repository(&workspace_root)?;
            collect_history(&repo, history_limit)
        })
        .await
        .map_err(|error| {
            AppError::internal(
                ErrorSource::Git,
                format!("Git history task failed: {error}"),
            )
        })?
    }

    pub async fn get_diff(
        &self,
        workspace_path: &str,
        path: &str,
        staged: bool,
    ) -> Result<GitDiffDto, AppError> {
        let workspace_root = canonicalize_workspace(workspace_path);
        let workspace_relative = normalize_workspace_relative_path(path);

        tokio::task::spawn_blocking(move || {
            let repo = open_repository(&workspace_root)?;
            let repo_root = repo_workdir(&repo)?;
            let repo_relative =
                workspace_path_to_repo_path(&repo_root, &workspace_root, &workspace_relative)?;
            let mut diff = diff_for_path(&repo, &repo_relative, staged)?;
            build_diff_payload(&mut diff, &repo_root, &workspace_root, staged)
        })
        .await
        .map_err(|error| {
            AppError::internal(ErrorSource::Git, format!("Git diff task failed: {error}"))
        })?
    }

    pub async fn get_file_status(
        &self,
        workspace_path: &str,
        path: &str,
    ) -> Result<GitFileStatusDto, AppError> {
        let workspace_root = canonicalize_workspace(workspace_path);
        let workspace_relative = normalize_workspace_relative_path(path);

        tokio::task::spawn_blocking(move || {
            let repo = open_repository(&workspace_root)?;
            let repo_root = repo_workdir(&repo)?;
            let repo_relative =
                workspace_path_to_repo_path(&repo_root, &workspace_root, &workspace_relative)?;
            let status = repo
                .status_file(Path::new(&repo_relative))
                .map_err(|error| {
                    AppError::recoverable(
                        ErrorSource::Git,
                        "git.status.file_failed",
                        format!(
                            "Unable to inspect Git status for '{}': {error}",
                            workspace_relative
                        ),
                    )
                })?;

            Ok(GitFileStatusDto {
                path: workspace_relative,
                staged_status: map_index_status(status),
                unstaged_status: map_worktree_status(status),
                is_untracked: status.is_wt_new(),
                is_ignored: status.is_ignored(),
            })
        })
        .await
        .map_err(|error| {
            AppError::internal(
                ErrorSource::Git,
                format!("Git file status task failed: {error}"),
            )
        })?
    }

    pub async fn stage(
        &self,
        workspace_id: &str,
        workspace_path: &str,
        paths: &[String],
    ) -> Result<GitSnapshotDto, AppError> {
        let workspace_root = canonicalize_workspace(workspace_path);
        let normalized_paths = normalize_workspace_relative_paths(paths)?;

        tokio::task::spawn_blocking(move || stage_paths(&workspace_root, &normalized_paths))
            .await
            .map_err(|error| {
                AppError::internal(ErrorSource::Git, format!("Git stage task failed: {error}"))
            })??;

        self.refresh(workspace_id, workspace_path).await
    }

    pub async fn unstage(
        &self,
        workspace_id: &str,
        workspace_path: &str,
        paths: &[String],
    ) -> Result<GitSnapshotDto, AppError> {
        let workspace_root = canonicalize_workspace(workspace_path);
        let normalized_paths = normalize_workspace_relative_paths(paths)?;

        tokio::task::spawn_blocking(move || unstage_paths(&workspace_root, &normalized_paths))
            .await
            .map_err(|error| {
                AppError::internal(
                    ErrorSource::Git,
                    format!("Git unstage task failed: {error}"),
                )
            })??;

        self.refresh(workspace_id, workspace_path).await
    }

    async fn get_or_create_sender(&self, workspace_id: &str) -> broadcast::Sender<GitStreamEvent> {
        let mut streams = self.streams.lock().await;

        if let Some(sender) = streams.get(workspace_id) {
            return sender.clone();
        }

        let (sender, _) = broadcast::channel(GIT_STREAM_BUFFER);
        streams.insert(workspace_id.to_string(), sender.clone());
        sender
    }
}

fn build_snapshot(
    workspace_id: &str,
    workspace_root: &Path,
    capabilities: GitRepoCapabilitiesDto,
) -> Result<GitSnapshotDto, AppError> {
    let parts = collect_snapshot_parts(workspace_root, DEFAULT_HISTORY_LIMIT)?;

    Ok(GitSnapshotDto {
        workspace_id: workspace_id.to_string(),
        repo_root: Some(parts.repo_root.to_string_lossy().to_string()),
        capabilities,
        head_ref: parts.head_ref,
        head_oid: parts.head_oid,
        is_detached: parts.is_detached,
        ahead_count: parts.ahead_count,
        behind_count: parts.behind_count,
        staged_files: parts.staged_files,
        unstaged_files: parts.unstaged_files,
        untracked_files: parts.untracked_files,
        recent_commits: parts.recent_commits,
        last_refreshed_at: Utc::now().to_rfc3339(),
    })
}

fn empty_snapshot(workspace_id: String, capabilities: GitRepoCapabilitiesDto) -> GitSnapshotDto {
    GitSnapshotDto {
        workspace_id,
        repo_root: None,
        capabilities,
        head_ref: None,
        head_oid: None,
        is_detached: false,
        ahead_count: 0,
        behind_count: 0,
        staged_files: Vec::new(),
        unstaged_files: Vec::new(),
        untracked_files: Vec::new(),
        recent_commits: Vec::new(),
        last_refreshed_at: Utc::now().to_rfc3339(),
    }
}

fn collect_snapshot_parts(
    workspace_root: &Path,
    history_limit: usize,
) -> Result<SnapshotParts, AppError> {
    let repo = open_repository(workspace_root)?;
    let repo_root = repo_workdir(&repo)?;
    let head = repo.head().ok();
    let head_ref = head
        .as_ref()
        .and_then(|reference| reference.shorthand())
        .map(str::to_string);
    let head_oid = head
        .as_ref()
        .and_then(|reference| reference.target())
        .map(|oid| oid.to_string());
    let is_detached = repo.head_detached().unwrap_or(false);
    let (ahead_count, behind_count) = collect_ahead_behind(&repo, head.as_ref())?;
    let staged_files = collect_staged_files(&repo, &repo_root, workspace_root)?;
    let unstaged_files = collect_unstaged_files(&repo, &repo_root, workspace_root)?;
    let untracked_files = collect_untracked_files(&repo, &repo_root, workspace_root)?;
    let recent_commits = collect_history(&repo, history_limit)?;

    Ok(SnapshotParts {
        repo_root,
        head_ref,
        head_oid,
        is_detached,
        ahead_count,
        behind_count,
        staged_files,
        unstaged_files,
        untracked_files,
        recent_commits,
    })
}

fn collect_workspace_overlay(workspace_root: &Path) -> Result<WorkspaceGitOverlay, AppError> {
    let repo = match Repository::discover(workspace_root) {
        Ok(repo) => repo,
        Err(error) if error.code() == git2::ErrorCode::NotFound => {
            return Ok(WorkspaceGitOverlay {
                repo_available: false,
                states: HashMap::new(),
            });
        }
        Err(error) => {
            return Err(AppError::recoverable(
                ErrorSource::Git,
                "git.repo.inaccessible",
                format!("Unable to read Git repository: {error}"),
            ));
        }
    };

    let repo_root = repo_workdir(&repo)?;

    let mut options = StatusOptions::new();
    options
        .include_untracked(true)
        .include_ignored(true)
        .include_unmodified(true)
        .recurse_untracked_dirs(true)
        .recurse_ignored_dirs(true);

    let statuses = repo.statuses(Some(&mut options)).map_err(|error| {
        AppError::recoverable(
            ErrorSource::Git,
            "git.status.read_failed",
            format!("Unable to read Git status: {error}"),
        )
    })?;

    let mut states = HashMap::new();

    for entry in statuses.iter() {
        let Some(repo_relative_path) = entry.path() else {
            continue;
        };

        let absolute_path = repo_root.join(repo_relative_path);
        let Ok(workspace_relative_path) = absolute_path.strip_prefix(workspace_root) else {
            continue;
        };

        let key = workspace_relative_path.to_string_lossy().to_string();
        let state = map_overlay_status(entry.status());

        merge_state_with_ancestors(&mut states, &key, state);
    }

    let index = repo.index().map_err(|error| {
        AppError::recoverable(
            ErrorSource::Git,
            "git.index.read_failed",
            format!("Unable to read Git index: {error}"),
        )
    })?;

    for entry in index.iter() {
        let repo_relative_path = String::from_utf8_lossy(entry.path.as_ref()).to_string();
        let absolute_path = repo_root.join(&repo_relative_path);
        let Ok(workspace_relative_path) = absolute_path.strip_prefix(workspace_root) else {
            continue;
        };

        let key = workspace_relative_path.to_string_lossy().to_string();
        merge_state_with_ancestors(&mut states, &key, GitFileState::Tracked);
    }

    Ok(WorkspaceGitOverlay {
        repo_available: true,
        states,
    })
}

fn collect_ahead_behind(
    repo: &Repository,
    head: Option<&git2::Reference<'_>>,
) -> Result<(u32, u32), AppError> {
    let Some(head) = head else {
        return Ok((0, 0));
    };

    if !head.is_branch() {
        return Ok((0, 0));
    }

    let Some(branch_name) = head.shorthand() else {
        return Ok((0, 0));
    };

    let branch = match repo.find_branch(branch_name, BranchType::Local) {
        Ok(branch) => branch,
        Err(_) => return Ok((0, 0)),
    };
    let upstream = match branch.upstream() {
        Ok(upstream) => upstream,
        Err(_) => return Ok((0, 0)),
    };
    let Some(local_oid) = branch.get().target() else {
        return Ok((0, 0));
    };
    let Some(upstream_oid) = upstream.get().target() else {
        return Ok((0, 0));
    };

    repo.graph_ahead_behind(local_oid, upstream_oid)
        .map(|(ahead, behind)| (ahead as u32, behind as u32))
        .map_err(|error| {
            AppError::recoverable(
                ErrorSource::Git,
                "git.status.ahead_behind_failed",
                format!("Unable to calculate upstream divergence: {error}"),
            )
        })
}

fn collect_staged_files(
    repo: &Repository,
    repo_root: &Path,
    workspace_root: &Path,
) -> Result<Vec<GitFileChangeDto>, AppError> {
    let head_tree = repo.head().ok().and_then(|head| head.peel_to_tree().ok());
    let index = repo.index().map_err(|error| {
        AppError::recoverable(
            ErrorSource::Git,
            "git.index.read_failed",
            format!("Unable to read Git index: {error}"),
        )
    })?;
    let mut diff_options = DiffOptions::new();
    let mut diff = repo
        .diff_tree_to_index(head_tree.as_ref(), Some(&index), Some(&mut diff_options))
        .map_err(|error| {
            AppError::recoverable(
                ErrorSource::Git,
                "git.diff.staged_failed",
                format!("Unable to build staged diff: {error}"),
            )
        })?;

    collect_file_changes_from_diff(&mut diff, repo_root, workspace_root)
}

fn collect_unstaged_files(
    repo: &Repository,
    repo_root: &Path,
    workspace_root: &Path,
) -> Result<Vec<GitFileChangeDto>, AppError> {
    let index = repo.index().map_err(|error| {
        AppError::recoverable(
            ErrorSource::Git,
            "git.index.read_failed",
            format!("Unable to read Git index: {error}"),
        )
    })?;
    let mut diff_options = DiffOptions::new();
    diff_options
        .include_untracked(false)
        .recurse_untracked_dirs(true);

    let mut diff = repo
        .diff_index_to_workdir(Some(&index), Some(&mut diff_options))
        .map_err(|error| {
            AppError::recoverable(
                ErrorSource::Git,
                "git.diff.unstaged_failed",
                format!("Unable to build unstaged diff: {error}"),
            )
        })?;

    collect_file_changes_from_diff(&mut diff, repo_root, workspace_root)
}

fn collect_untracked_files(
    repo: &Repository,
    repo_root: &Path,
    workspace_root: &Path,
) -> Result<Vec<GitFileChangeDto>, AppError> {
    let mut options = StatusOptions::new();
    options
        .include_untracked(true)
        .include_ignored(false)
        .include_unmodified(false)
        .recurse_untracked_dirs(true);

    let statuses = repo.statuses(Some(&mut options)).map_err(|error| {
        AppError::recoverable(
            ErrorSource::Git,
            "git.status.read_failed",
            format!("Unable to read Git status: {error}"),
        )
    })?;

    let mut files = Vec::new();

    for entry in statuses.iter() {
        if !entry.status().is_wt_new() {
            continue;
        }

        let Some(path) = entry.path() else {
            continue;
        };

        let Some(workspace_relative) =
            repo_path_to_workspace_path(repo_root, workspace_root, Path::new(path))
        else {
            continue;
        };

        files.push(GitFileChangeDto {
            path: workspace_relative,
            previous_path: None,
            status: GitChangeKind::Added,
            additions: 0,
            deletions: 0,
        });
    }

    sort_file_changes(&mut files);
    Ok(files)
}

fn collect_file_changes_from_diff(
    diff: &mut Diff<'_>,
    repo_root: &Path,
    workspace_root: &Path,
) -> Result<Vec<GitFileChangeDto>, AppError> {
    let mut find_options = DiffFindOptions::new();
    find_options.renames(true);
    let _ = diff.find_similar(Some(&mut find_options));

    let mut files = Vec::new();

    for (index, delta) in diff.deltas().enumerate() {
        let Some(path) = delta
            .new_file()
            .path()
            .or_else(|| delta.old_file().path())
            .and_then(|repo_relative| {
                repo_path_to_workspace_path(repo_root, workspace_root, repo_relative)
            })
        else {
            continue;
        };

        let previous_path = delta.old_file().path().and_then(|repo_relative| {
            repo_path_to_workspace_path(repo_root, workspace_root, repo_relative)
        });

        let (additions, deletions) = patch_line_stats(diff, index)?;

        files.push(GitFileChangeDto {
            path,
            previous_path,
            status: map_delta_status(delta.status()),
            additions,
            deletions,
        });
    }

    sort_file_changes(&mut files);
    Ok(files)
}

fn collect_history(repo: &Repository, limit: usize) -> Result<Vec<GitCommitSummaryDto>, AppError> {
    let ref_map = build_ref_map(repo)?;
    let head_oid = repo.head().ok().and_then(|head| head.target());
    let mut revwalk = repo.revwalk().map_err(|error| {
        AppError::recoverable(
            ErrorSource::Git,
            "git.history.revwalk_failed",
            format!("Unable to start commit history walk: {error}"),
        )
    })?;

    revwalk.push_head().map_err(|error| {
        AppError::recoverable(
            ErrorSource::Git,
            "git.history.head_failed",
            format!("Unable to walk HEAD history: {error}"),
        )
    })?;
    let _ = revwalk.set_sorting(Sort::TIME | Sort::TOPOLOGICAL);

    let mut commits = Vec::new();

    for oid_result in revwalk.take(limit) {
        let oid = oid_result.map_err(|error| {
            AppError::recoverable(
                ErrorSource::Git,
                "git.history.iteration_failed",
                format!("Unable to iterate commit history: {error}"),
            )
        })?;
        let commit = repo.find_commit(oid).map_err(|error| {
            AppError::recoverable(
                ErrorSource::Git,
                "git.history.commit_failed",
                format!("Unable to load commit '{oid}': {error}"),
            )
        })?;
        let committed_at = chrono::DateTime::<Utc>::from_timestamp(commit.time().seconds(), 0)
            .unwrap_or_else(Utc::now)
            .to_rfc3339();

        commits.push(GitCommitSummaryDto {
            id: oid.to_string(),
            short_id: short_oid(&oid.to_string()),
            summary: commit.summary().unwrap_or("No commit message").to_string(),
            author_name: commit.author().name().unwrap_or("Unknown").to_string(),
            committed_at,
            refs: ref_map.get(&oid).cloned().unwrap_or_default(),
            is_head: head_oid == Some(oid),
        });
    }

    Ok(commits)
}

fn build_ref_map(repo: &Repository) -> Result<HashMap<git2::Oid, Vec<String>>, AppError> {
    let mut ref_map: HashMap<git2::Oid, Vec<String>> = HashMap::new();

    let references = repo.references().map_err(|error| {
        AppError::recoverable(
            ErrorSource::Git,
            "git.history.refs_failed",
            format!("Unable to inspect Git references: {error}"),
        )
    })?;

    for reference in references.flatten() {
        let Some(target) = reference.target() else {
            continue;
        };
        let Some(name) = reference.shorthand() else {
            continue;
        };

        ref_map.entry(target).or_default().push(name.to_string());
    }

    if let Ok(head) = repo.head() {
        if let Some(target) = head.target() {
            ref_map.entry(target).or_default().push("HEAD".to_string());
        }
    }

    for refs in ref_map.values_mut() {
        refs.sort();
        refs.dedup();
    }

    Ok(ref_map)
}

fn diff_for_path<'repo>(
    repo: &'repo Repository,
    repo_relative_path: &str,
    staged: bool,
) -> Result<Diff<'repo>, AppError> {
    let mut diff_options = DiffOptions::new();
    diff_options
        .pathspec(repo_relative_path)
        .recurse_untracked_dirs(true)
        .include_untracked(!staged);

    if staged {
        let head_tree = repo.head().ok().and_then(|head| head.peel_to_tree().ok());
        let index = repo.index().map_err(|error| {
            AppError::recoverable(
                ErrorSource::Git,
                "git.index.read_failed",
                format!("Unable to read Git index: {error}"),
            )
        })?;

        repo.diff_tree_to_index(head_tree.as_ref(), Some(&index), Some(&mut diff_options))
            .map_err(|error| {
                AppError::recoverable(
                    ErrorSource::Git,
                    "git.diff.read_failed",
                    format!("Unable to read staged diff for '{repo_relative_path}': {error}"),
                )
            })
    } else {
        let index = repo.index().map_err(|error| {
            AppError::recoverable(
                ErrorSource::Git,
                "git.index.read_failed",
                format!("Unable to read Git index: {error}"),
            )
        })?;

        repo.diff_index_to_workdir(Some(&index), Some(&mut diff_options))
            .map_err(|error| {
                AppError::recoverable(
                    ErrorSource::Git,
                    "git.diff.read_failed",
                    format!("Unable to read working tree diff for '{repo_relative_path}': {error}"),
                )
            })
    }
}

fn build_diff_payload(
    diff: &mut Diff<'_>,
    repo_root: &Path,
    workspace_root: &Path,
    staged: bool,
) -> Result<GitDiffDto, AppError> {
    let mut find_options = DiffFindOptions::new();
    find_options.renames(true);
    let _ = diff.find_similar(Some(&mut find_options));

    let Some((index, delta)) = diff.deltas().enumerate().next() else {
        return Err(AppError::recoverable(
            ErrorSource::Git,
            "git.diff.not_found",
            "No diff content is available for this file",
        ));
    };

    let path = delta
        .new_file()
        .path()
        .or_else(|| delta.old_file().path())
        .and_then(|repo_relative| {
            repo_path_to_workspace_path(repo_root, workspace_root, repo_relative)
        })
        .ok_or_else(|| {
            AppError::recoverable(
                ErrorSource::Git,
                "git.diff.path_unavailable",
                "Diff target is outside the current workspace",
            )
        })?;

    let old_path = delta.old_file().path().and_then(|repo_relative| {
        repo_path_to_workspace_path(repo_root, workspace_root, repo_relative)
    });
    let new_path = delta.new_file().path().and_then(|repo_relative| {
        repo_path_to_workspace_path(repo_root, workspace_root, repo_relative)
    });
    let (additions, deletions) = patch_line_stats(diff, index)?;
    let patch = Patch::from_diff(diff, index).map_err(|error| {
        AppError::recoverable(
            ErrorSource::Git,
            "git.diff.patch_failed",
            format!("Unable to build patch view for '{path}': {error}"),
        )
    })?;
    let Some(patch) = patch else {
        return Ok(GitDiffDto {
            path,
            staged,
            status: map_delta_status(delta.status()),
            old_path,
            new_path,
            additions,
            deletions,
            is_binary: true,
            truncated: false,
            hunks: Vec::new(),
        });
    };

    if patch.num_hunks() == 0 && map_delta_status(delta.status()) == GitChangeKind::Added {
        let synthetic_hunks = build_added_file_fallback_hunks(workspace_root, &path);
        if !synthetic_hunks.is_empty() {
            let additions = if additions == 0 {
                synthetic_hunks
                    .iter()
                    .map(|hunk| hunk.lines.len() as u32)
                    .sum::<u32>()
            } else {
                additions
            };

            return Ok(GitDiffDto {
                path,
                staged,
                status: map_delta_status(delta.status()),
                old_path,
                new_path,
                additions,
                deletions,
                is_binary: false,
                truncated: false,
                hunks: synthetic_hunks,
            });
        }
    }

    let mut line_budget = 0usize;
    let mut truncated = false;
    let mut hunks = Vec::new();

    for hunk_index in 0..patch.num_hunks() {
        let (hunk, line_count) = patch.hunk(hunk_index).map_err(|error| {
            AppError::recoverable(
                ErrorSource::Git,
                "git.diff.hunk_failed",
                format!("Unable to read diff hunk for '{path}': {error}"),
            )
        })?;

        let mut lines = Vec::new();

        for line_index in 0..line_count {
            if line_budget >= MAX_DIFF_LINES {
                truncated = true;
                break;
            }

            let line = patch
                .line_in_hunk(hunk_index, line_index)
                .map_err(|error| {
                    AppError::recoverable(
                        ErrorSource::Git,
                        "git.diff.line_failed",
                        format!("Unable to read diff line for '{path}': {error}"),
                    )
                })?;

            let kind = match line.origin() {
                '+' => GitDiffLineKind::Add,
                '-' => GitDiffLineKind::Remove,
                _ => GitDiffLineKind::Context,
            };

            lines.push(GitDiffLineDto {
                kind,
                old_number: line.old_lineno(),
                new_number: line.new_lineno(),
                text: trim_patch_line(line.content()),
            });
            line_budget += 1;
        }

        hunks.push(GitDiffHunkDto {
            header: trim_patch_line(hunk.header()),
            lines,
        });

        if truncated {
            break;
        }
    }

    Ok(GitDiffDto {
        path,
        staged,
        status: map_delta_status(delta.status()),
        old_path,
        new_path,
        additions,
        deletions,
        is_binary: false,
        truncated,
        hunks,
    })
}

fn patch_line_stats(diff: &Diff<'_>, index: usize) -> Result<(u32, u32), AppError> {
    let patch = Patch::from_diff(diff, index).map_err(|error| {
        AppError::recoverable(
            ErrorSource::Git,
            "git.diff.patch_failed",
            format!("Unable to inspect diff stats: {error}"),
        )
    })?;
    let Some(patch) = patch else {
        return Ok((0, 0));
    };
    let (_, additions, deletions) = patch.line_stats().map_err(|error| {
        AppError::recoverable(
            ErrorSource::Git,
            "git.diff.stats_failed",
            format!("Unable to calculate diff stats: {error}"),
        )
    })?;

    Ok((additions as u32, deletions as u32))
}

fn open_repository(workspace_root: &Path) -> Result<Repository, AppError> {
    Repository::discover(workspace_root).map_err(|error| {
        if error.code() == git2::ErrorCode::NotFound {
            AppError::recoverable(
                ErrorSource::Git,
                "git.repo.not_found",
                "The current workspace is not inside a Git repository",
            )
        } else {
            AppError::recoverable(
                ErrorSource::Git,
                "git.repo.inaccessible",
                format!("Unable to read Git repository: {error}"),
            )
        }
    })
}

fn stage_paths(workspace_root: &Path, workspace_paths: &[String]) -> Result<(), AppError> {
    let repo = open_repository(workspace_root)?;
    let repo_root = repo_workdir(&repo)?;
    let mut index = repo.index().map_err(|error| {
        AppError::recoverable(
            ErrorSource::Git,
            "git.index.read_failed",
            format!("Unable to read Git index: {error}"),
        )
    })?;

    for workspace_relative in workspace_paths {
        let repo_relative =
            workspace_path_to_repo_path(&repo_root, workspace_root, workspace_relative)?;
        index.add_path(Path::new(&repo_relative)).map_err(|error| {
            AppError::recoverable(
                ErrorSource::Git,
                "git.stage.failed",
                format!("Unable to stage '{}': {error}", workspace_relative),
            )
        })?;
    }

    index.write().map_err(|error| {
        AppError::recoverable(
            ErrorSource::Git,
            "git.index.write_failed",
            format!("Unable to write staged changes: {error}"),
        )
    })
}

fn unstage_paths(workspace_root: &Path, workspace_paths: &[String]) -> Result<(), AppError> {
    let repo = open_repository(workspace_root)?;
    let repo_root = repo_workdir(&repo)?;
    let repo_relative_paths = workspace_paths
        .iter()
        .map(|path| workspace_path_to_repo_path(&repo_root, workspace_root, path))
        .collect::<Result<Vec<_>, _>>()?;
    let head_commit = repo.head().ok().and_then(|head| head.peel_to_commit().ok());
    let head_object = head_commit.as_ref().map(|commit| commit.as_object());

    repo.reset_default(head_object, repo_relative_paths.iter())
        .map_err(|error| {
            AppError::recoverable(
                ErrorSource::Git,
                "git.unstage.failed",
                format!("Unable to unstage selected files: {error}"),
            )
        })
}

fn repo_workdir(repo: &Repository) -> Result<PathBuf, AppError> {
    repo.workdir()
        .map(|path| std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf()))
        .ok_or_else(|| {
            AppError::recoverable(
                ErrorSource::Git,
                "git.repo.workdir_missing",
                "Bare repositories are not supported in the workspace Git drawer",
            )
        })
}

fn canonicalize_workspace(workspace_path: &str) -> PathBuf {
    std::fs::canonicalize(workspace_path).unwrap_or_else(|_| PathBuf::from(workspace_path))
}

fn workspace_path_to_repo_path(
    repo_root: &Path,
    workspace_root: &Path,
    workspace_relative_path: &str,
) -> Result<String, AppError> {
    let normalized = workspace_relative_path.trim().trim_matches('/');
    if normalized.is_empty() {
        return Err(AppError::recoverable(
            ErrorSource::Git,
            "git.path.empty",
            "Git path cannot be empty",
        ));
    }

    let absolute_path = workspace_root.join(normalized);
    let repo_relative = absolute_path.strip_prefix(repo_root).map_err(|_| {
        AppError::recoverable(
            ErrorSource::Git,
            "git.path.out_of_workspace",
            "The requested Git path is outside the repository root",
        )
    })?;

    Ok(repo_relative.to_string_lossy().to_string())
}

fn repo_path_to_workspace_path(
    repo_root: &Path,
    workspace_root: &Path,
    repo_relative_path: &Path,
) -> Option<String> {
    let absolute = repo_root.join(repo_relative_path);
    let workspace_relative = absolute.strip_prefix(workspace_root).ok()?;
    Some(workspace_relative.to_string_lossy().to_string())
}

fn normalize_workspace_relative_path(path: &str) -> String {
    path.trim().trim_matches('/').to_string()
}

fn normalize_workspace_relative_paths(paths: &[String]) -> Result<Vec<String>, AppError> {
    let normalized = paths
        .iter()
        .map(|path| normalize_workspace_relative_path(path))
        .filter(|path| !path.is_empty())
        .collect::<Vec<_>>();

    if normalized.is_empty() {
        return Err(AppError::recoverable(
            ErrorSource::Git,
            "git.path.empty",
            "At least one Git path is required",
        ));
    }

    Ok(normalized)
}

fn is_repo_available(workspace_root: &Path) -> Result<bool, AppError> {
    match Repository::discover(workspace_root) {
        Ok(_) => Ok(true),
        Err(error) if error.code() == git2::ErrorCode::NotFound => Ok(false),
        Err(error) => Err(AppError::recoverable(
            ErrorSource::Git,
            "git.repo.inaccessible",
            format!("Unable to read Git repository: {error}"),
        )),
    }
}

fn is_git_cli_available() -> bool {
    std::process::Command::new("git")
        .arg("--version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

fn map_overlay_status(status: Status) -> GitFileState {
    if status.is_ignored() {
        GitFileState::Ignored
    } else if status.is_wt_new() {
        GitFileState::Untracked
    } else if status.intersects(
        Status::WT_MODIFIED
            | Status::WT_DELETED
            | Status::WT_RENAMED
            | Status::WT_TYPECHANGE
            | Status::INDEX_MODIFIED
            | Status::INDEX_NEW
            | Status::INDEX_DELETED
            | Status::INDEX_RENAMED
            | Status::INDEX_TYPECHANGE,
    ) {
        GitFileState::Modified
    } else {
        GitFileState::Tracked
    }
}

fn map_delta_status(status: Delta) -> GitChangeKind {
    match status {
        Delta::Added => GitChangeKind::Added,
        Delta::Deleted => GitChangeKind::Deleted,
        Delta::Renamed => GitChangeKind::Renamed,
        Delta::Typechange => GitChangeKind::Typechange,
        Delta::Untracked => GitChangeKind::Added,
        Delta::Unreadable | Delta::Modified | Delta::Copied => GitChangeKind::Modified,
        Delta::Conflicted => GitChangeKind::Unmerged,
        _ => GitChangeKind::Modified,
    }
}

fn map_index_status(status: Status) -> Option<GitChangeKind> {
    if status.is_index_new() {
        Some(GitChangeKind::Added)
    } else if status.is_index_deleted() {
        Some(GitChangeKind::Deleted)
    } else if status.is_index_renamed() {
        Some(GitChangeKind::Renamed)
    } else if status.is_index_typechange() {
        Some(GitChangeKind::Typechange)
    } else if status.is_index_modified() {
        Some(GitChangeKind::Modified)
    } else {
        None
    }
}

fn map_worktree_status(status: Status) -> Option<GitChangeKind> {
    if status.is_wt_new() {
        Some(GitChangeKind::Added)
    } else if status.is_wt_deleted() {
        Some(GitChangeKind::Deleted)
    } else if status.is_wt_renamed() {
        Some(GitChangeKind::Renamed)
    } else if status.is_wt_typechange() {
        Some(GitChangeKind::Typechange)
    } else if status.is_wt_modified() {
        Some(GitChangeKind::Modified)
    } else {
        None
    }
}

fn trim_patch_line(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes)
        .trim_end_matches('\n')
        .trim_end_matches('\r')
        .to_string()
}

fn build_added_file_fallback_hunks(
    workspace_root: &Path,
    workspace_relative_path: &str,
) -> Vec<GitDiffHunkDto> {
    let absolute_path = workspace_root.join(workspace_relative_path);
    let Ok(contents) = std::fs::read_to_string(absolute_path) else {
        return Vec::new();
    };

    let lines: Vec<GitDiffLineDto> = contents
        .lines()
        .enumerate()
        .map(|(index, text)| GitDiffLineDto {
            kind: GitDiffLineKind::Add,
            old_number: None,
            new_number: Some((index + 1) as u32),
            text: text.to_string(),
        })
        .collect();

    if lines.is_empty() {
        return Vec::new();
    }

    vec![GitDiffHunkDto {
        header: format!("@@ -0,0 +1,{} @@", lines.len()),
        lines,
    }]
}

fn short_oid(oid: &str) -> String {
    oid.chars().take(7).collect()
}

fn sort_file_changes(files: &mut [GitFileChangeDto]) {
    files.sort_by(|left, right| left.path.cmp(&right.path));
}

fn merge_state(states: &mut HashMap<String, GitFileState>, path: String, next: GitFileState) {
    match states.get(&path) {
        Some(current) if state_priority(current) >= state_priority(&next) => {}
        _ => {
            states.insert(path, next);
        }
    }
}

fn merge_state_with_ancestors(
    states: &mut HashMap<String, GitFileState>,
    path: &str,
    next: GitFileState,
) {
    merge_state(states, path.to_string(), next);

    let mut ancestor = PathBuf::from(path);
    while ancestor.pop() {
        if ancestor.as_os_str().is_empty() {
            break;
        }

        merge_state(states, ancestor.to_string_lossy().to_string(), next);
    }
}

fn state_priority(state: &GitFileState) -> u8 {
    match state {
        GitFileState::Ignored => 1,
        GitFileState::Tracked => 2,
        GitFileState::Modified => 3,
        GitFileState::Untracked => 4,
    }
}
