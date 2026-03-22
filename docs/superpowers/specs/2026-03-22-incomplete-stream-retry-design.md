# Robust Recovery For Truncated Streaming Assistant Turns

## Summary

This design hardens the agent runtime against truncated streaming responses
where provider output starts semantically, but the response never reaches a
valid terminal boundary.

The motivating incident is a ZenMux-backed Anthropic stream that produced:

- a completed text block
- a started `tool_use` block with partial JSON input
- then an unexpected stream end

The response never produced:

- `content_block_stop` for the `tool_use`
- `message_delta` with a final `stop_reason`
- `message_stop`

Today, that kind of silent EOF can be misclassified as a successful assistant
turn. The runtime then marks the run `completed` even though the provider output
was incomplete and no valid tool call should have been executed.

The new design treats this as an explicit `incomplete stream` failure mode:

- keep real-time streaming output in the UI
- treat that output as provisional until the turn closes cleanly
- discard the broken turn from model history if the stream truncates
- retry from the most recent stable context
- fail with a precise reason only after bounded retries are exhausted

## Goals

- detect silently truncated provider streams instead of treating them as success
- preserve real-time streaming UX for users
- prevent incomplete assistant turns from polluting model history
- retry only the current assistant turn, not the entire run
- avoid replaying already-completed tool side effects
- surface precise failure reasons when recovery is exhausted

## Non-Goals

- this spec does not add provider failover or model fallback
- this spec does not retry whole runs or replay prior tool execution
- this spec does not guarantee recovery from every malformed provider response
- this spec does not redesign the full message model outside incomplete-stream handling

## Problem Shape

There are three materially different failure classes in the current runtime:

1. pre-stream request failure
   - no semantic assistant output has started yet
   - existing request-level retry is a good fit

2. explicit stream transport failure
   - the HTTP/body stream errors mid-response
   - current code usually maps this to terminal error

3. silent truncated EOF
   - semantic events have already been emitted
   - the byte stream ends without protocol-complete termination
   - current protocol code can still synthesize `Done`, which is unsafe

This spec addresses class `3`.

## Why Protocol-Layer Detection Is Required

The protocol layer is the only place that still knows whether the response is
structurally complete.

Only the protocol parser can reliably answer questions like:

- was `message_stop` received
- is any content block still open
- was a `tool_use` block fully closed
- was final `stop_reason` received
- is there unconsumed partial SSE data still buffered

If detection is delayed to `Agent` or desktop runtime layers, those layers only
see a final `AssistantMessage` and lose the structural evidence needed for
precise diagnosis and safe recovery.

## Core Decision

The runtime should use a two-phase turn lifecycle:

1. provisional streaming
   - deltas are shown to the user immediately
   - the turn is not yet considered stable history

2. committed completion
   - only after the protocol parser confirms a valid terminal boundary
   - only then may the turn become `completed` history and drive tool execution

If the stream ends without a valid terminal boundary, the turn becomes
`discarded`, not `completed`.

## Stability Model

### Stable Context

`stable context` means the most recent conversation state that is safe to replay
back into the model.

It includes only:

- messages already confirmed as `completed`
- tool results already confirmed as `completed`
- approved plan/checkpoint artifacts already persisted as final state

It excludes:

- streaming assistant partials
- messages later marked `discarded`
- incomplete tool calls
- any provisional output from the broken attempt

### Provisional Output

Assistant output emitted during the current turn remains visible in the UI while
streaming, but is provisional until the protocol parser commits it.

This preserves real-time UX without letting broken output contaminate history.

## Incomplete-Stream Detection

Each protocol implementation must define its own completion invariants.

For Anthropic-style streams, a turn may be committed only when:

- all started content blocks have been closed
- there is no unfinished partial tool JSON
- `message_stop` has been received
- if a final stop reason is required for semantic completion, it is present
- there is no leftover buffered SSE fragment that indicates partial event data

If the stream ends and any invariant is violated, the parser must emit a new
terminal error category:

- `incomplete_stream`

This must not be translated into `Done`.

### Examples Of Incomplete Conditions

- missing `message_stop`
- open `content_block_start` without matching `content_block_stop`
- `tool_use` block closed incorrectly or not closed at all
- partial tool JSON that never becomes parseable
- buffered unfinished SSE event data at EOF

## Retry Boundary

Retry remains scoped to a single assistant turn.

The retry unit is:

- one outbound LLM request representing the current turn
- using the most recent stable context snapshot
- after discarding provisional output from the failed attempt

The retry unit is not:

- a whole `AgentSession`
- a whole `continue_()` loop replay from the start
- a whole persisted thread run

## Retry Policy

For incomplete-stream failures:

- retry attempts: `3`
- delay schedule: `1s`, `2s`, `4s`
- max total retry wait budget: `10s`

Retry is allowed only when:

- the failure reason is `incomplete_stream`
- the run has not been cancelled
- the current turn has not already committed tool execution from the broken attempt

Retry is not allowed when:

- cancellation has started
- the failure is a normal provider error unrelated to stream truncation
- the turn has already committed irreversible state from the broken attempt

## Tool Execution Safety

The runtime must not execute tool calls from provisional or incomplete turns.

A tool call becomes executable only when:

- the tool-use block is structurally closed
- the tool input JSON is complete and parseable
- the enclosing assistant turn is still eligible to continue

