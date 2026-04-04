use std::collections::HashMap;
use std::ffi::OsString;
use std::path::{Path, PathBuf};

use crate::core::agent_session::{
    normalize_profile_response_language, response_style_system_instruction,
    ProfileResponseStyle,
};
use crate::core::subagent::TERM_PANEL_USAGE_NOTE;
use crate::model::errors::AppError;
use crate::persistence::repo::{profile_repo, settings_repo};

use super::context::PromptBuildContext;
use super::section::{PromptPhase, PromptSection, PromptSectionProvider};

const WORKSPACE_INSTRUCTION_FILE_NAMES: &[&str] = &["AGENTS.md", "CLAUDE.md", "AGENT.MD"];
const WORKSPACE_INSTRUCTION_MAX_CHARS: usize = 12_800;
const SHELL_GUIDE_TOOL_NAMES: &[&str] = &["python3", "python", "node", "npm", "uv", "git", "rg"];

#[derive(Debug, Clone)]
struct WorkspaceInstructionSnippet {
    file_name: &'static str,
    content: String,
    truncated: bool,
}

#[derive(Debug, Clone)]
struct ToolAvailability {
    name: &'static str,
    path: Option<PathBuf>,
}

pub struct BaseProvider;
pub struct WorkspaceProvider;
pub struct EnvironmentProvider;
pub struct ProfileProvider;

impl PromptSectionProvider for BaseProvider {
    async fn collect(&self, _ctx: &PromptBuildContext<'_>) -> Result<Vec<PromptSection>, AppError> {
        Ok(vec![
            PromptSection {
                key: "role",
                title: "Role",
                body: "You are Tiy Agent, an expert working assistant embedded in the user's desktop workspace.\nYou help users by reading files, searching code, editing files, executing commands, and writing new files.".to_string(),
                phase: PromptPhase::Core,
                order_in_phase: 10,
            },
            PromptSection {
                key: "behavioral_guidelines",
                title: "Behavioral Guidelines",
                body: "Guidelines:\n- Before taking tool actions or making substantive changes, send a brief, friendly reply that acknowledges the request and states the next step you are about to take.\n- Read files before editing. Understand existing code before making changes.\n- Use `read` to inspect files instead of shell commands such as `cat`, `sed`, or `head` when the file tool fits.\n- Use `search` to find content and `find` to locate files before broader shell exploration when the workspace-aware tools fit.\n- Use edit for precise, surgical changes. Use write only for new files or complete rewrites.\n- Use `shell` for one-shot non-interactive commands, and rely on the terminal panel tools only for their dedicated session workflow.\n- Prefer search and find over shell for file exploration — they are faster and respect ignore patterns.\n- For search, omit wildcard-only filePattern values such as `*` or `**/*`; leaving filePattern unset already searches the full selected directory.\n- Delegate proactively on substantial work. When the task is cross-file, unfamiliar, risky, or likely to benefit from a second pass, use a helper instead of doing all exploration and review yourself.\n- Use agent_explore to investigate unfamiliar areas, collect evidence, map dependencies, explain the current state, or gather the right files before choosing an implementation.\n- For complex tasks, briefly confirm your understanding of the goal, scope, or constraints before publishing an implementation plan.\n- When the user's goal is clear and the next action is low-risk, local, and reversible, move forward without unnecessary clarification.\n- Use clarify instead of guessing when the user should choose between multiple reasonable approaches, confirm a preference, decide scope, approve a risky action, or fill in missing requirements before you continue. Ask one concise question at a time, offer 2-5 short options when helpful, and mark the recommended option.\n- Do not use clarify to offload work you can reasonably infer, investigate, or complete yourself with the available tools.\n- Use update_plan to publish the current implementation plan once the intended change is clear.\n- Use update_plan before implementation when the work is complex, cross-file, risky, or likely to benefit from explicit pre-implementation review.\n- Do not use update_plan for pure analysis, architecture explanation, current-state summaries, or information gathering with no concrete implementation to plan.\n- When a requirement, preference, or scope decision is still unresolved, clarify first and wait for the answer before publishing update_plan.\n- In default mode, if the task is complex or risky enough to benefit from explicit pre-implementation approval, publish a plan with update_plan before making changes.\n- Use agent_review after implementation with target='code' or target='diff' to check regressions, edge cases, and consistency. The review helper is responsible for running the necessary type-check and test commands and returning the verification results alongside the code review findings.\n- After agent_review completes, treat its verification output as the default source of truth for post-implementation type-check and test status. Do not rerun the same verification commands yourself unless the helper explicitly could not run them, reported inconclusive results, or the user asked you to double-check.\n- Report verification status honestly. Explicitly distinguish between commands you ran yourself, commands the review helper ran, commands that failed, and checks that were not run.\n- Do not collapse main-agent verification and review-helper verification into a single vague claim such as 'verified' or 'checked'.\n- Do not imply that tests, type-checks, builds, or manual verification passed if you did not run them or do not have a trustworthy result for them.\n- When verification is partial, list which checks were run, which checks failed, which checks were not run, and whether the user needs to run anything manually.\n- If a verification command fails, say so directly and summarize the failure instead of softening it into a successful outcome.\n- Recommended flow for non-trivial tasks: agent_explore -> confirm goal -> update_plan -> wait for approval -> implement -> agent_review(target='code' or 'diff').\n- Skip delegation only when the task is small, obvious, and isolated enough that extra helper work would not pay off.\n- Adapt answer length and prose density to the active response style: in concise mode, give the shortest correct answer; in balanced mode, write enough to be clear — a few paragraphs, not a wall of bullets; in guided mode, explain reasoning and tradeoffs in full. Show file paths clearly when working with files.\n- When summarizing your actions, describe what you did in plain text — do not re-read or re-cat files to prove your work.\n- Flag risks, destructive operations, or ambiguity before acting. Ask when intent is unclear.".to_string(),
                phase: PromptPhase::Core,
                order_in_phase: 20,
            },
            PromptSection {
                key: "final_response_structure",
                title: "Final Response Structure",
                body: final_response_structure_system_instruction().to_string(),
                phase: PromptPhase::Core,
                order_in_phase: 30,
            },
        ])
    }
}

