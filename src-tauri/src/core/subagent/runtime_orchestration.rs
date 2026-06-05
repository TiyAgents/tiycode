use crate::core::agent_session_tools::web_search_agent_tool;
use crate::core::subagent::parallel_contract::{
    PARALLEL_SUBAGENT_DEFAULT_CONCURRENCY, PARALLEL_SUBAGENT_MAX_CONCURRENCY,
    PARALLEL_SUBAGENT_MAX_TASKS,
};
use crate::model::subagent::CustomSubagentModelRole;
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuntimeOrchestrationTool {
    Explore,
    Review,
    Parallel,
    Custom(String), // slug of the custom subagent
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SubagentProfile {
    Explore,
    Review,
    Custom {
        slug: String,
        system_prompt: String,
        allowed_tools: Vec<String>,
        model_role: CustomSubagentModelRole,
    },
}

pub fn runtime_orchestration_tools() -> Vec<AgentTool> {
    RuntimeOrchestrationTool::builtin_all()
        .into_iter()
        .map(RuntimeOrchestrationTool::as_agent_tool)
        .collect()
}

/// Convert a CustomSubagentRecord into an AgentTool for the main agent's tool set.
pub fn custom_subagent_as_tool(record: &crate::model::subagent::CustomSubagentRecord) -> AgentTool {
    let tool_name = format!("agent_{}", record.slug);
    AgentTool::new(
        &tool_name,
        &record.name,
        &record.invocation_description,
        serde_json::json!({
            "type": "object",
            "properties": {
                "task": {
                    "type": "string",
                    "description": "What task to delegate to this subagent. Be specific about goals, relevant files, and expected output format."
                }
            },
            "required": ["task"]
        }),
    )
}

impl RuntimeOrchestrationTool {
    pub fn builtin_all() -> [Self; 3] {
        [Self::Explore, Self::Review, Self::Parallel]
    }

    pub fn parse(tool_name: &str) -> Option<Self> {
        match tool_name {
            "agent_explore" => Some(Self::Explore),
            "agent_review" => Some(Self::Review),
            "agent_parallel" => Some(Self::Parallel),
            _ => {
                // Match custom subagent pattern: "agent_{slug}"
                if let Some(slug) = tool_name.strip_prefix("agent_") {
                    if !slug.is_empty() {
                        return Some(Self::Custom(slug.to_string()));
                    }
                }
                None
            }
        }
    }

    pub fn is_custom(&self) -> bool {
        matches!(self, Self::Custom(_))
    }

    pub fn tool_name(&self) -> String {
        match self {
            Self::Explore => "agent_explore".to_string(),
            Self::Review => "agent_review".to_string(),
            Self::Parallel => "agent_parallel".to_string(),
            Self::Custom(slug) => format!("agent_{slug}"),
        }
    }

    pub fn title(&self) -> String {
        match self {
            Self::Explore => "Agent Explore".to_string(),
            Self::Review => "Agent Review".to_string(),
            Self::Parallel => "Agent Parallel".to_string(),
            Self::Custom(slug) => format!("Agent {slug}"),
        }
    }

    pub fn description(&self) -> &str {
        match self {
            Self::Explore => {
                "Explore unfamiliar or cross-file areas before acting. Use this for current-state analysis, fact-finding, code reading, architecture summaries, and evidence gathering, then return a concise summary to the parent agent."
            }
            Self::Review => {
                "Review an implemented code change or diff, run the necessary type-check and test commands, and return risks, regressions, verification results, and concrete follow-ups. Use this after implementation to stress-test the work."
            }
            Self::Parallel => {
                "Delegate 1-5 independent subtasks to subagents with bounded concurrency. Use this for parallel exploration or review work only when tasks are independent and low side-effect; results are aggregated for the parent agent."
            }
            Self::Custom(_) => {
                // Custom subagents have their description set externally via custom_subagent_as_tool
                "Custom subagent."
            }
        }
    }

    /// Returns the built-in profile for this tool, or `None` for custom subagents
    /// and batch orchestration tools (which resolve delegates separately).
    pub fn profile(&self) -> Option<SubagentProfile> {
        match self {
            Self::Explore => Some(SubagentProfile::Explore),
            Self::Review => Some(SubagentProfile::Review),
            Self::Parallel | Self::Custom(_) => None,
        }
    }

    pub fn as_agent_tool(self) -> AgentTool {
        let parameters = match &self {
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
                    },
                    "planFilePath": {
                        "type": "string",
                        "description": "Optional absolute path to a plan file (e.g. ~/.tiy/plans/{threadId}.md). When provided, the review helper reads the plan and verifies each step was implemented."
                    }
                },
                "required": ["task"]
            }),
            Self::Parallel => serde_json::json!({
                "type": "object",
                "properties": {
                    "tasks": {
                        "type": "array",
                        "minItems": 1,
                        "maxItems": PARALLEL_SUBAGENT_MAX_TASKS,
                        "description": "Independent subagent tasks to run with bounded concurrency. Use only for separable, low side-effect exploration or review work.",
                        "items": {
                            "type": "object",
                            "properties": {
                                "agent": {
                                    "type": "string",
                                    "description": "Subagent tool name to invoke, such as agent_explore, agent_review, or agent_{customSlug}. Do not use agent_parallel recursively."
                                },
                                "task": {
                                    "type": "string",
                                    "description": "Specific delegated task for this subagent, including scope, relevant files, and expected output."
                                },
                                "target": {
                                    "type": "string",
                                    "enum": ["code", "diff"],
                                    "description": "Optional agent_review target for this item."
                                },
                                "reviewScope": {
                                    "type": "string",
                                    "enum": ["local", "diff_first_global"],
                                    "description": "Optional agent_review scope for this item."
                                },
                                "globalScanMode": {
                                    "type": "string",
                                    "enum": ["off", "auto"],
                                    "description": "Optional agent_review global scan mode for this item."
                                },
                                "changedFiles": {
                                    "type": "array",
                                    "items": { "type": "string" },
                                    "description": "Optional changed files for an agent_review item."
                                },
                                "preferredChecks": {
                                    "type": "array",
                                    "items": { "type": "string" },
                                    "description": "Optional verification commands for an agent_review item."
                                },
                                "riskHints": {
                                    "type": "array",
                                    "items": { "type": "string" },
                                    "description": "Optional risk hints for an agent_review item."
                                },
                                "planFilePath": {
                                    "type": "string",
                                    "description": "Optional plan file path for an agent_review item."
                                }
                            },
                            "required": ["agent", "task"]
                        }
                    },
                    "maxConcurrency": {
                        "type": "integer",
                        "minimum": 1,
                        "maximum": PARALLEL_SUBAGENT_MAX_CONCURRENCY,
                        "default": PARALLEL_SUBAGENT_DEFAULT_CONCURRENCY,
                        "description": "Maximum number of subagents to run at once. Defaults to 3 and is capped at 5."
                    },
                    "failFast": {
                        "type": "boolean",
                        "default": false,
                        "description": "If true, stop scheduling additional items after the first failure. Already-running helpers may still finish."
                    },
                    "mode": {
                        "type": "string",
                        "enum": ["explore", "review", "mixed"],
                        "description": "Optional high-level batch mode for result grouping."
                    },
                    "resultFormat": {
                        "type": "string",
                        "description": "Optional guidance for how the parent should compare or synthesize the returned summaries."
                    }
                },
                "required": ["tasks"]
            }),
            Self::Custom(_) => serde_json::json!({
                "type": "object",
                "properties": {
                    "task": {
                        "type": "string",
                        "description": "What task to delegate to this custom subagent."
                    }
                },
                "required": ["task"]
            }),
        };

        let name = self.tool_name();
        let title = self.title();
        let desc = self.description();
        AgentTool::new(&name, &title, desc, parameters)
    }
}

