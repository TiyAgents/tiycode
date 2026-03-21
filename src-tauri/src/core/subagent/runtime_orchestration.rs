use tiy_core::agent::AgentTool;

pub const TERM_STATUS_TOOL_DESCRIPTION: &str =
    "Inspect the status of the desktop app's embedded Terminal panel session for the current thread. Use this to check that panel's session state without mutating it. It does not inspect the agent runtime, CLI process, or host shell outside the panel.";
pub const TERM_OUTPUT_TOOL_DESCRIPTION: &str =
    "Read recent buffered output from the desktop app's embedded Terminal panel session for the current thread. Use this to inspect logs, prompts, or command results already shown in that panel. It does not read the agent runtime's own stdout/stderr or any shell outside the panel.";
pub const TERM_WRITE_TOOL_DESCRIPTION: &str =
    "Send input to the desktop app's embedded Terminal panel session for the current thread. Use this only to continue or control that panel's persistent session; do not use it as a replacement for one-shot shell execution.";
pub const TERM_RESTART_TOOL_DESCRIPTION: &str =
    "Restart the desktop app's embedded Terminal panel session for the current thread, optionally with new terminal dimensions. Use this when that panel session needs a clean restart; it does not restart the agent runtime itself.";
pub const TERM_CLOSE_TOOL_DESCRIPTION: &str =
    "Close the desktop app's embedded Terminal panel session for the current thread. Use this only to stop that panel's persistent session; it does not stop the agent runtime or close the desktop app.";
pub const TERM_PANEL_USAGE_NOTE: &str =
    "term_status and term_output refer to the desktop app's embedded Terminal panel for the current thread. Use them only for that panel's session state and recent buffered output; they do not inspect your own runtime, CLI session, or host shell outside the panel.";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeOrchestrationTool {
    Explore,
    Review,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SubagentProfile {
    Explore,
    Review,
}

pub fn runtime_orchestration_tools() -> Vec<AgentTool> {
    RuntimeOrchestrationTool::all()
        .into_iter()
        .map(RuntimeOrchestrationTool::as_agent_tool)
        .collect()
}

impl RuntimeOrchestrationTool {
    pub fn all() -> [Self; 2] {
        [Self::Explore, Self::Review]
    }

    pub fn parse(tool_name: &str) -> Option<Self> {
        match tool_name {
            "agent_explore" => Some(Self::Explore),
            "agent_review" => Some(Self::Review),
            _ => None,
        }
    }

    pub fn tool_name(self) -> &'static str {
        match self {
            Self::Explore => "agent_explore",
            Self::Review => "agent_review",
        }
    }

    pub fn title(self) -> &'static str {
        match self {
            Self::Explore => "Agent Explore",
            Self::Review => "Agent Review",
        }
    }

    pub fn description(self) -> &'static str {
        match self {
            Self::Explore => {
                "Explore unfamiliar or cross-file areas before acting. Use this for current-state analysis, fact-finding, code reading, architecture summaries, and evidence gathering, then return a concise summary to the parent agent."
            }
            Self::Review => {
                "Review an implemented code change or diff and return risks, regressions, gaps, and concrete follow-ups. Use this after implementation to stress-test the work."
            }
        }
    }

    pub fn profile(self) -> SubagentProfile {
        match self {
            Self::Explore => SubagentProfile::Explore,
            Self::Review => SubagentProfile::Review,
        }
    }

    pub fn as_agent_tool(self) -> AgentTool {
        let parameters = match self {
            Self::Explore => serde_json::json!({
                "type": "object",
                "properties": {
                    "task": {
                        "type": "string",
                        "description": "What to explore or analyze. Include the user goal, suspected files or subsystems, and the kind of evidence, explanation, or current-state summary you want back."
                    }
                },
                "required": ["task"]
            }),
            Self::Review => serde_json::json!({
                "type": "object",
                "properties": {
                    "task": {
                        "type": "string",
                        "description": "What to review. Summarize the implemented code or diff and call out the main risks or questions to check."
                    },
                    "target": {
                        "type": "string",
                        "enum": ["code", "diff"],
                        "description": "Review focus. Use 'code' for the current implementation or 'diff' for a patch-oriented pass. If omitted, review defaults to code-level review."
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
            Self::Explore => "helper_explore",
            Self::Review => "helper_review",
        }
    }

    pub fn system_prompt(self) -> &'static str {
        match self {
            Self::Explore => {
                "You are an internal explore helper. Your job is to investigate the workspace and gather context for the parent agent.\n\
Guidelines:\n\
- Stay strictly read-only. Do not modify any files.\n\
- Use search and find to locate relevant code efficiently. Read files to understand implementation details.\n\
- Focus on what matters: relevant files, key data structures, dependencies, and patterns.\n\
- Omit irrelevant noise. If a file is not useful, skip it without comment."
            }
            Self::Review => {
                "You are an internal review helper. Your job is to evaluate implemented code or diffs and provide constructive feedback.\n\
Guidelines:\n\
- Stay strictly read-only. Do not modify any files.\n\
- Use repository inspection tools. Check the current thread's Terminal panel output when it directly supports the review.\n\
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

        if self == Self::Review {
            tools.extend([
                AgentTool::new(
                    "term_status",
                    "Terminal Status",
                    TERM_STATUS_TOOL_DESCRIPTION,
                    serde_json::json!({
                        "type": "object",
                        "properties": {}
                    }),
                ),
                AgentTool::new(
                    "term_output",
                    "Terminal Output",
                    TERM_OUTPUT_TOOL_DESCRIPTION,
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
            RuntimeOrchestrationTool::parse("agent_explore"),
            Some(RuntimeOrchestrationTool::Explore)
        );
        assert_eq!(
            RuntimeOrchestrationTool::parse("agent_review"),
            Some(RuntimeOrchestrationTool::Review)
        );
        assert_eq!(RuntimeOrchestrationTool::parse("read"), None);
    }

    #[test]
    fn reviewer_profile_includes_terminal_tools() {
        let tools = SubagentProfile::Review.helper_tools();
        let tool_names: Vec<&str> = tools.iter().map(|tool| tool.name.as_str()).collect();

        assert!(tool_names.contains(&"term_status"));
        assert!(tool_names.contains(&"term_output"));
    }

    #[test]
    fn reviewer_terminal_tool_descriptions_clarify_terminal_panel_scope() {
        let tools = SubagentProfile::Review.helper_tools();
        let status_tool = tools
            .iter()
            .find(|tool| tool.name == "term_status")
            .expect("term_status tool");
        let output_tool = tools
            .iter()
            .find(|tool| tool.name == "term_output")
            .expect("term_output tool");

        assert!(status_tool.description.contains("Terminal panel"));
        assert!(status_tool
            .description
            .contains("does not inspect the agent runtime"));
        assert!(output_tool.description.contains("Terminal panel"));
        assert!(output_tool
            .description
            .contains("does not read the agent runtime"));
    }

    #[test]
    fn runtime_orchestration_tool_catalog_has_all_delegate_tools() {
        let tools = runtime_orchestration_tools();
        let tool_names: Vec<&str> = tools.iter().map(|tool| tool.name.as_str()).collect();

        assert_eq!(tool_names, vec!["agent_explore", "agent_review"]);
    }
}
