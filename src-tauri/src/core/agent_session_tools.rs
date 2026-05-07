use tiycore::agent::{AgentTool, AgentToolResult};
use tiycore::types::{
    AssistantMessage, ContentBlock, Model, Provider, StopReason, TextContent, Usage,
};

use crate::core::agent_session_types::{
    ResolvedModelRole, ResolvedRuntimeModelPlan, RuntimeModelPlan,
};
use crate::core::subagent::{
    runtime_orchestration_tools, RuntimeOrchestrationTool, SubagentProfile,
    TERM_CLOSE_TOOL_DESCRIPTION, TERM_OUTPUT_TOOL_DESCRIPTION, TERM_RESTART_TOOL_DESCRIPTION,
    TERM_STATUS_TOOL_DESCRIPTION, TERM_WRITE_TOOL_DESCRIPTION,
};

use super::agent_session::{
    CLARIFY_TOOL_NAME, DEFAULT_FULL_TOOL_PROFILE, PLAN_MODE_MISSING_CHECKPOINT_ERROR,
    PLAN_READ_ONLY_TOOL_PROFILE,
};

pub(crate) fn runtime_tools_for_profile(profile_name: &str) -> Vec<AgentTool> {
    let mut tools = vec![
        AgentTool::new(
            "read",
            "Read File",
            "Read a file inside the current workspace. Supports optional offset/limit windowing for large files and returns a truncated preview when the selected range exceeds safety limits.",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string" },
                    "offset": {
                        "type": "integer",
                        "description": "Optional 1-indexed line number to start reading from."
                    },
                    "limit": {
                        "type": "integer",
                        "description": "Optional maximum number of lines to read from the offset."
                    }
                },
                "required": ["path"]
            }),
        ),
        AgentTool::new(
            "list",
            "List Directory",
            "List files and folders inside the current workspace. Supports an optional preview limit for large directories.",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string" },
                    "limit": {
                        "type": "integer",
                        "description": "Optional maximum number of entries to return. Defaults to 500 and is capped for safety."
                    }
                }
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
                        "description": "Optional file type filter such as 'rust', 'ts', 'js', 'py', 'go', or 'json'. Applied as AND with filePattern — do not combine with a filePattern whose extension differs from the type (e.g. type='rust' + filePattern='*.toml' will match nothing). Use one or the other."
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
        AgentTool::new(
            "find",
            "Find Files",
            "Search for files by glob pattern. Returns matching file paths relative to the workspace. Respects common ignore patterns (.git, node_modules, target). Supports an optional preview limit and truncates output to 1000 results or 100KB.",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "pattern": {
                        "type": "string",
                        "description": "Glob pattern to match files by basename, e.g. '*.ts', 'Cargo.toml', '*.spec.ts'. Do NOT use path prefixes like '**/Cargo.toml' — find recurses automatically. For path-scoped searches, set the 'path' parameter instead."
                    },
                    "path": {
                        "type": "string",
                        "description": "Directory to search in (default: workspace root)"
                    },
                    "limit": {
                        "type": "integer",
                        "description": "Optional maximum number of matches to preview. Defaults to 1000 and is capped for safety."
                    }
                },
                "required": ["pattern"]
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
            CLARIFY_TOOL_NAME,
            "Clarify",
            "Ask the user one concise question when they need to choose between reasonable options, confirm a preference, approve a risky action, define scope, or provide missing requirements before you continue. Prefer this tool over guessing when multiple valid paths exist. Offer 2-5 short options when possible, mark the recommended option, and keep the wording brief.",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "header": {
                        "type": "string",
                        "description": "Optional short label for the UI, ideally 12 characters or fewer."
                    },
                    "question": {
                        "type": "string",
                        "description": "A single concise question for the user."
                    },
                    "options": {
                        "type": "array",
                        "minItems": 2,
                        "maxItems": 5,
                        "items": {
                            "type": "object",
                            "properties": {
                                "id": { "type": "string" },
                                "label": { "type": "string" },
                                "description": { "type": "string" },
                                "recommended": { "type": "boolean" }
                            },
                            "required": ["label", "description"]
                        }
                    }
                },
                "required": ["question", "options"]
            }),
        ),
        AgentTool::new(
            "update_plan",
            "Update Plan",
            "Publish the current implementation plan and pause for user approval before execution. The plan is saved to disk and persists across runs.\n\n\
## Workflow — complete these phases before calling this tool\n\n\
Phase 1 — Explore and understand:\n\
- Use read, search, find, list, and agent_explore to inspect relevant files, modules, and patterns.\n\
- Identify existing conventions, reusable modules, constraints, and dependencies.\n\
- Do NOT call update_plan until you have grounded your understanding in actual code evidence.\n\n\
Phase 2 — Clarify ambiguities:\n\
- If implementation-blocking uncertainty remains that code exploration cannot resolve, use clarify to ask the user.\n\
- Only ask questions the user must decide: scope, preference between valid approaches, priority tradeoffs.\n\
- Do NOT ask questions that code exploration can answer. Batch related questions. Wait for the answer before continuing.\n\
- Skip this phase if exploration resolved all uncertainties.\n\n\
Phase 3 — Converge on a recommendation:\n\
- Synthesize exploration evidence and clarification answers into ONE recommended approach.\n\
- Do not present multiple unranked alternatives. Every design decision must be grounded in inspected code or user input.\n\n\
Phase 4 — Call update_plan:\n\
- Only after phases 1-3 are complete, call this tool with a plan that satisfies the quality contract below.\n\n\
## Quality contract — every plan must satisfy\n\n\
- summary: what is being changed, why, and expected outcome (2-3 sentences).\n\
- context: write a thorough narrative of confirmed facts from inspected code, docs, or user input. Connect the facts into coherent paragraphs that explain the current state, how the relevant pieces fit together, and what constraints exist. Include file paths, type signatures, data flow direction, and version or compatibility details. The goal is a self-contained briefing a developer unfamiliar with the area can read and fully understand. Never speculate about uninspected files or architecture.\n\
- design: write a detailed prose description of the recommended approach. Explain the architecture or structural changes, walk through the data flow step by step, and articulate why this approach is chosen over alternatives by comparing tradeoffs explicitly. Cover edge cases the design handles and those it defers. The reader should finish this section understanding both the what and the why at a level sufficient to implement without further design questions.\n\
- keyImplementation: write a connected prose description of the specific files, modules, interfaces, data flows, or state transitions that carry the change. For each major component, explain what it does today, what changes, and how the changed pieces interact. Include type names, function signatures, and module boundaries. Vague references like 'update the relevant files' are not acceptable.\n\
- steps: concrete, ordered, actionable steps with affected files and intended outcomes.\n\
- verification: write a thorough description of how to validate the change succeeded. Cover type-checks, unit tests, integration tests, manual smoke tests, and behavioral verification. Mention specific commands, expected outputs, and edge cases worth verifying. Explain what each check proves and why it matters.\n\
- risks: main risks, edge cases, compatibility concerns, regression areas.\n\
- assumptions (optional): only non-blocking assumptions, not open questions.\n\n\
Prohibited: unresolved core ambiguities (use clarify first), TODO placeholders, vague steps, architecture guesses not backed by exploration, lengthy background essays without actionable information.\n\n\
You may call this tool multiple times in a run to incrementally refine the plan. Each call overwrites the previous version.",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "title": { "type": "string" },
                    "summary": { "type": "string" },
                    "context": { "type": "string" },
                    "design": { "type": "string" },
                    "keyImplementation": { "type": "string" },
                    "steps": {
                        "type": "array",
                        "items": {
                            "oneOf": [
                                { "type": "string" },
                                {
                                    "type": "object",
                                    "properties": {
                                        "id": { "type": "string" },
                                        "title": { "type": "string" },
                                        "description": { "type": "string" },
                                        "status": { "type": "string" },
                                        "files": {
                                            "type": "array",
                                            "items": { "type": "string" }
                                        }
                                    }
                                }
                            ]
                        }
                    },
                    "verification": { "type": "string" },
                    "risks": {
                        "type": "array",
                        "items": { "type": "string" }
                    },
                    "assumptions": {
                        "type": "array",
                        "items": { "type": "string" }
                    },
                    "plan": {
                        "type": "object",
                        "description": "Optional nested plan payload. If provided, the runtime reads planning fields from this object.",
                        "properties": {
                            "title": { "type": "string" },
                            "summary": { "type": "string" },
                            "context": { "type": "string" },
                            "design": { "type": "string" },
                            "keyImplementation": { "type": "string" },
                            "steps": {
                                "type": "array",
                                "items": {
                                    "oneOf": [
                                        { "type": "string" },
                                        {
                                            "type": "object",
                                            "properties": {
                                                "id": { "type": "string" },
                                                "title": { "type": "string" },
                                                "description": { "type": "string" },
                                                "status": { "type": "string" },
                                                "files": {
                                                    "type": "array",
                                                    "items": { "type": "string" }
                                                }
                                            }
                                        }
                                    ]
                                }
                            },
                            "verification": { "type": "string" },
                            "risks": {
                                "type": "array",
                                "items": { "type": "string" }
                            },
                            "assumptions": {
                                "type": "array",
                                "items": { "type": "string" }
                            }
                        }
                    }
                }
            }),
        ),
    ];
    tools.extend(runtime_orchestration_tools());

    // Shell is available in both profiles (plan mode applies read-only constraints via prompt).
    tools.push(AgentTool::new(
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
    ));

    if profile_name == DEFAULT_FULL_TOOL_PROFILE {
        tools.push(AgentTool::new(
            "edit",
            "Edit File",
            "Make a targeted edit to a file by specifying the exact text to find and its replacement. \
             The old_string must uniquely identify the text to replace (appear exactly once in the file). \
             Include enough surrounding context in old_string to make it unique. \
             If old_string is empty, a new file will be created with new_string as content. \
             Supports fuzzy matching for trailing whitespace and Unicode quote/dash differences.",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "Absolute path to the file to edit"
                    },
                    "old_string": {
                        "type": "string",
                        "description": "The exact text to find and replace. Must match exactly once in the file. Use empty string to create a new file."
                    },
                    "new_string": {
                        "type": "string",
                        "description": "The replacement text"
                    }
                },
                "required": ["path", "old_string", "new_string"]
            }),
        ));
        tools.push(AgentTool::new(
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
        ));
        tools.push(AgentTool::new(
            "term_write",
            "Terminal Write",
            TERM_WRITE_TOOL_DESCRIPTION,
            serde_json::json!({
                "type": "object",
                "properties": {
                    "data": {
                        "type": "string",
                        "description": "Input to send to the current thread's Terminal panel session."
                    }
                },
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
                    "cols": {
                        "type": "integer",
                        "description": "Optional terminal width in columns for the restarted Terminal panel session."
                    },
                    "rows": {
                        "type": "integer",
                        "description": "Optional terminal height in rows for the restarted Terminal panel session."
                    }
                }
            }),
        ));
        tools.push(AgentTool::new(
            "term_close",
            "Terminal Close",
            TERM_CLOSE_TOOL_DESCRIPTION,
            serde_json::json!({
                "type": "object",
                "properties": {}
            }),
        ));
    }

    // Task tracking tools (always available)
    tools.push(AgentTool::new(
        "create_task",
        "Create Task",
        "Create a new task board with steps to track implementation progress. Use this when starting a complex multi-step implementation. After creating a board, keep it current while you work instead of waiting until the very end to update statuses.",
        serde_json::json!({
            "type": "object",
            "properties": {
                "title": {
                    "type": "string",
                    "description": "Human-readable title for the task board."
                },
                "steps": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "properties": {
                            "description": { "type": "string" }
                        },
                        "required": ["description"]
                    },
                    "description": "Ordered list of task steps."
                }
            },
            "required": ["title", "steps"]
        }),
    ));
    tools.push(AgentTool::new(
        "update_task",
        "Update Task",
        "Update a task board or its steps. Call this after completing each implementation step to keep the board in sync with actual progress. The easiest pattern: call with action='advance_step' and no stepId — this completes the current active step and automatically starts the next one (or completes the board if no steps remain). If the app was interrupted or you are unsure which taskBoardId is current, call query_task first. Call after every step, not just at the end.",
        serde_json::json!({
            "type": "object",
            "properties": {
                "taskBoardId": {
                    "type": "string",
                    "description": "ID of the task board to update. If unknown after a restart or interruption, call query_task first."
                },
                "action": {
                    "type": "string",
                    "enum": ["start_step", "advance_step", "complete_step", "fail_step", "complete_board", "abandon_board"],
                    "description": "The action to perform. Use `advance_step` after finishing each step — it completes the current active step and auto-starts the next (or auto-completes the board). No stepId needed. Use `fail_step` if a step cannot be completed. Use `start_step` only to manually start a specific pending step."
                },
                "stepId": {
                    "type": "string",
                    "description": "Step ID. Required for start_step, complete_step, fail_step. Omit for advance_step to automatically target the current active step (recommended)."
                },
                "errorDetail": {
                    "type": "string",
                    "description": "Error description (required for fail_step)."
                },
                "reason": {
                    "type": "string",
                    "description": "Optional reason for abandoning the board."
                }
            },
            "required": ["taskBoardId", "action"]
        }),
    ));
    tools.push(AgentTool::new(
        "query_task",
        "Query Task",
        "Read the current thread's task-board state. Use this when resuming work after an interruption, restart, or any time you need to recover the current taskBoardId before calling update_task.",
        serde_json::json!({
            "type": "object",
            "properties": {
                "scope": {
                    "type": "string",
                    "enum": ["active", "all"],
                    "description": "Which task boards to return. Defaults to `active`. Use `all` only when you need the full thread task-board history."
                }
            }
        }),
    ));

    // Render artifact tool (always available) — supports charts, HTML, and SVG
    tools.push(AgentTool::new(
        "render",
        "Render",
        "Render a visual artifact into the current thread message. Supports Vega-Lite charts, HTML pages, and SVG graphics. For charts, provide a valid Vega-Lite JSON spec. For HTML or SVG, provide the source code as a string. The artifact is displayed inline in the conversation. Multiple renders can be produced in sequence within the same message.",
        serde_json::json!({
            "type": "object",
            "properties": {
                "title": {
                    "type": "string",
                    "description": "Short title displayed above the rendered artifact."
                },
                "caption": {
                    "type": "string",
                    "description": "Optional explanatory caption displayed below the title."
                },
                "library": {
                    "type": "string",
                    "enum": ["vega-lite", "html", "svg"],
                    "description": "Render type. Use 'vega-lite' for data visualizations (requires 'spec'), 'html' for HTML pages, or 'svg' for SVG graphics (both require 'source')."
                },
                "spec": {
                    "type": "object",
                    "description": "A complete Vega-Lite specification object. Required when library is 'vega-lite'. Must include at minimum '$schema', 'data', and 'mark' or 'layer'."
                },
                "source": {
                    "type": "string",
                    "description": "HTML or SVG source code string. Required when library is 'html' or 'svg'."
                }
            },
            "required": ["library"]
        }),
    ));

    tools
}

