use chrono::Utc;
use sqlx::SqlitePool;

use crate::model::errors::{AppError, ErrorSource};
use crate::model::goal::{GoalPayload, GoalRecord, GoalStatus, GoalVerdict, PauseReason};
use crate::persistence::repo::goal_repo;

use crate::core::app_state::GoalRuntimeState;

/// Default maximum turns for a goal before auto-pausing.
const DEFAULT_MAX_TURNS: i64 = 50;

/// Continuation prompt injected when the goal is still active.
const CONTINUATION_PROMPT_TEMPLATE: &str = "\
[Goal continuation — turns {turns_used}/{max_turns}]

**Objective:** {objective}

Continue working toward this objective. Take the next concrete step.

⚠️ When the goal is fully achieved, you MUST call:
  update_goal(status=\"complete\", evidence=\"<verifiable proof>\")
Without this call, the system will keep injecting continuation prompts.

If you are blocked and need user input, use the clarify tool.";

/// Challenge prompt when the model claimed completion but did not use the tool.
const CHALLENGE_EVIDENCE_PROMPT: &str = "\
在声明目标完成前，请提供具体证据：

1. 运行了哪些验证命令？输出是什么？
2. 修改了哪些文件？每个变更的目的是什么？

请附证据后调用 update_goal(status=\"complete\", evidence=\"...\")。如果实际上尚未完成，请忽略此提示继续工作。";

/// Challenge prompt when the model claimed completion but evidence was empty.
const MISSING_EVIDENCE_PROMPT: &str = "\
你调用了 update_goal(complete) 但没有提供 evidence。
请提供完成证据后重新调用 update_goal(status=\"complete\", evidence=\"<你的证据>\")。";

/// Guidance prompt when the agent appears stuck.
const GUIDANCE_PROMPT: &str = "\
你似乎不确定下一步。当前目标：{objective}

请规划下一步具体行动，然后执行。如果需要用户决策，请使用 clarify 工具。";

/// Completion-claim markers for detecting undeclared completion.
const COMPLETION_MARKERS: &[&str] = &[
    "done",
    "完成",
    "finished",
    "做好了",
    "搞定了",
    "全部完成",
    "所有任务已完成",
    "目标达成",
    "已完成",
];

/// Maximum consecutive idle turns before auto-pausing.
const MAX_IDLE_TURNS: u32 = 3;

/// GoalManager provides the goal lifecycle: create, evaluate, pause, complete, clear.
/// Each instance is bound to a single thread.
#[derive(Clone)]
pub struct GoalManager {
    pool: SqlitePool,
    thread_id: String,
    /// Shared runtime state for tool call tracking and idle/completion counters.
    runtime: std::sync::Arc<std::sync::Mutex<GoalRuntimeState>>,
}

impl GoalManager {
    /// Create a new GoalManager for a specific thread.
    /// Uses shared state for tool call tracking across instances.
    pub fn new(
        pool: SqlitePool,
        thread_id: String,
        runtime: std::sync::Arc<std::sync::Mutex<GoalRuntimeState>>,
    ) -> Self {
        Self {
            pool,
            thread_id,
            runtime,
        }
    }

    /// Lock the shared runtime state, recovering from poison.
    fn lock_runtime(&self) -> std::sync::MutexGuard<'_, GoalRuntimeState> {
        self.runtime.lock().unwrap_or_else(|poisoned| {
            tracing::warn!("goal_manager: runtime mutex poisoned, recovering");
            poisoned.into_inner()
        })
    }

    /// Record a tool call name from the current turn.
    pub fn record_tool_call(&self, tool_name: &str) {
        let mut guard = self.lock_runtime();
        guard
            .thread_tool_calls
            .entry(self.thread_id.clone())
            .or_default()
            .push(tool_name.to_string());
    }

    /// Consume and return the accumulated tool call names, resetting the list.
    fn drain_tool_calls(&self) -> Vec<String> {
        let mut guard = self.lock_runtime();
        guard
            .thread_tool_calls
            .remove(&self.thread_id)
            .unwrap_or_default()
    }

    // ── CRUD ──

    /// Create a new active goal. Fails if a goal already exists for this thread.
    pub async fn create_goal(
        &self,
        objective: &str,
        token_budget: Option<i64>,
    ) -> Result<GoalRecord, AppError> {
        if goal_repo::find_by_thread_id(&self.pool, &self.thread_id)
            .await?
            .is_some()
        {
            return Err(AppError::validation(
                ErrorSource::Settings,
                "a goal already exists for this thread — clear it first with /goal clear",
            ));
        }

        let record = GoalRecord {
            id: uuid::Uuid::now_v7().to_string(),
            thread_id: self.thread_id.clone(),
            objective: objective.to_string(),
            status: GoalStatus::Active,
            token_budget,
            tokens_used: 0,
            time_used_seconds: 0,
            turns_used: 0,
            max_turns: DEFAULT_MAX_TURNS,
            pause_reason: None,
            pause_detail: None,
            evidence: None,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };

        goal_repo::insert(&self.pool, &record).await?;
        Ok(record)
    }

    /// Load the current goal for this thread.
    pub async fn get_active(&self) -> Result<Option<GoalRecord>, AppError> {
        goal_repo::find_by_thread_id(&self.pool, &self.thread_id).await
    }

    /// Convert a goal record to a lightweight payload for stream events.
    pub fn to_payload(record: &GoalRecord) -> GoalPayload {
        GoalPayload::from(record.clone())
    }

    // ── Lifecycle ──

    /// Pause the goal for a given reason.
    pub async fn pause(
        &self,
        goal_id: &str,
        reason: PauseReason,
        detail: Option<String>,
    ) -> Result<(), AppError> {
        let updated = goal_repo::update_status(
            &self.pool,
            goal_id,
            GoalStatus::Paused,
            Some(reason),
            detail.as_deref(),
            None,
        )
        .await?;
        if !updated {
            return Err(AppError::not_found(ErrorSource::Settings, "goal"));
        }
        Ok(())
    }

    /// Mark the goal as complete with evidence.
    pub async fn mark_complete(&self, goal_id: &str, evidence: &str) -> Result<(), AppError> {
        if evidence.trim().is_empty() {
            return Err(AppError::validation(
                ErrorSource::Settings,
                "evidence is required to mark a goal as complete",
            ));
        }
        let updated = goal_repo::update_status(
            &self.pool,
            goal_id,
            GoalStatus::Complete,
            None,
            None,
            Some(evidence),
        )
        .await?;
        if !updated {
            return Err(AppError::not_found(ErrorSource::Settings, "goal"));
        }
        Ok(())
    }

    /// Mark the goal as budget-limited.
    pub async fn mark_budget_limited(&self, goal_id: &str) -> Result<(), AppError> {
        let updated = goal_repo::update_status(
            &self.pool,
            goal_id,
            GoalStatus::BudgetLimited,
            None,
            None,
            None,
        )
        .await?;
        if !updated {
            return Err(AppError::not_found(ErrorSource::Settings, "goal"));
        }
        Ok(())
    }

    /// Resume a paused goal.
    pub async fn resume(&self, goal_id: &str) -> Result<(), AppError> {
        let updated =
            goal_repo::update_status(&self.pool, goal_id, GoalStatus::Active, None, None, None)
                .await?;
        if !updated {
            return Err(AppError::not_found(ErrorSource::Settings, "goal"));
        }
        Ok(())
    }

    /// Clear / delete the goal.
    pub async fn clear(&self) -> Result<bool, AppError> {
        // Reset per-thread runtime counters so a subsequent create_goal
        // on the same thread starts with clean idle/completion tracking.
        self.reset_idle_counters();
        // Also drain any accumulated tool calls from the current turn.
        self.drain_tool_calls();
        goal_repo::delete_by_thread_id(&self.pool, &self.thread_id).await
    }

    /// Account usage after a turn. Increments turn count, tokens, and time.
    pub async fn account_usage(
        &self,
        goal_id: &str,
        tokens: i64,
        time_seconds: i64,
    ) -> Result<(), AppError> {
        goal_repo::account_usage(&self.pool, goal_id, tokens, time_seconds, 1).await
    }

    // ── Auto-resume ──

    /// Check if a paused goal should auto-resume when the user sends a new message.
    /// Returns Some(()) if the goal was auto-resumed, None if it shouldn't.
    pub async fn try_auto_resume(&self) -> Result<bool, AppError> {
        let goal = match self.get_active().await? {
            Some(g) => g,
            None => return Ok(false),
        };

        if goal.status != GoalStatus::Paused {
            return Ok(false);
        }

        let should_resume = goal
            .pause_reason
            .as_ref()
            .map(|r| r.auto_resume_on_user_message())
            .unwrap_or(false);

        if should_resume {
            goal_repo::update_status(&self.pool, &goal.id, GoalStatus::Active, None, None, None)
                .await?;
        }

        Ok(should_resume)
    }

    // ── Evaluation ──

    /// Evaluate whether the goal should continue, pause, or complete after a turn.
    /// Called at MessageEnd with the final assistant response and tool call names.
    pub async fn evaluate_after_turn(&self, response: &str, goal: &GoalRecord) -> GoalVerdict {
        // Only evaluate active goals.
        if goal.status != GoalStatus::Active {
            return GoalVerdict::Continue; // Should not get here, but safe default.
        }

        let tool_calls = self.drain_tool_calls();

        // ── Layer 1: Blocking signals (tool-based) ──
        if let Some(verdict) = self.detect_tool_based_blocking(&tool_calls, response) {
            return verdict;
        }

        // ── Layer 2: Idle detection ──
        if tool_calls.is_empty() {
            if let Some(verdict) = self.detect_idle_block(response) {
                return verdict;
            }

            // Completion claim without tool call
            if self.detect_completion_claim(response) {
                let should_pause = {
                    let mut guard = self.lock_runtime();
                    let count = guard
                        .completion_claim_count
                        .entry(self.thread_id.clone())
                        .or_default();
                    *count += 1;
                    *count >= 3
                };
                if should_pause {
                    // Reset counter before pausing
                    self.lock_runtime()
                        .completion_claim_count
                        .remove(&self.thread_id);
                    return GoalVerdict::Paused {
                        reason: PauseReason::IdleBlocked,
                        detail: Some("agent repeatedly claimed completion without providing evidence via update_goal".into()),
                    };
                }
                return GoalVerdict::ChallengeEvidence;
            }

            // Short response + no tools → guidance
            if response.trim().len() < 200 {
                return GoalVerdict::Continue; // Will render guidance prompt
            }

            return GoalVerdict::Continue;
        }

        // Reset idle counters since tools were called
        self.reset_idle_counters();

        // ── Layer 4: Budget checks ──
        if let Some(budget) = goal.token_budget {
            if goal.tokens_used >= budget {
                return GoalVerdict::BudgetLimited;
            }
        }

        if goal.turns_used >= goal.max_turns {
            return GoalVerdict::Paused {
                reason: PauseReason::BudgetExhausted,
                detail: Some(format!(
                    "{} of {} turns used",
                    goal.turns_used, goal.max_turns
                )),
            };
        }

        // ── Default: continue ──
        GoalVerdict::Continue
    }

    // ── Detection helpers ──

    fn detect_tool_based_blocking(
        &self,
        tool_calls: &[String],
        _response: &str,
    ) -> Option<GoalVerdict> {
        for tool_name in tool_calls {
            match tool_name.as_str() {
                "clarify" => {
                    return Some(GoalVerdict::Paused {
                        reason: PauseReason::ClarifyPending,
                        detail: Some("agent requested clarification".into()),
                    });
                }
                "update_plan" => {
                    return Some(GoalVerdict::Paused {
                        reason: PauseReason::PlanPending,
                        detail: Some("agent published a plan, awaiting approval".into()),
                    });
                }
                _ => {}
            }
        }
        None
    }

    fn detect_idle_block(&self, response: &str) -> Option<GoalVerdict> {
        let idle_count = self.increment_idle_count();
        let trimmed = response.trim().to_lowercase();

        if idle_count >= MAX_IDLE_TURNS {
            return Some(GoalVerdict::Paused {
                reason: PauseReason::IdleBlocked,
                detail: Some(format!(
                    "agent has not performed any tool calls for {idle_count} consecutive turns"
                )),
            });
        }

        // Lightweight heuristic: short question-like response + no tools
        if idle_count >= 2 {
            let blockers = [
                "should i",
                "do you want",
                "would you like",
                "请确认",
                "需要你决定",
                "which approach",
                "which option",
                "can you confirm",
                "let me know if",
                "before i proceed",
                "你的选择是",
                "你确认吗",
                "需要你同意",
            ];
            if trimmed.len() < 500 && blockers.iter().any(|b| trimmed.contains(b)) {
                return Some(GoalVerdict::Paused {
                    reason: PauseReason::IdleBlocked,
                    detail: Some("agent appears blocked, may need user input".into()),
                });
            }
        }
        None
    }

    fn detect_completion_claim(&self, response: &str) -> bool {
        let trimmed = response.trim().to_lowercase();
        COMPLETION_MARKERS.iter().any(|m| trimmed.contains(m))
    }

    fn increment_idle_count(&self) -> u32 {
        let mut guard = self.lock_runtime();
        let count = guard
            .idle_turn_count
            .entry(self.thread_id.clone())
            .or_default();
        *count += 1;
        *count
    }

    fn reset_idle_counters(&self) {
        let mut guard = self.lock_runtime();
        guard.idle_turn_count.remove(&self.thread_id);
        guard.completion_claim_count.remove(&self.thread_id);
    }

    // ── Prompt generation ──

    /// Generate the continuation prompt for the next turn.
    pub fn render_continuation_prompt(&self, goal: &GoalRecord) -> String {
        CONTINUATION_PROMPT_TEMPLATE
            .replace("{objective}", &goal.objective)
            .replace("{turns_used}", &goal.turns_used.to_string())
            .replace("{max_turns}", &goal.max_turns.to_string())
    }

    /// Generate a challenge-evidence prompt when the model failed to provide evidence.
    pub fn render_challenge_prompt(&self, variant: ChallengePromptVariant) -> String {
        match variant {
            ChallengePromptVariant::NoEvidence => MISSING_EVIDENCE_PROMPT.to_string(),
            ChallengePromptVariant::NoTool => CHALLENGE_EVIDENCE_PROMPT.to_string(),
        }
    }

    /// Generate a guidance prompt when the agent appears stuck.
    pub fn render_guidance_prompt(&self, objective: &str) -> String {
        GUIDANCE_PROMPT.replace("{objective}", objective)
    }
}

/// Variants for challenge prompts.
pub enum ChallengePromptVariant {
    /// Model called update_goal(complete) but evidence was empty.
    NoEvidence,
    /// Model claimed completion in text but didn't use the tool.
    NoTool,
}
