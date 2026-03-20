# LLM 429 Rate Limit Retry Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Add request-level, first-byte-only automatic retry for LLM `429 Too Many Requests` responses, and show a visible in-thread retry notice while the run stays active.

**Architecture:** Implement retry at the single-provider-request layer inside `tiy-core`, not by replaying `AgentSession` or `continue_()`. Add a small retry notification hook to `StreamOptions`, use it from the four streaming protocols before assistant output starts, then forward those notices through desktop `ThreadStreamEvent` and render them as a non-fatal runtime banner in the thread UI.

**Tech Stack:** Rust, `tiy-core`, Tauri 2, React 19, TypeScript, Vite

---

### Task 1: Add the retry notice contract in `tiy-core`

**Files:**
- Modify: `../tiy-core/src/types/context.rs`
- Modify: `../tiy-core/src/agent/agent.rs`
- Test: `../tiy-core/src/types/context.rs`

**Step 1: Add the retry notice callback type**

Add a new shared callback type next to `OnPayloadFn`:

```rust
pub type OnRateLimitRetryFn = Arc<
    dyn Fn(RateLimitRetryNotice) -> Pin<Box<dyn Future<Output = ()> + Send>>
        + Send
        + Sync,
>;
```

Add the payload struct:

```rust
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RateLimitRetryNotice {
    pub attempt: u32,
    pub max_attempts: u32,
    pub delay_ms: u64,
    pub reason: String,
}
```

**Step 2: Extend `StreamOptions`**

Add a skipped field:

```rust
#[serde(skip)]
pub on_rate_limit_retry: Option<OnRateLimitRetryFn>,
```

Keep it out of `PartialEq`, and redact it in `Debug` the same way `on_payload` is redacted.

**Step 3: Add an Agent setter**

In `../tiy-core/src/agent/agent.rs`, add:

```rust
pub fn set_on_rate_limit_retry<F, Fut>(&self, hook: F)
where
    F: Fn(RateLimitRetryNotice) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = ()> + Send + 'static,
{
    let hook = Arc::new(move |notice: RateLimitRetryNotice| {
        Box::pin(hook(notice)) as Pin<Box<dyn Future<Output = ()> + Send>>
    });
    self.hooks.write().on_rate_limit_retry = Some(hook);
}
```

Also thread `on_rate_limit_retry` from `AgentHooks` into `build_stream_options()`.

**Step 4: Write the failing test**

Add a focused unit test that proves `StreamOptions::default()` leaves the callback unset:

```rust
#[test]
fn stream_options_default_has_no_rate_limit_retry_hook() {
    let options = StreamOptions::default();
    assert!(options.on_rate_limit_retry.is_none());
}
```

**Step 5: Run the targeted test**

Run: `cargo test --manifest-path ../tiy-core/Cargo.toml stream_options_default_has_no_rate_limit_retry_hook -- --nocapture`

Expected: PASS after the new type and field are wired correctly.

**Step 6: Commit**

```bash
git add ../tiy-core/src/types/context.rs ../tiy-core/src/agent/agent.rs
git commit -m "feat(core): add rate limit retry hook contract"
```

### Task 2: Add shared retry helpers in `tiy-core` protocol common code

**Files:**
- Modify: `../tiy-core/src/protocol/common.rs`
- Test: `../tiy-core/src/protocol/common.rs`

**Step 1: Add retry parsing helpers**

Implement helpers with no provider-specific logic:

```rust
pub fn parse_retry_after_ms(value: Option<&str>) -> Option<u64> { /* ... */ }

pub fn is_retryable_rate_limit(status: reqwest::StatusCode, body: &str) -> bool { /* ... */ }

pub fn compute_rate_limit_delay_ms(
    attempt: u32,
    retry_after_ms: Option<u64>,
    max_delay_ms: u64,
) -> Option<u64> { /* ... */ }
```

Rules:

