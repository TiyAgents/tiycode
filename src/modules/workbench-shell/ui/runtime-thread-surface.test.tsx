import { describe, expect, it } from "vitest";

import { mapSnapshotToRunState } from "./runtime-thread-surface";
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
