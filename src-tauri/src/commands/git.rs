use std::sync::Arc;
use std::time::Duration;

use tauri::{ipc::Channel, State};
use tiycore::provider::get_provider;
use tiycore::types::{
    Context as TiyContext, Message as TiyMessage, OnPayloadFn, StopReason,
    StreamOptions as TiyStreamOptions, UserMessage,
};

use crate::core::agent_session::{
    resolve_runtime_model_role, ResolvedModelRole, RuntimeModelPlan, RuntimeModelRole,
};
use crate::core::app_state::AppState;
use crate::core::policy_engine::{PolicyEngine, PolicyVerdict};
use crate::core::tiycode_default_headers;
use crate::ipc::frontend_channels::GitStreamEvent;
use crate::model::errors::{AppError, ErrorCategory, ErrorSource};
use crate::model::git::{
    GitBranchDto, GitCommandResultDto, GitCommitSummaryDto, GitDiffDto, GitFileChangeDto,
    GitFileStatusDto, GitMutationAction, GitMutationResponseDto, GitSnapshotDto,
};
use crate::model::workspace::WorkspaceRecord;
use crate::persistence::repo::{audit_repo, workspace_repo};

const COMMIT_MESSAGE_MAX_TOKENS: u32 = 2048;
const COMMIT_MESSAGE_MAX_TOKENS_REASONING: u32 = 4096;
const COMMIT_MESSAGE_TIMEOUT: Duration = Duration::from_secs(60);
const COMMIT_MESSAGE_FILE_LIMIT: usize = 24;
const COMMIT_MESSAGE_DIFF_CHAR_BUDGET: usize = 48_000;
const COMMIT_MESSAGE_GENERATOR_PROMPT: &str = r#"You are a commit message generator for Git changes.

Your task is to produce exactly one commit message that follows Conventional Commits.

Input priority:
1. If staged files exist, generate the commit message using only staged changes.
2. If no staged files exist, generate the commit message using all modified, added, and deleted files in the working tree.

## Conventional Commits Format

### Simple Style (Default)
```
<type>[optional scope]: <emoji> <description>
```
Example: `feat(auth): ✨ add JWT token validation`

### Full Style
```
<type>[optional scope]: <emoji> <description>

<body>

<footer>
```

## Commit Types & Emojis

| Type | Emoji | Description | When to Use |
|------|-------|-------------|-------------|
| `feat` | ✨ | New feature | Adding new functionality |
| `fix` | 🐛 | Bug fix | Fixing an issue |
| `docs` | 📝 | Documentation | Documentation only changes |
| `style` | 🎨 | Code style | Formatting, missing semi-colons, etc |
| `refactor` | ♻️ | Code refactoring | Neither fixes bug nor adds feature |
| `perf` | ⚡️ | Performance | Performance improvements |
| `test` | ✅ | Testing | Adding missing tests |
| `chore` | 🔧 | Maintenance | Changes to build process or tools |
| `ci` | 👷 | CI/CD | Changes to CI configuration |
| `build` | 📦 | Build system | Changes affecting build system |
| `revert` | ⏪ | Revert | Reverting previous commit |

## Body Section Guidelines (Full Style)

The body should:
- Explain **what** changed and **why** (not how)
- Use bullet points for multiple changes
- Include motivation for the change
- Contrast behavior with previous behavior
- Reference related issues or decisions
- Be wrapped at 72 characters per line

Good body example:
```
Previously, the application allowed unauthenticated access to
user profile endpoints, creating a security vulnerability.

This commit adds comprehensive authentication middleware that:
- Validates JWT tokens on all protected routes
- Implements proper token refresh logic
- Adds rate limiting to prevent brute force attacks
- Logs authentication failures for monitoring

The change follows OAuth 2.0 best practices and improves
overall application security posture.
```

## Footer Section Guidelines (Full Style)

Footer contains:
- **Breaking changes**: Start with `BREAKING CHANGE:`
- **Issue references**: `Closes:`, `Fixes:`, `Refs:`
- **Review references**: `Reviewed-by:`, `Approved-by:`

Example footers:
```
BREAKING CHANGE: rename config.auth to config.authentication
Closes: #123, #124
```

## Scope Guidelines

Scope should be:
- A noun describing the section of codebase
- Consistent across the project
- Brief and meaningful

Common scopes:
- `api`, `auth`, `ui`, `db`, `config`, `deps`
- Component names: `button`, `modal`, `header`
- Module names: `parser`, `compiler`, `validator`

## Commit Splitting Strategy

