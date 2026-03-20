use tiy_core::agent::AgentTool;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeOrchestrationTool {
    DelegateResearch,
    DelegatePlanReview,
    DelegateCodeReview,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SubagentProfile {
    Scout,
    Planner,
    Reviewer,
}

pub fn runtime_orchestration_tools() -> Vec<AgentTool> {
    RuntimeOrchestrationTool::all()
        .into_iter()
        .map(RuntimeOrchestrationTool::as_agent_tool)
        .collect()
}

impl RuntimeOrchestrationTool {
    pub fn all() -> [Self; 3] {
        [
            Self::DelegateResearch,
            Self::DelegatePlanReview,
            Self::DelegateCodeReview,
        ]
    }

    pub fn parse(tool_name: &str) -> Option<Self> {
        match tool_name {
            "delegate_research" => Some(Self::DelegateResearch),
            "delegate_plan_review" => Some(Self::DelegatePlanReview),
            "delegate_code_review" => Some(Self::DelegateCodeReview),
            _ => None,
        }
    }

    pub fn tool_name(self) -> &'static str {
        match self {
            Self::DelegateResearch => "delegate_research",
            Self::DelegatePlanReview => "delegate_plan_review",
            Self::DelegateCodeReview => "delegate_code_review",
        }
    }

    pub fn title(self) -> &'static str {
        match self {
            Self::DelegateResearch => "Delegate Research",
            Self::DelegatePlanReview => "Delegate Plan Review",
            Self::DelegateCodeReview => "Delegate Code Review",
        }
    }

    pub fn description(self) -> &'static str {
        match self {
            Self::DelegateResearch => {
                "Run a scoped helper agent to investigate a question and return a summary."
            }
            Self::DelegatePlanReview => {
                "Run a scoped helper agent to review a plan and return a summary."
            }
            Self::DelegateCodeReview => {
                "Run a scoped helper agent to review code and return a summary."
            }
        }
    }

    pub fn profile(self) -> SubagentProfile {
        match self {
            Self::DelegateResearch => SubagentProfile::Scout,
            Self::DelegatePlanReview => SubagentProfile::Planner,
            Self::DelegateCodeReview => SubagentProfile::Reviewer,
        }
    }

    pub fn as_agent_tool(self) -> AgentTool {
        AgentTool::new(
            self.tool_name(),
            self.title(),
            self.description(),
            serde_json::json!({
                "type": "object",
                "properties": { "task": { "type": "string" } },
                "required": ["task"]
            }),
        )
    }
}

impl SubagentProfile {
    pub fn helper_kind(self) -> &'static str {
        match self {
            Self::Scout => "helper_scout",
            Self::Planner => "helper_planner",
            Self::Reviewer => "helper_reviewer",
        }
    }

    pub fn system_prompt(self) -> &'static str {
        match self {
            Self::Scout => {
                "You are an internal scout helper. Stay read-only, inspect the workspace with allowed tools, and summarize only the findings that matter to the parent run."
            }
            Self::Planner => {
                "You are an internal planning helper. Stay read-only, inspect relevant files, and return concise risks, gaps, and next-step suggestions for the parent run."
            }
            Self::Reviewer => {
                "You are an internal review helper. Stay read-only, use allowed repository inspection tools, and optionally inspect read-only terminal state when it directly supports the review."
            }
        }
    }

    pub fn helper_tools(self) -> Vec<AgentTool> {
        let mut tools = vec![
            AgentTool::new(
                "read_file",
                "Read File",
                "Read a file inside the current workspace.",
                serde_json::json!({
                    "type": "object",
                    "properties": { "path": { "type": "string" } },
                    "required": ["path"]
                }),
            ),
            AgentTool::new(
                "list_dir",
                "List Directory",
                "List files and folders inside the current workspace.",
                serde_json::json!({
                    "type": "object",
                    "properties": { "path": { "type": "string" } }
                }),
            ),
            AgentTool::new(
                "find_files",
                "Find Files",
                "Search for files by glob pattern. Returns matching file paths relative to the workspace.",
                serde_json::json!({
                    "type": "object",
                    "properties": {
                        "pattern": { "type": "string", "description": "Glob pattern, e.g. '*.ts', '*.json'" },
                        "path": { "type": "string", "description": "Directory to search in (default: workspace root)" }
                    },
                    "required": ["pattern"]
                }),
            ),
            AgentTool::new(
                "search_repo",
                "Search Repo",
                "Search the current workspace with ripgrep.",
                serde_json::json!({
                    "type": "object",
                    "properties": {
                        "query": { "type": "string" },
                        "directory": { "type": "string" },
                        "filePattern": { "type": "string" }
                    },
                    "required": ["query"]
                }),
            ),
        ];

        if self == Self::Reviewer {
            tools.extend([
                AgentTool::new(
                    "terminal_get_status",
                    "Terminal Status",
                    "Inspect the current thread terminal status without mutating it.",
                    serde_json::json!({
                        "type": "object",
                        "properties": {}
                    }),
                ),
                AgentTool::new(
                    "terminal_get_recent_output",
                    "Terminal Output",
                    "Read the recent terminal output for the current thread.",
                    serde_json::json!({
                        "type": "object",
                        "properties": {}
                    }),
                ),
            ]);
        }

        tools
    }
}

#[cfg(test)]
mod tests {
    use super::{runtime_orchestration_tools, RuntimeOrchestrationTool, SubagentProfile};

    #[test]
    fn parses_runtime_orchestration_tools() {
        assert_eq!(
            RuntimeOrchestrationTool::parse("delegate_research"),
            Some(RuntimeOrchestrationTool::DelegateResearch)
        );
        assert_eq!(
            RuntimeOrchestrationTool::parse("delegate_plan_review"),
            Some(RuntimeOrchestrationTool::DelegatePlanReview)
        );
        assert_eq!(
            RuntimeOrchestrationTool::parse("delegate_code_review"),
            Some(RuntimeOrchestrationTool::DelegateCodeReview)
        );
        assert_eq!(RuntimeOrchestrationTool::parse("read_file"), None);
    }

    #[test]
    fn reviewer_profile_includes_terminal_tools() {
        let tools = SubagentProfile::Reviewer.helper_tools();
        let tool_names: Vec<&str> = tools.iter().map(|tool| tool.name.as_str()).collect();

        assert!(tool_names.contains(&"terminal_get_status"));
        assert!(tool_names.contains(&"terminal_get_recent_output"));
    }

    #[test]
    fn runtime_orchestration_tool_catalog_has_all_delegate_tools() {
        let tools = runtime_orchestration_tools();
        let tool_names: Vec<&str> = tools.iter().map(|tool| tool.name.as_str()).collect();

        assert_eq!(
            tool_names,
            vec![
                "delegate_research",
                "delegate_plan_review",
                "delegate_code_review"
            ]
        );
    }
}
