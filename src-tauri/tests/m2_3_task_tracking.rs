//! M2.3 — Task Tracking tests
//!
//! Acceptance criteria:
//! - Task boards can be created with steps
//! - Task steps can be updated (start, complete, fail)
//! - Thread snapshot includes task boards
//! - Task boards are deleted when thread is deleted
//! - State machine guards prevent invalid transitions

mod test_helpers;

use sqlx::Row;
use tiycode::core::task_board_manager;
use tiycode::core::thread_manager::ThreadManager;
use tiycode::model::task_board::{
    CreateTaskInput, CreateTaskStep, QueryTaskScope, TaskBoardStatus, UpdateTaskAction,
    UpdateTaskInput,
};
use tiycode::model::task_item::TaskStage;

// =========================================================================
// T2.3.1 — Task Board CRUD
// =========================================================================

#[tokio::test]
async fn test_create_task_board() {
    let pool = test_helpers::setup_test_pool().await;
    test_helpers::seed_workspace(&pool, "ws-task", "/tmp/task").await;
    test_helpers::seed_thread(&pool, "t-task-1", "ws-task", None).await;

    let input = CreateTaskInput {
        title: "Implement Feature X".to_string(),
        steps: vec![
            CreateTaskStep {
                description: "Design API".to_string(),
            },
            CreateTaskStep {
                description: "Write code".to_string(),
            },
            CreateTaskStep {
                description: "Add tests".to_string(),
            },
        ],
    };

    let result = task_board_manager::create_task_board(&pool, "t-task-1", &input).await;
    assert!(
        result.is_ok(),
        "Failed to create task board: {:?}",
        result.err()
    );

    let board = result.unwrap();
    assert_eq!(board.title, "Implement Feature X");
    assert_eq!(board.status, TaskBoardStatus::Active);
    assert_eq!(board.tasks.len(), 3);
    assert!(
        board.active_task_id.is_some(),
        "First task should be active"
    );

    // First task is auto-started (InProgress), rest are Pending
    assert_eq!(board.tasks[0].stage, TaskStage::InProgress);
    assert_eq!(board.tasks[1].stage, TaskStage::Pending);
    assert_eq!(board.tasks[2].stage, TaskStage::Pending);
}

#[tokio::test]
async fn test_auto_complete_previous_board() {
    let pool = test_helpers::setup_test_pool().await;
    test_helpers::seed_workspace(&pool, "ws-auto", "/tmp/auto").await;
    test_helpers::seed_thread(&pool, "t-auto", "ws-auto", None).await;

    // Create first board
    let input1 = CreateTaskInput {
        title: "First Task".to_string(),
        steps: vec![CreateTaskStep {
            description: "Step 1".to_string(),
        }],
    };
    let board1 = task_board_manager::create_task_board(&pool, "t-auto", &input1)
        .await
        .unwrap();
    assert_eq!(board1.status, TaskBoardStatus::Active);

    // Create second board - should auto-complete first
    let input2 = CreateTaskInput {
        title: "Second Task".to_string(),
        steps: vec![CreateTaskStep {
            description: "Step 2".to_string(),
        }],
    };
    let board2 = task_board_manager::create_task_board(&pool, "t-auto", &input2)
        .await
        .unwrap();
    assert_eq!(board2.status, TaskBoardStatus::Active);

    // Verify first board is now completed
    let boards = task_board_manager::load_thread_task_boards(&pool, "t-auto")
        .await
        .unwrap();
    assert_eq!(boards.len(), 2);
    assert_eq!(boards[0].status, TaskBoardStatus::Completed);
    assert_eq!(boards[1].status, TaskBoardStatus::Active);
}

// =========================================================================
// T2.3.2 — Task Step Updates
// =========================================================================

