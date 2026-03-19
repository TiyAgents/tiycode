use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use git2::Repository;

use super::ToolOutput;
use crate::model::errors::{AppError, ErrorCategory, ErrorSource};
use crate::model::git::{GitCommandResultDto, GitMutationAction};

const GIT_OUTPUT_LIMIT: usize = 64_000;

pub async fn execute(
    tool_name: &str,
    input: &serde_json::Value,
    workspace_path: &str,
) -> Result<ToolOutput, AppError> {
    match tool_name {
        "git_add" | "git_stage" => {
            let paths = read_paths(input)?;
            stage_paths(workspace_path, &paths).await?;
            Ok(ToolOutput {
                success: true,
                result: serde_json::json!({
                    "action": "stage",
                    "paths": paths,
                }),
            })
        }
        "git_unstage" => {
            let paths = read_paths(input)?;
            unstage_paths(workspace_path, &paths).await?;
            Ok(ToolOutput {
                success: true,
                result: serde_json::json!({
                    "action": "unstage",
                    "paths": paths,
                }),
            })
        }
        "git_commit" => {
            let message = input["message"].as_str().unwrap_or_default();
            let result = commit(workspace_path, message).await?;
            Ok(ToolOutput {
                success: true,
                result: serde_json::to_value(result).unwrap_or_else(|_| serde_json::json!({})),
            })
        }
        "git_fetch" => {
            let result = fetch(workspace_path).await?;
            Ok(ToolOutput {
                success: true,
                result: serde_json::to_value(result).unwrap_or_else(|_| serde_json::json!({})),
            })
        }
        "git_pull" => {
            let result = pull(workspace_path).await?;
            Ok(ToolOutput {
                success: true,
                result: serde_json::to_value(result).unwrap_or_else(|_| serde_json::json!({})),
            })
        }
        "git_push" => {
            let result = push(workspace_path).await?;
            Ok(ToolOutput {
                success: true,
                result: serde_json::to_value(result).unwrap_or_else(|_| serde_json::json!({})),
            })
        }
        _ => Ok(ToolOutput {
            success: false,
            result: serde_json::json!({
                "error": format!("Unknown git tool: {tool_name}"),
            }),
        }),
    }
}

pub async fn commit(workspace_path: &str, message: &str) -> Result<GitCommandResultDto, AppError> {
    let trimmed = message.trim();
    if trimmed.is_empty() {
        return Err(git_error(
            "git.commit.message_invalid",
            "Commit message cannot be empty",
            false,
        ));
    }

    run_git_action(
        workspace_path,
        GitMutationAction::Commit,
        vec!["commit".to_string(), "-m".to_string(), trimmed.to_string()],
    )
    .await
}

pub async fn fetch(workspace_path: &str) -> Result<GitCommandResultDto, AppError> {
    run_git_action(
        workspace_path,
        GitMutationAction::Fetch,
        vec!["fetch".to_string(), "--prune".to_string()],
    )
    .await
}

pub async fn pull(workspace_path: &str) -> Result<GitCommandResultDto, AppError> {
    run_git_action(
        workspace_path,
        GitMutationAction::Pull,
        vec!["pull".to_string(), "--ff-only".to_string()],
    )
    .await
}

pub async fn push(workspace_path: &str) -> Result<GitCommandResultDto, AppError> {
    run_git_action(
        workspace_path,
        GitMutationAction::Push,
        vec!["push".to_string()],
    )
    .await
}

pub async fn stage_paths(workspace_path: &str, workspace_paths: &[String]) -> Result<(), AppError> {
    let workspace_root = canonicalize_workspace(workspace_path);
    let normalized_paths = normalize_workspace_relative_paths(workspace_paths)?;

    tokio::task::spawn_blocking(move || stage_paths_sync(&workspace_root, &normalized_paths))
        .await
        .map_err(|error| {
            AppError::internal(ErrorSource::Git, format!("Git stage task failed: {error}"))
        })?
}

pub async fn unstage_paths(
    workspace_path: &str,
    workspace_paths: &[String],
) -> Result<(), AppError> {
    let workspace_root = canonicalize_workspace(workspace_path);
    let normalized_paths = normalize_workspace_relative_paths(workspace_paths)?;

    tokio::task::spawn_blocking(move || unstage_paths_sync(&workspace_root, &normalized_paths))
        .await
        .map_err(|error| {
            AppError::internal(
                ErrorSource::Git,
                format!("Git unstage task failed: {error}"),
            )
        })?
}