Automatically suggest splitting when detecting:
1. **Mixed types**: Features + fixes in same commit
2. **Multiple concerns**: Unrelated changes
3. **Large scope**: Changes across many modules
4. **File patterns**: Source + test + docs together
5. **Dependencies**: Dependency updates mixed with features

## Best Practices

### DO:
- ✅ Write in present tense, imperative mood ("add" not "added")
- ✅ Keep first line under 50 characters (72 max)
- ✅ Capitalize first letter of description
- ✅ No period at end of subject line
- ✅ Separate subject from body with blank line
- ✅ Use body to explain what and why vs. how
- ✅ Reference issues and breaking changes

### DON'T:
- ❌ Mix multiple logical changes in one commit
- ❌ Include implementation details in subject
- ❌ Use past tense ("added" instead of "add")
- ❌ Make commits too large to review
- ❌ Commit broken code (unless WIP)
- ❌ Include sensitive information

## Examples
### Full Style Example
```bash
feat(auth): ✨ implement OAuth2 authentication flow

Add complete OAuth2 authentication system supporting multiple
providers (Google, GitHub, Microsoft). The implementation
follows RFC 6749 specification and includes:

- Authorization code flow with PKCE
- Refresh token rotation
- Scope-based permissions
- Session management with Redis
- Rate limiting per client

This provides users with secure single sign-on capabilities
while maintaining backwards compatibility with existing
JWT authentication.

BREAKING CHANGE: /api/auth endpoints now require client_id parameter
Closes: #456, #457
Refs: RFC-6749, RFC-7636
```

If information is insufficient, make the best reasonable inference from the available changes.
Return only the commit message."#;

#[tauri::command]
pub async fn git_get_snapshot(
    state: State<'_, AppState>,
    workspace_id: String,
) -> Result<GitSnapshotDto, AppError> {
    let workspace = load_workspace(&state, &workspace_id).await?;

    state
        .git_manager
        .get_snapshot(&workspace_id, &workspace.canonical_path)
        .await
}

#[tauri::command]
pub async fn git_get_history(
    state: State<'_, AppState>,
    workspace_id: String,
    limit: Option<usize>,
) -> Result<Vec<GitCommitSummaryDto>, AppError> {
    let workspace = load_workspace(&state, &workspace_id).await?;

    state
        .git_manager
        .get_history(&workspace.canonical_path, limit)
        .await
}

#[tauri::command]
pub async fn git_get_diff(
    state: State<'_, AppState>,
    workspace_id: String,
    path: String,
    staged: Option<bool>,
) -> Result<GitDiffDto, AppError> {
    let workspace = load_workspace(&state, &workspace_id).await?;

    state
        .git_manager
        .get_diff(&workspace.canonical_path, &path, staged.unwrap_or(false))
        .await
}

#[tauri::command]
pub async fn git_get_file_status(
    state: State<'_, AppState>,
    workspace_id: String,
    path: String,
) -> Result<GitFileStatusDto, AppError> {
    let workspace = load_workspace(&state, &workspace_id).await?;

    state
        .git_manager
        .get_file_status(&workspace.canonical_path, &path)
        .await
}

#[tauri::command]
pub async fn git_subscribe(
    state: State<'_, AppState>,
    workspace_id: String,
    on_event: Channel<GitStreamEvent>,
) -> Result<(), AppError> {
    let mut receiver = state.git_manager.subscribe(&workspace_id).await;
    let subscription_id = on_event.id();
    let git_manager = state.git_manager.clone();
    let workspace_id_for_cleanup = workspace_id.clone();

    let handle = tokio::spawn(async move {
        while let Ok(event) = receiver.recv().await {
            if on_event.send(event).is_err() {
                break;
            }
        }

        git_manager
            .finish_subscription(&workspace_id_for_cleanup, subscription_id)
            .await;
    });

    state
        .git_manager
        .register_subscription(&workspace_id, subscription_id, handle)
        .await;

    Ok(())
}

#[tauri::command]
pub async fn git_unsubscribe(
    state: State<'_, AppState>,
    workspace_id: String,
    subscription_id: u32,
) -> Result<(), AppError> {
    state
        .git_manager
        .unregister_subscription(&workspace_id, subscription_id)
        .await;

    Ok(())
}

#[tauri::command]
pub async fn git_refresh(
    state: State<'_, AppState>,
    workspace_id: String,
) -> Result<GitSnapshotDto, AppError> {
    let workspace = load_workspace(&state, &workspace_id).await?;

    state
        .git_manager
        .refresh(&workspace_id, &workspace.canonical_path)
        .await
}