- prefer valid `Retry-After`
- otherwise fall back to `1000, 2000, 4000, 8000`
- clamp to `max_delay_ms`
- return `None` if the chosen delay is invalid or exceeds policy

**Step 2: Add the async notifier helper**

Add a helper that emits the notice if the callback exists:

```rust
pub async fn emit_rate_limit_retry_notice(
    options: &StreamOptions,
    attempt: u32,
    max_attempts: u32,
    delay_ms: u64,
    reason: impl Into<String>,
) {
    if let Some(callback) = &options.on_rate_limit_retry {
        callback(RateLimitRetryNotice {
            attempt,
            max_attempts,
            delay_ms,
            reason: reason.into(),
        }).await;
    }
}
```

**Step 3: Add the failing tests**

Add tests for:

```rust
#[test]
fn parse_retry_after_ms_accepts_integer_seconds() {
    assert_eq!(parse_retry_after_ms(Some("4")), Some(4000));
}

#[test]
fn parse_retry_after_ms_rejects_invalid_values() {
    assert_eq!(parse_retry_after_ms(Some("abc")), None);
}

#[test]
fn compute_rate_limit_delay_ms_falls_back_exponential_schedule() {
    assert_eq!(compute_rate_limit_delay_ms(1, None, 15_000), Some(1_000));
    assert_eq!(compute_rate_limit_delay_ms(2, None, 15_000), Some(2_000));
}

#[test]
fn is_retryable_rate_limit_matches_429() {
    assert!(is_retryable_rate_limit(reqwest::StatusCode::TOO_MANY_REQUESTS, ""));
}
```

**Step 4: Run the targeted tests**

Run: `cargo test --manifest-path ../tiy-core/Cargo.toml protocol::common -- --nocapture`

Expected: New helper tests pass.

**Step 5: Commit**

```bash
git add ../tiy-core/src/protocol/common.rs
git commit -m "feat(core): add shared rate limit retry helpers"
```

### Task 3: Add first-byte-only retry to OpenAI-style protocols

**Files:**
- Modify: `../tiy-core/src/protocol/openai_completions.rs`
- Modify: `../tiy-core/src/protocol/openai_responses.rs`
- Test: `../tiy-core/src/protocol/openai_completions.rs`
- Test: `../tiy-core/src/protocol/openai_responses.rs`

**Step 1: Write the failing helper-focused test for OpenAI Completions**

Extract a tiny pure helper near the request path:

```rust
fn should_retry_pre_stream_response(
    status: reqwest::StatusCode,
    body: &str,
    saw_output: bool,
) -> bool {
    !saw_output && super::common::is_retryable_rate_limit(status, body)
}
```

Add:

```rust
#[test]
fn should_retry_pre_stream_response_only_before_output() {
    assert!(should_retry_pre_stream_response(
        reqwest::StatusCode::TOO_MANY_REQUESTS,
        "rate limit",
        false,
    ));
    assert!(!should_retry_pre_stream_response(
        reqwest::StatusCode::TOO_MANY_REQUESTS,
        "rate limit",
        true,
    ));
}
```

**Step 2: Run the test to verify the helper shape**

Run: `cargo test --manifest-path ../tiy-core/Cargo.toml should_retry_pre_stream_response_only_before_output -- --nocapture`

Expected: PASS once the helper exists.

**Step 3: Implement the retry loop in `openai_completions.rs`**

Wrap only the request-send and pre-stream response handling in a bounded retry loop:

```rust
let max_attempts = 4;
let mut attempt = 1;
let saw_output = false;

loop {
    let Some(response) = super::common::send_request_with_cancel(/* ... */).await? else {
        return Ok(());
    };

    if response.status().is_success() {
        break response;
    }

    let status = response.status();
    let retry_after = response.headers().get(reqwest::header::RETRY_AFTER)
        .and_then(|v| v.to_str().ok());
    let body = crate::types::read_error_body(response, limits.http.max_error_body_bytes).await;

    if attempt < max_attempts && should_retry_pre_stream_response(status, &body, saw_output) {
        let delay_ms = /* compute via common helper */;
        super::common::emit_rate_limit_retry_notice(&options, attempt, max_attempts, delay_ms, format!("HTTP {}: {}", status, body)).await;
        tokio::time::sleep(Duration::from_millis(delay_ms)).await;
        attempt += 1;
        continue;
    }

    // existing error path
}
```

