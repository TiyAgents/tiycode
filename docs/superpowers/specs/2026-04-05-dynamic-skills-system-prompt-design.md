# Dynamic Skills System Prompt Design

## Goal

Inject a `Skills` section into the main agent system prompt so the prompt always includes the current enabled skills available in the session.

## Scope

This change only adds dynamic skill availability metadata to the system prompt. It does not inject full `SKILL.md` bodies, pinned skill content, or prompt-budget-based skill body selection.

## Decisions

1. Only include skills whose runtime record has `enabled = true`.
2. Resolve skills dynamically from the existing extensions runtime instead of hardcoding a static list.
3. Render skills through a dedicated prompt provider instead of appending ad hoc text in the assembler.
4. Omit the section entirely when no enabled skills are available.

## Data Source

Use `ExtensionsManager::list_skills(Some(workspace_path), ConfigScope::Workspace)`.

This preserves the existing runtime behavior for:

- builtin skills
- workspace skills
- plugin-provided skills from enabled plugins
- workspace/global skill enabled-state overrides

## Prompt Shape

Render a new `## Skills` section containing:

1. A short explanation of what a skill is.
2. `### Available skills` with one bullet per enabled skill in the form:
   `- <name>: <description>. (file: <absolute path>/SKILL.md)`
3. `### How to use skills` with the fixed operating rules for discovery, trigger behavior, progressive loading, coordination, context hygiene, and fallback behavior.

## Placement

Add a dedicated `SkillsProvider` in the prompt provider layer and place the section in the `Capability` phase after the shell tooling guidance.

## Non-Goals

- Selecting a subset of skills by pin state
- Injecting full skill bodies into the system prompt
- Changing subagent prompt inheritance
- Adding a new settings surface for prompt-time skill filtering