pub(crate) fn runtime_tools_for_profile_with_extensions(
    profile_name: &str,
    extension_tools: Vec<AgentTool>,
) -> Vec<AgentTool> {
    let mut tools = runtime_tools_for_profile(profile_name);
    let mut names = tools
        .iter()
        .map(|tool| tool.name.clone())
        .collect::<std::collections::HashSet<_>>();

    for tool in extension_tools {
        if names.insert(tool.name.clone()) {
            tools.push(tool);
        }
    }

    tools
}

pub(crate) fn resolve_tool_profile_name(raw_plan: &RuntimeModelPlan, run_mode: &str) -> String {
    if let Some(profile_name) = raw_plan
        .tool_profile_by_mode
        .as_ref()
        .and_then(|value| value.get(run_mode))
        .and_then(serde_json::Value::as_str)
        .filter(|value| !value.trim().is_empty())
    {
        return profile_name.to_string();
    }

    match run_mode {
        "plan" => PLAN_READ_ONLY_TOOL_PROFILE.to_string(),
        _ => DEFAULT_FULL_TOOL_PROFILE.to_string(),
    }
}

pub(crate) fn resolve_helper_profile(tool: RuntimeOrchestrationTool) -> SubagentProfile {
    match tool {
        RuntimeOrchestrationTool::Explore => SubagentProfile::Explore,
        RuntimeOrchestrationTool::Review => SubagentProfile::Review,
    }
}