#[tokio::test]
async fn test_update_task_start_step() {
    let pool = test_helpers::setup_test_pool().await;
    test_helpers::seed_workspace(&pool, "ws-start", "/tmp/start").await;
    test_helpers::seed_thread(&pool, "t-start", "ws-start", None).await;

    let input = CreateTaskInput {
        title: "Test Task".to_string(),
        steps: vec![
            CreateTaskStep {
                description: "Step A".to_string(),
            },
            CreateTaskStep {
                description: "Step B".to_string(),
            },
        ],
    };
    let board = task_board_manager::create_task_board(&pool, "t-start", &input)
        .await
        .unwrap();

    // First step is already InProgress after creation; starting a different step should be rejected
    let step_b_id = &board.tasks[1].id;
    let update = UpdateTaskInput {
        task_board_id: board.id.clone(),
        action: UpdateTaskAction::StartStep {
            step_id: step_b_id.clone(),
        },
    };

    let result = task_board_manager::update_task_board(&pool, "t-start", &update).await;
    assert!(
        result.is_err(),
        "Should reject starting a second active step"
    );
}

#[tokio::test]
async fn test_update_task_complete_step() {
    let pool = test_helpers::setup_test_pool().await;
    test_helpers::seed_workspace(&pool, "ws-complete", "/tmp/complete").await;
    test_helpers::seed_thread(&pool, "t-complete", "ws-complete", None).await;

    let input = CreateTaskInput {
        title: "Test Task".to_string(),
        steps: vec![
            CreateTaskStep {
                description: "Step A".to_string(),
            },
            CreateTaskStep {
                description: "Step B".to_string(),
            },
        ],
    };
    let board = task_board_manager::create_task_board(&pool, "t-complete", &input)
        .await
        .unwrap();
    let step_id = &board.tasks[0].id;

    // First step is already InProgress — complete it directly
    let complete = UpdateTaskInput {
        task_board_id: board.id.clone(),
        action: UpdateTaskAction::CompleteStep {
            step_id: step_id.clone(),
        },
    };
    let updated = task_board_manager::update_task_board(&pool, "t-complete", &complete)
        .await
        .unwrap();

    assert_eq!(updated.tasks[0].stage, TaskStage::Completed);
    assert_eq!(updated.tasks[1].stage, TaskStage::InProgress);
    // Next task should be active and actually in progress
    assert_eq!(updated.active_task_id.as_ref(), Some(&updated.tasks[1].id));
}

#[tokio::test]
async fn test_update_task_complete_last_step_auto_completes_board() {
    let pool = test_helpers::setup_test_pool().await;
    test_helpers::seed_workspace(&pool, "ws-last", "/tmp/last").await;
    test_helpers::seed_thread(&pool, "t-last", "ws-last", None).await;

    let input = CreateTaskInput {
        title: "Single Step Task".to_string(),
        steps: vec![CreateTaskStep {
            description: "Only Step".to_string(),
        }],
    };
    let board = task_board_manager::create_task_board(&pool, "t-last", &input)
        .await
        .unwrap();

    let updated = task_board_manager::update_task_board(
        &pool,
        "t-last",
        &UpdateTaskInput {
            task_board_id: board.id.clone(),
            action: UpdateTaskAction::CompleteStep {
                step_id: board.tasks[0].id.clone(),
            },
        },
    )
    .await
    .unwrap();

    assert_eq!(updated.status, TaskBoardStatus::Completed);
    assert_eq!(updated.tasks[0].stage, TaskStage::Completed);
    assert_eq!(updated.active_task_id, None);
}

