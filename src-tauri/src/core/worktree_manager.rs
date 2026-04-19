//! Git worktree management for workspace rows.
//!
//! Worktrees are modeled as workspace rows with `kind = 'worktree'` and a
//! `parent_workspace_id` pointing to the repo's workspace row. This module
//! owns the lifecycle of those rows: listing, creating, and deleting the
//! underlying Git worktree alongside the DB record.
//!
//! The Git operations themselves are executed via the `git` CLI (already used
//! by the rest of the codebase). Creating worktrees via the CLI keeps feature
//! parity with `--track`, `-b` and remote branches, which is important for
//! downstream UX even though the read-only enumeration below also works via
//! `git2`.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use chrono::Utc;
use git2::Repository;
use serde::Serialize;
use sqlx::SqlitePool;
use tokio::task;

use crate::core::windows_process::configure_background_std_command;
use crate::model::errors::{AppError, ErrorCategory, ErrorSource};
use crate::model::workspace::{
    WorkspaceKind, WorkspaceRecord, WorkspaceStatus, WorktreeCreateInput,
};
use crate::persistence::repo::workspace_repo;

const WORKTREE_OUTPUT_LIMIT: usize = 32_000;

/// Information about a git worktree, including whether it has been registered
/// as a TiyCode workspace row.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorktreeInfoDto {
    /// The git worktree name (i.e. the directory name inside
    /// `.git/worktrees/`). Used as the first-6-char tag on the frontend.
    pub name: String,
    pub path: String,
    pub branch: Option<String>,
    pub head: Option<String>,
    pub is_valid: bool,
    pub is_locked: bool,
    /// Workspace id when this worktree has been registered.
    pub workspace_id: Option<String>,
}

pub struct WorktreeManager {
    pool: SqlitePool,
}

impl WorktreeManager {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    /// List the worktrees discovered from the parent repo, merged with any
    /// worktree workspaces already registered in the DB.
    pub async fn list(&self, parent: &WorkspaceRecord) -> Result<Vec<WorktreeInfoDto>, AppError> {
        require_repo_kind(parent)?;

        let repo_root = parent.canonical_path.clone();
        let git_infos = task::spawn_blocking(move || enumerate_git_worktrees(&repo_root))
            .await
            .map_err(|error| {
                AppError::internal(
                    ErrorSource::Git,
                    format!("worktree enumeration task failed: {error}"),
                )
            })??;

        let registered = workspace_repo::list_worktrees_of(&self.pool, &parent.id).await?;

        let mut results = Vec::with_capacity(git_infos.len().max(registered.len()));
        for info in git_infos {
            let matching =
                registered
                    .iter()
                    .find(|row| match (&row.worktree_name, &row.canonical_path) {
                        (Some(name), _) if name == &info.name => true,
                        (_, path) => path == &info.path,
                    });
            results.push(WorktreeInfoDto {
                workspace_id: matching.map(|row| row.id.clone()),
                ..info
            });
        }
        Ok(results)
    }

    /// Create a new worktree for the given repo workspace, register a
    /// `kind='worktree'` workspace row for it, and return the new record.
    pub async fn create(
        &self,
        parent: &WorkspaceRecord,
        input: WorktreeCreateInput,
    ) -> Result<WorkspaceRecord, AppError> {
        require_repo_kind(parent)?;
        let branch = validate_branch_name(&input.branch)?;
        let slug = slugify_branch(branch);
        // The worktree name is the short logical label for this worktree. The
        // first 6 characters (= the random hex prefix) are what the sidebar
        // displays as a hash tag, so uniqueness across siblings is guaranteed.
        let hex6 = random_hex6();
        let worktree_name = format!("{hex6}-{slug}");
        let target_path = match input
            .path
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            Some(custom) => {
                let p = PathBuf::from(custom);
                // Resolve relative paths against the parent repo directory so
                // the user can pass e.g. `../my-worktree` in the dialog.
                let resolved = if p.is_relative() {
                    PathBuf::from(&parent.canonical_path).join(&p)
                } else {
                    p
                };

                // Reject target paths that fall inside the parent repo working
                // tree. Creating a worktree inside its own repo causes Git
                // conflicts and confusing file-system state.
                let parent_dir = PathBuf::from(&parent.canonical_path);
                if resolved.starts_with(&parent_dir) {
                    return Err(worktree_error(
                        "workspace.worktree.path_inside_repo",
                        format!(
                            "Worktree path '{}' must not be inside the parent repository '{}'",
                            resolved.display(),
                            parent_dir.display(),
                        ),
                        false,
                    ));
                }

                resolved
            }
            None => default_worktree_path(&parent.name, &hex6)?,
        };

