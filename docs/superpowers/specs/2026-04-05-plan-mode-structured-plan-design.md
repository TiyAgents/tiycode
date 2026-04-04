# Plan Mode Structured Plan Artifact Design

## Summary

This design upgrades Plan mode from a step-oriented outline into a complete
implementation proposal artifact. The new plan format keeps the existing plan
approval checkpoint, but makes each published plan carry enough structure for a
user to review the implementation intent before code changes begin.

The main change is to expand the plan artifact beyond `summary`, `steps`, and
`risks`. A complete plan should also describe the confirmed current context, the
recommended design approach, the key implementation points, and the planned
verification strategy. This makes the Plan card useful as both a review surface
and an execution handoff contract.

## Goals

- make Plan mode output feel like a complete implementation proposal instead of
  a rewritten task list
- capture plan information in a structured artifact rather than relying on
  prompt-only formatting
- preserve the existing approval checkpoint and revision flow
- allow the frontend to render richer plan sections consistently
- keep the artifact reusable for implementation handoff and future plan history
- remain backward compatible with already-persisted plan messages

## Non-Goals

- this spec does not redesign approval actions or waiting-approval behavior
- this spec does not introduce execution progress tracking into the plan artifact
- this spec does not require a new run mode or planner agent
- this spec does not redesign unrelated assistant reply formatting outside Plan
  mode

## Current Problems

### The artifact model is narrower than the planning prompt

The current Plan mode prompt already asks the model to include `summary`,
ordered `steps`, `verification`, and `risks`. However, the persisted
`PlanArtifact` only stores `title`, `summary`, `steps`, and `risks`. This means
the most durable contract available to the model is still a step list with
minimal framing.

### The Plan card only renders a subset of the intended planning content

The frontend currently renders the plan summary, ordered steps, and risks. Even
if the model tries to provide richer planning content in prose, the UI does not
have stable sections for it. As a result, the output gravitates toward
step-centric descriptions.

### The current structure is weak at expressing why the change should be done in a specific way

Users evaluating a plan usually need more than a sequence of edits. They need
to know the confirmed context, the chosen design direction, the critical
implementation points, and how the result will be validated. Those are not
first-class fields today.

## Product Direction

### What a good Plan mode output should contain

A complete plan should answer six user-facing questions before implementation:

1. What problem are we solving and what outcome are we targeting?
2. What relevant current-state context has already been confirmed?
3. What design approach is recommended and why?
4. What are the key implementation points that matter most?
5. What are the concrete execution steps?
6. How will we verify the change and what risks remain?

### Target plan structure

Plan mode should publish a structured artifact with these logical sections:

- `summary`: the implementation goal and expected result
- `context`: the confirmed current-state evidence that shapes the solution
- `design`: the recommended solution and important tradeoffs
- `keyImplementation`: the main modules, interfaces, data flow, or state changes
  that will carry the implementation
- `steps`: the ordered execution plan
- `verification`: the checks that will confirm the implementation works
- `risks`: the major regression areas, edge cases, and compatibility concerns
- `assumptions`: non-blocking assumptions made while preparing the plan

The plan remains implementation-oriented. These sections are not meant to become
an architecture essay. They should be concise, concrete, and directly useful for
approval and handoff.

## Plan Artifact Model

### Proposed payload

`update_plan` should support the following structured payload:

- `title`
- `summary`
- `context`
- `design`
- `keyImplementation`
- `steps`
- `verification`
- `risks`
- `assumptions`
- `planRevision`
- `needsContextResetOption`

Recommended shapes:

- `summary`: `string`
- `context`: `string[]`
- `design`: `string[]`
- `keyImplementation`: `string[]`
- `steps`: `PlanStep[]`
- `verification`: `string[]`
- `risks`: `string[]`
- `assumptions`: `string[]`

The existing `PlanStep` shape remains valid:

- `id`
- `title`
- `description`
- `status`
- `files`

### Why arrays are preferred for most new sections

`context`, `design`, `keyImplementation`, `verification`, and `assumptions`
should use string arrays instead of one large blob. This keeps the artifact easy
to render, compare between revisions, and populate from model output without
parsing brittle markdown.

### Backward compatibility

Older plan artifacts that only contain `summary`, `steps`, and `risks` must
continue to render correctly.

Compatibility rules:

- absent new fields should deserialize as empty arrays
- old plans should continue to show the same summary, steps, and risks sections
- frontend formatting helpers should treat missing sections as optional rather
  than malformed
- markdown fallback should still produce a readable plan for older persisted
  messages