impl PromptSectionProvider for WorkspaceProvider {
    async fn collect(&self, ctx: &PromptBuildContext<'_>) -> Result<Vec<PromptSection>, AppError> {
        let mut sections = Vec::new();

        if let Some(section) = build_project_context_section(ctx.workspace_path) {
            sections.push(PromptSection {
                key: "project_context",
                title: "Project Context (workspace instructions)",
                body: section,
                phase: PromptPhase::WorkspacePreference,
                order_in_phase: 10,
            });
        }

        Ok(sections)
    }
}

impl PromptSectionProvider for EnvironmentProvider {
    async fn collect(&self, ctx: &PromptBuildContext<'_>) -> Result<Vec<PromptSection>, AppError> {
        Ok(vec![
            PromptSection {
                key: "system_environment",
                title: "System Environment",
                body: build_system_environment_body(),
                phase: PromptPhase::RuntimeContext,
                order_in_phase: 10,
            },
            PromptSection {
                key: "sandbox_permissions",
                title: "Sandbox & Permissions",
                body: build_sandbox_permissions_body(ctx.pool, ctx.run_mode, ctx.workspace_path).await?,
                phase: PromptPhase::RuntimeContext,
                order_in_phase: 20,
            },
            PromptSection {
                key: "shell_tooling_guide",
                title: "Shell Tooling Guide",
                body: build_shell_tooling_guide_body(),
                phase: PromptPhase::Capability,
                order_in_phase: 10,
            },
        ])
    }
}