        if target_path.exists() {
            return Err(worktree_error(
                "workspace.worktree.path_exists",
                format!(
                    "Worktree target path '{}' already exists",
                    target_path.display()
                ),
                false,
            ));
        }

        let parent_path_string = parent.canonical_path.clone();
        let target_path_string = target_path.to_string_lossy().to_string();
        let worktree_name_clone = worktree_name.clone();
        let branch_name = branch.to_string();
        let base_ref = input
            .base_ref
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string);
        let create_branch = input.create_branch;

        task::spawn_blocking(move || {
            run_git_worktree_add(
                &parent_path_string,
                &target_path_string,
                &worktree_name_clone,
                &branch_name,
                create_branch,
                base_ref.as_deref(),
            )
        })
        .await
        .map_err(|error| {
            AppError::internal(
                ErrorSource::Git,
                format!("git worktree add task failed: {error}"),
            )
        })??;

        // Canonicalize the freshly created worktree path so subsequent lookups match.
        let canonical = task::spawn_blocking({
            let target = target_path.clone();
            move || dunce::canonicalize(&target)
        })
        .await
        .map_err(|error| {
            AppError::internal(
                ErrorSource::Workspace,
                format!("worktree path canonicalization task failed: {error}"),
            )
        })?
        .map_err(|error| {
            AppError::recoverable(
                ErrorSource::Workspace,
                "workspace.worktree.path_invalid",
                format!(
                    "Cannot resolve worktree path '{}': {error}",
                    target_path.display()
                ),
            )
        })?;
        let canonical_str = canonical.to_string_lossy().to_string();

        // Derive git_common_dir from the worktree (via git2), fall back to parent.git.
        let git_common_dir = task::spawn_blocking({
            let canonical = canonical.clone();
            move || -> Option<String> {
                Repository::open(&canonical)
                    .ok()
                    .map(|repo| repo.commondir().to_path_buf())
                    .and_then(|p| dunce::canonicalize(&p).ok())
                    .map(|p| p.to_string_lossy().to_string())
            }
        })
        .await
        .unwrap_or(None)
        .unwrap_or_else(|| format!("{}/.git", parent.canonical_path));

        let display_path = derive_display_path(&canonical).await;
        let name = derive_name_from_path(&canonical);

        let now = Utc::now();
        let record = WorkspaceRecord {
            id: uuid::Uuid::now_v7().to_string(),
            name,
            path: target_path.to_string_lossy().to_string(),
            canonical_path: canonical_str,
            display_path,
            is_default: false,
            is_git: true,
            auto_work_tree: false,
            status: WorkspaceStatus::Ready,
            last_validated_at: Some(now),
            created_at: now,
            updated_at: now,
            kind: WorkspaceKind::Worktree,
            parent_workspace_id: Some(parent.id.clone()),
            git_common_dir: Some(git_common_dir),
            branch: Some(branch.to_string()),
            worktree_name: Some(worktree_name.clone()),
        };

        workspace_repo::insert(&self.pool, &record).await?;

        // Make sure the parent workspace is tagged as a repo from now on.
        if parent.kind != WorkspaceKind::Repo {
            workspace_repo::update_kind_metadata(
                &self.pool,
                &parent.id,
                WorkspaceKind::Repo,
                None,
                None,
                None,
                None,
            )
            .await?;
        }

        tracing::info!(
            parent_workspace_id = %parent.id,
            workspace_id = %record.id,
            worktree_name = %worktree_name,
            branch = %branch,
            path = %record.canonical_path,
            "worktree created"
        );

        Ok(record)
    }

    /// Physically remove the worktree directory and its `.git/worktrees/<name>`
    /// registration. The caller is responsible for the DB row deletion via
    /// `WorkspaceManager::remove`. If the worktree is already gone this is a
    /// no-op returning `Ok(())`.
    pub async fn remove_physical(
        &self,
        worktree: &WorkspaceRecord,
        force: bool,
    ) -> Result<(), AppError> {
        if worktree.kind != WorkspaceKind::Worktree {
            return Err(worktree_error(
                "workspace.worktree.kind_mismatch",
                "workspace_remove_worktree can only be called on a worktree row",
                false,
            ));
        }

        let Some(parent_id) = worktree.parent_workspace_id.as_deref() else {
            return Ok(());
        };
        let parent = workspace_repo::find_by_id(&self.pool, parent_id).await?;
        let Some(parent) = parent else {
            return Ok(());
        };

        let parent_path = parent.canonical_path.clone();
        let worktree_path = worktree.canonical_path.clone();
        task::spawn_blocking(move || run_git_worktree_remove(&parent_path, &worktree_path, force))
            .await
            .map_err(|error| {
                AppError::internal(
                    ErrorSource::Git,
                    format!("git worktree remove task failed: {error}"),
                )
            })??;

        Ok(())
    }

    /// Run `git worktree prune` on the parent repo. Useful when a worktree
    /// directory was deleted externally.
    pub async fn prune(&self, parent: &WorkspaceRecord) -> Result<(), AppError> {
        require_repo_kind(parent)?;
        let repo_root = parent.canonical_path.clone();
        task::spawn_blocking(move || run_git_worktree_prune(&repo_root))
            .await
            .map_err(|error| {
                AppError::internal(
                    ErrorSource::Git,
                    format!("git worktree prune task failed: {error}"),
                )
            })??;
        Ok(())
    }
}

