import type { LanguagePreference } from "@/app/providers/language-provider";
import type { MessageAttachmentDto, WorkspaceDto } from "@/shared/types/api";
import { buildProjectOptionFromPath } from "@/modules/workbench-shell/model/helpers";
import type {
  ProjectOption,
  WorkspaceItem,
} from "@/modules/workbench-shell/model/types";
import type { ThreadContextUsage } from "@/modules/workbench-shell/model/thread-store";

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
    parseTokenCount(options.fallbackContextWindow) ??
    parseTokenCount(options.runtimeUsage?.contextWindow);
  // Use the cross-protocol unified `contextSize` (= input + output +
  // cache_read + cache_write) as the "context occupancy" figure for the
  // badge. This is what tiycore 0.2.10-rc.2 exposes via
  // `Usage::context_size()` and works consistently across OpenAI /
  // Anthropic / Google. `totalTokens` is intentionally NOT used here — it
  // is the wire-level per-response total and is provider-dependent
  // (OpenAI/Google: prompt+completion; Anthropic: input+output+cache).
  const contextSize = options.runtimeUsage?.contextSize ?? 0;
  const inputTokens = options.runtimeUsage?.inputTokens ?? 0;
  const outputTokens = options.runtimeUsage?.outputTokens ?? 0;
  const cacheReadTokens = options.runtimeUsage?.cacheReadTokens ?? 0;
  const cacheWriteTokens = options.runtimeUsage?.cacheWriteTokens ?? 0;
  const totalTokens = options.runtimeUsage?.totalTokens ?? 0;
  // Anthropic / ZenMux(Anthropic) report cache reads as a separate bucket, but
  // they still count against the prompt context window and the provider's input
  // billing. Surface the combined "input" figure (raw input + cache hits) so
  // the header's `In … · Out …` numbers match the `used / total` total above.
  const effectiveInputTokens = inputTokens + cacheReadTokens;
  const rawUsedPercent =
    contextWindow && contextWindow > 0
      ? Math.round((contextSize / contextWindow) * 100)
      : 0;
  const usageRatio =
    contextWindow && contextWindow > 0
      ? Math.min(contextSize / contextWindow, 1)
      : 0;
  const usedPercent =
    contextWindow && contextWindow > 0
      ? Math.min(rawUsedPercent, 100)
      : 0;
  const leftPercent = Math.max(0, 100 - rawUsedPercent);
  const isExceeded = Boolean(
    contextWindow && contextWindow > 0 && contextSize > contextWindow,
  );

  return {
    contextWindow,
    inputTokens,
    outputTokens,
    cacheReadTokens,
    cacheWriteTokens,
    effectiveInputTokens,
    isExceeded,
    leftPercent,
    modelDisplayName:
      options.fallbackModelDisplayName ??
      options.runtimeUsage?.modelDisplayName ??
      null,
    rawUsedPercent,
    totalTokens,
    // New: expose the source of truth for the percentage so consumers can
    // label the figure precisely.
    contextSize,
    usageRatio,
    usedLabel: formatCompactTokenCount(contextSize),
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
  command?: import("@/modules/workbench-shell/model/composer-commands").ComposerCommandInvocation;
  threadId: string;
};
