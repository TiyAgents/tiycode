//! Task board manager for thread task tracking.

use chrono::Utc;
use sqlx::SqlitePool;
use uuid::Uuid;

use crate::model::errors::{AppError, ErrorSource};
use crate::model::task_board::{CreateTaskInput, TaskBoardDto, TaskBoardStatus, UpdateTaskAction};
use crate::model::task_item::{TaskItemDto, TaskItemRecord, TaskStage};
use crate::persistence::repo::{task_board_repo, task_item_repo};

/// Create a new task board with steps for a thread.
///
/// The entire operation (auto-complete previous board, insert board, insert items,
/// set active task) runs inside a single transaction.
pub async fn create_task_board(
    pool: &SqlitePool,
    thread_id: &str,
    input: &CreateTaskInput,
) -> Result<TaskBoardDto, AppError> {
    let board_id = Uuid::new_v4().to_string();
    let now = Utc::now().to_rfc3339();

    let mut tx = pool.begin().await?;

    // Auto-complete any existing active task board for this thread
    sqlx::query(
        "UPDATE task_boards SET status = 'completed', updated_at = ? WHERE thread_id = ? AND status = 'active'"
    )
    .bind(&now)
    .bind(thread_id)
    .execute(&mut *tx)
    .await?;

    // Insert the new board
    sqlx::query(
        "INSERT INTO task_boards (id, thread_id, title, status, active_task_id, created_at, updated_at)
         VALUES (?, ?, ?, 'active', NULL, ?, ?)"
    )
    .bind(&board_id)
    .bind(thread_id)
    .bind(&input.title)
    .bind(&now)
    .bind(&now)
    .execute(&mut *tx)
    .await?;

    // Insert task items; first step is auto-started (InProgress)
    let mut first_task_id: Option<String> = None;
    for (idx, step) in input.steps.iter().enumerate() {
        let task_id = Uuid::new_v4().to_string();
        let stage = if idx == 0 {
            first_task_id = Some(task_id.clone());
            TaskStage::InProgress
        } else {
            TaskStage::Pending
        };

        sqlx::query(
            "INSERT INTO task_items (id, task_board_id, description, stage, sort_order, error_detail, created_at, updated_at)
             VALUES (?, ?, ?, ?, ?, NULL, ?, ?)"
        )
        .bind(&task_id)
        .bind(&board_id)
        .bind(&step.description)
        .bind(stage.as_str())
        .bind(idx as i32)
        .bind(&now)
        .bind(&now)
        .execute(&mut *tx)
        .await?;
    }

    // Set active task to first task
    if let Some(ref first_id) = first_task_id {
        sqlx::query("UPDATE task_boards SET active_task_id = ?, updated_at = ? WHERE id = ?")
            .bind(first_id)
            .bind(&now)
            .bind(&board_id)
            .execute(&mut *tx)
            .await?;
    }

    tx.commit().await?;

    // Load and return the DTO
    load_task_board_dto(pool, &board_id).await
}