#[tokio::test]
async fn test_update_task_advance_step_uses_active_step() {
    let pool = test_helpers::setup_test_pool().await;
    test_helpers::seed_workspace(&pool, "ws-advance", "/tmp/advance").await;
    test_helpers::seed_thread(&pool, "t-advance", "ws-advance", None).await;

    let input = CreateTaskInput {
        title: "Advance Task".to_string(),
        steps: vec![
            CreateTaskStep {
                description: "Step A".to_string(),
            },
            CreateTaskStep {
                description: "Step B".to_string(),
            },
        ],
    };
    let board = task_board_manager::create_task_board(&pool, "t-advance", &input)
        .await
        .unwrap();

    let updated = task_board_manager::update_task_board(
        &pool,
        "t-advance",
        &UpdateTaskInput {
            task_board_id: board.id.clone(),
            action: UpdateTaskAction::AdvanceStep { step_id: None },
        },
    )
    .await
    .unwrap();

    assert_eq!(updated.tasks[0].stage, TaskStage::Completed);
    assert_eq!(updated.tasks[1].stage, TaskStage::InProgress);
    assert_eq!(updated.active_task_id.as_ref(), Some(&updated.tasks[1].id));
}

#[tokio::test]
async fn test_update_task_advance_step_treats_empty_step_id_as_missing() {
    let pool = test_helpers::setup_test_pool().await;
    test_helpers::seed_workspace(&pool, "ws-advance-empty", "/tmp/advance-empty").await;
    test_helpers::seed_thread(&pool, "t-advance-empty", "ws-advance-empty", None).await;

    let input = CreateTaskInput {
        title: "Advance Task Empty Step Id".to_string(),
        steps: vec![
            CreateTaskStep {
                description: "Step A".to_string(),
            },
            CreateTaskStep {
                description: "Step B".to_string(),
            },
        ],
    };
    let board = task_board_manager::create_task_board(&pool, "t-advance-empty", &input)
        .await
        .unwrap();

    let updated = task_board_manager::update_task_board(
        &pool,
        "t-advance-empty",
        &UpdateTaskInput {
            task_board_id: board.id.clone(),
            action: UpdateTaskAction::AdvanceStep {
                step_id: Some("".to_string()),
            },
        },
    )
    .await
    .unwrap();

    assert_eq!(updated.tasks[0].stage, TaskStage::Completed);
    assert_eq!(updated.tasks[1].stage, TaskStage::InProgress);
    assert_eq!(updated.active_task_id.as_ref(), Some(&updated.tasks[1].id));
}

#[tokio::test]
async fn test_update_task_fail_step() {
    let pool = test_helpers::setup_test_pool().await;
    test_helpers::seed_workspace(&pool, "ws-fail", "/tmp/fail").await;
    test_helpers::seed_thread(&pool, "t-fail", "ws-fail", None).await;

    let input = CreateTaskInput {
        title: "Test Task".to_string(),
        steps: vec![
            CreateTaskStep {
                description: "Step A".to_string(),
            },
            CreateTaskStep {
                description: "Step B".to_string(),
            },
        ],
    };
    let board = task_board_manager::create_task_board(&pool, "t-fail", &input)
        .await
        .unwrap();
    let step_id = &board.tasks[0].id;

    // First step is InProgress — fail it
    let update = UpdateTaskInput {
        task_board_id: board.id.clone(),
        action: UpdateTaskAction::FailStep {
            step_id: step_id.clone(),
            error_detail: "Something went wrong".to_string(),
        },
    };

    let updated = task_board_manager::update_task_board(&pool, "t-fail", &update)
        .await
        .unwrap();
    assert_eq!(updated.tasks[0].stage, TaskStage::Failed);
    assert_eq!(
        updated.tasks[0].error_detail,
        Some("Something went wrong".to_string())
    );
    assert_eq!(updated.tasks[1].stage, TaskStage::InProgress);
    // active_task_id should advance to the next in-progress step
    assert_eq!(updated.active_task_id.as_ref(), Some(&updated.tasks[1].id));
}

