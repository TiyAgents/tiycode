import { describe, expect, it, beforeEach, vi } from "vitest";

const { invokeMock, isTauriMock } = vi.hoisted(() => ({
  invokeMock: vi.fn(),
  isTauriMock: vi.fn(),
}));

vi.mock("@tauri-apps/api/core", () => ({
  invoke: invokeMock,
  isTauri: isTauriMock,
  Channel: class MockChannel<T> {
    onmessage: ((event: T) => void) | null = null;
  },
}));

import {
  normalizeThreadStreamEvent,
  threadPromoteRuntimeQueueMessage,
  goalGetState,
  goalSet,
  goalPause,
  goalResume,
  goalClear,
  goalEvaluate,
} from "./agent-commands";
import type { RawThreadStreamEvent } from "./agent-commands";
import type { ThreadStreamEvent } from "@/shared/types/api";
import type { GoalPayload, GoalEvaluateResult } from "./agent-commands";

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

describe("threadPromoteRuntimeQueueMessage", () => {
  beforeEach(() => {
    invokeMock.mockReset();
    isTauriMock.mockReset();
  });

  it("invokes the promote command and normalizes the queue snapshot", async () => {
    isTauriMock.mockReturnValue(true);
    invokeMock.mockResolvedValueOnce({
      steering_depth: 1,
      follow_up_depth: 0,
      is_deferring_steering: false,
      messages: [
        {
          id: "queue-message-1",
          kind: "steer",
          content: "Handle this now",
          status: "pending",
          created_at: "2026-05-20T00:00:00Z",
          updated_at: "2026-05-20T00:00:01Z",
        },
      ],
      events: [
        {
          id: "event-1",
          kind: "steer",
          action: "transferred",
          count: 1,
          queue_depth: 1,
          remaining: 0,
          created_at: "2026-05-20T00:00:01Z",
        },
      ],
    });

    await expect(threadPromoteRuntimeQueueMessage("thread-1", "queue-message-1")).resolves.toMatchObject({
      steeringDepth: 1,
      followUpDepth: 0,
      messages: [expect.objectContaining({ id: "queue-message-1", kind: "steer" })],
      events: [expect.objectContaining({ id: "event-1", action: "transferred" })],
    });
    expect(invokeMock).toHaveBeenCalledWith("thread_promote_runtime_queue_message", {
      threadId: "thread-1",
      messageId: "queue-message-1",
    });
  });

  it("requires Tauri runtime", async () => {
    isTauriMock.mockReturnValue(false);

    await expect(threadPromoteRuntimeQueueMessage("thread-1", "queue-message-1"))
      .rejects.toThrow("thread_promote_runtime_queue_message requires Tauri runtime");
    expect(invokeMock).not.toHaveBeenCalled();
  });

  it("propagates invoke rejections", async () => {
    isTauriMock.mockReturnValue(true);
    invokeMock.mockRejectedValueOnce(new Error("promote failed"));

    await expect(threadPromoteRuntimeQueueMessage("thread-1", "queue-message-1"))
      .rejects.toThrow("promote failed");
  });
});

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
          metadata: null,
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
      metadata: null,
    });
  });

  it("normalizes command metadata on a consumed queue user message event", () => {
    const metadata = {
      composer: {
        kind: "command",
        displayText: "/init",
        effectivePrompt: "Generate or update AGENTS.md",
      },
    };
    const result = normalizeThreadStreamEvent({
      type: "user_message_recorded",
      run_id: "run-1",
      message_id: "msg-user-1",
      content: "/init",
      created_at: "2026-05-20T00:00:02Z",
      metadata,
    }) as Extract<ThreadStreamEvent, { type: "user_message_recorded" }>;

    expect(result).toEqual({
      type: "user_message_recorded",
      runId: "run-1",
      messageId: "msg-user-1",
      content: "/init",
      createdAt: "2026-05-20T00:00:02Z",
      metadata,
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

// ── #2: Goal bridge tests ──

function makeGoalPayload(overrides: Partial<GoalPayload> = {}): GoalPayload {
  return {
    id: "goal-1",
    threadId: "thread-1",
    objective: "Build a todo app",
    status: "active",
    tokensUsed: 0,
    timeUsedSeconds: 0,
    turnsUsed: 0,
    maxTurns: 50,
    tokenBudget: null,
    pauseReason: null,
    pauseDetail: null,
    evidence: null,
    lastEvaluatedRunId: null,
    ...overrides,
  };
}

function makeGoalEvaluateResult(
  overrides: Partial<GoalEvaluateResult> = {},
): GoalEvaluateResult {
  return {
    goal: makeGoalPayload(),
    verdict: "continue",
    continuationPrompt: null,
    ...overrides,
  };
}

describe("goalGetState", () => {
  beforeEach(() => {
    invokeMock.mockReset();
    isTauriMock.mockReset();
  });

  it("invokes goal_get_state and returns GoalPayload", async () => {
    isTauriMock.mockReturnValue(true);
    const payload = makeGoalPayload();
    invokeMock.mockResolvedValueOnce(payload);

    const result = await goalGetState("thread-1");
    expect(result).toEqual(payload);
    expect(invokeMock).toHaveBeenCalledWith("goal_get_state", { threadId: "thread-1" });
  });

  it("returns null when no active goal exists", async () => {
    isTauriMock.mockReturnValue(true);
    invokeMock.mockResolvedValueOnce(null);

    const result = await goalGetState("thread-1");
    expect(result).toBeNull();
  });

  it("requires Tauri runtime", async () => {
    isTauriMock.mockReturnValue(false);

    await expect(goalGetState("thread-1")).rejects.toThrow(
      "goal_get_state requires Tauri runtime",
    );
    expect(invokeMock).not.toHaveBeenCalled();
  });
});

describe("goalSet", () => {
  beforeEach(() => {
    invokeMock.mockReset();
    isTauriMock.mockReset();
  });

  it("invokes goal_set with required params", async () => {
    isTauriMock.mockReturnValue(true);
    const payload = makeGoalPayload({ objective: "Build feature X" });
    invokeMock.mockResolvedValueOnce(payload);

    const result = await goalSet("thread-1", "Build feature X");
    expect(result).toEqual(payload);
    expect(invokeMock).toHaveBeenCalledWith("goal_set", {
      threadId: "thread-1",
      objective: "Build feature X",
      tokenBudget: undefined,
    });
  });

  it("invokes goal_set with optional tokenBudget", async () => {
    isTauriMock.mockReturnValue(true);
    const payload = makeGoalPayload({ tokenBudget: 10000 });
    invokeMock.mockResolvedValueOnce(payload);

    const result = await goalSet("thread-1", "Build feature X", 10000);
    expect(result.tokenBudget).toBe(10000);
    expect(invokeMock).toHaveBeenCalledWith("goal_set", {
      threadId: "thread-1",
      objective: "Build feature X",
      tokenBudget: 10000,
    });
  });

  it("requires Tauri runtime", async () => {
    isTauriMock.mockReturnValue(false);

    await expect(goalSet("thread-1", "obj")).rejects.toThrow(
      "goal_set requires Tauri runtime",
    );
    expect(invokeMock).not.toHaveBeenCalled();
  });
});

describe("goalPause", () => {
  beforeEach(() => {
    invokeMock.mockReset();
    isTauriMock.mockReset();
  });

  it("invokes goal_pause and returns GoalPayload", async () => {
    isTauriMock.mockReturnValue(true);
    const payload = makeGoalPayload({ status: "paused", pauseReason: "user_requested" });
    invokeMock.mockResolvedValueOnce(payload);

    const result = await goalPause("thread-1");
    expect(result).toEqual(payload);
    expect(invokeMock).toHaveBeenCalledWith("goal_pause", { threadId: "thread-1" });
  });

  it("returns null when no active goal to pause", async () => {
    isTauriMock.mockReturnValue(true);
    invokeMock.mockResolvedValueOnce(null);

    const result = await goalPause("thread-1");
    expect(result).toBeNull();
  });

  it("requires Tauri runtime", async () => {
    isTauriMock.mockReturnValue(false);

    await expect(goalPause("thread-1")).rejects.toThrow(
      "goal_pause requires Tauri runtime",
    );
    expect(invokeMock).not.toHaveBeenCalled();
  });
});

describe("goalResume", () => {
  beforeEach(() => {
    invokeMock.mockReset();
    isTauriMock.mockReset();
  });

  it("invokes goal_resume and returns GoalPayload", async () => {
    isTauriMock.mockReturnValue(true);
    const payload = makeGoalPayload({ status: "active" });
    invokeMock.mockResolvedValueOnce(payload);

    const result = await goalResume("thread-1");
    expect(result).toEqual(payload);
    expect(invokeMock).toHaveBeenCalledWith("goal_resume", { threadId: "thread-1" });
  });

  it("returns null when no paused goal exists", async () => {
    isTauriMock.mockReturnValue(true);
    invokeMock.mockResolvedValueOnce(null);

    const result = await goalResume("thread-1");
    expect(result).toBeNull();
  });

  it("requires Tauri runtime", async () => {
    isTauriMock.mockReturnValue(false);

    await expect(goalResume("thread-1")).rejects.toThrow(
      "goal_resume requires Tauri runtime",
    );
    expect(invokeMock).not.toHaveBeenCalled();
  });
});

describe("goalClear", () => {
  beforeEach(() => {
    invokeMock.mockReset();
    isTauriMock.mockReset();
  });

  it("invokes goal_clear and returns true", async () => {
    isTauriMock.mockReturnValue(true);
    invokeMock.mockResolvedValueOnce(true);

    const result = await goalClear("thread-1");
    expect(result).toBe(true);
    expect(invokeMock).toHaveBeenCalledWith("goal_clear", { threadId: "thread-1" });
  });

  it("returns false when no goal to clear", async () => {
    isTauriMock.mockReturnValue(true);
    invokeMock.mockResolvedValueOnce(false);

    const result = await goalClear("thread-1");
    expect(result).toBe(false);
  });

  it("requires Tauri runtime", async () => {
    isTauriMock.mockReturnValue(false);

    await expect(goalClear("thread-1")).rejects.toThrow(
      "goal_clear requires Tauri runtime",
    );
    expect(invokeMock).not.toHaveBeenCalled();
  });
});

describe("goalEvaluate", () => {
  beforeEach(() => {
    invokeMock.mockReset();
    isTauriMock.mockReset();
  });

  it("invokes goal_evaluate with required params", async () => {
    isTauriMock.mockReturnValue(true);
    const result = makeGoalEvaluateResult({ verdict: "continue" });
    invokeMock.mockResolvedValueOnce(result);

    const outcome = await goalEvaluate("thread-1");
    expect(outcome).toEqual(result);
    expect(invokeMock).toHaveBeenCalledWith("goal_evaluate", {
      threadId: "thread-1",
      response: undefined,
    });
  });

  it("invokes goal_evaluate with optional response", async () => {
    isTauriMock.mockReturnValue(true);
    const result = makeGoalEvaluateResult({ verdict: "challenge_evidence" });
    invokeMock.mockResolvedValueOnce(result);

    const outcome = await goalEvaluate("thread-1", "Some progress");
    expect(outcome!.verdict).toBe("challenge_evidence");
    expect(invokeMock).toHaveBeenCalledWith("goal_evaluate", {
      threadId: "thread-1",
      response: "Some progress",
    });
  });

  it("returns null when no active goal exists", async () => {
    isTauriMock.mockReturnValue(true);
    invokeMock.mockResolvedValueOnce(null);

    const result = await goalEvaluate("thread-1");
    expect(result).toBeNull();
  });

  it("passes through the skipped verdict for already-accepted goals", async () => {
    isTauriMock.mockReturnValue(true);
    const result = makeGoalEvaluateResult({ verdict: "skipped", continuationPrompt: null });
    invokeMock.mockResolvedValueOnce(result);

    const outcome = await goalEvaluate("thread-1");
    expect(outcome!.verdict).toBe("skipped");
    expect(outcome!.continuationPrompt).toBeNull();
  });

  it("requires Tauri runtime", async () => {
    isTauriMock.mockReturnValue(false);

    await expect(goalEvaluate("thread-1")).rejects.toThrow(
      "goal_evaluate requires Tauri runtime",
    );
    expect(invokeMock).not.toHaveBeenCalled();
  });
});
