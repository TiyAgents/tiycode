use std::path::Path;

use crate::core::agent_session::{
    normalize_profile_response_language, response_style_system_instruction, ProfileResponseStyle,
};
use crate::core::shell_runtime::current_shell;
use crate::core::subagent::TERM_PANEL_USAGE_NOTE;
use crate::extensions::{ConfigScope, ExtensionsManager};
use crate::model::errors::AppError;
use crate::persistence::repo::{profile_repo, settings_repo};

use super::context::PromptBuildContext;
use super::section::{PromptPhase, PromptSection, PromptSectionProvider};

const WORKSPACE_INSTRUCTION_FILE_NAMES: &[&str] = &["AGENTS.md", "CLAUDE.md", "AGENT.MD"];
const WORKSPACE_INSTRUCTION_MAX_CHARS: usize = 12_800;

#[derive(Debug, Clone)]
struct WorkspaceInstructionSnippet {
    file_name: &'static str,
    content: String,
    truncated: bool,
}

pub struct BaseProvider;
pub struct WorkspaceProvider;
pub struct EnvironmentProvider;
pub struct SkillsProvider;
pub struct ProfileProvider;

impl PromptSectionProvider for BaseProvider {
    async fn collect(&self, _ctx: &PromptBuildContext<'_>) -> Result<Vec<PromptSection>, AppError> {
        Ok(vec![
            PromptSection {
                key: "role",
                title: "Role",
                body: "You are TiyCode, an AI-first desktop coding agent embedded in the user's workspace.\nYou help users by understanding goals expressed through conversation, then reading files, searching code, editing files, executing commands, and writing new files to move the work forward.".to_string(),
                phase: PromptPhase::Core,
                order_in_phase: 10,
            },
            PromptSection {
                key: "behavioral_guidelines",
                title: "Behavioral Guidelines",
                body: "Guidelines:\n- Before taking tool actions or making substantive changes, send a brief, friendly reply that acknowledges the request and states the next step you are about to take.\n- Read files before editing. Understand existing code before making changes.\n- Use `read` to inspect files instead of shell commands such as `cat`, `sed`, or `head` when the file tool fits.\n- Use `search` to find content and `find` to locate files before broader shell exploration when the workspace-aware tools fit.\n- Use edit for precise, surgical changes. Use write only for new files or complete rewrites.\n- Use `shell` for one-shot non-interactive commands, and rely on the terminal panel tools only for their dedicated session workflow.\n- Prefer search and find over shell for file exploration — they are faster and respect ignore patterns.\n- For search, omit wildcard-only filePattern values such as `*` or `**/*`; leaving filePattern unset already searches the full selected directory.\n- Delegate proactively on substantial work. When the task is cross-file, unfamiliar, risky, or likely to benefit from a second pass, use a helper instead of doing all exploration and review yourself.\n- Use agent_explore to investigate unfamiliar areas, collect evidence, map dependencies, explain the current state, or gather the right files before choosing an implementation.\n- For complex tasks, briefly confirm your understanding of the goal, scope, or constraints before publishing an implementation plan.\n- When the user's goal is clear and the next action is low-risk, local, and reversible, move forward without unnecessary clarification.\n- Use clarify instead of guessing when the user should choose between multiple reasonable approaches, confirm a preference, decide scope, approve a risky action, or fill in missing requirements before you continue. Ask one concise question at a time, offer 2-5 short options when helpful, and mark the recommended option.\n- Do not use clarify to offload work you can reasonably infer, investigate, or complete yourself with the available tools.\n- Use update_plan to publish the current implementation plan once the intended change is clear.\n- Use update_plan before implementation when the work is complex, cross-file, risky, or likely to benefit from explicit pre-implementation review.\n- Do not use update_plan for pure analysis, architecture explanation, current-state summaries, or information gathering with no concrete implementation to plan.\n- When a requirement, preference, or scope decision is still unresolved, clarify first and wait for the answer before publishing update_plan.\n- In default mode, if the task is complex or risky enough to benefit from explicit pre-implementation approval, publish a plan with update_plan before making changes.\n- When calling update_plan, follow the quality contract in the tool description: explore first, then provide all required sections (summary, context, design, keyImplementation, steps, verification, risks). Do not publish plans with unresolved ambiguities or vague steps.\n- When you create a task board, treat it as a live execution tracker. After completing each implementation step, you MUST call `update_task` with `advance_step` to mark the step done and start the next one. Do not batch multiple step completions at the end.\n- Call `advance_step` (without a `stepId`) immediately after finishing the work described by the current active step. This is the simplest and most reliable way to keep the board current.\n- If you need to continue an existing task board but do not know the current `taskBoardId`, call `query_task` first.\n- After an interruption, restart, or resumed thread where task context may be incomplete, call `query_task` with `scope='active'` before attempting `update_task`.\n- Use `query_task` with `scope='all'` only when you need task-board history, or when the active board is missing and you need to decide whether to continue or create a new board.\n- If a step fails, call `update_task` with `fail_step` immediately, providing a clear `errorDetail`.\n- Before your final response in a run, verify the task board reflects reality: every finished step should be marked completed or failed, and the active step should match what you are currently working on.\n- Use agent_review after implementation with target='code' or target='diff' to check regressions, edge cases, and consistency. The review helper is responsible for running the necessary type-check and test commands and returning the verification results alongside the code review findings.\n- When a plan was published with update_plan, pass the plan file path to agent_review via the planFilePath parameter so the review helper can verify each plan step was implemented.\n- After agent_review completes, treat its verification output as the default source of truth for post-implementation type-check and test status. Do not rerun the same verification commands yourself unless the helper explicitly could not run them, reported inconclusive results, or the user asked you to double-check.\n- Report verification status honestly. Explicitly distinguish between commands you ran yourself, commands the review helper ran, commands that failed, and checks that were not run.\n- Do not collapse main-agent verification and review-helper verification into a single vague claim such as 'verified' or 'checked'.\n- Do not imply that tests, type-checks, builds, or manual verification passed if you did not run them or do not have a trustworthy result for them.\n- When verification is partial, list which checks were run, which checks failed, which checks were not run, and whether the user needs to run anything manually.\n- If a verification command fails, say so directly and summarize the failure instead of softening it into a successful outcome.\n- Recommended flow for non-trivial tasks: agent_explore -> confirm goal -> update_plan -> wait for approval -> implement -> agent_review(target='code' or 'diff').\n- Skip delegation only when the task is small, obvious, and isolated enough that extra helper work would not pay off.\n- Adapt answer length and prose density to the active response style: in concise mode, give the shortest correct answer; in balanced mode, write enough to be clear — a few paragraphs, not a wall of bullets; in guided mode, explain reasoning and tradeoffs in full. Show file paths clearly when working with files.\n- When summarizing your actions, describe what you did in plain text — do not re-read or re-cat files to prove your work.\n- Flag risks, destructive operations, or ambiguity before acting. Ask when intent is unclear.".to_string(),
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
                body: build_sandbox_permissions_body(ctx.pool, ctx.run_mode, ctx.workspace_path)
                    .await?,
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

impl PromptSectionProvider for SkillsProvider {
    async fn collect(&self, ctx: &PromptBuildContext<'_>) -> Result<Vec<PromptSection>, AppError> {
        let skills = ExtensionsManager::new(ctx.pool.clone())
            .list_skills(Some(ctx.workspace_path), ConfigScope::Workspace)
            .await?;
        let enabled_skills = skills
            .into_iter()
            .filter(|skill| skill.enabled)
            .collect::<Vec<_>>();

        if enabled_skills.is_empty() {
            return Ok(Vec::new());
        }

        let mut lines = vec![
            "A skill is a set of local instructions to follow that is stored in a `SKILL.md` file. Below is the list of skills that can be used. Each entry includes a name, description, and file path so you can open the source for full instructions when using a specific skill.".to_string(),
            String::new(),
            "### Available skills".to_string(),
        ];

        for skill in enabled_skills {
            let description = skill
                .description
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .unwrap_or("No description provided.");
            let skill_file = Path::new(&skill.path).join("SKILL.md");
            lines.push(format!(
                "- {}: {} (file: {})",
                skill.name,
                description,
                skill_file.display()
            ));
        }

        lines.push(String::new());
        lines.push("### How to use skills".to_string());
        lines.push("- Discovery: The list above is the skills available in this session (name + description + file path). Skill bodies live on disk at the listed paths.".to_string());
        lines.push("- Trigger rules: If the user names a skill (with `$SkillName` or plain text) OR the task clearly matches a skill's description shown above, you must use that skill for that turn. Multiple mentions mean use them all. Do not carry skills across turns unless re-mentioned.".to_string());
        lines.push("- Missing/blocked: If a named skill isn't in the list or the path can't be read, say so briefly and continue with the best fallback.".to_string());
        lines.push("- How to use a skill (progressive disclosure):".to_string());
        lines.push("  1. After deciding to use a skill, open its `SKILL.md`. Before using a skill, read its `SKILL.md` completely unless the file is clearly only metadata plus links and the relevant workflow section has been fully loaded.".to_string());
        lines.push("  2. When `SKILL.md` references relative paths (for example, `scripts/foo.py`), resolve them relative to the skill directory listed above first, and only consider other paths if needed.".to_string());
        lines.push("  3. If `SKILL.md` points to extra folders such as `references/`, load only the specific files needed for the request; don't bulk-load everything.".to_string());
        lines.push("  4. If `scripts/` exist, prefer running or patching them instead of retyping large code blocks.".to_string());
        lines.push(
            "  5. If `assets/` or templates exist, reuse them instead of recreating from scratch."
                .to_string(),
        );
        lines.push("- Coordination and sequencing:".to_string());
        lines.push("  - If multiple skills apply, choose the minimal set that covers the request and state the order you'll use them.".to_string());
        lines.push("  - Announce which skill(s) you're using and why (one short line). If you skip an obvious skill, say why.".to_string());
        lines.push("- Context hygiene:".to_string());
        lines.push("  - Keep context small: summarize long sections instead of pasting them; only load extra files when needed.".to_string());
        lines.push("  - Avoid deep reference-chasing: prefer opening only files directly linked from `SKILL.md` unless you're blocked.".to_string());
        lines.push("  - When variants exist (frameworks, providers, domains), pick only the relevant reference file(s) and note that choice.".to_string());
        lines.push("- Safety and fallback: If a skill can't be applied cleanly (missing files, unclear instructions), state the issue, pick the next-best approach, and continue.".to_string());

        Ok(vec![PromptSection {
            key: "skills",
            title: "Skills",
            body: lines.join("\n"),
            phase: PromptPhase::Capability,
            order_in_phase: 20,
        }])
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
            normalize_profile_response_language(ctx.raw_plan.response_language.as_deref())
                .is_some();
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

        // NOTE: Dynamic values like the current date are intentionally excluded from
        // the system prompt to keep it stable for LLM prompt prefix caching.
        // The date is injected via the runtime context message in agent_session.rs.
        sections.push(PromptSection {
            key: "runtime_context",
            title: "Runtime Context",
            body: format!("Workspace path: {}", ctx.workspace_path),
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
    use crate::core::workspace_paths::{merge_writable_roots, parse_writable_roots};

    let approval_policy = settings_repo::policy_get(pool, "approval_policy")
        .await?
        .map(|record| parse_approval_policy_mode(&record.value_json))
        .unwrap_or_else(|| "require_for_mutations".to_string());

    let writable_roots: Vec<String> = settings_repo::policy_get(pool, "writable_roots")
        .await?
        .map(|record| parse_writable_roots(&record.value_json))
        .map(|roots| merge_writable_roots(&roots))
        .unwrap_or_else(|| merge_writable_roots(&[]));

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
    let current_date = chrono::Local::now().format("%Y-%m-%d").to_string();

    format!(
        "- Operating system: {}\n- Architecture: {}\n- Default shell: {}\n- Current date: {}",
        std::env::consts::OS,
        std::env::consts::ARCH,
        shell,
        current_date,
    )
}

fn build_shell_tooling_guide_body() -> String {
    format!(
        "- Shell commands run through the user's default shell (`{shell}`).\n- This section is a shell command selection and boundary guide. Prefer workspace-aware tools (`read`, `list`, `search`, `find`, `edit`) before shell when they fit.\n- Use `shell` for one-shot non-interactive commands in the workspace.\n- Use `term_status`, `term_output`, `term_write`, `term_restart`, and `term_close` only for the desktop app's embedded Terminal panel session for the current thread. They inspect or control that persistent panel session and do not replace one-shot `shell` execution.\n- Do not assume any particular CLI tool (for example `node`, `python`, `pip`, `git`, or `rg`) is available on the user's machine. Verify availability with a quick probe (such as `command -v <tool>`) before proposing a shell command that depends on it, or prefer the workspace-aware tools when they can accomplish the task.\n- When `rg` is unavailable, fall back to the built-in `search` and `find` tools before broad shell scans.",
        shell = current_shell()
    )
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_system_environment_body_omits_cli_tool_section() {
        let body = build_system_environment_body();

        assert!(body.contains("- Operating system:"));
        assert!(body.contains("- Architecture:"));
        assert!(body.contains("- Default shell:"));
        assert!(body.contains("- Current date:"));
        assert!(!body.contains("Common CLI tools"));
    }

    #[test]
    fn build_shell_tooling_guide_body_is_static_and_tool_agnostic() {
        let body = build_shell_tooling_guide_body();

        assert!(body.contains("Shell commands run through the user's default shell"));
        assert!(body.contains("Prefer workspace-aware tools"));
        assert!(body.contains("Do not assume any particular CLI tool"));
        assert!(body.contains("When `rg` is unavailable"));
    }
}

pub(crate) fn final_response_structure_system_instruction() -> &'static str {
    "For conclusion-oriented replies, choose a structure that matches the task instead of forcing one template for every situation.\n- Keep the outer Markdown layout disciplined: use at most two heading levels in one reply, avoid turning every sub-point into its own heading, and prefer short sections with lists underneath over a long chain of peer headers.\n- When the reply is more than a very small update, prefer a clearly structured Markdown presentation instead of one dense block of prose.\n- Use short Markdown section headers for the main sections only. Put supporting detail inside numbered lists or flat bullet lists rather than promoting each detail to a new heading.\n- Use numbered lists for ordered reasons, changes, or options. Use flat bullet lists for evidence, verification items, or supporting facts.\n- Use emphasis or inline code sparingly to highlight the key conclusion, the recommended option, commands, file paths, settings, or identifiers that the user should notice quickly. Do not overload the reply with inline code formatting.\n- For simple tasks, you may compress the structure into a short paragraph or a short flat list, but keep a clear top-down order.\n- Use one of these default patterns:\n\n  - Debug or problem analysis: conclusion -> causes 1, 2, and 3 if relevant -> evidence tied to each cause -> recommendation options 1, 2, and 3 with a recommended option.\n\n  - Code change or result report: outcome -> key changes 1, 2, and 3 if relevant -> verification or evidence -> next steps, risks, or follow-up recommendation.\n\n  - Comparison or decision support: recommendation -> options 1, 2, and 3 -> tradeoffs and evidence -> clearly state the recommended option and why.\n\n  - Direct explanation or question answering: direct answer -> key points 1, 2, and 3 if relevant -> examples or evidence when helpful -> next step only if it adds value.\n- Do not force explicit headings on every reply unless the task benefits from a more structured presentation.\n- Write complete, grammatically whole sentences in every bullet point and paragraph. Avoid telegraph-style fragments (e.g. bare noun phrases like 'Plugin 执行协议已改为结构化'). Instead write full sentences that include subject, verb, and enough context to stand on their own.\n- When three or more closely related points share a single theme, merge them into one short paragraph with a topic sentence instead of listing each as a separate bullet.\n- If a single section exceeds roughly 8-10 lines of output, consider whether it should be split into two sections with distinct headers, or whether some detail can be folded into a summary sentence."
}

pub(crate) fn run_mode_prompt_body(run_mode: &str) -> String {
    match run_mode {
        "plan" => format!(
            "Plan mode is active.\n\
\n\
## Goal\n\
Your sole objective is to produce a concrete, evidence-based implementation plan that can be directly approved and executed. You are NOT implementing the change — you are building the plan.\n\
\n\
## Available tools\n\
Read-only tools: read, list, search, find, term_status, term_output, agent_explore.\n\
Planning tools: clarify, update_plan.\n\
{TERM_PANEL_USAGE_NOTE}\n\
Do NOT use edit, write, shell, or any mutating tool unless the user explicitly requests execution.\n\
\n\
## Workflow — follow these phases in order\n\
\n\
### Phase 1: Explore and understand\n\
Before writing any plan, build a grounded understanding of the task and the codebase.\n\
- Use read, search, find, and list to inspect relevant files, modules, and patterns.\n\
- Use agent_explore for cross-file investigation, dependency mapping, and current-state analysis.\n\
- Identify existing patterns, reusable modules, constraints, and conventions.\n\
- Do NOT rush to call update_plan. Invest enough exploration to base the plan on evidence, not speculation.\n\
- If the codebase is unfamiliar or the scope is broad, explore before forming any opinion.\n\
\n\
### Phase 2: Clarify ambiguities\n\
After exploration, determine whether any implementation-blocking uncertainty remains that you cannot resolve from code alone.\n\
- Use clarify ONLY for decisions the user must make: scope choices, preference between valid approaches, priority tradeoffs, or constraints not discoverable in code.\n\
- Do NOT ask questions that code exploration can answer.\n\
- Batch related questions into a single clarify call. Offer 2-4 concise options with a recommended choice when possible.\n\
- After calling clarify, STOP and wait for the user's answer before continuing.\n\
- Skip this phase entirely if exploration resolved all uncertainties.\n\
\n\
### Phase 3: Converge on a recommendation\n\
Synthesize exploration evidence and any clarification answers into a single recommended approach.\n\
- Converge to ONE recommended approach. Do not present multiple unranked alternatives.\n\
- Ensure every major design decision is grounded in inspected code, user input, or documented constraints.\n\
- If you discover that a previously assumed approach is invalid during convergence, return to Phase 1 for targeted exploration.\n\
\n\
### Phase 4: Publish the plan\n\
Call update_plan to publish the formal implementation plan. This is the only way to complete a plan-mode run.\n\
- A prose answer alone does NOT complete the run. You must call update_plan.\n\
- Once published, the run pauses for user approval before any implementation can begin.\n\
- The plan is automatically saved to a file on disk (the file path is returned in the tool result). This file persists across runs and can be referenced during implementation and review.\n\
- You may call update_plan multiple times during a single run to incrementally refine the plan. Each call overwrites the previous plan file. Use this to capture progress as your understanding deepens rather than waiting until the very end.\n\
\n\
## Plan quality contract — what makes a plan approvable\n\
\n\
Every plan published via update_plan must satisfy these requirements:\n\
\n\
Content requirements:\n\
- `summary`: State what is being changed, why, and the expected outcome. Keep it to 2-3 sentences.\n\
- `context`: Write a thorough narrative of confirmed facts from inspected code, documentation, or user input. Do not output a bare bullet list — connect the facts into coherent paragraphs that tell the reader exactly what the current state is, how the relevant pieces fit together, and what constraints or conventions exist. Include file paths, type signatures, data flow direction, and any version or compatibility details you discovered. The goal is a self-contained briefing that someone unfamiliar with the code area can read and fully understand the starting point. Never speculate about files, architecture, or behavior you have not verified.\n\
- `design`: Write a detailed prose description of the recommended approach. Explain the architecture or structural changes, walk through the data flow or control flow step by step, and articulate why this approach is chosen over alternatives by comparing tradeoffs explicitly. Cover edge cases the design handles and those it deliberately defers. Do not reduce this to a bare list of decisions — the reader should finish this section understanding both the what and the why at a level sufficient to implement without further design questions.\n\
- `keyImplementation`: Write a connected prose description of the specific files, modules, interfaces, data flows, or state transitions that carry the change. For each major component, explain what it does today, what changes, and how the changed pieces interact with each other. Include type names, function signatures, and module boundaries where they clarify the narrative. Vague references like 'update the relevant files' are not acceptable — every touched file or interface should be named and its role in the change explained.\n\
- `steps`: Write concrete, ordered, actionable steps. Each step should specify the affected file(s) or subsystem(s) and the intended outcome. Prefer steps that are independently understandable and verifiable.\n\
- `verification`: Write a thorough description of how to validate the change succeeded. Cover type-checks, unit tests, integration tests, manual smoke tests, and any behavioral verification relevant to the change. Mention specific commands to run, expected outputs, and edge cases worth verifying manually. Do not reduce this to a bare checklist — explain what each check proves and why it matters.\n\
- `risks`: List the main risks, edge cases, compatibility concerns, and likely regression areas.\n\
- `assumptions`: Include only non-blocking assumptions clearly labeled as such, not open questions.\n\
\n\
Prohibited in a plan:\n\
- Unresolved core ambiguities pushed to the approval step — if a key decision is still open, use clarify first.\n\
- TODO placeholders, 'to be decided' items, or vague 'investigate further' steps.\n\
- Lengthy background essays that add no actionable implementation information.\n\
- Architecture or file structure guesses not backed by exploration evidence.\n\
- Repeating the user's original request verbatim as context.\n\
\n\
Quality bar:\n\
- The plan must be specific enough that implementation can proceed directly from it after approval.\n\
- Someone reading only the plan should understand: what changes, where in the codebase, what gets reused, and how success is verified.\n\
- Thoroughness is valued — narrative sections (context, design, keyImplementation, verification) should be detailed enough that a developer unfamiliar with the area can understand and implement the change without asking follow-up questions. Prefer connected prose over bare bullet lists for these sections."
        ),
        _ => format!(
            "Default execution mode is active.\n- Use the configured tool profile, subject to policy, approvals, and workspace boundaries.\n- {TERM_PANEL_USAGE_NOTE}\n- Use clarify instead of guessing when the user should choose between multiple reasonable approaches, confirm a preference, decide scope, approve a risky action, or fill in missing requirements before you continue.\n- When the next step is clear and low-risk, move the task forward without unnecessary clarification.\n- If implementation should pause for review first because the work is complex, cross-file, or risky, publish an implementation plan with update_plan before making changes.\n- If an unresolved requirement, preference, or scope decision blocks the implementation plan, use clarify first and wait for the answer before calling update_plan.\n- When calling update_plan, follow the quality contract described in the update_plan tool description. Explore the codebase first, then provide a concrete plan with all required sections.\n- Prefer the smallest sufficient action that moves the task forward."
        ),
    }
}
