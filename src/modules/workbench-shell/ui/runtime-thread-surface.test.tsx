import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";

import { createMachine } from "@/shared/lib/create-machine";
import { mapSnapshotToRunState, isTaskBoardTool, getDefaultToolOpenState } from "./runtime-thread-surface-logic";
import { LongMessageBody, shouldRenderTextPartAsPlainText } from "./long-message-body";
import { mapMessageParts, mapSnapshotMessage, mergeSnapshotMessages, mergeSnapshotTools } from "./runtime-thread-surface-state";
import type { SurfaceMessage } from "./runtime-thread-surface-state";
import type { MessageDto, RunStatus, ThreadSnapshotDto } from "@/shared/types/api";

function makeMessage(overrides: Partial<MessageDto> = {}): MessageDto {
  return {
    attachments: [],
    contentMarkdown: "legacy markdown body",
    createdAt: "2026-05-06T00:00:00Z",
    id: "message-1",
    messageType: "plain_message",
    metadata: null,
    parts: null,
    role: "assistant",
    runId: "run-1",
    status: "completed",
    threadId: "thread-1",
    ...overrides,
  };
}

function makeSnapshot(activeStatus: RunStatus | null): ThreadSnapshotDto {
  return {
    thread: {
      id: "thread-1",
      workspaceId: "workspace-1",
      profileId: null,
      title: "Test thread",
      status: activeStatus ? "running" : "idle",
      lastActiveAt: "2026-04-22T00:00:00Z",
      createdAt: "2026-04-22T00:00:00Z",
    },
    messages: [],
    hasMoreMessages: false,
    activeRun: activeStatus
      ? {
          id: "run-1",
          threadId: "thread-1",
          runMode: "default",
          status: activeStatus,
          modelId: null,
          modelDisplayName: null,
          contextWindow: null,
          errorMessage: null,
          startedAt: "2026-04-22T00:00:00Z",
          usage: {
            inputTokens: 0,
            outputTokens: 0,
            cacheReadTokens: 0,
            cacheWriteTokens: 0,
            totalTokens: 0,
          },
        }
      : null,
    latestRun: null,
    toolCalls: [],
    helpers: [],
    taskBoards: [],
    activeTaskBoardId: null,
  };
}

type TestSurfaceTool = Parameters<typeof mergeSnapshotTools>[0][number];

function makeTool(overrides: Partial<TestSurfaceTool>): TestSurfaceTool {
  return {
    id: "tool-1",
    name: "read",
    runId: "run-1",
    startedAt: "2026-05-06T00:00:00Z",
    state: "output-available",
    ...overrides,
  };
}

function t(key: string, params?: Record<string, unknown>) {
  if (params && "count" in params) {
    return `${key}:${params.count}`;
  }

  return key;
}

describe("message text rendering policy", () => {
  it("renders user text parts as plain text so HTML stays inert", () => {
    expect(shouldRenderTextPartAsPlainText("user")).toBe(true);
  });

  it("keeps assistant and system text parts on the rich Markdown path", () => {
    expect(shouldRenderTextPartAsPlainText("assistant")).toBe(false);
    expect(shouldRenderTextPartAsPlainText("system")).toBe(false);
  });

  it("renders user message HTML as escaped text without creating image nodes", () => {
    const html = renderToStaticMarkup(
      <LongMessageBody
        message={{
          content: "<img src=x onerror=alert(1)>\n<script>alert(1)</script>",
          id: "user-message-1",
          messageType: "plain_message",
          parts: [{ type: "text", text: "<img src=x onerror=alert(1)>\n<script>alert(1)</script>" }],
          role: "user",
          status: "completed",
        }}
        t={t as never}
      />,
    );

    expect(html).toContain("&lt;img src=x onerror=alert(1)&gt;");
    expect(html).toContain("&lt;script&gt;alert(1)&lt;/script&gt;");
    expect(html).not.toContain("<img");
    expect(html).not.toContain("<script");
  });

  it("keeps assistant messages on the Markdown rendering path", () => {
    const html = renderToStaticMarkup(
      <LongMessageBody
        message={{
          content: "**bold**",
          id: "assistant-message-1",
          messageType: "plain_message",
          parts: [{ type: "text", text: "**bold**" }],
          role: "assistant",
          status: "completed",
        }}
        t={t as never}
      />,
    );

    expect(html).toContain('data-streamdown="strong"');
    expect(html).toContain(">bold</span>");
    expect(html).not.toContain("**bold**");
  });
});

