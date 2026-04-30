import { createStore, shallowEqual } from "@/shared/lib/create-store";
import type {
  ThreadRunStatus,
  WorkspaceItem,
} from "@/modules/workbench-shell/model/types";
import type { PendingThreadRun } from "@/modules/workbench-shell/ui/dashboard-workbench-logic";
import { syncToBackend } from "@/shared/lib/ipc-sync";
import { threadDelete } from "@/services/bridge/thread-commands";

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

export type ThreadStatusSource = "stream" | "tauri_event" | "snapshot" | "optimistic";

export interface ThreadStatusRecord {
  status: ThreadRunStatus;
  runId: string | null;
  updatedAt: number;
  source: ThreadStatusSource;
}

export interface ThreadStoreState {
  [key: string]: unknown;
  /** Workspace list with nested thread items. */
  workspaces: WorkspaceItem[];
  /** ID of the default workspace. */
  defaultWorkspaceId: string | null;
  /** Flat map of thread-id → status record. Single source of truth for all
   *  thread run statuses consumed by the sidebar and runtime surfaces. */
  threadStatuses: Record<string, ThreadStatusRecord>;
  /** Currently active (selected) thread ID. */
  activeThreadId: string | null;
  /** Whether the workbench is in "new thread" mode (no thread selected). */
  isNewThreadMode: boolean;
  /** Pending runs keyed by thread ID. */
  pendingRuns: Record<string, PendingThreadRun>;
  /** Per-workspace display count for sidebar pagination. */
  displayCounts: Record<string, number>;
  /** Per-workspace "has more threads" flag. */
  hasMore: Record<string, boolean>;
  /** Per-workspace "load more" pending state. */
  loadMorePending: Record<string, boolean>;
  /** Per-workspace expand/collapse state. */
  openWorkspaces: Record<string, boolean>;
  /** Whether the initial sidebar sync has completed. */
  sidebarReady: boolean;
}

// ---------------------------------------------------------------------------
// Store
// ---------------------------------------------------------------------------

export const threadStore = createStore<ThreadStoreState>({
  workspaces: [],
  defaultWorkspaceId: null,
  threadStatuses: {},
  activeThreadId: null,
  isNewThreadMode: true,
  pendingRuns: {},
  displayCounts: {},
  hasMore: {},
  loadMorePending: {},
  openWorkspaces: {},
  sidebarReady: false,
});

// ---------------------------------------------------------------------------
// Actions — Thread Status
// ---------------------------------------------------------------------------

/**
 * Write a thread's run status into the store.
 *
 * **Phase 1 transition period**: called directly by Tauri global events,
 * ThreadStream callbacks, and optimistic writes.
 *
 * **Phase 3 onwards**: only called by the `runLifecycleMachine` subscribe
 * callback. External event sources will send events to the state machine
 * first, and the machine will sync validated states here.
 *
 * Includes a minimal runId-based out-of-order guard: if the store already
 * holds a *newer* runId with a later `updatedAt`, the stale write is
 * silently ignored. This prevents a late-arriving `run-finished` event
 * from overwriting a newer `running` status — one of the core bugs of the
 * three-source inconsistency.
 */
export function setThreadStatus(
  threadId: string,
  status: ThreadRunStatus,
  meta: {
    runId?: string | null;
    source?: ThreadStatusSource;
    updatedAt?: number;
  } = {},
): void {
  threadStore.setState((prev) => {
    const existing = prev.threadStatuses[threadId];

    // Minimal out-of-order guard: ignore stale runId writes.
    if (
      existing &&
      existing.runId !== null &&
      meta.runId !== undefined &&
      meta.runId !== null &&
      existing.runId !== meta.runId &&
      existing.updatedAt > (meta.updatedAt ?? 0)
    ) {
      return {}; // no update
    }

    return {
      threadStatuses: {
        ...prev.threadStatuses,
        [threadId]: {
          status,
          runId: meta.runId ?? existing?.runId ?? null,
          source: meta.source ?? "tauri_event",
          updatedAt: meta.updatedAt ?? Date.now(),
        },
      },
    };
  });
}

export function batchSetThreadStatuses(
  updates: Record<
    string,
    { status: ThreadRunStatus; runId?: string | null; source?: ThreadStatusSource; updatedAt?: number }
  >,
): void {
  threadStore.setState((prev) => {
    const next = { ...prev.threadStatuses };
    for (const [threadId, upd] of Object.entries(updates)) {
      const existing = next[threadId];
      if (
        existing &&
        existing.runId !== null &&
        upd.runId !== undefined &&
        upd.runId !== null &&
        existing.runId !== upd.runId &&
        existing.updatedAt > (upd.updatedAt ?? 0)
      ) {
        continue;
      }
      next[threadId] = {
        status: upd.status,
        runId: upd.runId ?? existing?.runId ?? null,
        source: upd.source ?? "tauri_event",
        updatedAt: upd.updatedAt ?? Date.now(),
      };
    }
    return { threadStatuses: next };
  });
}