async fn run_git_action(
    workspace_path: &str,
    action: GitMutationAction,
    args: Vec<String>,
) -> Result<GitCommandResultDto, AppError> {
    ensure_git_cli_available()?;

    let workspace_root = canonicalize_workspace(workspace_path);

    tokio::task::spawn_blocking(move || {
        let repo_root = discover_repo_root(&workspace_root)?;
        run_git_command(&repo_root, action, args)
    })
    .await
    .map_err(|error| {
        AppError::internal(ErrorSource::Git, format!("Git CLI task failed: {error}"))
    })?
}

fn run_git_command(
    repo_root: &Path,
    action: GitMutationAction,
    args: Vec<String>,
) -> Result<GitCommandResultDto, AppError> {
    let output = Command::new("git")
        .args(&args)
        .current_dir(repo_root)
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_PAGER", "cat")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .map_err(|error| {
            git_error(
                "git.cli.spawn_failed",
                format!("Failed to launch Git CLI: {error}"),
                true,
            )
        })?;

    let stdout = trim_output(&String::from_utf8_lossy(&output.stdout));
    let stderr = trim_output(&String::from_utf8_lossy(&output.stderr));

    if !output.status.success() {
        return Err(map_cli_failure(action, &stdout, &stderr));
    }

    Ok(GitCommandResultDto {
        action,
        summary: success_summary(action),
        stdout: (!stdout.is_empty()).then_some(stdout),
        stderr: (!stderr.is_empty()).then_some(stderr),
    })
}

fn read_paths(input: &serde_json::Value) -> Result<Vec<String>, AppError> {
    let Some(values) = input["paths"].as_array() else {
        return Err(git_error(
            "git.path.invalid",
            "Git mutation requires a non-empty 'paths' array",
            true,
        ));
    };

    let paths = values
        .iter()
        .filter_map(|value| value.as_str())
        .map(str::to_string)
        .collect::<Vec<_>>();

    if paths.is_empty() {
        return Err(git_error(
            "git.path.invalid",
            "Git mutation requires at least one path",
            true,
        ));
    }

    Ok(paths)
}

fn stage_paths_sync(workspace_root: &Path, workspace_paths: &[String]) -> Result<(), AppError> {
    let repo = open_repository(workspace_root)?;
    let repo_root = repo_workdir(&repo)?;
    let mut index = repo.index().map_err(|error| {
        git_error(
            "git.index.read_failed",
            format!("Unable to read Git index: {error}"),
            true,
        )
    })?;

    for workspace_relative in workspace_paths {
        let repo_relative =
            workspace_path_to_repo_path(&repo_root, workspace_root, workspace_relative)?;
        index.add_path(Path::new(&repo_relative)).map_err(|error| {
            git_error(
                "git.stage.failed",
                format!("Unable to stage '{workspace_relative}': {error}"),
                true,
            )
        })?;
    }

    index.write().map_err(|error| {
        git_error(
            "git.index.write_failed",
            format!("Unable to write staged changes: {error}"),
            true,
        )
    })
}

fn unstage_paths_sync(workspace_root: &Path, workspace_paths: &[String]) -> Result<(), AppError> {
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
            git_error(
                "git.unstage.failed",
                format!("Unable to unstage selected files: {error}"),
                true,
            )
        })
}

fn discover_repo_root(workspace_root: &Path) -> Result<PathBuf, AppError> {
    let repo = open_repository(workspace_root)?;
    repo_workdir(&repo)
}