Do not emit `AssistantMessageEvent::Start` until a successful response is accepted.

**Step 4: Apply the same shape to `openai_responses.rs`**

Use the same helper pattern and keep retry bounded to the pre-stream HTTP phase only.

**Step 5: Add a focused failure-classification test for Responses**

Add:

```rust
#[test]
fn openai_responses_does_not_retry_non_429_errors() {
    assert!(!should_retry_pre_stream_response(
        reqwest::StatusCode::UNAUTHORIZED,
        "invalid api key",
        false,
    ));
}
```

**Step 6: Run targeted protocol tests**

Run: `cargo test --manifest-path ../tiy-core/Cargo.toml openai_ -- --nocapture`

Expected: Existing and new OpenAI protocol tests pass.

**Step 7: Commit**

```bash
git add ../tiy-core/src/protocol/openai_completions.rs ../tiy-core/src/protocol/openai_responses.rs
git commit -m "feat(core): retry openai requests on pre-stream 429"
```

### Task 4: Add the same retry behavior to Anthropic and Google protocols

**Files:**
- Modify: `../tiy-core/src/protocol/anthropic.rs`
- Modify: `../tiy-core/src/protocol/google.rs`
- Test: `../tiy-core/src/protocol/anthropic.rs`
- Test: `../tiy-core/src/protocol/google.rs`

**Step 1: Add the failing Anthropic test**

Introduce the same local helper and test:

```rust
#[test]
fn anthropic_pre_stream_retry_requires_429_and_no_output() {
    assert!(should_retry_pre_stream_response(
        reqwest::StatusCode::TOO_MANY_REQUESTS,
        "too many requests",
        false,
    ));
    assert!(!should_retry_pre_stream_response(
        reqwest::StatusCode::BAD_REQUEST,
        "validation failed",
        false,
    ));
}
```

**Step 2: Add the failing Google test**

```rust
#[test]
fn google_pre_stream_retry_stops_after_output_starts() {
    assert!(!should_retry_pre_stream_response(
        reqwest::StatusCode::TOO_MANY_REQUESTS,
        "rate limit",
        true,
    ));
}
```

**Step 3: Implement the retry loop in `anthropic.rs`**

Mirror the OpenAI task:

- read status, headers, and bounded error body
- retry only before `AssistantMessageEvent::Start`
- emit the retry notice before sleeping
- preserve existing terminal error behavior when retry is not allowed

**Step 4: Implement the retry loop in `google.rs`**

Use the same bounded loop and helper functions so provider behavior stays consistent.

**Step 5: Run targeted tests**

Run: `cargo test --manifest-path ../tiy-core/Cargo.toml "anthropic_pre_stream_retry|google_pre_stream_retry" -- --nocapture`

Expected: The new protocol tests pass.

**Step 6: Run a broader compile check**

Run: `cargo test --manifest-path ../tiy-core/Cargo.toml --no-run`

Expected: `tiy-core` compiles after all four protocols share the same retry shape.

**Step 7: Commit**

```bash
git add ../tiy-core/src/protocol/anthropic.rs ../tiy-core/src/protocol/google.rs
git commit -m "feat(core): retry anthropic and google requests on pre-stream 429"
```

### Task 5: Add desktop retry event plumbing from Rust to TypeScript