// ---------------------------------------------------------------------------
// Actions — Workspaces
// ---------------------------------------------------------------------------

export function setWorkspaces(workspaces: WorkspaceItem[]): void {
  threadStore.setState({ workspaces });
}

export function updateWorkspace(
  workspaceId: string,
  updater: (ws: WorkspaceItem) => WorkspaceItem,
): void {
  threadStore.setState((prev) => ({
    workspaces: prev.workspaces.map((w) =>
      w.id === workspaceId ? updater(w) : w,
    ),
  }));
}

export function removeWorkspace(workspaceId: string): void {
  threadStore.setState((prev) => {
    const workspace = prev.workspaces.find((w) => w.id === workspaceId);
    const threadIdsToClean = workspace
      ? workspace.threads.map((t) => t.id)
      : [];
    const nextStatuses = { ...prev.threadStatuses };
    for (const tid of threadIdsToClean) {
      delete nextStatuses[tid];
    }
    return {
      workspaces: prev.workspaces.filter((w) => w.id !== workspaceId),
      threadStatuses: nextStatuses,
    };
  });
}

// ---------------------------------------------------------------------------
// Actions — Threads
// ---------------------------------------------------------------------------

export function setActiveThread(
  threadId: string | null,
  isNewThread?: boolean,
): void {
  threadStore.setState({
    activeThreadId: threadId,
    isNewThreadMode: isNewThread ?? (threadId === null),
  });
}

export function removeThread(threadId: string): void {
  threadStore.setState((prev) => {
    const nextStatuses = { ...prev.threadStatuses };
    delete nextStatuses[threadId];
    return {
      workspaces: prev.workspaces.map((w) => ({
        ...w,
        threads: w.threads.filter((t) => t.id !== threadId),
      })),
      threadStatuses: nextStatuses,
    };
  });
}

/**
 * Delete a thread from the backend, then remove it from the store.
 *
 * Uses optimistic removal for immediate UI feedback with rollback on failure.
 * Deduplicates by thread ID (`'first'` strategy) to prevent accidental
 * double-deletes from rapid clicks.
 */
export function deleteThread(threadId: string): Promise<void> {
  return syncToBackend(threadStore, () => threadDelete(threadId), {
    optimistic: (s) => {
      const nextStatuses = { ...s.threadStatuses };
      delete nextStatuses[threadId];
      const isActive = s.activeThreadId === threadId;
      return {
        threadStatuses: nextStatuses,
        workspaces: s.workspaces.map((w) => ({
          ...w,
          threads: w.threads.filter((t) => t.id !== threadId),
        })),
        ...(isActive ? { activeThreadId: null, isNewThreadMode: true } : {}),
      };
    },
    dedupe: { key: `thread-delete:${threadId}`, strategy: "first" },
  });
}

export function updateThreadTitle(threadId: string, title: string): void {
  threadStore.setState((prev) => ({
    workspaces: prev.workspaces.map((w) => ({
      ...w,
      threads: w.threads.map((t) =>
        t.id === threadId ? { ...t, name: title } : t,
      ),
    })),
  }));
}

// ---------------------------------------------------------------------------
// Actions — Pending Runs
// ---------------------------------------------------------------------------

export function addPendingRun(threadId: string, run: PendingThreadRun): void {
  threadStore.setState((prev) => ({
    pendingRuns: { ...prev.pendingRuns, [threadId]: run },
  }));
}

export function removePendingRun(threadId: string): void {
  threadStore.setState((prev) => {
    const next = { ...prev.pendingRuns };
    delete next[threadId];
    return { pendingRuns: next };
  });
}

// ---------------------------------------------------------------------------
// Actions — Sidebar Pagination
// ---------------------------------------------------------------------------

export function setDisplayCount(workspaceId: string, count: number): void {
  threadStore.setState((prev) => ({
    displayCounts: { ...prev.displayCounts, [workspaceId]: count },
  }));
}

export function setHasMore(workspaceId: string, hasMore: boolean): void {
  threadStore.setState((prev) => ({
    hasMore: { ...prev.hasMore, [workspaceId]: hasMore },
  }));
}

export function setLoadMorePending(
  workspaceId: string,
  pending: boolean,
): void {
  threadStore.setState((prev) => ({
    loadMorePending: { ...prev.loadMorePending, [workspaceId]: pending },
  }));
}

export function setOpenWorkspace(
  workspaceId: string,
  open: boolean,
): void {
  threadStore.setState((prev) => ({
    openWorkspaces: { ...prev.openWorkspaces, [workspaceId]: open },
  }));
}

export function setSidebarReady(ready: boolean): void {
  threadStore.setState({ sidebarReady: ready });
}

export function setDefaultWorkspaceId(id: string | null): void {
  threadStore.setState({ defaultWorkspaceId: id });
}

// ---------------------------------------------------------------------------
// Re-exports for convenience
// ---------------------------------------------------------------------------

export { useStore } from "@/shared/lib/create-store";
export { shallowEqual };
