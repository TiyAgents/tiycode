# Thread Task Tracking Design

Date: 2026-03-23
Status: Draft for review
Scope: Add Codex-like implementation task tracking inside the current thread surface without reusing planning checkpoints for execution progress.

## Summary

This design introduces a thread-scoped task tracking system for the implementation phase of an agent run. It keeps planning and execution separate:

- `update_plan` remains the formal planning tool used before implementation begins.
- `create_task` creates the execution task board once work starts.
- `update_task` updates task progress as implementation advances.

The user-facing experience lives inside the current thread surface and shows a compact progress card such as `1 out of 4 tasks completed`, followed by per-task status rows. There is no approval step in this runtime flow. The task board is persisted at the thread level so it can survive run completion, app restarts, and later continuation in the same thread.

## Goals

- Show Codex-like implementation progress directly inside the active thread.
- Separate implementation tracking from pre-implementation planning.
- Support multiple task items with explicit statuses:
  - `pending`
  - `in_progress`
  - `completed`
  - `blocked`
- Persist task boards on the thread so progress can continue across runs.
- Default to a single active board per thread, while allowing explicit phase changes when the agent starts a new stage.
- Keep the front-end state model simple by sending complete task board snapshots instead of partial diffs.

## Non-Goals

- Replacing or extending `update_plan` to also represent implementation state.
- Adding a workspace-level task center.
- Inferring completion primarily from tools, file edits, or tests.
- Introducing a new approval flow for implementation tracking.
- Building generalized project management features outside the thread UI.

## User Experience

### Active Task Board

The active task board is rendered inside the current thread surface, above the composer and separate from ordinary chat messages. It is always treated as the current execution state rather than as a normal assistant message.

The card includes:

- Progress header, for example `1 out of 4 tasks completed`
- Optional title and summary for the current stage
- Ordered task list with visible status markers
- Optional footer metadata such as files changed, verification summary, or last updated time

### Task Item Presentation

Each task row shows one of four states:

- `pending`: default inactive state
- `in_progress`: current active step, visually emphasized
- `completed`: checked and struck through
- `blocked`: warning treatment with optional reason text

If the task list is long, the UI may collapse lower-priority rows after the first 6-8 items.

### History Behavior

The current active board is fixed in the thread surface.

If the agent explicitly starts a new stage, the previous board moves into the thread timeline as a historical stage card and the new stage becomes the active board. This preserves phase history without cluttering the active runtime surface.

## Core Product Rules

### Planning vs Execution

- `update_plan` is only for pre-implementation planning.
- `create_task` and `update_task` are only for implementation tracking.
- Agent prompts and runtime behavior should explicitly discourage using `update_plan` as a progress mechanism after implementation has started.

### Thread-Scoped Lifetime

Task boards belong to the thread, not only to a single run.

This means:

- the board remains visible after a run finishes
- a later run in the same thread can continue updating the same board
- the thread snapshot must include task board state

### Stage Handling

The default behavior is to update the current active board.

If the agent explicitly indicates a new stage, the system creates a new board and marks the old one as historical. This keeps the common path simple while preserving meaningful milestones in long-running threads.

## Architecture

The system is split into four layers:

1. Agent tool layer
2. Rust domain and persistence layer
3. Thread event and snapshot layer
4. Front-end rendering layer

### 1. Agent Tool Layer

Two new tools are introduced:

#### `create_task`

Creates the active task board for implementation tracking.

Recommended input shape:

```json
{
  "title": "Fix thread history pagination and loading flow",
  "summary": "Patch backend snapshot loading, then add frontend load-older support and verify.",
  "stageAction": "replace_current",
  "tasks": [
    { "id": "inspect", "title": "Inspect thread snapshot pagination", "status": "completed" },
    { "id": "patch-backend", "title": "Patch backend snapshot loading", "status": "in_progress" },
    { "id": "patch-frontend", "title": "Add load older messages support", "status": "pending" },
    { "id": "verify", "title": "Run targeted verification", "status": "pending" }
  ]
}
```

Supported `stageAction` values:

- `replace_current`
- `start_new_stage`

Default behavior should be `replace_current`.

#### `update_task`

Updates the active board and one or more task items as implementation progresses.

Recommended input shape:

```json
{
  "boardId": "tb_123",
  "summary": "Backend pagination is fixed; wiring frontend history loading now.",
  "tasks": [
    { "id": "patch-backend", "status": "completed" },
    { "id": "patch-frontend", "status": "in_progress" }
  ],
  "metadata": {
    "filesChanged": 2,
    "linesAdded": 27,
    "linesRemoved": 1
  }
}
```

`update_task` should be incremental, but the back end should always return and broadcast a fully materialized board snapshot after applying the update.

The runtime should expose an `advance_step` path for the common case:

- complete the current `in_progress` step
- auto-start the next pending step
- auto-complete the board when no pending steps remain

### 2. Rust Domain and Persistence Layer

#### Data Model

Add a dedicated thread-scoped task tracking model.

##### `task_boards`

One record per execution stage card.

Recommended fields:

- `id`
- `thread_id`
- `source_run_id`
- `title`
- `summary`
- `status`
- `stage_index`
- `is_active`
- `created_at`
- `updated_at`
- `completed_at`
- `metadata_json`

Recommended board statuses:

- `active`
- `completed`
- `archived`

##### `task_items`