/// Update a task board or its items based on action.
pub async fn update_task_board(
    pool: &SqlitePool,
    thread_id: &str,
    input: &crate::model::task_board::UpdateTaskInput,
) -> Result<TaskBoardDto, AppError> {
    // Verify the task board belongs to this thread
    let board = task_board_repo::find_by_id(pool, &input.task_board_id)
        .await?
        .ok_or_else(|| AppError::not_found(ErrorSource::Thread, "Task board not found"))?;

    if board.thread_id != thread_id {
        return Err(AppError::not_found(
            ErrorSource::Thread,
            "Task board not found in this thread",
        ));
    }

    match &input.action {
        UpdateTaskAction::StartStep { step_id } => {
            // Board must be active
            if board.status != TaskBoardStatus::Active {
                return Err(AppError::validation(
                    ErrorSource::Thread,
                    "Cannot update steps on a non-active task board",
                ));
            }

            let task = task_item_repo::find_by_id(pool, step_id)
                .await?
                .ok_or_else(|| AppError::not_found(ErrorSource::Thread, "Task step not found"))?;

            if task.task_board_id != input.task_board_id {
                return Err(AppError::not_found(
                    ErrorSource::Thread,
                    "Task step not in this board",
                ));
            }

            // Step must be Pending
            if task.stage != TaskStage::Pending {
                return Err(AppError::validation(
                    ErrorSource::Thread,
                    &format!(
                        "Cannot start step: current stage is '{}', expected 'pending'",
                        task.stage.as_str()
                    ),
                ));
            }

            let items = task_item_repo::list_by_task_board(pool, &input.task_board_id).await?;
            if let Some(active_step) = items
                .iter()
                .find(|item| item.stage == TaskStage::InProgress && item.id != *step_id)
            {
                return Err(AppError::validation(
                    ErrorSource::Thread,
                    &format!(
                        "Cannot start step '{}' while step '{}' is still in progress",
                        step_id, active_step.id
                    ),
                ));
            }

            task_item_repo::update_stage(pool, step_id, &TaskStage::InProgress, None).await?;
            task_board_repo::update_active_task(pool, &input.task_board_id, Some(step_id)).await?;
        }
        UpdateTaskAction::AdvanceStep { step_id } => {
            if board.status != TaskBoardStatus::Active {
                return Err(AppError::validation(
                    ErrorSource::Thread,
                    "Cannot update steps on a non-active task board",
                ));
            }

            let explicit_step_id = step_id
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty());

            let step_id = match explicit_step_id.or(board.active_task_id.as_deref()) {
                Some(step_id) => step_id,
                None => {
                    return Err(AppError::validation(
                        ErrorSource::Thread,
                        "Cannot advance task board: no active step was found",
                    ))
                }
            };

            let task = task_item_repo::find_by_id(pool, step_id)
                .await?
                .ok_or_else(|| AppError::not_found(ErrorSource::Thread, "Task step not found"))?;

            if task.task_board_id != input.task_board_id {
                return Err(AppError::not_found(
                    ErrorSource::Thread,
                    "Task step not in this board",
                ));
            }

            if task.stage != TaskStage::InProgress {
                return Err(AppError::validation(
                    ErrorSource::Thread,
                    &format!(
                        "Cannot advance step: current stage is '{}', expected 'in_progress'",
                        task.stage.as_str()
                    ),
                ));
            }

            task_item_repo::update_stage(pool, step_id, &TaskStage::Completed, None).await?;
            advance_after_step_completion(pool, &input.task_board_id).await?;
        }
        UpdateTaskAction::CompleteStep { step_id } => {
            if board.status != TaskBoardStatus::Active {
                return Err(AppError::validation(
                    ErrorSource::Thread,
                    "Cannot update steps on a non-active task board",
                ));
            }

            let task = task_item_repo::find_by_id(pool, step_id)
                .await?
                .ok_or_else(|| AppError::not_found(ErrorSource::Thread, "Task step not found"))?;

            if task.task_board_id != input.task_board_id {
                return Err(AppError::not_found(
                    ErrorSource::Thread,
                    "Task step not in this board",
                ));
            }

            // Step must be InProgress
            if task.stage != TaskStage::InProgress {
                return Err(AppError::validation(
                    ErrorSource::Thread,
                    &format!(
                        "Cannot complete step: current stage is '{}', expected 'in_progress'",
                        task.stage.as_str()
                    ),
                ));
            }

            task_item_repo::update_stage(pool, step_id, &TaskStage::Completed, None).await?;
            advance_after_step_completion(pool, &input.task_board_id).await?;
        }
        UpdateTaskAction::FailStep {
            step_id,
            error_detail,
        } => {
            if board.status != TaskBoardStatus::Active {
                return Err(AppError::validation(
                    ErrorSource::Thread,
                    "Cannot update steps on a non-active task board",
                ));
            }

            let task = task_item_repo::find_by_id(pool, step_id)
                .await?
                .ok_or_else(|| AppError::not_found(ErrorSource::Thread, "Task step not found"))?;

            if task.task_board_id != input.task_board_id {
                return Err(AppError::not_found(
                    ErrorSource::Thread,
                    "Task step not in this board",
                ));
            }

            // Step must be InProgress or Pending
            if task.stage != TaskStage::InProgress && task.stage != TaskStage::Pending {
                return Err(AppError::validation(
                    ErrorSource::Thread,
                    &format!("Cannot fail step: current stage is '{}', expected 'in_progress' or 'pending'", task.stage.as_str()),
                ));
            }

            task_item_repo::update_stage(pool, step_id, &TaskStage::Failed, Some(error_detail))
                .await?;
            activate_next_pending_task(pool, &input.task_board_id).await?;
        }
        UpdateTaskAction::CompleteBoard => {
            if board.status != TaskBoardStatus::Active {
                return Err(AppError::validation(
                    ErrorSource::Thread,
                    "Task board is already completed or abandoned",
                ));
            }
            complete_board(pool, &input.task_board_id).await?;
        }
        UpdateTaskAction::AbandonBoard { reason: _ } => {
            if board.status != TaskBoardStatus::Active {
                return Err(AppError::validation(
                    ErrorSource::Thread,
                    "Task board is already completed or abandoned",
                ));
            }
            task_board_repo::update_status(pool, &input.task_board_id, &TaskBoardStatus::Abandoned)
                .await?;
        }
    }

    load_task_board_dto(pool, &input.task_board_id).await
}

