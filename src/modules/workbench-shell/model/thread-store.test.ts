import { describe, expect, it, vi, beforeEach } from "vitest";

import {
  threadStore,
  setThreadStatus,
  batchSetThreadStatuses,
  setWorkspaces,
  removeWorkspace,
  setActiveThread,
  removeThread,
  deleteThread,
  updateThreadTitle,
  addPendingRun,
  removePendingRun,
  setDisplayCount,
  setHasMore,
  setLoadMorePending,
  setOpenWorkspace,
  setSidebarReady,
} from "./thread-store";
import type { PendingThreadRun } from "../ui/dashboard-workbench-logic";
import { threadDelete } from "@/services/bridge/thread-commands";

// ---------------------------------------------------------------------------
// Mock IPC
// ---------------------------------------------------------------------------

vi.mock("@/services/bridge/thread-commands", () => ({
  threadDelete: vi.fn().mockResolvedValue(undefined),
}));

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

function makePendingRun(overrides?: Partial<PendingThreadRun>): PendingThreadRun {
  return {
    id: "run-1",
    displayText: "test",
    effectivePrompt: "test prompt",
    attachments: [],
    metadata: null,
    runMode: "default",
    threadId: "thread-1",
    ...overrides,
  };
}

function setupWorkspaceState() {
  threadStore.reset();
  setWorkspaces([
    {
      id: "ws-1",
      name: "Workspace 1",
      defaultOpen: true,
      threads: [
        { id: "thread-1", name: "Thread 1", time: "now", active: true, status: "completed" },
        { id: "thread-2", name: "Thread 2", time: "now", active: false, status: "completed" },
      ],
    },
  ]);
}

// ---------------------------------------------------------------------------
// setThreadStatus
// ---------------------------------------------------------------------------

describe("setThreadStatus", () => {
  it("writes a status record for a thread", () => {
    threadStore.reset();
    setThreadStatus("thread-1", "running", { runId: "run-1", source: "stream" });

    const record = threadStore.getState().threadStatuses["thread-1"];
    expect(record).toBeDefined();
    expect(record.status).toBe("running");
    expect(record.runId).toBe("run-1");
    expect(record.source).toBe("stream");
    expect(record.updatedAt).toBeGreaterThan(0);
  });

  it("defaults source to tauri_event when not provided", () => {
    threadStore.reset();
    setThreadStatus("thread-1", "idle");

    expect(threadStore.getState().threadStatuses["thread-1"].source).toBe(
      "tauri_event",
    );
  });

  it("allows overwriting with same runId", () => {
    threadStore.reset();
    setThreadStatus("thread-1", "running", { runId: "run-1" });
    setThreadStatus("thread-1", "completed", { runId: "run-1" });

    expect(threadStore.getState().threadStatuses["thread-1"].status).toBe(
      "completed",
    );
  });

  it("ignores stale writes within the same runId", () => {
    threadStore.reset();

    const now = Date.now();
    setThreadStatus("thread-1", "running", {
      runId: "run-1",
      source: "stream",
      updatedAt: now + 1000,
    });

    // Try to overwrite with same runId but older timestamp — should be rejected
    setThreadStatus("thread-1", "failed", {
      runId: "run-1",
      source: "tauri_event",
      updatedAt: now,
    });

    expect(threadStore.getState().threadStatuses["thread-1"].status).toBe(
      "running",
    );
    expect(threadStore.getState().threadStatuses["thread-1"].runId).toBe(
      "run-1",
    );
  });

  it("accepts writes from a different runId (new run started)", () => {
    threadStore.reset();

    // First run completes
    setThreadStatus("thread-1", "running", { runId: "run-1" });
    setThreadStatus("thread-1", "completed", { runId: "run-1" });

    // Second run starts — must NOT be blocked by the guard
    setThreadStatus("thread-1", "running", { runId: "run-2" });

    expect(threadStore.getState().threadStatuses["thread-1"].status).toBe(
      "running",
    );
    expect(threadStore.getState().threadStatuses["thread-1"].runId).toBe(
      "run-2",
    );
  });

  it("accepts writes from a new runId even if the existing record has the same runId", () => {
    threadStore.reset();
    setThreadStatus("thread-1", "running", { runId: "run-1" });
    setThreadStatus("thread-1", "completed", { runId: "run-1" });
    // re-start with same runId
    setThreadStatus("thread-1", "running", { runId: "run-1" });

    expect(threadStore.getState().threadStatuses["thread-1"].status).toBe(
      "running",
    );
  });

  it("handles missing existing record gracefully", () => {
    threadStore.reset();
    setThreadStatus("new-thread", "running", { runId: "run-1" });

    expect(
      threadStore.getState().threadStatuses["new-thread"].status,
    ).toBe("running");
  });
});