fn require_repo_kind(parent: &WorkspaceRecord) -> Result<(), AppError> {
    let allowed = matches!(parent.kind, WorkspaceKind::Repo)
        || (matches!(parent.kind, WorkspaceKind::Standalone) && parent.is_git);
    if allowed {
        Ok(())
    } else {
        Err(worktree_error(
            "workspace.worktree.parent_not_repo",
            "Worktrees can only be created on a Git repository workspace",
            false,
        ))
    }
}

fn validate_branch_name(name: &str) -> Result<&str, AppError> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return Err(worktree_error(
            "workspace.worktree.branch_empty",
            "Branch name cannot be empty",
            false,
        ));
    }

    let full_ref = format!("refs/heads/{trimmed}");
    if !git2::Reference::is_valid_name(&full_ref) {
        return Err(worktree_error(
            "workspace.worktree.branch_invalid",
            format!("'{trimmed}' is not a valid branch name"),
            false,
        ));
    }
    Ok(trimmed)
}

fn slugify_branch(branch: &str) -> String {
    let lower = branch.to_ascii_lowercase();
    let mut out = String::with_capacity(lower.len());
    let mut last_dash = false;
    for ch in lower.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch);
            last_dash = false;
        } else if ch == '-' || ch == '_' || ch == '.' {
            if !last_dash {
                out.push('-');
                last_dash = true;
            }
        } else {
            if !last_dash {
                out.push('-');
                last_dash = true;
            }
        }
    }
    let trimmed = out.trim_matches('-').to_string();
    if trimmed.is_empty() {
        "worktree".to_string()
    } else if trimmed.len() > 40 {
        trimmed[..40].trim_matches('-').to_string()
    } else {
        trimmed
    }
}

fn random_hex6() -> String {
    let raw = uuid::Uuid::new_v4().simple().to_string();
    raw[..6].to_string()
}

/// Compute the default worktree directory under the TiyCode data root:
/// `~/.tiy/workspace/<hex6>/<safe-repo-name>`. Ensures the parent directory
/// exists so `git worktree add` will succeed even on a fresh install where
/// `~/.tiy/workspace/` has never been created.
fn default_worktree_path(repo_name: &str, hex6: &str) -> Result<PathBuf, AppError> {
    let home = dirs::home_dir().ok_or_else(|| {
        AppError::internal(ErrorSource::Workspace, "cannot resolve HOME directory")
    })?;
    let safe_name = sanitize_repo_name_for_path(repo_name);
    let parent_dir = home.join(".tiy").join("workspace").join(hex6);

    if let Err(error) = std::fs::create_dir_all(&parent_dir) {
        return Err(worktree_error(
            "workspace.worktree.prepare_default_path_failed",
            format!(
                "Failed to create default worktree parent directory '{}': {error}",
                parent_dir.display()
            ),
            false,
        ));
    }

    Ok(parent_dir.join(safe_name))
}