/// Reconcile the active task board for a thread when a run reaches a terminal state.
///
/// This keeps `active_task_id` aligned with the real in-progress step and auto-completes the
/// board when every step is complete. If no step is currently in progress but pending work
/// remains, the next pending step is started so later runs can resume from a consistent state.
pub async fn reconcile_active_task_board(
    pool: &SqlitePool,
    thread_id: &str,
) -> Result<Option<TaskBoardDto>, AppError> {
    let Some(board) = task_board_repo::find_active_by_thread(pool, thread_id).await? else {
        return Ok(None);
    };

    let items = task_item_repo::list_by_task_board(pool, &board.id).await?;
    if items.is_empty() {
        if board.active_task_id.is_some() {
            task_board_repo::update_active_task(pool, &board.id, None).await?;
            return Ok(Some(load_task_board_dto(pool, &board.id).await?));
        }
        return Ok(None);
    }

    if all_steps_terminal(&items) {
        complete_board(pool, &board.id).await?;
        return Ok(Some(load_task_board_dto(pool, &board.id).await?));
    }

    if let Some(active_step) = first_in_progress_task(&items) {
        if board.active_task_id.as_deref() != Some(active_step.id.as_str()) {
            task_board_repo::update_active_task(pool, &board.id, Some(&active_step.id)).await?;
            return Ok(Some(load_task_board_dto(pool, &board.id).await?));
        }
        return Ok(None);
    }

    if let Some(next_pending) = first_pending_task(&items) {
        task_item_repo::update_stage(pool, &next_pending.id, &TaskStage::InProgress, None).await?;
        task_board_repo::update_active_task(pool, &board.id, Some(&next_pending.id)).await?;
        return Ok(Some(load_task_board_dto(pool, &board.id).await?));
    }

    if board.active_task_id.is_some() {
        task_board_repo::update_active_task(pool, &board.id, None).await?;
        return Ok(Some(load_task_board_dto(pool, &board.id).await?));
    }

    Ok(None)
}

async fn advance_after_step_completion(pool: &SqlitePool, board_id: &str) -> Result<(), AppError> {
    let items = task_item_repo::list_by_task_board(pool, board_id).await?;
    if let Some(next_pending) = first_pending_task(&items) {
        task_item_repo::update_stage(pool, &next_pending.id, &TaskStage::InProgress, None).await?;
        task_board_repo::update_active_task(pool, board_id, Some(&next_pending.id)).await?;
    } else {
        complete_board(pool, board_id).await?;
    }

    Ok(())
}

