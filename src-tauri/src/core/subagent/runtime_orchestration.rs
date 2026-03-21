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
    pub fn all() -> [Self; 2] {
        [Self::DelegateResearch, Self::DelegateCodeReview]
    }

    pub fn parse(tool_name: &str) -> Option<Self> {
        match tool_name {
            "agent_research" => Some(Self::DelegateResearch),
            "agent_plan" => Some(Self::DelegatePlanReview),
            "agent_review" => Some(Self::DelegateCodeReview),
            _ => None,
        }
    }

    pub fn tool_name(self) -> &'static str {
        match self {
            Self::DelegateResearch => "agent_research",
            Self::DelegatePlanReview => "agent_review",
            Self::DelegateCodeReview => "agent_review",
        }
    }

    pub fn title(self) -> &'static str {
        match self {
            Self::DelegateResearch => "Agent Research",
            Self::DelegatePlanReview => "Agent Review",
            Self::DelegateCodeReview => "Agent Review",
        }
    }

    pub fn description(self) -> &'static str {
        match self {
            Self::DelegateResearch => {
                "Investigate unfamiliar or cross-file areas before acting. Use this to gather evidence, relevant files, dependencies, and architecture context, then return a concise summary to the parent agent."
            }
            Self::DelegatePlanReview => {
                "Review a proposed implementation plan before coding. Focus on risks, missing steps, edge cases, and better sequencing, then return concise recommendations to the parent agent."
            }
            Self::DelegateCodeReview => {
                "Review a plan, code change, or diff and return risks, regressions, gaps, and concrete follow-ups. Use target='plan' before implementation and target='code' or 'diff' after implementation."
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
        let parameters = match self {
            Self::DelegateResearch => serde_json::json!({
                "type": "object",
                "properties": {
                    "task": {
                        "type": "string",
                        "description": "What to investigate. Include the user goal, suspected files or subsystems, and the kind of evidence you want back."
                    }
                },
                "required": ["task"]
            }),
            Self::DelegateCodeReview => serde_json::json!({
                "type": "object",
                "properties": {
                    "task": {
                        "type": "string",
                        "description": "What to review. Summarize the plan, code, or diff, and call out the main risks or questions to check."
                    },
                    "target": {
                        "type": "string",
                        "enum": ["plan", "code", "diff"],
                        "description": "Review focus. Use 'plan' before implementation. Use 'code' or 'diff' after implementation. If omitted, review defaults to code-level review."
                    }
                },
                "required": ["task"]
            }),
            Self::DelegatePlanReview => serde_json::json!({
                "type": "object",
                "properties": {
                    "task": {
                        "type": "string",
                        "description": "What plan to review and what risks or concerns to stress-test."
                    }
                },
                "required": ["task"]
            }),
        };

        AgentTool::new(
            self.tool_name(),
            self.title(),
            self.description(),
            parameters,
        )
    }
}

impl SubagentProfile {
    pub fn helper_kind(self) -> &'static str {
        match self {
            Self::Scout => "helper_scout",
            Self::Planner => "helper_plan_reviewer",
            Self::Reviewer => "helper_reviewer",
        }
    }

    pub fn system_prompt(self) -> &'static str {
        match self {
            Self::Scout => {
                "You are an internal scout helper. Your job is to investigate the workspace and gather context for the parent agent.\n\
Guidelines:\n\
- Stay strictly read-only. Do not modify any files.\n\
- Use search and find to locate relevant code efficiently. Read files to understand implementation details.\n\
- Focus on what matters: relevant files, key data structures, dependencies, and patterns.\n\
- Omit irrelevant noise. If a file is not useful, skip it without comment."
            }
            Self::Planner => {
                "You are an internal planning helper. Your job is to analyze context and produce an actionable plan for the parent agent.\n\
Guidelines:\n\
- Stay strictly read-only. Do not modify any files.\n\
- Inspect relevant files to understand the current state before planning.\n\
- Identify risks, edge cases, and gaps in the proposed approach.\n\
- Return a concrete, ordered list of next steps. Each step should name specific files and functions to change."
            }
            Self::Reviewer => {
                "You are an internal review helper. Your job is to evaluate code or plans and provide constructive feedback.\n\
Guidelines:\n\
- Stay strictly read-only. Do not modify any files.\n\
- Use repository inspection tools. Check terminal output when it directly supports the review.\n\
- Focus on correctness, edge cases, error handling, and consistency with existing patterns.\n\
- Distinguish critical issues from suggestions. Be specific: reference file paths and line ranges."
            }
        }
    }

    pub fn helper_tools(self) -> Vec<AgentTool> {
        let mut tools = vec![
            AgentTool::new(
                "read",
                "Read File",
                "Read a file inside the current workspace.",
                serde_json::json!({
                    "type": "object",
                    "properties": { "path": { "type": "string" } },
                    "required": ["path"]
                }),
            ),
            AgentTool::new(
                "list",
                "List Directory",
                "List files and folders inside the current workspace.",
                serde_json::json!({
                    "type": "object",
                    "properties": { "path": { "type": "string" } }
                }),
            ),
            AgentTool::new(
                "find",
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
                "search",
                "Search Repo",
                "Search the current workspace with ripgrep. Results are preview-limited for safety; omit wildcard-only filePattern values like '*' or '**/*'.",
                serde_json::json!({
                    "type": "object",
                    "properties": {
                        "query": {
                            "type": "string",
                            "description": "Search term or regex."
                        },
                        "directory": {
                            "type": "string",
                            "description": "Directory to search in (default: workspace root)."
                        },
                        "filePattern": {
                            "type": "string",
                            "description": "Optional glob filter such as '*.rs' or 'src/**/*.ts'. Omit it to search all files; do not pass '*' or '**/*'."
                        },
                        "maxResults": {
                            "type": "integer",
                            "description": "Optional preview limit for returned matches. Defaults to 100 and is capped for context safety."
                        }
                    },
                    "required": ["query"]
                }),
            ),
        ];

        if self == Self::Reviewer {
            tools.extend([
                AgentTool::new(
                    "term_status",
                    "Terminal Status",
                    "Inspect the current thread terminal status without mutating it.",
                    serde_json::json!({
                        "type": "object",
                        "properties": {}
                    }),
                ),
                AgentTool::new(
                    "term_output",
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
            RuntimeOrchestrationTool::parse("agent_research"),
            Some(RuntimeOrchestrationTool::DelegateResearch)
        );
        assert_eq!(
            RuntimeOrchestrationTool::parse("agent_plan"),
            Some(RuntimeOrchestrationTool::DelegatePlanReview)
        );
        assert_eq!(
            RuntimeOrchestrationTool::parse("agent_review"),
            Some(RuntimeOrchestrationTool::DelegateCodeReview)
        );
        assert_eq!(RuntimeOrchestrationTool::parse("read"), None);
    }

    #[test]
    fn reviewer_profile_includes_terminal_tools() {
        let tools = SubagentProfile::Reviewer.helper_tools();
        let tool_names: Vec<&str> = tools.iter().map(|tool| tool.name.as_str()).collect();

        assert!(tool_names.contains(&"term_status"));
        assert!(tool_names.contains(&"term_output"));
    }

    #[test]
    fn runtime_orchestration_tool_catalog_has_all_delegate_tools() {
        let tools = runtime_orchestration_tools();
        let tool_names: Vec<&str> = tools.iter().map(|tool| tool.name.as_str()).collect();

        assert_eq!(tool_names, vec!["agent_research", "agent_review"]);
    }
}
