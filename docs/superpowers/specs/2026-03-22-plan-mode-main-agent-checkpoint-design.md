# Plan Mode Main-Agent Checkpoint Design

## Summary

This design removes the current dedicated Plan helper flow and turns Plan mode
into a main-agent planning checkpoint that always pauses before implementation.

The new behavior has four core properties:

- the main agent owns implementation planning directly
- Plan mode remains read-only and never performs implementation
- publishing a plan always emits a persistent Plan artifact and then pauses in
  `waiting_approval`
- implementation begins only after the user explicitly approves one of two
  actions: direct execution or execution after context cleanup

This keeps planning visible and iterative while enforcing a hard approval gate
between planning and code changes.

## Goals

- remove the dedicated `Plan Agent` / `agent_plan` helper path
- make Plan mode mean "produce an implementation plan before execution"
- allow the main agent to enter planning when task complexity warrants it
- emit a structured `update_plan` artifact before any implementation starts
- pause the run after plan publication and require explicit approval to proceed
- support iterative plan review where the user can revise the plan without
  approving it
- support a context-clean implementation handoff built from a summary of
  pre-plan context plus the approved full plan

## Non-Goals

- this spec does not redesign the entire run-mode model beyond Plan mode
- this spec does not remove the existing read-only restrictions in Plan mode
- this spec does not add new approval options beyond the two approved actions
- this spec does not redesign unrelated helper flows such as explore or review
- this spec does not require a general-purpose workflow engine for all agent
  phases

## Current Problems

### Plan mode is coupled to a dedicated helper instead of the main agent

Today the runtime treats planning as a required `agent_plan` helper call. That
means the main agent cannot naturally own the planning step even though the user
experiences planning as part of the primary thread.

### Plan mode completion is enforced by helper success instead of a user-facing checkpoint

The current run can only finish in Plan mode after at least one successful
`agent_plan` call. This models planning as an internal implementation detail
instead of a product-level checkpoint that the user can inspect and approve.

### Plan publication and implementation approval are not first-class persistent artifacts

`PlanUpdated` currently behaves like a transient frontend update instead of a
durable thread artifact with explicit approval state and revision history.

### The UI mixes plan-generation signals with Plan mode state

The frontend currently flips thread state based on `agent_plan` tool activity.
That makes Plan mode mean both "the thread is intentionally in planning" and
"a helper happened to be called," which creates confusing state transitions.

## Product Behavior

### What Plan mode means

Plan mode means the main agent should analyze the task, inspect relevant code,
and produce an implementation plan before any code changes are allowed.

While Plan mode is active:

- the main agent stays read-only
- mutating tools remain blocked
- the main agent may answer questions, clarify scope, and revise the plan
- the main agent must not begin implementation

### Main agent planning responsibility

The dedicated Plan helper is removed. The main agent is responsible for:

- deciding whether planning is needed
- reading relevant files and gathering context
- producing the structured implementation plan
- revising the plan based on user feedback

The main agent may reach this planning checkpoint in two ways:

- the user explicitly starts the run in `plan` mode
- the main agent decides during a default run that the task is complex enough
  that implementation should pause until a plan is produced and approved

### Plan publication

The main agent publishes a plan through a new `update_plan` action. This action
represents "this is the current implementation plan" rather than "call an
internal planning helper."

Publishing a plan must immediately:

1. persist the latest plan as a `plan` message
2. emit a plan update event for the thread UI
3. create a separate implementation approval prompt
4. move the run into `waiting_approval`
5. stop further execution in the current run

### Two-step plan flow

Plan publication and implementation approval remain separate UI steps:

1. the Plan card updates to show the latest complete plan
2. a separate approval prompt appears below it

The approval prompt exposes exactly two actions:

- `按计划实施`
- `清理上下文后按计划实施`

### Reviewing and revising the plan

The user is not required to approve the plan immediately. If the user responds
with new instructions, concerns, or revision requests instead of approving:

- the current approval prompt becomes invalid
- the thread remains in Plan mode
- the main agent continues planning with the new information
- a later `update_plan` call replaces the latest plan revision and creates a new
  approval prompt

This allows repeated planning iterations without leaving Plan mode.

### Approval closes Plan mode

After either approval action succeeds:

- the planning checkpoint ends
- Plan mode is closed
- a new implementation run starts in `default` mode

The implementation run is a fresh run. The planning run is not resumed.

## Plan Artifact Model

### Structured plan payload

`update_plan` should carry a structured payload that can be rendered directly in
the thread and reused for implementation handoff.

Recommended fields:

- `title`
- `summary`
- `steps`
- `risks`
- `openQuestions`
- `planRevision`
- `needsContextResetOption`

Each `steps` item should include:

- `id`
- `title`
- `description`
- `status`
- `files`

### Persistent plan message

Every successful `update_plan` call must persist a thread message with
`message_type = "plan"`.

Recommended metadata fields:

- `planRevision`
- `runModeAtCreation`
- `approvalState`
- `generatedFromRunId`

The latest plan message becomes the source of truth for the active Plan card.

## Approval Model

### Approval prompt as a separate persistent artifact

After every successful `update_plan`, the runtime must persist a separate
message with `message_type = "approval_prompt"`.

This message represents implementation approval, not tool approval.

Recommended metadata fields:

- `kind = "implementation_plan_approval"`
- `planRevision`
- `planMessageId`
- `options`
- `expiresOnNewUserMessage = true`