// ---------------------------------------------------------------------------
// batchSetThreadStatuses
// ---------------------------------------------------------------------------

describe("batchSetThreadStatuses", () => {
  it("writes multiple statuses at once", () => {
    threadStore.reset();
    batchSetThreadStatuses({
      "thread-1": { status: "running", runId: "run-1" },
      "thread-2": { status: "idle", runId: null },
    });

    const statuses = threadStore.getState().threadStatuses;
    expect(Object.keys(statuses)).toHaveLength(2);
    expect(statuses["thread-1"].status).toBe("running");
    expect(statuses["thread-2"].status).toBe("idle");
  });
});

// ---------------------------------------------------------------------------
// setWorkspaces / removeWorkspace
// ---------------------------------------------------------------------------

describe("setWorkspaces", () => {
  it("replaces the workspace list", () => {
    threadStore.reset();
    setWorkspaces([
      {
        id: "ws-1",
        name: "WS 1",
        defaultOpen: true,
        threads: [],
      },
    ]);

    expect(threadStore.getState().workspaces).toHaveLength(1);
    expect(threadStore.getState().workspaces[0].id).toBe("ws-1");
  });
});

describe("removeWorkspace", () => {
  it("removes workspace and cleans up threadStatuses for its threads", () => {
    setupWorkspaceState();
    setThreadStatus("thread-1", "running", { runId: "run-1" });
    setThreadStatus("thread-2", "idle", { runId: null });

    removeWorkspace("ws-1");

    expect(threadStore.getState().workspaces).toHaveLength(0);
    expect(
      threadStore.getState().threadStatuses["thread-1"],
    ).toBeUndefined();
    expect(
      threadStore.getState().threadStatuses["thread-2"],
    ).toBeUndefined();
  });
});

// ---------------------------------------------------------------------------
// setActiveThread / removeThread / updateThreadTitle
// ---------------------------------------------------------------------------

describe("setActiveThread", () => {
  it("sets activeThreadId and isNewThreadMode", () => {
    threadStore.reset();
    setActiveThread("thread-1", false);

    expect(threadStore.getState().activeThreadId).toBe("thread-1");
    expect(threadStore.getState().isNewThreadMode).toBe(false);
  });

  it("defaults isNewThreadMode to true when threadId is null", () => {
    threadStore.reset();
    setActiveThread("thread-1", false);
    setActiveThread(null);

    expect(threadStore.getState().isNewThreadMode).toBe(true);
  });
});

describe("removeThread", () => {
  it("removes thread from workspaces and threadStatuses", () => {
    setupWorkspaceState();
    setThreadStatus("thread-1", "running", { runId: "run-1" });

    removeThread("thread-1");

    const ws = threadStore.getState().workspaces[0];
    expect(ws.threads).toHaveLength(1);
    expect(ws.threads[0].id).toBe("thread-2");
    expect(
      threadStore.getState().threadStatuses["thread-1"],
    ).toBeUndefined();
  });
});

describe("updateThreadTitle", () => {
  it("updates thread name in workspaces", () => {
    setupWorkspaceState();
    updateThreadTitle("thread-1", "New Title");

    const thread = threadStore
      .getState()
      .workspaces[0].threads.find((t) => t.id === "thread-1");
    expect(thread?.name).toBe("New Title");
  });
});

// ---------------------------------------------------------------------------
// Pending Runs
// ---------------------------------------------------------------------------

describe("addPendingRun / removePendingRun", () => {
  it("adds and removes pending runs", () => {
    threadStore.reset();

    const run = makePendingRun();
    addPendingRun("thread-1", run);
    expect(threadStore.getState().pendingRuns["thread-1"]).toEqual(run);

    removePendingRun("thread-1");
    expect(
      threadStore.getState().pendingRuns["thread-1"],
    ).toBeUndefined();
  });

  it("removePendingRun is a no-op for unknown thread", () => {
    threadStore.reset();
    removePendingRun("unknown");
    expect(threadStore.getState().pendingRuns).toEqual({});
  });
});

// ---------------------------------------------------------------------------
// Sidebar Pagination
// ---------------------------------------------------------------------------

