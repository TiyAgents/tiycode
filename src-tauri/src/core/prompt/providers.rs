use crate::core::subagent::TERM_PANEL_USAGE_NOTE;

/// Static final response structure instruction text.
/// Retained for backward-compat tests that snapshot the content.
#[allow(dead_code)]
pub(crate) fn final_response_structure_system_instruction() -> &'static str {
    "For conclusion-oriented replies, choose a structure that matches the task instead of forcing one template for every situation.\n- Keep the outer Markdown layout disciplined: use at most two heading levels in one reply, avoid turning every sub-point into its own heading, and prefer short sections with lists underneath over a long chain of peer headers.\n- When the reply is more than a very small update, prefer a clearly structured Markdown presentation instead of one dense block of prose.\n- Use short Markdown section headers for the main sections only. Put supporting detail inside numbered lists or flat bullet lists rather than promoting each detail to a new heading.\n- Use numbered lists for ordered reasons, changes, or options. Use flat bullet lists for evidence, verification items, or supporting facts.\n- Use emphasis or inline code sparingly to highlight the key conclusion, the recommended option, commands, file paths, settings, or identifiers that the user should notice quickly. Do not overload the reply with inline code formatting.\n- For simple tasks, you may compress the structure into a short paragraph or a short flat list, but keep a clear top-down order.\n- Use one of these default patterns:\n\n  - Debug or problem analysis: conclusion -> causes 1, 2, and 3 if relevant -> evidence tied to each cause -> recommendation options 1, 2, and 3 with a recommended option.\n\n  - Code change or result report: outcome -> key changes 1, 2, and 3 if relevant -> verification or evidence -> next steps, risks, or follow-up recommendation.\n\n  - Comparison or decision support: recommendation -> options 1, 2, and 3 -> tradeoffs and evidence -> clearly state the recommended option and why.\n\n  - Direct explanation or question answering: direct answer -> key points 1, 2, and 3 if relevant -> examples or evidence when helpful -> next step only if it adds value.\n- Do not force explicit headings on every reply unless the task benefits from a more structured presentation.\n- Write complete, grammatically whole sentences in every bullet point and paragraph. Avoid telegraph-style fragments (e.g. bare noun phrases like 'Plugin 执行协议已改为结构化'). Instead write full sentences that include subject, verb, and enough context to stand on their own.\n- When three or more closely related points share a single theme, merge them into one short paragraph with a topic sentence instead of listing each as a separate bullet.\n- If a single section exceeds roughly 8-10 lines of output, consider whether it should be split into two sections with distinct headers, or whether some detail can be folded into a summary sentence."
}

/// Static run mode prompt body text.
/// Retained for backward-compat tests that snapshot the content.
#[allow(dead_code)]
pub(crate) fn run_mode_prompt_body(run_mode: &str) -> String {
    match run_mode {
        "plan" => format!(
            "Plan mode is active.\n\
\n\
## Goal\n\
Your sole objective is to produce a concrete, evidence-based implementation plan that can be directly approved and executed. You are NOT implementing the change — you are building the plan.\n\
\n\
## Available tools\n\
Read-only tools: read, list, search, find, term_status, term_output, agent_explore, agent_parallel.\n\
Shell tool: shell — use ONLY for read-only commands (e.g. git log, npm ls, command -v, skill CLIs for information gathering). Never use shell to create, modify, or delete files or to run system-changing commands.\n\
Planning tools: clarify, update_plan.\n\
{TERM_PANEL_USAGE_NOTE}\n\
Do NOT use edit, write, or any mutating tool unless the user explicitly requests execution.\n\
\n\
## Workflow — follow these phases in order\n\
\n\
### Phase 1: Explore and understand\n\
Before writing any plan, build a grounded understanding of the task and the codebase.\n\
- Use read, search, find, and list to inspect relevant files, modules, and patterns.\n\
- Use agent_parallel when broad read-only exploration can be split into 1-5 independent topics; prefer this over sequential agent_explore calls for separable areas such as backend/frontend/persistence, data flow/UI state/tests, or security/performance/compatibility probes. Keep each subtask low side-effect and independent.\n\
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
