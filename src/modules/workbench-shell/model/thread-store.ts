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

/** Real-time token usage reported by RuntimeThreadSurface (consumed by TopBar). */
export type ThreadContextUsage = {
  contextWindow: string | null;
  inputTokens: number;
  outputTokens: number;
  cacheReadTokens: number;
  cacheWriteTokens: number;
  totalTokens: number;
  modelDisplayName: string | null;
  runId: string;
};


export interface ThreadStatusRecord {
  status: ThreadRunStatus;
  runId: string | null;
  /**
   * Wall-clock start time (Unix ms) of the thread's currently active run.
   * Populated from `run_started` events (live) and from the sidebar list
   * snapshot (post-restart recovery).  Retained as metadata for debugging
   * and potential future use; the workbench header elapsed timer is driven
   * by a separate frontend `TimerSlot` mechanism in
   * `use-thread-elapsed-timer.ts` that only accumulates active running
   * time (excluding waiting_approval / needs_reply pauses).
   */
  startedAtMs: number | null;
  /** Backend-computed thread-level cumulative active running seconds
   *  (excluding pauses). Populated from the sidebar snapshot on app start;
   *  used to seed the frontend `TimerSlot` in
   *  `use-thread-elapsed-timer.ts`, including completed/interrupted history
   *  so later runs continue from the prior thread total. */
  elapsedRunningSeconds: number | null;
  updatedAt: number;
  source: ThreadStatusSource;
}

export interface ThreadStoreState {
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
  /** Profile ID override for the active thread (set when selecting a thread
   *  with a specific profile, or when creating a new thread). */
  activeThreadProfileIdOverride: string | null;
  /** Real-time token usage reported by RuntimeThreadSurface (consumed by TopBar). */
  runtimeContextUsage: ThreadContextUsage | null;
  /** Thread ID currently being inline-renamed in the sidebar. */
  editingThreadId: string | null;
  /** IDs of pending runs that have already been submitted to prevent
   *  re-submission after component unmount/remount cycles. */
  handledPendingRunIds: Record<string, number>;
  /** Goal state per thread. */
  goalState: Record<string, GoalStoreState | null>;
}

export interface GoalStoreState {
  id: string;
  threadId: string;
  objective: string;
  status: "active" | "paused" | "budget_limited" | "complete";
  tokensUsed: number;
  timeUsedSeconds: number;
  turnsUsed: number;
  maxTurns: number;
  tokenBudget?: number | null;
  pauseReason?: string | null;
  pauseDetail?: string | null;
  evidence?: string | null;
  lastEvaluatedRunId?: string | null;
  judgePassed?: boolean;
  judgeCompleteness?: number | null;
  judgeFindings?: string | null;
  judgeSummary?: string | null;
  judgeEvaluatedRunId?: string | null;
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
  activeThreadProfileIdOverride: null,
  runtimeContextUsage: null,
  editingThreadId: null,
  handledPendingRunIds: {},
  goalState: {},
});

// ---------------------------------------------------------------------------
// Actions — Thread Status
// ---------------------------------------------------------------------------

/**
 * Write a thread's run status into the store.
 *
 * Two guards prevent stale or out-of-order events from corrupting the status:
 *
 * **Guard A — Cross-run terminal protection**: A late-arriving terminal event
 * (completed, failed, …) from a *previous* run cannot overwrite the *current*
 * active run's status. This prevents the core race condition where
 * `thread-run-finished(run-1, completed)` arrives after `RUN_STARTED(run-2)`.
 *
 * **Guard B — Idle downgrade protection**: A null-runId "idle" write (e.g. from
 * a stale snapshot `reset()`) cannot overwrite an active status that carries a
 * real runId.
 */