fn sanitize_repo_name_for_path(name: &str) -> String {
    let trimmed = name.trim();
    let mut out = String::with_capacity(trimmed.len());
    for ch in trimmed.chars() {
        if ch.is_alphanumeric() || ch == '-' || ch == '_' || ch == '.' {
            out.push(ch);
        } else if ch.is_whitespace() || ch == '/' || ch == '\\' {
            out.push('-');
        }
    }
    let cleaned = out.trim_matches(|c: char| c == '-' || c == '.').to_string();
    if cleaned.is_empty() {
        "workspace".to_string()
    } else {
        cleaned
    }
}

fn derive_name_from_path(path: &Path) -> String {
    path.file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("Worktree")
        .to_string()
}

async fn derive_display_path(path: &Path) -> String {
    if let Some(home) = dirs::home_dir() {
        if let Ok(relative) = path.strip_prefix(&home) {
            return format!("~/{}", relative.display());
        }
        if let Ok(canonical_home) = dunce::canonicalize(&home) {
            if let Ok(relative) = path.strip_prefix(&canonical_home) {
                return format!("~/{}", relative.display());
            }
        }
    }
    path.display().to_string()
}

fn enumerate_git_worktrees(repo_root: &str) -> Result<Vec<WorktreeInfoDto>, AppError> {
    let repo = Repository::open(repo_root).map_err(|error| {
        worktree_error(
            "workspace.worktree.repo_open_failed",
            format!("Unable to open Git repository: {error}"),
            false,
        )
    })?;

    let names = repo.worktrees().map_err(|error| {
        worktree_error(
            "workspace.worktree.enumerate_failed",
            format!("Unable to list Git worktrees: {error}"),
            false,
        )
    })?;

    let mut out = Vec::new();
    for idx in 0..names.len() {
        let Some(name) = names.get(idx) else { continue };
        let worktree = match repo.find_worktree(name) {
            Ok(wt) => wt,
            Err(_) => continue,
        };

        let path = dunce::canonicalize(worktree.path())
            .unwrap_or_else(|_| worktree.path().to_path_buf())
            .to_string_lossy()
            .to_string();
        let is_valid = worktree.validate().is_ok();
        let is_locked = matches!(
            worktree.is_locked(),
            Ok(git2::WorktreeLockStatus::Locked(_))
        );

        let (branch, head) = if is_valid {
            match Repository::open(worktree.path()) {
                Ok(wt_repo) => {
                    let head = wt_repo.head().ok();
                    let branch = head
                        .as_ref()
                        .and_then(|h| h.shorthand().map(|s| s.to_string()));
                    let oid = head
                        .as_ref()
                        .and_then(|h| h.target())
                        .map(|oid| oid.to_string());
                    (branch, oid)
                }
                Err(_) => (None, None),
            }
        } else {
            (None, None)
        };

        out.push(WorktreeInfoDto {
            name: name.to_string(),
            path,
            branch,
            head,
            is_valid,
            is_locked,
            workspace_id: None,
        });
    }

    Ok(out)
}

fn run_git_worktree_add(
    repo_root: &str,
    target_path: &str,
    worktree_name: &str,
    branch: &str,
    create_branch: bool,
    base_ref: Option<&str>,
) -> Result<(), AppError> {
    // `git worktree add` uses the final path component as the worktree
    // registration name. We don't force it explicitly; the caller is expected
    // to produce a target_path whose basename already matches worktree_name
    // when that identity matters.
    let _ = worktree_name;

    let mut args: Vec<String> = vec!["worktree".to_string(), "add".to_string()];

    if create_branch {
        args.push("-b".to_string());
        args.push(branch.to_string());
        args.push(target_path.to_string());
        if let Some(base) = base_ref {
            args.push(base.to_string());
        }
    } else {
        args.push(target_path.to_string());
        args.push(branch.to_string());
    }

    run_git_cli(repo_root, &args).map(|_| ())
}

fn run_git_worktree_remove(
    repo_root: &str,
    worktree_path: &str,
    force: bool,
) -> Result<(), AppError> {
    let mut args: Vec<String> = vec!["worktree".to_string(), "remove".to_string()];
    if force {
        args.push("--force".to_string());
    }
    args.push(worktree_path.to_string());
    match run_git_cli(repo_root, &args) {
        Ok(_) => Ok(()),
        Err(error) => {
            // Non-existing worktree (already removed) → tolerate.
            let detail = error.detail.clone().unwrap_or_default().to_lowercase();
            let message = error.user_message.to_lowercase();
            if detail.contains("not a working tree")
                || detail.contains("no such")
                || message.contains("not a working tree")
                || message.contains("no such")
            {
                // Also run prune so the admin store is cleaned.
                let _ = run_git_worktree_prune(repo_root);
                Ok(())
            } else {
                Err(error)
            }
        }
    }
}