describe("mapMessageParts", () => {
  it("falls back to a single text part for legacy markdown-only messages", () => {
    expect(mapMessageParts(null, "legacy markdown")).toEqual([{ type: "text", text: "legacy markdown" }]);
  });

  it("maps chart and text parts without losing order", () => {
    const result = mapMessageParts([
      { type: "text", text: "intro" },
      { type: "chart", artifactId: "chart-1", library: "vega-lite", spec: { mark: "line" }, title: "Demo", caption: "Chart caption" },
    ], "ignored");

    expect(result).toHaveLength(2);
    expect(result[0]).toEqual({ type: "text", text: "intro" });
    expect(result[1]).toMatchObject({ type: "chart", artifactId: "chart-1", library: "vega-lite", title: "Demo", caption: "Chart caption" });
  });

  it("preserves unknown parts as safe fallback values", () => {
    const result = mapMessageParts([{ type: "artifact-x", foo: "bar" }], "ignored");
    expect(result[0]).toEqual({ type: "artifact-x", value: { type: "artifact-x", foo: "bar" } });
  });
});

describe("mapSnapshotMessage", () => {
  it("prefers structured parts when both parts and legacy markdown are present", () => {
    const message = mapSnapshotMessage(makeMessage({
      contentMarkdown: "legacy body",
      parts: [{ type: "text", text: "structured body" }],
    }));

    expect(message.content).toBe("legacy body");
    expect(message.parts).toEqual([{ type: "text", text: "structured body" }]);
  });
});

describe("mapSnapshotToRunState", () => {
  it("treats cancelling snapshots as still running (aligned with backend derive_thread_status)", () => {
    expect(mapSnapshotToRunState(makeSnapshot("cancelling"))).toBe("running");
  });

  it("still keeps waiting_tool_result snapshots in running state", () => {
    expect(mapSnapshotToRunState(makeSnapshot("waiting_tool_result"))).toBe("running");
  });

  it("maps approval and reply states directly from the active run", () => {
    expect(mapSnapshotToRunState(makeSnapshot("waiting_approval"))).toBe("waiting_approval");
    expect(mapSnapshotToRunState(makeSnapshot("needs_reply"))).toBe("needs_reply");
  });

  it("maps failed, interrupted, and limit states from the active run", () => {
    expect(mapSnapshotToRunState(makeSnapshot("failed"))).toBe("failed");
    expect(mapSnapshotToRunState(makeSnapshot("interrupted"))).toBe("interrupted");
    expect(mapSnapshotToRunState(makeSnapshot("limit_reached"))).toBe("limit_reached");
  });

  it("falls back to completed when there is no active run", () => {
    expect(mapSnapshotToRunState(makeSnapshot(null))).toBe("completed");
  });
});

describe("isTaskBoardTool", () => {
  it("returns true for task board tool names", () => {
    expect(isTaskBoardTool("create_task")).toBe(true);
    expect(isTaskBoardTool("update_task")).toBe(true);
    expect(isTaskBoardTool("query_task")).toBe(true);
  });

  it("returns false for non-task tool names", () => {
    expect(isTaskBoardTool("read")).toBe(false);
    expect(isTaskBoardTool("edit")).toBe(false);
    expect(isTaskBoardTool("shell")).toBe(false);
    expect(isTaskBoardTool("agent_explore")).toBe(false);
    expect(isTaskBoardTool("update_plan")).toBe(false);
  });

  it("returns false for empty and edge-case strings", () => {
    expect(isTaskBoardTool("")).toBe(false);
    expect(isTaskBoardTool("create_task_extra")).toBe(false);
    expect(isTaskBoardTool("CREATE_TASK")).toBe(false);
  });
});

