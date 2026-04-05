import {
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
  type MouseEvent as ReactMouseEvent,
} from "react";
import { invoke, isTauri } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { open } from "@tauri-apps/plugin-dialog";
import {
  Boxes,
  ChevronDown,
  Folder,
  FolderOpen,
  FolderPlus,
  GitBranch,
  LoaderCircle,
  MessageSquarePlus,
  MoreHorizontal,
  Trash2,
} from "lucide-react";
import {
  useLanguage,
  type LanguagePreference,
} from "@/app/providers/language-provider";
import { useTheme, type ThemePreference } from "@/app/providers/theme-provider";
import { useMarketplaceController } from "@/modules/marketplace-center/model/use-marketplace-controller";
import { MarketplaceOverlay } from "@/modules/marketplace-center/ui/marketplace-overlay";
import {
  buildProfileModelPlan,
  buildRunModelPlanFromSelection,
} from "@/modules/settings-center/model/run-model-plan";
import type { ComposerSubmission } from "@/modules/workbench-shell/model/composer-commands";
import {
  useSettingsController,
  type SettingsCategory,
} from "@/modules/settings-center/model/use-settings-controller";
import { AI_ELEMENTS_THREAD_TITLE } from "@/modules/workbench-shell/model/ai-elements-task-demo";
import { SettingsCenterOverlay } from "@/modules/settings-center/ui/settings-center-overlay";
import { ThreadTerminalPanel } from "@/features/terminal/ui/thread-terminal-panel";
import type {
  MessageAttachmentDto,
  RunMode,
  ThreadSummaryDto,
  WorkspaceDto,
} from "@/shared/types/api";
import {
  threadCreate,
  threadDelete,
  threadList,
  workspaceAdd,
  workspaceList,
  workspaceRemove,
  workspaceSetDefault,
} from "@/services/bridge";
import type { RunState } from "@/services/thread-stream";
import {
  DEFAULT_TERMINAL_HEIGHT,
  DRAWER_LIST_LABEL_CLASS,
  DRAWER_LIST_ROW_CLASS,
  DRAWER_LIST_STACK_CLASS,
  LANGUAGE_OPTIONS,
  MIN_TERMINAL_HEIGHT,
  MIN_WORKBENCH_HEIGHT,
  MOCK_USER_SESSION,
  PANEL_VISIBILITY_STORAGE_KEY,
  RECENT_PROJECTS,
  THEME_OPTIONS,
  TOPBAR_HEIGHT,
  UPDATE_STATUS_DURATION,
  WORKSPACE_ITEMS,
  DRAWER_OVERFLOW_ACTION_CLASS,
} from "@/modules/workbench-shell/model/fixtures";
import {
  activateThread,
  buildProjectOptionFromPath,
  buildInitialWorkspaces,
  buildWorkspaceItemsFromDtos,
  buildThreadTitle,
  clearActiveThreads,
  getActiveThread,
  isEditableSelectionTarget,
  isNodeInsideContainer,
  mergeRecentProjects,
  readPanelVisibilityState,
  readStoredUserSession,
  selectContainerContents,
} from "@/modules/workbench-shell/model/helpers";
import type {
  DrawerPanel,
  PanelVisibilityState,
  ProjectOption,
  ThreadStatus as WorkbenchThreadStatus,
  WorkbenchOverlay,
  WorkspaceItem,
} from "@/modules/workbench-shell/model/types";
import { NewThreadEmptyState } from "@/modules/workbench-shell/ui/new-thread-empty-state";
import { ProjectPanel } from "@/modules/workbench-shell/ui/project-panel";
import {
  RuntimeThreadSurface,
  type ThreadContextUsage,
} from "@/modules/workbench-shell/ui/runtime-thread-surface";
import {
  GitDiffPreviewPanel,
  GitPanel,
  type GitDiffSelection,
} from "@/modules/workbench-shell/ui/source-control-panels";
import { ThreadStatusIndicator } from "@/modules/workbench-shell/ui/thread-status-indicator";
import { WorkbenchPromptComposer } from "@/modules/workbench-shell/ui/workbench-prompt-composer";
import { WorkbenchTopBar } from "@/modules/workbench-shell/ui/workbench-top-bar";
import { useSystemMetadata } from "@/features/system-info/model/use-system-metadata";
import { cn } from "@/shared/lib/utils";
import { getInvokeErrorMessage } from "@/shared/lib/invoke-error";
import { WorkbenchSegmentedControl } from "@/shared/ui/workbench-segmented-control";
import { terminalStore } from "@/features/terminal/model/terminal-store";

const NEW_THREAD_TERMINAL_KEY_SUFFIX = "__new_thread__";
const UNBOUND_NEW_THREAD_TERMINAL_STATE_KEY = "__new_thread_pending__";
const DEFAULT_TERMINAL_COLLAPSED = true;
const WORKSPACE_THREAD_PAGE_SIZE = 10;
const SIDEBAR_AUTO_REFRESH_INTERVAL_MS = 2_000;
const SIDEBAR_AUTO_REFRESH_GRACE_MS = 20_000;

function buildInitialWorkspaceThreadDisplayCounts() {
  return Object.fromEntries(
    WORKSPACE_ITEMS.map((workspace) => [
      workspace.id,
      Math.min(WORKSPACE_THREAD_PAGE_SIZE, workspace.threads.length),
    ]),
  );
}

function buildInitialWorkspaceThreadHasMore() {
  return Object.fromEntries(
    WORKSPACE_ITEMS.map((workspace) => [
      workspace.id,
      workspace.threads.length > WORKSPACE_THREAD_PAGE_SIZE,
    ]),
  );
}

function getNewThreadTerminalBindingKey(workspaceId: string) {
  return `${workspaceId}:${NEW_THREAD_TERMINAL_KEY_SUFFIX}`;
}

function buildWorkspaceBindings(workspaceEntries: ReadonlyArray<WorkspaceDto>) {
  return workspaceEntries.reduce<Record<string, string>>(
    (bindings, workspace) => {
      bindings[workspace.path] = workspace.id;
      bindings[workspace.canonicalPath] = workspace.id;
      return bindings;
    },
    {},
  );
}

function buildProjectOptionFromWorkspace(workspace: WorkspaceDto) {
  const project = buildProjectOptionFromPath(
    workspace.canonicalPath || workspace.path,
  );
  if (!project) {
    return null;
  }

  return {
    ...project,
    id: workspace.id,
    name: workspace.name,
  };
}