impl PromptSectionProvider for ProfileProvider {
    async fn collect(&self, ctx: &PromptBuildContext<'_>) -> Result<Vec<PromptSection>, AppError> {
        let mut sections = Vec::new();
        let mut profile_lines = Vec::new();
        if let Some(custom_instructions) = ctx.raw_plan.custom_instructions.as_deref() {
            let trimmed = custom_instructions.trim();
            if !trimmed.is_empty() {
                profile_lines.push(trimmed.to_string());
            }
        }
        let mut profile_response_parts = build_profile_response_prompt_parts_from_runtime(
            ctx.raw_plan.response_language.as_deref(),
            ctx.raw_plan.response_style.as_deref(),
        );
        let runtime_has_response_language =
            normalize_profile_response_language(ctx.raw_plan.response_language.as_deref()).is_some();
        let runtime_has_explicit_response_style = ctx
            .raw_plan
            .response_style
            .as_deref()
            .map(str::trim)
            .is_some_and(|value| !value.is_empty());

        if let Some(profile_id) = ctx.raw_plan.profile_id.as_deref() {
            if let Some(profile) = profile_repo::find_by_id(ctx.pool, profile_id).await? {
                if profile_lines.is_empty() {
                    if let Some(custom_instructions) = profile.custom_instructions.as_deref() {
                        let trimmed = custom_instructions.trim();
                        if !trimmed.is_empty() {
                            profile_lines.push(trimmed.to_string());
                        }
                    }
                }

                if !runtime_has_response_language {
                    if let Some(language) =
                        normalize_profile_response_language(profile.response_language.as_deref())
                    {
                        profile_response_parts.insert(
                            0,
                            format!(
                                "Respond in {language} unless the user explicitly asks for a different language."
                            ),
                        );
                    }
                }

                if !runtime_has_explicit_response_style {
                    profile_response_parts = build_profile_response_prompt_parts_from_runtime(
                        if runtime_has_response_language {
                            ctx.raw_plan.response_language.as_deref()
                        } else {
                            profile.response_language.as_deref()
                        },
                        profile.response_style.as_deref(),
                    );
                }
            }
        }

        profile_lines.extend(profile_response_parts);

        if !profile_lines.is_empty() {
            sections.push(PromptSection {
                key: "profile_instructions",
                title: "Profile Instructions",
                body: profile_lines.join("\n"),
                phase: PromptPhase::WorkspacePreference,
                order_in_phase: 20,
            });
        }

        sections.push(PromptSection {
            key: "run_mode",
            title: "Run Mode",
            body: run_mode_prompt_body(ctx.run_mode),
            phase: PromptPhase::RuntimeContext,
            order_in_phase: 30,
        });

        let date = chrono::Local::now().format("%Y-%m-%d").to_string();
        sections.push(PromptSection {
            key: "runtime_context",
            title: "Runtime Context",
            body: format!("Current date: {date}\nWorkspace path: {}", ctx.workspace_path),
            phase: PromptPhase::RuntimeContext,
            order_in_phase: 40,
        });

        Ok(sections)
    }
}

fn build_project_context_section(workspace_path: &str) -> Option<String> {
    let snippet = collect_workspace_instruction_snippet(workspace_path)?;
    let mut body =
        "Workspace instruction file found at the workspace root. Follow it when relevant."
            .to_string();
    body.push_str("\n\n");
    body.push_str(&format!("### {}\n", snippet.file_name));
    body.push_str("```md\n");
    body.push_str(&snippet.content);
    if snippet.truncated {
        body.push_str("\n[Truncated for prompt size.]");
    }
    body.push_str("\n```");

    Some(body)
}