describe("mergeSnapshotTools", () => {
  it("keeps the more advanced live terminal state when merging stale snapshots", () => {
    const deniedSnapshot = makeTool({ state: "output-denied", result: "denied snapshot" });
    const errorLive = makeTool({ state: "output-error", error: "live error" });

    expect(mergeSnapshotTools([deniedSnapshot], [errorLive])[0]).toBe(errorLive);

    const errorSnapshot = makeTool({ state: "output-error", error: "snapshot error" });
    const availableLive = makeTool({ state: "output-available", result: "live result" });

    expect(mergeSnapshotTools([errorSnapshot], [availableLive])[0]).toBe(availableLive);
  });

  it("keeps the snapshot when a live terminal state is less advanced", () => {
    const availableSnapshot = makeTool({ state: "output-available", result: "snapshot result" });
    const deniedLive = makeTool({ state: "output-denied", result: "denied live" });

    expect(mergeSnapshotTools([availableSnapshot], [deniedLive])[0]).toBe(availableSnapshot);
  });

  it("keeps the snapshot when both snapshot and live have equal state", () => {
    const snapshotTool = makeTool({ state: "output-available", result: "snapshot result" });
    const liveTool = makeTool({ state: "output-available", result: "live result" });

    expect(mergeSnapshotTools([snapshotTool], [liveTool])[0]).toBe(snapshotTool);
  });

  it("advances from non-terminal states when live is more advanced", () => {
    const snapshotTool = makeTool({ state: "input-streaming" });
    const liveTool = makeTool({ state: "input-available" });

    expect(mergeSnapshotTools([snapshotTool], [liveTool])[0]).toBe(liveTool);
  });

  it("appends live-only tools that are absent from the snapshot", () => {
    const snapshotTool = makeTool({ id: "tool-1", state: "output-available" });
    const liveTool = makeTool({ id: "tool-2", state: "input-available" });

    const result = mergeSnapshotTools([snapshotTool], [liveTool]);
    expect(result).toHaveLength(2);
    expect(result[0]).toBe(snapshotTool);
    expect(result[1]).toBe(liveTool);
  });

  it("returns snapshot tools unchanged when live tools array is empty", () => {
    const snapshotTool = makeTool({ state: "output-available" });
    const result = mergeSnapshotTools([snapshotTool], []);
    expect(result).toEqual([snapshotTool]);
  });
});

describe("getDefaultToolOpenState", () => {
  it("defaults task board tools to collapsed", () => {
    expect(getDefaultToolOpenState("create_task", "input-available", undefined)).toBe(false);
    expect(getDefaultToolOpenState("update_task", "output-available", undefined)).toBe(false);
    expect(getDefaultToolOpenState("query_task", "input-streaming", undefined)).toBe(false);
    expect(getDefaultToolOpenState("render", "output-available", undefined)).toBe(false);
  });

  it("respects explicit open state for task board tools", () => {
    expect(getDefaultToolOpenState("create_task", "output-available", true)).toBe(true);
    expect(getDefaultToolOpenState("update_task", "output-available", false)).toBe(false);
  });

  it("defaults non-task running tools to expanded", () => {
    expect(getDefaultToolOpenState("read", "input-available", undefined)).toBe(true);
    expect(getDefaultToolOpenState("shell", "input-streaming", undefined)).toBe(true);
  });

  it("force-expands non-task running tools even with explicit false", () => {
    expect(getDefaultToolOpenState("read", "input-available", false)).toBe(true);
  });

  it("defaults non-task completed tools to expanded", () => {
    expect(getDefaultToolOpenState("read", "output-available", undefined)).toBe(true);
    expect(getDefaultToolOpenState("edit", "output-error", undefined)).toBe(true);
  });

  it("respects explicit open state for non-task completed tools", () => {
    expect(getDefaultToolOpenState("read", "output-available", false)).toBe(false);
    expect(getDefaultToolOpenState("edit", "output-available", true)).toBe(true);
  });
});

function makeSurfaceMessage(overrides: Partial<SurfaceMessage> = {}): SurfaceMessage {
  return {
    createdAt: "2026-05-06T00:00:00Z",
    id: "msg-1",
    messageType: "plain_message",
    attachments: [],
    role: "assistant",
    runId: "run-1",
    content: "hello",
    parts: [{ type: "text", text: "hello" }],
    status: "completed",
    ...overrides,
  };
}

