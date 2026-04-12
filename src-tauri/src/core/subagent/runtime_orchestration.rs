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
                        "description": "Review focus. Use 'code' for the current implementation or 'diff' for a patch-oriented pass. Diff reviews default to a diff-first, global-aware pass."
                    },
                    "reviewScope": {
                        "type": "string",
                        "enum": ["local", "diff_first_global"],
                        "description": "Choose 'local' for a focused implementation review or 'diff_first_global' to start from the diff and then do a bounded global impact scan."
                    },
                    "globalScanMode": {
                        "type": "string",
                        "enum": ["off", "auto"],
                        "description": "Control the bounded global impact probe. Use 'auto' to inspect adjacent dependencies, exports, and tests when the diff suggests broader risk."
                    },
                    "changedFiles": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "Optional relative file paths that are already known to be in scope for the review."
                    },
                    "preferredChecks": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "Optional verification commands the helper should try first when they fit the active repository."
                    },
                    "riskHints": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "Optional risk cues such as cross_platform, persistence, schema, runtime, config, or tests."
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

    pub fn system_prompt(self) -> String {
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
- `search` may interpret the query as a regex-like pattern. Prefer simple literal keywords first. If the text includes regex-special characters, simplify the query or escape it before searching.\n\
\n\
Examples:\n\
- Bad tool calls: `search {}`, `read {}`, `find {}`, `search {\"path\":\"src\"}`, `read {\"query\":\"title\"}`.\n\
- Good tool calls: `search {\"query\":\"thread title\"}`, `find {\"pattern\":\"*thread*title*\",\"path\":\"src\"}`, `read {\"path\":\"src/modules/workbench-shell/ui/runtime-thread-surface.tsx\"}`.\n\
- Prefer this workflow when investigating code: first use `find` to locate likely files, then use `search` to locate relevant text or symbols, then use `read` to inspect the exact implementation. Only call a tool once you know the required arguments."
                    .to_string()
            }
            Self::Review => {
                "You are an internal review helper. Your job is to evaluate implemented code or diffs, run verification commands, and provide constructive feedback.\n\
Guidelines:\n\
- Do not modify any files. Only use the shell tool for read-only diagnostic commands.\n\
- Prefer repository inspection tools over shell whenever they fit. Use `git_status`, `git_diff`, and `git_log` for Git-aware inspection, then `read`, `search`, and `find` for exact implementation context.\n\
- Check the current thread's Terminal panel output when it directly supports the review.\n\
- Focus on correctness, edge cases, error handling, consistency with existing patterns, and repository-appropriate conventions for the active project.\n\
- Adapt to the current stack. Infer build, test, and project structure from repository files and instructions instead of assuming a particular framework.\n\
- Distinguish direct diff problems from wider system-impact risks. Be specific: reference file paths and line ranges when available.\n\
\n\
Verification:\n\
- After reviewing code or diffs, determine the necessary project type-check and test commands, then run them with the shell tool (e.g. `npm run typecheck`, `cargo test`, or whatever the project uses). This is mandatory, not optional.\n\
- If the workspace instructions or project config indicate specific build/test commands, prefer those.\n\
- Treat this verification work as part of your core responsibility so the parent agent does not need to duplicate it by default.\n\
- If the shell tool is unavailable or a command is rejected by the approval policy, explicitly state in your summary that manual verification is still needed and list the exact commands the parent agent should run.\n\
\n\
Diff-first, global-aware review behavior:\n\
- When the request target is `diff`, begin from the current workspace changes. Use `git_status` and `git_diff` when the changed file list is not already provided.\n\
- Review the changed code first.\n\
- If the request asks for a bounded global scan, inspect adjacent callers, exports, shared types, tests, configs, or runtime boundaries that are plausibly affected by the diff.\n\
- Keep that global scan bounded: at most one dependency hop and at most 8 additional files unless a smaller set is sufficient.\n\
- If the bounded global scan cannot be completed, record that in the coverage limitations instead of pretending the review is complete.\n\
\n\
Return format:\n\
- Return exactly one JSON object. Do not wrap it in markdown fences and do not add any prose before or after it.\n\
- Required top-level keys: `verdict`, `directFindings`, `globalFindings`, `verification`, `coverage`, `followUp`.\n\
- `verdict` must be one of `pass`, `fail`, or `needs_attention`.\n\
- Findings must stay concrete, actionable, and repository-specific.\n\
- Use `directFindings` for issues directly supported by the changed code or diff.\n\
- Use `globalFindings` for bounded downstream or cross-cutting risks discovered during the global impact probe.\n\
- `verification` must list every verification command you attempted, with command, status, summary, and key output when useful.\n\
- `coverage` must say whether diff review happened, whether the global scan happened, which paths were scanned, which were left unscanned, and what limitations remain.\n\
- `followUp` should be `[]` when nothing remains, otherwise list exact next steps for the parent agent or user.\n\
- Keep the JSON concise. The parent agent needs actionable signal, not exhaustive logs."
                    .to_string()
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
                    "git_status",
                    "Git Status",
                    "Inspect repository status in the current workspace without modifying anything. Use this to enumerate changed files before reading diffs.",
                    serde_json::json!({
                        "type": "object",
                        "properties": {
                            "path": { "type": "string", "description": "Optional relative path to narrow the status query." }
                        }
                    }),
                ),
                AgentTool::new(
                    "git_diff",
                    "Git Diff",
                    "Read the current Git diff in the workspace, optionally scoped to a path or staged changes. Prefer this over shelling out for diff inspection.",
                    serde_json::json!({
                        "type": "object",
                        "properties": {
                            "path": { "type": "string", "description": "Optional relative path to inspect." },
                            "staged": { "type": "boolean", "description": "Set true to inspect staged changes instead of working tree changes." },
                            "contextLines": {
                                "type": "integer",
                                "minimum": 1,
                                "maximum": 20,
                                "description": "Optional number of unified diff context lines. Defaults to 3 and is capped for safety."
                            }
                        }
                    }),
                ),
                AgentTool::new(
                    "git_log",
                    "Git Log",
                    "Inspect recent Git history in the current workspace without modifying anything. Useful for understanding prior behavior or nearby changes.",
                    serde_json::json!({
                        "type": "object",
                        "properties": {
                            "path": { "type": "string", "description": "Optional relative path to filter history." },
                            "limit": {
                                "type": "integer",
                                "minimum": 1,
                                "maximum": 100,
                                "description": "Optional maximum number of commits to return. Defaults to 10 and is capped for safety."
                            }
                        }
                    }),
                ),
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

        assert!(tool_names.contains(&"git_status"));
        assert!(tool_names.contains(&"git_diff"));
        assert!(tool_names.contains(&"git_log"));
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
        let review_scope_enum = tool.parameters["properties"]["reviewScope"]["enum"]
            .as_array()
            .expect("reviewScope enum should exist");

        assert!(tool.description.contains("type-check and test commands"));
        assert!(tool.description.contains("verification results"));
        assert!(task_description.contains("type-check or test commands"));
        assert_eq!(review_scope_enum.len(), 2);
    }

    #[test]
    fn review_git_tool_schema_exposes_numeric_safety_bounds() {
        let tools = SubagentProfile::Review.helper_tools();
        let git_diff = tools
            .iter()
            .find(|tool| tool.name == "git_diff")
            .expect("git_diff tool");
        let git_log = tools
            .iter()
            .find(|tool| tool.name == "git_log")
            .expect("git_log tool");

        assert_eq!(
            git_diff.parameters["properties"]["contextLines"]["minimum"],
            1
        );
        assert_eq!(
            git_diff.parameters["properties"]["contextLines"]["maximum"],
            20
        );
        assert_eq!(git_log.parameters["properties"]["limit"]["minimum"], 1);
        assert_eq!(git_log.parameters["properties"]["limit"]["maximum"], 100);
    }

    #[test]
    fn review_helper_prompt_requires_parent_follow_up_summary() {
        let prompt = SubagentProfile::Review.system_prompt();

        assert!(prompt.contains("This is mandatory, not optional"));
        assert!(prompt.contains("parent agent does not need to duplicate it by default"));
        assert!(prompt.contains("Return exactly one JSON object"));
        assert!(prompt.contains("globalFindings"));
        assert!(prompt.contains("followUp"));
    }
}