impl SubagentProfile {
    pub fn helper_kind(&self) -> String {
        match self {
            Self::Explore => "helper_explore".to_string(),
            Self::Review => "helper_review".to_string(),
            Self::Custom { slug, .. } => format!("helper_custom_{slug}"),
        }
    }

    /// Phase 7: Subagent body is now rendered by SubagentBodySource via the Composer.
    /// This method is retained only for backward-compat tests; production code
    /// should use `Composer::build` with the appropriate `PromptSurface`.
    /// See docs/prompt-injection-refactor.md § 4 阶段 7.
    pub fn system_prompt(&self) -> String {
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
            Self::Custom { system_prompt, .. } => system_prompt.clone(),
        }
    }

    pub fn helper_tools(&self, web_search_enabled: bool) -> Vec<AgentTool> {
        // For Custom profiles, dynamically resolve from allowed_tools
        if let Self::Custom { allowed_tools, .. } = self {
            return self.resolve_custom_tools(allowed_tools);
        }

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
                        "pattern": { "type": "string", "description": "Glob pattern to match files by basename, e.g. '*.ts', 'Cargo.toml'. Do NOT use '**/Cargo.toml' — find recurses automatically." },
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
                            "description": "Optional file type filter such as 'rust', 'ts', 'js', 'py', 'go', or 'json'. Applied as AND with filePattern — do not combine with a filePattern whose extension differs from the type. Use one or the other."
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

        if web_search_enabled {
            tools.push(web_search_agent_tool());
        }

        if *self == Self::Review {
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

    /// Build the tool list for a custom subagent based on its allowed_tools names.
    /// Only includes tools from the full candidate set (no agent_* tools allowed).
    fn resolve_custom_tools(&self, allowed_tools: &[String]) -> Vec<AgentTool> {
        let all_candidate_tools = Self::all_candidate_tools();
        all_candidate_tools
            .into_iter()
            .filter(|tool| allowed_tools.iter().any(|name| name == &tool.name))
            .collect()
    }

    /// Returns the complete set of tools that a custom subagent may choose from.
    /// Excludes agent_* orchestration tools to prevent recursive calls.
    fn all_candidate_tools() -> Vec<AgentTool> {
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
                "Search for files by glob pattern.",
                serde_json::json!({
                    "type": "object",
                    "properties": {
                        "pattern": { "type": "string" },
                        "path": { "type": "string" }
                    },
                    "required": ["pattern"]
                }),
            ),
            AgentTool::new(
                "search",
                "Search Repo",
                "Search the current workspace with a built-in search engine.",
                serde_json::json!({
                    "type": "object",
                    "properties": {
                        "query": { "type": "string" },
                        "directory": { "type": "string" },
                        "filePattern": { "type": "string" },
                        "maxResults": { "type": "integer" },
                        "queryMode": { "type": "string", "enum": ["literal", "regex"] },
                        "caseInsensitive": { "type": "boolean" }
                    },
                    "required": ["query"]
                }),
            ),
            web_search_agent_tool(),
            AgentTool::new(
                "edit",
                "Edit File",
                "Make a targeted edit to a file by specifying the exact text to find and its replacement.",
                serde_json::json!({
                    "type": "object",
                    "properties": {
                        "path": { "type": "string" },
                        "old_string": { "type": "string" },
                        "new_string": { "type": "string" }
                    },
                    "required": ["path", "old_string", "new_string"]
                }),
            ),
            AgentTool::new(
                "write",
                "Write File",
                "Write or overwrite a file inside the current workspace.",
                serde_json::json!({
                    "type": "object",
                    "properties": {
                        "path": { "type": "string" },
                        "content": { "type": "string" }
                    },
                    "required": ["path", "content"]
                }),
            ),
            AgentTool::new(
                "shell",
                "Run Command",
                "Run a non-interactive shell command inside the current workspace.",
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
            AgentTool::new(
                "git_status",
                "Git Status",
                "Inspect repository status.",
                serde_json::json!({
                    "type": "object",
                    "properties": {
                        "path": { "type": "string" }
                    }
                }),
            ),
            AgentTool::new(
                "git_diff",
                "Git Diff",
                "Read the current Git diff.",
                serde_json::json!({
                    "type": "object",
                    "properties": {
                        "path": { "type": "string" },
                        "staged": { "type": "boolean" },
                        "contextLines": { "type": "integer" }
                    }
                }),
            ),
            AgentTool::new(
                "git_log",
                "Git Log",
                "Inspect recent Git history.",
                serde_json::json!({
                    "type": "object",
                    "properties": {
                        "path": { "type": "string" },
                        "limit": { "type": "integer" }
                    }
                }),
            ),
        ];
        // Terminal tools
        tools.push(AgentTool::new(
            "term_status",
            "Terminal Status",
            TERM_STATUS_TOOL_DESCRIPTION,
            serde_json::json!({ "type": "object", "properties": {} }),
        ));
        tools.push(AgentTool::new(
            "term_output",
            "Terminal Output",
            TERM_OUTPUT_TOOL_DESCRIPTION,
            serde_json::json!({ "type": "object", "properties": {} }),
        ));
        tools.push(AgentTool::new(
            "term_write",
            "Terminal Write",
            TERM_WRITE_TOOL_DESCRIPTION,
            serde_json::json!({
                "type": "object",
                "properties": { "data": { "type": "string" } },
                "required": ["data"]
            }),
        ));
        tools.push(AgentTool::new(
            "term_restart",
            "Terminal Restart",
            TERM_RESTART_TOOL_DESCRIPTION,
            serde_json::json!({
                "type": "object",
                "properties": {
                    "cols": { "type": "integer" },
                    "rows": { "type": "integer" }
                }
            }),
        ));
        tools.push(AgentTool::new(
            "term_close",
            "Terminal Close",
            TERM_CLOSE_TOOL_DESCRIPTION,
            serde_json::json!({ "type": "object", "properties": {} }),
        ));
        tools
    }
}

