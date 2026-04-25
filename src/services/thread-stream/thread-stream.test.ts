import { beforeEach, describe, expect, it, vi } from "vitest";
import type { ThreadStreamEvent } from "./types";

const {
  threadCancelRunMock,
  threadCompactContextMock,
  threadExecuteApprovedPlanMock,
  threadStartRunMock,
  threadSubscribeRunMock,
  toolApprovalRespondMock,
  toolClarifyRespondMock,
} = vi.hoisted(() => ({
  threadCancelRunMock: vi.fn(),
  threadCompactContextMock: vi.fn(),
  threadExecuteApprovedPlanMock: vi.fn(),
  threadStartRunMock: vi.fn(),
  threadSubscribeRunMock: vi.fn(),
  toolApprovalRespondMock: vi.fn(),
  toolClarifyRespondMock: vi.fn(),
}));

vi.mock("@/services/bridge", () => ({
  threadCancelRun: threadCancelRunMock,
  threadCompactContext: threadCompactContextMock,
  threadExecuteApprovedPlan: threadExecuteApprovedPlanMock,
  threadStartRun: threadStartRunMock,
  threadSubscribeRun: threadSubscribeRunMock,
  toolApprovalRespond: toolApprovalRespondMock,
  toolClarifyRespond: toolClarifyRespondMock,
}));

import { ThreadStream } from "@/services/thread-stream/thread-stream";

const usage = {
  inputTokens: 1,
  outputTokens: 2,
  cacheReadTokens: 3,
  cacheWriteTokens: 4,
  totalTokens: 10,
};

const helperSnapshot = {
  totalToolCalls: 0,
  completedSteps: 0,
  currentAction: null,
  recentActions: [],
  toolCounts: {},
};

function emit(events: ThreadStreamEvent[]) {
  return (_threadId: string, _input: unknown, onEvent: (event: ThreadStreamEvent) => void) => {
    for (const event of events) onEvent(event);
    return Promise.resolve("run-1");
  };
}