async fn build_sandbox_permissions_body(
    pool: &sqlx::SqlitePool,
    run_mode: &str,
    workspace_path: &str,
) -> Result<String, AppError> {
    use crate::core::workspace_paths::parse_writable_roots;

    let approval_policy = settings_repo::policy_get(pool, "approval_policy")
        .await?
        .map(|record| parse_approval_policy_mode(&record.value_json))
        .unwrap_or_else(|| "require_for_mutations".to_string());

    let writable_roots: Vec<String> = settings_repo::policy_get(pool, "writable_roots")
        .await?
        .map(|record| parse_writable_roots(&record.value_json))
        .unwrap_or_default();

    let run_mode_line = if run_mode == "plan" {
        "Plan mode is active, so mutating tools are blocked."
    } else {
        "Default mode is active, so tool use follows the configured approval policy."
    };

    let mut lines = vec![
        "- Effective runtime sandbox: workspace-scoped tool execution with policy checks.".to_string(),
        format!("- Workspace boundary: file and path-aware tools are restricted to the current workspace (`{workspace_path}`)."),
        format!("- Approval policy: {approval_policy}."),
        "- Read-only tools are generally auto-allowed; mutating tools may require approval.".to_string(),
        format!("- {run_mode_line}"),
    ];

    if !writable_roots.is_empty() {
        let roots_display: Vec<String> = writable_roots
            .iter()
            .map(|root| format!("`{root}`"))
            .collect();
        lines.push(format!(
            "- Additional writable roots: {}. File tools (read, write, edit, list, find, search) can operate on files under these paths in addition to the workspace.",
            roots_display.join(", ")
        ));
    }

    lines.push("- Outer host sandbox metadata is not exposed here; rely on these effective runtime constraints.".to_string());

    Ok(lines.join("\n"))
}

fn parse_approval_policy_mode(value_json: &str) -> String {
    let parsed: serde_json::Value = serde_json::from_str(value_json).unwrap_or_default();

    if let Some(value) = parsed.as_str() {
        return value.to_string();
    }

    parsed
        .get("mode")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("require_for_mutations")
        .to_string()
}

fn collect_workspace_instruction_snippet(
    workspace_path: &str,
) -> Option<WorkspaceInstructionSnippet> {
    let workspace_root = Path::new(workspace_path);
    if !workspace_root.is_dir() {
        return None;
    }

    WORKSPACE_INSTRUCTION_FILE_NAMES
        .iter()
        .find_map(|file_name| {
            let path = workspace_root.join(file_name);
            if !path.is_file() {
                return None;
            }

            let raw = std::fs::read(&path).ok()?;
            let content = normalize_prompt_doc_content(&String::from_utf8_lossy(&raw));
            if content.is_empty() {
                return None;
            }

            let (content, truncated) = truncate_chars(&content, WORKSPACE_INSTRUCTION_MAX_CHARS);
            Some(WorkspaceInstructionSnippet {
                file_name,
                content,
                truncated,
            })
        })
}

fn normalize_prompt_doc_content(value: &str) -> String {
    value
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

fn truncate_chars(value: &str, max_chars: usize) -> (String, bool) {
    let char_count = value.chars().count();
    if char_count <= max_chars {
        return (value.to_string(), false);
    }

    let truncated = value.chars().take(max_chars).collect::<String>();
    (truncated.trim_end().to_string(), true)
}

fn build_system_environment_body() -> String {
    let shell = current_shell();
    let tool_lines = detect_shell_tools()
        .into_iter()
        .map(|tool| match tool.path {
            Some(path) => format!("- {}: available at {}", tool.name, path.display()),
            None => format!("- {}: not found on PATH", tool.name),
        })
        .collect::<Vec<_>>()
        .join("\n");

    format!(
        "- Operating system: {}\n- Architecture: {}\n- Default shell: {}\n- Common CLI tools:\n{}",
        std::env::consts::OS,
        std::env::consts::ARCH,
        shell,
        tool_lines
    )
}

fn build_shell_tooling_guide_body() -> String {
    let tool_lookup = detect_shell_tools()
        .into_iter()
        .map(|tool| (tool.name, tool.path.is_some()))
        .collect::<HashMap<_, _>>();

    let python_hint = if tool_lookup.get("python3").copied().unwrap_or(false) {
        "Prefer `python3` for Python commands in shell examples."
    } else if tool_lookup.get("python").copied().unwrap_or(false) {
        "Use `python` for Python commands in shell examples."
    } else {
        "Do not assume Python is available; verify before proposing Python shell commands."
    };

    let node_hint = if tool_lookup.get("node").copied().unwrap_or(false)
        || tool_lookup.get("npm").copied().unwrap_or(false)
    {
        "Node tooling is available. Prefer `npm` scripts when the workspace defines them."
    } else {
        "Do not assume Node tooling is available; verify before proposing Node shell commands."
    };

    let uv_hint = if tool_lookup.get("uv").copied().unwrap_or(false) {
        "Use `uv` for lightweight Python environment and script execution when that fits the task."
    } else {
        "Do not assume `uv` is available."
    };

    let rg_hint = if tool_lookup.get("rg").copied().unwrap_or(false) {
        "Prefer `rg` for text search and file discovery before broader shell commands."
    } else {
        "If `rg` is unavailable, fall back to the built-in search and find tools before broad shell scans."
    };

    let git_hint = if tool_lookup.get("git").copied().unwrap_or(false) {
        "Use `git` for repo status, diff, and history checks when repository context matters."
    } else {
        "Do not assume `git` is available in shell commands."
    };

    format!(
        "- Shell commands run through the user's default shell (`{}`).\n- This section is a shell command selection and boundary guide. Prefer workspace-aware tools (`read`, `list`, `search`, `find`, `edit`) before shell when they fit.\n- Use `shell` for one-shot non-interactive commands in the workspace.\n- Use `term_status`, `term_output`, `term_write`, `term_restart`, and `term_close` only for the desktop app's embedded Terminal panel session for the current thread. They inspect or control that persistent panel session and do not replace one-shot `shell` execution.\n- {}\n- {}\n- {}\n- {}\n- {}",
        current_shell(),
        rg_hint,
        python_hint,
        node_hint,
        uv_hint,
        git_hint
    )
}

fn current_shell() -> String {
    std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_string())
}

