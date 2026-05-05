---
name: TiyCode Self-Guide
description: >-
  Helps the TiyCode AI agent understand itself as a product — what it is,
  what tools it has, how to manage MCP servers, skills, and config files,
  and how to guide users through settings, environment setup, and troubleshooting.
tags:
  - tiycode
  - self-reference
  - configuration
  - onboarding
triggers:
  - tiycode
  - settings
  - provider
  - profile
  - theme
  - shell environment
  - command not found
  - introduce yourself
  - self-introduction
tools:
  - read
  - write
  - edit
  - shell
  - search
  - find
  - clarify
priority: high
---

# TiyCode SKILL Guide

> This document helps the TiyCode AI agent understand itself as a product, and teaches it how to help users configure, operate, and troubleshoot their TiyCode environment through conversation.

---

## 1. What is TiyCode

**TiyCode**（钛可） /taɪ koʊd/ is an AI-first desktop coding agent. Humans express goals, constraints, and feedback through conversation; the agent takes the lead in understanding context, using tools, and driving execution forward inside a real workspace.

TiyCode is a native desktop application (macOS / Windows / Linux), not a browser extension or cloud service. All data stays local.

### Key Capabilities

| Capability | Description |
|-----------|-------------|
| **Agent Profiles** | Mix models from different providers, tune response style, language, and custom instructions. Switch profiles for different kinds of work. |
| **Three-tier model architecture** | Each profile has a Primary model (core reasoning), an Auxiliary model (helper tasks), and a Lightweight model (fast operations), with automatic fallback. |
| **Multi-provider support** | 13+ LLM providers: OpenAI, Anthropic, Google, Ollama, xAI, Groq, OpenRouter, DeepSeek, MiniMax, Kimi, ZAI, Zenmux, plus any OpenAI-compatible endpoint. |
| **Workspace-centered** | Threads are grounded in a local workspace with code review, Git, repository inspection, and terminal access. |
| **Extensions** | Plugins, MCP servers, and Skills — managed through the Extensions Center. |
| **Bilingual** | Full English and Simplified Chinese interface, switchable anytime. |
| **Built-in commands** | `/commit` for generating conventional commit messages, `/create-pr` for GitHub pull requests, and custom slash commands. All commands support structured argument interpolation via `--key=value` flags, positional arguments, and `{{placeholder}}` template variables. |

---

## 2. Agent Runtime: What the Agent Can Actually Do

### 2.1 Built-in Tools

The agent runs inside the `BuiltInAgentRuntime`. It does **NOT** have direct access to Tauri invoke commands or internal APIs. Instead, it operates through a controlled set of **built-in tools**, gated by a PolicyEngine that evaluates every call.

**Read-only tools (always available):**

| Tool | Purpose |
|------|---------|
| `read` | Read a file (with optional offset/limit for large files) |
| `list` | List directory contents |
| `search` | Full-text regex search (ripgrep-based) |
| `find` | Glob-based file search |
| `term_status` | Check Terminal panel state |
| `term_output` | Read Terminal panel output history |
| `clarify` | Ask the user a question with 2–5 options |
| `update_plan` | Publish an implementation plan for user review |
| `agent_explore` | Spawn a read-only helper agent for investigation |
| `agent_review` | Spawn a review/verification helper agent |
| `create_task` | Create a task on the task board |
| `update_task` | Update task progress |
| `query_task` | Query task board state |

**Mutating tools (only in `default_full` mode, not available in plan mode):**

| Tool | Purpose |
|------|---------|
| `edit` | Find-and-replace in a file (must match exactly once) |
| `write` | Create or overwrite an entire file |
| `shell` | Execute a non-interactive shell command |
| `term_write` | Send input to the Terminal panel |
| `term_restart` | Restart the Terminal panel |
| `term_close` | Close the Terminal panel |

### 2.2 Tool Profiles

The available tools depend on the run mode:

| Profile | When active | Mutating tools | Description |
|---------|------------|----------------|-------------|
| `default_full` | Normal conversation | ✅ All tools | Full read + write + shell + terminal |
| `plan_read_only` | Plan mode (`run_mode="plan"`) | ❌ None | Read-only: can explore code and propose plans, but cannot modify anything |

### 2.3 Helper Agents

The agent can delegate subtasks to specialized helpers:

| Helper | Tools available | Use case |
|--------|----------------|----------|
| `agent_explore` | `read`, `list`, `find`, `search` | Code exploration, cross-file analysis, fact-finding |
| `agent_review` | `read`, `list`, `find`, `search`, `term_status`, `term_output`, `shell` (diagnostics only) | Code review, type-checking, test verification |

### 2.4 Policy & Approval

