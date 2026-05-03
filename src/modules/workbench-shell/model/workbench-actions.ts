/**
 * Cross-store orchestration actions for the workbench.
 *
 * These functions coordinate multiple domain stores (threadStore, projectStore,
 * composerStore, uiLayoutStore, settingsStore, terminalStore) and handle IPC
 * calls that span multiple domains. They are NOT React hooks — they use
 * `getState()` / `setState()` directly so they can be called from any context.
 *
 * @remarks
 * - IPC calls come BEFORE store writes to avoid inconsistent UI state on failure.
 * - After an `await`, always re-read store state instead of using cached values
 *   (JavaScript's single-threaded model means synchronous getState/setState
 *   sequences are atomic, but `await` yields the microtask queue).
 */
import { isTauri } from "@tauri-apps/api/core";
import { threadCreate, threadDelete, threadList } from "@/services/bridge/thread-commands";
import { workspaceRemove, workspaceAdd, workspaceList, workspaceEnsureDefault } from "@/services/bridge/workspace-commands";
import { threadStore } from "./thread-store";
import { projectStore } from "./project-store";
import { composerStore } from "./composer-store";
import { uiLayoutStore } from "./ui-layout-store";
import { settingsStore } from "@/modules/settings-center/model/settings-store";
import { terminalStore } from "@/features/terminal/model/terminal-store";
import { getInvokeErrorMessage } from "@/shared/lib/invoke-error";
import {
  activateThread,
  clearActiveThreads,
  buildThreadTitle,
  mergeRecentProjects,
  buildWorkspaceItemsFromDtos,
  getActiveThread,
} from "@/modules/workbench-shell/model/helpers";
import { isSameWorkspacePath } from "@/shared/lib/workspace-path";
import {
  buildProjectOptionFromWorkspace,
  findWorkspaceForThread,
  getNewThreadTerminalBindingKey,
  mergeLocalFallbackThreads,
  resolveThreadProfileId,
  type PendingThreadRun,
} from "@/modules/workbench-shell/ui/dashboard-workbench-logic";
import {
  buildWorkspaceBindings,
  buildWorkspaceBindingsForEntry,
  findWorkspaceByPath,
} from "@/modules/workbench-shell/model/workspace-path-bindings";
import type { ProjectOption, WorkspaceItem } from "@/modules/workbench-shell/model/types";
import type { MessageAttachmentDto, RunMode, ThreadSummaryDto, WorkspaceDto } from "@/shared/types/api";
import type { LanguagePreference } from "@/app/providers/language-provider";

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

export interface NewThreadSubmission {
  value: string;
  runMode: RunMode;
  displayText?: string;
  effectivePrompt: string;
  attachments?: MessageAttachmentDto[];
  metadata?: Record<string, unknown> | null;
  commandBehavior?: "clear" | "compact" | "none" | "prompt";
}

