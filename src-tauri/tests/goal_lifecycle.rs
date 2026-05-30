#[cfg(test)]
mod tests {
    use sqlx::sqlite::{SqliteConnectOptions, SqlitePool, SqlitePoolOptions};
    use std::str::FromStr;
    use tiycode_lib::core::app_state::GoalRuntimeState;
    use tiycode_lib::core::goal_manager::{ChallengePromptVariant, GoalManager};
    use tiycode_lib::model::goal::{GoalStatus, GoalVerdict, PauseReason};
    use tiycode_lib::persistence::repo::goal_repo;

    async fn setup_pool() -> SqlitePool {
        let options = SqliteConnectOptions::from_str("sqlite::memory:")
            .unwrap()
            .foreign_keys(true);
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(options)
            .await
            .unwrap();
        tiycode_lib::persistence::sqlite::run_migrations(&pool)
            .await
            .unwrap();

        // Seed workspace and thread for FK constraints
        let now = chrono::Utc::now().to_rfc3339();
        sqlx::query(
            "INSERT INTO workspaces (id, name, path, canonical_path, display_path,
                    is_default, is_git, auto_work_tree, status, created_at, updated_at)
             VALUES ('ws-test', 'Test Workspace', '/tmp/test', '/tmp/test', '/tmp/test',
                     0, 0, 0, 'ready', ?, ?)",
        )
        .bind(&now)
        .bind(&now)
        .execute(&pool)
        .await
        .expect("failed to seed workspace");

        sqlx::query(
            "INSERT INTO threads (id, workspace_id, title, status, last_active_at, created_at, updated_at)
             VALUES ('thread-1', 'ws-test', 'Test Thread', 'idle', ?, ?, ?)",
        )
        .bind(&now)
        .bind(&now)
        .bind(&now)
        .execute(&pool)
        .await
        .expect("failed to seed thread");