#[tauri::command]
pub async fn git_stage(
    state: State<'_, AppState>,
    workspace_id: String,
    paths: Vec<String>,
) -> Result<GitSnapshotDto, AppError> {
    let workspace = load_workspace(&state, &workspace_id).await?;

    state
        .git_manager
        .stage(&workspace_id, &workspace.canonical_path, &paths)
        .await
}

#[tauri::command]
pub async fn git_unstage(
    state: State<'_, AppState>,
    workspace_id: String,
    paths: Vec<String>,
) -> Result<GitSnapshotDto, AppError> {
    let workspace = load_workspace(&state, &workspace_id).await?;

    state
        .git_manager
        .unstage(&workspace_id, &workspace.canonical_path, &paths)
        .await
}

#[tauri::command]
pub async fn git_commit(
    state: State<'_, AppState>,
    workspace_id: String,
    message: String,
    approved: Option<bool>,
) -> Result<GitMutationResponseDto, AppError> {
    let workspace = load_workspace(&state, &workspace_id).await?;
    let git_manager = state.git_manager.clone();
    let workspace_id_for_run = workspace_id.clone();
    let workspace_path = workspace.canonical_path.clone();
    let message_for_run = message.clone();
    let input = serde_json::json!({ "message": message.trim() });

    authorize_and_run_git_mutation(
        &state,
        &workspace,
        GitMutationAction::Commit,
        &input,
        approved.unwrap_or(false),
        move || async move {
            git_manager
                .commit(&workspace_id_for_run, &workspace_path, &message_for_run)
                .await
        },
    )
    .await
}

#[tauri::command]
pub async fn git_generate_commit_message(
    state: State<'_, AppState>,
    workspace_id: String,
    model_plan: serde_json::Value,
    language: Option<String>,
    prompt: Option<String>,
) -> Result<String, AppError> {
    let workspace = load_workspace(&state, &workspace_id).await?;
    let raw_plan: RuntimeModelPlan = serde_json::from_value(model_plan).unwrap_or_default();
    let selected_model = select_commit_message_model_role(&raw_plan)?;
    let model_role = resolve_runtime_model_role(&state.pool, selected_model).await?;
    let snapshot = state
        .git_manager
        .get_snapshot(&workspace_id, &workspace.canonical_path)
        .await?;
    let (source_label, staged, changes) = select_commit_message_changes(&snapshot)?;
    let effective_language = normalize_commit_message_language(language.as_deref());
    let prompt = build_commit_message_prompt(
        &state,
        &workspace,
        &workspace.canonical_path,
        source_label,
        staged,
        &changes,
        &effective_language,
        prompt.as_deref(),
    )
    .await?;

    generate_commit_message(&model_role, &prompt).await
}

#[tauri::command]
pub async fn git_fetch(
    state: State<'_, AppState>,
    workspace_id: String,
    approved: Option<bool>,
) -> Result<GitMutationResponseDto, AppError> {
    let workspace = load_workspace(&state, &workspace_id).await?;
    let git_manager = state.git_manager.clone();
    let workspace_id_for_run = workspace_id.clone();
    let workspace_path = workspace.canonical_path.clone();
    let input = serde_json::json!({});

    authorize_and_run_git_mutation(
        &state,
        &workspace,
        GitMutationAction::Fetch,
        &input,
        approved.unwrap_or(false),
        move || async move {
            git_manager
                .fetch(&workspace_id_for_run, &workspace_path)
                .await
        },
    )
    .await
}

#[tauri::command]
pub async fn git_pull(
    state: State<'_, AppState>,
    workspace_id: String,
    approved: Option<bool>,
) -> Result<GitMutationResponseDto, AppError> {
    let workspace = load_workspace(&state, &workspace_id).await?;
    let git_manager = state.git_manager.clone();
    let workspace_id_for_run = workspace_id.clone();
    let workspace_path = workspace.canonical_path.clone();
    let input = serde_json::json!({});

    authorize_and_run_git_mutation(
        &state,
        &workspace,
        GitMutationAction::Pull,
        &input,
        approved.unwrap_or(false),
        move || async move {
            git_manager
                .pull(&workspace_id_for_run, &workspace_path)
                .await
        },
    )
    .await
}

#[tauri::command]
pub async fn git_push(
    state: State<'_, AppState>,
    workspace_id: String,
    approved: Option<bool>,
) -> Result<GitMutationResponseDto, AppError> {
    let workspace = load_workspace(&state, &workspace_id).await?;
    let git_manager = state.git_manager.clone();
    let workspace_id_for_run = workspace_id.clone();
    let workspace_path = workspace.canonical_path.clone();
    let input = serde_json::json!({});

    authorize_and_run_git_mutation(
        &state,
        &workspace,
        GitMutationAction::Push,
        &input,
        approved.unwrap_or(false),
        move || async move {
            git_manager
                .push(&workspace_id_for_run, &workspace_path)
                .await
        },
    )
    .await
}

