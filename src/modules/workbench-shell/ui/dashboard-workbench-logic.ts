import type { LanguagePreference } from "@/app/providers/language-provider";
import type { RunState } from "@/services/thread-stream";
import type { MessageAttachmentDto, RunMode, WorkspaceDto } from "@/shared/types/api";
import { WORKSPACE_ITEMS } from "@/modules/workbench-shell/model/fixtures";
import { buildProjectOptionFromPath } from "@/modules/workbench-shell/model/helpers";
import type {
  ProjectOption,
  ThreadStatus as WorkbenchThreadStatus,
  ThreadRunStatus,
  WorkspaceItem,
} from "@/modules/workbench-shell/model/types";
import type { ThreadContextUsage } from "@/modules/workbench-shell/ui/runtime-thread-surface";

export function resolveThreadProfileId(
  threadProfileId: string | null,
  globalActiveProfileId: string,
): string {
  return threadProfileId || globalActiveProfileId;
}

export function resolveActiveThreadWorkbenchProfileId(
  threadProfileId: string | null,
  globalActiveProfileId: string,
): string {
  return threadProfileId || globalActiveProfileId;
}

export const NEW_THREAD_TERMINAL_KEY_SUFFIX = "__new_thread__";
export const UNBOUND_NEW_THREAD_TERMINAL_STATE_KEY = "__new_thread_pending__";
export const DEFAULT_TERMINAL_COLLAPSED = true;
export const WORKSPACE_THREAD_PAGE_SIZE = 10;
export const SIDEBAR_AUTO_REFRESH_INTERVAL_MS = 2_000;
export const SIDEBAR_AUTO_REFRESH_GRACE_MS = 20_000;
// Minimum gap between two fully independent `syncWorkspaceSidebar` executions.
// If a caller invokes it again within this window of the previous run finishing,
// the call is coalesced onto a single trailing run. Without this, any feedback
// loop elsewhere in the component (effect dependency on state that sync itself
// mutates) will saturate the IPC queue and block thread list rendering.
export const SIDEBAR_SYNC_MIN_GAP_MS = 300;

export function buildInitialWorkspaceThreadDisplayCounts() {
  return Object.fromEntries(
    WORKSPACE_ITEMS.map((workspace) => [
      workspace.id,
      Math.min(WORKSPACE_THREAD_PAGE_SIZE, workspace.threads.length),
    ]),
  );
}

export function buildInitialWorkspaceThreadHasMore() {
  return Object.fromEntries(
    WORKSPACE_ITEMS.map((workspace) => [
      workspace.id,
      workspace.threads.length > WORKSPACE_THREAD_PAGE_SIZE,
    ]),
  );
}

export function getNewThreadTerminalBindingKey(workspaceId: string) {
  return `${workspaceId}:${NEW_THREAD_TERMINAL_KEY_SUFFIX}`;
}

export function buildProjectOptionFromWorkspace(workspace: WorkspaceDto, language: LanguagePreference = "en"): ProjectOption | null {
  const project = buildProjectOptionFromPath(
    workspace.canonicalPath || workspace.path,
    language,
  );
  if (!project) {
    return null;
  }

  return {
    ...project,
    id: workspace.id,
    name: workspace.name,
    kind: workspace.kind,
    parentWorkspaceId: workspace.parentWorkspaceId ?? null,
    worktreeHash: workspace.worktreeName
      ? workspace.worktreeName.slice(0, 6)
      : null,
    branch: workspace.branch ?? null,
  };
}

export function findWorkspaceForThread(
  workspaces: ReadonlyArray<WorkspaceItem>,
  threadId: string | null,
) {
  if (!threadId) {
    return null;
  }

  return (
    workspaces.find((workspace) =>
      workspace.threads.some((thread) => thread.id === threadId),
    ) ?? null
  );
}

export function mergeLocalFallbackThreads(options: {
  currentWorkspaces: ReadonlyArray<WorkspaceItem>;
  syncedWorkspaces: ReadonlyArray<WorkspaceItem>;
}) {
  return options.syncedWorkspaces.map((workspace) => {
    const currentWorkspace =
      options.currentWorkspaces.find(
        (candidate) => candidate.id === workspace.id,
      ) ?? null;

    if (!currentWorkspace) {
      return workspace;
    }

    const syncedThreadIds = new Set(workspace.threads.map((thread) => thread.id));
    const fallbackThreads = currentWorkspace.threads.filter((thread) => {
      if (syncedThreadIds.has(thread.id)) {
        return false;
      }

      return true;
    });

    if (fallbackThreads.length === 0) {
      return workspace;
    }

    return {
      ...workspace,
      threads: [...workspace.threads, ...fallbackThreads],
    };
  });
}