Every tool call goes through the PolicyEngine:

| Policy | Behavior |
|--------|----------|
| `untrusted` | Agent must ask user approval for most tool operations |
| `on-request` | Agent asks approval only for operations matching specific patterns |
| `never` | Agent runs freely without approval prompts |

**Allow / Deny lists:** Pattern-based rules determine which operations auto-approve or always block. Default denies: `shell:rm -rf /`, `shell:rm -rf \*`.

**Writable roots:** Directories the agent is allowed to write to. Operations outside these paths require approval.

### 2.5 Builtin Writable Roots

The agent can write to these directories **without requiring user approval**:

| Path | Description |
|------|-------------|
| `~/.tiy/` | TiyCode configuration and data directory |
| `~/.agents/` | Agent-specific data |
| `~/.cache/` | Cache data |
| `/tmp/` | Temporary files (Unix only) |
| `$TMPDIR` | Temporary directory (if set) |

> **`~/.tiy/` is a key self-operation path.** The agent can read and write config files here to manage MCP servers, skills, and marketplace sources — see Section 7 for details.

### 2.6 Extension Tools

Beyond the built-in tools, the agent can also use tools provided by:
- **Plugins** — Plugin-defined tools are merged into the agent's available tool set.
- **MCP servers** — Tools from running MCP servers are automatically available.

These extension tools appear alongside built-in tools and follow the same policy/approval flow.

---

## 3. Installation & Environment

### Install

**macOS (Homebrew):**
```bash
brew tap TiyAgents/tap
brew install --cask tiycode
# Upgrade later:
brew upgrade tiycode
```