#[tokio::test]
async fn test_update_task_complete_board() {
    let pool = test_helpers::setup_test_pool().await;
    test_helpers::seed_workspace(&pool, "ws-board", "/tmp/board").await;
    test_helpers::seed_thread(&pool, "t-board", "ws-board", None).await;

    let input = CreateTaskInput {
        title: "Test Task".to_string(),
        steps: vec![CreateTaskStep {
            description: "Step A".to_string(),
        }],
    };
    let board = task_board_manager::create_task_board(&pool, "t-board", &input)
        .await
        .unwrap();

    let update = UpdateTaskInput {
        task_board_id: board.id.clone(),
        action: UpdateTaskAction::CompleteBoard,
    };

    let updated = task_board_manager::update_task_board(&pool, "t-board", &update)
        .await
        .unwrap();
    assert_eq!(updated.status, TaskBoardStatus::Completed);
}

#[tokio::test]
async fn test_query_task_active_returns_only_active_board() {
    let pool = test_helpers::setup_test_pool().await;
    test_helpers::seed_workspace(&pool, "ws-query-active", "/tmp/query-active").await;
    test_helpers::seed_thread(&pool, "t-query-active", "ws-query-active", None).await;

    let first_board = task_board_manager::create_task_board(
        &pool,
        "t-query-active",
        &CreateTaskInput {
            title: "Completed Board".to_string(),
            steps: vec![CreateTaskStep {
                description: "Step 1".to_string(),
            }],
        },
    )
    .await
    .unwrap();
    let second_board = task_board_manager::create_task_board(
        &pool,
        "t-query-active",
        &CreateTaskInput {
            title: "Active Board".to_string(),
            steps: vec![CreateTaskStep {
                description: "Step 2".to_string(),
            }],
        },
    )
    .await
    .unwrap();

    let queried = task_board_manager::query_thread_task_boards(
        &pool,
        "t-query-active",
        QueryTaskScope::Active,
    )
    .await
    .unwrap();

    assert_eq!(first_board.status, TaskBoardStatus::Active);
    assert_eq!(queried.scope, QueryTaskScope::Active);
    assert_eq!(
        queried.active_task_board_id.as_deref(),
        Some(second_board.id.as_str())
    );
    assert_eq!(queried.task_boards.len(), 1);
    assert_eq!(queried.task_boards[0].id, second_board.id);
    assert_eq!(queried.task_boards[0].status, TaskBoardStatus::Active);
}

#[tokio::test]
async fn test_query_task_active_returns_empty_when_no_active_board_exists() {
    let pool = test_helpers::setup_test_pool().await;
    test_helpers::seed_workspace(&pool, "ws-query-none", "/tmp/query-none").await;
    test_helpers::seed_thread(&pool, "t-query-none", "ws-query-none", None).await;

    let board = task_board_manager::create_task_board(
        &pool,
        "t-query-none",
        &CreateTaskInput {
            title: "Single Board".to_string(),
            steps: vec![CreateTaskStep {
                description: "Only step".to_string(),
            }],
        },
    )
    .await
    .unwrap();

    task_board_manager::update_task_board(
        &pool,
        "t-query-none",
        &UpdateTaskInput {
            task_board_id: board.id,
            action: UpdateTaskAction::CompleteBoard,
        },
    )
    .await
    .unwrap();

    let queried =
        task_board_manager::query_thread_task_boards(&pool, "t-query-none", QueryTaskScope::Active)
            .await
            .unwrap();

    assert_eq!(queried.scope, QueryTaskScope::Active);
    assert_eq!(queried.active_task_board_id, None);
    assert!(queried.task_boards.is_empty());
}