pub(crate) fn resolve_helper_model_role(
    model_plan: &ResolvedRuntimeModelPlan,
    tool: RuntimeOrchestrationTool,
) -> ResolvedModelRole {
    match tool {
        RuntimeOrchestrationTool::Explore | RuntimeOrchestrationTool::Review => model_plan
            .auxiliary
            .clone()
            .unwrap_or_else(|| model_plan.primary.clone()),
    }
}

/// Maximum characters for a tool result output when replayed from history.
///
/// This deliberately oversizes vs. the aggressive (800) and recent (3200)
/// thresholds in `context_compression` so that history loaded for summary
/// generation retains enough raw material for the LLM to produce a faithful
/// summary. `render_compact_summary_history` applies its own holistic budget
/// downstream, so keeping more here improves summary quality without
/// inflating the final context sent to the primary model.
pub(crate) const HISTORY_TOOL_RESULT_MAX_CHARS: usize = 20_480;

pub(crate) fn assistant_message_with_blocks(
    blocks: Vec<ContentBlock>,
    model: &Model,
) -> AssistantMessage {
    AssistantMessage::builder()
        .content(blocks)
        .api(effective_api_for_model(model))
        .provider(model.provider.clone())
        .model(model.id.clone())
        .usage(Usage::default())
        .stop_reason(StopReason::Stop)
        .build()
        .expect("assistant history message should always build")
}

