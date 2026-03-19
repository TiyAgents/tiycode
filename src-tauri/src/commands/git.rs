use tauri::{ipc::Channel, State};

use crate::core::app_state::AppState;
use crate::core::policy_engine::{PolicyEngine, PolicyVerdict};
use crate::ipc::frontend_channels::GitStreamEvent;
use crate::model::errors::{AppError, ErrorCategory, ErrorSource};
use crate::model::git::{
    GitCommandResultDto, GitCommitSummaryDto, GitDiffDto, GitFileStatusDto, GitMutationAction,
    GitMutationResponseDto, GitSnapshotDto,
};
use crate::model::workspace::WorkspaceRecord;
use crate::persistence::repo::{audit_repo, workspace_repo};

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

    tokio::spawn(async move {
        while let Ok(event) = receiver.recv().await {
            if on_event.send(event).is_err() {
                break;
            }
        }
    });

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
            "default",
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