#[cfg(test)]
mod tests {
    use super::{runtime_orchestration_tools, RuntimeOrchestrationTool, SubagentProfile};
    use crate::model::subagent::CustomSubagentModelRole;

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
        assert_eq!(
            RuntimeOrchestrationTool::parse("agent_parallel"),
            Some(RuntimeOrchestrationTool::Parallel)
        );
        assert_eq!(RuntimeOrchestrationTool::parse("read"), None);
    }

    #[test]
    fn reviewer_profile_includes_terminal_tools() {
        let tools = SubagentProfile::Review.helper_tools(false);
        let tool_names: Vec<&str> = tools.iter().map(|tool| tool.name.as_str()).collect();

        assert!(tool_names.contains(&"git_status"));
        assert!(tool_names.contains(&"git_diff"));
        assert!(tool_names.contains(&"git_log"));
        assert!(tool_names.contains(&"term_status"));
        assert!(tool_names.contains(&"term_output"));
        assert!(!tool_names.contains(&"web_search"));
    }

    #[test]
    fn reviewer_profile_includes_web_search_when_enabled() {
        let tools = SubagentProfile::Review.helper_tools(true);
        let tool_names: Vec<&str> = tools.iter().map(|tool| tool.name.as_str()).collect();

        assert!(tool_names.contains(&"web_search"));
        assert!(tool_names.contains(&"git_status"));
    }

    #[test]
    fn reviewer_terminal_tool_descriptions_clarify_terminal_panel_scope() {
        let tools = SubagentProfile::Review.helper_tools(false);
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

        assert_eq!(
            tool_names,
            vec!["agent_explore", "agent_review", "agent_parallel"]
        );
    }

    #[test]
    fn agent_parallel_tool_schema_has_bounded_tasks() {
        let tool = RuntimeOrchestrationTool::Parallel.as_agent_tool();
        assert_eq!(tool.name, "agent_parallel");
        assert_eq!(tool.parameters["required"], serde_json::json!(["tasks"]));
        assert_eq!(tool.parameters["properties"]["tasks"]["maxItems"], 5);
        assert_eq!(
            tool.parameters["properties"]["maxConcurrency"]["maximum"],
            5
        );
        assert_eq!(
            tool.parameters["properties"]["maxConcurrency"]["default"],
            3
        );
        assert!(tool.description.contains("bounded concurrency"));
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
        let tools = SubagentProfile::Review.helper_tools(false);
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

    #[test]
    fn parses_custom_subagent_tool_names() {
        assert_eq!(
            RuntimeOrchestrationTool::parse("agent_refactor"),
            Some(RuntimeOrchestrationTool::Custom("refactor".to_string()))
        );
        assert_eq!(
            RuntimeOrchestrationTool::parse("agent_code-review"),
            Some(RuntimeOrchestrationTool::Custom("code-review".to_string()))
        );
        // "agent_" alone (empty slug) should not match
        assert_eq!(RuntimeOrchestrationTool::parse("agent_"), None);
    }

    #[test]
    fn custom_profile_resolves_tools_from_allowlist() {
        let profile = SubagentProfile::Custom {
            slug: "test".to_string(),
            system_prompt: "You are a test helper.".to_string(),
            allowed_tools: vec!["read".to_string(), "search".to_string()],
            model_role: CustomSubagentModelRole::Auxiliary,
        };
        let tools = profile.helper_tools(false);
        let tool_names: Vec<&str> = tools.iter().map(|t| t.name.as_str()).collect();
        assert!(tool_names.contains(&"read"));
        assert!(tool_names.contains(&"search"));
        assert!(!tool_names.contains(&"web_search"));
        assert!(!tool_names.contains(&"edit"));
        assert!(!tool_names.contains(&"shell"));
    }

    #[test]
    fn custom_profile_can_allow_web_search_tool() {
        let profile = SubagentProfile::Custom {
            slug: "test".to_string(),
            system_prompt: "You are a test helper.".to_string(),
            allowed_tools: vec!["web_search".to_string()],
            model_role: CustomSubagentModelRole::Auxiliary,
        };
        let tools = profile.helper_tools(false);
        let tool_names: Vec<&str> = tools.iter().map(|t| t.name.as_str()).collect();
        assert!(tool_names.contains(&"web_search"));
        assert!(!tool_names.contains(&"read"));
    }
}