#[tokio::test]
async fn test_query_task_all_returns_all_boards_and_active_id() {
    let pool = test_helpers::setup_test_pool().await;
    test_helpers::seed_workspace(&pool, "ws-query-all", "/tmp/query-all").await;
    test_helpers::seed_thread(&pool, "t-query-all", "ws-query-all", None).await;

    let first_board = task_board_manager::create_task_board(
        &pool,
        "t-query-all",
        &CreateTaskInput {
            title: "Board One".to_string(),
            steps: vec![CreateTaskStep {
                description: "Step 1".to_string(),
            }],
        },
    )
    .await
    .unwrap();
    let second_board = task_board_manager::create_task_board(
        &pool,
        "t-query-all",
        &CreateTaskInput {
            title: "Board Two".to_string(),
            steps: vec![CreateTaskStep {
                description: "Step 2".to_string(),
            }],
        },
    )
    .await
    .unwrap();

    let queried =
        task_board_manager::query_thread_task_boards(&pool, "t-query-all", QueryTaskScope::All)
            .await
            .unwrap();

    assert_eq!(queried.scope, QueryTaskScope::All);
    assert_eq!(
        queried.active_task_board_id.as_deref(),
        Some(second_board.id.as_str())
    );
    assert_eq!(queried.task_boards.len(), 2);
    assert_eq!(queried.task_boards[0].id, first_board.id);
    assert_eq!(queried.task_boards[0].status, TaskBoardStatus::Completed);
    assert_eq!(queried.task_boards[1].id, second_board.id);
    assert_eq!(queried.task_boards[1].status, TaskBoardStatus::Active);
}

// =========================================================================
// T2.3.3 — State Machine Validation
// =========================================================================

#[tokio::test]
async fn test_cannot_start_already_started_step() {
    let pool = test_helpers::setup_test_pool().await;
    test_helpers::seed_workspace(&pool, "ws-sm1", "/tmp/sm1").await;
    test_helpers::seed_thread(&pool, "t-sm1", "ws-sm1", None).await;

    let input = CreateTaskInput {
        title: "SM Test".to_string(),
        steps: vec![CreateTaskStep {
            description: "Step A".to_string(),
        }],
    };
    let board = task_board_manager::create_task_board(&pool, "t-sm1", &input)
        .await
        .unwrap();
    let step_id = &board.tasks[0].id;

    // Step is already InProgress — starting it again should fail
    let update = UpdateTaskInput {
        task_board_id: board.id.clone(),
        action: UpdateTaskAction::StartStep {
            step_id: step_id.clone(),
        },
    };
    let result = task_board_manager::update_task_board(&pool, "t-sm1", &update).await;
    assert!(
        result.is_err(),
        "Should reject starting an already in-progress step"
    );
}

#[tokio::test]
async fn test_cannot_complete_pending_step() {
    let pool = test_helpers::setup_test_pool().await;
    test_helpers::seed_workspace(&pool, "ws-sm2", "/tmp/sm2").await;
    test_helpers::seed_thread(&pool, "t-sm2", "ws-sm2", None).await;

    let input = CreateTaskInput {
        title: "SM Test".to_string(),
        steps: vec![
            CreateTaskStep {
                description: "Step A".to_string(),
            },
            CreateTaskStep {
                description: "Step B".to_string(),
            },
        ],
    };
    let board = task_board_manager::create_task_board(&pool, "t-sm2", &input)
        .await
        .unwrap();
    let step_b_id = &board.tasks[1].id; // Step B is Pending

    // Completing a Pending step should fail
    let update = UpdateTaskInput {
        task_board_id: board.id.clone(),
        action: UpdateTaskAction::CompleteStep {
            step_id: step_b_id.clone(),
        },
    };
    let result = task_board_manager::update_task_board(&pool, "t-sm2", &update).await;
    assert!(result.is_err(), "Should reject completing a pending step");
}