If the stream truncates after emitting partial `tool_use`, that tool call is
discarded outright:

- no approval request
- no persisted `tool_calls` record as a valid requested tool
- no execution

This rule avoids turning malformed provider output into workspace side effects.

## Message Lifecycle Changes

### New Message Status

Add a new persisted assistant message status:

- `discarded`

Meaning:

- the user may have seen this content in real time
- the runtime determined the turn was incomplete
- the content is kept for diagnostics and user visibility
- the content must not be replayed into model history

### State Transitions

Recommended assistant message lifecycle:

- `streaming` -> `completed`
- `streaming` -> `discarded`
- `streaming` -> `failed`

`discarded` is distinct from `failed`:

- `failed` describes a terminal run or message failure
- `discarded` describes a provisional turn that was intentionally excluded from
  stable history during recovery

## Event Model

Add explicit events so the frontend can distinguish recovery from terminal
failure.

### New Thread Events

- `run_retrying`
  - `run_id`
  - `attempt`
  - `max_attempts`
  - `delay_ms`
  - `reason`

- `message_discarded`
  - `run_id`
  - `message_id`
  - `reason`

### Event Semantics

`run_retrying`:

- informational only
- does not mark the run failed
- keeps the run in `running`

`message_discarded`:

- marks the visible provisional output as excluded from stable history
- should arrive before the next retry begins

Existing terminal events remain unchanged:

- `run_completed`
- `run_failed`
- `run_cancelled`
- `run_interrupted`

## Frontend Behavior

### Streaming UX

The frontend should continue rendering assistant deltas immediately.

This preserves the current real-time experience.

### Discarded Output UX

When a provisional message is discarded:

- keep it visible in the thread
- style it as weak/diagnostic output
- default to expanded full content
- attach a short reason such as:
  - `Response was truncated during tool use. This partial output was discarded and the turn was retried.`

The content remains useful for debugging, especially when comparing with
provider-side logs.

### Retry Notice

While the runtime is backing off before retrying, the thread UI should show a
lightweight retry notice, for example:

- `Last response was truncated during tool use. Retrying 2/3 from the latest stable context in about 2 seconds.`

### History Reconstruction

When reloading a thread:

- render `discarded` messages in the transcript
- exclude them from the model-history reconstruction path

## Agent Runtime Changes

### `tiy-core` Protocol Layer

Protocol parsers must:

- track turn completion invariants
- refuse to emit `Done` when the stream is structurally incomplete
- emit an explicit incomplete-stream terminal error instead

### `tiy-core::agent::Agent`

`run_turn()` should support:

- capturing the current stable context snapshot before issuing the request
- receiving an incomplete-stream terminal result
- discarding provisional assistant output from the failed attempt
- retrying the same turn from the stable snapshot

The retry loop must be per-turn, not per-run.

### Desktop `AgentSession`

`AgentSession` should:

- forward retry notices to desktop thread events
- mark the current assistant message `discarded` when instructed by the core
- keep run state as `running` during retry backoff

### `AgentRunManager`

`AgentRunManager` should:

- persist `discarded` message status
- avoid turning discarded/provisional turns into stable thread history
- settle the run as `failed` only after retry exhaustion

## Failure Reasons

After retries are exhausted, the final failure reason should be explicit and
structured.

Recommended shape:

- machine code:
  - `stream.incomplete.missing_message_stop`
  - `stream.incomplete.open_tool_use_block`
  - `stream.incomplete.truncated_tool_input`
  - or a combined umbrella code such as `stream.incomplete`

- human-readable detail:
  - `Anthropic stream ended unexpectedly: missing message_stop, tool_use block at index 1 remained open, partial tool input was truncated.`

This message should be visible both in persisted run failure state and in the
thread UI.

## Rollout Strategy

### Phase 1

Implement the full flow for:

- Anthropic protocol
- ZenMux Anthropic-compatible endpoint

### Phase 2

Generalize the same incomplete-stream contract to:

- OpenAI Responses
- OpenAI Completions
- Google

The abstraction should be protocol-agnostic even if only Anthropic lands first.

## Testing

### Protocol Tests

Add parser tests that replay the exact truncated ZenMux log pattern:

- completed text block
- started tool-use block
- truncated input JSON
- EOF without message-level termination

Expected result:

- no `Done`
- explicit incomplete-stream terminal error

### Agent Tests

Add turn-level retry tests verifying:

- provisional output is visible during streaming
- incomplete turns are not committed to stable history
- retries use the last stable context snapshot
- exhausted retries return terminal failure with a clear reason

### Desktop Integration Tests

Add integration coverage verifying:

- discarded messages persist with status `discarded`
- discarded messages render in thread history
- discarded messages are excluded from replayed model history
- retry notices keep run state active
- terminal failure appears only after retries are exhausted

## Risks

- holding provisional content in a separate lifecycle adds message-state complexity
- protocol-specific completeness rules must be precise to avoid false positives
- retrying after partial visible output may momentarily feel confusing without
  strong UI copy

These risks are acceptable because the current failure mode can silently mark a
broken turn as successful, which is much worse.

## Open Questions

No open product questions remain for v1 of this design. The defaults are:

- `3` retries
- `1s / 2s / 4s` backoff
- `10s` total retry budget
- discarded partial output remains visible and expanded by default
- retries are scoped to the current assistant turn only