describe("mergeSnapshotMessages", () => {
  it("returns snapshot messages when current messages is empty", () => {
    const snapshot = [makeSurfaceMessage({ id: "msg-1" })];
    const result = mergeSnapshotMessages(snapshot, [], null);
    expect(result.messages).toBe(snapshot);
  });

  it("keeps local message when local status is more advanced", () => {
    const snapshotMsg = makeSurfaceMessage({ id: "msg-1", status: "streaming", content: "partial" });
    const localMsg = makeSurfaceMessage({ id: "msg-1", status: "completed", content: "full" });
    const result = mergeSnapshotMessages([snapshotMsg], [localMsg], null);
    expect(result.messages[0]).toBe(localMsg);
  });

  it("keeps local assistant message when it has richer parts than snapshot", () => {
    const snapshotMsg = makeSurfaceMessage({
      id: "msg-1",
      role: "assistant",
      status: "completed",
      content: "hello",
      parts: [{ type: "text", text: "hello" }],
    });
    const localMsg = makeSurfaceMessage({
      id: "msg-1",
      role: "assistant",
      status: "completed",
      content: "hello",
      parts: [
        { type: "text", text: "hello" },
        { type: "chart", artifactId: "chart-1", library: "vega-lite", spec: {}, source: null, title: null, caption: null, status: "ready", error: null },
      ],
    });
    const result = mergeSnapshotMessages([snapshotMsg], [localMsg], null);
    expect(result.messages[0]).toBe(localMsg);
  });

  it("keeps snapshot when local has fewer parts", () => {
    const snapshotMsg = makeSurfaceMessage({
      id: "msg-1",
      role: "assistant",
      status: "completed",
      content: "hello",
      parts: [
        { type: "text", text: "hello" },
        { type: "chart", artifactId: "chart-1", library: "vega-lite", spec: {}, source: null, title: null, caption: null, status: "ready", error: null },
      ],
    });
    const localMsg = makeSurfaceMessage({
      id: "msg-1",
      role: "assistant",
      status: "completed",
      content: "hello",
      parts: [{ type: "text", text: "hello" }],
    });
    const result = mergeSnapshotMessages([snapshotMsg], [localMsg], null);
    expect(result.messages[0]).toBe(snapshotMsg);
  });

  it("keeps local message when snapshot has empty content but local has content", () => {
    const snapshotMsg = makeSurfaceMessage({
      id: "msg-1",
      role: "assistant",
      status: "completed",
      content: "",
      parts: [],
    });
    const localMsg = makeSurfaceMessage({
      id: "msg-1",
      role: "assistant",
      status: "completed",
      content: "hello world",
      parts: [{ type: "text", text: "hello world" }],
    });
    const result = mergeSnapshotMessages([snapshotMsg], [localMsg], null);
    expect(result.messages[0]).toBe(localMsg);
  });

  it("appends local streaming assistant not yet in snapshot", () => {
    const snapshotMsg = makeSurfaceMessage({ id: "msg-1" });
    const localStreamMsg = makeSurfaceMessage({
      id: "msg-2",
      role: "assistant",
      status: "streaming",
      content: "partial",
    });
    const result = mergeSnapshotMessages([snapshotMsg], [snapshotMsg, localStreamMsg], null);
    expect(result.messages).toHaveLength(2);
    expect(result.messages[1]).toBe(localStreamMsg);
  });

  it("resolves optimistic user message when backend has persisted it", () => {
    const persistedMsg = makeSurfaceMessage({
      id: "backend-id-1",
      role: "user",
      content: "user input",
      parts: [{ type: "text", text: "user input" }],
    });
    const optimisticMsg = makeSurfaceMessage({
      id: "local-user-1",
      role: "user",
      content: "user input",
      parts: [{ type: "text", text: "user input" }],
    });
    const result = mergeSnapshotMessages([persistedMsg], [optimisticMsg], "local-user-1");
    expect(result.messages[0].id).toBe("local-user-1");
    expect(result.lastOptimisticUserId).toBeNull();
  });
});

/**
 * Tests for the snapshot-loading event buffering and replay pattern.
 *
 * The loadSnapshot code does:
 * 1. Set snapshotLoadingRef = true, clear eventBuffer
 * 2. Await snapshot from IPC
 * 3. runMachine.reset(snapshotState)
 * 4. Replay buffered events via runMachine.send()
 * 5. Clear buffer, set snapshotLoadingRef = false
 *
 * While step 2 is in flight, stream events are pushed into the buffer
 * instead of being sent to the machine. This test suite validates that
 * the pattern produces correct state after reset + replay.
 */
