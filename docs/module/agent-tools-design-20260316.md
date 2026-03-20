# Agent Tools Design

## Summary

This document defines the built-in tool surface exposed to the Tiy Agent
runtime after removing the sidecar architecture.

In the new model:

- tools are registered by Rust runtime
- `tiy-core` receives those tools as part of the active `AgentSession`
- privileged execution remains in Rust through `ToolGateway`
- helper delegation tools are runtime-owned orchestration tools, not sidecar-only
  internals

## Goals

- define one centralized built-in tool catalog
- separate runtime orchestration tools from privileged system tools
- map every tool family to its Rust owner
- define `default` and `plan` mode tool behavior
- ensure helper-agent orchestration cannot bypass policy

## Non-Goals

- no exhaustive listing of third-party extension tools
- no promise that every listed tool is already implemented
- no direct embedding of CLI/TUI-only extension mechanisms from `pi-mono`

## Core Principles

### Tools Are A Rust Runtime Concern

The active tool set seen by `tiy-core` is prepared by `AgentSession`, not by a
TypeScript sidecar.

### Privileged Execution Stays Behind `ToolGateway`

Any tool that touches the local system must still pass through:

- `ToolGateway`
- `PolicyEngine`
- subsystem executors
- audit persistence

### Internal Orchestration Tools Are First-Class

Some tools do not cross privileged system boundaries but still matter to product
behavior. These should be modeled explicitly as runtime orchestration tools
instead of being hidden inside an implementation detail.

## Tool Categories

### Runtime Orchestration Tools

Owned by:

- `AgentSession`
- `HelperAgentOrchestrator`

Recommended v1 tools:

- `agent_research`
- `agent_review`
- `format_final_response`
- `summarize_helper_result`

Characteristics:

- no direct privileged system execution
- may create helper tasks or reshape runtime context
- results are folded back into the parent run

### Workspace And File Tools

Owned by:

- `WorkspaceManager`
- filesystem executors through `ToolGateway`

Recommended v1 tools:

- `read`
- `list`
- `search`
- `write`
- `patch`
- `open_workspace_in_app`

### Git Tools

Owned by:

- `GitManager`
- executed through `ToolGateway`

Recommended v1 tools:

- `git_status`
- `git_diff`
- `git_log`
- `git_stage`
- `git_unstage`
- `git_commit`
- `git_fetch`
- `git_pull`
- `git_push`

### Terminal Tools

Owned by:

- `TerminalManager`
- executed through `ToolGateway`

Recommended v1 tools:

- `term_status`
- `term_output`
- `term_write`
- `term_restart`

Important constraint:

- raw PTY ownership remains in Rust
- helper-agent and parent agent use the same terminal permission boundary

### Marketplace And Extension Tools

Owned by:

- `MarketplaceHost`
- MCP / extension executors through `ToolGateway`

Recommended v1 tools:

- `market_install`
- `mcp_call`

## Ownership Matrix

| Tool Family | Runtime Registration | Rust Executor | Notes |
|---|---|---|---|
| Runtime orchestration | `AgentSession` | runtime internal | helper delegation and result folding |
| Workspace/file tools | `AgentSession` | filesystem executors | path and sandbox checks required |
| Git tools | `AgentSession` | `GitManager` | remote actions still depend on policy |
| Terminal tools | `AgentSession` | `TerminalManager` | thread-scoped and policy-gated |
| Marketplace/MCP tools | `AgentSession` | `MarketplaceHost` or MCP executor | extension-safe boundary required |

## `Plan` Mode Tool Matrix

`plan` mode is enforced by runtime tool selection plus policy ceilings.

| Tool Category | `default` Mode | `plan` Mode |
|---|---|---|
| Runtime orchestration tools | allowed | allowed if helper profile is read-only |
| Read-only file/search tools | allowed by normal policy | allowed by normal policy |
| Read-only Git tools | allowed by normal policy | allowed by normal policy |
| Read-only terminal inspection | allowed by normal policy | allowed by normal policy with command allowlist |
| Mutating file tools | allowed or approval-gated | denied or explicitly escalated |
| Mutating Git tools | allowed or approval-gated | denied or explicitly escalated |
| Terminal write/restart tools | allowed or approval-gated | denied or explicitly escalated |
| Marketplace/runtime mutation tools | policy-gated | denied or explicitly escalated |

Key rule:

- helper tasks inherit the same `plan` mode ceiling as the parent run
- helper delegation must not create an escape hatch around read-only mode

## Runtime Tool Selection

`AgentSession` should select tools from named profiles, not by ad hoc lists
spread across the codebase.

Recommended profiles:

- `default_full`
- `plan_read_only`
- `helper_scout`
- `helper_planner`
- `helper_reviewer`

Each profile should define:

- visible tool names
- optional terminal command restrictions
- helper delegation allowance
- approval allowance
- run-mode compatibility
- max concurrent helper count
- policy hints for audit and telemetry

Recommended v1 helper-profile rules:

- `helper_scout` / `helper_planner` / `helper_reviewer` are all read-only
  profiles
- all three are allowed in `plan` mode only in their read-only form
- helpers do not open independent approval UI in v1
- if a helper reaches an approval-required or mutating tool path, runtime should
  fold back an escalation-needed result to the parent run instead of waiting for
  helper-local approval

## Relationship To Other Documents

- runtime and run lifecycle:
  `docs/module/agent-run-design-20260316.md`
- superseding overall design:
  `docs/superpowers/specs/2026-03-19-built-in-agent-runtime-tiy-core-design.md`
- technical architecture:
  `docs/technical-architecture-20260316.md`
- terminal-specific behavior:
  `docs/module/terminal-design-20260316.md`
- Git-specific behavior:
  `docs/module/git-design-20260316.md`

## ADR

### ADR-AT1: Built-In Tools Must Be Registered By Rust Runtime Profiles

#### Status

Accepted

#### Context

The old tool catalog assumed a sidecar-defined tool description layer. That no
longer matches the built-in runtime architecture.

#### Decision

Move tool registration responsibility into Rust `AgentSession` profiles, keep
privileged execution in `ToolGateway`, and model helper delegation as explicit
runtime orchestration tools.

#### Consequences

Positive:

- tool availability becomes auditable in one place
- `plan` mode enforcement becomes clearer
- helper orchestration is visible in the product model

Negative:

- runtime profile maintenance now lives fully in Rust
