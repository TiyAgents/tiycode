/// Typed run mode representing the agent's execution mode.
/// Replaces the old `&str` pattern ("plan" / "default").
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RunMode {
    /// Plan mode: agent only researches and produces a plan, no mutations
    Plan,
    /// Default mode: agent can execute tools according to policy
    Default,
}

impl RunMode {
    pub fn as_str(&self) -> &'static str {
        match self {
            RunMode::Plan => "plan",
            RunMode::Default => "default",
        }
    }

    pub fn from_str(s: &str) -> Self {
        match s {
            "plan" => RunMode::Plan,
            _ => RunMode::Default,
        }
    }
}

impl std::fmt::Display for RunMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}
