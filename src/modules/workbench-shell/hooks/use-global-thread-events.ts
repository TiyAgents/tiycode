/**
 * Global Tauri event listeners for thread lifecycle changes.
 *
 * Listens for thread-run-started, thread-run-finished, and thread-title-updated
 * events from the backend, dispatching them through the run-state machine and
 * thread store. This hook replaces three useEffect blocks in DashboardWorkbench.
 *
 * Returns a `sidebarAutoRefreshUntilRef` that can be shared with the sidebar
 * auto-poll hook so the poll knows when to extend its grace period.
 */
import { useEffect, useRef, type MutableRefObject } from "react";
import { isTauri } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import {
  dispatchGlobalEvent,
  dispatchRunFinishedEvent,
} from "@/modules/workbench-shell/model/run-event-dispatcher";
import {
  threadStore,
  updateThreadTitle as setStoreThreadTitle,
} from "@/modules/workbench-shell/model/thread-store";
import { SIDEBAR_AUTO_REFRESH_GRACE_MS } from "@/modules/workbench-shell/ui/dashboard-workbench-logic";

export interface GlobalThreadEventsOptions {
  /** Sidebar sync function — ref-stable wrapper around the coalesced runner. */
  syncWorkspaceSidebar: () => Promise<void>;
}

export interface GlobalThreadEventsResult {
  /** Shared ref between event listeners and the sidebar auto-poll hook. */
  sidebarAutoRefreshUntilRef: MutableRefObject<number>;
}

export function useGlobalThreadEvents(
  opts: GlobalThreadEventsOptions,
): GlobalThreadEventsResult {
  const { syncWorkspaceSidebar } = opts;

  // Keep the auto-refresh grace period as a ref so the event handlers can
  // extend it without re-creating the effect (avoids unlisten/re-listen).
  const sidebarAutoRefreshUntilRef = useRef(0);

  useEffect(() => {
    if (!isTauri()) {
      return;
    }

    const unlistenPromises: Array<Promise<UnlistenFn>> = [];

    unlistenPromises.push(
      listen<{ threadId: string; runId: string }>(
        "thread-run-started",
        (event) => {
          const { threadId, runId } = event.payload;
          dispatchGlobalEvent(threadId, "RUN_STARTED", { runId });
          sidebarAutoRefreshUntilRef.current =
            Date.now() + SIDEBAR_AUTO_REFRESH_GRACE_MS;
        },
      ),
    );

    unlistenPromises.push(
      listen<{ threadId: string; runId: string; status: string }>(
        "thread-run-finished",
        (event) => {
          const { threadId, runId, status } = event.payload;
          dispatchRunFinishedEvent(threadId, runId, status);
          void syncWorkspaceSidebar().catch(() => {});
        },
      ),
    );

    unlistenPromises.push(
      listen<{ threadId: string; title: string }>(
        "thread-title-updated",
        (event) => {
          const { threadId, title } = event.payload;
          const trimmedTitle = title.trim();
          if (!trimmedTitle) return;

          if (threadStore.getState().editingThreadId === threadId) return;
          setStoreThreadTitle(threadId, trimmedTitle);
        },
      ),
    );

    return () => {
      for (const promise of unlistenPromises) {
        void promise.then((unlisten) => unlisten());
      }
    };
  }, [syncWorkspaceSidebar]);

  return { sidebarAutoRefreshUntilRef };
}