fn detect_shell_tools() -> Vec<ToolAvailability> {
    SHELL_GUIDE_TOOL_NAMES
        .iter()
        .map(|name| ToolAvailability {
            name,
            path: find_command_on_path(name),
        })
        .collect()
}

fn find_command_on_path(command: &str) -> Option<PathBuf> {
    let path_value = std::env::var_os("PATH")?;
    let candidates = executable_candidates(command);

    for directory in std::env::split_paths(&path_value) {
        for candidate in &candidates {
            let path = directory.join(candidate);
            if path.is_file() {
                return Some(path);
            }
        }
    }

    None
}

fn executable_candidates(command: &str) -> Vec<OsString> {
    #[cfg(target_os = "windows")]
    {
        if Path::new(command).extension().is_some() {
            return vec![OsString::from(command)];
        }

        let pathext =
            std::env::var_os("PATHEXT").unwrap_or_else(|| OsString::from(".COM;.EXE;.BAT;.CMD"));
        let mut candidates = vec![OsString::from(command)];

        for ext in pathext.to_string_lossy().split(';') {
            let trimmed = ext.trim();
            if trimmed.is_empty() {
                continue;
            }
            candidates.push(OsString::from(format!("{command}{trimmed}")));
        }

        candidates
    }

    #[cfg(not(target_os = "windows"))]
    {
        vec![OsString::from(command)]
    }
}

fn build_profile_response_prompt_parts_from_runtime(
    response_language: Option<&str>,
    response_style: Option<&str>,
) -> Vec<String> {
    let mut parts = Vec::new();

    if let Some(language) = normalize_profile_response_language(response_language) {
        parts.push(format!(
            "Respond in {language} unless the user explicitly asks for a different language."
        ));
    }

    parts.push(
        response_style_system_instruction(normalize_profile_response_style(response_style))
            .to_string(),
    );

    parts
}

fn normalize_profile_response_style(value: Option<&str>) -> ProfileResponseStyle {
    match value.unwrap_or("balanced").trim().to_lowercase().as_str() {
        "concise" => ProfileResponseStyle::Concise,
        "guide" | "guided" => ProfileResponseStyle::Guide,
        _ => ProfileResponseStyle::Balanced,
    }
}

