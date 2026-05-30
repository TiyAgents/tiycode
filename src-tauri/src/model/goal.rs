use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Status of a persisted goal.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GoalStatus {
    Active,
    Paused,
    BudgetLimited,
    Complete,
}

impl GoalStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            GoalStatus::Active => "active",
            GoalStatus::Paused => "paused",
            GoalStatus::BudgetLimited => "budget_limited",
            GoalStatus::Complete => "complete",
        }
    }

    pub fn from_str(s: &str) -> Self {
        match s {
            "active" => GoalStatus::Active,
            "paused" => GoalStatus::Paused,
            "budget_limited" => GoalStatus::BudgetLimited,
            "complete" => GoalStatus::Complete,
            unknown => {
                tracing::warn!(%unknown, "unknown GoalStatus value, defaulting to Active");
                GoalStatus::Active
            }
        }
    }
}

/// Why the goal was paused. Drives auto-resume behavior.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PauseReason {
    /// User explicitly called /goal pause — requires manual /goal resume
    UserRequested,
    /// Agent called clarify tool — auto-resume on user reply
    ClarifyPending,
    /// Agent called update_plan — auto-resume on plan approval
    PlanPending,
    /// Agent has not performed tool calls for consecutive turns — auto-resume
    /// on user message
    IdleBlocked,
    /// Turn budget exhausted — requires explicit /goal resume
    BudgetExhausted,
    /// Run interrupted (Ctrl+C / cancel) — auto-resume on next user message
    Interrupted,
}

impl PauseReason {
    pub fn as_str(&self) -> &'static str {
        match self {
            PauseReason::UserRequested => "user_requested",
            PauseReason::ClarifyPending => "clarify_pending",
            PauseReason::PlanPending => "plan_pending",
            PauseReason::IdleBlocked => "idle_blocked",
            PauseReason::BudgetExhausted => "budget_exhausted",
            PauseReason::Interrupted => "interrupted",
        }
    }

    pub fn from_str(s: &str) -> Self {
        match s {
            "user_requested" => PauseReason::UserRequested,
            "clarify_pending" => PauseReason::ClarifyPending,
            "plan_pending" => PauseReason::PlanPending,
            "idle_blocked" => PauseReason::IdleBlocked,
            "budget_exhausted" => PauseReason::BudgetExhausted,
            "interrupted" => PauseReason::Interrupted,
            unknown => {
                tracing::warn!(%unknown, "unknown PauseReason value, defaulting to UserRequested");
                PauseReason::UserRequested
            }
        }
    }

    /// Whether the goal should auto-resume when the user sends a new message.
    pub fn auto_resume_on_user_message(&self) -> bool {
        matches!(
            self,
            PauseReason::ClarifyPending
                | PauseReason::PlanPending
                | PauseReason::IdleBlocked
                | PauseReason::Interrupted
        )
    }
}

/// Verdict from the post-turn evaluation.
#[derive(Debug, Clone)]
pub enum GoalVerdict {
    /// Goal is still active — inject continuation prompt
    Continue,
    /// Model claimed completion but evidence is missing — inject challenge
    ChallengeEvidence,
    /// Goal achieved with evidence
    Complete { evidence: String },
    /// Goal paused for a specific reason
    Paused {
        reason: PauseReason,
        detail: Option<String>,
    },
    /// Token budget exhausted
    BudgetLimited,
}

/// Internal database record.
#[derive(Debug, Clone)]
pub struct GoalRecord {
    pub id: String,
    pub thread_id: String,
    pub objective: String,
    pub status: GoalStatus,
    pub token_budget: Option<i64>,
    pub tokens_used: i64,
    pub time_used_seconds: i64,
    pub turns_used: i64,
    pub max_turns: i64,
    pub pause_reason: Option<PauseReason>,
    pub pause_detail: Option<String>,
    pub evidence: Option<String>,
    pub last_evaluated_run_id: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// DTO sent to the frontend.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GoalDto {
    pub id: String,
    pub thread_id: String,
    pub objective: String,
    pub status: GoalStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub token_budget: Option<i64>,
    pub tokens_used: i64,
    pub time_used_seconds: i64,
    pub turns_used: i64,
    pub max_turns: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pause_reason: Option<PauseReason>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pause_detail: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub evidence: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_evaluated_run_id: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

impl From<GoalRecord> for GoalDto {
    fn from(r: GoalRecord) -> Self {
        Self {
            id: r.id,
            thread_id: r.thread_id,
            objective: r.objective,
            status: r.status,
            token_budget: r.token_budget,
            tokens_used: r.tokens_used,
            time_used_seconds: r.time_used_seconds,
            turns_used: r.turns_used,
            max_turns: r.max_turns,
            pause_reason: r.pause_reason,
            pause_detail: r.pause_detail,
            evidence: r.evidence,
            last_evaluated_run_id: r.last_evaluated_run_id,
            created_at: r.created_at.to_rfc3339(),
            updated_at: r.updated_at.to_rfc3339(),
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GoalCreateInput {
    pub objective: String,
    pub token_budget: Option<i64>,
}

/// Lightweight goal payload for ThreadStreamEvent — avoids cloning the full record.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GoalPayload {
    pub id: String,
    pub thread_id: String,
    pub objective: String,
    pub status: GoalStatus,
    pub tokens_used: i64,
    pub time_used_seconds: i64,
    pub turns_used: i64,
    pub max_turns: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub token_budget: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pause_reason: Option<PauseReason>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pause_detail: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub evidence: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_evaluated_run_id: Option<String>,
}

impl From<GoalRecord> for GoalPayload {
    fn from(r: GoalRecord) -> Self {
        Self {
            id: r.id,
            thread_id: r.thread_id,
            objective: r.objective,
            status: r.status,
            tokens_used: r.tokens_used,
            time_used_seconds: r.time_used_seconds,
            turns_used: r.turns_used,
            max_turns: r.max_turns,
            token_budget: r.token_budget,
            pause_reason: r.pause_reason,
            pause_detail: r.pause_detail,
            evidence: r.evidence,
            last_evaluated_run_id: r.last_evaluated_run_id,
        }
    }
}