export function setThreadStatus(
  threadId: string,
  status: ThreadRunStatus,
  meta: {
    runId?: string | null;
    source?: ThreadStatusSource;
    startedAtMs?: number | null;
    elapsedRunningSeconds?: number | null;
  } = {},
): void {
  threadStore.setState((prev) => {
    const existing = prev.threadStatuses[threadId];
    const incomingRunId = meta.runId ?? null;

    // Guard A: reject terminal events from a *different* (older) run when
    // the current run is still tracked. This is the fix for the cross-run
    // overwrite race condition.
    if (
      existing &&
      existing.runId !== null &&
      incomingRunId !== null &&
      incomingRunId !== existing.runId &&
      isTerminalStatus(status)
    ) {
      return {}; // stale terminal event from old run — ignore
    }

    // Guard B: reject idle/null downgrade of an active running state.
    // Prevents stale snapshot resets from clobbering live stream state.
    if (
      existing &&
      existing.runId !== null &&
      isActiveOrPendingStatus(existing.status) &&
      status === "idle" &&
      incomingRunId === null
    ) {
      return {}; // reject — don't downgrade active state with a null-runId idle write
    }

    // Guard C: reject snapshot downgrades of stream-sourced active states
    // to terminal or idle. Snapshots are point-in-time DB reads and always
    // lag behind the real-time stream for active runs. Within active states
    // (running, waiting_approval, needs_reply) the snapshot can still provide
    // useful transitions (e.g., running → waiting_approval from old snapshot).
    if (
      existing &&
      existing.source === "stream" &&
      isActiveOrPendingStatus(existing.status) &&
      meta.source === "snapshot" &&
      !isActiveOrPendingStatus(status) &&
      status !== existing.status
    ) {
      return {};
    }

    // Derive the next startedAtMs. Snapshots are the only place the value
    // arrives without an associated run_started event; for any other write
    // we either accept the explicit value or fall back to the existing one
    // (preserving the timer across status flips like running → waiting_approval
    // → running within the same run). On terminal status the timer is cleared
    // because the run is done and the wall-clock value is no longer meaningful.
    const isTerminal = isTerminalStatus(status);
    const startedAtMs = isTerminal
      ? null
      : meta.startedAtMs !== undefined
        ? meta.startedAtMs
        : (existing?.startedAtMs ?? null);

    return {
      threadStatuses: {
        ...prev.threadStatuses,
        [threadId]: {
          status,
          runId: isTerminal ? null : (incomingRunId ?? existing?.runId ?? null),
          startedAtMs,
          elapsedRunningSeconds: meta.elapsedRunningSeconds !== undefined
            ? meta.elapsedRunningSeconds
            : (existing?.elapsedRunningSeconds ?? null),
          source: meta.source ?? "tauri_event",
          updatedAt: Date.now(),
        },
      },
    };
  });
}

export function batchSetThreadStatuses(
  updates: Record<
    string,
    {
      status: ThreadRunStatus;
      runId?: string | null;
      source?: ThreadStatusSource;
      startedAtMs?: number | null;
      elapsedRunningSeconds?: number | null;
    }
  >,
): void {
  threadStore.setState((prev) => {
    const next = { ...prev.threadStatuses };
    for (const [threadId, upd] of Object.entries(updates)) {
      const existing = next[threadId];
      const incomingRunId = upd.runId ?? null;

      // Guard A: reject stale terminal events from a different (older) run.
      if (
        existing &&
        existing.runId !== null &&
        incomingRunId !== null &&
        incomingRunId !== existing.runId &&
        isTerminalStatus(upd.status)
      ) {
        continue;
      }
      // Guard B: reject idle/null downgrade of an active running state.
      if (
        existing &&
        existing.runId !== null &&
        isActiveOrPendingStatus(existing.status) &&
        upd.status === "idle" &&
        incomingRunId === null
      ) {
        continue;
      }

      // Guard C: reject snapshot downgrades of stream-sourced active states
      // to terminal or idle. Snapshots are point-in-time DB reads and always
      // lag behind the real-time stream for active runs. Within active states
      // (running, waiting_approval, needs_reply) the snapshot can still provide
      // useful transitions (e.g., running → waiting_approval from old snapshot).
      if (
        existing &&
        existing.source === "stream" &&
        isActiveOrPendingStatus(existing.status) &&
        upd.source === "snapshot" &&
        !isActiveOrPendingStatus(upd.status) &&
        upd.status !== existing.status
      ) {
        continue;
      }

      const isTerminal = isTerminalStatus(upd.status);
      const startedAtMs = isTerminal
        ? null
        : upd.startedAtMs !== undefined
          ? upd.startedAtMs
          : (existing?.startedAtMs ?? null);

      next[threadId] = {
        status: upd.status,
        runId: isTerminal ? null : (incomingRunId ?? existing?.runId ?? null),
        startedAtMs,
        elapsedRunningSeconds: upd.elapsedRunningSeconds !== undefined
          ? upd.elapsedRunningSeconds ?? null
          : (existing?.elapsedRunningSeconds ?? null),
        source: upd.source ?? "tauri_event",
        updatedAt: Date.now(),
      };
    }
    return { threadStatuses: next };
  });
}