#[tauri::command]
pub async fn git_list_branches(
    state: State<'_, AppState>,
    workspace_id: String,
) -> Result<Vec<GitBranchDto>, AppError> {
    let workspace = load_workspace(&state, &workspace_id).await?;

    state
        .git_manager
        .list_branches(&workspace.canonical_path)
        .await
}

#[tauri::command]
pub async fn git_checkout_branch(
    state: State<'_, AppState>,
    workspace_id: String,
    branch: String,
    approved: Option<bool>,
) -> Result<GitMutationResponseDto, AppError> {
    let workspace = load_workspace(&state, &workspace_id).await?;
    let git_manager = state.git_manager.clone();
    let workspace_id_for_run = workspace_id.clone();
    let workspace_path = workspace.canonical_path.clone();
    let branch_for_run = branch.clone();
    let input = serde_json::json!({ "branch": branch.trim() });

    authorize_and_run_git_mutation(
        &state,
        &workspace,
        GitMutationAction::Checkout,
        &input,
        approved.unwrap_or(false),
        move || async move {
            git_manager
                .checkout_branch(&workspace_id_for_run, &workspace_path, &branch_for_run)
                .await
        },
    )
    .await
}

#[tauri::command]
pub async fn git_create_branch(
    state: State<'_, AppState>,
    workspace_id: String,
    branch: String,
    approved: Option<bool>,
) -> Result<GitMutationResponseDto, AppError> {
    let workspace = load_workspace(&state, &workspace_id).await?;
    let git_manager = state.git_manager.clone();
    let workspace_id_for_run = workspace_id.clone();
    let workspace_path = workspace.canonical_path.clone();
    let branch_for_run = branch.clone();
    let input = serde_json::json!({ "branch": branch.trim() });

    authorize_and_run_git_mutation(
        &state,
        &workspace,
        GitMutationAction::CreateBranch,
        &input,
        approved.unwrap_or(false),
        move || async move {
            git_manager
                .create_branch(&workspace_id_for_run, &workspace_path, &branch_for_run)
                .await
        },
    )
    .await
}

#[tauri::command]
pub async fn git_generate_branch_name(
    state: State<'_, AppState>,
    workspace_id: String,
    model_plan: serde_json::Value,
) -> Result<String, AppError> {
    let workspace = load_workspace(&state, &workspace_id).await?;
    let raw_plan: RuntimeModelPlan = serde_json::from_value(model_plan).unwrap_or_default();
    let selected_model = select_commit_message_model_role(&raw_plan)?;
    let model_role = resolve_runtime_model_role(&state.pool, selected_model).await?;
    let snapshot = state
        .git_manager
        .get_snapshot(&workspace_id, &workspace.canonical_path)
        .await?;
    let branches = state
        .git_manager
        .list_branches(&workspace.canonical_path)
        .await?;
    let prompt =
        build_branch_name_prompt(&state, &workspace.canonical_path, &snapshot, &branches).await?;

    generate_with_lite_model(
        &model_role,
        "You generate a single Git branch name. Return ONLY the branch name, nothing else. No explanation, no markdown.",
        &prompt,
    )
    .await
    .and_then(|raw| {
        let cleaned = raw
            .trim()
            .trim_matches('`')
            .trim()
            .lines()
            .next()
            .unwrap_or("")
            .trim()
            .to_string();
        if cleaned.is_empty() {
            return Err(AppError::recoverable(
                ErrorSource::Git,
                "git.branch_name.empty",
                "The model returned an empty branch name. Try again.",
            ));
        }
        Ok(cleaned)
    })
}

async fn load_workspace(state: &AppState, workspace_id: &str) -> Result<WorkspaceRecord, AppError> {
    workspace_repo::find_by_id(&state.pool, workspace_id)
        .await?
        .ok_or_else(|| AppError::not_found(ErrorSource::Workspace, "workspace"))
}