fn ensure_git_cli_available() -> Result<(), AppError> {
    let available = Command::new("git")
        .arg("--version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|status| status.success())
        .unwrap_or(false);

    if available {
        Ok(())
    } else {
        Err(git_error(
            "git.cli.unavailable",
            "Git CLI is not installed or is not available on PATH",
            false,
        ))
    }
}

fn map_cli_failure(action: GitMutationAction, stdout: &str, stderr: &str) -> AppError {
    let combined = format!("{stdout}\n{stderr}").to_lowercase();

    if combined.contains("authentication failed")
        || combined.contains("permission denied (publickey)")
        || combined.contains("could not read username")
        || combined.contains("could not read from remote repository")
    {
        return git_error(
            "git.remote.auth_failed",
            format!(
                "Git {} failed because repository authentication was rejected",
                action.as_str()
            ),
            true,
        );
    }

    if combined.contains("could not resolve host")
        || combined.contains("failed to connect")
        || combined.contains("connection timed out")
        || combined.contains("network is unreachable")
    {
        return git_error(
            "git.remote.network_failed",
            format!(
                "Git {} failed because the remote host could not be reached",
                action.as_str()
            ),
            true,
        );
    }

    if action == GitMutationAction::Commit {
        if combined.contains("nothing to commit") {
            return git_error(
                "git.commit.no_staged_changes",
                "There are no staged changes to commit",
                false,
            );
        }

        if combined.contains("author identity unknown")
            || combined.contains("please tell me who you are")
        {
            return git_error(
                "git.commit.identity_missing",
                "Git user.name and user.email must be configured before committing",
                false,
            );
        }
    }

    if action == GitMutationAction::Pull
        && (combined.contains("not possible because you have unmerged files")
            || combined.contains("your local changes to the following files would be overwritten")
            || combined.contains("please commit your changes or stash them"))
    {
        return git_error(
            "git.pull.blocked_by_local_changes",
            "Git pull was blocked by local changes or merge conflicts",
            false,
        );
    }

    if action == GitMutationAction::Push
        && (combined.contains("has no upstream branch")
            || combined.contains("no upstream configured")
            || combined.contains("set-upstream"))
    {
        return git_error(
            "git.push.no_upstream",
            "The current branch has no upstream remote configured",
            false,
        );
    }

    git_error(
        &format!("git.{}.failed", action.as_str()),
        format!(
            "Git {} failed{}",
            action.as_str(),
            render_cli_hint(stdout, stderr)
        ),
        true,
    )
}

fn render_cli_hint(stdout: &str, stderr: &str) -> String {
    let message = if !stderr.is_empty() { stderr } else { stdout };
    if message.is_empty() {
        String::new()
    } else {
        format!(": {}", first_line(message))
    }
}

fn first_line(text: &str) -> &str {
    text.lines().next().unwrap_or(text)
}

fn success_summary(action: GitMutationAction) -> String {
    match action {
        GitMutationAction::Commit => "Committed staged changes".to_string(),
        GitMutationAction::Fetch => "Fetched remote updates".to_string(),
        GitMutationAction::Pull => "Pulled remote updates".to_string(),
        GitMutationAction::Push => "Pushed local commits".to_string(),
    }
}

fn trim_output(raw: &str) -> String {
    let trimmed = raw.trim();
    if trimmed.len() > GIT_OUTPUT_LIMIT {
        trimmed[..GIT_OUTPUT_LIMIT].to_string()
    } else {
        trimmed.to_string()
    }
}

fn open_repository(workspace_root: &Path) -> Result<Repository, AppError> {
    Repository::discover(workspace_root).map_err(|error| {
        if error.code() == git2::ErrorCode::NotFound {
            git_error(
                "git.repo.not_found",
                "The current workspace is not inside a Git repository",
                false,
            )
        } else {
            git_error(
                "git.repo.inaccessible",
                format!("Unable to read Git repository: {error}"),
                true,
            )
        }
    })
}

fn repo_workdir(repo: &Repository) -> Result<PathBuf, AppError> {
    repo.workdir()
        .map(|path| std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf()))
        .ok_or_else(|| {
            git_error(
                "git.repo.workdir_missing",
                "Bare repositories are not supported in the workspace Git drawer",
                false,
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
        return Err(git_error(
            "git.path.empty",
            "Git path cannot be empty",
            false,
        ));
    }

    let absolute_path = workspace_root.join(normalized);
    let repo_relative = absolute_path.strip_prefix(repo_root).map_err(|_| {
        git_error(
            "git.path.out_of_workspace",
            "The requested Git path is outside the repository root",
            false,
        )
    })?;

    Ok(repo_relative.to_string_lossy().to_string())
}

fn normalize_workspace_relative_paths(paths: &[String]) -> Result<Vec<String>, AppError> {
    let normalized = paths
        .iter()
        .map(|path| path.trim().trim_matches('/').to_string())
        .filter(|path| !path.is_empty())
        .collect::<Vec<_>>();

    if normalized.is_empty() {
        return Err(git_error(
            "git.path.empty",
            "At least one Git path is required",
            false,
        ));
    }

    Ok(normalized)
}

fn git_error(code: &str, message: impl Into<String>, retryable: bool) -> AppError {
    AppError {
        error_code: code.to_string(),
        category: ErrorCategory::Recoverable,
        source: ErrorSource::Git,
        user_message: message.into(),
        detail: None,
        retryable,
    }
}