pub(crate) fn effective_api_for_model(model: &Model) -> tiycore::types::Api {
    if let Some(api) = model.api.clone() {
        return api;
    }

    match &model.provider {
        Provider::OpenAI | Provider::OpenAIResponses | Provider::AzureOpenAIResponses => {
            tiycore::types::Api::OpenAIResponses
        }
        Provider::Anthropic | Provider::MiniMax | Provider::MiniMaxCN | Provider::KimiCoding => {
            tiycore::types::Api::AnthropicMessages
        }
        Provider::Google | Provider::GoogleGeminiCli | Provider::GoogleAntigravity => {
            tiycore::types::Api::GoogleGenerativeAi
        }
        Provider::GoogleVertex => tiycore::types::Api::GoogleVertex,
        Provider::Ollama => tiycore::types::Api::Ollama,
        Provider::XAI
        | Provider::Groq
        | Provider::OpenRouter
        | Provider::OpenAICompatible
        | Provider::OpenAICodex
        | Provider::GitHubCopilot
        | Provider::Cerebras
        | Provider::VercelAiGateway
        | Provider::ZAI
        | Provider::Mistral
        | Provider::HuggingFace
        | Provider::OpenCode
        | Provider::OpenCodeGo
        | Provider::DeepSeek
        | Provider::Zenmux
        | Provider::Bai => tiycore::types::Api::OpenAICompletions,
        Provider::AmazonBedrock => tiycore::types::Api::BedrockConverseStream,
        Provider::Custom(name) => tiycore::types::Api::Custom(name.clone()),
    }
}