async fn authorize_and_run_git_mutation<F, Fut>(
    state: &AppState,
    workspace: &WorkspaceRecord,
    action: GitMutationAction,
    input: &serde_json::Value,
    approved: bool,
    run: F,
) -> Result<GitMutationResponseDto, AppError>
where
    F: FnOnce() -> Fut,
    Fut: std::future::Future<Output = Result<(GitCommandResultDto, GitSnapshotDto), AppError>>,
{
    let policy_engine = PolicyEngine::new(state.pool.clone());
    let check = policy_engine
        .evaluate(
            action.tool_name(),
            input,
            Some(&workspace.canonical_path),
            &[],
            "default",
            None,
        )
        .await?;
    let policy_json = serde_json::to_string(&check).unwrap_or_default();

    match &check.verdict {
        PolicyVerdict::Deny { reason } => {
            let error = git_policy_error("git.policy.denied", reason.clone(), false);
            record_git_audit(
                &state.pool,
                workspace,
                action,
                "git_policy_denied",
                &policy_json,
                &serde_json::json!({ "reason": reason }).to_string(),
            )
            .await?;
            return Err(error);
        }
        PolicyVerdict::RequireApproval { reason } if !approved => {
            record_git_audit(
                &state.pool,
                workspace,
                action,
                "git_approval_required",
                &policy_json,
                &serde_json::json!({ "reason": reason }).to_string(),
            )
            .await?;

            return Ok(GitMutationResponseDto::ApprovalRequired {
                action,
                reason: reason.clone(),
            });
        }
        PolicyVerdict::RequireApproval { .. } => {
            record_git_audit(
                &state.pool,
                workspace,
                action,
                "git_approval_granted",
                &policy_json,
                &serde_json::json!({ "approved": true }).to_string(),
            )
            .await?;
        }
        PolicyVerdict::AutoAllow => {}
    }

    match run().await {
        Ok((result, snapshot)) => {
            let result_json = serde_json::to_string(&result).unwrap_or_default();
            record_git_audit(
                &state.pool,
                workspace,
                action,
                "git_completed",
                &policy_json,
                &result_json,
            )
            .await?;

            Ok(GitMutationResponseDto::Completed { result, snapshot })
        }
        Err(error) => {
            record_git_audit(
                &state.pool,
                workspace,
                action,
                "git_failed",
                &policy_json,
                &serde_json::json!({
                    "errorCode": error.error_code,
                    "message": error.user_message,
                })
                .to_string(),
            )
            .await?;
            Err(error)
        }
    }
}

async fn record_git_audit(
    pool: &sqlx::SqlitePool,
    workspace: &WorkspaceRecord,
    action: GitMutationAction,
    audit_action: &str,
    policy_json: &str,
    result_json: &str,
) -> Result<(), AppError> {
    audit_repo::insert(
        pool,
        &audit_repo::AuditInsert {
            actor_type: "user".to_string(),
            actor_id: None,
            source: "git_panel".to_string(),
            workspace_id: Some(workspace.id.clone()),
            thread_id: None,
            run_id: None,
            tool_call_id: None,
            action: audit_action.to_string(),
            target_type: Some("git_action".to_string()),
            target_id: Some(action.as_str().to_string()),
            policy_check_json: Some(policy_json.to_string()),
            result_json: Some(result_json.to_string()),
        },
    )
    .await
}

fn git_policy_error(code: &str, message: String, retryable: bool) -> AppError {
    AppError {
        error_code: code.to_string(),
        category: ErrorCategory::Recoverable,
        source: ErrorSource::Git,
        user_message: message,
        detail: None,
        retryable,
    }
}

fn select_commit_message_model_role(
    raw_plan: &RuntimeModelPlan,
) -> Result<RuntimeModelRole, AppError> {
    raw_plan
        .lightweight
        .clone()
        .or_else(|| raw_plan.auxiliary.clone())
        .or_else(|| raw_plan.primary.clone())
        .ok_or_else(|| {
            AppError::recoverable(
                ErrorSource::Settings,
                "settings.commit_message.model_missing",
                "Select an enabled lite, assistant, or primary model in the current profile before generating a commit message.",
            )
        })
}

fn select_commit_message_changes(
    snapshot: &GitSnapshotDto,
) -> Result<(&'static str, bool, Vec<GitFileChangeDto>), AppError> {
    if !snapshot.staged_files.is_empty() {
        return Ok((
            "staged changes only",
            true,
            snapshot
                .staged_files
                .iter()
                .take(COMMIT_MESSAGE_FILE_LIMIT)
                .cloned()
                .collect(),
        ));
    }

    let working_tree_changes = snapshot
        .unstaged_files
        .iter()
        .chain(snapshot.untracked_files.iter())
        .take(COMMIT_MESSAGE_FILE_LIMIT)
        .cloned()
        .collect::<Vec<_>>();

    if !working_tree_changes.is_empty() {
        return Ok(("working tree changes", false, working_tree_changes));
    }

    Err(AppError::recoverable(
        ErrorSource::Git,
        "git.commit_message.no_changes",
        "There are no staged or working tree changes to summarize.",
    ))
}