**Files:**
- Modify: `src-tauri/src/ipc/frontend_channels.rs`
- Modify: `src-tauri/src/core/agent_session.rs`
- Modify: `src/shared/types/api.ts`
- Modify: `src/services/bridge/agent-commands.ts`
- Modify: `src/services/thread-stream/thread-stream.ts`
- Test: `src-tauri/tests/m1_7_frontend_integration.rs`

**Step 1: Add the new Rust event variant**

Add:

```rust
RateLimitRetrying {
    run_id: String,
    attempt: u32,
    max_attempts: u32,
    delay_ms: u64,
    reason: String,
},
```

**Step 2: Forward retry notices from `AgentSession`**

Register the new `tiy-core` hook during agent configuration:

```rust
agent.set_on_rate_limit_retry({
    let event_tx = spec_event_tx.clone();
    let run_id = spec.run_id.clone();
    move |notice| {
        let event_tx = event_tx.clone();
        let run_id = run_id.clone();
        async move {
            let _ = event_tx.send(ThreadStreamEvent::RateLimitRetrying {
                run_id,
                attempt: notice.attempt,
                max_attempts: notice.max_attempts,
                delay_ms: notice.delay_ms,
                reason: notice.reason,
            });
        }
    }
});
```

**Step 3: Extend the shared TS event union**

Add:

```ts
| {
    type: "rate_limit_retrying";
    runId: string;
    attempt: number;
    maxAttempts: number;
    delayMs: number;
    reason: string;
  }
```

**Step 4: Extend bridge normalization**

Normalize camelCase and snake_case payloads in `src/services/bridge/agent-commands.ts`.

**Step 5: Add the ThreadStream callback**

In `src/services/thread-stream/thread-stream.ts`, add:

```ts
export type RetryNoticeEvent = {
  runId: string;
  attempt: number;
  maxAttempts: number;
  delayMs: number;
  reason: string;
};

onRetryNotice: ((event: RetryNoticeEvent) => void) | null = null;
```

Handle the new event type without calling `onError`.

**Step 6: Add the failing Rust serialization test**

In `src-tauri/tests/m1_7_frontend_integration.rs`:

```rust
#[test]
fn test_thread_stream_event_rate_limit_retrying_serialization() {
    let event = ThreadStreamEvent::RateLimitRetrying {
        run_id: "run-1".into(),
        attempt: 2,
        max_attempts: 4,
        delay_ms: 4000,
        reason: "HTTP 429".into(),
    };

    let json = serde_json::to_value(&event).unwrap();
    assert_eq!(json["type"].as_str().unwrap(), "rate_limit_retrying");
    assert_eq!(json["attempt"].as_u64().unwrap(), 2);
}
```

Also add this variant to the "all variants" coverage vector in the same test file.

**Step 7: Run targeted tests and typecheck**

Run: `cargo test --manifest-path src-tauri/Cargo.toml m1_7_frontend_integration -- --nocapture`

Expected: Rust-side event serialization passes.

Run: `npm run typecheck`

Expected: TypeScript event unions and adapters compile cleanly.

**Step 8: Commit**

```bash
git add src-tauri/src/ipc/frontend_channels.rs src-tauri/src/core/agent_session.rs src/shared/types/api.ts src/services/bridge/agent-commands.ts src/services/thread-stream/thread-stream.ts src-tauri/tests/m1_7_frontend_integration.rs
git commit -m "feat(runtime): add rate limit retry thread events"
```

### Task 6: Render the retry notice in the runtime thread UI

**Files:**
- Modify: `src/modules/workbench-shell/ui/runtime-thread-surface.tsx`
- Test: `src/modules/workbench-shell/ui/runtime-thread-surface.tsx`

**Step 1: Add local retry notice state**

Add a small local state near `runtimeError`:

```tsx
const [retryNotice, setRetryNotice] = useState<{
  attempt: number;
  maxAttempts: number;
  delayMs: number;
  reason: string;
} | null>(null);
```

**Step 2: Wire the new stream callback**

Inside the `ThreadStream` setup:

