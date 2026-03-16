# Agent Tools Design

## Summary

This document centralizes the built-in tool surface supported by Tiy Agent.

Before this document, tool design was spread across:

- the high-level architecture document
- `Tool Gateway + Policy`
- subsystem-specific documents such as Terminal, Git, and Index

This document provides one place to answer:

1. which tools are built into the core product
2. which subsystem owns each tool family
3. how those tools behave under `default` and `plan` run modes

## Goals

- define a centralized built-in tool catalog
- separate internal agent tools from privileged system tools
- map tool families to owning subsystems
- define recommended v1 `Plan` mode restrictions
- keep extension tools distinct from built-in core tools

## Non-Goals

- no exhaustive listing of third-party extension tools
- no promise that every tool listed here is already fully implemented
- no replacement for subsystem-level execution details

## Core Principle

Built-in tools follow one architectural rule:

- tools are described in TypeScript
- privileged execution happens in Rust

Internal tools that do not cross system boundaries may remain in the sidecar.

## Tool Categories

### Internal Agent Tools

Owned primarily by the sidecar.

Recommended v1 tools:

- `summarize_context`
- `rewrite_plan`
- `rank_candidates`
- `format_final_response`

Characteristics:

- no direct privileged system access
- used for reasoning, planning, ranking, and output shaping

### Workspace and File Tools

Owned by:

- `WorkspaceManager`
- filesystem executors through `ToolGateway`

Recommended v1 tools:

- `read_file`
- `list_dir`
- `search_repo`
- `write_file`
- `apply_patch`
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

- `terminal_get_status`
- `terminal_get_recent_output`
- `terminal_write_input`
- `terminal_restart`

Do not expose:

- direct PTY ownership
- unrestricted raw shell streaming into the sidecar

### Marketplace and Extension Tools

Owned by:

- `MarketplaceHost`
- extension executors through `ToolGateway`

Recommended v1 tools:

- `marketplace_install`
- `mcp_call`

Important rule:

- extension-provided tools are not automatically built-in core tools
- extension tools still go through the same gateway and policy path

## Ownership Matrix

| Tool Family | TS Sidecar | Rust Owner | Notes |
|---|---|---|---|
| Internal agent tools | define + execute | none | no privileged boundary crossing |
| Workspace/file tools | define only | workspace/filesystem executors | path and sandbox checks required |
| Git tools | define only | `GitManager` | remote actions also depend on network policy |
| Terminal tools | define only | `TerminalManager` | thread-scoped and policy-gated |
| Marketplace/MCP tools | define only | `MarketplaceHost` or MCP executor | extension-safe path required |

## `Plan` Mode Tool Matrix

`Plan` mode is a real run mode, not just a UI rendering choice.

Recommended v1 behavior:

| Tool Category | `default` Mode | `plan` Mode |
|---|---|---|
| Internal agent tools | allowed | allowed |
| Read-only file/search tools | allowed by normal policy | allowed by normal policy |
| Read-only Git tools | allowed by normal policy | allowed by normal policy |
| Mutating file tools | allowed or approval-gated by policy | denied or explicitly escalated |
| Mutating Git tools | allowed or approval-gated by policy | denied or explicitly escalated |
| Terminal write/restart tools | allowed or approval-gated by policy | denied or explicitly escalated |
| Marketplace/runtime mutation tools | policy-gated | denied or explicitly escalated |

Key idea:

- `Plan` mode may inspect and reason
- `Plan` mode should not silently mutate local state

After the user explicitly launches a new `default` execution run from the plan, the tool matrix falls back to normal execution rules. The difference is how the execution context is built:

- `ContinueInThread`: full current-thread context
- `CleanContextFromPlan`: reduced plan-centric execution context

## Relationship to Other Documents

- high-level architecture: [technical-architecture-20260316.md](/Users/jorbenzhu/Documents/Workplace/TiyAgents/tiy-desktop/docs/technical-architecture-20260316.md)
- gateway and approval model: [tool-gateway-policy-design-20260316.md](/Users/jorbenzhu/Documents/Workplace/TiyAgents/tiy-desktop/docs/module/tool-gateway-policy-design-20260316.md)
- sidecar tool description layer: [agent-sidecar-design-20260316.md](/Users/jorbenzhu/Documents/Workplace/TiyAgents/tiy-desktop/docs/module/agent-sidecar-design-20260316.md)
- terminal-specific tools: [terminal-design-20260316.md](/Users/jorbenzhu/Documents/Workplace/TiyAgents/tiy-desktop/docs/module/terminal-design-20260316.md)
- Git-specific behavior: [git-design-20260316.md](/Users/jorbenzhu/Documents/Workplace/TiyAgents/tiy-desktop/docs/module/git-design-20260316.md)
- search and retrieval backing: [index-design-20260316.md](/Users/jorbenzhu/Documents/Workplace/TiyAgents/tiy-desktop/docs/module/index-design-20260316.md)

## ADR

### ADR-AT1: Core product tools need a centralized catalog

#### Status

Accepted

#### Context

Tool behavior naturally lives close to each owning subsystem, but implementation planning also needs a centralized view of the core built-in tool surface and its mode-specific constraints.

#### Decision

Keep execution details in subsystem documents, and use this document as the centralized built-in tool catalog and run-mode matrix for the core product.

#### Consequences

##### Positive

- easier implementation planning
- clearer distinction between built-in and extension tools
- clearer `Plan` mode expectations

##### Negative

- cross-document consistency now matters more

## Implementation Notes

- use this document as the source when defining `tool-registry.ts`
- keep executor behavior in the owning subsystem documents
- extend the matrix when new built-in tool families are added