describe("sidebar pagination actions", () => {
  it("setDisplayCount updates per-workspace count", () => {
    threadStore.reset();
    setDisplayCount("ws-1", 20);
    expect(threadStore.getState().displayCounts["ws-1"]).toBe(20);
  });

  it("setHasMore updates per-workspace flag", () => {
    threadStore.reset();
    setHasMore("ws-1", true);
    expect(threadStore.getState().hasMore["ws-1"]).toBe(true);
  });

  it("setLoadMorePending updates per-workspace pending state", () => {
    threadStore.reset();
    setLoadMorePending("ws-1", true);
    expect(threadStore.getState().loadMorePending["ws-1"]).toBe(true);
  });

  it("setOpenWorkspace updates per-workspace open state", () => {
    threadStore.reset();
    setOpenWorkspace("ws-1", true);
    expect(threadStore.getState().openWorkspaces["ws-1"]).toBe(true);
  });

  it("setSidebarReady sets sidebar ready flag", () => {
    threadStore.reset();
    setSidebarReady(true);
    expect(threadStore.getState().sidebarReady).toBe(true);
  });
});

// ---------------------------------------------------------------------------
// deleteThread (async, optimistic + rollback + dedupe)
// ---------------------------------------------------------------------------

describe("deleteThread", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    threadStore.reset();
  });

  it("optimistically removes the thread from workspaces and statuses", async () => {
    setupWorkspaceState();

    const p = deleteThread("thread-1");

    // Optimistic update is synchronous (executed before IPC resolves).
    const state = threadStore.getState();
    expect(state.workspaces[0].threads).toHaveLength(1);
    expect(state.workspaces[0].threads[0].id).toBe("thread-2");
    expect(state.threadStatuses["thread-1"]).toBeUndefined();

    await p;
    expect(vi.mocked(threadDelete)).toHaveBeenCalledWith("thread-1");
  });

  it("resets activeThreadId when deleting the active thread", async () => {
    setupWorkspaceState();
    setActiveThread("thread-1", false);

    const p = deleteThread("thread-1");

    const state = threadStore.getState();
    expect(state.activeThreadId).toBeNull();
    expect(state.isNewThreadMode).toBe(true);

    await p;
  });

  it("resets activeThreadId when deleting the active thread (different thread ID)", async () => {
    setupWorkspaceState();
    setActiveThread("thread-2", false);

    const p = deleteThread("thread-2");
    // thread-2 was the active thread, so activeThreadId should be cleared
    const state = threadStore.getState();
    expect(state.activeThreadId).toBeNull();
    expect(state.workspaces[0].threads).toHaveLength(1);

    await p;
  });

  it("preserves activeThreadId when deleting a non-active thread", async () => {
    setupWorkspaceState();
    // Set thread-1 as active, delete thread-2 (non-active)
    setActiveThread("thread-1", false);

    const p = deleteThread("thread-2");
    // thread-1 should remain active
    const state = threadStore.getState();
    expect(state.activeThreadId).toBe("thread-1");
    expect(state.workspaces[0].threads).toHaveLength(1);
    // Only thread-1 should remain
    expect(state.workspaces[0].threads[0].id).toBe("thread-1");

    await p;
  });

  it("rolls back on IPC failure", async () => {
    // Make the IPC call reject
    vi.mocked(threadDelete).mockRejectedValueOnce(new Error("IPC failure"));

    setupWorkspaceState();
    const snapshot = threadStore.getState();

    const p = deleteThread("thread-1");
    // Expect rejection
    await expect(p).rejects.toThrow("IPC failure");

    // After rollback, the thread should be restored.
    const state = threadStore.getState();
    expect(state.workspaces[0].threads).toHaveLength(2);
    expect(
      state.workspaces[0].threads.find((t) => t.id === "thread-1"),
    ).toBeTruthy();
    // Active state should also be restored
    expect(state.activeThreadId).toBe(snapshot.activeThreadId);
  });

  it("deduplicates concurrent deleteThread calls for the same thread (first strategy)", async () => {
    vi.mocked(threadDelete).mockResolvedValue(undefined);

    setupWorkspaceState();

    // First call starts but doesn't resolve yet
    const p1 = deleteThread("thread-1");
    // Second call should reject with SyncError (first strategy)
    const p2 = deleteThread("thread-1");

    await expect(p2).rejects.toThrow("Request superseded");
    await p1;
  });
});