async fn activate_next_pending_task(pool: &SqlitePool, board_id: &str) -> Result<(), AppError> {
    let items = task_item_repo::list_by_task_board(pool, board_id).await?;
    if let Some(next_pending) = first_pending_task(&items) {
        task_item_repo::update_stage(pool, &next_pending.id, &TaskStage::InProgress, None).await?;
        task_board_repo::update_active_task(pool, board_id, Some(&next_pending.id)).await?;
    } else if all_steps_terminal(&items) {
        complete_board(pool, board_id).await?;
    } else {
        task_board_repo::update_active_task(pool, board_id, None).await?;
    }

    Ok(())
}

async fn complete_board(pool: &SqlitePool, board_id: &str) -> Result<(), AppError> {
    task_board_repo::update_active_task(pool, board_id, None).await?;
    task_board_repo::update_status(pool, board_id, &TaskBoardStatus::Completed).await?;
    Ok(())
}

fn first_pending_task(items: &[TaskItemRecord]) -> Option<&TaskItemRecord> {
    items.iter().find(|item| item.stage == TaskStage::Pending)
}

fn all_steps_terminal(items: &[TaskItemRecord]) -> bool {
    !items.is_empty()
        && items
            .iter()
            .all(|item| item.stage == TaskStage::Completed || item.stage == TaskStage::Failed)
}

fn first_in_progress_task(items: &[TaskItemRecord]) -> Option<&TaskItemRecord> {
    items
        .iter()
        .find(|item| item.stage == TaskStage::InProgress)
}

/// Load a task board DTO with all its tasks.
pub async fn load_task_board_dto(
    pool: &SqlitePool,
    board_id: &str,
) -> Result<TaskBoardDto, AppError> {
    let board = task_board_repo::find_by_id(pool, board_id)
        .await?
        .ok_or_else(|| AppError::not_found(ErrorSource::Thread, "Task board not found"))?;

    let items = task_item_repo::list_by_task_board(pool, board_id).await?;
    let tasks: Vec<TaskItemDto> = items.into_iter().map(|r| r.into()).collect();

    Ok(TaskBoardDto {
        id: board.id,
        thread_id: board.thread_id,
        title: board.title,
        status: board.status,
        active_task_id: board.active_task_id,
        tasks,
        created_at: board.created_at,
        updated_at: board.updated_at,
    })
}

/// Load all task boards for a thread.
pub async fn load_thread_task_boards(
    pool: &SqlitePool,
    thread_id: &str,
) -> Result<Vec<TaskBoardDto>, AppError> {
    let boards = task_board_repo::list_by_thread(pool, thread_id).await?;

    if boards.is_empty() {
        return Ok(Vec::new());
    }

    let board_ids: Vec<String> = boards.iter().map(|b| b.id.clone()).collect();
    let all_tasks = task_item_repo::list_dtos_by_task_boards(pool, &board_ids).await?;

    // Group tasks by board
    let mut result = Vec::new();
    for board in boards {
        let tasks: Vec<TaskItemDto> = all_tasks
            .iter()
            .filter(|t| t.task_board_id == board.id)
            .cloned()
            .collect();

        result.push(TaskBoardDto {
            id: board.id,
            thread_id: board.thread_id,
            title: board.title,
            status: board.status,
            active_task_id: board.active_task_id,
            tasks,
            created_at: board.created_at,
            updated_at: board.updated_at,
        });
    }

    Ok(result)
}

/// Get active task board for a thread.
pub async fn get_active_task_board(
    pool: &SqlitePool,
    thread_id: &str,
) -> Result<Option<TaskBoardDto>, AppError> {
    let board = task_board_repo::find_active_by_thread(pool, thread_id).await?;

    match board {
        Some(b) => {
            let dto = load_task_board_dto(pool, &b.id).await?;
            Ok(Some(dto))
        }
        None => Ok(None),
    }
}