```tsx
stream.onRetryNotice = (event) => {
  setRetryNotice({
    attempt: event.attempt,
    maxAttempts: event.maxAttempts,
    delayMs: event.delayMs,
    reason: event.reason,
  });
};
```

Clear it on:

- `message_delta`
- `message_completed`
- `run_completed`
- `run_failed`
- `run_cancelled`
- `run_interrupted`

**Step 3: Render the banner**

Add a lightweight inline notice near the existing runtime status/error area:

```tsx
{retryNotice ? (
  <div className="rounded-md border border-amber-300/60 bg-amber-50 px-3 py-2 text-xs text-amber-900">
    {`Encountered rate limiting. Retrying ${retryNotice.attempt}/${retryNotice.maxAttempts} in about ${Math.ceil(retryNotice.delayMs / 1000)} seconds.`}
  </div>
) : null}
```

Keep it visually distinct from error styling.

**Step 4: Add a tiny pure formatter helper**

Extract the banner string to a helper so it is easy to test manually and reason about:

```tsx
function formatRetryNotice(delayMs: number, attempt: number, maxAttempts: number) {
  return `Encountered rate limiting. Retrying ${attempt}/${maxAttempts} in about ${Math.ceil(delayMs / 1000)} seconds.`;
}
```

**Step 5: Run typecheck**

Run: `npm run typecheck`

Expected: The runtime thread surface compiles with the new callback and state.

**Step 6: Manual verification**

Run the app and simulate a retry notice event or stub provider response.

Expected:

- the thread stays in running state
- the retry notice appears
- the notice disappears when output resumes
- terminal failures still render through the existing error path

**Step 7: Commit**

```bash
git add src/modules/workbench-shell/ui/runtime-thread-surface.tsx
git commit -m "feat(workbench): show rate limit retry notice"
```

### Task 7: Run end-to-end verification and tighten regressions

**Files:**
- Modify: `../tiy-core/src/protocol/common.rs`
- Modify: `src-tauri/tests/m1_5_agent_run.rs`
- Modify: `src-tauri/tests/m1_7_frontend_integration.rs`
- Modify: `docs/superpowers/specs/2026-03-20-llm-429-rate-limit-retry-design.md`

**Step 1: Add a desktop regression test shape**

In `src-tauri/tests/m1_5_agent_run.rs`, add a focused test around the new runtime semantics. The test should assert that retry notices do not transition the run into `failed` before the terminal outcome:

```rust
#[test]
fn rate_limit_retry_notice_is_non_terminal() {
    // construct a retrying event and assert the run-status mapping does not treat it as failed
}
```

If `m1_5_agent_run.rs` is a poor fit after inspection, move this exact assertion into the nearest run-manager-focused Rust test file instead of forcing it.

**Step 2: Run Rust desktop test suites**

Run: `cargo test --manifest-path src-tauri/Cargo.toml m1_5_agent_run m1_7_frontend_integration -- --nocapture`

Expected: Retry notice handling does not break run-state or event coverage tests.

**Step 3: Run full compile checks**

Run: `cargo test --manifest-path ../tiy-core/Cargo.toml --no-run`

Expected: `tiy-core` compiles.

Run: `cargo test --manifest-path src-tauri/Cargo.toml --no-run`

Expected: desktop Rust compiles.

Run: `npm run typecheck`

Expected: frontend TypeScript compiles.

**Step 4: Update the spec only if implementation drifted**

If the actual callback/type names differ from the spec, make a small sync edit to:

`docs/superpowers/specs/2026-03-20-llm-429-rate-limit-retry-design.md`

Do not expand scope. Only reconcile naming or file placement.

**Step 5: Final commit**

```bash
git add ../tiy-core src-tauri src/services src/shared src/modules docs/superpowers/specs/2026-03-20-llm-429-rate-limit-retry-design.md
git commit -m "feat(runtime): harden LLM 429 retry handling"
```
