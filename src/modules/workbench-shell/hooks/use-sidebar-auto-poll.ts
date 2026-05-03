/**
 * Sidebar auto-poll hook.
 *
 * Polls the sidebar at a regular interval while live threads are active or
 * within the grace period after a global thread event was received. Works
 * in tandem with `useGlobalThreadEvents` — the caller passes the shared
 * `sidebarAutoRefreshUntilRef` from that hook so both can extend/read the
 * poll window.
 */
import { useEffect, type MutableRefObject } from "react";
import { isTauri } from "@tauri-apps/api/core";
import { useStore } from "@/shared/lib/create-store";
import { threadStore } from "@/modules/workbench-shell/model/thread-store";
import { projectStore } from "@/modules/workbench-shell/model/project-store";
import { getInvokeErrorMessage } from "@/shared/lib/invoke-error";
import {
  SIDEBAR_AUTO_REFRESH_GRACE_MS,
  SIDEBAR_AUTO_REFRESH_INTERVAL_MS,
} from "@/modules/workbench-shell/ui/dashboard-workbench-logic";

export interface SidebarAutoPollOptions {
  /** Ref-stable sync entrypoint (wraps the coalesced runner). */
  syncWorkspaceSidebar: () => Promise<void>;
  /** Shared ref from useGlobalThreadEvents. Both hooks extend/read this. */
  sidebarAutoRefreshUntilRef: MutableRefObject<number>;
  /** Whether the coalesced runner is currently executing a sync. */
  isSyncRunning: () => boolean;
}

export function useSidebarAutoPoll(opts: SidebarAutoPollOptions): void {
  const { syncWorkspaceSidebar, sidebarAutoRefreshUntilRef, isSyncRunning } =
    opts;

  // Subscribe to threads with live status so the poll knows when to activate.
  const hasSidebarLiveThreads = useStore(
    threadStore,
    (s) =>
      s.workspaces.some((workspace) =>
        workspace.threads.some(
          (thread) =>
            thread.status === "running" || thread.status === "needs-reply",
        ),
      ),
  );

  useEffect(() => {
    if (!isTauri() || typeof window === "undefined") {
      return;
    }

    // Extend the grace period whenever live threads appear.
    if (hasSidebarLiveThreads) {
      sidebarAutoRefreshUntilRef.current =
        Date.now() + SIDEBAR_AUTO_REFRESH_GRACE_MS;
    }

    const shouldPoll =
      hasSidebarLiveThreads ||
      Date.now() < sidebarAutoRefreshUntilRef.current;

    if (!shouldPoll) {
      return;
    }

    const interval = window.setInterval(() => {
      const withinGrace =
        Date.now() < sidebarAutoRefreshUntilRef.current;
      // Re-read from the store snapshot to catch status transitions that
      // happened after the effect started.
      const live = threadStore
        .getState()
        .workspaces.some((w) =>
          w.threads.some(
            (t) => t.status === "running" || t.status === "needs-reply"),
        );

      if (!live && !withinGrace) {
        window.clearInterval(interval);
        return;
      }

      if (isSyncRunning()) {
        return;
      }

      void syncWorkspaceSidebar().catch((error) => {
        const message = getInvokeErrorMessage(
          error,
          /* fallback */ "Failed to refresh thread list",
        );
        projectStore.setState({ terminalBootstrapError: message });
      });
    }, SIDEBAR_AUTO_REFRESH_INTERVAL_MS);

    return () => window.clearInterval(interval);
  }, [hasSidebarLiveThreads, syncWorkspaceSidebar, sidebarAutoRefreshUntilRef, isSyncRunning]);
}