fn normalize_commit_message_language(value: Option<&str>) -> String {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("English")
        .to_string()
}

fn normalize_commit_message_prompt(value: Option<&str>) -> String {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(COMMIT_MESSAGE_GENERATOR_PROMPT)
        .to_string()
}

async fn build_commit_message_prompt(
    state: &AppState,
    workspace: &WorkspaceRecord,
    workspace_path: &str,
    source_label: &str,
    staged: bool,
    changes: &[GitFileChangeDto],
    language: &str,
    prompt: Option<&str>,
) -> Result<String, AppError> {
    let change_summary = changes
        .iter()
        .map(|change| {
            format!(
                "- {} [{}] (+{} -{})",
                change.path,
                git_change_kind_label(change),
                change.additions,
                change.deletions
            )
        })
        .collect::<Vec<_>>()
        .join("\n");

    let mut remaining_budget = COMMIT_MESSAGE_DIFF_CHAR_BUDGET;
    let mut rendered_diffs = Vec::new();

    for change in changes {
        if remaining_budget == 0 {
            break;
        }

        let diff = state
            .git_manager
            .get_diff(workspace_path, &change.path, staged)
            .await?;
        let rendered = render_diff_for_commit_prompt(&diff);

        if rendered.len() > remaining_budget {
            rendered_diffs.push(truncate_to_char_boundary(&rendered, remaining_budget));
            break;
        }

        remaining_budget -= rendered.len();
        rendered_diffs.push(rendered);
    }

    let file_count_note = if changes.len() == COMMIT_MESSAGE_FILE_LIMIT {
        "\n- Only the first 24 matching files are included when the change set is larger."
    } else {
        ""
    };
    let effective_prompt = normalize_commit_message_prompt(prompt);
    let runtime_language_rules = format!(
        "Runtime language control:\n- Configured commit message language: {language}\n- Output the entire commit message in {language}.\n- Treat this runtime language control as higher priority than any conflicting language instructions inside the base prompt.\n- Do not mix languages within the same message."
    );

    Ok(format!(
        "# {runtime_language_rules}\n\n# Base prompt:\n\n{effective_prompt}\n\n# Git context:\n- Workspace: {}\n- Input source: {source_label}{file_count_note}\n\n# Changed files:\n{change_summary}\n\n# Detailed diffs:\n```bash\n{}\n```\n\n",
        workspace.canonical_path,
        rendered_diffs.join("\n\n")
    ))
}

fn git_change_kind_label(change: &GitFileChangeDto) -> &'static str {
    match change.status {
        crate::model::git::GitChangeKind::Added => "added",
        crate::model::git::GitChangeKind::Deleted => "deleted",
        crate::model::git::GitChangeKind::Renamed => "renamed",
        crate::model::git::GitChangeKind::Typechange => "typechange",
        crate::model::git::GitChangeKind::Unmerged => "unmerged",
        crate::model::git::GitChangeKind::Modified => "modified",
    }
}

fn render_diff_for_commit_prompt(diff: &GitDiffDto) -> String {
    if diff.is_binary {
        return format!(
            "File: {}\nStatus: binary {}\n",
            diff.path,
            git_diff_status_label(diff)
        );
    }

    let mut rendered = format!(
        "File: {}\nStatus: {}\nScope: {}\n",
        diff.path,
        git_diff_status_label(diff),
        if diff.staged {
            "staged"
        } else {
            "working_tree"
        }
    );

    if let Some(old_path) = diff.old_path.as_deref() {
        rendered.push_str(&format!("--- {old_path}\n"));
    }
    if let Some(new_path) = diff.new_path.as_deref() {
        rendered.push_str(&format!("+++ {new_path}\n"));
    }

    for hunk in &diff.hunks {
        rendered.push_str(&hunk.header);
        rendered.push('\n');

        for line in &hunk.lines {
            let prefix = match line.kind {
                crate::model::git::GitDiffLineKind::Add => '+',
                crate::model::git::GitDiffLineKind::Remove => '-',
                crate::model::git::GitDiffLineKind::Context => ' ',
            };
            rendered.push(prefix);
            rendered.push_str(&line.text);
            rendered.push('\n');
        }
    }

    rendered.trim_end().to_string()
}

fn git_diff_status_label(diff: &GitDiffDto) -> &'static str {
    match diff.status {
        crate::model::git::GitChangeKind::Added => "added",
        crate::model::git::GitChangeKind::Deleted => "deleted",
        crate::model::git::GitChangeKind::Renamed => "renamed",
        crate::model::git::GitChangeKind::Typechange => "typechange",
        crate::model::git::GitChangeKind::Unmerged => "unmerged",
        crate::model::git::GitChangeKind::Modified => "modified",
    }
}