function isTerminalStatus(s: ThreadRunStatus): boolean {
  return s === "completed" || s === "failed" || s === "cancelled" || s === "interrupted" || s === "limit_reached";
}

function isActiveOrPendingStatus(s: ThreadRunStatus): boolean {
  return s === "running" || s === "waiting_approval" || s === "needs_reply";
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
  const threadIdsToClean = threadStore
    .getState()
    .workspaces.find((w) => w.id === workspaceId)
    ?.threads.map((t) => t.id) ?? [];
  threadStore.setState((prev) => {
    const nextStatuses = { ...prev.threadStatuses };
    for (const tid of threadIdsToClean) {
      delete nextStatuses[tid];
    }
    const nextGoalState = { ...prev.goalState };
    for (const tid of threadIdsToClean) {
      delete nextGoalState[tid];
    }
    return {
      workspaces: prev.workspaces.filter((w) => w.id !== workspaceId),
      threadStatuses: nextStatuses,
      goalState: nextGoalState,
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
    runtimeContextUsage: null, // Reset token usage when switching threads
  });
}

export function removeThread(threadId: string): void {
  threadStore.setState((prev) => {
    const nextStatuses = { ...prev.threadStatuses };
    delete nextStatuses[threadId];
    const nextGoalState = { ...prev.goalState };
    delete nextGoalState[threadId];
    return {
      workspaces: prev.workspaces.map((w) => ({
        ...w,
        threads: w.threads.filter((t) => t.id !== threadId),
      })),
      threadStatuses: nextStatuses,
      goalState: nextGoalState,
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
      const nextGoalState = { ...s.goalState };
      delete nextGoalState[threadId];
      const isActive = s.activeThreadId === threadId;
      return {
        threadStatuses: nextStatuses,
        goalState: nextGoalState,
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

const PENDING_RUN_HANDLED_TTL_MS = 5 * 60 * 1000; // 5 minutes

function pruneHandledPendingRunIds(
  handledPendingRunIds: Record<string, number>,
  now: number,
): Record<string, number> {
  return Object.fromEntries(
    Object.entries(handledPendingRunIds).filter(
      ([, ts]) => now - ts <= PENDING_RUN_HANDLED_TTL_MS,
    ),
  );
}

/**
 * Mark a pending run as handled at the store level.
 * Survives component unmount/remount cycles, unlike a component-scoped ref.
 */
export function markPendingRunHandled(pendingRunId: string): void {
  threadStore.setState((prev) => {
    const now = Date.now();
    return {
      handledPendingRunIds: {
        ...pruneHandledPendingRunIds(prev.handledPendingRunIds, now),
        [pendingRunId]: now,
      },
    };
  });
}

/**
 * Check whether a pending run has already been submitted.
 * Returns false for entries older than TTL (allows retry after long periods).
 */
export function isPendingRunHandled(pendingRunId: string): boolean {
  const ts = threadStore.getState().handledPendingRunIds[pendingRunId];
  if (!ts) return false;
  if (Date.now() - ts > PENDING_RUN_HANDLED_TTL_MS) return false;
  return true;
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