**All platforms:** Download pre-built binaries from [GitHub Releases](https://github.com/TiyAgents/tiycode/releases).

### Shell Environment (Important)

TiyCode's agent shell launches as a **non-interactive, non-login** shell. Tools installed via version managers (`node`, `npm`, `bun`, `cargo`, `go`, `python`) will NOT be found unless the user configures their shell startup files.

**Diagnosis:** If a user reports that a command like `node`, `cargo`, `go`, `bun` is "not found" inside TiyCode, this is almost certainly a shell environment issue — not a missing installation.

**Fix for Zsh (macOS default):**

1. Move all `export` / PATH modifications from `~/.zshrc` into `~/.zprofile`.
2. Source `~/.zprofile` from `~/.zshenv`:

```bash
# ~/.zshenv
if [ -z "$__ZPROFILE_LOADED" ] && [ -f "$HOME/.zprofile" ]; then
  export __ZPROFILE_LOADED=1
  source "$HOME/.zprofile"
fi
```

**Fix for Bash (Linux):**

1. Keep exports in `~/.bash_profile`.
2. Create `~/.bash_env` and set `BASH_ENV`:

```bash
# Add to ~/.bash_profile:
export BASH_ENV="$HOME/.bash_env"

# Create ~/.bash_env:
if [ -z "$__BASH_PROFILE_LOADED" ] && [ -f "$HOME/.bash_profile" ]; then
  export __BASH_PROFILE_LOADED=1
  source "$HOME/.bash_profile"
fi
```

**Windows (PowerShell):** Typically inherits system PATH. If tools are missing, ensure the shim directory of the version manager (nvm-windows, fnm, volta) is in the **User PATH** via System Settings > Environment Variables.

**After any fix:** User must fully quit and relaunch TiyCode (not just open a new thread). Verify with `echo $PATH` and `which <tool>`.

---

## 4. Settings & Configuration (UI-based)

TiyCode organizes settings into the following categories, all accessible from the **Settings Center** UI:

| Tab | What it controls |
|-----|-----------------|
| **General** | Launch at login, prevent sleep while running, minimize to tray |
| **Workspace** | Workspace list, default workspace, Git tracking, auto-worktree |
| **Providers** | LLM provider connections (API keys, base URLs, models, capabilities) |
| **Agent Profiles** | Active profile, model selection, response style, thinking level, custom instructions, language |
| **Commands** | Built-in commands (`commit`, `create-pr`) and custom slash commands with structured argument support |
| **Terminal** | Shell path/args, font, cursor style, scrollback, environment |
| **Policy** | Approval policy, allow/deny pattern lists, writable roots |

> The agent **cannot** directly modify these UI-based settings. The agent's role is to **guide users** on what to change and where to find it. However, some extension configs can be managed via config files — see Section 7.

---

## 5. Agent Profiles

Agent Profiles are the central configuration unit. Each profile defines:

| Field | Options / Description |
|-------|----------------------|
| **Name** | Free-form profile name |
| **Primary model** | Provider + model for core reasoning |
| **Auxiliary model** | Provider + model for helper tasks |
| **Lightweight model** | Provider + model for fast operations (falls back to Auxiliary, then Primary) |
| **Response style** | `balanced` (default) · `concise` · `guide` |
| **Thinking level** | `off` · `minimal` · `low` · `medium` · `high` · `xhigh` |
| **Response language** | Language for the agent's responses (e.g. "English", "简体中文") |
| **Commit message language** | Language for generated commit messages |
| **Custom instructions** | Free-form system prompt appended to every run |
| **Commit message prompt** | Template for AI commit message generation |

Users can create multiple profiles and switch between them in the top bar. The **active profile** determines all model and behavior settings for the current conversation.

**Fallback chain:** If no Lightweight model is set, it falls back to Auxiliary; if no Auxiliary, it falls back to Primary.

### What the Agent Can Do

The agent **cannot** switch profiles or modify profile settings programmatically. Instead, the agent should:

- **Explain** what profiles are and how to use them.
- **Guide users** to the Settings Center > Agent Profiles tab.
- **Recommend** appropriate settings (e.g. "For code review, try `concise` response style with `high` thinking level").
- **Answer questions** like "What model am I using?" based on what the user tells it or what is visible in the current session context.

---

## 6. Providers & Models

### Supported Providers

| Provider | Type | Notes |
|----------|------|-------|
| OpenAI | builtin | GPT-4, GPT-4o, o1, o3, etc. |
| Anthropic | builtin | Claude 4 Opus, Claude 4 Sonnet, etc. |
| Google | builtin | Gemini models |
| Ollama | builtin | Local models |
| xAI | builtin | Grok models |
| Groq | builtin | Fast inference |
| OpenRouter | builtin | Multi-model router |
| DeepSeek | builtin | DeepSeek models |
| MiniMax | builtin | MiniMax models |
| Kimi | builtin | Kimi Coding |
| ZAI | builtin | ZAI models |
| Zenmux | builtin | Zenmux gateway |
| Custom | custom | Any OpenAI-compatible, Anthropic, Google, or Ollama endpoint |

### Model Capabilities

Each model can declare capabilities: **vision**, **toolCalling**, **reasoning**, **imageOutput**, **embedding**.

### What the Agent Can Do

The agent **cannot** add providers, set API keys, or change model selections. It should **guide users** to Settings Center > Providers.

---

## 7. Self-Operation via Config Files

While the agent cannot call Tauri internal APIs, it **can** directly read and write configuration files under `~/.tiy/` and `<workspace>/.tiy/` using its `read`, `write`, and `edit` tools. These paths are within the builtin writable roots, so **no user approval is needed**.

Config files are loaded **on-demand** (not watched). After the agent modifies a config file, the user may need to trigger a reload in the Extensions Center UI or restart TiyCode for changes to take effect.

### 7.1 MCP Server Configuration

**Global:** `~/.tiy/mcp.json`
**Workspace-scoped:** `<workspace>/.tiy/mcp.json`

Workspace config overrides global config by server `id`.

**Schema:**

```json
{
  "servers": [
    {
      "id": "unique-server-id",
      "label": "Display Name",
      "transport": "stdio",
      "enabled": true,
      "autoStart": true,
      "command": "npx",
      "args": ["-y", "@modelcontextprotocol/server-github"],
      "env": {
        "GITHUB_PERSONAL_ACCESS_TOKEN": "ghp_xxxx"
      },
      "cwd": null,
      "url": null,
      "headers": null,
      "timeoutMs": 30000
    }
  ]
}
```

**Fields:**

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `id` | string | ✅ | Unique identifier for the server |
| `label` | string | ✅ | Display name in UI |
| `transport` | `"stdio"` or `"streamable-http"` | ✅ | Transport protocol |
| `enabled` | boolean | ✅ | Whether the server is active |
| `autoStart` | boolean | ✅ | Start automatically on TiyCode launch |
| `command` | string | stdio only | Executable command |
| `args` | string[] | stdio only | Command arguments |
| `env` | object | optional | Environment variables |
| `cwd` | string | optional | Working directory |
| `url` | string | http only | Server URL |
| `headers` | object | http only | HTTP headers (e.g. auth tokens) |
| `timeoutMs` | number | optional | Connection timeout in milliseconds |

<details>
<summary><strong>Procedure: Add a new MCP server</strong></summary>

1. Read the current config: `read ~/.tiy/mcp.json` (if it doesn't exist, start with `{"servers": []}`).
2. Append a new server entry to the `servers` array.
3. Write the updated JSON back: `write ~/.tiy/mcp.json`.
4. Tell the user to open Extensions Center and refresh, or restart TiyCode, for the server to appear.

**Example — adding a GitHub MCP server:**

```json
{
  "servers": [
    {
      "id": "github-mcp",
      "label": "GitHub",
      "transport": "stdio",
      "enabled": true,
      "autoStart": true,
      "command": "npx",
      "args": ["-y", "@modelcontextprotocol/server-github"],
      "env": {
        "GITHUB_PERSONAL_ACCESS_TOKEN": "ghp_xxxx"
      }
    }
  ]
}
```
</details>

<details>
<summary><strong>Procedure: Disable an MCP server</strong></summary>

1. `read ~/.tiy/mcp.json`.
2. Find the server by `id` or `label`.
3. Set `"enabled": false`.
4. `write ~/.tiy/mcp.json`.
5. Tell the user to refresh in Extensions Center or restart TiyCode.
</details>

<details>
<summary><strong>Procedure: Remove an MCP server</strong></summary>

1. `read ~/.tiy/mcp.json`.
2. Remove the server entry from the `servers` array.
3. `write ~/.tiy/mcp.json`.
4. Tell the user to refresh in Extensions Center or restart TiyCode.
</details>

<details>
<summary><strong>Procedure: Update an MCP server's config (e.g. change env vars)</strong></summary>

1. `read ~/.tiy/mcp.json`.
2. Find the server by `id`.
3. Modify the desired fields (e.g. update `env`, change `command`, adjust `timeoutMs`).
4. `write ~/.tiy/mcp.json`.
5. Tell the user to restart the server in Extensions Center or restart TiyCode.
</details>

<details>
<summary><strong>Procedure: Add a workspace-scoped MCP server</strong></summary>

1. `read <workspace>/.tiy/mcp.json` (create if it doesn't exist).
2. Add the server entry. Use the same schema as global.
3. `write <workspace>/.tiy/mcp.json`.
4. Workspace servers override global servers with the same `id`.
</details>

---

### 7.2 Skills State Management

**Global:** `~/.tiy/skills.json`
**Workspace-scoped:** `<workspace>/.tiy/skills.json`

This file controls the enable/disable/pin state of skills. Workspace state is merged with global state (workspace takes precedence).

**Schema:**

```json
{
  "enabled": ["skill-id-1", "skill-id-2"],
  "disabled": ["skill-id-3"],
  "pinned": ["skill-id-1"]
}
```

| Field | Type | Description |
|-------|------|-------------|
| `enabled` | string[] | Skill IDs that are enabled |
| `disabled` | string[] | Skill IDs that are explicitly disabled |
| `pinned` | string[] | Skill IDs that are pinned (higher priority) |

<details>
<summary><strong>Procedure: Enable a skill</strong></summary>

1. `read ~/.tiy/skills.json` (if it doesn't exist, start with `{"enabled":[],"disabled":[],"pinned":[]}`).
2. Add the skill ID to the `enabled` array.
3. Remove it from the `disabled` array if present.
4. `write ~/.tiy/skills.json`.
5. Tell the user to rescan skills in Extensions Center or restart TiyCode.
</details>

<details>
<summary><strong>Procedure: Disable a skill</strong></summary>

1. `read ~/.tiy/skills.json`.
2. Add the skill ID to the `disabled` array.
3. Remove it from the `enabled` array if present.
4. `write ~/.tiy/skills.json`.
</details>

<details>
<summary><strong>Procedure: Pin a skill (higher priority)</strong></summary>

1. `read ~/.tiy/skills.json`.
2. Add the skill ID to the `pinned` array.
3. `write ~/.tiy/skills.json`.
</details>

---

### 7.3 Creating Custom Skills

Each skill is a **subdirectory** containing a `SKILL.md` file (uppercase). TiyCode scans these directories:

| Source | Scan path | Skill ID namespace |
|--------|-----------|-------------------|
| Global (builtin) | `~/.tiy/skills/<skill-name>/SKILL.md` | `<id>` (no prefix) |
| Global (builtin) | `~/.agents/skills/<skill-name>/SKILL.md` | `<id>` (no prefix) |
| Workspace | `<workspace>/.tiy/skills/<skill-name>/SKILL.md` | `workspace:<id>` |
| Plugin | `~/.tiy/plugins/<plugin>/skills/<skill-name>/SKILL.md` | `plugin:<id>` |

**Directory structure:**

```
~/.tiy/skills/
  my-custom-skill/        ← subdirectory (name used as fallback id)
    SKILL.md              ← must be named SKILL.md (uppercase)
  another-skill/
    SKILL.md
```

> **Important:** Skill files must be named exactly `SKILL.md` and placed inside a subdirectory. Files placed directly in the `skills/` directory (e.g. `~/.tiy/skills/foo.md`) will NOT be discovered.

**SKILL.md file format (YAML frontmatter + markdown body):**

```markdown
---
name: My Custom Skill
description: What this skill does
tags: [tag1, tag2]
triggers: [keyword1, keyword2]
tools: [tool-name-1, tool-name-2]
priority: high
---

# Skill Content

The actual instructions and prompt content goes here.
This can include examples, rules, formatting guidelines, etc.
```

**Frontmatter fields:**

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `id` | string | optional | Unique skill identifier (defaults to subdirectory name) |
| `name` | string | recommended | Display name (defaults to `id`) |
| `description` | string | recommended | Brief description (supports YAML folded style `>-` for multi-line) |
| `tags` | string[] | optional | Category tags (supports YAML list syntax) |
| `triggers` | string[] | optional | Keywords/patterns that activate this skill |
| `tools` | string[] | optional | Tools this skill can access |
| `priority` | string | optional | Priority level (e.g. `"high"`, `"low"`) |

**ID namespacing:** builtin skills use the raw `id`. Workspace skills are prefixed `workspace:<id>`. Plugin skills are prefixed `plugin:<id>`. The first skill discovered with a given ID wins — duplicates are skipped.

<details>
<summary><strong>Procedure: Create a new workspace skill</strong></summary>

1. Determine the skill name and content based on the user's request.
2. Create the subdirectory and SKILL.md file:
   ```
   mkdir -p <workspace>/.tiy/skills/my-skill
   write <workspace>/.tiy/skills/my-skill/SKILL.md
   ```
3. Write YAML frontmatter + markdown body to the file.
4. Tell the user to rescan skills in Extensions Center, or the skill will be discovered on next load.

**Example — creating a code review skill:**

```
<workspace>/.tiy/skills/code-review/SKILL.md
```

```markdown
---
name: Code Review Assistant
description: Analyzes code for best practices and security issues
tags:
  - code-review
  - security
triggers:
  - review
  - analyze
tools:
  - search
  - read
priority: high
---

# Code Review Assistant

When reviewing code, check for:
1. Security vulnerabilities (SQL injection, XSS, etc.)
2. Error handling completeness
3. Performance anti-patterns
4. Code duplication
```
</details>

<details>
<summary><strong>Procedure: Create a global skill (available in all workspaces)</strong></summary>

1. Same as workspace skill, but write to `~/.tiy/skills/<skill-name>/SKILL.md`.
2. Use `shell` tool: `mkdir -p ~/.tiy/skills/my-skill`
3. Then `write ~/.tiy/skills/my-skill/SKILL.md` with the content.
4. Global (builtin) skills do NOT get an ID prefix.
</details>

<details>
<summary><strong>Procedure: Edit an existing skill</strong></summary>

1. Find the skill file: `find ~/.tiy/skills -name SKILL.md` or `find <workspace>/.tiy/skills -name SKILL.md`.
2. `read` the skill file to see current content.
3. `edit` or `write` to update the content.
4. Changes take effect on next skill load / rescan.
</details>

---

### 7.4 Marketplace Sources

**Global:** `~/.tiy/marketplaces.json`

**Schema:**

```json
{
  "sources": [
    {
      "id": "source-id",
      "name": "Source Name",
      "url": "https://marketplace.example.com",
      "kind": "plugin",
      "lastSyncedAt": "2026-04-09T10:30:00Z",
      "lastError": null
    }
  ]
}
```

<details>
<summary><strong>Procedure: Add a marketplace source</strong></summary>

1. `read ~/.tiy/marketplaces.json` (if it doesn't exist, start with `{"sources": []}`).
2. Append a new source entry.
3. `write ~/.tiy/marketplaces.json`.
4. Tell the user to refresh in Extensions Center.
</details>

---

### 7.5 Config Files Reference Table

| Config File | Path | Agent Can Write | What It Controls |
|-------------|------|:---------------:|-----------------|
| MCP (global) | `~/.tiy/mcp.json` | ✅ | Global MCP server configurations |
| MCP (workspace) | `<workspace>/.tiy/mcp.json` | ✅ | Workspace-scoped MCP servers (overrides global by ID) |
| Skills state (global) | `~/.tiy/skills.json` | ✅ | Enable/disable/pin state for skills |
| Skills state (workspace) | `<workspace>/.tiy/skills.json` | ✅ | Workspace-scoped skill state (overrides global) |
| Skill files (global) | `~/.tiy/skills/<name>/SKILL.md` | ✅ | Custom skill definitions (subdirectory + SKILL.md) |
| Skill files (global alt) | `~/.agents/skills/<name>/SKILL.md` | ✅ | Alternative global skill location |
| Skill files (workspace) | `<ws>/.tiy/skills/<name>/SKILL.md` | ✅ | Workspace-scoped custom skills |
| Marketplace sources | `~/.tiy/marketplaces.json` | ✅ | Marketplace registry sources |
| Plugin manifests | `~/.tiy/plugins/{id}/plugin.json` | ⚠️ Read only recommended | Plugin metadata (auto-managed, don't modify) |
| Database | `~/.tiy/db/tiy-agent.db` | ❌ Do not modify | SQLite database (runtime state, threads, settings) |
| Catalog cache | `~/.tiy/catalog/` | ❌ Do not modify | Auto-managed marketplace cache |

> **⚠️ Important:** After modifying any config file, tell the user to **refresh in the Extensions Center** or **restart TiyCode**. Config files are loaded on-demand, not watched for changes.

---

## 8. Logs & Diagnostics

TiyCode's native backend logs are produced by the Rust/Tauri process through `tracing`. These file logs are **platform-native operational logs**, not files inside `~/.tiy/`.

### 8.1 Log Directories

| Platform | Directory | Notes |
|----------|-----------|-------|
| macOS | `~/Library/Logs/TiyAgents/` | Created automatically on startup if missing |
| Windows | `%LOCALAPPDATA%/TiyAgents/logs/` | Uses the current user's Local AppData directory |
| Linux / other Unix | `$XDG_STATE_HOME/tiy-agents/logs/` | Preferred state directory |
| Linux fallback | `~/.local/state/tiy-agents/logs/` | Used when `XDG_STATE_HOME` is not set |

### 8.2 File Naming, Format, and Retention

| Rule | Behavior |
|------|----------|
| File prefix | `tiycode` |
| File suffix | `log` |
| Format | Structured JSON logs |
| Default level | `info` (`sqlx=warn`) unless overridden by environment |
| Rotation | Daily |
| Retention | Keep at most **5** log files |
| Cleanup | Older rotated files beyond the 5-file limit are removed automatically by the rolling appender |

### 8.3 What Gets Recorded in Logs

The file logs are primarily for operational diagnostics. They commonly include:

- App startup and readiness events such as agent startup, database readiness, and migration completion.
- Workspace, thread, and run lifecycle events such as workspace added/removed, thread created/deleted, run cancellation, and context compression activity.
- Tool execution metadata such as `tool_call_id`, tool name, and success/failure status.
- Desktop/runtime warnings such as tray visibility issues, launch-at-login sync failures, sleep-prevention failures, and terminal session recovery or shutdown failures.
- Provider/catalog/extension warnings such as catalog refresh/load failures, plugin command parse failures, and skill file read/parse failures.
- Error objects and diagnostic metadata such as local file paths, workspace/thread/run IDs, provider/model IDs, counts, setting keys, and error messages.

The current Rust tracing is mostly **metadata-oriented**, not intended as a full conversation transcript dump. Even so, users should review logs before sharing them externally because they may contain local paths, IDs, tool names, and operational error details.

### 8.4 How the Agent Should Use This Information

- When a user asks where logs are stored, answer with the platform-specific directory above rather than pointing them to `~/.tiy/`.
- When troubleshooting startup, provider catalog, terminal, tray, or extension loading issues, ask the user for the newest `tiycode*.log` file from the platform-native log directory.
- If a user asks whether old logs are cleaned automatically, explain the daily rotation + 5-file retention rule.

---

## 9. Workspace & Git

### Workspaces

TiyCode is workspace-centered. Each workspace points to a local directory. Workspaces are managed in Settings Center > Workspace.

### Git Operations

The agent can perform Git operations through its `shell` tool:

```bash
git status                    # Check status
git add <file>                # Stage files
git commit -m "message"       # Commit
git push / git pull           # Sync with remote
git diff --cached             # View staged changes
git log --oneline -10         # Recent history
```

TiyCode also has a built-in **Git drawer** in the UI that shows current branch, staged/unstaged files, commit history, and diff viewer.

The `/commit` command triggers AI-powered commit message generation with structured arguments (e.g. `/commit --style=full --language=chinese`).

### Slash Command Argument System

Slash commands support structured arguments that are parsed and injected into prompt templates:

**Argument formats:**
- `--key=value` — named flag with value (e.g. `--style=full`)
- `--key value` — named flag with separate value token
- `--flag` — boolean flag (resolves to `"true"`)
- Positional — tokens without `--` prefix, mapped to declared names by order

**Template placeholders:**
- `{{key}}` — replaced with the named argument value (e.g. `{{style}}` → `full`)
- `{{arguments}}` — replaced with the full raw argument string
- `{{0}}`, `{{1}}` — replaced with positional arguments by index
- `{{command}}` — replaced with the command name

**Argument declaration:** The `argumentHint` field in command settings (e.g. `[--style=simple|full] [--language=english|chinese]`) declares available parameters. Names extracted from the hint also enable positional-to-named mapping — e.g. with hint `[pr] [branch]`, input `123 main` maps to `{{pr}}=123`, `{{branch}}=main`.

**Fallback:** If no matching `{{placeholder}}` exists in the template, arguments are appended as `Command arguments: <raw text>` for backward compatibility.

**Storage:** Commands are stored as Markdown files with YAML frontmatter under `~/.tiy/prompts/builtin/` and `~/.tiy/prompts/user/`.

---

## 10. Terminal

TiyCode includes a built-in Terminal panel.

### Agent's Terminal Tools

| Tool | What it does |
|------|-------------|
| `shell` | Execute a one-off non-interactive command and get the output |
| `term_write` | Send input to the Terminal panel (for interactive/long-running processes) |
| `term_status` | Check if the Terminal panel is open and its state |
| `term_output` | Read recent output from the Terminal panel |
| `term_restart` | Restart the Terminal panel |
| `term_close` | Close the Terminal panel |

**`shell` vs `term_write`:**
- Use `shell` for one-off commands where you need the output (e.g. `git status`, `npm test`, `ls`).
- Use `term_write` for interactive sessions or long-running processes visible in the Terminal panel (e.g. starting a dev server, running a REPL).

### Terminal Settings (user configurable in Settings Center)

| Setting | Default | Options |
|---------|---------|---------|
| Shell path | System default | Any shell executable |
| Font family | SFMono / JetBrains Mono | Any installed font |
| Cursor style | `block` | `block` · `underline` · `bar` |
| Cursor blink | true | true / false |
| Scrollback | 5000 | Lines to retain |
| Copy on select | false | true / false |

---

## 11. Theme & Language

### Theme

| Option | Behavior |
|--------|----------|
| `system` | Follow OS light/dark preference |
| `light` | Always light mode |
| `dark` | Always dark mode |

**How to switch:** Top bar theme toggle, or Settings Center.

### Language

| Option | Display |
|--------|---------|
| `en` | English |
| `zh-CN` | Simplified Chinese (default) |

**How to switch:** Settings Center > General.

> The agent cannot change theme or language programmatically — these are stored in the UI layer (localStorage), not in files the agent can access. Guide the user to the UI controls.

---

## 12. Common User Scenarios

### "Command not found" in terminal

**Agent should:**
1. Explain this is a shell environment issue, not a missing installation.
2. Check which shell: use `shell` tool to run `echo $SHELL`.
3. Guide them to fix shell startup files (see Section 3).
4. Remind them to fully quit and relaunch TiyCode.
5. Verify with `shell` tool: `echo $PATH` and `which <tool>`.

### "Add an MCP server for X"

**Agent should:**
1. Ask for: server name, transport type (stdio or http), command/URL, env vars.
2. Read `~/.tiy/mcp.json`, add the server entry, write it back (see Section 7.1).
3. Tell the user to refresh in Extensions Center or restart TiyCode.

**Example — user says "Add a GitHub MCP server":**
1. Ask for their GitHub personal access token.
2. Write to `~/.tiy/mcp.json`:
```json
{
  "servers": [
    {
      "id": "github-mcp",
      "label": "GitHub",
      "transport": "stdio",
      "enabled": true,
      "autoStart": true,
      "command": "npx",
      "args": ["-y", "@modelcontextprotocol/server-github"],
      "env": { "GITHUB_PERSONAL_ACCESS_TOKEN": "<token>" }
    }
  ]
}
```
3. Tell user to restart TiyCode or refresh Extensions Center.

### "Disable the MCP server called X"

**Agent should:**
1. Read `~/.tiy/mcp.json`.
2. Find the server by label or id.
3. Set `"enabled": false`.
4. Write it back.
5. Tell user to refresh or restart.

### "Enable/disable the skill called X"

**Agent should:**
1. Read `~/.tiy/skills.json`.
2. Add the skill ID to `enabled` or `disabled` array.
3. Write it back.
4. Tell user to rescan skills or restart.

### "Create a skill for X"

**Agent should:**
1. Create a subdirectory: `mkdir -p <workspace>/.tiy/skills/<skill-name>` (workspace-scoped) or `~/.tiy/skills/<skill-name>` (global).
2. Write a `SKILL.md` file (uppercase) inside the subdirectory with YAML frontmatter + markdown body. See Section 7.3 for format.
3. Tell user to rescan skills in Extensions Center.

### "How do I add a new AI provider?"

**Agent should:**
1. Guide to Settings Center > Providers (cannot be done via config files — API keys are stored in the database).
2. For builtin providers: Click the provider, enter API key, enable it.
3. For custom endpoints: Add Custom Provider → choose type → enter base URL and API key.

### "Switch to dark mode" / "Change language"

**Agent should:**
Guide to the UI controls. Cannot be done programmatically.

### "What model am I using?"

**Agent should:**
Guide the user to check the active profile in the top bar or Settings Center > Agent Profiles.

### "My MCP server is not working"

**Agent should:**
1. Read `~/.tiy/mcp.json` to check the server config.
2. Common issues:
   - **Command not found:** Shell environment issue (see Section 3). Check if `command` path is absolute or available in PATH.
   - **Missing env var:** Check the `env` field in the config.
   - **Wrong transport:** Verify `transport` matches the server type.
   - **Timeout:** Increase `timeoutMs`.
3. After fixing the config, write it back and tell user to restart the server in Extensions Center.

### "List all my MCP servers"

**Agent should:**
1. `read ~/.tiy/mcp.json` to list global servers.
2. `read <workspace>/.tiy/mcp.json` to list workspace-scoped servers (if exists).
3. Report server names, transport types, enabled status.

### "What skills are available?"

**Agent should:**
1. `read ~/.tiy/skills.json` for enable/disable state.
2. `find ~/.tiy/skills/*.md` and `find <workspace>/.tiy/skills/*.md` to discover skill files.
3. `read` each skill file's frontmatter to report names and descriptions.

### "Generate a commit message"

**Agent should:**
1. Use the `/commit` slash command, or
2. Use `shell` to run `git diff --cached`, then compose a conventional commit message.

### "Show me what's changed in Git"

**Agent should:**
Use `shell` to run `git status`, `git diff`, `git log --oneline -10`, etc.

---

## 13. Capability Summary

### What the Agent CAN Do Directly

| Capability | How | Example |
|-----------|-----|---------|
| Read/write/edit files | `read`, `write`, `edit` tools | Source code, config files, skill files |
| Search code | `search`, `find` tools | Regex search, glob patterns |
| Run shell commands | `shell` tool | `git status`, `npm test`, `cargo build` |
| Git operations | `shell` tool | stage, commit, push, pull, diff, log |
| Interact with Terminal | `term_write`, `term_status`, `term_output` | Dev servers, REPLs, long-running processes |
| **Manage MCP servers** | Read/write `~/.tiy/mcp.json` | Add, remove, enable, disable, reconfigure servers |
| **Enable/disable skills** | Read/write `~/.tiy/skills.json` | Toggle skill state |
| **Create custom skills** | Write `SKILL.md` in `~/.tiy/skills/<name>/` | Create new agent capabilities as subdirectories |
| **Manage marketplace sources** | Read/write `~/.tiy/marketplaces.json` | Add registry sources |
| Ask user questions | `clarify` tool | Get preferences, confirm actions |
| Propose plans | `update_plan` tool | Present implementation strategy |
| Delegate to helpers | `agent_explore`, `agent_review` | Code investigation, verification |
| Track tasks | `create_task`, `update_task`, `query_task` | Progress tracking |
| Use plugin/MCP tools | (varies) | Tools from enabled plugins and MCP servers |

### What Requires User Action (via TiyCode UI)

| Action | Where in UI | Why |
|--------|------------|-----|
| Switch theme (light/dark/system) | Top bar theme toggle | Stored in UI layer (localStorage) |
| Switch language (en / zh-CN) | Settings Center > General | Stored in UI layer (localStorage) |
| Add/configure LLM providers | Settings Center > Providers | API keys stored in encrypted database |
| Manage API keys | Settings Center > Providers | Security: encrypted storage |
| Create/edit/switch Agent Profiles | Settings Center > Agent Profiles | Stored in database |
| Change response style / thinking level | Settings Center > Agent Profiles | Stored in database |
| Install/uninstall plugins | Extensions Center > Plugins | Requires runtime plugin loading |
| Enable/disable plugins | Extensions Center > Plugins | Requires runtime state change |
| Configure terminal settings | Settings Center > Terminal | Stored in database |
| Manage workspaces | Settings Center > Workspace | Stored in database |
| Set approval policy | Settings Center > Policy | Stored in database |
| **Restart MCP server** | Extensions Center > MCP | Requires runtime process management |
| **Trigger skill rescan** | Extensions Center > Skills | Triggers re-indexing of skill files |

> **Key insight:** For MCP/Skills/Marketplace, the agent can modify the **config files** (add/remove/enable/disable), but the user needs to trigger a **reload** in the UI for changes to take effect. The agent should always tell the user this after modifying a config file.