fn run_git_worktree_prune(repo_root: &str) -> Result<(), AppError> {
    let args = vec!["worktree".to_string(), "prune".to_string()];
    run_git_cli(repo_root, &args).map(|_| ())
}

fn run_git_cli(repo_root: &str, args: &[String]) -> Result<String, AppError> {
    ensure_git_cli_available()?;

    let mut command = Command::new("git");
    configure_background_std_command(&mut command);

    let output = command
        .args(args)
        .current_dir(repo_root)
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_PAGER", "cat")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .map_err(|error| {
            worktree_error(
                "workspace.worktree.cli_spawn_failed",
                format!("Failed to launch Git CLI: {error}"),
                true,
            )
        })?;

    let stdout = trim_output(&String::from_utf8_lossy(&output.stdout));
    let stderr = trim_output(&String::from_utf8_lossy(&output.stderr));

    if !output.status.success() {
        let action = args
            .iter()
            .skip(1)
            .next()
            .map(|s| s.as_str())
            .unwrap_or("command");
        return Err(AppError {
            error_code: format!("workspace.worktree.{action}_failed"),
            category: ErrorCategory::Recoverable,
            source: ErrorSource::Git,
            user_message: first_line(if !stderr.is_empty() { &stderr } else { &stdout })
                .to_string(),
            detail: Some(format!("stdout: {stdout}\nstderr: {stderr}")),
            retryable: false,
        });
    }

    Ok(stdout)
}

fn ensure_git_cli_available() -> Result<(), AppError> {
    let mut command = Command::new("git");
    configure_background_std_command(&mut command);
    let available = command
        .arg("--version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    if available {
        Ok(())
    } else {
        Err(worktree_error(
            "workspace.worktree.cli_unavailable",
            "Git CLI is not installed or is not available on PATH",
            false,
        ))
    }
}

fn trim_output(raw: &str) -> String {
    let trimmed = raw.trim();
    if trimmed.len() > WORKTREE_OUTPUT_LIMIT {
        trimmed[..WORKTREE_OUTPUT_LIMIT].to_string()
    } else {
        trimmed.to_string()
    }
}

fn first_line(text: &str) -> &str {
    text.lines().next().unwrap_or(text)
}

fn worktree_error(code: &str, message: impl Into<String>, retryable: bool) -> AppError {
    AppError {
        error_code: code.to_string(),
        category: ErrorCategory::Recoverable,
        source: ErrorSource::Workspace,
        user_message: message.into(),
        detail: None,
        retryable,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slugify_handles_common_branch_names() {
        assert_eq!(slugify_branch("feature/foo-bar"), "feature-foo-bar");
        assert_eq!(slugify_branch("fix/auth_bug"), "fix-auth-bug");
        assert_eq!(slugify_branch("Main"), "main");
        assert_eq!(slugify_branch("   "), "worktree");
    }

    #[test]
    fn random_hex6_is_six_chars() {
        let value = random_hex6();
        assert_eq!(value.len(), 6);
        assert!(value.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn default_worktree_path_uses_tiy_workspace_root() {
        let path = default_worktree_path("my-repo", "abcdef").expect("should compute path");
        let home = dirs::home_dir().expect("home available");
        let expected = home
            .join(".tiy")
            .join("workspace")
            .join("abcdef")
            .join("my-repo");
        assert_eq!(path, expected);
        // The parent directory should exist on disk after the call.
        assert!(expected.parent().unwrap().is_dir());
    }

    #[test]
    fn sanitize_repo_name_replaces_path_and_whitespace_characters() {
        assert_eq!(sanitize_repo_name_for_path("my repo"), "my-repo");
        assert_eq!(sanitize_repo_name_for_path("a/b\\c"), "a-b-c");
        assert_eq!(sanitize_repo_name_for_path("  -repo.-"), "repo");
        assert_eq!(sanitize_repo_name_for_path("***"), "workspace");
    }
}
