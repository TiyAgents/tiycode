import { describe, expect, it, vi } from "vitest";

import { normalizeThreadStreamEvent } from "./agent-commands";
import type { RawThreadStreamEvent } from "./agent-commands";
import type { ThreadStreamEvent } from "@/shared/types/api";

function makeRawEvent(overrides: Record<string, unknown> = {}): RawThreadStreamEvent {
  return {
    type: "artifact_updated",
    runId: "run-1",
    run_id: "run-1",
    messageId: "msg-1",
    message_id: "msg-1",
    artifactId: "artifact-1",
    artifact_id: "artifact-1",
    artifactType: "chart",
    artifact_type: "chart",
    status: "started",
    payload: { library: "vega-lite", spec: { mark: "line" } },
    error: null,
    ...overrides,
  };
}

describe("normalizeThreadStreamEvent artifact_updated", () => {
  it("normalizes a valid artifact_updated event with all fields", () => {
    const result = normalizeThreadStreamEvent(makeRawEvent()) as Extract<
      ThreadStreamEvent,
      { type: "artifact_updated" }
    >;

    expect(result.type).toBe("artifact_updated");
    expect(result.runId).toBe("run-1");
    expect(result.messageId).toBe("msg-1");
    expect(result.artifactId).toBe("artifact-1");
    expect(result.artifactType).toBe("chart");
    expect(result.status).toBe("started");
    expect(result.payload).toEqual({ library: "vega-lite", spec: { mark: "line" } });
    expect(result.error).toBeUndefined();
  });

  it("falls back to snake_case keys when camelCase is missing", () => {
    const raw = makeRawEvent();
    delete raw.runId;
    delete raw.messageId;
    delete raw.artifactId;
    delete raw.artifactType;

    const result = normalizeThreadStreamEvent(raw) as Extract<
      ThreadStreamEvent,
      { type: "artifact_updated" }
    >;

    expect(result.runId).toBe("run-1");
    expect(result.messageId).toBe("msg-1");
    expect(result.artifactId).toBe("artifact-1");
    expect(result.artifactType).toBe("chart");
  });

  it("throws when a required field is missing entirely", () => {
    const raw = makeRawEvent();
    delete raw.runId;
    delete raw.run_id;

    expect(() => normalizeThreadStreamEvent(raw)).toThrow(/missing runId/);
  });

  it("maps null payload to undefined error", () => {
    const raw = makeRawEvent({ error: null, payload: null });
    const result = normalizeThreadStreamEvent(raw);

    expect(result).toMatchObject({ error: undefined });
  });

  it("maps a string error field", () => {
    const raw = makeRawEvent({ error: "rendering failed" });
    const result = normalizeThreadStreamEvent(raw) as Extract<
      ThreadStreamEvent,
      { type: "artifact_updated" }
    >;

    expect(result.error).toBe("rendering failed");
  });

  it("validates and maps known status strings", () => {
    for (const status of ["started", "delta", "completed", "failed"]) {
      const result = normalizeThreadStreamEvent(makeRawEvent({ status }));
      expect(result).toMatchObject({ type: "artifact_updated", status });
    }
  });

  it("falls back to 'completed' for unknown status values", () => {
    const warnSpy = vi.spyOn(console, "warn").mockImplementation(() => {});
    const raw = makeRawEvent({ status: "unknown_status" });

    const result = normalizeThreadStreamEvent(raw) as Extract<
      ThreadStreamEvent,
      { type: "artifact_updated" }
    >;

    expect(result.status).toBe("completed");
    expect(warnSpy).toHaveBeenCalledWith(
      expect.stringContaining('Unknown artifact status "unknown_status"'),
    );

    warnSpy.mockRestore();
  });
});

describe("normalizeThreadStreamEvent queue_updated", () => {
  it("normalizes a snake_case runtime queue snapshot", () => {
    const result = normalizeThreadStreamEvent({
      type: "queue_updated",
      run_id: "run-1",
      queue: {
        steering_depth: 1,
        follow_up_depth: 2,
        is_deferring_steering: true,
        messages: [
          {
            id: "q-1",
            kind: "follow_up",
            content: "Next step",
            status: "pending",
            created_at: "2026-05-20T00:00:00Z",
            updated_at: "2026-05-20T00:00:01Z",
          },
        ],
        events: [
          {
            id: "evt-1",
            kind: "follow_up",
            action: "enqueued",
            count: 1,
            queue_depth: 2,
            created_at: "2026-05-20T00:00:01Z",
          },
        ],
      },
    }) as Extract<ThreadStreamEvent, { type: "queue_updated" }>;

    expect(result.runId).toBe("run-1");
    expect(result.queue).toEqual({
      steeringDepth: 1,
      followUpDepth: 2,
      isDeferringSteering: true,
      messages: [
        {
          id: "q-1",
          kind: "follow_up",
          content: "Next step",
          status: "pending",
          createdAt: "2026-05-20T00:00:00Z",
          updatedAt: "2026-05-20T00:00:01Z",
        },
      ],
      events: [
        {
          id: "evt-1",
          kind: "follow_up",
          action: "enqueued",
          count: 1,
          queueDepth: 2,
          remaining: undefined,
          countDropped: undefined,
          createdAt: "2026-05-20T00:00:01Z",
        },
      ],
    });
  });
});

describe("normalizeThreadStreamEvent user_message_recorded", () => {
  it("normalizes a snake_case user message event", () => {
    const result = normalizeThreadStreamEvent({
      type: "user_message_recorded",
      run_id: "run-1",
      message_id: "msg-user-1",
      content: "Use the simpler approach",
      created_at: "2026-05-20T00:00:02Z",
    }) as Extract<ThreadStreamEvent, { type: "user_message_recorded" }>;

    expect(result).toEqual({
      type: "user_message_recorded",
      runId: "run-1",
      messageId: "msg-user-1",
      content: "Use the simpler approach",
      createdAt: "2026-05-20T00:00:02Z",
    });
  });
});

describe("normalizeThreadStreamEvent request_retrying", () => {
  it("normalizes request_retrying with snake_case retry fields", () => {
    const result = normalizeThreadStreamEvent({
      type: "request_retrying",
      run_id: "run-1",
      attempt: 2,
      max_retries: 5,
      delay_ms: 750,
      reason: "stream disconnected",
      status: 503,
    }) as Extract<ThreadStreamEvent, { type: "request_retrying" }>;

    expect(result).toEqual({
      type: "request_retrying",
      runId: "run-1",
      attempt: 2,
      maxRetries: 5,
      delayMs: 750,
      reason: "stream disconnected",
      status: 503,
    });
  });

  it("normalizes missing request_retrying status to null", () => {
    const result = normalizeThreadStreamEvent({
      type: "request_retrying",
      runId: "run-1",
      attempt: 1,
      maxRetries: 3,
      delayMs: 500,
      reason: "operation timed out",
    }) as Extract<ThreadStreamEvent, { type: "request_retrying" }>;

    expect(result.status).toBeNull();
  });

  it("normalizes malformed request_retrying status to null", () => {
    const result = normalizeThreadStreamEvent({
      type: "request_retrying",
      runId: "run-1",
      attempt: 1,
      maxRetries: 3,
      delayMs: 500,
      reason: "operation timed out",
      status: "not-a-number",
    }) as Extract<ThreadStreamEvent, { type: "request_retrying" }>;

    expect(result.status).toBeNull();
  });
});