export interface ThreadDeleteOptions {
  /** If true, skip IPC (used in non-Tauri environments). */
  skipIpc?: boolean;
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/** Pending thread creation promises, keyed by workspace ID. Used for deduplication. */
const pendingCreations: Record<string, Promise<string>> = {};

// ---------------------------------------------------------------------------
// getOrCreateNewThreadId
// ---------------------------------------------------------------------------

/**
 * Get an existing terminal-thread binding for a workspace, or create a new
 * thread via IPC. Deduplicates concurrent creation requests for the same
 * workspace.
 *
 * After creation, the thread's profile is persisted in the threadStore and
 * the binding is recorded in the projectStore.
 */
export async function getOrCreateNewThreadId(workspaceId: string): Promise<string> {
  const bindingKey = getNewThreadTerminalBindingKey(workspaceId);
  const existingBinding = projectStore.getState().terminalThreadBindings[bindingKey] ?? null;
  if (existingBinding) {
    return existingBinding;
  }

  const inFlight = pendingCreations[workspaceId];
  if (inFlight) {
    return inFlight;
  }

  const activeAgentProfileId = settingsStore.getState().activeAgentProfileId;

  const creationPromise = threadCreate(workspaceId, "", activeAgentProfileId)
    .then((thread) => {
      // Update thread profile in store
      threadStore.setState((prev) => ({
        workspaces: prev.workspaces.map((workspace) => ({
          ...workspace,
          threads: workspace.threads.map((candidate) =>
            candidate.id === thread.id
              ? { ...candidate, profileId: thread.profileId }
              : candidate,
          ),
        })),
      }));

      // Record profile override
      threadStore.setState({
        activeThreadProfileIdOverride: thread.profileId ?? activeAgentProfileId,
      });

      // Record terminal binding
      projectStore.setState((prev) => {
        const currentBinding = prev.terminalThreadBindings[bindingKey];
        if (currentBinding === thread.id) {
          return {};
        }
        return {
          terminalThreadBindings: {
            ...prev.terminalThreadBindings,
            [bindingKey]: thread.id,
          },
        };
      });

      return thread.id;
    })
    .finally(() => {
      delete pendingCreations[workspaceId];
    });

  pendingCreations[workspaceId] = creationPromise;
  return creationPromise;
}

// ---------------------------------------------------------------------------
// selectThread
// ---------------------------------------------------------------------------

/**
 * Select a thread in the sidebar. Transitions out of new-thread mode, aligns
 * the selected project with the thread's workspace, and sets the profile
 * override based on the thread's persisted profile.
 */
export function selectThread(threadId: string): void {
  const { workspaces } = threadStore.getState();
  const { selectedProject: currentProject } = projectStore.getState();
  const activeAgentProfileId = settingsStore.getState().activeAgentProfileId;

  const nextActiveThread = workspaces
    .flatMap((w) => w.threads)
    .find((t) => t.id === threadId) ?? null;

  const resolvedProfileId = resolveThreadProfileId(
    nextActiveThread?.profileId ?? null,
    activeAgentProfileId,
  );

  // Clear new-thread terminal binding if we were in that mode
  if (currentProject) {
    const bindingKey = getNewThreadTerminalBindingKey(currentProject.id);
    projectStore.setState((prev) => {
      if (!(bindingKey in prev.terminalThreadBindings)) return {};
      const next = { ...prev.terminalThreadBindings };
      delete next[bindingKey];
      return { terminalThreadBindings: next };
    });
  }

  threadStore.setState({
    isNewThreadMode: false,
    activeThreadProfileIdOverride: resolvedProfileId,
    editingThreadId: null,
  });
  threadStore.setState((prev) => ({
    workspaces: activateThread(prev.workspaces, threadId),
  }));

  // Align selected project with the thread's workspace
  const nextWorkspace = workspaces.find((w) =>
    w.threads.some((t) => t.id === threadId),
  );
  if (nextWorkspace && nextWorkspace.id !== currentProject?.id) {
    const { recentProjects } = projectStore.getState();
    const projectForWorkspace = recentProjects.find(
      (p) => p.id === nextWorkspace.id,
    );
    if (projectForWorkspace) {
      projectStore.setState({
        selectedProject: {
          ...projectForWorkspace,
          lastOpenedLabel: "Just now",
        },
      });
    }
  }

  uiLayoutStore.setState({ activeWorkspaceMenuId: null });
}

// ---------------------------------------------------------------------------
// selectProject
// ---------------------------------------------------------------------------

/** Select a project for the new-thread empty state. */
export function selectProject(project: ProjectOption): void {
  const nextProject = {
    ...project,
    lastOpenedLabel: "Just now",
  };
  projectStore.setState((prev) => ({
    selectedProject: nextProject,
    recentProjects: mergeRecentProjects(prev.recentProjects, nextProject),
  }));
}

// ---------------------------------------------------------------------------
// activateWorkspace
// ---------------------------------------------------------------------------

/**
 * Activate a workspace as the new-thread target. Clears any existing new-thread
 * binding, sets the project, expands the workspace in the sidebar, and resets
 * composer/composer error/delete phase.
 */
export function activateWorkspace(workspaceId: string, project: ProjectOption): void {
  // Clear existing new-thread binding
  const bindingKey = getNewThreadTerminalBindingKey(workspaceId);
  projectStore.setState((prev) => {
    if (!(bindingKey in prev.terminalThreadBindings)) return {};
    const next = { ...prev.terminalThreadBindings };
    delete next[bindingKey];
    return { terminalThreadBindings: next };
  });

  // Set project + recent
  projectStore.setState((prev) => ({
    selectedProject: project,
    recentProjects: mergeRecentProjects(prev.recentProjects, project),
  }));

  // Expand workspace in sidebar and clear active threads
  threadStore.setState((prev) => ({
    workspaces: clearActiveThreads(prev.workspaces),
    openWorkspaces: { ...prev.openWorkspaces, [workspaceId]: true },
  }));

  // Reset workbench state
  threadStore.setState({
    isNewThreadMode: true,
    activeThreadProfileIdOverride: null,
    editingThreadId: null,
  });
  composerStore.setState({ error: null });
  projectStore.setState({ terminalBootstrapError: null });
  uiLayoutStore.setState({ activeWorkspaceMenuId: null });
}

// ---------------------------------------------------------------------------
// deleteThread / confirmDeleteThread
// ---------------------------------------------------------------------------

/**
 * Delete a thread (IPC + store cleanup).
 *
 * Removes the thread from all workspaces, cleans up terminal sessions,
 * terminal bindings, pending runs, and UI state. If the deleted thread is the
 * active thread, transitions to new-thread mode.
 */
export async function deleteThread(threadId: string, options?: ThreadDeleteOptions): Promise<void> {
  if (options?.skipIpc !== true && isTauri()) {
    await threadDelete(threadId);
  }

  const { workspaces, activeThreadId } = threadStore.getState();
  const isDeletingActiveThread = activeThreadId === threadId;

  // Find the workspace that owns this thread (for active project fallback)
  const activeThreadWorkspace = findWorkspaceForThread(workspaces, threadId);

  // Clean up terminal session
  terminalStore.removeSession(threadId);

  // Remove thread from workspaces
  threadStore.setState((prev) => {
    const next = prev.workspaces.map((w) => ({
      ...w,
      threads: w.threads.filter((t) => t.id !== threadId),
    }));
    return {
      workspaces: isDeletingActiveThread ? clearActiveThreads(next) : next,
    };
  });

  // Remove pending runs
  threadStore.setState((prev) => {
    if (!(threadId in prev.pendingRuns)) return {};
    const next = { ...prev.pendingRuns };
    delete next[threadId];
    return { pendingRuns: next };
  });

  // Clean terminal collapsed state
  uiLayoutStore.setState((prev) => {
    if (!(threadId in prev.terminalCollapsedByThreadKey)) return {};
    const next = { ...prev.terminalCollapsedByThreadKey };
    delete next[threadId];
    return { terminalCollapsedByThreadKey: next };
  });

  // Clean terminal bindings
  projectStore.setState((prev) => {
    const next: Record<string, string> = {};
    for (const [key, tid] of Object.entries(prev.terminalThreadBindings)) {
      if (tid !== threadId) {
        next[key] = tid;
      }
    }
    return { terminalThreadBindings: next };
  });

  // If deleting active thread, transition to new-thread mode
  if (isDeletingActiveThread) {
    const { recentProjects } = projectStore.getState();
    const activeThreadProject = activeThreadWorkspace
      ? recentProjects.find((p) => p.id === activeThreadWorkspace.id) ?? null
      : null;

    projectStore.setState({
      selectedProject: activeThreadProject ?? projectStore.getState().selectedProject,
    });
    threadStore.setState({
      isNewThreadMode: true,
      activeThreadProfileIdOverride: null,
    });
    composerStore.setState({ error: null });
  }
}

// ---------------------------------------------------------------------------
// removeWorkspace
// ---------------------------------------------------------------------------

/**
 * Remove a workspace (IPC + store cleanup).
 *
 * Handles worktree confirmation dialogs, cleans up threads, terminal bindings,
 * pending runs, open workspace states, and ensures the selected project stays
 * valid.
 */
export async function removeWorkspace(workspace: WorkspaceItem): Promise<void> {
  const workspaceThreadIds = new Set(workspace.threads.map((t) => t.id));
  const nextThreadBindingKey = getNewThreadTerminalBindingKey(workspace.id);

  const { selectedProject, recentProjects } = projectStore.getState();

  const isRemovingSelectedProject =
    selectedProject?.id === workspace.id
    || isSameWorkspacePath(selectedProject?.path, workspace.path);

  const fallbackSelectedProject = isRemovingSelectedProject
    ? (recentProjects.find(
        (p) =>
          p.id !== workspace.id
          && !isSameWorkspacePath(p.path, workspace.path),
      ) ?? null)
    : selectedProject;

  // Clean up pending thread creations for this workspace
  delete pendingCreations[workspace.id];

  await workspaceRemove(workspace.id, true);

  // Re-read store state after async IPC to avoid stale state
  const { activeThreadId, workspaces } = threadStore.getState();
  const activeThreadWorkspace = findWorkspaceForThread(workspaces, activeThreadId ?? "");
  const isRemovingActiveWorkspace = activeThreadWorkspace?.id === workspace.id;

  // Transition to new-thread mode if removing the active workspace
  if (isRemovingActiveWorkspace) {
    threadStore.setState({
      isNewThreadMode: true,
      workspaces: clearActiveThreads(threadStore.getState().workspaces),
    });
    composerStore.setState({ error: null });
  }

  // Update selected project if needed
  if (isRemovingSelectedProject) {
    projectStore.setState({ selectedProject: fallbackSelectedProject });
  }

  // Remove from recent projects
  projectStore.setState((prev) => ({
    recentProjects: prev.recentProjects.filter(
      (p) =>
        p.id !== workspace.id
        && !isSameWorkspacePath(p.path, workspace.path),
    ),
  }));

  // Clean pending runs for threads in this workspace
  threadStore.setState((prev) => ({
    pendingRuns: Object.fromEntries(
      Object.entries(prev.pendingRuns).filter(
        ([tid]) => !workspaceThreadIds.has(tid),
      ),
    ),
  }));

  // Clean terminal bindings
  projectStore.setState((prev) => ({
    terminalThreadBindings: Object.fromEntries(
      Object.entries(prev.terminalThreadBindings).filter(
        ([bindingKey, tid]) =>
          bindingKey !== nextThreadBindingKey
          && !workspaceThreadIds.has(tid),
      ),
    ),
    terminalWorkspaceBindings: Object.fromEntries(
      Object.entries(prev.terminalWorkspaceBindings).filter(
        ([, wsId]) => wsId !== workspace.id,
      ),
    ),
  }));

  // Clean terminal collapsed state for removed threads
  uiLayoutStore.setState((prev) => {
    const next = { ...prev.terminalCollapsedByThreadKey };
    let changed = false;
    for (const key of Object.keys(next)) {
      if (workspaceThreadIds.has(key)) {
        delete next[key];
        changed = true;
      }
    }
    return changed ? { terminalCollapsedByThreadKey: next } : {};
  });

  // Clean open workspace state
  threadStore.setState((prev) => {
    if (!(workspace.id in prev.openWorkspaces)) return {};
    const next = { ...prev.openWorkspaces };
    delete next[workspace.id];
    return { openWorkspaces: next };
  });

  uiLayoutStore.setState({ activeWorkspaceMenuId: null });
}

// ---------------------------------------------------------------------------
// submitNewThread
// ---------------------------------------------------------------------------

/**
 * Submit a new thread from the composer.
 *
 * This is the most complex cross-store action. It:
 * 1. Ensures the backend workspace exists (if needed)
 * 2. Reads or creates a terminal-bound thread
 * 3. Builds the optimistic thread entry
 * 4. Updates all relevant stores
 * 5. Transitions out of new-thread mode
 */
export async function submitNewThread(submission: NewThreadSubmission): Promise<void> {
  let { selectedProject } = projectStore.getState();
  if (!selectedProject) {
    if (!isTauri()) return;
    const defaultWorkspace = await workspaceEnsureDefault();
    selectedProject = {
      id: defaultWorkspace.id,
      name: defaultWorkspace.name,
      path: defaultWorkspace.canonicalPath || defaultWorkspace.path,
      lastOpenedLabel: "Just now",
    };
  }

  const { activeAgentProfileId } = settingsStore.getState();

  let project = selectedProject;
  let workspaceId = project.id;

  // Ensure backend workspace exists in Tauri
  if (isTauri()) {
    try {
      const projectToEnsure = project;
      const workspaceEntries = await workspaceList();
      const existingWorkspace = findWorkspaceByPath(
        workspaceEntries as unknown as WorkspaceDto[],
        projectToEnsure.path,
      );

      const ensuredWorkspace: WorkspaceDto = existingWorkspace
        ? existingWorkspace
        : await workspaceAdd(projectToEnsure.path, projectToEnsure.name);

      const ensuredProject =
        buildProjectOptionFromWorkspace(ensuredWorkspace) ?? {
          id: ensuredWorkspace.id,
          name: ensuredWorkspace.name,
          path: ensuredWorkspace.canonicalPath || ensuredWorkspace.path,
          lastOpenedLabel: "Just now",
        };

      project = ensuredProject;
      workspaceId = ensuredWorkspace.id;

      // Update project store with ensured workspace data
      projectStore.setState((prev) => ({
        selectedProject: ensuredProject,
        recentProjects: mergeRecentProjects(prev.recentProjects, {
          ...ensuredProject,
          lastOpenedLabel: "Just now",
        }),
        terminalWorkspaceBindings: {
          ...prev.terminalWorkspaceBindings,
          ...buildWorkspaceBindingsForEntry(
            ensuredWorkspace,
            projectToEnsure?.path,
          ),
        },
      }));
    } catch (error) {
      throw new Error(
        getInvokeErrorMessage(error, "Failed to initialize workspace"),
      );
    }
  }

  // Re-read workspaces after async IPC to avoid stale state
  const { workspaces } = threadStore.getState();
  const { newThreadRunMode } = composerStore.getState();

  // Find or match the workspace in the sidebar
  const existingWorkspace =
    workspaces.find(
      (w) =>
        w.id === workspaceId
        || w.id === project.id,
    )
    ?? workspaces.find(
      (w) =>
        w.name === project.name
        || isSameWorkspacePath(w.path, project.path),
    )
    ?? null;

  const nextPendingRunId =
    typeof crypto !== "undefined" && "randomUUID" in crypto
      ? crypto.randomUUID()
      : `${Date.now()}`;

  // Get or create the terminal-bound thread
  let threadId: string;
  try {
    threadId = await getOrCreateNewThreadId(workspaceId);
  } catch (error) {
    throw new Error(
      getInvokeErrorMessage(error, "Failed to create thread"),
    );
  }

  const nextThreadName = buildThreadTitle(submission.value || submission.effectivePrompt);

  const nextThread = {
    id: threadId,
    profileId: activeAgentProfileId,
    name: nextThreadName,
    time: "Just now",
    active: true,
    status: "running" as const,
  };

  // Update project store
  projectStore.setState((prev) => ({
    selectedProject: project,
    recentProjects: mergeRecentProjects(prev.recentProjects, project),
  }));

  // Update workspaces with the new thread
  threadStore.setState((prev) => {
    const cleared = clearActiveThreads(prev.workspaces);

    if (existingWorkspace) {
      return {
        workspaces: cleared.map((w) =>
          w.id === existingWorkspace.id
            ? {
                ...w,
                name: project.name,
                path: project.path,
                threads: [nextThread, ...w.threads],
              }
            : w,
        ),
      };
    }

    return {
      workspaces: [
        {
          id: workspaceId,
          name: project.name,
          defaultOpen: true,
          path: project.path,
          kind: project.kind,
          parentWorkspaceId: project.parentWorkspaceId,
          worktreeHash: project.worktreeHash ?? null,
          branch: project.branch ?? null,
          threads: [nextThread],
        },
        ...cleared,
      ],
    };
  });

  // Expand the workspace in sidebar
  threadStore.setState((prev) => ({
    openWorkspaces: {
      ...prev.openWorkspaces,
      [existingWorkspace?.id ?? workspaceId]: true,
    },
  }));

  // Add pending run
  threadStore.setState((prev) => {
    const newRun: PendingThreadRun = {
      id: nextPendingRunId,
      displayText: submission.displayText ?? submission.value,
      effectivePrompt: submission.effectivePrompt,
      attachments: (submission.attachments ?? []) as unknown as PendingThreadRun["attachments"],
      metadata: submission.metadata ?? null,
      runMode: submission.runMode ?? newThreadRunMode,
      threadId,
    };
    return {
      pendingRuns: {
        ...prev.pendingRuns,
        [threadId]: newRun,
      },
    };
  });

  // Bind profile to thread
  threadStore.setState((prev) => ({
    workspaces: prev.workspaces.map((w) => ({
      ...w,
      threads: w.threads.map((t) =>
        t.id === threadId
          ? { ...t, profileId: activeAgentProfileId }
          : t,
      ),
    })),
  }));
  threadStore.setState({ activeThreadProfileIdOverride: activeAgentProfileId });

  // Transition to thread mode
  threadStore.setState({ isNewThreadMode: false });

  // Clear new-thread terminal binding
  const bindingKey = getNewThreadTerminalBindingKey(workspaceId);
  projectStore.setState((prev) => {
    if (!(bindingKey in prev.terminalThreadBindings)) return {};
    const next = { ...prev.terminalThreadBindings };
    delete next[bindingKey];
    return { terminalThreadBindings: next };
  });

  // Reset composer
  composerStore.setState({
    newThreadValue: "",
    newThreadRunMode: "default",
    error: null,
  });
}

// ---------------------------------------------------------------------------
// enterNewThreadMode
// ---------------------------------------------------------------------------

/** Enter new-thread mode, clearing the active thread and resetting state. */
export function enterNewThreadMode(): void {
  const { selectedProject } = projectStore.getState();

  if (selectedProject) {
    const bindingKey = getNewThreadTerminalBindingKey(selectedProject.id);
    projectStore.setState((prev) => {
      if (!(bindingKey in prev.terminalThreadBindings)) return {};
      const next = { ...prev.terminalThreadBindings };
      delete next[bindingKey];
      return { terminalThreadBindings: next };
    });
  }

  threadStore.setState({
    isNewThreadMode: true,
    activeThreadProfileIdOverride: null,
  });
  threadStore.setState((prev) => ({
    workspaces: clearActiveThreads(prev.workspaces),
  }));
  composerStore.setState({ error: null });
  projectStore.setState({ terminalBootstrapError: null });
}

// ---------------------------------------------------------------------------
// Sidebar sync — extracted from DashboardWorkbench
// ---------------------------------------------------------------------------

let sidebarSyncVersion = 0;

export interface SidebarSyncOptions {
  language: LanguagePreference;
  preserveSelectedProjectIfMissing?: boolean;
  threadDisplayCountOverrides: Record<string, number>;
}

export async function performSidebarSync(options: SidebarSyncOptions): Promise<void> {
  const syncStart = performance.now();
  const version = ++sidebarSyncVersion;

  const WORKSPACE_THREAD_PAGE_SIZE = 10;

  const t0 = performance.now();
  console.log(`⏱ [sidebar-sync] firing workspaceList() at ${t0.toFixed(1)}ms since page load`);
  const workspaceEntries = await workspaceList();
  console.log(`⏱ [sidebar-sync] workspaceList: ${(performance.now() - t0).toFixed(1)}ms (${workspaceEntries.length} workspaces)`);

  const nextDisplayCounts = Object.fromEntries(
    workspaceEntries.map((workspace) => [
      workspace.id,
      options.threadDisplayCountOverrides[workspace.id] ??
        threadStore.getState().displayCounts[workspace.id] ??
        WORKSPACE_THREAD_PAGE_SIZE,
    ]),
  );

  const { terminalThreadBindings } = projectStore.getState();

  const listVisibleWorkspaceThreads = async (
    workspaceId: string,
    visibleLimit: number,
  ): Promise<{ hasMore: boolean; threads: Array<ThreadSummaryDto> }> => {
    const desiredVisibleCount = visibleLimit + 1;
    const pendingTerminalThreadIds = new Set(Object.values(terminalThreadBindings));
    const visibleThreads: Array<ThreadSummaryDto> = [];
    let offset = 0;
    let hasMoreRawThreads = true;

    while (visibleThreads.length < desiredVisibleCount && hasMoreRawThreads) {
      const rawLimit = Math.max(
        WORKSPACE_THREAD_PAGE_SIZE + 1,
        desiredVisibleCount - visibleThreads.length,
      );
      const batch = await threadList(workspaceId, rawLimit, offset);
      offset += batch.length;
      hasMoreRawThreads = batch.length === rawLimit;

      for (const thread of batch) {
        if (pendingTerminalThreadIds.has(thread.id)) continue;
        visibleThreads.push(thread);
        if (visibleThreads.length >= desiredVisibleCount) break;
      }

      if (batch.length === 0) break;
    }

    return {
      hasMore: visibleThreads.length > visibleLimit,
      threads: visibleThreads.slice(0, visibleLimit),
    };
  };

  const t1 = performance.now();
  const threadEntries = await Promise.all(
    workspaceEntries.map(
      async (workspace) =>
        [
          workspace.id,
          await listVisibleWorkspaceThreads(
            workspace.id,
            nextDisplayCounts[workspace.id] ?? WORKSPACE_THREAD_PAGE_SIZE,
          ),
        ] as const,
    ),
  );
  console.log(`⏱ [sidebar-sync] threadList for ${workspaceEntries.length} workspace(s): ${(performance.now() - t1).toFixed(1)}ms`);

  // Discard stale sync results
  if (sidebarSyncVersion !== version) return;

  const threadsByWorkspaceId = Object.fromEntries(
    threadEntries.map(([workspaceId, result]) => [workspaceId, result.threads]),
  );
  const nextHasMoreByWorkspaceId = Object.fromEntries(
    threadEntries.map(([workspaceId, result]) => [workspaceId, result.hasMore]),
  );

  const nextProjects = workspaceEntries
    .map((workspace) => buildProjectOptionFromWorkspace(workspace, options.language))
    .filter((project): project is ProjectOption => project !== null);

  const nextBindings = buildWorkspaceBindings(workspaceEntries);

  const defaultWorkspace = workspaceEntries.find((w) => w.isDefault) ?? null;
  const defaultProject = defaultWorkspace === null
    ? null
    : (nextProjects.find(
        (project) =>
          project.id === defaultWorkspace.id ||
          isSameWorkspacePath(project.path, defaultWorkspace.canonicalPath) ||
          isSameWorkspacePath(project.path, defaultWorkspace.path),
      ) ?? null);

  projectStore.setState((prev) => {
    const current = prev.terminalWorkspaceBindings;
    const liveWorkspaceIds = new Set(workspaceEntries.map((w) => w.id));
    const preservedAliases: Record<string, string> = {};
    for (const [pathKey, workspaceId] of Object.entries(current)) {
      if (liveWorkspaceIds.has(workspaceId)) {
        preservedAliases[pathKey] = workspaceId;
      }
    }
    const merged = { ...preservedAliases, ...nextBindings };

    const currentKeys = Object.keys(current);
    const mergedKeys = Object.keys(merged);
    if (currentKeys.length === mergedKeys.length) {
      let identical = true;
      for (const key of mergedKeys) {
        if (current[key] !== merged[key]) {
          identical = false;
          break;
        }
      }
      if (identical) return {};
    }
    return { terminalWorkspaceBindings: merged };
  });

  projectStore.setState({ recentProjects: nextProjects });
  threadStore.setState({ defaultWorkspaceId: defaultWorkspace?.id ?? null });
  threadStore.setState({ displayCounts: nextDisplayCounts });
  threadStore.setState({ hasMore: nextHasMoreByWorkspaceId });
  threadStore.setState((prev) => ({
    loadMorePending: Object.fromEntries(
      workspaceEntries.map((workspace) => [
        workspace.id,
        prev.loadMorePending[workspace.id] ?? false,
      ]),
    ),
  }));

  projectStore.setState((prev) => {
    const current = prev.selectedProject;
    if (current) {
      const matchingProject = nextProjects.find(
        (project) =>
          project.id === current.id || isSameWorkspacePath(project.path, current.path),
      ) ?? null;

      if (matchingProject) return { selectedProject: matchingProject };
      if (options.preserveSelectedProjectIfMissing ?? true) return {};
      return { selectedProject: defaultProject ?? nextProjects[0] ?? null };
    }
    return { selectedProject: defaultProject ?? nextProjects[0] ?? null };
  });

  threadStore.setState((prev) => {
    const activeThreadId = getActiveThread(prev.workspaces)?.id ?? null;
    const syncedWorkspaces = buildWorkspaceItemsFromDtos(
      workspaceEntries,
      threadsByWorkspaceId,
      activeThreadId,
      options.language,
    );
    const mergedWithFallbacks = mergeLocalFallbackThreads({
      currentWorkspaces: prev.workspaces,
      syncedWorkspaces,
    });

    const nextWorkspaces = mergedWithFallbacks.map((workspace) => {
      const currentWorkspace = prev.workspaces.find((c) => c.id === workspace.id) ?? null;
      if (!currentWorkspace) return workspace;

      return {
        ...workspace,
        threads: workspace.threads.map((thread) => {
          const currentThread = currentWorkspace.threads.find((c) => c.id === thread.id);
          if (!currentThread) return thread;

          const currentTitle = currentThread.name.trim();
          const syncedTitle = thread.name.trim();
          if (
            currentTitle &&
            currentTitle !== "New Thread" &&
            syncedTitle === "New Thread"
          ) {
            return { ...thread, name: currentThread.name };
          }
          return thread;
        }),
      };
    });

    const nextOpenWorkspaces: Record<string, boolean> = {};
    for (const workspace of workspaceEntries) {
      nextOpenWorkspaces[workspace.id] =
        prev.openWorkspaces[workspace.id] ??
        (workspace.isDefault || workspaceEntries.length === 1);
    }

    return { workspaces: nextWorkspaces, openWorkspaces: nextOpenWorkspaces };
  });

  console.log(`⏱ [sidebar-sync] total: ${(performance.now() - syncStart).toFixed(1)}ms`);
}