### Approval options

The approval prompt must expose exactly two actions:

- `apply_plan`
- `apply_plan_with_context_reset`

These map to the user-facing labels:

- `按计划实施`
- `清理上下文后按计划实施`

### Waiting approval behavior

Once the approval prompt is emitted:

- the run moves to `waiting_approval`
- the thread status becomes `waiting_approval`
- the main agent stops generating further implementation content

This is a hard checkpoint, not a soft prompt instruction.

## Implementation Handoff

### Direct implementation handoff

If the user selects `按计划实施`, the runtime starts a new implementation run in
`default` mode using:

- the current thread context
- the latest approved full plan
- a system handoff instruction that the plan has been approved and execution
  should now begin

### Context-clean implementation handoff

If the user selects `清理上下文后按计划实施`, the runtime starts a new
implementation run in `default` mode, but it does not reuse the full planning
conversation.

Instead it must:

1. find the approved plan revision
2. identify the thread context that existed before that plan was first produced
3. compress only that pre-plan context into a summary
4. start a fresh implementation run with:
   - the pre-plan context summary
   - the latest approved full plan
   - the approval action
   - a system handoff instruction that planning discussion has been folded away

This preserves task background while removing planning-iteration noise from the
implementation context.

### Why the summary boundary matters

The context reset path must summarize only context from before the plan was
created. It must not summarize the later planning discussion itself. Otherwise
the implementation run would still inherit the planning noise that this option
is meant to remove.

## State Transitions

### Planning flow

Recommended high-level flow:

1. user starts in `plan` mode, or the main agent explicitly enters planning
2. main agent investigates and drafts the implementation plan
3. main agent calls `update_plan`
4. runtime persists the plan, emits the Plan card update, persists the approval
   prompt, and moves the run to `waiting_approval`
5. user either approves or sends more feedback

### Revision flow

If the user sends more feedback while waiting on plan approval:

1. the pending approval for the previous plan revision expires
2. the thread remains in Plan mode
3. the next planning run uses the user's feedback to revise the plan
4. the main agent may publish a new plan revision

### Implementation flow

If the user approves the latest plan revision:

1. runtime validates that the approval still matches the latest pending revision
2. Plan mode is closed
3. a new implementation run starts in `default` mode
4. the implementation run treats the approved plan as the execution baseline

## Frontend Behavior

### Plan card

The thread UI should render the latest persisted plan artifact, not a helper
summary. The Plan card should update whenever a newer plan revision is
published.

### Approval prompt

The approval prompt should render as a separate timeline message below the plan,
with the two approved actions only.

### Plan mode toggling

The frontend must stop using `agent_plan` tool activity as a signal for Plan
mode. Plan mode should be driven by explicit planning state and runtime
checkpoint transitions.

### Iterative planning

When the thread is waiting for plan approval and the user sends a normal follow
up message instead of clicking approval:

- the UI should leave the thread in Plan mode
- the stale approval prompt should no longer be actionable
- the new message should be treated as plan feedback

## Backend Responsibilities

### Agent session runtime

The runtime should:

- remove the `agent_plan` helper path
- remove completion gating based on helper success
- add a main-agent `update_plan` action
- stop the current run immediately after successful plan publication
- treat implementation approval as a run-level checkpoint rather than as a tool
  approval

### Run manager and persistence

The run manager should:

- persist plan messages
- persist implementation approval prompts
- maintain enough state to recover a waiting approval checkpoint after refresh
- create a new implementation run on approval rather than resuming the planning
  run
- invalidate stale approval prompts when the user sends more planning feedback

## Failure Handling and Guardrails

### Approval must be revision-safe

Each approval action must be bound to a specific `planRevision`. If a newer plan
revision has already been published, approving an older revision must fail with
a clear message directing the user to approve the latest version.

### Plan mode must remain non-mutating

Even if the main agent believes the plan is complete, it must not use mutating
tools while Plan mode is active. The only valid transition into implementation
is through explicit approval.

### Plan publication must halt execution

After `update_plan` succeeds, the run must stop at the planning checkpoint. It
must not continue into implementation within the same run.

### Implementation runs must be fresh

Approving a plan must create a new run rather than resuming the planning run.
This keeps the planning checkpoint explicit and avoids carrying old execution
state into implementation.

### Context reset fallback must not silently change semantics

If pre-plan context compression fails, the runtime must not silently fall back
to direct implementation. The action should fail clearly and preserve the
user's intended approval semantics.

### Approval actions must be idempotent

Repeated clicks on the same approval option for the same still-valid plan
revision must not create multiple implementation runs.

### Implementation may re-enter planning if assumptions break

If implementation discovers that the approved plan is no longer valid, the main
agent should stop implementation, explain the mismatch, and return to a new
planning checkpoint with a new plan revision and approval request.

## Validation

- verify that Plan mode no longer exposes or requires `agent_plan`
- verify that publishing a plan persists both a `plan` message and an
  `approval_prompt` message
- verify that plan publication transitions the run into `waiting_approval`
- verify that approving `按计划实施` creates a new implementation run in
  `default` mode
- verify that approving `清理上下文后按计划实施` builds implementation context
  from a summary of pre-plan context plus the approved full plan
- verify that sending follow-up feedback while waiting approval invalidates the
  old approval and keeps the thread in Plan mode
- verify that stale plan revisions cannot be approved after a newer revision is
  published
