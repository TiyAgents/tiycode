/**
 * Subscribes to Git events for a workspace and surfaces the snapshot via
 * a local hook state. Consumed by TopBar for branch display.
 */
import { useEffect, useState } from "react";
import { isTauri } from "@tauri-apps/api/core";
import { gitSubscribe, gitGetSnapshot } from "@/services/bridge";
import type { GitSnapshotDto } from "@/shared/types/api";

export interface GitSnapshotState {
  snapshot: GitSnapshotDto | null;
}

/**
 * Returns the latest Git snapshot for the given workspace.
 *
 * @param workspaceId - The workspace to subscribe to, or null to unsubscribe.
 */
export function useGitSnapshot(workspaceId: string | null): GitSnapshotState {
  const [snapshot, setSnapshot] = useState<GitSnapshotDto | null>(null);

  useEffect(() => {
    if (!isTauri() || !workspaceId) {
      setSnapshot(null);
      return;
    }

    // Clear stale snapshot immediately
    setSnapshot(null);

    let cancelled = false;
    let unsubscribe: (() => Promise<void>) | null = null;

    // Subscribe first so we don't miss events during the initial fetch
    void gitSubscribe(workspaceId, (event) => {
      if (cancelled) return;
      if (event.type === "snapshot_updated") {
        setSnapshot(event.snapshot);
      }
    })
      .then((nextUnsubscribe) => {
        if (cancelled) {
          void nextUnsubscribe().catch(() => {});
          return;
        }
        unsubscribe = nextUnsubscribe;
      })
      .catch(() => {});

    // Then fetch the initial snapshot
    void gitGetSnapshot(workspaceId)
      .then((initialSnapshot) => {
        if (!cancelled && initialSnapshot) {
          setSnapshot(initialSnapshot);
        }
      })
      .catch(() => {});

    return () => {
      cancelled = true;
      if (unsubscribe) {
        void unsubscribe().catch(() => {});
      }
    };
  }, [workspaceId]);

  return { snapshot };
}
