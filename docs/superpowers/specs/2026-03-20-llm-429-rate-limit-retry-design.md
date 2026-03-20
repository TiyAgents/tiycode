# Robust 429 Retry Handling For LLM Requests

## Summary

This design makes LLM runs more resilient when providers return `429 Too Many
Requests`.

The key decision is to retry at the single-request boundary instead of replaying
an entire agent run. In this product, rate limits often happen after several
tool calls have already completed. Retrying the whole `continue_()` cycle would
risk repeating tool execution and duplicating side effects. Request-level retry
keeps prior tool work intact and only retries the blocked LLM round-trip.

The user experience should remain transparent. When the runtime is waiting to
retry after a rate limit, the thread UI should explicitly show that the run is
still active and that the system is backing off before retrying.

## Goals

- automatically recover from transient provider-side rate limits
- avoid replaying completed tool calls
- show clear in-thread status when the runtime is backing off
- preserve the existing run lifecycle semantics for success, failure, and cancel
- keep the retry policy narrow to low-risk, clearly recoverable scenarios

## Non-Goals

- this spec does not add model fallback or provider failover
- this spec does not retry arbitrary 4xx errors
- this spec does not auto-retry after assistant output has already started
- this spec does not add run-level replay after interruption or failure

## Why Run-Level Retry Is Rejected

Retrying `AgentSession::run()` or `Agent::continue_()` from the desktop layer is
not a safe fit for this product.

In real usage, a run can already have:

- completed multiple tool calls
- persisted tool activity to the database
- emitted UI events that the frontend already rendered
- mutated workspace state through approved tools

If a later LLM request in that same run returns `429`, replaying the entire run
can re-execute tools, produce duplicate output, and blur the boundary between
"same run recovering" and "new run starting". The correct retry boundary is the
single outbound provider request that failed before it started streaming any new
assistant content.

## Architecture

### Retry Boundary

Retry should live in `tiy-core` protocol/provider request handling, not in
`src-tauri/src/core/agent_session.rs`.

The retry unit is:

- one outbound LLM HTTP request
- before the first assistant delta of that request has been emitted

The retry unit is not:

- an entire `AgentSession`
- an entire `continue_()` loop
- a persisted desktop run

### Layer Responsibilities

1. `tiy-core` protocol layer
   - detects retryable rate-limit responses
   - computes retry delay
   - retries only when no assistant output has started
   - exposes a structured retry notification callback

2. `AgentSession`
   - wires the retry notification callback from `tiy-core` into desktop events
   - remains the owner of one in-memory session
   - does not replay the run itself

3. `AgentRunManager`
   - keeps the run in `running` state during retry backoff
   - does not create a new run or reset persisted history

4. `Frontend Thread UI`
   - displays a non-fatal retry notice while the run is still active
   - clears the notice once output resumes or the run settles

## Retry Policy

### When To Retry

Automatic retry is allowed only when all of the following are true:

- the provider response is a rate-limit condition
- the failure happened before the first assistant delta for that request
- the run has not been cancelled
- the computed wait time is within configured limits

Rate-limit conditions include:

- HTTP `429`
- provider error text that clearly indicates rate limiting, such as
  `rate limit` or `too many requests`

### When Not To Retry

Automatic retry must not happen when:

- any assistant text delta for the current request has already been emitted
- a provider stream has already started and later fails mid-stream
- the error is clearly non-retryable, such as `400`, `401`, or `403`
- the runtime is already cancelling
- the provider asks for a wait longer than the configured maximum

This "first-byte only" rule keeps message assembly simple and avoids duplicate
or partially repeated assistant output.

## Backoff Strategy

### Delay Selection

The retry delay should be chosen in this order:

1. use `Retry-After` when present and valid
2. otherwise use exponential backoff with jitter

Recommended fallback schedule:

- attempt 1: about `1s`
- attempt 2: about `2s`
- attempt 3: about `4s`
- attempt 4: about `8s`

Recommended caps:

- maximum attempts: `4`
- maximum single delay: `15s`
- maximum total wait budget: `30s`

If `Retry-After` exceeds the configured cap, the request should fail normally
with a clear error instead of silently waiting too long.

### Interaction With Existing `max_retry_delay_ms`

`tiy-core::agent::Agent` already exposes `set_max_retry_delay_ms(...)`, but the
desktop product currently does not use it to provide visible retry state. This
design keeps that cap concept, but adds product-owned retry semantics:

