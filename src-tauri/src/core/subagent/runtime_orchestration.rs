use tiycore::agent::AgentTool;

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
                "Review an implemented code change or diff, run the necessary type-check and test commands, and return risks, regressions, verification results, and concrete follow-ups. Use this after implementation to stress-test the work."
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
                        "description": "What to review. Summarize the implemented code or diff, call out the main risks or questions to check, and mention any project-specific type-check or test commands the helper should prioritize."
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
- Omit irrelevant noise. If a file is not useful, skip it without comment.\n\
\n\
Tool-use protocol:\n\
- Tool calls must strictly match each tool's JSON schema. Treat the schema as a hard protocol, not a suggestion.\n\
- Never invent field names, omit required fields, pass an empty object, or call a tool before you know the required arguments.\n\
- Before every tool call, verify which tool you are calling, which fields are required, whether you have concrete values for all required fields, and whether the field names are exactly correct.\n\
- If any required field is missing or uncertain, do not call the tool yet. Use another valid tool call to gather the missing context, or explain what input is missing.\n\
- If a tool call fails because your arguments were invalid, do not repeat the same invalid call. Read the error, correct the arguments, and only then try again.\n\
- Do not claim that tools are unavailable, broken, or unusable unless you have evidence of a system-level failure. A single invalid tool call means your arguments were wrong, not that the tool system is broken.\n\
- For this helper, pay special attention to required fields: `read` requires `path`, `find` requires `pattern`, and `search` requires `query`. `list` may omit `path`, but include it when it helps narrow the scope.\n\
- `search` defaults to literal matching. Only treat the query as a regular expression when you explicitly set `queryMode` to `regex`. Prefer simple literal keywords first, and only opt into regex when you need pattern matching.\n\
\n\
Examples:\n\
- Bad tool calls: `search {}`, `read {}`, `find {}`, `search {\"path\":\"src\"}`, `read {\"query\":\"title\"}`.\n\
- Good tool calls: `search {\"query\":\"thread title\"}`, `find {\"pattern\":\"*thread*title*\",\"path\":\"src\"}`, `read {\"path\":\"src/modules/workbench-shell/ui/runtime-thread-surface.tsx\"}`.\n\
- Prefer this workflow when investigating code: first use `find` to locate likely files, then use `search` to locate relevant text or symbols, then use `read` to inspect the exact implementation. Only call a tool once you know the required arguments."
            }
            Self::Review => {
                "You are an internal review helper. Your job is to evaluate implemented code or diffs, run verification commands, and provide constructive feedback.\n\
Guidelines:\n\
- Do not modify any files. Only use the shell tool for read-only diagnostic commands.\n\
- Use repository inspection tools. Check the current thread's Terminal panel output when it directly supports the review.\n\
- Focus on correctness, edge cases, error handling, and consistency with existing patterns.\n\
- Distinguish critical issues from suggestions. Be specific: reference file paths and line ranges.\n\
\n\
Verification:\n\
- After reviewing code or diffs, determine the necessary project type-check and test commands, then run them with the shell tool (e.g. `npm run typecheck`, `cargo test`, or whatever the project uses). This is mandatory, not optional.\n\
- If the workspace instructions or project config indicate specific build/test commands, prefer those.\n\
- Treat this verification work as part of your core responsibility so the parent agent does not need to duplicate it by default.\n\
- If the shell tool is unavailable or a command is rejected by the approval policy, explicitly state in your summary that manual verification is still needed and list the exact commands the parent agent should run.\n\
\n\
Return format:\n\
- Structure your response so the parent agent can quickly assess implementation status.\n\
- Lead with an overall verdict: PASS, FAIL, or NEEDS ATTENTION.\n\
- Section 1 — Review findings: critical issues, warnings, and suggestions with file paths and line ranges.\n\
- Section 2 — Verification results: for each command run, state the command, whether it passed or failed, and quote key error output (truncated if long). If verification was skipped, say so and list the commands that need manual execution.\n\
- Section 3 — Parent agent follow-up: say `none` when verification is complete and the parent agent does not need to rerun the same type-check or test commands. Otherwise list the exact remaining verification commands, why they still need manual execution, and any other action the parent agent should take.\n\
- Keep the summary concise. The parent agent needs actionable signal, not exhaustive logs.\n\
- When reviewing documents, architecture specs, or design proposals (as opposed to code), prefer a discursive, paragraph-oriented format over bullet-heavy enumeration. Group related observations into themed paragraphs with clear topic sentences. Reserve bullet lists for genuinely discrete, independent items like a checklist of action items."
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
                "Search the current workspace with a built-in cross-platform search engine. Supports literal or regex queries, optional context lines, file glob filters, and files/count output modes. Results are preview-limited for safety; omit wildcard-only filePattern values like '*' or '**/*'.",
                serde_json::json!({
                    "type": "object",
                    "properties": {
                        "query": {
                            "type": "string",
                            "description": "Search text or regex pattern. Defaults to literal mode, so special regex characters are matched as plain text unless queryMode='regex'."
                        },
                        "directory": {
                            "type": "string",
                            "description": "Directory to search in (default: workspace root)."
                        },
                        "filePattern": {
                            "type": "string",
                            "description": "Optional glob filter such as '*.rs' or 'src/**/*.ts'. Omit it to search all files; do not pass '*' or '**/*'."
                        },
                        "type": {
                            "type": "string",
                            "description": "Optional file type filter such as 'rust', 'ts', 'js', 'py', 'go', or 'json'. More natural than filePattern for language-targeted searches."
                        },
                        "maxResults": {
                            "type": "integer",
                            "description": "Optional preview limit for returned matches. Defaults to 100 and is capped for context safety."
                        },
                        "offset": {
                            "type": "integer",
                            "description": "Optional number of matches or files to skip before collecting results."
                        },
                        "queryMode": {
                            "type": "string",
                            "enum": ["literal", "regex"],
                            "description": "Use 'literal' for plain text matching (default) or 'regex' for regular expression search."
                        },
                        "outputMode": {
                            "type": "string",
                            "enum": ["content", "files_with_matches", "count"],
                            "description": "Choose 'content' for matching lines, 'files_with_matches' for unique matching files, or 'count' for per-file match counts."
                        },
                        "caseInsensitive": {
                            "type": "boolean",
                            "description": "Set true for case-insensitive matching."
                        },
                        "context": {
                            "type": "integer",
                            "description": "Optional number of context lines to include before and after each match in content mode."
                        },
                        "beforeContext": {
                            "type": "integer",
                            "description": "Optional number of lines to include before each match in content mode. Overrides the shared context value for the before side."
                        },
                        "afterContext": {
                            "type": "integer",
                            "description": "Optional number of lines to include after each match in content mode. Overrides the shared context value for the after side."
                        },
                        "timeoutMs": {
                            "type": "integer",
                            "description": "Optional search timeout in milliseconds. When the timeout is hit, the tool returns partial results and marks the response as incomplete."
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
                AgentTool::new(
                    "shell",
                    "Run Command",
                    "Run a non-interactive shell command inside the current workspace. Use this only for diagnostic and verification commands such as type-checking and test suites. Do not use it to modify files or state.",
                    serde_json::json!({
                        "type": "object",
                        "properties": {
                            "command": { "type": "string" },
                            "cwd": { "type": "string" },
                            "timeout": { "type": "number" }
                        },
                        "required": ["command"]
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

    #[test]
    fn agent_review_tool_description_mentions_verification_ownership() {
        let tool = RuntimeOrchestrationTool::Review.as_agent_tool();
        let task_description = tool.parameters["properties"]["task"]["description"]
            .as_str()
            .expect("task description should exist");

        assert!(tool.description.contains("type-check and test commands"));
        assert!(tool.description.contains("verification results"));
        assert!(task_description.contains("type-check or test commands"));
    }

    #[test]
    fn review_helper_prompt_requires_parent_follow_up_summary() {
        let prompt = SubagentProfile::Review.system_prompt();

        assert!(prompt.contains("This is mandatory, not optional"));
        assert!(prompt.contains("parent agent does not need to duplicate it by default"));
        assert!(prompt.contains("Section 3 — Parent agent follow-up"));
        assert!(prompt.contains("does not need to rerun the same type-check or test commands"));
    }
}