/// Maximum size for a single tool result sent to the LLM (8 MB).
/// OpenAI Responses API enforces a 10 MB limit per `input[n].output` field;
/// this leaves headroom for protocol overhead and JSON escaping.
const MAX_TOOL_RESULT_SIZE: usize = 8_000_000;

pub(crate) fn agent_tool_result_from_output(
    output: crate::core::executors::ToolOutput,
) -> AgentToolResult {
    // Use compact JSON (no pretty-print) to reduce whitespace overhead.
    let mut rendered =
        serde_json::to_string(&output.result).unwrap_or_else(|_| output.result.to_string());

    // Hard safety cap — truncate if the serialized result is still too large.
    if rendered.len() > MAX_TOOL_RESULT_SIZE {
        rendered.truncate(MAX_TOOL_RESULT_SIZE);
        // Ensure we don't cut in the middle of a multi-byte UTF-8 char
        while !rendered.is_char_boundary(rendered.len()) {
            rendered.pop();
        }
        rendered.push_str("\n\n[Tool output truncated: exceeded 8MB limit]");
    }

    if output.success {
        AgentToolResult {
            content: vec![ContentBlock::Text(TextContent::new(rendered))],
            details: Some(output.result),
        }
    } else {
        AgentToolResult {
            content: vec![ContentBlock::Text(TextContent::new(format!(
                "Error: {rendered}"
            )))],
            details: Some(output.result),
        }
    }
}

pub(crate) fn agent_error_result(message: impl Into<String>) -> AgentToolResult {
    AgentToolResult {
        content: vec![ContentBlock::Text(TextContent::new(format!(
            "Error: {}",
            message.into()
        )))],
        details: None,
    }
}

pub(crate) fn validate_clarify_input(value: &serde_json::Value) -> Result<(), String> {
    let Some(question) = value.get("question").and_then(serde_json::Value::as_str) else {
        return Err("clarify requires a non-empty question".to_string());
    };

    if question.trim().is_empty() {
        return Err("clarify requires a non-empty question".to_string());
    }

    let Some(options) = value.get("options").and_then(serde_json::Value::as_array) else {
        return Err("clarify requires 2 to 5 options".to_string());
    };

    if !(2..=5).contains(&options.len()) {
        return Err("clarify requires 2 to 5 options".to_string());
    }

    let recommended_count = options
        .iter()
        .filter(|option| {
            option
                .get("recommended")
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(false)
        })
        .count();

    if recommended_count > 1 {
        return Err("clarify may mark at most one option as recommended".to_string());
    }

    for option in options {
        let label = option
            .get("label")
            .and_then(serde_json::Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty());
        let description = option
            .get("description")
            .and_then(serde_json::Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty());

        if label.is_none() || description.is_none() {
            return Err("clarify options must include non-empty label and description".to_string());
        }
    }

    Ok(())
}

pub(crate) fn plan_mode_missing_checkpoint_error(
    run_mode: &str,
    checkpoint_requested: bool,
) -> Option<&'static str> {
    if run_mode == "plan" && !checkpoint_requested {
        Some(PLAN_MODE_MISSING_CHECKPOINT_ERROR)
    } else {
        None
    }
}
