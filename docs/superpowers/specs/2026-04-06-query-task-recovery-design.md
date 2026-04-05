# Query Task Recovery Design

Date: 2026-04-06
Status: Draft for review
Scope: Add a read-only `query_task` runtime tool so the model can recover current thread task-board context after interruptions, app restarts, or session rehydration gaps.

## Summary

The current task tracking system persists task boards at the thread level and includes them in thread snapshots, but the runtime tool surface only exposes `create_task` and `update_task`. When the app is interrupted or restarted during execution, the model can lose the in-memory `taskBoardId` and fail to continue with `update_task`, even though the task board still exists in persisted thread state.

This design adds a dedicated `query_task` tool that lets the model rehydrate task-board context for the current thread on demand. The tool defaults to returning the active task board and can optionally return all task boards for the thread. It is read-only, thread-scoped, and intentionally separate from `update_task` so recovery remains explicit and debuggable.

## Goals

- Let the model recover the current thread's task-board context after interruption or restart.
- Provide a minimal, explicit way to obtain the current `activeTaskBoardId`.
- Keep `create_task`, `update_task`, and `query_task` responsibilities distinct.
- Reuse existing `TaskBoardDto` shapes instead of introducing parallel task-board payloads.
- Make the recovery path easy to describe in runtime instructions and easy to verify in tests.

## Non-Goals

- Changing thread snapshot behavior or task-board persistence rules.
- Making `update_task` implicitly resolve missing task-board identifiers.
- Adding cross-thread task queries.
- Modifying frontend rendering for task boards.
- Reconciling or mutating task state as part of a query.

## User Problem

The failure mode is narrow but common:

1. A task board is created and work begins.
2. The app is interrupted, restarted, or resumes with incomplete conversational memory.
3. The model remembers that a task exists, but forgets the concrete `taskBoardId`.
4. A follow-up `update_task` call fails because the required identifier is missing or stale.

Because there is no explicit recovery tool, the model cannot reliably self-heal from this state even though the backend already stores the task board.

## Tool Contract

### Tool Name

`query_task`

### Purpose

Read the current thread's task-board state so the model can recover task context before issuing `update_task`.

### Input

```json
{
  "scope": "active" | "all"
}
```

Rules:

- `scope` is optional.
- Default `scope` is `active`.
- The tool never accepts a `threadId`; it always queries the current runtime thread.

### Output

```json
{
  "scope": "active",
  "activeTaskBoardId": "tb_123",
  "taskBoards": [
    { "...TaskBoardDto" }
  ]
}
```

Rules:

- `taskBoards` always uses the existing `TaskBoardDto` shape.
- When `scope = "active"` and an active board exists, `taskBoards` contains only that board.
- When `scope = "active"` and no active board exists, `activeTaskBoardId` is `null` and `taskBoards` is an empty array.
- When `scope = "all"`, `taskBoards` contains all thread task boards in the same order used by thread snapshots.
- `activeTaskBoardId` is included for both scopes so the model can branch on one field.

## Architecture

### Model Layer

Add two task-query types to `src-tauri/src/model/task_board.rs`:

- `QueryTaskInput`
- `QueryTaskResult`

`QueryTaskInput` should deserialize the optional `scope` field and default it to `active`.

`QueryTaskResult` should contain:

- `scope`
- `active_task_board_id`
- `task_boards`

The DTO should serialize to camelCase for consistency with the existing runtime tool payloads.

### Task Board Manager

Add a pure read helper in `src-tauri/src/core/task_board_manager.rs`, for example:

`query_thread_task_boards(pool, thread_id, scope) -> QueryTaskResult`

Behavior:

- For `active`, call the existing active-board loader and return either one board or none.
- For `all`, call the existing thread-level task-board loader and compute `activeTaskBoardId` from the active board if present.
- Do not reconcile state.
- Do not update timestamps.
- Do not mutate board or step status.

This keeps `query_task` free from side effects and makes it safe to call repeatedly during recovery.

### Agent Session Integration

Update `src-tauri/src/core/agent_session.rs` in two places:

1. Register `query_task` in the runtime tool list with a small schema containing the optional `scope` enum.
2. Extend the existing task-tool execution path so `create_task`, `update_task`, and `query_task` are all handled in the same persisted tool-call flow.

`query_task` should:

- Persist its tool call like other task tools.
- Return structured JSON in both `content` and `details`, matching current task-tool conventions.
- Mark the tool call as completed or failed through `tool_call_repo`.

`query_task` should not emit `ThreadStreamEvent::TaskBoardUpdated`, because it does not change task state.

## Recovery Strategy

The runtime instructions should explicitly establish `query_task` as the recovery path for task tracking.

Recommended guidance:

- If you need to continue an existing task board but do not know the `taskBoardId`, call `query_task` first.
- After an interruption, restart, or resumed thread where task context may be incomplete, call `query_task` with `scope = "active"` before attempting `update_task`.
- Use `scope = "all"` only when the active board is missing or when you need to inspect board history before deciding whether to continue or create a new board.

This makes recovery deterministic:

1. Query active task state.
2. Read `activeTaskBoardId`.
3. Resume progress with `update_task`.

## Testing

Add coverage at two layers.

### Task Board Manager Tests

Extend `src-tauri/tests/m2_3_task_tracking.rs` with tests for:

- `query_task(active)` returning the active board only.
- `query_task(active)` returning `null` plus an empty list when no active board exists.
- `query_task(all)` returning all boards and the correct `activeTaskBoardId`.

### Agent Session / Tool Registration Tests

Add or extend runtime-tool tests to verify:

- `query_task` is registered in the runtime tool list.
- The tool returns the agreed JSON structure.
- Tool-call persistence works for successful and failing query flows.
- No `TaskBoardUpdated` event is emitted for a read-only query.

## Rollout Notes

This is a low-risk additive change:

- Existing task-board storage remains unchanged.
- Existing `create_task` and `update_task` callers remain valid.
- Recovery becomes explicit instead of implicit, which improves debuggability when a resumed run behaves unexpectedly.

## Recommendation

Implement `query_task` as a dedicated read-only tool instead of overloading `update_task`. The extra tool is justified because it makes the recovery contract explicit, keeps mutation semantics clean, and directly addresses the restart/interruption failure mode without changing existing task-update behavior.