pub(crate) fn final_response_structure_system_instruction() -> &'static str {
    "For conclusion-oriented replies, choose a structure that matches the task instead of forcing one template for every situation.\n- Keep the outer Markdown layout disciplined: use at most two heading levels in one reply, avoid turning every sub-point into its own heading, and prefer short sections with lists underneath over a long chain of peer headers.\n- When the reply is more than a very small update, prefer a clearly structured Markdown presentation instead of one dense block of prose.\n- Use short Markdown section headers for the main sections only. Put supporting detail inside numbered lists or flat bullet lists rather than promoting each detail to a new heading.\n- Use numbered lists for ordered reasons, changes, or options. Use flat bullet lists for evidence, verification items, or supporting facts.\n- Use emphasis or inline code sparingly to highlight the key conclusion, the recommended option, commands, file paths, settings, or identifiers that the user should notice quickly. Do not overload the reply with inline code formatting.\n- For simple tasks, you may compress the structure into a short paragraph or a short flat list, but keep a clear top-down order.\n- Use one of these default patterns:\n\n  - Debug or problem analysis: conclusion -> causes 1, 2, and 3 if relevant -> evidence tied to each cause -> recommendation options 1, 2, and 3 with a recommended option.\n\n  - Code change or result report: outcome -> key changes 1, 2, and 3 if relevant -> verification or evidence -> next steps, risks, or follow-up recommendation.\n\n  - Comparison or decision support: recommendation -> options 1, 2, and 3 -> tradeoffs and evidence -> clearly state the recommended option and why.\n\n  - Direct explanation or question answering: direct answer -> key points 1, 2, and 3 if relevant -> examples or evidence when helpful -> next step only if it adds value.\n- Do not force explicit headings on every reply unless the task benefits from a more structured presentation.\n- Write complete, grammatically whole sentences in every bullet point and paragraph. Avoid telegraph-style fragments (e.g. bare noun phrases like 'Plugin 执行协议已改为结构化'). Instead write full sentences that include subject, verb, and enough context to stand on their own.\n- When three or more closely related points share a single theme, merge them into one short paragraph with a topic sentence instead of listing each as a separate bullet.\n- If a single section exceeds roughly 8-10 lines of output, consider whether it should be split into two sections with distinct headers, or whether some detail can be folded into a summary sentence."
}

pub(crate) fn run_mode_prompt_body(run_mode: &str) -> String {
    match run_mode {
        "plan" => format!(
            "Plan mode is active.\n- Use only read-only tools plus clarify and update_plan: read, list, search, find, term_status, term_output, clarify, update_plan.\n- {TERM_PANEL_USAGE_NOTE}\n- Use agent_explore for read-only investigation and current-state analysis.\n- If key implementation details are still unclear, use clarify for one concise clarifying question before publishing a plan. Offer 2-3 short options when helpful.\n- After calling clarify, wait for the user's answer before continuing or calling update_plan.\n- Use update_plan only for the formal pre-implementation plan, not for general analysis or explanation.\n- For implementation-oriented requests, a prose answer alone does not complete the run.\n- Before the run can end, you must call update_plan to publish the implementation plan.\n- If you still need a requirement, preference, or decision, use clarify instead of finishing without a plan.\n- Do not include unresolved questions or TODO-style placeholders inside update_plan. Publish the plan only after the open decisions needed for implementation have been confirmed.\n- Once you publish a plan with update_plan, the run will pause for user approval before any implementation can begin.\n- Do NOT use edit, write, or shell unless the user explicitly requests execution.\n- Focus on analysis, explanation, and actionable planning. Identify risks, gaps, and concrete next steps."
        ),
        _ => format!(
            "Default execution mode is active.\n- Use the configured tool profile, subject to policy, approvals, and workspace boundaries.\n- {TERM_PANEL_USAGE_NOTE}\n- Use clarify instead of guessing when the user should choose between multiple reasonable approaches, confirm a preference, decide scope, approve a risky action, or fill in missing requirements before you continue.\n- When the next step is clear and low-risk, move the task forward without unnecessary clarification.\n- If implementation should pause for review first because the work is complex, cross-file, or risky, publish an implementation plan with update_plan before making changes.\n- If an unresolved requirement, preference, or scope decision blocks the implementation plan, use clarify first and wait for the answer before calling update_plan.\n- Prefer the smallest sufficient action that moves the task forward."
        ),
    }
}
