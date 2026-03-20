# SubAgent Minimal Collapsible Design

## Summary

This design updates SubAgent presentation inside the thread timeline so helper
activity reads like lightweight folded runtime context instead of a separate
card system.

The new behavior has three parts:

- remove the current helper card border, background, and status badge
- render each helper in a single-line collapsed header that matches the visual
  language of Thought and Reasoning
- add a lightweight execution-summary footnote in the expanded view, with room
  for future token metrics once the backend exposes them

The goal is not to hide SubAgent work. The goal is to make it scan as folded
supporting context while preserving fast access to the detailed helper trace.

## Goals

- make SubAgent items feel visually aligned with existing folded Thought UI
- reduce thread noise from bordered helper cards
- keep helper state readable in one compact line
- keep `running` and `failed` helper activity expanded by default
- keep `completed` helper activity collapsed by default
- preserve helper detail visibility through a fixed-height scrollable content
  area
- add an execution-summary footnote for expanded helper content

## Non-Goals

- this spec does not expose full child-thread transcripts for helpers
- this spec does not change helper orchestration or event ordering
- this spec does not add user preferences for helper default expansion
- this spec does not require a backend migration in the first UI pass
- this spec does not force tool groups or other timeline blocks to adopt the
  new visual language immediately

## Current Problems

### Helper cards compete too much with primary thread content

The current helper UI renders as a bordered surface with background fill and a
status badge. That makes folded SubAgent activity feel too heavy relative to
assistant messages and Thought blocks.

### Helper summaries do not follow the existing folded-runtime language

Thought and Reasoning already use a minimal single-line trigger pattern. Helper
cards introduce a second visual system, so the thread reads as a mix of
unrelated block styles.

### Expanded helper content lacks a concise execution footer

The expanded helper block shows details such as current action and recent
actions, but it does not end with a compact summary line that answers "how much
work happened here?" at a glance.

## Product Behavior

### Collapsed helper row

Each helper should collapse to one row with exactly four visual parts:

- icon
- name or summary text
- lightweight status text
- collapse chevron

The row should have no card border, no filled background, and no pill badge.
Hover feedback should stay subtle and text-first, similar to Thought and
Reasoning triggers.

### Summary text

The main collapsed label should continue to use the current helper summary
source, derived from helper kind, input summary, and tool-call count, but it
should be trimmed and styled for scanability rather than card-title emphasis.

The summary should remain the primary text. Status should stay visually lighter
than the summary so it acts as a hint instead of the focal point.

### Status text

Collapsed helper rows should use lightweight inline status labels:

- `running`
- `done`
- `failed`

These replace the existing colored badge. The text may still use semantic color
weight, but it should remain visually quiet.

### Default expansion rules

- `completed` helpers render collapsed by default
- `running` helpers render expanded by default
- `failed` helpers render expanded by default

This keeps settled helper work out of the way while keeping active or broken
work obvious.

### Expanded helper content

Expanded helper content should appear directly under the trigger line with no
card shell. The content region should:

- use a fixed visible height
- allow internal vertical scrolling
- keep the most relevant runtime details in readable order

Recommended content order:

1. tool-count summary
2. progress summary
3. current action
4. latest helper message
5. recent actions
6. final summary or error

### Execution-summary footnote

When a helper is expanded, the bottom of the content should show a lightweight
footnote line:

`Execution Summary: 12 tool uses, 123.4s elapsed`

Once helper usage metrics are available from the backend, the same line should
expand naturally to:

`Execution Summary: 12 tool uses, 123.4s elapsed, input tokens 23k, output tokens 1k`

The footnote should remain understated and should not read like a second title.

## Architecture

### Introduce a reusable compact collapsible shell

The current helper rendering is written inline inside
`RuntimeThreadSurface`. This change should extract the visual fold/unfold
structure into a reusable compact component, referred to here as
`CompactCollapsible`.

Responsibilities of `CompactCollapsible`:

- render the single-line trigger row
- manage the open/close affordance and content animation
- render the main content slot
- render an optional footnote slot

Responsibilities that remain helper-specific:

- choosing icon, summary text, and status text
- deciding default-open behavior
- formatting helper detail content
- formatting execution-summary text

This separation keeps the helper branch in `RuntimeThreadSurface` focused on
runtime data instead of raw presentation wiring.

### Scope of first adoption

The first consumer should be helper/SubAgent rendering only.

Tool groups and other runtime artifacts may adopt the same shell later, but
that should remain a follow-up so this change stays tightly scoped.

## Data Flow

### Existing data used in V1

The current frontend helper model already provides enough data for the first
visual pass:

- helper kind
- input summary
- output summary
- error summary
- started and finished timestamps
- tool counts
- total tool calls
- completed steps
- current action
- recent actions
- latest streamed helper message

That means the new layout can ship without changing the event payload shape.

### Execution-summary fields

V1 summary data should be derived as follows:

- `tool uses` -> `helper.totalToolCalls`
- `elapsed` -> derived from `startedAt` and `finishedAt`, or `now` while still
  running

### Deferred metrics

`input tokens` and `output tokens` are not currently available on
`RunHelperDto` or `SubagentProgressSnapshot`.

Therefore:

- V1 should not invent token values or show placeholder noise
- the UI should render only the metrics that are actually available
- the footnote layout should be designed so token metrics can be appended later
  without structural redesign

## Rendering Rules

### Collapsed state

- no border
- no container background
- no badge
- one row only
- summary text truncates when necessary
- status text stays visible
- chevron rotation matches the existing collapsible language

### Expanded state

- content appears inline below the row
- content uses a fixed-height scroll container
- content spacing should feel closer to Thought details than to a card body
- the execution-summary footnote remains pinned at the bottom of the expanded
  block after the scrollable detail area

### Status-specific behavior

- `running`: expanded by default, execution summary updates elapsed time as the
  helper continues
- `failed`: expanded by default, error text remains visible in the detail area
- `completed`: collapsed by default, but the user can expand it to inspect
  summary and execution details

## Error Handling

- if `finishedAt` is absent for a running helper, elapsed time should be derived
  from the current clock
- if there is no `inputSummary`, the row should still render cleanly from helper
  kind and tool count
- if there are no tool counts yet, the content and footer should omit those
  fragments instead of showing zero-heavy clutter
- if future token metrics are absent, the footer should degrade gracefully to
  the metrics that do exist

## Testing

### Visual behavior

- a completed helper renders as a one-line folded row with no border or badge
- a running helper renders expanded by default with the same minimal trigger row
- a failed helper renders expanded by default

### Interaction behavior

- the helper chevron rotates correctly on open and close
- collapsed helper rows remain one line tall even with long summaries
- expanded helper content is fixed-height and internally scrollable

### Execution-summary behavior

- expanded helpers show `Execution Summary`
- V1 shows tool uses and elapsed time when available
- missing token metrics do not render placeholder labels
- future token metrics can be appended without changing helper layout rules

### Regression checks

- helper timeline ordering stays unchanged
- helper stream updates still refresh current action and recent actions
- completed helper entries remain user-expandable after auto-collapsing
