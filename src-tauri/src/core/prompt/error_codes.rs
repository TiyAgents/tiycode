/// SoftFailed error code constants.
/// All SectionOutcome::SoftFailed codes must be registered here.
/// See § 3.18 failure-explainability requirement.
pub mod codes {
    // ---- Template errors ----
    /// Template file not found at compile time
    pub const TEMPLATE_NOT_FOUND: &str = "template.not_found";
    /// Template is missing a declared placeholder key
    pub const TEMPLATE_MISSING_KEY: &str = "template.missing_key";
    /// Template has an undeclared placeholder key
    pub const TEMPLATE_UNDECLARED_KEY: &str = "template.undeclared_key";

    // ---- Source errors ----
    /// Source execution timed out
    pub const SOURCE_TIMEOUT: &str = "source.timeout";
    /// Source cyclically depends on another signal
    pub const SOURCE_SIGNAL_CYCLE: &str = "source.signal_cycle";
    /// Signal computation failed
    pub const SOURCE_SIGNAL_FAILED: &str = "source.signal_failed";

    // ---- I/O errors ----
    /// Failed to read workspace file (AGENTS.md etc.)
    pub const IO_WORKSPACE_READ: &str = "io.workspace_read";
    /// Failed to load skills from DB
    pub const SKILLS_LOAD_FAILED: &str = "skills.load_failed";
    /// Failed to load profile from DB
    pub const PROFILE_LOAD_FAILED: &str = "profile.load_failed";
    /// Failed to load plan checkpoint
    pub const PLAN_LOAD_FAILED: &str = "plan.load_failed";
    /// Failed to load active goal
    pub const GOAL_LOAD_FAILED: &str = "goal.load_failed";

    // ---- Budget errors ----
    /// Section truncated by per-section budget
    pub const BUDGET_TRUNCATED: &str = "budget.truncated";
    /// Section evicted by total budget
    pub const BUDGET_EVICTED: &str = "budget.evicted";
}

/// All registered error codes for startup lint test.
pub const ALL_ERROR_CODES: &[&str] = &[
    codes::TEMPLATE_NOT_FOUND,
    codes::TEMPLATE_MISSING_KEY,
    codes::TEMPLATE_UNDECLARED_KEY,
    codes::SOURCE_TIMEOUT,
    codes::SOURCE_SIGNAL_CYCLE,
    codes::SOURCE_SIGNAL_FAILED,
    codes::IO_WORKSPACE_READ,
    codes::SKILLS_LOAD_FAILED,
    codes::PROFILE_LOAD_FAILED,
    codes::PLAN_LOAD_FAILED,
    codes::GOAL_LOAD_FAILED,
    codes::BUDGET_TRUNCATED,
    codes::BUDGET_EVICTED,
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_codes_registered() {
        let expected: &[&str] = &[
            codes::TEMPLATE_NOT_FOUND,
            codes::TEMPLATE_MISSING_KEY,
            codes::TEMPLATE_UNDECLARED_KEY,
            codes::SOURCE_TIMEOUT,
            codes::SOURCE_SIGNAL_CYCLE,
            codes::SOURCE_SIGNAL_FAILED,
            codes::IO_WORKSPACE_READ,
            codes::SKILLS_LOAD_FAILED,
            codes::PROFILE_LOAD_FAILED,
            codes::PLAN_LOAD_FAILED,
            codes::GOAL_LOAD_FAILED,
            codes::BUDGET_TRUNCATED,
            codes::BUDGET_EVICTED,
        ];

        let mut all_codes: Vec<_> = ALL_ERROR_CODES.to_vec();
        all_codes.sort();
        let mut expected_sorted: Vec<_> = expected.to_vec();
        expected_sorted.sort();

        assert_eq!(
            all_codes, expected_sorted,
            "ALL_ERROR_CODES is out of sync with codes module constants"
        );
    }
}