- request-level retry instead of run-level retry
- explicit desktop event emission before each retry
- product-owned retry attempt limits and visibility

## Desktop Event Model

### New Thread Event

Add a new `ThreadStreamEvent` variant:

- `RateLimitRetrying`

Recommended payload:

- `run_id`
- `attempt`
- `max_attempts`
- `delay_ms`
- `reason`

Example meaning:

- the run is still active
- the runtime hit a retryable rate limit
- it is waiting `delay_ms` before retrying the same request

### Event Semantics

This event is informational only.

It should:

- not mark the run as failed
- not create a new run
- not change the thread to `failed`
- be emitted while the run remains `running`

Existing terminal events stay unchanged:

- `run_failed` still means retry has been exhausted or the error is not
  retryable
- `run_completed` still means the run finished successfully

## Frontend Behavior

### Thread Stream Mapping

Update the thread stream adapter and shared event types so the frontend can
consume `rate_limit_retrying`.

The adapter should expose a dedicated callback such as:

- `onRetryNotice`

This should be separate from `onError` so the UI does not confuse active retry
with terminal failure.

### UI Presentation

`runtime-thread-surface.tsx` should show a lightweight in-thread notice while a
retry is pending.

Recommended copy:

- `Encountered rate limiting. Retrying 2/4 in about 4 seconds.`

Behavior:

- show the notice while waiting to retry
- clear it as soon as normal output resumes
- clear it on `run_completed`, `run_cancelled`, or `run_interrupted`
- replace it with the normal terminal error path only if the run eventually
  emits `run_failed`

## Implementation Shape

### `tiy-core`

Add a small retry helper in the protocol/common layer or an adjacent shared
request utility that:

- classifies retryable rate-limit responses
- parses `Retry-After`
- computes exponential backoff with jitter
- emits a retry callback before sleeping
- aborts cleanly when the cancellation token fires

Each protocol implementation that performs an outbound request should use this
helper before turning a `429` into a terminal error event.

The helper must track whether the request has emitted its first assistant delta.
If output has started, later errors should remain terminal for that request.

### `tiy-desktop`

Update the desktop runtime surfaces to carry retry notices end to end:

- `src-tauri/src/ipc/frontend_channels.rs`
- `src/services/bridge/agent-commands.ts`
- `src/shared/types/api.ts`
- `src/services/thread-stream/thread-stream.ts`
- `src/modules/workbench-shell/ui/runtime-thread-surface.tsx`

`AgentSession` should register the retry callback on the main agent runtime
request path and forward those notices as `ThreadStreamEvent::RateLimitRetrying`.

## Persistence And State

No new database table is required for v1.

Retry notices are transient UI/runtime events only. They do not need durable
message records because:

- they are operational status, not conversation content
- they should disappear naturally once the request resumes or the run ends
- the canonical persisted error remains `run.error_message` when retries are
  exhausted

## Testing

### `tiy-core` Unit Tests

- `429` with valid `Retry-After` retries using the provided delay
- `429` without `Retry-After` uses fallback exponential backoff
- `400`, `401`, and `403` do not retry
- retry stops once the configured attempt limit is reached
- retry aborts immediately when cancellation fires during backoff
- no retry occurs after the first assistant delta has been emitted

### Desktop Integration Tests

- a run whose first request attempt returns `429` and second attempt succeeds
  still completes as one run
- no duplicate assistant messages are persisted
- no duplicate tool events are emitted for earlier tool work
- the frontend receives `rate_limit_retrying` before the successful retry
- a retry-exhausted run still emits the normal `run_failed`

### Regression Coverage

- existing approval flow remains unchanged
- helper-orchestration events remain unchanged
- non-rate-limit provider failures still surface through the existing error path

## Risks

### Provider Error Shape Variance

Different providers expose rate limits differently. HTTP `429` is reliable, but
string matching on error bodies is necessarily more fragile. The classifier
should prefer status code and only use message text as a fallback.

### Duplicate Retry Layers

If a provider library or HTTP client already performs hidden retries, the
desktop product can lose control over timing and visibility. The final
implementation should make sure only one visible retry policy is active for this
path.

### UI Drift

If the frontend treats retry notices as ordinary errors, the thread may look
failed while still running. The adapter must keep retry notices on a separate
callback and state lane.

## Rollout

1. add request-level retry utilities in `tiy-core`
2. add structured retry notification plumbing to desktop events
3. render retry notice in the runtime thread UI
4. add unit and integration coverage
5. validate against a provider stub that returns `429` before success