describe("ThreadStream event routing", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("routes run, message, plan, reasoning, queue, task board, title, and usage events", async () => {
    const stream = new ThreadStream();
    const onRawEvent = vi.fn();
    const onRunStateChange = vi.fn();
    const onMessage = vi.fn();
    const onPlan = vi.fn();
    const onReasoning = vi.fn();
    const onQueue = vi.fn();
    const onTaskBoard = vi.fn();
    const onThreadTitle = vi.fn();
    const onUsage = vi.fn();
    stream.onRawEvent = onRawEvent;
    stream.onRunStateChange = onRunStateChange;
    stream.onMessage = onMessage;
    stream.onPlan = onPlan;
    stream.onReasoning = onReasoning;
    stream.onQueue = onQueue;
    stream.onTaskBoard = onTaskBoard;
    stream.onThreadTitle = onThreadTitle;
    stream.onUsage = onUsage;

    threadStartRunMock.mockImplementationOnce(emit([
      { type: "run_started", runId: "run-1", runMode: "default" },
      { type: "message_delta", runId: "run-1", messageId: "msg-1", delta: "hi" },
      { type: "message_completed", runId: "run-1", messageId: "msg-1", content: "hi!" },
      { type: "plan_updated", runId: "run-1", plan: { steps: ["a"] } },
      { type: "reasoning_updated", runId: "run-1", messageId: "reason-1", reasoning: "thinking" },
      { type: "queue_updated", runId: "run-1", queue: ["q"] },
      {
        type: "task_board_updated",
        runId: "run-1",
        taskBoard: {
          id: "board-1",
          threadId: "thread-1",
          title: "Plan",
          status: "active",
          activeTaskId: null,
          tasks: [],
          createdAt: "2026-04-25T00:00:00Z",
          updatedAt: "2026-04-25T00:00:00Z",
        },
      },
      { type: "thread_title_updated", runId: "run-1", threadId: "thread-1", title: "New title" },
      { type: "thread_usage_updated", runId: "run-1", modelDisplayName: "Model", contextWindow: "128k", usage },
      { type: "run_completed", runId: "run-1" },
    ]));

    await expect(stream.startRun("thread-1", { prompt: "hi" })).resolves.toBe("run-1");

    expect(onRawEvent).toHaveBeenCalledTimes(10);
    expect(onRunStateChange).toHaveBeenCalledWith("running", "run-1");
    expect(onRunStateChange).toHaveBeenCalledWith("completed", "run-1");
    expect(onMessage).toHaveBeenCalledWith({ kind: "delta", runId: "run-1", messageId: "msg-1", delta: "hi" });
    expect(onMessage).toHaveBeenCalledWith({ kind: "completed", runId: "run-1", messageId: "msg-1", content: "hi!" });
    expect(onPlan).toHaveBeenCalledWith({ runId: "run-1", plan: { steps: ["a"] } });
    expect(onReasoning).toHaveBeenCalledWith({ runId: "run-1", messageId: "reason-1", reasoning: "thinking" });
    expect(onQueue).toHaveBeenCalledWith({ runId: "run-1", queue: ["q"] });
    expect(onTaskBoard).toHaveBeenCalledWith({ taskBoard: expect.objectContaining({ id: "board-1" }) });
    expect(onThreadTitle).toHaveBeenCalledWith({ runId: "run-1", threadId: "thread-1", title: "New title" });
    expect(onUsage).toHaveBeenCalledWith({ runId: "run-1", modelDisplayName: "Model", contextWindow: "128k", usage });
  });

  it("routes tool, approval, clarify, helper, compression, and error events", async () => {
    const stream = new ThreadStream();
    const onToolEvent = vi.fn();
    const onApproval = vi.fn();
    const onHelperEvent = vi.fn();
    const onContextCompressing = vi.fn();
    const onRunStateChange = vi.fn();
    const onError = vi.fn();
    stream.onToolEvent = onToolEvent;
    stream.onApproval = onApproval;
    stream.onHelperEvent = onHelperEvent;
    stream.onContextCompressing = onContextCompressing;
    stream.onRunStateChange = onRunStateChange;
    stream.onError = onError;

    threadStartRunMock.mockImplementationOnce(emit([
      { type: "tool_requested", runId: "run-1", toolCallId: "tool-1", toolName: "read", toolInput: { path: "a" } },
      { type: "tool_running", runId: "run-1", toolCallId: "tool-1" },
      { type: "tool_completed", runId: "run-1", toolCallId: "tool-1", result: { ok: true } },
      { type: "approval_required", runId: "run-1", toolCallId: "tool-2", toolName: "edit", toolInput: {}, reason: "confirm" },
      { type: "approval_resolved", runId: "run-1", toolCallId: "tool-2", approved: true },
      { type: "clarify_required", runId: "run-1", toolCallId: "tool-3", toolName: "clarify", toolInput: { question: "?" } },
      { type: "clarify_resolved", runId: "run-1", toolCallId: "tool-3", response: { answer: "yes" } },
      { type: "subagent_started", runId: "run-1", subtaskId: "sub-1", helperKind: "review", startedAt: "now", snapshot: helperSnapshot },
      { type: "subagent_progress", runId: "run-1", subtaskId: "sub-1", helperKind: "review", startedAt: "now", activity: "started", message: "reading", snapshot: helperSnapshot },
      { type: "subagent_completed", runId: "run-1", subtaskId: "sub-1", helperKind: "review", startedAt: "now", summary: "ok", snapshot: helperSnapshot },
      { type: "context_compressing", runId: "run-1" },
      { type: "run_limit_reached", runId: "run-1", error: "too many turns", maxTurns: 10 },
    ]));

    await stream.startRun("thread-1", { prompt: "hi" });

    expect(onToolEvent).toHaveBeenCalledWith(expect.objectContaining({ kind: "requested", toolName: "read" }));
    expect(onToolEvent).toHaveBeenCalledWith(expect.objectContaining({ kind: "running", toolName: "read" }));
    expect(onToolEvent).toHaveBeenCalledWith(expect.objectContaining({ kind: "completed", result: { ok: true } }));
    expect(onApproval).toHaveBeenCalledWith(expect.objectContaining({ kind: "required", reason: "confirm" }));
    expect(onApproval).toHaveBeenCalledWith(expect.objectContaining({ kind: "resolved", toolName: "edit", approved: true }));
    expect(onToolEvent).toHaveBeenCalledWith(expect.objectContaining({ kind: "clarify-required", toolName: "clarify" }));
    expect(onToolEvent).toHaveBeenCalledWith(expect.objectContaining({ kind: "clarify-resolved", response: { answer: "yes" } }));
    expect(onHelperEvent).toHaveBeenCalledWith(expect.objectContaining({ kind: "started", subtaskId: "sub-1" }));
    expect(onHelperEvent).toHaveBeenCalledWith(expect.objectContaining({ kind: "progress", message: "reading" }));
    expect(onHelperEvent).toHaveBeenCalledWith(expect.objectContaining({ kind: "completed", summary: "ok" }));
    expect(onContextCompressing).toHaveBeenCalledWith("run-1");
    expect(onRunStateChange).toHaveBeenCalledWith("limit_reached", "run-1");
    expect(onError).toHaveBeenCalledWith("too many turns", "run-1");
  });

  it("hides runtime orchestration tool events", async () => {
    const stream = new ThreadStream();
    const onToolEvent = vi.fn();
    stream.onToolEvent = onToolEvent;

    threadStartRunMock.mockImplementationOnce(emit([
      { type: "tool_requested", runId: "run-1", toolCallId: "tool-hidden", toolName: "agent_review", toolInput: {} },
      { type: "tool_running", runId: "run-1", toolCallId: "tool-hidden" },
      { type: "tool_completed", runId: "run-1", toolCallId: "tool-hidden", result: {} },
    ]));

    await stream.startRun("thread-1", { prompt: "review" });

    expect(onToolEvent).not.toHaveBeenCalled();
  });

  it("does not deliver events after dispose", async () => {
    const stream = new ThreadStream();
    const onMessage = vi.fn();
    stream.onMessage = onMessage;
    stream.dispose();

    threadStartRunMock.mockImplementationOnce(emit([
      { type: "message_delta", runId: "run-1", messageId: "msg-1", delta: "hidden" },
    ]));

    await stream.startRun("thread-1", { prompt: "hi" });

    expect(onMessage).not.toHaveBeenCalled();
    expect(stream.isActive).toBe(false);
  });
});