/// Files at or below this threshold get detailed diffs in the branch name prompt.
const BRANCH_NAME_DIFF_FILE_THRESHOLD: usize = 8;
/// Character budget for diffs in the branch name prompt (smaller than commit messages).
const BRANCH_NAME_DIFF_CHAR_BUDGET: usize = 16_000;

async fn build_branch_name_prompt(
    state: &AppState,
    workspace_path: &str,
    snapshot: &GitSnapshotDto,
    branches: &[GitBranchDto],
) -> Result<String, AppError> {
    let current_branch = snapshot.head_ref.as_deref().unwrap_or("(detached HEAD)");

    // Collect existing local branch names for naming convention analysis
    let local_branch_names: Vec<&str> = branches
        .iter()
        .filter(|b| !b.is_remote)
        .map(|b| b.name.as_str())
        .collect();

    // Collect changes — staged take priority (same logic as commit message)
    let (source_label, staged, changes): (&str, bool, Vec<GitFileChangeDto>) =
        if !snapshot.staged_files.is_empty() {
            ("staged changes", true, snapshot.staged_files.clone())
        } else {
            let wt: Vec<GitFileChangeDto> = snapshot
                .unstaged_files
                .iter()
                .chain(snapshot.untracked_files.iter())
                .cloned()
                .collect();
            ("working tree changes", false, wt)
        };

    // File summary (always included)
    let change_summary = changes
        .iter()
        .take(20)
        .map(|f| {
            format!(
                "- {} [{}] (+{} -{})",
                f.path,
                git_change_kind_label(f),
                f.additions,
                f.deletions,
            )
        })
        .collect::<Vec<_>>()
        .join("\n");

    let changes_section = if change_summary.is_empty() {
        "No local changes detected.".to_string()
    } else {
        format!("Source: {source_label}\n{change_summary}")
    };

    // Detailed diffs — only when file count is small enough to keep context short
    let diffs_section = if !changes.is_empty() && changes.len() <= BRANCH_NAME_DIFF_FILE_THRESHOLD {
        let mut remaining_budget = BRANCH_NAME_DIFF_CHAR_BUDGET;
        let mut rendered_diffs = Vec::new();

        for change in &changes {
            if remaining_budget == 0 {
                break;
            }

            let diff = state
                .git_manager
                .get_diff(workspace_path, &change.path, staged)
                .await?;
            let rendered = render_diff_for_commit_prompt(&diff);

            if rendered.len() > remaining_budget {
                rendered_diffs.push(truncate_to_char_boundary(&rendered, remaining_budget));
                break;
            }

            remaining_budget -= rendered.len();
            rendered_diffs.push(rendered);
        }

        if rendered_diffs.is_empty() {
            String::new()
        } else {
            format!(
                "\n## Detailed diffs\n```\n{}\n```\n",
                rendered_diffs.join("\n\n")
            )
        }
    } else {
        String::new()
    };

    let branches_section = if local_branch_names.is_empty() {
        "No existing branches.".to_string()
    } else {
        local_branch_names
            .iter()
            .take(30)
            .cloned()
            .collect::<Vec<_>>()
            .join(", ")
    };

    Ok(format!(
        r#"Generate a Git branch name for the following context.

## Rules
- Follow the naming convention of existing branches (e.g. feat/xxx, fix/xxx, chore/xxx).
- If no convention is apparent, use conventional style: <type>/<short-description>
- Types: feat, fix, refactor, chore, docs, style, test, perf, ci, build
- Use lowercase, hyphens for spaces, no special characters.
- Keep it concise (under 40 chars total).
- Return ONLY the branch name. No explanation.

## Current branch
{current_branch}

## Existing local branches
{branches_section}

## Local changes
{changes_section}
{diffs_section}"#
    ))
}

async fn generate_commit_message(
    model_role: &ResolvedModelRole,
    prompt: &str,
) -> Result<String, AppError> {
    generate_with_lite_model(
        model_role,
        "You generate a single commit message from Git changes. Return only the commit message.",
        prompt,
    )
    .await
    .and_then(|raw| {
        normalize_generated_commit_message(&raw).ok_or_else(|| {
            AppError::recoverable(
                ErrorSource::Settings,
                "settings.commit_message.empty",
                "The model returned an empty commit message. Try again.",
            )
        })
    })
}