#[tokio::test]
async fn test_reconcile_active_board_starts_next_pending_step() {
    let pool = test_helpers::setup_test_pool().await;
    test_helpers::seed_workspace(&pool, "ws-reconcile", "/tmp/reconcile").await;
    test_helpers::seed_thread(&pool, "t-reconcile", "ws-reconcile", None).await;

    let board = task_board_manager::create_task_board(
        &pool,
        "t-reconcile",
        &CreateTaskInput {
            title: "Reconcile Task".to_string(),
            steps: vec![
                CreateTaskStep {
                    description: "Step A".to_string(),
                },
                CreateTaskStep {
                    description: "Step B".to_string(),
                },
            ],
        },
    )
    .await
    .unwrap();

    sqlx::query("UPDATE task_items SET stage = 'completed' WHERE id = ?")
        .bind(&board.tasks[0].id)
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("UPDATE task_items SET stage = 'pending' WHERE id = ?")
        .bind(&board.tasks[1].id)
        .execute(&pool)
        .await
        .unwrap();

    let reconciled = task_board_manager::reconcile_active_task_board(&pool, "t-reconcile")
        .await
        .unwrap()
        .expect("board should be reconciled");

    assert_eq!(reconciled.status, TaskBoardStatus::Active);
    assert_eq!(reconciled.tasks[1].stage, TaskStage::InProgress);
    assert_eq!(
        reconciled.active_task_id.as_ref(),
        Some(&reconciled.tasks[1].id)
    );
}

#[tokio::test]
async fn test_cannot_update_completed_board() {
    let pool = test_helpers::setup_test_pool().await;
    test_helpers::seed_workspace(&pool, "ws-sm3", "/tmp/sm3").await;
    test_helpers::seed_thread(&pool, "t-sm3", "ws-sm3", None).await;

    let input = CreateTaskInput {
        title: "SM Test".to_string(),
        steps: vec![CreateTaskStep {
            description: "Step A".to_string(),
        }],
    };
    let board = task_board_manager::create_task_board(&pool, "t-sm3", &input)
        .await
        .unwrap();

    // Complete the board
    let complete = UpdateTaskInput {
        task_board_id: board.id.clone(),
        action: UpdateTaskAction::CompleteBoard,
    };
    task_board_manager::update_task_board(&pool, "t-sm3", &complete)
        .await
        .unwrap();

    // Trying to start a step on a completed board should fail
    let update = UpdateTaskInput {
        task_board_id: board.id.clone(),
        action: UpdateTaskAction::StartStep {
            step_id: board.tasks[0].id.clone(),
        },
    };
    let result = task_board_manager::update_task_board(&pool, "t-sm3", &update).await;
    assert!(
        result.is_err(),
        "Should reject step updates on a completed board"
    );

    // Trying to complete an already-completed board should also fail
    let update2 = UpdateTaskInput {
        task_board_id: board.id.clone(),
        action: UpdateTaskAction::CompleteBoard,
    };
    let result2 = task_board_manager::update_task_board(&pool, "t-sm3", &update2).await;
    assert!(
        result2.is_err(),
        "Should reject completing an already-completed board"
    );
}

// =========================================================================
// T2.3.4 — Thread Deletion Cascade
// =========================================================================

#[tokio::test]
async fn test_task_boards_deleted_with_thread() {
    let pool = test_helpers::setup_test_pool().await;
    test_helpers::seed_workspace(&pool, "ws-delete", "/tmp/delete").await;
    test_helpers::seed_thread(&pool, "t-delete", "ws-delete", None).await;

    let input = CreateTaskInput {
        title: "To Delete".to_string(),
        steps: vec![CreateTaskStep {
            description: "Step".to_string(),
        }],
    };
    task_board_manager::create_task_board(&pool, "t-delete", &input)
        .await
        .unwrap();

    // Verify board exists
    let boards = task_board_manager::load_thread_task_boards(&pool, "t-delete")
        .await
        .unwrap();
    assert_eq!(boards.len(), 1);

    // Delete thread using ThreadManager
    let manager = ThreadManager::new(pool.clone());
    manager.delete("t-delete").await.unwrap();

    // Verify board is gone
    let row = sqlx::query("SELECT COUNT(*) as count FROM task_boards WHERE thread_id = 't-delete'")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(row.get::<i64, _>("count"), 0);

    // Verify items are also gone
    let items_row = sqlx::query("SELECT COUNT(*) as count FROM task_items WHERE task_board_id IN (SELECT id FROM task_boards WHERE thread_id = 't-delete')")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(items_row.get::<i64, _>("count"), 0);
}
