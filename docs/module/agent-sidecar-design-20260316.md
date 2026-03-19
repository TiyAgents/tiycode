# Agent Sidecar Design (Deprecated)

## Status

Deprecated as of 2026-03-19.

Tiy Agent no longer adopts the `TS Agent Sidecar` scheme described in the
original version of this document. The new direction is:

- no standalone TypeScript sidecar process
- no `pi-agent` runtime as the desktop execution core
- `tiy-core` becomes the built-in single-agent kernel
- Rust owns the desktop orchestration layer through
  `BuiltInAgentRuntime`, `AgentSession`, and `HelperAgentOrchestrator`

## Reason For Deprecation

The old sidecar design assumed that the decision layer should live in a
long-lived child process that multiplexes runs, performs model routing, and
orchestrates helper tasks. After reviewing the current `tiy-core` capability
surface and comparing it with the `pi-mono` coding-agent architecture, that
split is no longer the best fit for Tiy Agent:

- `tiy-core` already provides a stateful agent loop, streaming events, tool
  execution hooks, context transforms, steering/follow-up queues, and
  security/resource limits
- helper-agent behavior can be modeled as a Rust-owned orchestration concern
  instead of a sidecar-native runtime concern
- run lifecycle, policy, tool execution, helper coordination, and persistence
  become simpler and more auditable when they stay inside the Rust main process

## Replacement Documents

Use these documents instead:

- `docs/superpowers/specs/2026-03-19-built-in-agent-runtime-tiy-core-design.md`
- `docs/module/agent-run-design-20260316.md`
- `docs/module/agent-tools-design-20260316.md`
- `docs/technical-architecture-20260316.md`
- `docs/implementation-plan-20260316.md`

## Migration Guidance

Any earlier reference to:

- `SidecarManager`
- `sidecar_protocol`
- `agent-sidecar/`
- sidecar health metrics
- sidecar-owned `SubAgent` orchestration

should now be interpreted as design debt to be removed or replaced by the
built-in runtime model.