async fn generate_with_lite_model(
    model_role: &ResolvedModelRole,
    system_prompt: &str,
    user_prompt: &str,
) -> Result<String, AppError> {
    // Lightweight generation (commit messages, branch names) does not benefit from
    // reasoning/thinking tokens.  Disable reasoning so the protocol layer omits
    // thinking/reasoning parameters, preventing reasoning tokens from consuming
    // the COMMIT_MESSAGE_MAX_TOKENS budget.
    // If the original model had reasoning enabled, bump max_tokens as a fallback —
    // some reasoning-only models ignore the disable and still produce reasoning tokens.
    let was_reasoning = model_role.model.reasoning;
    let mut model_role = model_role.clone();
    model_role.model.reasoning = false;

    let provider = get_provider(&model_role.model.provider).ok_or_else(|| {
        AppError::recoverable(
            ErrorSource::Settings,
            "settings.commit_message.provider_missing",
            format!(
                "Provider type '{:?}' is not registered for commit message generation.",
                model_role.model.provider
            ),
        )
    })?;

    let context = TiyContext {
        system_prompt: Some(system_prompt.to_string()),
        messages: vec![TiyMessage::User(UserMessage::text(user_prompt.to_string()))],
        tools: None,
    };

    let options = TiyStreamOptions {
        api_key: model_role.api_key.clone(),
        max_tokens: Some(if was_reasoning {
            COMMIT_MESSAGE_MAX_TOKENS_REASONING
        } else {
            COMMIT_MESSAGE_MAX_TOKENS
        }),
        headers: Some(tiycode_default_headers()),
        on_payload: build_provider_options_payload_hook(model_role.provider_options.clone()),
        ..TiyStreamOptions::default()
    };

    let completion = provider
        .stream(&model_role.model, &context, options)
        .try_result(COMMIT_MESSAGE_TIMEOUT)
        .await;

    let message = match completion {
        Some(message) => message,
        None => {
            return Err(AppError::recoverable(
                ErrorSource::Settings,
                "settings.commit_message.timeout",
                "Commit message generation timed out. Try again with fewer changes or a faster lite model.",
            ))
        }
    };

    if message.stop_reason == StopReason::Error {
        let detail = message
            .error_message
            .clone()
            .unwrap_or_else(|| "commit message generation failed".to_string());
        return Err(AppError::recoverable(
            ErrorSource::Settings,
            "settings.commit_message.failed",
            detail,
        ));
    }

    normalize_generated_commit_message(&message.text_content()).ok_or_else(|| {
        AppError::recoverable(
            ErrorSource::Settings,
            "settings.lite_model.empty",
            "The model returned an empty response. Try again.",
        )
    })
}

fn build_provider_options_payload_hook(
    provider_options: Option<serde_json::Value>,
) -> Option<OnPayloadFn> {
    let provider_options = provider_options?;

    Some(Arc::new(move |payload, _model| {
        let provider_options = provider_options.clone();
        Box::pin(async move {
            let mut merged = payload;
            merge_json_value(&mut merged, &provider_options);
            Some(merged)
        })
    }))
}

fn merge_json_value(base: &mut serde_json::Value, patch: &serde_json::Value) {
    match (base, patch) {
        (serde_json::Value::Object(base_map), serde_json::Value::Object(patch_map)) => {
            for (key, patch_value) in patch_map {
                if let Some(base_value) = base_map.get_mut(key) {
                    merge_json_value(base_value, patch_value);
                } else {
                    base_map.insert(key.clone(), patch_value.clone());
                }
            }
        }
        (base_value, patch_value) => {
            *base_value = patch_value.clone();
        }
    }
}

fn normalize_generated_commit_message(raw: &str) -> Option<String> {
    let mut cleaned = raw.trim().to_string();

    if cleaned.starts_with("```") && cleaned.ends_with("```") {
        let first_newline = cleaned.find('\n')?;
        cleaned = cleaned[first_newline + 1..cleaned.len().saturating_sub(3)]
            .trim()
            .to_string();
    }

    for prefix in [
        "Commit message:",
        "commit message:",
        "提交信息：",
        "提交信息:",
    ] {
        if let Some(stripped) = cleaned.strip_prefix(prefix) {
            cleaned = stripped.trim().to_string();
            break;
        }
    }

    let cleaned = cleaned.trim().to_string();
    if cleaned.is_empty() {
        return None;
    }

    Some(cleaned)
}

fn truncate_to_char_boundary(value: &str, max_bytes: usize) -> String {
    if value.len() <= max_bytes {
        return value.to_string();
    }

    let mut end = max_bytes;
    while !value.is_char_boundary(end) {
        end -= 1;
    }

    value[..end].trim_end().to_string()
}