## Prompt Contract

### Plan mode prompt requirements

Plan mode instructions should stop describing the desired plan as only
`summary`, `ordered steps`, `verification`, and `risks`. The prompt should
instead define a fuller contract that maps directly to the expanded artifact.

Recommended planning instruction:

- `summary` must state the user goal, intended change, and expected outcome
- `context` must only include facts already confirmed from inspected code,
  documentation, or user input
- `design` must describe the recommended approach and major tradeoffs
- `keyImplementation` must name the important files, modules, interfaces, or
  state transitions involved in the change
- `steps` must be concrete, ordered, and directly executable after approval
- `verification` must specify how the result will be checked
- `risks` must call out likely regressions, edge cases, or migration concerns
- `assumptions` may capture non-blocking assumptions, but unresolved blocking
  decisions should still require `clarify` before plan publication

### Blocking vs non-blocking uncertainty

The runtime should continue to treat unresolved requirements and scope decisions
as blockers to `update_plan`. However, a plan may still include explicit
assumptions when they do not prevent safe implementation and are clearly
identified as assumptions rather than open questions.

## Frontend Rendering

### Plan card layout

The Plan card should render the expanded sections in a stable top-down order:

1. header metadata
2. title
3. summary
4. context
5. design
6. key implementation
7. steps
8. verification
9. risks
10. assumptions

### Presentation guidelines

- `summary` should remain visible without requiring expansion
- `steps` should remain the most prominent detailed section because they drive
  implementation handoff
- `context`, `design`, `key implementation`, `verification`, `risks`, and
  `assumptions` should render as compact bullet groups when present
- empty sections should not render placeholders
- older plans should keep their existing visual appearance as closely as
  possible

### Why this should stay in the Plan card rather than assistant prose

If these sections are only expressed in assistant markdown, the durable plan
contract remains ambiguous. Rendering them from metadata keeps the plan card as
the single source of truth for plan review and approval.

## Runtime and Parsing Changes

### Backend plan artifact parsing

`build_plan_artifact_from_tool_input` should read the new optional fields from
`update_plan` input and normalize them to empty arrays when absent.

The parser should also preserve the current fallbacks:

- use `summary`, `description`, or `overview` when deriving the summary
- synthesize a single fallback step when no valid steps are provided
- default `needsContextResetOption` to `true` unless explicitly overridden

### Markdown generation

`plan_markdown` should be extended so persisted plan messages remain readable in
any surface that falls back to markdown. The markdown should mirror the expanded
section order while omitting empty sections.

## Validation Rules

The plan artifact should enforce a minimum completeness bar for Plan mode.

Recommended rules:

- `summary` must be non-empty
- at least one `steps` entry must exist after normalization
- `verification` should be present for implementation-oriented plans; if the
  model omits it, the runtime may inject a conservative fallback such as
  "Run relevant type-check, tests, and manual behavior verification"
- `design` and `keyImplementation` are strongly recommended, but the runtime may
  allow them to be empty during the first compatibility phase

This should be treated as a phased rollout:

1. add storage and rendering support first
2. update prompt requirements
3. later tighten validation once the model reliably populates the new fields

## Testing Strategy

### Backend tests

- add parsing coverage for the expanded plan payload
- verify backward compatibility with older minimal plan payloads
- verify markdown generation omits empty sections and includes populated ones
- verify fallback behavior for missing optional sections and empty steps

### Prompt tests

- update prompt assertions so Plan mode explicitly requires `context`, `design`,
  `keyImplementation`, `steps`, `verification`, and `risks`
- keep assertions that unresolved blocking decisions require `clarify` before
  `update_plan`

### Frontend tests

- extend metadata parsing tests for the new optional fields
- add rendering coverage for plans with all sections populated
- add rendering coverage for older plans with only summary, steps, and risks

## Rollout Plan

### Phase 1: Schema and compatibility

- extend the Rust `PlanArtifact`
- extend frontend metadata parsing and formatting helpers
- keep the UI tolerant of missing fields

### Phase 2: Prompt and rendering

- update Plan mode prompt instructions to target the richer artifact
- render the new sections in the Plan card

### Phase 3: Completeness tightening

- add stronger completeness checks for `verification`
- evaluate whether `design` and `keyImplementation` should become required once
  model behavior is stable

## Expected Outcome

After this change, Plan mode should produce plans that read like approval-ready
implementation proposals instead of lightly elaborated edit checklists. Users
should be able to review the confirmed context, chosen design, critical
implementation details, execution steps, verification strategy, and residual
risks from the Plan card alone.
