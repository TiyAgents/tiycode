use std::collections::HashMap;
use std::path::{Path, PathBuf};

use git2::{Repository, Status, StatusOptions};

use crate::core::index_manager::GitFileState;
use crate::model::errors::{AppError, ErrorSource};

#[derive(Debug, Clone)]
pub struct WorkspaceGitOverlay {
    pub repo_available: bool,
    pub states: HashMap<String, GitFileState>,
}

pub struct GitManager;

impl GitManager {
    pub fn new() -> Self {
        Self
    }

    pub async fn get_workspace_overlay(
        &self,
        workspace_path: &str,
    ) -> Result<WorkspaceGitOverlay, AppError> {
        let workspace_root =
            std::fs::canonicalize(workspace_path).unwrap_or_else(|_| PathBuf::from(workspace_path));

        tokio::task::spawn_blocking(move || collect_workspace_overlay(&workspace_root))
            .await
            .map_err(|error| {
                AppError::internal(
                    ErrorSource::Git,
                    format!("Git overlay task failed: {error}"),
                )
            })?
    }
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

    let repo_root = repo.workdir().ok_or_else(|| {
        AppError::recoverable(
            ErrorSource::Git,
            "git.repo.workdir_missing",
            "Bare repositories are not supported in the workspace tree",
        )
    })?;
    let repo_root = std::fs::canonicalize(repo_root).unwrap_or_else(|_| repo_root.to_path_buf());

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
        let state = map_status(entry.status());

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

fn map_status(status: Status) -> GitFileState {
    if status.is_ignored() {
        GitFileState::Ignored
    } else if status.is_wt_new() {
        GitFileState::Untracked
    } else {
        GitFileState::Tracked
    }
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
        GitFileState::Untracked => 3,
    }
}