describe("ThreadStream commands", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("subscribes, compacts context, and executes approved plans with event callbacks", async () => {
    threadSubscribeRunMock.mockResolvedValueOnce("run-sub");
    threadCompactContextMock.mockResolvedValueOnce("run-compact");
    threadExecuteApprovedPlanMock.mockResolvedValueOnce("run-plan");

    const stream = new ThreadStream();

    await expect(stream.subscribe("thread-1")).resolves.toBe("run-sub");
    await expect(stream.compactContext("thread-1", "short", null)).resolves.toBe("run-compact");
    await expect(stream.executeApprovedPlan("thread-1", "msg-1", "apply_plan")).resolves.toBe("run-plan");

    expect(threadSubscribeRunMock).toHaveBeenCalledWith("thread-1", expect.any(Function));
    expect(threadCompactContextMock).toHaveBeenCalledWith("thread-1", "short", null, expect.any(Function));
    expect(threadExecuteApprovedPlanMock).toHaveBeenCalledWith("thread-1", "msg-1", "apply_plan", expect.any(Function));
    expect(stream.runId).toBe("run-plan");

    stream.reset();
    expect(stream.runId).toBeNull();
  });

  it("responds to approval and clarify requests", async () => {
    toolApprovalRespondMock.mockResolvedValueOnce(undefined);
    toolClarifyRespondMock.mockResolvedValueOnce(undefined);

    const stream = new ThreadStream();
    await stream.respondToApproval("tool-1", "run-1", true);
    await stream.respondToClarify("tool-2", { answer: "ok" });

    expect(toolApprovalRespondMock).toHaveBeenCalledWith("tool-1", "run-1", true);
    expect(toolClarifyRespondMock).toHaveBeenCalledWith("tool-2", { answer: "ok" });
  });

  it("透传后端的幂等取消结果，不触发错误回调", async () => {
    const stream = new ThreadStream();
    const onError = vi.fn();
    stream.onError = onError;

    threadCancelRunMock.mockResolvedValueOnce(false);

    await expect(stream.cancelRun("thread-1")).resolves.toBe(false);
    expect(threadCancelRunMock).toHaveBeenCalledWith("thread-1");
    expect(onError).not.toHaveBeenCalled();
  });

  it("在真实取消失败时仍然上报错误并抛出异常", async () => {
    const stream = new ThreadStream();
    const onError = vi.fn();
    stream.onError = onError;

    threadCancelRunMock.mockRejectedValueOnce(new Error("cancel failed"));

    await expect(stream.cancelRun("thread-2")).rejects.toThrow("cancel failed");
    expect(onError).toHaveBeenCalledWith("cancel failed", "");
  });

  it("reports command errors from plain object rejections", async () => {
    const stream = new ThreadStream();
    const onError = vi.fn();
    stream.onError = onError;
    threadStartRunMock.mockRejectedValueOnce({ error: "backend unavailable" });

    await expect(stream.startRun("thread-1", { prompt: "hi" })).rejects.toEqual({ error: "backend unavailable" });
    expect(onError).toHaveBeenCalledWith("backend unavailable", "");
  });
});
