import { describe, expect, it } from "vitest";

import { mapSnapshotToRunState, isTaskBoardTool, getDefaultToolOpenState } from "./runtime-thread-surface-logic";
import type { RunStatus, ThreadSnapshotDto } from "@/shared/types/api";

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

describe("mapSnapshotToRunState", () => {
  it("treats cancelling snapshots as cancelled instead of running", () => {
    expect(mapSnapshotToRunState(makeSnapshot("cancelling"))).toBe("cancelled");
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

describe("getDefaultToolOpenState", () => {
  it("defaults task board tools to collapsed", () => {
    expect(getDefaultToolOpenState("create_task", "input-available", undefined)).toBe(false);
    expect(getDefaultToolOpenState("update_task", "output-available", undefined)).toBe(false);
    expect(getDefaultToolOpenState("query_task", "input-streaming", undefined)).toBe(false);
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