function findWorkspaceForThread(
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

function resolveProjectForWorkspace(
  workspace: WorkspaceItem | null,
  recentProjects: ReadonlyArray<ProjectOption>,
) {
  if (!workspace) {
    return null;
  }

  const matchedProject = recentProjects.find(
    (project) =>
      (workspace.path && project.path === workspace.path) ||
      project.id === workspace.id ||
      project.name === workspace.name,
  );

  if (matchedProject) {
    return matchedProject;
  }

  if (!workspace.path) {
    return null;
  }

  return buildProjectOptionFromPath(workspace.path);
}

function mergeLocalFallbackThreads(options: {
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

function mapRunStateToWorkbenchThreadStatus(
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

function mapRunFinishedStatusToThreadStatus(
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

function formatCompactTokenCount(value: number) {
  return new Intl.NumberFormat("en", {
    maximumFractionDigits: 1,
    notation: "compact",
  }).format(value);
}

function buildThreadContextBadgeData(options: {
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

type PendingThreadRun = {
  id: string;
  displayText: string;
  effectivePrompt: string;
  attachments: MessageAttachmentDto[];
  metadata: Record<string, unknown> | null;
  runMode: RunMode;
  threadId: string;
};

export function DashboardWorkbench() {
  const { data } = useSystemMetadata();
  const { theme, setTheme } = useTheme();
  const { language, setLanguage } = useLanguage();
  const {
    itemStates: marketplaceItemStates,
    installItem,
    uninstallItem,
    enableItem,
    disableItem,
  } = useMarketplaceController();
  const {
    general: generalPreferences,
    workspaces: settingsWorkspaces,
    providerCatalog,
    providers,
    commands,
    policy,
    updateGeneralPreference,
    addWorkspace,
    removeWorkspace,
    setDefaultWorkspace,
    addProvider,
    removeProvider,
    updateProvider,
    fetchProviderModels,
    testProviderModelConnection,
    agentProfiles,
    activeAgentProfileId,
    addAgentProfile,
    removeAgentProfile,
    updateAgentProfile,
    setActiveAgentProfile,
    duplicateAgentProfile,
    updatePolicySetting,
    addAllowEntry,
    removeAllowEntry,
    updateAllowEntry,
    addDenyEntry,
    removeDenyEntry,
    updateDenyEntry,
    addWritableRoot,
    removeWritableRoot,
    updateWritableRoot,
    addCommand,
    removeCommand,
    updateCommand,
  } = useSettingsController();
  const [workspaces, setWorkspaces] = useState<Array<WorkspaceItem>>(() =>
    isTauri() ? [] : buildInitialWorkspaces(),
  );
  const [recentProjects, setRecentProjects] = useState<Array<ProjectOption>>(
    () => (isTauri() ? [] : [...RECENT_PROJECTS]),
  );
  const [selectedProject, setSelectedProject] = useState<ProjectOption | null>(
    () => (isTauri() ? null : (RECENT_PROJECTS[0] ?? null)),
  );
  const [isNewThreadMode, setNewThreadMode] = useState(true);
  const [activeOverlay, setActiveOverlay] = useState<WorkbenchOverlay>(null);
  const [activeSettingsCategory, setActiveSettingsCategory] =
    useState<SettingsCategory>("account");
  const [panelVisibilityState, setPanelVisibilityState] =
    useState<PanelVisibilityState>(() => readPanelVisibilityState());
  const [terminalCollapsedByThreadKey, setTerminalCollapsedByThreadKey] =
    useState<Record<string, boolean>>({});
  const [terminalHeight, setTerminalHeight] = useState(DEFAULT_TERMINAL_HEIGHT);
  const [terminalResize, setTerminalResize] = useState<{
    startY: number;
    startHeight: number;
  } | null>(null);
  const [terminalThreadBindings, setTerminalThreadBindings] = useState<
    Record<string, string>
  >({});
  const [composerValue, setComposerValue] = useState("");
  const [composerError, setComposerError] = useState<string | null>(null);
  const [openSettingsSection, setOpenSettingsSection] = useState<
    "theme" | "language" | null
  >(null);
  const [isUserMenuOpen, setUserMenuOpen] = useState(false);
  const [userSession, setUserSession] = useState(() => readStoredUserSession());
  const [isCheckingUpdates, setCheckingUpdates] = useState(false);
  const [updateStatus, setUpdateStatus] = useState<string | null>(null);
  const [terminalBootstrapError, setTerminalBootstrapError] = useState<
    string | null
  >(null);
  const [pendingDeleteThreadId, setPendingDeleteThreadId] = useState<
    string | null
  >(null);
  const [deletingThreadId, setDeletingThreadId] = useState<string | null>(null);
  const [isAddingWorkspace, setAddingWorkspace] = useState(false);
  const [activeWorkspaceMenuId, setActiveWorkspaceMenuId] = useState<
    string | null
  >(null);
  const [workspaceAction, setWorkspaceAction] = useState<{
    workspaceId: string;
    kind: "open" | "remove";
  } | null>(null);
  const [pendingThreadRuns, setPendingThreadRuns] = useState<
    Record<string, PendingThreadRun>
  >({});
  const [newThreadRunMode, setNewThreadRunMode] = useState<RunMode>("default");
  const [runtimeContextUsage, setRuntimeContextUsage] =
    useState<ThreadContextUsage | null>(null);
  const [terminalWorkspaceBindings, setTerminalWorkspaceBindings] = useState<
    Record<string, string>
  >({});
  const [defaultWorkspaceId, setDefaultWorkspaceId] = useState<string | null>(
    null,
  );
  const [workspaceThreadDisplayCounts, setWorkspaceThreadDisplayCounts] =
    useState<Record<string, number>>(() =>
      isTauri() ? {} : buildInitialWorkspaceThreadDisplayCounts(),
    );
  const [workspaceThreadHasMore, setWorkspaceThreadHasMore] = useState<
    Record<string, boolean>
  >(() => (isTauri() ? {} : buildInitialWorkspaceThreadHasMore()));
  const [workspaceThreadLoadMorePending, setWorkspaceThreadLoadMorePending] =
    useState<Record<string, boolean>>({});
  const [openWorkspaces, setOpenWorkspaces] = useState<Record<string, boolean>>(
    () =>
      isTauri()
        ? {}
        : Object.fromEntries(
            WORKSPACE_ITEMS.map((workspace) => [
              workspace.id,
              workspace.defaultOpen,
            ]),
          ),
  );
  const [activeDrawerPanel, setActiveDrawerPanel] =
    useState<DrawerPanel>("project");
  const [selectedDiffSelection, setSelectedDiffSelection] =
    useState<GitDiffSelection | null>(null);
  const mainContentRef = useRef<HTMLElement | null>(null);
  const overlayContentRef = useRef<HTMLDivElement | null>(null);
  const userMenuRef = useRef<HTMLDivElement | null>(null);
  const workspaceMenuRef = useRef<HTMLDivElement | null>(null);
  const syncVersionRef = useRef(0);
  const sidebarAutoRefreshUntilRef = useRef(0);
  const sidebarSyncInFlightRef = useRef(false);
  const workspaceThreadDisplayCountsRef = useRef<Record<string, number>>(
    isTauri() ? {} : buildInitialWorkspaceThreadDisplayCounts(),
  );
  const newThreadCreationRef = useRef<Record<string, Promise<string>>>({});

  const activeThread = getActiveThread(workspaces);
  const selectedProjectWorkspaceId =
    selectedProject === null
      ? null
      : (terminalWorkspaceBindings[selectedProject.path] ?? null);
  const activeThreadWorkspace = findWorkspaceForThread(
    workspaces,
    activeThread?.id ?? null,
  );
  const activeThreadProject = resolveProjectForWorkspace(
    activeThreadWorkspace,
    recentProjects,
  );
  const currentProject = isNewThreadMode
    ? selectedProject
    : activeThreadProject;
  const resolvedWorkspaceId =
    currentProject === null
      ? null
      : (terminalWorkspaceBindings[currentProject.path] ?? null);
  const newThreadTerminalBindingKey =
    selectedProjectWorkspaceId === null
      ? null
      : getNewThreadTerminalBindingKey(selectedProjectWorkspaceId);
  const resolvedTerminalThreadId = isNewThreadMode
    ? newThreadTerminalBindingKey === null
      ? null
      : (terminalThreadBindings[newThreadTerminalBindingKey] ?? null)
    : (activeThread?.id ?? null);
  const { isSidebarOpen, isDrawerOpen } = panelVisibilityState;
  const activeTerminalStateKey = isNewThreadMode
    ? (newThreadTerminalBindingKey ?? UNBOUND_NEW_THREAD_TERMINAL_STATE_KEY)
    : (activeThread?.id ?? null);
  const isTerminalCollapsed =
    activeTerminalStateKey === null
      ? DEFAULT_TERMINAL_COLLAPSED
      : (terminalCollapsedByThreadKey[activeTerminalStateKey] ??
        DEFAULT_TERMINAL_COLLAPSED);
  const isSettingsOpen = activeOverlay === "settings";
  const isMarketplaceOpen = activeOverlay === "marketplace";
  const isOverlayOpen = activeOverlay !== null;
  const isMacOS =
    data?.platform === "macos" ||
    (typeof navigator !== "undefined" && navigator.userAgent.includes("Mac"));
  const isWindows =
    data?.platform === "windows" ||
    (typeof navigator !== "undefined" &&
      navigator.userAgent.includes("Windows"));
  const selectedRunModelPlan = useMemo(
    () =>
      buildRunModelPlanFromSelection(
        activeAgentProfileId,
        agentProfiles,
        providers,
      ),
    [activeAgentProfileId, agentProfiles, providers],
  );
  const activeAgentProfile = useMemo(
    () =>
      agentProfiles.find((profile) => profile.id === activeAgentProfileId) ??
      agentProfiles[0] ??
      null,
    [activeAgentProfileId, agentProfiles],
  );
  const commitMessageModelPlan = useMemo(
    () =>
      activeAgentProfile
        ? buildProfileModelPlan(activeAgentProfile, providers)
        : null,
    [activeAgentProfile, providers],
  );
  const contextBadge = useMemo(
    () =>
      buildThreadContextBadgeData({
        fallbackContextWindow:
          selectedRunModelPlan?.primary?.contextWindow ?? null,
        fallbackModelDisplayName:
          selectedRunModelPlan?.primary?.modelDisplayName ??
          selectedRunModelPlan?.primary?.modelId ??
          null,
        runtimeUsage: runtimeContextUsage,
      }),
    [runtimeContextUsage, selectedRunModelPlan],
  );
  const hasSidebarLiveThreads = useMemo(
    () =>
      workspaces.some((workspace) =>
        workspace.threads.some(
          (thread) =>
            thread.status === "running" || thread.status === "needs-reply",
        ),
      ),
    [workspaces],
  );

  useEffect(() => {
    workspaceThreadDisplayCountsRef.current = workspaceThreadDisplayCounts;
  }, [workspaceThreadDisplayCounts]);

  const setSidebarOpen = (
    nextState: boolean | ((current: boolean) => boolean),
  ) => {
    setPanelVisibilityState((current) => ({
      ...current,
      isSidebarOpen:
        typeof nextState === "function"
          ? nextState(current.isSidebarOpen)
          : nextState,
    }));
  };

  const setDrawerOpen = (
    nextState: boolean | ((current: boolean) => boolean),
  ) => {
    setPanelVisibilityState((current) => ({
      ...current,
      isDrawerOpen:
        typeof nextState === "function"
          ? nextState(current.isDrawerOpen)
          : nextState,
    }));
  };

  const setTerminalCollapsed = (
    nextState: boolean | ((current: boolean) => boolean),
  ) => {
    setTerminalCollapsedByThreadKey((current) => {
      if (activeTerminalStateKey === null) {
        return current;
      }

      const resolvedNextState =
        typeof nextState === "function"
          ? nextState(
              current[activeTerminalStateKey] ?? DEFAULT_TERMINAL_COLLAPSED,
            )
          : nextState;

      if (resolvedNextState === DEFAULT_TERMINAL_COLLAPSED) {
        if (!(activeTerminalStateKey in current)) {
          return current;
        }

        const next = { ...current };
        delete next[activeTerminalStateKey];
        return next;
      }

      return {
        ...current,
        [activeTerminalStateKey]: resolvedNextState,
      };
    });
  };

  useEffect(() => {
    if (isNewThreadMode || !resolvedTerminalThreadId) {
      setRuntimeContextUsage(null);
    }
  }, [isNewThreadMode, resolvedTerminalThreadId]);

  const getMaxTerminalHeight = () => {
    if (typeof window === "undefined") {
      return DEFAULT_TERMINAL_HEIGHT;
    }

    return Math.max(
      MIN_TERMINAL_HEIGHT,
      window.innerHeight - TOPBAR_HEIGHT - MIN_WORKBENCH_HEIGHT,
    );
  };

  const listVisibleWorkspaceThreads = useCallback(
    async (
      workspaceId: string,
      visibleLimit: number,
    ): Promise<{ hasMore: boolean; threads: Array<ThreadSummaryDto> }> => {
      const desiredVisibleCount = visibleLimit + 1;
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
          visibleThreads.push(thread);

          if (visibleThreads.length >= desiredVisibleCount) {
            break;
          }
        }

        if (batch.length === 0) {
          break;
        }
      }

      return {
        hasMore: visibleThreads.length > visibleLimit,
        threads: visibleThreads.slice(0, visibleLimit),
      };
    },
    [],
  );

  const clearNewThreadBindingForWorkspace = useCallback((workspaceId: string) => {
    const bindingKey = getNewThreadTerminalBindingKey(workspaceId);
    newThreadCreationRef.current = Object.fromEntries(
      Object.entries(newThreadCreationRef.current).filter(
        ([candidateWorkspaceId]) => candidateWorkspaceId !== workspaceId,
      ),
    );
    setTerminalThreadBindings((current) => {
      if (!(bindingKey in current)) {
        return current;
      }

      const next = { ...current };
      delete next[bindingKey];
      return next;
    });
  }, []);

  const getOrCreateNewThreadId = useCallback(
    async (workspaceId: string) => {
      const existingBinding =
        terminalThreadBindings[getNewThreadTerminalBindingKey(workspaceId)] ?? null;
      if (existingBinding) {
        return existingBinding;
      }

      const inFlight = newThreadCreationRef.current[workspaceId];
      if (inFlight) {
        return inFlight;
      }

      const creationPromise = threadCreate(workspaceId, "")
        .then((thread) => {
          setTerminalThreadBindings((current) => {
            const bindingKey = getNewThreadTerminalBindingKey(workspaceId);
            if (current[bindingKey] === thread.id) {
              return current;
            }

            return {
              ...current,
              [bindingKey]: thread.id,
            };
          });
          return thread.id;
        })
        .finally(() => {
          newThreadCreationRef.current = Object.fromEntries(
            Object.entries(newThreadCreationRef.current).filter(
              ([candidateWorkspaceId]) => candidateWorkspaceId !== workspaceId,
            ),
          );
        });

      newThreadCreationRef.current = {
        ...newThreadCreationRef.current,
        [workspaceId]: creationPromise,
      };

      return creationPromise;
    },
    [terminalThreadBindings],
  );

  const syncWorkspaceSidebar = useCallback(
    async ({
      preserveSelectedProjectIfMissing = true,
      threadDisplayCountOverrides = {},
    }: {
      preserveSelectedProjectIfMissing?: boolean;
      threadDisplayCountOverrides?: Record<string, number>;
    } = {}) => {
      const version = ++syncVersionRef.current;

      const workspaceEntries = await workspaceList();
      const nextDisplayCounts = Object.fromEntries(
        workspaceEntries.map((workspace) => [
          workspace.id,
          threadDisplayCountOverrides[workspace.id] ??
            workspaceThreadDisplayCountsRef.current[workspace.id] ??
            WORKSPACE_THREAD_PAGE_SIZE,
        ]),
      );
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

      // Discard stale sync results — a newer sync has been initiated while we were fetching.
      if (syncVersionRef.current !== version) {
        return;
      }

      const threadsByWorkspaceId = Object.fromEntries(
        threadEntries.map(([workspaceId, result]) => [
          workspaceId,
          result.threads,
        ]),
      );
      const nextHasMoreByWorkspaceId = Object.fromEntries(
        threadEntries.map(([workspaceId, result]) => [
          workspaceId,
          result.hasMore,
        ]),
      );
      const nextProjects = workspaceEntries
        .map((workspace) => buildProjectOptionFromWorkspace(workspace))
        .filter((project): project is ProjectOption => project !== null);
      const nextBindings = buildWorkspaceBindings(workspaceEntries);
      const defaultWorkspace =
        workspaceEntries.find((workspace) => workspace.isDefault) ?? null;
      const defaultProject =
        defaultWorkspace === null
          ? null
          : (nextProjects.find(
              (project) =>
                project.id === defaultWorkspace.id ||
                project.path === defaultWorkspace.canonicalPath ||
                project.path === defaultWorkspace.path,
            ) ?? null);

      setTerminalWorkspaceBindings(nextBindings);
      setRecentProjects(nextProjects);
      setDefaultWorkspaceId(defaultWorkspace?.id ?? null);
      setWorkspaceThreadDisplayCounts(nextDisplayCounts);
      setWorkspaceThreadHasMore(nextHasMoreByWorkspaceId);
      setWorkspaceThreadLoadMorePending((current) =>
        Object.fromEntries(
          workspaceEntries.map((workspace) => [
            workspace.id,
            current[workspace.id] ?? false,
          ]),
        ),
      );
      setSelectedProject((current) => {
        if (current) {
          const matchingProject =
            nextProjects.find(
              (project) =>
                project.id === current.id || project.path === current.path,
            ) ?? null;

          if (matchingProject) {
            return matchingProject;
          }

          if (preserveSelectedProjectIfMissing) {
            return current;
          }

          return defaultProject ?? nextProjects[0] ?? null;
        }

        return defaultProject ?? nextProjects[0] ?? null;
      });
      setWorkspaces((current) => {
        const activeThreadId = getActiveThread(current)?.id ?? null;
        const syncedWorkspaces = buildWorkspaceItemsFromDtos(
          workspaceEntries,
          threadsByWorkspaceId,
          activeThreadId,
        );
        const mergedWithFallbacks = mergeLocalFallbackThreads({
          currentWorkspaces: current,
          syncedWorkspaces,
        });

        return mergedWithFallbacks.map((workspace) => {
          const currentWorkspace =
            current.find((candidate) => candidate.id === workspace.id) ?? null;

          if (!currentWorkspace) {
            return workspace;
          }

          return {
            ...workspace,
            threads: workspace.threads.map((thread) => {
              const currentThread = currentWorkspace.threads.find(
                (candidate) => candidate.id === thread.id,
              );

              if (!currentThread) {
                return thread;
              }

              const currentTitle = currentThread.name.trim();
              const syncedTitle = thread.name.trim();
              if (
                currentTitle
                && currentTitle !== "新对话"
                && syncedTitle === "新对话"
              ) {
                return {
                  ...thread,
                  name: currentThread.name,
                };
              }

              return thread;
            }),
          };
        });
      });
      setOpenWorkspaces((current) =>
        Object.fromEntries(
          workspaceEntries.map((workspace) => [
            workspace.id,
            current[workspace.id] ??
              (workspace.isDefault || workspaceEntries.length === 1),
          ]),
        ),
      );
    },
    [listVisibleWorkspaceThreads],
  );

  // ---------------------------------------------------------------------------
  // Global Tauri event listeners — react to background thread lifecycle changes
  // without needing a per-thread stream subscription.
  // ---------------------------------------------------------------------------
  useEffect(() => {
    if (!isTauri()) {
      return;
    }

    const unlistenPromises: Array<Promise<UnlistenFn>> = [];

    unlistenPromises.push(
      listen<{ threadId: string; runId: string }>(
        "thread-run-started",
        (event) => {
          const { threadId } = event.payload;

          setWorkspaces((current) =>
            current.map((workspace) => ({
              ...workspace,
              threads: workspace.threads.map((thread) =>
                thread.id === threadId
                  ? { ...thread, status: "running" as const }
                  : thread,
              ),
            })),
          );

          // Extend the sidebar auto-refresh grace period so the polling
          // keeps running while background threads are active.
          sidebarAutoRefreshUntilRef.current =
            Date.now() + SIDEBAR_AUTO_REFRESH_GRACE_MS;
        },
      ),
    );

    unlistenPromises.push(
      listen<{ threadId: string; runId: string; status: string }>(
        "thread-run-finished",
        (event) => {
          const { threadId, status } = event.payload;
          const threadStatus = mapRunFinishedStatusToThreadStatus(status);

          setWorkspaces((current) =>
            current.map((workspace) => ({
              ...workspace,
              threads: workspace.threads.map((thread) =>
                thread.id === threadId
                  ? { ...thread, status: threadStatus }
                  : thread,
              ),
            })),
          );

          // Perform a full sidebar sync to reconcile any missed state
          // (e.g. title generated shortly after, ordering changes).
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

          if (!trimmedTitle) {
            return;
          }

          setWorkspaces((current) =>
            current.map((workspace) => ({
              ...workspace,
              threads: workspace.threads.map((thread) =>
                thread.id === threadId
                  ? { ...thread, name: trimmedTitle }
                  : thread,
              ),
            })),
          );
        },
      ),
    );

    return () => {
      for (const promise of unlistenPromises) {
        void promise.then((unlisten) => unlisten());
      }
    };
  }, [syncWorkspaceSidebar]);

  useEffect(() => {
    if (!isTauri() || typeof window === "undefined") {
      return;
    }

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
      const withinGrace = Date.now() < sidebarAutoRefreshUntilRef.current;
      if (!hasSidebarLiveThreads && !withinGrace) {
        window.clearInterval(interval);
        return;
      }

      if (sidebarSyncInFlightRef.current) {
        return;
      }

      sidebarSyncInFlightRef.current = true;
      void syncWorkspaceSidebar().catch((error) => {
        const message = getInvokeErrorMessage(error, "刷新线程列表失败");
        setTerminalBootstrapError(message);
      }).finally(() => {
        sidebarSyncInFlightRef.current = false;
      });
    }, SIDEBAR_AUTO_REFRESH_INTERVAL_MS);

    return () => window.clearInterval(interval);
  }, [hasSidebarLiveThreads, syncWorkspaceSidebar]);

  useEffect(() => {
    if (typeof window === "undefined") {
      return;
    }

    const syncTerminalHeight = () => {
      setTerminalHeight((current) => Math.min(current, getMaxTerminalHeight()));
    };

    syncTerminalHeight();
    window.addEventListener("resize", syncTerminalHeight);

    return () => window.removeEventListener("resize", syncTerminalHeight);
  }, []);

  useEffect(() => {
    if (!isTauri()) {
      return;
    }

    let cancelled = false;

    void syncWorkspaceSidebar()
      .then(() => {
        if (cancelled) {
          return;
        }
      })
      .catch((error) => {
        if (cancelled) {
          return;
        }

        const message = getInvokeErrorMessage(error, "工作区初始化失败");
        setTerminalBootstrapError(message);
      });

    return () => {
      cancelled = true;
    };
  }, [syncWorkspaceSidebar]);

  useEffect(() => {
    if (!terminalResize || typeof window === "undefined") {
      return;
    }

    const handleMouseMove = (event: MouseEvent) => {
      const deltaY = terminalResize.startY - event.clientY;
      const nextHeight = terminalResize.startHeight + deltaY;
      const clampedHeight = Math.min(
        getMaxTerminalHeight(),
        Math.max(MIN_TERMINAL_HEIGHT, nextHeight),
      );

      setTerminalHeight(clampedHeight);
    };

    const handleMouseUp = () => {
      setTerminalResize(null);
    };

    const originalCursor = document.body.style.cursor;
    const originalUserSelect = document.body.style.userSelect;

    document.body.style.cursor = "row-resize";
    document.body.style.userSelect = "none";

    window.addEventListener("mousemove", handleMouseMove);
    window.addEventListener("mouseup", handleMouseUp);

    return () => {
      document.body.style.cursor = originalCursor;
      document.body.style.userSelect = originalUserSelect;
      window.removeEventListener("mousemove", handleMouseMove);
      window.removeEventListener("mouseup", handleMouseUp);
    };
  }, [terminalResize]);

  useEffect(() => {
    if (!isTauri() || !currentProject) {
      return;
    }

    if (terminalWorkspaceBindings[currentProject.path]) {
      return;
    }

    let cancelled = false;
    setTerminalBootstrapError(null);

    void workspaceList()
      .then(async (workspaces) => {
        const existing = workspaces.find(
          (workspace) =>
            workspace.path === currentProject.path ||
            workspace.canonicalPath === currentProject.path,
        );
        if (existing) {
          return existing;
        }

        return workspaceAdd(currentProject.path, currentProject.name);
      })
      .then((workspace) => {
        if (cancelled) {
          return;
        }

        setTerminalWorkspaceBindings((current) => {
          if (
            current[currentProject.path] &&
            current[workspace.canonicalPath]
          ) {
            return current;
          }

          return {
            ...current,
            [currentProject.path]: workspace.id,
            [workspace.path]: workspace.id,
            [workspace.canonicalPath]: workspace.id,
          };
        });
        void syncWorkspaceSidebar().catch((refreshError) => {
          if (cancelled) {
            return;
          }

          const message = getInvokeErrorMessage(
            refreshError,
            "刷新工作区列表失败",
          );
          setTerminalBootstrapError(message);
        });
      })
      .catch((error) => {
        if (cancelled) {
          return;
        }

        const message = getInvokeErrorMessage(error, "添加工作区失败");
        setTerminalBootstrapError(message);
      });

    return () => {
      cancelled = true;
    };
  }, [currentProject, syncWorkspaceSidebar, terminalWorkspaceBindings]);

  useEffect(() => {
    if (
      !isTauri() ||
      !selectedProject ||
      !selectedProjectWorkspaceId ||
      selectedProjectWorkspaceId === defaultWorkspaceId
    ) {
      return;
    }

    let cancelled = false;

    void workspaceSetDefault(selectedProjectWorkspaceId)
      .then(() => {
        if (cancelled) {
          return;
        }

        setDefaultWorkspaceId(selectedProjectWorkspaceId);
      })
      .catch((error) => {
        if (cancelled) {
          return;
        }

        const message = getInvokeErrorMessage(error, "更新默认工作区失败");
        setTerminalBootstrapError(message);
      });

    return () => {
      cancelled = true;
    };
  }, [defaultWorkspaceId, selectedProject, selectedProjectWorkspaceId]);

  const handleTerminalResizeStart = (
    event: ReactMouseEvent<HTMLDivElement>,
  ) => {
    if (event.button !== 0) {
      return;
    }

    event.preventDefault();
    setTerminalResize({ startY: event.clientY, startHeight: terminalHeight });
  };

  useEffect(() => {
    if (!isUserMenuOpen || typeof window === "undefined") {
      return;
    }

    const handlePointerDown = (event: MouseEvent) => {
      const target = event.target as Node | null;

      if (target && userMenuRef.current?.contains(target)) {
        return;
      }

      setUserMenuOpen(false);
      setOpenSettingsSection(null);
    };

    window.addEventListener("mousedown", handlePointerDown);
    return () => window.removeEventListener("mousedown", handlePointerDown);
  }, [isUserMenuOpen]);

  useEffect(() => {
    if (!activeWorkspaceMenuId || typeof window === "undefined") {
      return;
    }

    const handlePointerDown = (event: MouseEvent) => {
      const target = event.target as Node | null;

      if (target && workspaceMenuRef.current?.contains(target)) {
        return;
      }

      setActiveWorkspaceMenuId(null);
    };

    window.addEventListener("mousedown", handlePointerDown);
    return () => window.removeEventListener("mousedown", handlePointerDown);
  }, [activeWorkspaceMenuId]);

  useEffect(() => {
    if (typeof window === "undefined") {
      return;
    }

    if (userSession) {
      window.localStorage.setItem(
        "tiy-agent-auth-session",
        JSON.stringify(userSession),
      );
      return;
    }

    window.localStorage.removeItem("tiy-agent-auth-session");
  }, [userSession]);

  useEffect(() => {
    if (typeof window === "undefined") {
      return;
    }

    window.localStorage.setItem(
      PANEL_VISIBILITY_STORAGE_KEY,
      JSON.stringify(panelVisibilityState),
    );
  }, [panelVisibilityState]);

  useEffect(() => {
    if (!updateStatus || typeof window === "undefined") {
      return;
    }

    const timeout = window.setTimeout(() => {
      setUpdateStatus(null);
    }, UPDATE_STATUS_DURATION);

    return () => window.clearTimeout(timeout);
  }, [updateStatus]);

  useEffect(() => {
    if (!selectedDiffSelection || typeof window === "undefined") {
      return;
    }

    const handleKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        setSelectedDiffSelection(null);
      }
    };

    window.addEventListener("keydown", handleKeyDown);
    return () => window.removeEventListener("keydown", handleKeyDown);
  }, [selectedDiffSelection]);

  useEffect(() => {
    if (!activeOverlay || typeof window === "undefined") {
      return;
    }

    const handleKeyDown = (event: KeyboardEvent) => {
      if (event.defaultPrevented) {
        return;
      }

      if (event.key === "Escape") {
        setActiveOverlay(null);
      }
    };

    window.addEventListener("keydown", handleKeyDown);
    return () => window.removeEventListener("keydown", handleKeyDown);
  }, [activeOverlay]);

  useEffect(() => {
    if (!isOverlayOpen) {
      return;
    }

    setActiveWorkspaceMenuId(null);
  }, [isOverlayOpen]);

  useEffect(() => {
    if (typeof window === "undefined") {
      return;
    }

    const handleKeyDown = (event: KeyboardEvent) => {
      if (
        !(event.metaKey || event.ctrlKey) ||
        event.altKey ||
        event.key.toLowerCase() !== "a"
      ) {
        return;
      }

      if (isEditableSelectionTarget(event.target)) {
        return;
      }

      const selection = window.getSelection();
      const selectionInsideOverlayContent =
        isNodeInsideContainer(
          overlayContentRef.current,
          selection?.anchorNode ?? null,
        ) ||
        isNodeInsideContainer(
          overlayContentRef.current,
          selection?.focusNode ?? null,
        );
      const targetInsideOverlayContent = isNodeInsideContainer(
        overlayContentRef.current,
        event.target instanceof Node ? event.target : null,
      );
      const selectionInsideMainContent =
        isNodeInsideContainer(
          mainContentRef.current,
          selection?.anchorNode ?? null,
        ) ||
        isNodeInsideContainer(
          mainContentRef.current,
          selection?.focusNode ?? null,
        );
      const targetInsideMainContent = isNodeInsideContainer(
        mainContentRef.current,
        event.target instanceof Node ? event.target : null,
      );

      if (
        overlayContentRef.current &&
        (targetInsideOverlayContent || selectionInsideOverlayContent)
      ) {
        event.preventDefault();
        selectContainerContents(overlayContentRef.current);
        return;
      }

      if (
        mainContentRef.current &&
        (targetInsideMainContent || selectionInsideMainContent)
      ) {
        event.preventDefault();
        selectContainerContents(mainContentRef.current);
        return;
      }

      event.preventDefault();
      selection?.removeAllRanges();
    };

    window.addEventListener("keydown", handleKeyDown);
    return () => window.removeEventListener("keydown", handleKeyDown);
  }, []);

  const handleWorkspaceToggle = (workspaceId: string) => {
    setOpenWorkspaces((current) => ({
      ...current,
      [workspaceId]: !current[workspaceId],
    }));
  };

  const handleWorkspaceShowMore = useCallback(
    (workspaceId: string) => {
      const nextDisplayCount =
        (workspaceThreadDisplayCountsRef.current[workspaceId] ??
          WORKSPACE_THREAD_PAGE_SIZE) + WORKSPACE_THREAD_PAGE_SIZE;

      setWorkspaceThreadDisplayCounts((current) => ({
        ...current,
        [workspaceId]: nextDisplayCount,
      }));
      setWorkspaceThreadLoadMorePending((current) => ({
        ...current,
        [workspaceId]: true,
      }));
      setTerminalBootstrapError(null);

      void syncWorkspaceSidebar({
        threadDisplayCountOverrides: {
          [workspaceId]: nextDisplayCount,
        },
      })
        .catch((error) => {
          const message = getInvokeErrorMessage(error, "加载更多线程失败");
          setTerminalBootstrapError(message);
        })
        .finally(() => {
          setWorkspaceThreadLoadMorePending((current) => ({
            ...current,
            [workspaceId]: false,
          }));
        });
    },
    [syncWorkspaceSidebar],
  );

  const handleEnterNewThreadMode = () => {
    if (selectedProjectWorkspaceId) {
      clearNewThreadBindingForWorkspace(selectedProjectWorkspaceId);
    }

    setNewThreadMode(true);
    setWorkspaces((current) => clearActiveThreads(current));
    setComposerError(null);
    setPendingDeleteThreadId(null);
    setTerminalBootstrapError(null);
  };

  const handleThreadSelect = (threadId: string) => {
    setNewThreadMode(false);
    setActiveWorkspaceMenuId(null);
    setPendingDeleteThreadId(null);
    setWorkspaces((current) => activateThread(current, threadId));
  };

  const handleProjectSelect = (project: ProjectOption) => {
    const nextProject = {
      ...project,
      lastOpenedLabel: "刚刚",
    };

    setSelectedProject(nextProject);
    setRecentProjects((current) => mergeRecentProjects(current, nextProject));
  };

  const handleNewThreadForWorkspace = (workspace: WorkspaceItem) => {
    if (!workspace.path) {
      return;
    }

    clearNewThreadBindingForWorkspace(workspace.id);

    const projectFromPath = buildProjectOptionFromPath(workspace.path);
    const nextProject = {
      ...(projectFromPath ?? {
        id: workspace.id,
        name: workspace.name,
        path: workspace.path,
        lastOpenedLabel: "刚刚",
      }),
      id: workspace.id,
      name: workspace.name,
      path: workspace.path,
      lastOpenedLabel: "刚刚",
    };

    setSelectedProject(nextProject);
    setRecentProjects((current) => mergeRecentProjects(current, nextProject));
    setOpenWorkspaces((current) => ({
      ...current,
      [workspace.id]: true,
    }));
    setNewThreadMode(true);
    setWorkspaces((current) => clearActiveThreads(current));
    setComposerError(null);
    setPendingDeleteThreadId(null);
    setTerminalBootstrapError(null);
    setActiveWorkspaceMenuId(null);
  };

  const updateActiveThreadStatus = useCallback(
    (status: WorkbenchThreadStatus) => {
      if (!activeThread?.id) {
        return;
      }

      setWorkspaces((current) =>
        current.map((workspace) => ({
          ...workspace,
          threads: workspace.threads.map((thread) =>
            thread.id === activeThread.id
              ? {
                  ...thread,
                  status,
                }
              : thread,
          ),
        })),
      );
    },
    [activeThread?.id],
  );

  const handleRuntimeThreadRunStateChange = useCallback(
    (state: RunState) => {
      updateActiveThreadStatus(mapRunStateToWorkbenchThreadStatus(state));
    },
    [updateActiveThreadStatus],
  );

  const handleRuntimeThreadTitleChange = useCallback(
    (threadId: string, title: string) => {
      const trimmedTitle = title.trim();
      if (!trimmedTitle) {
        return;
      }

      setWorkspaces((current) =>
        current.map((workspace) => ({
          ...workspace,
          threads: workspace.threads.map((thread) =>
            thread.id === threadId
              ? {
                  ...thread,
                  name: trimmedTitle,
                }
              : thread,
          ),
        })),
      );

      void syncWorkspaceSidebar().catch((error) => {
        const message = getInvokeErrorMessage(error, "刷新线程列表失败");
        setTerminalBootstrapError(message);
      });
    },
    [syncWorkspaceSidebar],
  );

  const handleThreadDeleteRequest = useCallback((threadId: string) => {
    setPendingDeleteThreadId(threadId);
    setTerminalBootstrapError(null);
  }, []);

  const handleThreadDeleteConfirm = useCallback(
    (threadId: string) => {
      if (deletingThreadId) {
        return;
      }

      void (async () => {
        setDeletingThreadId(threadId);
        setTerminalBootstrapError(null);

        try {
          if (isTauri()) {
            await threadDelete(threadId);
          }

          const isDeletingActiveThread = activeThread?.id === threadId;
          terminalStore.removeSession(threadId);

          setWorkspaces((current) => {
            const next = current.map((workspace) => ({
              ...workspace,
              threads: workspace.threads.filter(
                (thread) => thread.id !== threadId,
              ),
            }));

            return isDeletingActiveThread ? clearActiveThreads(next) : next;
          });
          setPendingThreadRuns((current) => {
            if (!(threadId in current)) {
              return current;
            }

            const next = { ...current };
            delete next[threadId];
            return next;
          });
          setTerminalCollapsedByThreadKey((current) => {
            if (!(threadId in current)) {
              return current;
            }

            const next = { ...current };
            delete next[threadId];
            return next;
          });
          setTerminalThreadBindings((current) => {
            const next = Object.fromEntries(
              Object.entries(current).filter(
                ([, boundThreadId]) => boundThreadId !== threadId,
              ),
            );
            return next;
          });

          if (isDeletingActiveThread) {
            setSelectedProject((current) => activeThreadProject ?? current);
            setNewThreadMode(true);
            setComposerError(null);
          }

          if (isTauri()) {
            void syncWorkspaceSidebar().catch((error) => {
              const message = getInvokeErrorMessage(error, "刷新线程列表失败");
              setTerminalBootstrapError(message);
            });
          }
        } catch (error) {
          const message = getInvokeErrorMessage(error, "删除线程失败");
          setTerminalBootstrapError(message);
        } finally {
          setDeletingThreadId(null);
          setPendingDeleteThreadId((current) =>
            current === threadId ? null : current,
          );
        }
      })();
    },
    [
      activeThread?.id,
      activeThreadProject,
      deletingThreadId,
      syncWorkspaceSidebar,
    ],
  );

  const handleComposerSubmit = (submission: ComposerSubmission) => {
    const trimmedValue = submission.displayText?.trim() ?? "";
    const commandBehavior = submission.command?.behavior ?? null;
    const effectivePrompt = submission.effectivePrompt;

    if (!effectivePrompt) {
      return;
    }

    if (isNewThreadMode) {
      if (commandBehavior === "clear" || commandBehavior === "compact") {
        setComposerValue("");
        setComposerError(null);
        return;
      }

      void (async () => {
        if (!selectedProject) {
          return;
        }

        if (isTauri() && !resolvedWorkspaceId) {
          setComposerError(
            "Workspace is still preparing. Try again in a moment.",
          );
          return;
        }

        const project = {
          ...selectedProject,
          lastOpenedLabel: "刚刚",
        };
        const existingWorkspace = workspaces.find(
          (workspace) =>
            workspace.id === resolvedWorkspaceId ||
            workspace.id === project.id ||
            workspace.name === project.name ||
            (workspace.path && workspace.path === project.path),
        );
        const nextPendingRunId =
          typeof crypto !== "undefined" && "randomUUID" in crypto
            ? crypto.randomUUID()
            : `${Date.now()}`;

        let persistedThreadId =
          newThreadTerminalBindingKey === null
            ? null
            : (terminalThreadBindings[newThreadTerminalBindingKey] ?? null);
        const nextThreadName = buildThreadTitle(trimmedValue || effectivePrompt);

        try {
          if (isTauri() && resolvedWorkspaceId) {
            if (!persistedThreadId) {
              persistedThreadId = await getOrCreateNewThreadId(resolvedWorkspaceId);
            }
          }
        } catch (error) {
          const message = getInvokeErrorMessage(error, "创建线程失败");
          setComposerError(message);
          return;
        }

        const nextThread = {
          id: persistedThreadId ?? `${project.id}-thread-${Date.now()}`,
          name: nextThreadName,
          time: "刚刚",
          active: true,
          status: "running" as const,
        };

        setSelectedProject(project);
        setRecentProjects((current) => mergeRecentProjects(current, project));
        setWorkspaces((current) => {
          const cleared = clearActiveThreads(current);

          if (existingWorkspace) {
            return cleared.map((workspace) =>
              workspace.id === existingWorkspace.id
                ? {
                    ...workspace,
                    name: project.name,
                    path: project.path,
                    threads: [nextThread, ...workspace.threads],
                  }
                : workspace,
            );
          }

          return [
            {
              id: resolvedWorkspaceId ?? project.id,
              name: project.name,
              defaultOpen: true,
              path: project.path,
              threads: [nextThread],
            },
            ...cleared,
          ];
        });
        setOpenWorkspaces((current) => ({
          ...current,
          [existingWorkspace?.id ?? resolvedWorkspaceId ?? project.id]: true,
        }));

        if (activeTerminalStateKey) {
          setTerminalCollapsedByThreadKey((current) => {
            if (!(activeTerminalStateKey in current)) {
              return current;
            }

            const next = {
              ...current,
              [nextThread.id]: current[activeTerminalStateKey],
            };
            delete next[activeTerminalStateKey];
            return next;
          });
        }

        if (persistedThreadId) {
          setTerminalThreadBindings((current) => {
            if (!newThreadTerminalBindingKey || !persistedThreadId) {
              return current;
            }

            if (current[newThreadTerminalBindingKey] === persistedThreadId) {
              return current;
            }

            return {
              ...current,
              [newThreadTerminalBindingKey]: persistedThreadId,
            };
          });

          setPendingThreadRuns((current) => ({
            ...current,
            [persistedThreadId]: {
              id: nextPendingRunId,
              displayText: submission.displayText,
              effectivePrompt,
              attachments: submission.attachments,
              metadata: submission.metadata ?? null,
              runMode: submission.runMode ?? newThreadRunMode,
              threadId: persistedThreadId,
            },
          }));
        }

        setNewThreadMode(false);
        setNewThreadRunMode("default");
        setComposerValue("");
        setComposerError(null);
      })();
      return;
    }

    setComposerValue("");
    setComposerError(null);
  };

  const handleThemeSelect = (nextTheme: ThemePreference) => {
    setTheme(nextTheme);
    setOpenSettingsSection("theme");
  };

  const handleLanguageSelect = (nextLanguage: LanguagePreference) => {
    setLanguage(nextLanguage);
    setOpenSettingsSection("language");
  };

  const handleOpenSettings = (category: SettingsCategory = "account") => {
    setActiveSettingsCategory(category);
    setActiveOverlay("settings");
    setUserMenuOpen(false);
    setOpenSettingsSection(null);
  };

  const handleOpenMarketplace = () => {
    setActiveOverlay("marketplace");
    setUserMenuOpen(false);
    setOpenSettingsSection(null);
  };

  const handleChooseWorkspaceFolder = useCallback(() => {
    if (!isTauri() || isAddingWorkspace) {
      return;
    }

    void (async () => {
      setAddingWorkspace(true);
      setTerminalBootstrapError(null);

      try {
        const selectedPath = await open({
          directory: true,
          multiple: false,
          title: "Choose workspace folder",
        });

        if (typeof selectedPath !== "string") {
          return;
        }

        const nextProject = buildProjectOptionFromPath(selectedPath);

        if (!nextProject) {
          return;
        }

        const workspaceEntries = await workspaceList();
        const existingWorkspace =
          workspaceEntries.find(
            (workspace) =>
              workspace.path === selectedPath ||
              workspace.canonicalPath === selectedPath,
          ) ?? null;
        const workspace =
          existingWorkspace ??
          (await workspaceAdd(selectedPath, nextProject.name));

        await syncWorkspaceSidebar();
        setOpenWorkspaces((current) => ({
          ...current,
          [workspace.id]: true,
        }));
      } catch (error) {
        const message = getInvokeErrorMessage(error, "Failed to add workspace");
        setTerminalBootstrapError(message);
      } finally {
        setAddingWorkspace(false);
      }
    })();
  }, [isAddingWorkspace, syncWorkspaceSidebar]);

  const handleWorkspaceMenuToggle = (workspaceId: string) => {
    setActiveWorkspaceMenuId((current) =>
      current === workspaceId ? null : workspaceId,
    );
  };

  const handleOpenWorkspaceInSystem = useCallback(
    (workspace: WorkspaceItem) => {
      if (!isTauri() || !workspace.path || workspaceAction) {
        return;
      }

      const appId = isWindows ? "explorer" : "finder";

      void (async () => {
        setWorkspaceAction({
          workspaceId: workspace.id,
          kind: "open",
        });
        setTerminalBootstrapError(null);

        try {
          await invoke("open_workspace_in_app", {
            targetPath: workspace.path,
            appId,
            appPath: null,
          });
          setActiveWorkspaceMenuId(null);
        } catch (error) {
          const message = getInvokeErrorMessage(
            error,
            `Couldn't open ${workspace.name}`,
          );
          setTerminalBootstrapError(message);
        } finally {
          setWorkspaceAction(null);
        }
      })();
    },
    [isWindows, workspaceAction],
  );

  const handleWorkspaceRemove = useCallback(
    (workspace: WorkspaceItem) => {
      if (!isTauri() || workspaceAction) {
        return;
      }

      void (async () => {
        const workspaceThreadIds = new Set(
          workspace.threads.map((thread) => thread.id),
        );
        const nextThreadBindingKey = getNewThreadTerminalBindingKey(
          workspace.id,
        );
        newThreadCreationRef.current = Object.fromEntries(
          Object.entries(newThreadCreationRef.current).filter(
            ([candidateWorkspaceId]) => candidateWorkspaceId !== workspace.id,
          ),
        );
        const isRemovingActiveWorkspace =
          activeThreadWorkspace?.id === workspace.id;
        const shouldPreserveSelectedProject =
          selectedProject?.id !== workspace.id &&
          selectedProject?.path !== workspace.path;

        setWorkspaceAction({
          workspaceId: workspace.id,
          kind: "remove",
        });
        setTerminalBootstrapError(null);

        try {
          await workspaceRemove(workspace.id);

          if (isRemovingActiveWorkspace) {
            setNewThreadMode(true);
            setWorkspaces((current) => clearActiveThreads(current));
            setComposerError(null);
            setSelectedDiffSelection(null);
          }

          setPendingDeleteThreadId((current) =>
            current && workspaceThreadIds.has(current) ? null : current,
          );
          setPendingThreadRuns((current) =>
            Object.fromEntries(
              Object.entries(current).filter(
                ([threadId]) => !workspaceThreadIds.has(threadId),
              ),
            ),
          );
          setTerminalThreadBindings((current) =>
            Object.fromEntries(
              Object.entries(current).filter(
                ([bindingKey, threadId]) =>
                  bindingKey !== nextThreadBindingKey &&
                  !workspaceThreadIds.has(threadId),
              ),
            ),
          );
          setTerminalWorkspaceBindings((current) =>
            Object.fromEntries(
              Object.entries(current).filter(
                ([, workspaceId]) => workspaceId !== workspace.id,
              ),
            ),
          );
          setTerminalCollapsedByThreadKey((current) =>
            Object.fromEntries(
              Object.entries(current).filter(
                ([threadId]) => !workspaceThreadIds.has(threadId),
              ),
            ),
          );
          setOpenWorkspaces((current) => {
            if (!(workspace.id in current)) {
              return current;
            }

            const next = { ...current };
            delete next[workspace.id];
            return next;
          });
          setActiveWorkspaceMenuId(null);

          await syncWorkspaceSidebar({
            preserveSelectedProjectIfMissing: shouldPreserveSelectedProject,
          });
        } catch (error) {
          const message = getInvokeErrorMessage(
            error,
            `Failed to remove ${workspace.name}`,
          );
          setTerminalBootstrapError(message);
        } finally {
          setWorkspaceAction(null);
        }
      })();
    },
    [
      activeThreadWorkspace?.id,
      selectedProject?.id,
      selectedProject?.path,
      syncWorkspaceSidebar,
      workspaceAction,
    ],
  );

  const handleUserMenuToggle = () => {
    setUserMenuOpen((current) => {
      const nextOpen = !current;
      setOpenSettingsSection(null);
      return nextOpen;
    });
  };

  const handleLogin = () => {
    setUserSession(MOCK_USER_SESSION);
    setOpenSettingsSection(null);
    setUserMenuOpen(!isOverlayOpen);
  };

  const handleLogout = () => {
    setUserSession(null);
    setOpenSettingsSection(null);
    setUserMenuOpen(false);
  };

  const handleCheckUpdates = () => {
    if (isCheckingUpdates) {
      return;
    }

    setCheckingUpdates(true);

    window.setTimeout(() => {
      setCheckingUpdates(false);
      setUpdateStatus(`当前已是最新版本 v${data?.version ?? "0.1.0"}`);
    }, 900);
  };

  const workspaceOpenLabel = isWindows
    ? "Open in Explorer"
    : isMacOS
      ? "Open in Finder"
      : "Open folder";
  const canOpenWorkspaceInSystem = isTauri() && (isMacOS || isWindows);
  const selectedThemeOption =
    THEME_OPTIONS.find((option) => option.value === theme) ?? THEME_OPTIONS[0];
  const selectedThemeSummary =
    theme === "system" ? "跟随系统" : selectedThemeOption.label;
  const selectedLanguageOption =
    LANGUAGE_OPTIONS.find((option) => option.value === language) ??
    LANGUAGE_OPTIONS[1];
  const newThreadTerminalIdleMessage = !selectedProject
    ? "选择workspace后可进入 Terminal"
    : !resolvedWorkspaceId && !terminalBootstrapError
      ? "Preparing workspace…"
      : undefined;

  useEffect(() => {
    if (!isMacOS || typeof window === "undefined") {
      return;
    }

    const handleKeyDown = (event: KeyboardEvent) => {
      if (
        event.defaultPrevented ||
        !event.metaKey ||
        event.ctrlKey ||
        event.altKey ||
        event.shiftKey
      ) {
        return;
      }

      if (event.key !== ",") {
        return;
      }

      event.preventDefault();
      handleOpenSettings("account");
    };

    window.addEventListener("keydown", handleKeyDown);
    return () => window.removeEventListener("keydown", handleKeyDown);
  }, [isMacOS]);

  return (
    <main className="h-screen overflow-hidden select-none bg-app-canvas text-app-foreground">
      <WorkbenchTopBar
        isMacOS={isMacOS}
        isWindows={isWindows}
        isSidebarOpen={isSidebarOpen}
        isDrawerOpen={isDrawerOpen}
        isTerminalCollapsed={isTerminalCollapsed}
        isUserMenuOpen={isUserMenuOpen}
        isOverlayOpen={isOverlayOpen}
        isLoggedIn={Boolean(userSession)}
        userSession={userSession}
        isCheckingUpdates={isCheckingUpdates}
        updateStatus={updateStatus}
        openSettingsSection={openSettingsSection}
        userMenuRef={userMenuRef}
        selectedLanguageLabel={selectedLanguageOption.label}
        selectedThemeSummary={selectedThemeSummary}
        language={language}
        theme={theme}
        onToggleUserMenu={handleUserMenuToggle}
        onLogin={handleLogin}
        onLogout={handleLogout}
        onCheckUpdates={handleCheckUpdates}
        onOpenSettings={() => handleOpenSettings("account")}
        onSelectLanguage={handleLanguageSelect}
        onSelectTheme={handleThemeSelect}
        onToggleSettingsSection={setOpenSettingsSection}
        onToggleSidebar={() => setSidebarOpen((current) => !current)}
        onToggleDrawer={() => setDrawerOpen((current) => !current)}
        onToggleTerminal={() => setTerminalCollapsed((current) => !current)}
      />

      <div className="flex h-full min-h-0 pt-9">
        <aside
          className={cn(
            "overflow-hidden bg-app-sidebar transition-[width,opacity,transform] duration-300 ease-[cubic-bezier(0.22,1,0.36,1)]",
            isSidebarOpen
              ? "w-[320px] border-r border-app-border opacity-100 translate-x-0"
              : "w-0 border-r-0 opacity-0 -translate-x-2 pointer-events-none",
          )}
        >
          <div className="flex h-full min-h-0 flex-col px-3 pb-3 pt-4">
            <div className="space-y-1">
              <button
                type="button"
                className={cn(
                  "group flex w-full items-center gap-2.5 rounded-xl border px-3 py-2.5 text-left transition-[transform,box-shadow,background-color,border-color,color] duration-200 active:scale-[0.99]",
                  isNewThreadMode
                    ? "border-app-border-strong bg-app-surface-active text-app-foreground shadow-[0_4px_14px_rgba(15,23,42,0.08)]"
                    : "border-transparent bg-transparent text-app-muted hover:border-app-border hover:bg-app-surface-hover hover:text-app-foreground hover:shadow-[0_4px_14px_rgba(15,23,42,0.08)]",
                )}
                onClick={handleEnterNewThreadMode}
              >
                <MessageSquarePlus
                  className={cn(
                    "size-4 shrink-0 transition-colors duration-200",
                    isNewThreadMode
                      ? "text-app-foreground"
                      : "text-app-subtle group-hover:text-app-foreground",
                  )}
                />
                <span className="truncate text-sm font-medium">New thread</span>
              </button>

              <button
                type="button"
                className={cn(
                  "group flex w-full items-center gap-2.5 rounded-xl border px-3 py-2.5 text-left transition-[transform,box-shadow,background-color,border-color,color] duration-200 active:scale-[0.99]",
                  isMarketplaceOpen
                    ? "border-app-border-strong bg-app-surface-active text-app-foreground shadow-[0_4px_14px_rgba(15,23,42,0.08)]"
                    : "border-transparent bg-transparent text-app-muted hover:border-app-border hover:bg-app-surface-hover hover:text-app-foreground hover:shadow-[0_4px_14px_rgba(15,23,42,0.08)]",
                )}
                onClick={handleOpenMarketplace}
              >
                <Boxes
                  className={cn(
                    "size-4 shrink-0 transition-colors duration-200",
                    isMarketplaceOpen
                      ? "text-app-foreground"
                      : "text-app-subtle group-hover:text-app-foreground",
                  )}
                />
                <span className="truncate text-sm font-medium">
                  Marketplace
                </span>
              </button>
            </div>

            <div className="mt-6 flex items-center justify-between px-3">
              <span className="text-xs uppercase tracking-[0.14em] text-app-subtle">
                WORKSPACE
              </span>
              <button
                type="button"
                aria-label="Add workspace"
                title="Add workspace"
                className="inline-flex size-7 items-center justify-center rounded-md text-app-subtle transition-colors hover:bg-app-surface-hover hover:text-app-foreground disabled:cursor-not-allowed disabled:opacity-60"
                onClick={handleChooseWorkspaceFolder}
                disabled={isAddingWorkspace}
              >
                {isAddingWorkspace ? (
                  <LoaderCircle className="size-3.5 animate-spin" />
                ) : (
                  <FolderPlus className="size-3.5" />
                )}
              </button>
            </div>

            <div className="mx-1 mt-3 h-px shrink-0 bg-app-border" />

            <div className="mt-3 min-h-0 flex-1 overflow-auto overscroll-contain [scrollbar-width:none] [&::-webkit-scrollbar]:hidden">
              <div className="space-y-1.5">
                {workspaces.map((workspace) => {
                  const isOpen =
                    openWorkspaces[workspace.id] ?? workspace.defaultOpen;
                  const FolderIcon = isOpen ? FolderOpen : Folder;
                  const isWorkspaceMenuOpen =
                    activeWorkspaceMenuId === workspace.id;
                  const isOpeningWorkspace =
                    workspaceAction?.workspaceId === workspace.id &&
                    workspaceAction.kind === "open";
                  const isRemovingWorkspace =
                    workspaceAction?.workspaceId === workspace.id &&
                    workspaceAction.kind === "remove";
                  const visibleThreadCount =
                    workspaceThreadDisplayCounts[workspace.id] ??
                    WORKSPACE_THREAD_PAGE_SIZE;
                  const visibleThreads = workspace.threads.slice(
                    0,
                    visibleThreadCount,
                  );
                  const hasMoreThreads =
                    (workspaceThreadHasMore[workspace.id] ?? false) ||
                    workspace.threads.length > visibleThreadCount;
                  const isLoadingMoreThreads =
                    workspaceThreadLoadMorePending[workspace.id] ?? false;

                  return (
                    <div key={workspace.id} className="space-y-1">
                      <div className="group px-1">
                        <div
                          ref={
                            isWorkspaceMenuOpen ? workspaceMenuRef : undefined
                          }
                          className="relative"
                        >
                          <button
                            type="button"
                            className={cn(
                              "flex items-center gap-2 pr-10 text-app-muted hover:bg-app-surface-hover hover:text-app-foreground",
                              DRAWER_LIST_ROW_CLASS,
                            )}
                            onClick={() => handleWorkspaceToggle(workspace.id)}
                          >
                            <FolderIcon className="size-4 shrink-0 text-app-muted" />
                            <span className={DRAWER_LIST_LABEL_CLASS}>
                              {workspace.name}
                            </span>
                          </button>
                          <button
                            type="button"
                            aria-label="更多操作"
                            title="更多操作"
                            aria-haspopup="menu"
                            aria-expanded={isWorkspaceMenuOpen}
                            className={cn(
                              DRAWER_OVERFLOW_ACTION_CLASS,
                              isWorkspaceMenuOpen &&
                                "opacity-100 text-app-foreground",
                            )}
                            onClick={(event) => {
                              event.stopPropagation();
                              handleWorkspaceMenuToggle(workspace.id);
                            }}
                          >
                            <MoreHorizontal className="size-4" />
                          </button>

                          {isWorkspaceMenuOpen ? (
                            <div className="absolute right-0 top-[calc(100%+0.35rem)] z-20 min-w-[11rem] overflow-hidden rounded-xl border border-app-border bg-app-menu/98 p-1 shadow-[0_18px_40px_-26px_rgba(15,23,42,0.38)] backdrop-blur-xl dark:bg-app-menu/94">
                              <button
                                type="button"
                                role="menuitem"
                                className="flex w-full items-center gap-2 rounded-lg px-2.5 py-2 text-left text-sm text-app-foreground transition-colors hover:bg-app-surface-hover disabled:cursor-not-allowed disabled:text-app-subtle"
                                onClick={(event) => {
                                  event.stopPropagation();
                                  handleNewThreadForWorkspace(workspace);
                                }}
                                disabled={
                                  !workspace.path ||
                                  isOpeningWorkspace ||
                                  isRemovingWorkspace
                                }
                              >
                                <MessageSquarePlus className="size-4 shrink-0" />
                                <span>New Thread</span>
                              </button>
                              <button
                                type="button"
                                role="menuitem"
                                className="flex w-full items-center gap-2 rounded-lg px-2.5 py-2 text-left text-sm text-app-foreground transition-colors hover:bg-app-surface-hover disabled:cursor-not-allowed disabled:text-app-subtle"
                                onClick={(event) => {
                                  event.stopPropagation();
                                  handleOpenWorkspaceInSystem(workspace);
                                }}
                                disabled={
                                  !canOpenWorkspaceInSystem ||
                                  !workspace.path ||
                                  isOpeningWorkspace ||
                                  isRemovingWorkspace
                                }
                              >
                                {isOpeningWorkspace ? (
                                  <LoaderCircle className="size-4 shrink-0 animate-spin" />
                                ) : (
                                  <FolderOpen className="size-4 shrink-0" />
                                )}
                                <span>{workspaceOpenLabel}</span>
                              </button>
                              <button
                                type="button"
                                role="menuitem"
                                className="flex w-full items-center gap-2 rounded-lg px-2.5 py-2 text-left text-sm text-app-danger transition-colors hover:bg-app-danger/10 disabled:cursor-not-allowed disabled:opacity-60"
                                onClick={(event) => {
                                  event.stopPropagation();
                                  handleWorkspaceRemove(workspace);
                                }}
                                disabled={
                                  isOpeningWorkspace || isRemovingWorkspace
                                }
                              >
                                {isRemovingWorkspace ? (
                                  <LoaderCircle className="size-4 shrink-0 animate-spin" />
                                ) : (
                                  <Trash2 className="size-4 shrink-0" />
                                )}
                                <span>Remove</span>
                              </button>
                            </div>
                          ) : null}
                        </div>
                      </div>

                      {isOpen && visibleThreads.length > 0 ? (
                        <div className={cn(DRAWER_LIST_STACK_CLASS, "pl-2.5")}>
                          {visibleThreads.map((thread) => {
                            const isDeletePending =
                              pendingDeleteThreadId === thread.id;
                            const isDeleting = deletingThreadId === thread.id;

                            return (
                              <div key={thread.id} className="group relative">
                                <button
                                  type="button"
                                  className={cn(
                                    `${DRAWER_LIST_ROW_CLASS} border pr-[4.5rem]`,
                                    thread.active
                                      ? "border-app-border-strong bg-app-surface-active text-app-foreground"
                                      : "border-transparent bg-transparent text-app-muted hover:bg-app-surface-hover hover:text-app-foreground",
                                  )}
                                  onClick={() => handleThreadSelect(thread.id)}
                                >
                                  <div className="flex items-center gap-2">
                                    <ThreadStatusIndicator
                                      status={thread.status}
                                      emphasis={
                                        thread.active ? "default" : "subtle"
                                      }
                                    />
                                    <p className={DRAWER_LIST_LABEL_CLASS}>
                                      {thread.name}
                                    </p>
                                  </div>
                                </button>
                                <span
                                  className={cn(
                                    "pointer-events-none absolute right-2.5 top-1/2 -translate-y-1/2 text-[11px] text-app-subtle transition-opacity duration-200",
                                    isDeletePending || isDeleting
                                      ? "opacity-0"
                                      : "group-hover:opacity-0",
                                  )}
                                >
                                  {thread.time}
                                </span>
                                {isDeletePending || isDeleting ? (
                                  <button
                                    type="button"
                                    aria-label={
                                      isDeleting
                                        ? "正在删除 thread"
                                        : "确认删除 thread"
                                    }
                                    title={isDeleting ? "Deleting" : "Delete"}
                                    className="absolute right-1.5 top-1/2 inline-flex h-7 -translate-y-1/2 items-center justify-center rounded-md border border-app-danger/20 bg-app-danger/10 px-2 text-[11px] font-medium text-app-danger transition-colors hover:border-app-danger/30 hover:bg-app-danger/14 disabled:cursor-not-allowed disabled:opacity-80"
                                    onClick={(event) => {
                                      event.stopPropagation();
                                      handleThreadDeleteConfirm(thread.id);
                                    }}
                                    disabled={isDeleting}
                                  >
                                    {isDeleting ? (
                                      <LoaderCircle className="size-3.5 animate-spin" />
                                    ) : (
                                      "Delete"
                                    )}
                                  </button>
                                ) : (
                                  <button
                                    type="button"
                                    aria-label="删除 thread"
                                    title="Delete thread"
                                    className="absolute right-1.5 top-1/2 flex size-6 -translate-y-1/2 items-center justify-center rounded-md text-app-danger opacity-0 transition-all duration-200 hover:bg-app-danger/10 hover:text-app-danger group-hover:opacity-100"
                                    onClick={(event) => {
                                      event.stopPropagation();
                                      handleThreadDeleteRequest(thread.id);
                                    }}
                                  >
                                    <Trash2 className="size-4" />
                                  </button>
                                )}
                              </div>
                            );
                          })}
                          {hasMoreThreads ? (
                            <button
                              type="button"
                              className={cn(
                                `${DRAWER_LIST_ROW_CLASS} flex items-center justify-end gap-2 text-app-muted hover:bg-app-surface-hover hover:text-app-foreground`,
                                isLoadingMoreThreads && "cursor-wait",
                              )}
                              onClick={() =>
                                handleWorkspaceShowMore(workspace.id)
                              }
                              disabled={isLoadingMoreThreads}
                            >
                              <span>Show more</span>
                              {isLoadingMoreThreads ? (
                                <LoaderCircle className="size-3.5 animate-spin" />
                              ) : null}
                            </button>
                          ) : null}
                        </div>
                      ) : null}
                    </div>
                  );
                })}
              </div>
            </div>
          </div>
        </aside>

        <section className="min-h-0 min-w-0 flex-1">
          <div className="flex h-full min-h-0 flex-col">
            <div className="flex min-h-0 flex-1 overflow-hidden">
              <section
                ref={mainContentRef}
                className="min-h-0 min-w-0 flex-1 select-text bg-app-canvas"
              >
                <div className="flex h-full min-h-0 flex-col">
                  {isNewThreadMode ? (
                    <div className="relative min-h-0 flex-1 overflow-hidden bg-app-canvas">
                      <div className="pointer-events-none absolute left-1/2 top-0 h-56 w-[72rem] -translate-x-1/2 rounded-full bg-[radial-gradient(circle,rgba(120,180,255,0.11),transparent_68%)] blur-3xl" />
                      <div className="relative flex h-full min-h-0 flex-col">
                        <div className="flex min-h-0 flex-1 items-center justify-center px-6 pb-8 pt-6">
                          <NewThreadEmptyState
                            recentProjects={recentProjects}
                            selectedProject={selectedProject}
                            isOverlayOpen={isOverlayOpen}
                            onSelectProject={handleProjectSelect}
                          />
                        </div>

                        <div className="shrink-0 px-6 pb-6 pt-4">
                          <WorkbenchPromptComposer
                            activeAgentProfileId={activeAgentProfileId}
                            agentProfiles={agentProfiles}
                            canSubmitWhenAttachmentsOnly={false}
                            commands={commands.commands}
                            error={composerError}
                            onErrorMessageChange={setComposerError}
                            onRunModeChange={setNewThreadRunMode}
                            onSelectAgentProfile={setActiveAgentProfile}
                            onStop={() => undefined}
                            onSubmit={handleComposerSubmit}
                            placeholder="Ask Tiy anything, @ to add files, / for commands, $ for skills"
                            providers={providers}
                            runMode={newThreadRunMode}
                            showRunModeToggle
                            status="ready"
                            value={composerValue}
                            workspaceId={selectedProjectWorkspaceId}
                            onValueChange={setComposerValue}
                          />
                        </div>
                      </div>
                    </div>
                  ) : (
                    <>
                      <div className="flex h-12 items-center gap-3 px-5">
                        <div className="min-w-0 flex-1">
                          <div className="flex min-w-0 items-center gap-2">
                            {activeThread ? (
                              <ThreadStatusIndicator
                                status={activeThread.status}
                              />
                            ) : null}
                            <p className="truncate text-sm font-semibold text-app-foreground">
                              {activeThread?.name ?? AI_ELEMENTS_THREAD_TITLE}
                            </p>
                          </div>
                        </div>
                        <div className="ml-auto flex shrink-0 items-center gap-1.5">
                          <div className="group/context-window relative shrink-0">
                            <span
                              tabIndex={0}
                              className="relative inline-flex overflow-hidden rounded-full border border-app-border bg-app-surface-muted text-[11px] text-app-muted outline-none"
                            >
                              <span
                                className="pointer-events-none absolute inset-y-0 left-0 rounded-full bg-primary/12"
                                style={{
                                  width: `${contextBadge.usageRatio * 100}%`,
                                }}
                              />
                              <span className="relative inline-flex items-center gap-1.5 px-2 py-0.5">
                                <span className="text-app-subtle">Context</span>
                                <span className="font-semibold text-app-foreground">
                                  {contextBadge.usedLabel} /{" "}
                                  {contextBadge.totalLabel}
                                </span>
                              </span>
                            </span>
                            <div className="pointer-events-none absolute left-1/2 top-[calc(100%+0.5rem)] z-20 w-max min-w-[190px] -translate-x-1/2 translate-y-1 rounded-xl border border-app-border bg-app-menu px-3 py-2 text-center opacity-0 shadow-[0_14px_32px_rgba(15,23,42,0.14)] transition-[opacity,transform] duration-150 group-hover/context-window:translate-y-0 group-hover/context-window:opacity-100 group-focus-within/context-window:translate-y-0 group-focus-within/context-window:opacity-100 dark:shadow-[0_16px_36px_rgba(0,0,0,0.38)]">
                              <p className="whitespace-nowrap text-[11px] font-semibold text-app-foreground">
                                {contextBadge.usedPercent}% used
                                <span className="font-normal text-app-subtle">
                                  {" "}
                                  ({contextBadge.leftPercent}% left)
                                </span>
                              </p>
                              {contextBadge.modelDisplayName ? (
                                <p className="mt-1 whitespace-nowrap text-[11px] text-app-subtle">
                                  {contextBadge.modelDisplayName}
                                </p>
                              ) : null}
                              <p className="mt-1 whitespace-nowrap text-[11px] text-app-muted">
                                {contextBadge.usedLabel} /{" "}
                                {contextBadge.totalLabel} tokens used
                              </p>
                              <p className="mt-1 whitespace-nowrap text-[11px] text-app-muted">
                                In{" "}
                                {formatCompactTokenCount(
                                  contextBadge.inputTokens,
                                )}{" "}
                                · Out{" "}
                                {formatCompactTokenCount(
                                  contextBadge.outputTokens,
                                )}
                              </p>
                              {contextBadge.cacheReadTokens > 0 ||
                              contextBadge.cacheWriteTokens > 0 ? (
                                <p className="mt-1 whitespace-nowrap text-[11px] text-app-subtle">
                                  Cache R{" "}
                                  {formatCompactTokenCount(
                                    contextBadge.cacheReadTokens,
                                  )}{" "}
                                  · W{" "}
                                  {formatCompactTokenCount(
                                    contextBadge.cacheWriteTokens,
                                  )}
                                </p>
                              ) : null}
                            </div>
                          </div>
                          <button
                            type="button"
                            className="inline-flex items-center gap-1.5 text-xs text-app-subtle transition-colors hover:text-app-foreground"
                          >
                            <GitBranch className="size-3.5" />
                            <span>main</span>
                            <ChevronDown className="size-3.5" />
                          </button>
                        </div>
                      </div>

                      <RuntimeThreadSurface
                        activeAgentProfileId={activeAgentProfileId}
                        agentProfiles={agentProfiles}
                        commands={commands.commands}
                        key={resolvedTerminalThreadId ?? "runtime-thread-surface"}
                        initialPromptRequest={
                          resolvedTerminalThreadId
                            ? (pendingThreadRuns[resolvedTerminalThreadId] ?? null)
                            : null
                        }
                        onConsumeInitialPrompt={(id) => {
                          setPendingThreadRuns((current) => {
                            const next = Object.fromEntries(
                              Object.entries(current).filter(
                                ([, pendingRun]) => pendingRun.id !== id,
                              ),
                            );
                            return Object.keys(next).length ===
                              Object.keys(current).length
                              ? current
                              : next;
                          });
                        }}
                        onContextUsageChange={setRuntimeContextUsage}
                        onRunStateChange={handleRuntimeThreadRunStateChange}
                        onSelectAgentProfile={setActiveAgentProfile}
                        onThreadTitleChange={handleRuntimeThreadTitleChange}
                        providers={providers}
                        threadId={resolvedTerminalThreadId}
                        threadTitle={
                          activeThread?.name ?? AI_ELEMENTS_THREAD_TITLE
                        }
                        workspaceId={resolvedWorkspaceId}
                      />
                    </>
                  )}
                </div>
              </section>

              <aside
                className={cn(
                  "min-h-0 shrink-0 overflow-hidden bg-app-drawer transition-[width,opacity,transform] duration-300 ease-[cubic-bezier(0.22,1,0.36,1)]",
                  isDrawerOpen
                    ? "w-[360px] border-l border-app-border opacity-100 translate-x-0"
                    : "w-0 border-l-0 opacity-0 translate-x-2 pointer-events-none",
                )}
              >
                <div className="flex h-full min-h-0 flex-col">
                  <div className="sticky top-0 z-10 bg-app-drawer/95 px-3 py-2 backdrop-blur-xl">
                    <WorkbenchSegmentedControl
                      value={activeDrawerPanel}
                      className="w-full min-w-0"
                      options={[
                        {
                          value: "project",
                          label: "文件树",
                          title: "文件树 · Project Panel",
                          content: <FolderOpen className="size-4" />,
                        },
                        {
                          value: "git",
                          label: "版本控制",
                          title: "版本控制 · Git Panel",
                          content: <GitBranch className="size-4" />,
                        },
                      ]}
                      onValueChange={(panel) =>
                        setActiveDrawerPanel(panel as DrawerPanel)
                      }
                    />
                  </div>

                  <div className="min-h-0 flex-1 overscroll-none">
                    {activeDrawerPanel === "project" ? (
                      <ProjectPanel
                        currentProject={currentProject}
                        workspaceId={resolvedWorkspaceId}
                        workspaceBootstrapError={terminalBootstrapError}
                      />
                    ) : (
                      <GitPanel
                        workspaceId={resolvedWorkspaceId}
                        currentProject={currentProject}
                        workspaceBootstrapError={terminalBootstrapError}
                        layoutResizeSignal={
                          isTerminalCollapsed ? 0 : terminalHeight
                        }
                        commitMessageLanguage={
                          activeAgentProfile?.commitMessageLanguage ?? "English"
                        }
                        commitMessagePrompt={
                          activeAgentProfile?.commitMessagePrompt ?? ""
                        }
                        commitMessageModelPlan={commitMessageModelPlan}
                        onOpenDiffPreview={setSelectedDiffSelection}
                      />
                    )}
                  </div>
                </div>
              </aside>
            </div>

            <section
              className={cn(
                "relative shrink-0 overflow-hidden bg-app-terminal transition-[height,opacity,border-color] duration-300 ease-[cubic-bezier(0.22,1,0.36,1)]",
                isTerminalCollapsed
                  ? "border-t border-transparent opacity-0 pointer-events-none"
                  : "border-t border-app-border opacity-100",
              )}
              style={{ height: isTerminalCollapsed ? 0 : terminalHeight }}
            >
              <div
                className={cn(
                  "group absolute inset-x-0 top-0 z-10 flex h-4 -translate-y-1/2 items-start justify-center transition-opacity duration-200",
                  isTerminalCollapsed
                    ? "opacity-0"
                    : "cursor-row-resize opacity-100",
                )}
                role="presentation"
                onMouseDown={handleTerminalResizeStart}
              >
                <div className="mt-1.5 h-[2px] w-9 rounded-full bg-app-border opacity-50 transition-all duration-200 ease-out group-hover:w-14 group-hover:bg-app-border-strong group-hover:opacity-100" />
              </div>
              <div
                className={cn(
                  "flex h-full min-h-0 flex-col transition-opacity duration-200",
                  isTerminalCollapsed ? "opacity-0" : "opacity-100 delay-75",
                )}
              >
                <ThreadTerminalPanel
                  threadId={resolvedTerminalThreadId}
                  threadTitle={activeThread?.name ?? AI_ELEMENTS_THREAD_TITLE}
                  active={!isTerminalCollapsed}
                  bootstrapError={terminalBootstrapError}
                  isPendingThread={isNewThreadMode}
                  idleMessage={newThreadTerminalIdleMessage}
                  onCollapse={() => setTerminalCollapsed(true)}
                />
              </div>
            </section>
          </div>
        </section>
      </div>

      {selectedDiffSelection ? (
        <GitDiffPreviewPanel
          workspaceId={resolvedWorkspaceId}
          selection={selectedDiffSelection}
          onClose={() => setSelectedDiffSelection(null)}
        />
      ) : null}

      {isSettingsOpen ? (
        <SettingsCenterOverlay
          activeCategory={activeSettingsCategory}
          agentProfiles={agentProfiles}
          activeAgentProfileId={activeAgentProfileId}
          contentRef={overlayContentRef}
          generalPreferences={generalPreferences}
          isCheckingUpdates={isCheckingUpdates}
          language={language}
          policy={policy}
          commands={commands}
          providerCatalog={providerCatalog}
          providers={providers}
          systemMetadata={data}
          theme={theme}
          updateStatus={updateStatus}
          userSession={userSession}
          workspaces={settingsWorkspaces}
          onAddAgentProfile={addAgentProfile}
          onAddAllowEntry={addAllowEntry}
          onAddCommand={addCommand}
          onAddDenyEntry={addDenyEntry}
          onAddProvider={addProvider}
          onAddWorkspace={addWorkspace}
          onAddWritableRoot={addWritableRoot}
          onCheckUpdates={handleCheckUpdates}
          onClose={() => setActiveOverlay(null)}
          onDuplicateAgentProfile={duplicateAgentProfile}
          onLogin={handleLogin}
          onLogout={handleLogout}
          onRemoveAgentProfile={removeAgentProfile}
          onRemoveAllowEntry={removeAllowEntry}
          onRemoveCommand={removeCommand}
          onRemoveDenyEntry={removeDenyEntry}
          onRemoveProvider={removeProvider}
          onRemoveWorkspace={removeWorkspace}
          onRemoveWritableRoot={removeWritableRoot}
          onSelectCategory={setActiveSettingsCategory}
          onSelectLanguage={handleLanguageSelect}
          onSelectTheme={handleThemeSelect}
          onSetActiveAgentProfile={setActiveAgentProfile}
          onSetDefaultWorkspace={setDefaultWorkspace}
          onUpdateAgentProfile={updateAgentProfile}
          onUpdateAllowEntry={updateAllowEntry}
          onUpdateCommand={updateCommand}
          onUpdateDenyEntry={updateDenyEntry}
          onUpdateGeneralPreference={updateGeneralPreference}
          onUpdatePolicySetting={updatePolicySetting}
          onUpdateProvider={updateProvider}
          onFetchProviderModels={fetchProviderModels}
          onTestProviderModelConnection={testProviderModelConnection}
          onUpdateWritableRoot={updateWritableRoot}
        />
      ) : null}

      {isMarketplaceOpen ? (
        <MarketplaceOverlay
          contentRef={overlayContentRef}
          itemStates={marketplaceItemStates}
          onClose={() => setActiveOverlay(null)}
          onDisableItem={disableItem}
          onEnableItem={enableItem}
          onInstallItem={installItem}
          onUninstallItem={uninstallItem}
        />
      ) : null}
    </main>
  );
}