        pool
    }

    fn test_runtime() -> std::sync::Arc<std::sync::Mutex<GoalRuntimeState>> {
        std::sync::Arc::new(std::sync::Mutex::new(GoalRuntimeState::default()))
    }

    #[tokio::test]
    async fn create_goal_persists_and_loads() {
        let pool = setup_pool().await;
        let mgr = GoalManager::new(pool.clone(), "thread-1".into(), test_runtime());

        let record = mgr.create_goal("Build a todo app", None).await.unwrap();
        assert_eq!(record.objective, "Build a todo app");
        assert_eq!(record.status, GoalStatus::Active);
        assert_eq!(record.max_turns, 50);
        assert!(record.last_evaluated_run_id.is_none());

        let loaded = mgr.get_active().await.unwrap().unwrap();
        assert_eq!(loaded.id, record.id);
        assert_eq!(loaded.objective, "Build a todo app");
    }

    #[tokio::test]
    async fn create_goal_fails_when_already_exists() {
        let pool = setup_pool().await;
        let mgr = GoalManager::new(pool.clone(), "thread-1".into(), test_runtime());

        mgr.create_goal("First goal", None).await.unwrap();
        let err = mgr.create_goal("Second goal", None).await.unwrap_err();
        assert!(err.user_message.contains("already exists"));
    }

    #[tokio::test]
    async fn mark_evaluated_if_needed_claims_each_run_once_for_active_goal() {
        let pool = setup_pool().await;
        let mgr = GoalManager::new(pool.clone(), "thread-1".into(), test_runtime());
        let goal = mgr.create_goal("Test goal", None).await.unwrap();

        assert!(
            goal_repo::mark_evaluated_if_needed(&pool, &goal.id, "run-1")
                .await
                .unwrap()
        );
        assert!(
            !goal_repo::mark_evaluated_if_needed(&pool, &goal.id, "run-1")
                .await
                .unwrap()
        );

        let loaded = mgr.get_active().await.unwrap().unwrap();
        assert_eq!(loaded.last_evaluated_run_id.as_deref(), Some("run-1"));
    }

    #[tokio::test]
    async fn mark_evaluated_if_needed_skips_non_active_goal() {
        let pool = setup_pool().await;
        let mgr = GoalManager::new(pool.clone(), "thread-1".into(), test_runtime());
        let goal = mgr.create_goal("Test goal", None).await.unwrap();
        mgr.pause(&goal.id, PauseReason::UserRequested, None)
            .await
            .unwrap();

        assert!(
            !goal_repo::mark_evaluated_if_needed(&pool, &goal.id, "run-1")
                .await
                .unwrap()
        );
    }

    #[tokio::test]
    async fn evaluate_after_run_accounts_usage_once_per_run() {
        let pool = setup_pool().await;
        let mgr = GoalManager::new(pool.clone(), "thread-1".into(), test_runtime());
        let goal = mgr.create_goal("Test goal", None).await.unwrap();

        sqlx::query(
            "INSERT INTO thread_runs (id, thread_id, run_mode, status, started_at, finished_at)
             VALUES ('run-1', 'thread-1', 'default', 'completed', '2026-04-22T09:00:00Z', '2026-04-22T09:00:42Z')",
        )
        .execute(&pool)
        .await
        .unwrap();

        let first = mgr
            .evaluate_after_run("run-1", Some("Some progress".to_string()))
            .await
            .unwrap()
            .expect("first evaluation should return an outcome");
        assert_eq!(first.verdict, "continue");

        let after_first = mgr.get_active().await.unwrap().unwrap();
        assert_eq!(after_first.turns_used, goal.turns_used + 1);
        assert_eq!(after_first.time_used_seconds, 42);
        assert_eq!(after_first.last_evaluated_run_id.as_deref(), Some("run-1"));

        let second = mgr
            .evaluate_after_run("run-1", Some("Duplicate terminal event".to_string()))
            .await
            .unwrap()
            .expect("duplicate evaluation should return skipped state");
        assert_eq!(second.verdict, "skipped");

        let after_second = mgr.get_active().await.unwrap().unwrap();
        assert_eq!(after_second.turns_used, after_first.turns_used);
        assert_eq!(
            after_second.time_used_seconds,
            after_first.time_used_seconds
        );
    }

    #[tokio::test]
    async fn evaluate_after_turn_no_completion_continues() {
        let pool = setup_pool().await;
        let mgr = GoalManager::new(pool.clone(), "thread-1".into(), test_runtime());
        let goal = mgr.create_goal("Test goal", None).await.unwrap();

        // Simulate a tool call so idle counter doesn't trigger
        mgr.record_tool_call("read");

        let verdict = mgr.evaluate_after_turn("Some progress", &goal);
        assert!(matches!(verdict, GoalVerdict::Continue));
    }

    #[tokio::test]
    async fn evaluate_after_turn_clarify_triggers_pause() {
        let pool = setup_pool().await;
        let mgr = GoalManager::new(pool.clone(), "thread-1".into(), test_runtime());
        let goal = mgr.create_goal("Test goal", None).await.unwrap();

        // Record a clarify tool call
        mgr.record_tool_call("clarify");

        let verdict = mgr.evaluate_after_turn("What do you think?", &goal);
        assert!(matches!(
            verdict,
            GoalVerdict::Paused {
                reason: PauseReason::ClarifyPending,
                ..
            }
        ));
    }

    #[tokio::test]
    async fn evaluate_after_turn_update_plan_triggers_pause() {
        let pool = setup_pool().await;
        let mgr = GoalManager::new(pool.clone(), "thread-1".into(), test_runtime());
        let goal = mgr.create_goal("Test goal", None).await.unwrap();

        mgr.record_tool_call("update_plan");

        let verdict = mgr.evaluate_after_turn("Here is the plan", &goal);
        assert!(matches!(
            verdict,
            GoalVerdict::Paused {
                reason: PauseReason::PlanPending,
                ..
            }
        ));
    }

    #[tokio::test]
    async fn evaluate_after_turn_idle_three_turns_pauses() {
        let pool = setup_pool().await;
        let mgr = GoalManager::new(pool.clone(), "thread-1".into(), test_runtime());
        let goal = mgr.create_goal("Test goal", None).await.unwrap();

        // Three consecutive idle turns
        mgr.evaluate_after_turn("Just chatting", &goal);
        mgr.evaluate_after_turn("Still chatting", &goal);
        let verdict = mgr.evaluate_after_turn("More chat", &goal);

        assert!(matches!(
            verdict,
            GoalVerdict::Paused {
                reason: PauseReason::IdleBlocked,
                ..
            }
        ));
    }

    #[tokio::test]
    async fn evaluate_after_turn_max_turns_pauses() {
        let pool = setup_pool().await;
        let mgr = GoalManager::new(pool.clone(), "thread-1".into(), test_runtime());
        let goal = mgr.create_goal("Test goal", None).await.unwrap();

        // Set turns_used to at least max_turns via account_usage
        goal_repo::account_usage(&pool, &goal.id, 0, 0, goal.max_turns)
            .await
            .unwrap();

        mgr.record_tool_call("read");

        // This should push it to max and pause
        let fresh_goal = mgr.get_active().await.unwrap().unwrap();
        let verdict = mgr.evaluate_after_turn("Working...", &fresh_goal);

        assert!(matches!(
            verdict,
            GoalVerdict::Paused {
                reason: PauseReason::BudgetExhausted,
                ..
            }
        ));
    }

    #[tokio::test]
    async fn completion_claim_detection_without_tool_challenges() {
        let pool = setup_pool().await;
        let mgr = GoalManager::new(pool.clone(), "thread-1".into(), test_runtime());
        let goal = mgr.create_goal("Test goal", None).await.unwrap();

        // Model says "done" but doesn't call goal_scored
        let verdict = mgr.evaluate_after_turn(
            "All done! The goal is complete and everything is finished.",
            &goal,
        );

        assert!(matches!(verdict, GoalVerdict::ChallengeEvidence));
    }

    #[tokio::test]
    async fn pause_and_resume_lifecycle() {
        let pool = setup_pool().await;
        let mgr = GoalManager::new(pool.clone(), "thread-1".into(), test_runtime());
        let goal = mgr.create_goal("Test goal", None).await.unwrap();

        // Pause
        mgr.pause(&goal.id, PauseReason::UserRequested, None)
            .await
            .unwrap();
        let paused = mgr.get_active().await.unwrap().unwrap();
        assert_eq!(paused.status, GoalStatus::Paused);

        // Resume
        mgr.resume(&goal.id).await.unwrap();
        let resumed = mgr.get_active().await.unwrap().unwrap();
        assert_eq!(resumed.status, GoalStatus::Active);
    }

    #[tokio::test]
    async fn auto_resume_clarify_pending() {
        let pool = setup_pool().await;
        let mgr = GoalManager::new(pool.clone(), "thread-1".into(), test_runtime());
        let goal = mgr.create_goal("Test goal", None).await.unwrap();

        mgr.pause(&goal.id, PauseReason::ClarifyPending, None)
            .await
            .unwrap();

        let resumed = mgr.try_auto_resume().await.unwrap();
        assert!(resumed, "ClarifyPending should auto-resume");

        let active = mgr.get_active().await.unwrap().unwrap();
        assert_eq!(active.status, GoalStatus::Active);
    }

    #[tokio::test]
    async fn auto_resume_skips_user_requested() {
        let pool = setup_pool().await;
        let mgr = GoalManager::new(pool.clone(), "thread-1".into(), test_runtime());
        let goal = mgr.create_goal("Test goal", None).await.unwrap();

        mgr.pause(&goal.id, PauseReason::UserRequested, None)
            .await
            .unwrap();

        let resumed = mgr.try_auto_resume().await.unwrap();
        assert!(!resumed, "UserRequested should NOT auto-resume");

        let paused = mgr.get_active().await.unwrap().unwrap();
        assert_eq!(paused.status, GoalStatus::Paused);
    }

    #[tokio::test]
    async fn mark_complete_with_evidence() {
        let pool = setup_pool().await;
        let mgr = GoalManager::new(pool.clone(), "thread-1".into(), test_runtime());
        let goal = mgr.create_goal("Test goal", None).await.unwrap();

        mgr.mark_complete(&goal.id, "All tests pass, files created")
            .await
            .unwrap();

        let completed = mgr.get_active().await.unwrap().unwrap();
        assert_eq!(completed.status, GoalStatus::Complete);
        assert_eq!(
            completed.evidence.as_deref(),
            Some("All tests pass, files created")
        );
    }

    #[tokio::test]
    async fn mark_budget_limited() {
        let pool = setup_pool().await;
        let mgr = GoalManager::new(pool.clone(), "thread-1".into(), test_runtime());
        let goal = mgr.create_goal("Test goal", None).await.unwrap();

        mgr.mark_budget_limited(&goal.id).await.unwrap();

        let limited = mgr.get_active().await.unwrap().unwrap();
        assert_eq!(limited.status, GoalStatus::BudgetLimited);
    }

    #[tokio::test]
    async fn clear_removes_goal() {
        let pool = setup_pool().await;
        let mgr = GoalManager::new(pool.clone(), "thread-1".into(), test_runtime());
        mgr.create_goal("Test goal", None).await.unwrap();

        let cleared = mgr.clear().await.unwrap();
        assert!(cleared);

        let gone = mgr.get_active().await.unwrap();
        assert!(gone.is_none());
    }

    #[tokio::test]
    async fn continuation_prompt_renders_correctly() {
        let pool = setup_pool().await;
        let mgr = GoalManager::new(pool.clone(), "thread-1".into(), test_runtime());
        let goal = mgr.create_goal("Build feature X", None).await.unwrap();

        let prompt = mgr.render_continuation_prompt(&goal);
        assert!(prompt.contains("Build feature X"));
        assert!(prompt.contains("goal_scored"));
        assert!(prompt.contains("clarify"));
    }

    #[tokio::test]
    async fn challenge_prompt_renders_variants() {
        let mgr = GoalManager::new(setup_pool().await, "thread-1".into(), test_runtime());

        let no_evidence = mgr.render_challenge_prompt(ChallengePromptVariant::NoEvidence);
        assert!(no_evidence.contains("did not provide evidence"));

        let no_tool = mgr.render_challenge_prompt(ChallengePromptVariant::NoTool);
        assert!(no_tool.contains("provide concrete evidence"));
        assert!(no_tool.contains("goal_scored"));
    }
}
