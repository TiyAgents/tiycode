use chrono::Utc;
use serde::Serialize;
use sqlx::SqlitePool;

use crate::model::errors::{AppError, ErrorSource};
use crate::model::goal::{GoalPayload, GoalRecord, GoalStatus, GoalVerdict, PauseReason};
use crate::persistence::repo::goal_repo;

use crate::core::app_state::GoalRuntimeState;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GoalEvaluationOutcome {
    pub goal: GoalPayload,
    pub verdict: String,
    pub continuation_prompt: Option<String>,
}

/// Default maximum turns for a goal before auto-pausing.
const DEFAULT_MAX_TURNS: i64 = 50;

/// Continuation prompt injected when the goal is still active.
const CONTINUATION_PROMPT_TEMPLATE: &str = "\
[Goal continuation — turns {turns_used}/{max_turns}]

**Objective:** {objective}

Continue working toward this objective. Take the next concrete step.

⚠️ Completion is now decided by independent verification. When you believe the
goal is achieved, you MUST call:
  agent_judge(task=\"explain why you believe the goal is achieved / what to verify\")
A Judge will evaluate whether the project satisfies the goal's consistency and
completeness.
- The goal is only marked verified when the Judge returns passed=true.
- If a previous Judge verification did not pass, read its findings, fix each one,
  then call agent_judge again.
You cannot declare completion yourself; only a passing Judge verdict counts.

If you are blocked and need user input, use the clarify tool.";

/// Challenge prompt when the model claimed completion but has not requested
/// Judge verification yet.
const CHALLENGE_EVIDENCE_PROMPT: &str = "\
You appear to believe the goal is complete, but you have not requested independent
verification. You cannot self-declare completion.

When you are confident the goal is achieved, call:
  agent_judge(task=\"explain why you believe the goal is achieved / what to verify\")
The goal is only marked verified when the Judge returns passed=true. If the goal
is not actually complete, ignore this prompt and continue working.";

/// Guidance prompt when the agent appears stuck.
const GUIDANCE_PROMPT: &str = "\
You seem unsure about the next step. Current objective: {objective}

Plan a concrete next action and execute it. If you need a user decision, use the clarify tool.";

/// Completion-claim markers for detecting undeclared completion.
const COMPLETION_MARKERS: &[&str] = &[
    "done",
    "complete",
    "finished",
    "all tasks completed",
    "goal achieved",
    "target met",
    "task finished",
    "everything is done",
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
            turns_used: 0,
            max_turns: DEFAULT_MAX_TURNS,
            pause_reason: None,
            pause_detail: None,
            evidence: None,
            last_evaluated_run_id: None,
            judge_passed: false,
            judge_completeness: None,
            judge_findings: None,
            judge_summary: None,
            judge_evaluated_run_id: None,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };

        match goal_repo::insert(&self.pool, &record).await {
            Ok(()) => Ok(record),
            Err(e) => {
                if e.to_string().contains("UNIQUE constraint failed") {
                    Err(AppError::validation(
                        ErrorSource::Settings,
                        "a goal already exists for this thread — clear it first with /goal clear",
                    ))
                } else {
                    Err(e)
                }
            }
        }
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
        // Reset idle counters so resumed goals don't immediately re-pause.
        self.reset_idle_counters();
        Ok(())
    }

    /// Clear / delete the goal.
    pub async fn clear(&self) -> Result<bool, AppError> {
        // Remove all per-thread runtime state so a subsequent create_goal
        // on the same thread starts with clean counters.
        self.lock_runtime().cleanup_thread(&self.thread_id);
        goal_repo::delete_by_thread_id(&self.pool, &self.thread_id).await
    }

    /// Account usage after a turn. Increments turn count and tokens.
    pub async fn account_usage(&self, goal_id: &str, tokens: i64) -> Result<(), AppError> {
        goal_repo::account_usage(&self.pool, goal_id, tokens, 1).await
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
    /// Called synchronously — it only performs CPU-bound checks and Mutex operations
    /// with no I/O or async work.
    pub fn evaluate_after_turn(&self, response: &str, goal: &GoalRecord) -> GoalVerdict {
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
                        detail: Some("agent repeatedly claimed completion without requesting Judge verification via agent_judge".into()),
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
                // agent_judge is the main-agent-only acceptance request. It is
                // handled by the tool execution pipeline (execute_judge_tool),
                // which runs the Judge and records the verdict. Evaluation must
                // not treat it as a blocking tool — like any tool call it shows
                // the agent acted and should reset idle tendencies.
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

    /// Generate the continuation prompt for the next turn. When a prior Judge
    /// verification did not pass, the most recent findings are appended so the
    /// agent can fix them before re-requesting verification.
    pub fn render_continuation_prompt(&self, goal: &GoalRecord) -> String {
        let mut prompt = CONTINUATION_PROMPT_TEMPLATE
            .replace("{objective}", &goal.objective)
            .replace("{turns_used}", &goal.turns_used.to_string())
            .replace("{max_turns}", &goal.max_turns.to_string());

        if goal.judge_evaluated_run_id.is_some() && !goal.judge_passed {
            if let Some(findings_json) = goal.judge_findings.as_deref() {
                if let Ok(findings) = serde_json::from_str::<Vec<String>>(findings_json) {
                    let findings: Vec<String> = findings
                        .into_iter()
                        .filter(|f| !f.trim().is_empty())
                        .take(10)
                        .collect();
                    if !findings.is_empty() {
                        prompt.push_str(
                            "\n\nMost recent Judge findings to address before re-verifying:",
                        );
                        for finding in findings {
                            let trimmed = finding.trim();
                            let truncated: String = trimmed.chars().take(500).collect();
                            prompt.push_str(&format!("\n- {truncated}"));
                        }
                    }
                }
            }
        }

        prompt
    }

    /// Generate a challenge prompt nudging the agent to request Judge
    /// verification when it claims completion without calling `agent_judge`.
    pub fn render_challenge_prompt(&self) -> String {
        CHALLENGE_EVIDENCE_PROMPT.to_string()
    }

    /// Generate a guidance prompt when the agent appears stuck.
    pub fn render_guidance_prompt(&self, objective: &str) -> String {
        GUIDANCE_PROMPT.replace("{objective}", objective)
    }

    pub async fn evaluate_after_run(
        &self,
        run_id: &str,
        response: Option<String>,
    ) -> Result<Option<GoalEvaluationOutcome>, AppError> {
        let goal = match self.get_active().await? {
            Some(goal) => goal,
            None => return Ok(None),
        };

        // Acceptance is now decided exclusively by the Judge: a verified goal is
        // `Complete && judge_passed`. Any non-Active goal stops continuation,
        // preserving existing pause/budget semantics. The legacy combination
        // `Complete && !judge_passed` should not occur after migration backfill;
        // if it does, log it and still stop continuation rather than re-opening.
        if goal.status != GoalStatus::Active {
            if goal.status == GoalStatus::Complete && !goal.judge_passed {
                tracing::warn!(
                    goal_id = %goal.id,
                    "goal is Complete without judge_passed; treating as terminal and not re-opening"
                );
            }
            return Ok(Some(GoalEvaluationOutcome {
                goal: Self::to_payload(&goal),
                verdict: "skipped".to_string(),
                continuation_prompt: None,
            }));
        }

        if goal.last_evaluated_run_id.as_deref() == Some(run_id) {
            return Ok(Some(GoalEvaluationOutcome {
                goal: Self::to_payload(&goal),
                verdict: "skipped".to_string(),
                continuation_prompt: None,
            }));
        }

        let claimed = goal_repo::mark_evaluated_if_needed(&self.pool, &goal.id, run_id).await?;
        if !claimed {
            let current = self.get_active().await?.unwrap_or(goal);
            return Ok(Some(GoalEvaluationOutcome {
                goal: Self::to_payload(&current),
                verdict: "skipped".to_string(),
                continuation_prompt: None,
            }));
        }

        let response = match response {
            Some(value) if !value.is_empty() => value,
            _ => {
                let recent = crate::persistence::repo::message_repo::list_recent(
                    &self.pool,
                    &self.thread_id,
                    None,
                    10,
                )
                .await?;
                recent
                    .iter()
                    .rev()
                    .find(|message| message.role == "assistant")
                    .map(|message| message.content_markdown.clone())
                    .unwrap_or_default()
            }
        };

        let verdict = self.evaluate_after_turn(&response, &goal);

        let current = self.get_active().await?;
        let current_is_active = current
            .as_ref()
            .map(|goal| goal.status == GoalStatus::Active)
            .unwrap_or(false);

        if !current_is_active {
            let payload = current
                .as_ref()
                .map(Self::to_payload)
                .unwrap_or_else(|| Self::to_payload(&goal));
            return Ok(Some(GoalEvaluationOutcome {
                goal: payload,
                verdict: "skipped".to_string(),
                continuation_prompt: None,
            }));
        }

        let current = current.unwrap();

        match &verdict {
            GoalVerdict::Continue => {}
            GoalVerdict::ChallengeEvidence => {}
            GoalVerdict::Paused { reason, detail } => {
                self.pause(&current.id, reason.clone(), detail.clone())
                    .await?;
            }
            GoalVerdict::BudgetLimited => {
                self.mark_budget_limited(&current.id).await?;
            }
        }

        // Bump goal turn counter for any run that did real work. We still consult
        // run duration to filter out zero-work runs (e.g. an immediately-interrupted
        // run shouldn't burn a turn against max_turns); active running time is
        // tracked separately on thread_runs.elapsed_running_secs and is no longer
        // billed against the goal here.
        if let Some(run_seconds) =
            crate::persistence::repo::run_repo::get_run_duration(&self.pool, run_id)
                .await
                .unwrap_or(None)
        {
            if run_seconds > 0 {
                self.account_usage(&current.id, 0).await.ok();
            }
        }

        let updated = self.get_active().await?;
        let payload = updated
            .as_ref()
            .map(Self::to_payload)
            .unwrap_or_else(|| Self::to_payload(&current));

        let (verdict_str, continuation_prompt) = match &verdict {
            GoalVerdict::Continue => (
                "continue",
                Some(self.render_continuation_prompt(updated.as_ref().unwrap_or(&goal))),
            ),
            GoalVerdict::ChallengeEvidence => (
                "challenge_evidence",
                Some(format!(
                    "{}\n\n{}",
                    self.render_challenge_prompt(),
                    self.render_continuation_prompt(updated.as_ref().unwrap_or(&goal))
                )),
            ),
            GoalVerdict::Paused { reason: _, detail } => ("paused", detail.clone()),
            GoalVerdict::BudgetLimited => ("budget_limited", None),
        };

        Ok(Some(GoalEvaluationOutcome {
            goal: payload,
            verdict: verdict_str.to_string(),
            continuation_prompt,
        }))
    }
}