One record per task row inside a board.

Recommended fields:

- `id`
- `board_id`
- `thread_id`
- `title`
- `description`
- `status`
- `sort_index`
- `started_at`
- `completed_at`
- `updated_at`
- `metadata_json`

Recommended item statuses:

- `pending`
- `in_progress`
- `completed`
- `blocked`

#### Manager Responsibilities

Add a dedicated manager, for example `task_board_manager`, responsible for:

- creating or replacing the active board
- starting a new stage
- updating task items
- auto-starting the next step when progress advances
- marking boards completed when all items complete
- loading all boards for a thread
- producing a DTO snapshot for the front end

This should stay separate from the existing plan checkpoint logic.

#### Repository Responsibilities

Add repos for:

- `task_board_repo`
- `task_item_repo`

The repos should support:

- create board
- archive or complete current board
- find active board by thread
- list boards by thread ordered by stage and update time
- replace task items for a board
- update selected task items by stable task id

### 3. Thread Event and Snapshot Layer

#### Front-End Thread Events

The front end should receive complete board snapshots rather than partial task patches.

Primary event:

- `task_board_updated`

Payload should include the entire normalized board DTO, including derived completion counts.

This event should fire after:

- `create_task`
- `update_task`
- automatic board completion
- explicit stage transitions

#### Thread Snapshot

Extend `ThreadSnapshotDto` with:

- `taskBoards: TaskBoardDto[]`
- `activeTaskBoardId: string | null`

This ensures thread reload, app restart, and run recovery all restore task tracking state together with messages, tools, and helper activity.

### 4. Front-End Rendering Layer

#### Surface Integration

Update `runtime-thread-surface.tsx` to render:

- current active task board in the runtime surface
- historical task stage cards in the timeline when relevant

Suggested component split:

- `src/modules/workbench-shell/model/task-board.ts`
- `src/modules/workbench-shell/ui/task-board-card.tsx`
- `src/modules/workbench-shell/ui/task-stage-history-card.tsx`

This keeps formatting and state interpretation out of the already-large thread surface component.

#### Relationship to Existing `Queue`

The current `queue_updated` flow should not become the user-facing implementation tracker.

Recommended direction:

- keep `queue_updated` for runtime internals or debugging
- treat the new task board as the main user-facing execution tracker
- optionally reduce or hide the raw queue JSON display later

## Status and Transition Rules

### Board Lifecycle

`create_task` with `replace_current`:

- if no active board exists, create one
- if an active board exists, update that board in place

`create_task` with `start_new_stage`:

- mark the current active board completed or archived
- create a new board with `stage_index + 1`
- set the new board as active

`update_task`:

- only updates the active board
- may update board title, summary, metadata, and selected task items

Automatic board completion:

- if all task items are `completed`, the back end may mark the board `completed`

### Error Handling

Return recoverable errors for:

- missing `boardId`
- task item ids that do not exist on the active board
- thread and run mismatches

Do not silently create missing task ids during `update_task`. Failing fast is safer than drifting task state because of an agent typo.

## API and DTO Design

Recommended DTOs:

### `TaskItemDto`

- `id`
- `boardId`
- `title`
- `description`
- `status`
- `sortIndex`
- `startedAt`
- `completedAt`
- `updatedAt`
- `metadata`

### `TaskBoardDto`

- `id`
- `threadId`
- `sourceRunId`
- `title`
- `summary`
- `status`
- `stageIndex`
- `isActive`
- `completedCount`
- `totalCount`
- `metadata`
- `items`
- `createdAt`
- `updatedAt`
- `completedAt`

The server should compute `completedCount` and `totalCount` so the UI can render the progress header without extra client-side derivation.

## Agent Prompting Contract

The implementation-focused runtime instructions should add explicit guidance:

- once implementation begins and the work is multi-step, first call `create_task`
- use `update_task` whenever a key step starts, completes, or becomes blocked
- do not use `update_plan` to express implementation progress
- use `start_new_stage` only when beginning a clearly separate implementation phase

This contract is required to make the task board a dependable source of truth instead of an optional decoration.

## Testing Strategy

### Rust

Add tests for:

- creating the first active board on a thread
- replacing the current board
- starting a new stage
- updating task item statuses
- auto-completing a board when all items complete
- loading task boards through thread snapshot recovery

### Front End

Add tests or focused state validation for:

- active board rendering
- completion count display
- task item status presentation
- historical stage rendering
- thread switching and snapshot restore behavior

### Manual Verification

Validate:

- the thread shows `X out of Y tasks completed`
- `in_progress` moves as tasks advance
- starting a new stage moves the old board into history
- task boards persist after run completion
- task boards restore after thread reload or app restart

## Risks

- Mixing task tracking with existing queue rendering could create duplicate progress UIs if both are shown prominently.
- If task ids are not stable across updates, task history will jitter and front-end rendering will become unreliable.
- If board updates are only emitted as partial patches, recovery and remount logic will become unnecessarily complex.
- If the active board is rendered as a normal message, long threads will bury the current execution state.

## Recommendation

Implement a dedicated execution tracking system with `create_task` and `update_task`, backed by thread-scoped `task_boards` and `task_items`, and render the active board as a persistent runtime card inside the current thread surface.

This is the cleanest way to achieve Codex-like implementation tracking while preserving the existing semantics of `update_plan`.
