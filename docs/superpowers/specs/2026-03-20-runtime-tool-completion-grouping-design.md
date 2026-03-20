# Runtime Tool Completion Grouping Design

## Summary

This design updates the runtime thread surface so completed tool calls collapse
more aggressively and read as grouped execution history instead of a long list
of repeated cards.

The behavior change has two parts:

- a single tool card should automatically collapse as soon as its status becomes
  `completed`
- a contiguous run of tool timeline entries should be folded into one visual
  group when every tool in that run is currently `completed`

The grouping rule is dynamic. A tool that is still waiting for approval or still
running does not enter a group yet, but it may join a completed group later once
its visible status becomes `completed`.

## Goals

- automatically collapse each tool card once execution completes
- reduce visual noise from long runs of completed tool activity
- keep running and approval-blocked tools individually visible
- allow the timeline to regroup itself when an in-flight tool later completes
- preserve the existing detailed parameter and result view for each individual
  tool call inside a group

## Non-Goals

- this spec does not change backend tool persistence or runtime event payloads
- this spec does not merge tools by name or semantic similarity
- this spec does not reorder timeline entries
- this spec does not introduce a permanent user preference for group expansion
- this spec does not change helper-task grouping behavior

## Current Problems

### Completed tools stay open based on mount timing

The current tool card uses `defaultOpen`, so the open state is only chosen when
the card first mounts. A tool that starts open while running stays open after it
completes, which makes the completed state feel inconsistent.

### Repeated tool activity creates unnecessary vertical noise

Long bursts of completed tools create a stack of near-identical cards. This
makes it harder to scan the thread for what is still active versus what is
already settled.

### Approval and running states should remain visible, not hidden

The product still needs approval-required tools and actively running tools to be
obvious. Grouping should not hide unfinished work behind a collapsed summary.

## Product Behavior

### Single tool collapse rule

Each individual tool card should be rendered with controlled open state instead
of `defaultOpen`.

Rules:

- if a tool is not `completed`, the card stays expanded
- if a tool becomes `completed`, the card automatically collapses
- if a completed tool later appears from snapshot load, it renders collapsed from
  the start

This keeps the completed state deterministic instead of depending on when the
card first entered the tree.

### Dynamic completed-tool grouping

The timeline should be transformed after the normal message/helper/tool entries
are built and sorted.

A tool group is formed when:

- the entries are consecutive in the rendered timeline
- the entries are all tool entries
- the entries all currently have `state = "output-available"`

A tool group must break when:

- a non-tool entry appears
- a tool entry appears that is not completed

This rule is evaluated from current visible state, not historical first-seen
state. That means the grouping is allowed to change over time.

Example:

1. `list` is completed
2. `read` is waiting approval
3. `read` is approved and later completed

While step 2 is waiting approval, it is rendered outside the completed group.
Once step 3 becomes completed, the timeline is recalculated and the completed
sequence may merge into a single group if the surrounding entries are also
completed tools.

### Group presentation

Each completed tool group should render as one collapsible block with:

- a summary title of `Tools × N`
- a completed-style badge
- collapsed by default

When expanded, the group shows the original tool cards in chronological order,
including each call's parameters and result payload.

The label intentionally avoids naming a specific tool because grouping no longer
requires matching tool names.

## Architecture

### State ownership

The source state remains unchanged:

- `messages` hold message entries
- `helpers` hold helper entries
- `tools` hold individual tool entries

No grouping metadata needs to be persisted in state or sent from the backend.

### Derived timeline model

`RuntimeThreadSurface` should continue to build the flat sorted timeline first.
After that, the UI should derive a second presentation model that can contain:

- message entries
- helper entries
- single tool entries
- completed tool group entries

This keeps runtime event handling simple and localizes the grouping behavior to
render-time derivation.

### Controlled tool open state

Single tool rendering should switch from uncontrolled `defaultOpen` to a
controlled `open` value derived from the tool state.

Recommended rule:

- `open = tool.state !== "output-available"`

That guarantees all completed single tools are collapsed immediately.

Tool groups should use the same principle and default to collapsed.

## Rendering Rules

### Messages and helpers

Message and helper entries keep their existing behavior and also act as hard
group boundaries for completed tools.

### Single unfinished tool

Any tool that is waiting approval, approval-responded, running, denied, or in an
error state stays outside a completed group and renders as its own card.

### Completed tool group

If the derived presentation pass sees a contiguous run of two or more completed
tools, it should emit one group entry instead of multiple standalone entries.

If the contiguous run contains only one completed tool, that entry may remain a
single tool card rather than a group. The key product behavior is automatic
collapse on completion; grouping is primarily for bursts of repeated completed
activity.

## Data Flow

1. frontend receives snapshot data and stream events as it does today
2. `tools` state is updated per tool id with no grouping awareness
3. the view derives sorted timeline entries
4. the view performs a second pass to compress contiguous completed tools into
   group entries
5. React renders either a single tool card or a collapsed tool group depending
   on the derived presentation entry

## Error Handling

- failed and denied tools never enter a completed group
- approval-required tools never enter a completed group
- if a tool transitions from unfinished to completed, regrouping happens on the
  next render without any migration step
- if a tool payload is missing input or output, the existing per-tool fallbacks
  still apply inside the group

## Testing

### UI behavior checks

- a running tool renders expanded
- the same tool collapses automatically when its state becomes completed
- a completed tool loaded from snapshot renders collapsed

### Grouping checks

- two adjacent completed tools render as one `Tools × 2` group
- a completed tool followed by a running tool does not group across the running
  entry
- once that running tool later completes, the completed sequence recomputes and
  groups dynamically
- helper entries and messages break completed-tool groups

### Regression checks

- approval actions still work for approval-required tools
- failed and denied tools remain individually visible
- tool parameter and result content still render correctly inside grouped output