describe("snapshot loading event buffer and replay", () => {
  // We use createMachine directly to simulate the run-lifecycle machine behavior.
  // This mirrors the exact pattern used in runtime-thread-surface.tsx.
  function createTestMachine(initial: string = "idle") {
    return createMachine<
      "idle" | "running" | "waiting_approval" | "completed" | "failed",
      "RUN_STARTED" | "APPROVAL_REQUIRED" | "RUN_COMPLETED" | "RUN_FAILED",
      { runId: string | null }
    >({
      initial: initial as "idle",
      context: { runId: null },
      states: {
        idle: {
          on: {
            RUN_STARTED: {
              target: "running",
              action: (_ctx, payload) => ({
                runId: (payload as { runId?: string })?.runId ?? null,
              }),
            },
          },
        },
        running: {
          on: {
            APPROVAL_REQUIRED: "waiting_approval",
            RUN_COMPLETED: "completed",
            RUN_FAILED: "failed",
          },
        },
        waiting_approval: {
          on: {
            RUN_COMPLETED: "completed",
            RUN_FAILED: "failed",
          },
        },
        completed: {
          on: {
            RUN_STARTED: {
              target: "running",
              action: (_ctx, payload) => ({
                runId: (payload as { runId?: string })?.runId ?? null,
              }),
            },
          },
        },
        failed: {
          on: {
            RUN_STARTED: {
              target: "running",
              action: (_ctx, payload) => ({
                runId: (payload as { runId?: string })?.runId ?? null,
              }),
            },
          },
        },
      },
    });
  }

  type BufferedEvent = {
    event: "RUN_STARTED" | "APPROVAL_REQUIRED" | "RUN_COMPLETED" | "RUN_FAILED";
    payload?: { runId?: string };
  };

  it("replays buffered events in order after machine reset", () => {
    const machine = createTestMachine("idle");
    const buffer: BufferedEvent[] = [];

    // Simulate: snapshot loading starts, machine is in idle.
    // While loading, these stream events arrive and get buffered:
    buffer.push({ event: "RUN_STARTED", payload: { runId: "run-1" } });
    buffer.push({ event: "APPROVAL_REQUIRED" });

    // Snapshot returns — reset to "running" (snapshot state)
    machine.reset("running", { runId: "run-1" });
    expect(machine.getState()).toBe("running");

    // Replay buffered events
    for (const { event, payload } of buffer) {
      machine.send(event, payload);
    }

    // RUN_STARTED from running is invalid (ignored), but APPROVAL_REQUIRED
    // transitions running → waiting_approval.
    expect(machine.getState()).toBe("waiting_approval");
  });

  it("naturally rejects invalid transitions during replay", () => {
    const machine = createTestMachine("idle");
    const buffer: BufferedEvent[] = [];

    // Events buffered while snapshot was loading
    buffer.push({ event: "RUN_STARTED", payload: { runId: "run-1" } });
    buffer.push({ event: "RUN_COMPLETED" });

    // Snapshot returns as "completed" (run already finished)
    machine.reset("completed", { runId: "run-1" });
    expect(machine.getState()).toBe("completed");

    // Replay: RUN_STARTED would re-enter running, RUN_COMPLETED would go back
    // to completed. The machine handles this gracefully.
    for (const { event, payload } of buffer) {
      machine.send(event, payload);
    }

    // RUN_STARTED transitions completed → running, RUN_COMPLETED → completed
    expect(machine.getState()).toBe("completed");
  });

  it("preserves forward transition that arrived during snapshot IPC", () => {
    const machine = createTestMachine("idle");
    const buffer: BufferedEvent[] = [];

    // A forward transition arrives during the IPC round-trip
    buffer.push({ event: "RUN_COMPLETED" });

    // Snapshot returns as "running" (stale — run has since completed)
    machine.reset("running", { runId: "run-1" });
    expect(machine.getState()).toBe("running");

    // Replay the buffered RUN_COMPLETED event
    for (const { event, payload } of buffer) {
      machine.send(event, payload);
    }

    // Machine correctly advances to completed
    expect(machine.getState()).toBe("completed");
  });

  it("handles empty buffer after reset without errors", () => {
    const machine = createTestMachine("idle");
    const buffer: BufferedEvent[] = [];

    machine.reset("running", { runId: "run-1" });

    // Replay empty buffer — should be a no-op
    for (const { event, payload } of buffer) {
      machine.send(event, payload);
    }

    expect(machine.getState()).toBe("running");
  });
});