export function mapRunStateToWorkbenchThreadStatus(
  state: RunState | "idle",
): WorkbenchThreadStatus {
  switch (state) {
    case "running":
      return "running";
    case "waiting_approval":
    case "limit_reached":
      return "needs-reply";
    case "interrupted":
      return "interrupted";
    case "failed":
      return "failed";
    default:
      return "completed";
  }
}

export function mapRunFinishedStatusToThreadStatus(
  status: string,
): WorkbenchThreadStatus {
  switch (status) {
    case "failed":
      return "failed";
    case "interrupted":
      return "interrupted";
    case "cancelled":
      return "interrupted";
    case "limit_reached":
      return "needs-reply";
    default:
      return "completed";
  }
}

function parseTokenCount(value: string | null | undefined) {
  if (!value) {
    return null;
  }

  const normalized = value.replace(/[^\d]/g, "");
  if (!normalized) {
    return null;
  }

  const parsed = Number.parseInt(normalized, 10);
  return Number.isFinite(parsed) ? parsed : null;
}

export function formatCompactTokenCount(value: number) {
  return new Intl.NumberFormat("en", {
    maximumFractionDigits: 1,
    notation: "compact",
  }).format(value);
}

export function buildThreadContextBadgeData(options: {
  fallbackContextWindow: string | null;
  fallbackModelDisplayName: string | null;
  runtimeUsage: ThreadContextUsage | null;
}) {
  const contextWindow =
    parseTokenCount(options.runtimeUsage?.contextWindow) ??
    parseTokenCount(options.fallbackContextWindow);
  const totalTokens = options.runtimeUsage?.totalTokens ?? 0;
  const inputTokens = options.runtimeUsage?.inputTokens ?? 0;
  const outputTokens = options.runtimeUsage?.outputTokens ?? 0;
  const cacheReadTokens = options.runtimeUsage?.cacheReadTokens ?? 0;
  const cacheWriteTokens = options.runtimeUsage?.cacheWriteTokens ?? 0;
  const usageRatio =
    contextWindow && contextWindow > 0
      ? Math.min(totalTokens / contextWindow, 1)
      : 0;
  const usedPercent =
    contextWindow && contextWindow > 0
      ? Math.min(Math.round((totalTokens / contextWindow) * 100), 100)
      : 0;
  const leftPercent = Math.max(0, 100 - usedPercent);

  return {
    contextWindow,
    inputTokens,
    outputTokens,
    cacheReadTokens,
    cacheWriteTokens,
    leftPercent,
    modelDisplayName:
      options.runtimeUsage?.modelDisplayName ??
      options.fallbackModelDisplayName,
    totalTokens,
    usageRatio,
    usedLabel: formatCompactTokenCount(totalTokens),
    totalLabel: contextWindow ? formatCompactTokenCount(contextWindow) : "N/A",
    usedPercent,
  };
}

export type PendingThreadRun = {
  id: string;
  displayText: string;
  effectivePrompt: string;
  attachments: MessageAttachmentDto[];
  metadata: Record<string, unknown> | null;
  runMode: RunMode;
  threadId: string;
};

// ---------------------------------------------------------------------------
// Phase 1: RunState → ThreadRunStatus mapping
// ---------------------------------------------------------------------------

/**
 * Map a {@link RunState} (the legacy per-surface union) to the unified
 * {@link ThreadRunStatus} type.  The two types are structurally identical
 * at this point, but the explicit mapping guards against future divergence.
 */
export function mapRunStateToThreadRunStatus(
  state: RunState | "idle",
): ThreadRunStatus {
  return state as ThreadRunStatus;
}

/**
 * Map a run-finished status string (from the `thread-run-finished` Tauri
 * event payload) to a {@link ThreadRunStatus} value.
 */
export function mapRunFinishedStatusToThreadRunStatus(
  status: string,
): ThreadRunStatus {
  switch (status) {
    case "failed":
      return "failed";
    case "interrupted":
      return "interrupted";
    case "cancelled":
      return "cancelled";
    case "limit_reached":
      return "limit_reached";
    default:
      return "completed";
  }
}
