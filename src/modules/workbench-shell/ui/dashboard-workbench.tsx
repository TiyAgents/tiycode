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
  FolderOpen,
  GitBranch,
} from "lucide-react";
import {
  useLanguage,
  type LanguagePreference,
} from "@/app/providers/language-provider";
import { useTheme, type ThemePreference } from "@/app/providers/theme-provider";
import { useT } from "@/i18n";
import { useExtensionsController, type ExtensionScope } from "@/modules/extensions-center/model/use-extensions-controller";
import {
  buildProfileModelPlan,
  buildRunModelPlanFromSelection,
} from "@/modules/settings-center/model/run-model-plan";
import type { ComposerSubmission } from "@/modules/workbench-shell/model/composer-commands";
import {
  useSettingsController,
  type SettingsCategory,
} from "@/modules/settings-center/model/use-settings-controller";
import { useAppUpdater } from "@/modules/workbench-shell/hooks/use-app-updater";
import {
  DEFAULT_TERMINAL_COLLAPSED,
  SIDEBAR_AUTO_REFRESH_GRACE_MS,
  SIDEBAR_AUTO_REFRESH_INTERVAL_MS,
  SIDEBAR_SYNC_MIN_GAP_MS,
  UNBOUND_NEW_THREAD_TERMINAL_STATE_KEY,
  WORKSPACE_THREAD_PAGE_SIZE,
  
  
  buildProjectOptionFromWorkspace,
  buildThreadContextBadgeData,
  formatCompactTokenCount,
  findWorkspaceForThread,
  getNewThreadTerminalBindingKey,
  
  mapRunFinishedStatusToThreadRunStatus,
  
  mergeLocalFallbackThreads,
  resolveActiveThreadWorkbenchProfileId,
  resolveThreadProfileId,
} from "@/modules/workbench-shell/ui/dashboard-workbench-logic";
import { isOnboardingCompleted } from "@/modules/onboarding/model/use-onboarding";
import type {
  GitSnapshotDto,
  ThreadSummaryDto,
} from "@/shared/types/api";
import {
  threadCreate,
  threadDelete,
  threadList,
  threadUpdateProfile,
  threadUpdateTitle,
  workspaceAdd,
  workspaceEnsureDefault,
  workspaceList,
  workspaceRemove,
  gitGetSnapshot,
  gitSubscribe,
} from "@/services/bridge";
import {
  DEFAULT_TERMINAL_HEIGHT,
  LANGUAGE_OPTIONS,
  MIN_TERMINAL_HEIGHT,
  MIN_WORKBENCH_HEIGHT,
  RECENT_PROJECTS,
  THEME_OPTIONS,
  TOPBAR_HEIGHT,
  UPDATE_STATUS_DURATION,
  
} from "@/modules/workbench-shell/model/fixtures";
import {
  activateThread,
  buildProjectOptionFromPath,
  
  buildWorkspaceItemsFromDtos,
  buildThreadTitle,
  clearActiveThreads,
  getActiveThread,
  isEditableSelectionTarget,
  isNodeInsideContainer,
  mergeRecentProjects,
  selectContainerContents,
} from "@/modules/workbench-shell/model/helpers";
import {
  addRemovedWorkspacePath,
  buildWorkspaceBindings,
  buildWorkspaceBindingsForEntry,
  deleteRemovedWorkspacePath,
  findWorkspaceByPath,
  getWorkspaceBindingId,
  hasRemovedWorkspacePath,
  resolveProjectForWorkspace,
} from "@/modules/workbench-shell/model/workspace-path-bindings";
import type {
  DrawerPanel,
  ProjectOption,
  WorkspaceItem,
} from "@/modules/workbench-shell/model/types";
import type { ExtensionDetail, SkillPreview } from "@/shared/types/extensions";
import { NewThreadEmptyState } from "@/modules/workbench-shell/ui/new-thread-empty-state";
import type { NewWorktreeDialogContext } from "@/modules/workbench-shell/ui/new-worktree-dialog";
import { ProjectPanel } from "@/modules/workbench-shell/ui/project-panel";
import { BranchSelector } from "@/modules/workbench-shell/ui/branch-selector";
import {
  RuntimeThreadSurface,
  type ThreadContextUsage,
} from "@/modules/workbench-shell/ui/runtime-thread-surface";
import {
  GitPanel,
} from "@/modules/workbench-shell/ui/source-control-panels";
import { ThreadStatusIndicator } from "@/modules/workbench-shell/ui/thread-status-indicator";
import { DashboardTerminalOrchestrator } from "@/modules/workbench-shell/ui/dashboard-terminal-orchestrator";
import { DashboardSidebar } from "@/modules/workbench-shell/ui/dashboard-sidebar";
import { DashboardOverlays } from "@/modules/workbench-shell/ui/dashboard-overlays";
import { WorkbenchPromptComposer } from "@/modules/workbench-shell/ui/workbench-prompt-composer";
import { WorkbenchTopBar } from "@/modules/workbench-shell/ui/workbench-top-bar";
import { useSystemMetadata } from "@/features/system-info/model/use-system-metadata";
import { cn } from "@/shared/lib/utils";
import { getInvokeErrorMessage } from "@/shared/lib/invoke-error";
import { isSameWorkspacePath } from "@/shared/lib/workspace-path";
import { waitForBackendReady } from "@/shared/lib/backend-ready";
import { WorkbenchSegmentedControl } from "@/shared/ui/workbench-segmented-control";
import { terminalStore } from "@/features/terminal/model/terminal-store";
import {
  threadStore,
  useStore,
  setThreadStatus,
  updateThreadTitle as setStoreThreadTitle,
  setDisplayCount as setStoreDisplayCount,
  setLoadMorePending as setStoreLoadMorePending,
  setOpenWorkspace as setStoreOpenWorkspace,
  setSidebarReady as setStoreSidebarReady,
  shallowEqual,
} from "@/modules/workbench-shell/model/thread-store";
import {
  uiLayoutStore,
  openOverlay,
  setOpenSettingsSection,
  setActiveDrawerPanel,
  setSelectedDiffSelection,
  setTerminalCollapsed,
  setTerminalHeight,
  setTerminalResize,
  setUserMenuOpen,
  setActiveWorkspaceMenuId,
  setShowOnboarding,
  removeTerminalCollapsedForThreads,
} from "@/modules/workbench-shell/model/ui-layout-store";
import {
  composerStore,
  setNewThreadValue,
  setNewThreadRunMode,
  setComposerError,
  clearNewThreadComposer,
} from "@/modules/workbench-shell/model/composer-store";


export function DashboardWorkbench() {
  const { data } = useSystemMetadata();
  const { theme, setTheme } = useTheme();
  const { language, setLanguage } = useLanguage();
  const t = useT();
  const [selectedProject, setSelectedProject] = useState<ProjectOption | null>(
    () => (isTauri() ? null : (RECENT_PROJECTS[0] ?? null)),
  );
  const {
    addMarketplaceSource,
    addMcpServer,
    detailByKey: extensionDetailByKey,
    disableExtension,
    disableSkill,
    enableExtension,
    enableSkill,
    error: extensionsError,
    extensions,
    configDiagnostics,
    getMarketplaceSourceRemovePlan,
    installMarketplaceItem,
    isLoading: areExtensionsLoading,
    loadDetail: loadExtensionDetail,
    loadSkillPreview,
    marketplaceItems,
    marketplaceSources,
    mcpServers,
    pluginCommandEntries,
    enabledSkillEntries,
    refresh: refreshExtensions,
    refreshMarketplaceSource,
    removeMarketplaceSource,
    removeMcpServer,
    rescanSkills,
    restartMcpServer,
    skillPreviewByKey,
    skills: extensionSkills,
    uninstallExtension,
    updateMcpServer,
  } = useExtensionsController(selectedProject?.path ?? null);
  const currentExtensionScope: ExtensionScope = selectedProject ? "workspace" : "global";
  /**
   * Resolve the scope that an enable/disable/update mutation should target for
   * a specific extension id. The scope of an item is determined by where it is
   * installed (plugins are always user-level; MCP/Skill scope is surfaced by
   * the backend on each record), _not_ by whatever scope the UI happens to be
   * showing. Using the UI's scope blindly would cause user-level MCP/Skill
   * toggles to land in the workspace config file, which violates the
   * install-location rule.
   */
  const resolveItemScope = useCallback(
    (id: string): ExtensionScope => {
      const mcpScope = mcpServers.find((server) => server.id === id)?.scope;
      if (mcpScope === "workspace" || mcpScope === "global") {
        return mcpScope;
      }
      const skillScope = extensionSkills.find((skill) => skill.id === id)?.scope;
      if (skillScope === "workspace" || skillScope === "global") {
        return skillScope;
      }
      // Plugins (and anything we can't classify from loaded state) live at the
      // user level today, so default to "global". This also matches the
      // behavior of marketplace-installed extensions.
      return "global";
    },
    [mcpServers, extensionSkills],
  );
  const extensionDetailById = useMemo<Record<string, ExtensionDetail>>(() => {
    // Entries are keyed by `${scope}:${workspacePath ?? "global"}:${id}` in the
    // controller cache. When the extensions list shows items from multiple
    // scopes at once (e.g. in a workspace view that includes user-level MCP
    // entries) we must not filter by the outer UI scope, otherwise details
    // loaded under an item's real scope would be invisible. Indexing by id is
    // sufficient because the controller keeps one entry per (scope, id) pair.
    return Object.fromEntries(
      Object.entries(extensionDetailByKey).map(([, value]) => [value.summary.id, value]),
    );
  }, [extensionDetailByKey]);
  const skillPreviewById = useMemo<Record<string, SkillPreview>>(() => {
    return Object.fromEntries(
      Object.entries(skillPreviewByKey).map(([, value]) => [value.record.id, value]),
    );
  }, [skillPreviewByKey]);
  const {
    general: generalPreferences,
    workspaces: settingsWorkspaces,
    providerCatalog,
    providers,
    commands,
    terminal,
    availableShells,
    policy,
    backendHydrated: settingsHydrated,
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
    updateTerminalSetting,
  } = useSettingsController();

  // ── Phase 1: threadStore subscriptions (replaces 9 useState + 3 useRef) ──
  const workspaces = useStore(threadStore, (s) => s.workspaces, shallowEqual);
  
  const isNewThreadMode = useStore(threadStore, (s) => s.isNewThreadMode);
  const isSidebarReady = useStore(threadStore, (s) => s.sidebarReady);
  const pendingThreadRuns = useStore(threadStore, (s) => s.pendingRuns, shallowEqual);

  // ── Phase 2: uiLayoutStore + composerStore subscriptions (replaces 16 useState) ──
  const activeOverlay = useStore(uiLayoutStore, (s) => s.activeOverlay);
  const panelVisibilityState = useStore(uiLayoutStore, (s) => s.panelVisibility, shallowEqual);
  const terminalCollapsedByThreadKey = useStore(uiLayoutStore, (s) => s.terminalCollapsedByThreadKey, shallowEqual);
  const terminalHeight = useStore(uiLayoutStore, (s) => s.terminalHeight);
  const terminalResize = useStore(uiLayoutStore, (s) => s.terminalResize);
  const isUserMenuOpen = useStore(uiLayoutStore, (s) => s.isUserMenuOpen);
  const activeWorkspaceMenuId = useStore(uiLayoutStore, (s) => s.activeWorkspaceMenuId);
  const activeDrawerPanel = useStore(uiLayoutStore, (s) => s.activeDrawerPanel);

  const composerValue = useStore(composerStore, (s) => s.newThreadValue);
  const composerError = useStore(composerStore, (s) => s.error);
  const newThreadRunMode = useStore(composerStore, (s) => s.newThreadRunMode);

  // ── Init onboarding visibility (one-time, based on localStorage) ──
  useEffect(() => {
    if (!isOnboardingCompleted()) {
      setShowOnboarding(true);
    }
  }, []);

  const [recentProjects, setRecentProjects] = useState<Array<ProjectOption>>(
    () => (isTauri() ? [] : [...RECENT_PROJECTS]),
  );
  const [terminalThreadBindings, setTerminalThreadBindings] = useState<
    Record<string, string>
  >({});
  const [activeThreadProfileIdOverride, setActiveThreadProfileIdOverride] = useState<string | null>(null);
  const appUpdater = useAppUpdater();
  const isCheckingUpdates = appUpdater.phase === "checking";
  const updateStatus =
    appUpdater.phase === "upToDate"
      ? t("dashboard.upToDate", { version: data?.version ?? "0.1.0" })
      : null;
  const [terminalBootstrapError, setTerminalBootstrapError] = useState<
    string | null
  >(null);
  const [pendingDeleteThreadId, setPendingDeleteThreadId] = useState<
    string | null
  >(null);
  const [deletingThreadId, setDeletingThreadId] = useState<string | null>(null);
  const [editingThreadId, setEditingThreadId] = useState<string | null>(null);
  const [isAddingWorkspace, setAddingWorkspace] = useState(false);
  const [workspaceAction, setWorkspaceAction] = useState<{
    workspaceId: string;
    kind: "open" | "remove";
  } | null>(null);
  const [worktreeDialogContext, setWorktreeDialogContext] =
    useState<NewWorktreeDialogContext | null>(null);
  const composerCommands = useMemo(
    () => [...commands.commands, ...pluginCommandEntries],
    [commands.commands, pluginCommandEntries],
  );
  const [runtimeContextUsage, setRuntimeContextUsage] =
    useState<ThreadContextUsage | null>(null);
  const [terminalWorkspaceBindings, setTerminalWorkspaceBindings] = useState<
    Record<string, string>
  >({});
  // Ref mirror of `terminalWorkspaceBindings` — used by effects that need to
  // read the latest bindings without depending on the object identity (which
  // changes on every sync and would otherwise cause re-entrant effect firing).
  const terminalWorkspaceBindingsRef = useRef(terminalWorkspaceBindings);
  terminalWorkspaceBindingsRef.current = terminalWorkspaceBindings;
  const [topBarGitSnapshot, setTopBarGitSnapshot] = useState<GitSnapshotDto | null>(null);
  const mainContentRef = useRef<HTMLElement | null>(null);
  const overlayContentRef = useRef<HTMLDivElement | null>(null);
  const userMenuRef = useRef<HTMLDivElement | null>(null);
  const workspaceMenuRef = useRef<HTMLDivElement | null>(null);
  const syncVersionRef = useRef(0);
  const editingThreadIdRef = useRef<string | null>(null);
  const sidebarAutoRefreshUntilRef = useRef(0);
  const sidebarSyncInFlightRef = useRef(false);
  // Coalescing / throttling state for `syncWorkspaceSidebar`. `inFlightPromise`
  // holds the currently-running sync (if any) so concurrent callers share it
  // instead of stacking IPC requests. `lastFinishedAt` records when the last
  // run completed, so the *next* call is delayed to honour the minimum gap.
  // `pendingOptions` accumulates the options for a trailing call when one is
  // queued, so overrides from any caller are merged correctly.
  const sidebarSyncInFlightPromiseRef = useRef<Promise<void> | null>(null);
  const sidebarSyncLastFinishedAtRef = useRef(0);
  const sidebarSyncPendingPromiseRef = useRef<Promise<void> | null>(null);
  const sidebarSyncPendingOptionsRef = useRef<{
    preserveSelectedProjectIfMissing?: boolean;
    threadDisplayCountOverrides: Record<string, number>;
  } | null>(null);
  const newThreadCreationRef = useRef<Record<string, Promise<string>>>({});
  const terminalThreadBindingsRef = useRef(terminalThreadBindings);
  terminalThreadBindingsRef.current = terminalThreadBindings;
  const removedWorkspacePathsRef = useRef<Set<string>>(new Set());

  const activeThread = getActiveThread(workspaces);
  const selectedProjectWorkspaceId = getWorkspaceBindingId(
    terminalWorkspaceBindings,
    selectedProject?.path ?? null,
  );
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
  const resolvedWorkspaceId = getWorkspaceBindingId(
    terminalWorkspaceBindings,
    currentProject?.path ?? null,
  );
  const { isSidebarOpen, isDrawerOpen } = panelVisibilityState;
  const isProjectPanelAutoRefreshActive =
    resolvedWorkspaceId !== null
    && isSidebarOpen
    && isDrawerOpen
    && activeDrawerPanel === "project";
  const isGitPanelAutoRefreshActive =
    resolvedWorkspaceId !== null
    && isSidebarOpen
    && isDrawerOpen
    && activeDrawerPanel === "git";
  const newThreadTerminalBindingKey =
    selectedProjectWorkspaceId === null
      ? null
      : getNewThreadTerminalBindingKey(selectedProjectWorkspaceId);
  const resolvedTerminalThreadId = isNewThreadMode
    ? newThreadTerminalBindingKey === null
      ? null
      : (terminalThreadBindings[newThreadTerminalBindingKey] ?? null)
    : (activeThread?.id ?? null);
  const activeTerminalStateKey = isNewThreadMode
    ? (newThreadTerminalBindingKey ?? UNBOUND_NEW_THREAD_TERMINAL_STATE_KEY)
    : (activeThread?.id ?? null);
  const isTerminalCollapsed =
    activeTerminalStateKey === null
      ? DEFAULT_TERMINAL_COLLAPSED
      : (terminalCollapsedByThreadKey[activeTerminalStateKey] ??
        DEFAULT_TERMINAL_COLLAPSED);
  const isMarketplaceOpen = activeOverlay === "marketplace";
  const isOverlayOpen = activeOverlay !== null;
  const isMacOS =
    data?.platform === "macos" ||
    (typeof navigator !== "undefined" && navigator.userAgent.includes("Mac"));
  const isWindows =
    data?.platform === "windows" ||
    (typeof navigator !== "undefined" &&
      navigator.userAgent.includes("Windows"));
  const workbenchActiveProfileId = useMemo(
    () =>
      resolveActiveThreadWorkbenchProfileId(
        isNewThreadMode ? null : activeThreadProfileIdOverride,
        activeAgentProfileId,
      ),
    [activeAgentProfileId, activeThreadProfileIdOverride, isNewThreadMode],
  );
  const selectedRunModelPlan = useMemo(
    () =>
      buildRunModelPlanFromSelection(
        workbenchActiveProfileId,
        agentProfiles,
        providers,
      ),
    [workbenchActiveProfileId, agentProfiles, providers],
  );
  const workbenchActiveAgentProfile = useMemo(
    () => {
      const matchedProfile = agentProfiles.find((profile) => profile.id === workbenchActiveProfileId) ?? null;
      if (matchedProfile) {
        return matchedProfile;
      }
      return isNewThreadMode ? (agentProfiles[0] ?? null) : null;
    },
    [workbenchActiveProfileId, agentProfiles, isNewThreadMode],
  );
  const commitMessageModelPlan = useMemo(
    () =>
      workbenchActiveAgentProfile
        ? buildProfileModelPlan(workbenchActiveAgentProfile, providers)
        : null,
    [workbenchActiveAgentProfile, providers],
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

  const branchSnapshot = useMemo(() => {
    if (!topBarGitSnapshot) return null;
    return {
      headRef: topBarGitSnapshot.headRef,
      isDetached: topBarGitSnapshot.isDetached,
      stagedFiles: topBarGitSnapshot.stagedFiles,
      unstagedFiles: topBarGitSnapshot.unstagedFiles,
      untrackedFiles: topBarGitSnapshot.untrackedFiles,
    conflictedFiles: topBarGitSnapshot.conflictedFiles,
    };
  }, [
    topBarGitSnapshot?.headRef,
    topBarGitSnapshot?.isDetached,
    topBarGitSnapshot?.stagedFiles,
    topBarGitSnapshot?.unstagedFiles,
    topBarGitSnapshot?.untrackedFiles,
    topBarGitSnapshot?.conflictedFiles,
  ]);

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
      const pendingTerminalThreadIds = new Set(
        Object.values(terminalThreadBindingsRef.current),
      );
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
          if (pendingTerminalThreadIds.has(thread.id)) {
            continue;
          }
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
    const pendingThreadId = terminalThreadBindingsRef.current[bindingKey] ?? null;
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
    if (pendingThreadId && isTauri()) {
      terminalStore.removeSession(pendingThreadId);
      void threadDelete(pendingThreadId).catch((error) => {
        console.warn("[terminal] failed to delete pending thread:", pendingThreadId, error);
      });
    }
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

      const creationPromise = threadCreate(workspaceId, "", activeAgentProfileId)
        .then((thread) => {
          setActiveThreadProfileIdOverride(thread.profileId ?? activeAgentProfileId);
          threadStore.setState((prev) => ({
            workspaces: prev.workspaces.map((workspace) => ({
              ...workspace,
              threads: workspace.threads.map((candidate) =>
                candidate.id === thread.id
                  ? {
                      ...candidate,
                      profileId: thread.profileId,
                    }
                  : candidate,
              ),
            })),
          }));
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
    [activeAgentProfileId, terminalThreadBindings],
  );

  useEffect(() => {
    if (
      !isNewThreadMode ||
      isTerminalCollapsed ||
      !selectedProjectWorkspaceId ||
      resolvedTerminalThreadId
    ) {
      return;
    }

    getOrCreateNewThreadId(selectedProjectWorkspaceId).catch((error) => {
      setTerminalBootstrapError(
        getInvokeErrorMessage(error, "Failed to prepare terminal"),
      );
    });
  }, [
    isNewThreadMode,
    isTerminalCollapsed,
    selectedProjectWorkspaceId,
    resolvedTerminalThreadId,
    getOrCreateNewThreadId,
  ]);

  // The actual work of a sidebar sync. Held in a ref so that the public
  // `syncWorkspaceSidebar` callback below can have stable identity (empty
  // deps) — callbacks that depend on it (useEffect, useCallback) then stay
  // stable too, which is critical because `syncWorkspaceSidebar` mutates
  // state that several effects observe, and we don't want those effects to
  // re-trigger sync.
  const runSyncWorkspaceSidebarRef = useRef<
    (options: {
      preserveSelectedProjectIfMissing?: boolean;
      threadDisplayCountOverrides: Record<string, number>;
    }) => Promise<void>
  >(async () => {});

  runSyncWorkspaceSidebarRef.current = async ({
    preserveSelectedProjectIfMissing = true,
    threadDisplayCountOverrides = {},
  }: {
    preserveSelectedProjectIfMissing?: boolean;
    threadDisplayCountOverrides: Record<string, number>;
  }) => {
      const syncStart = performance.now();
      const version = ++syncVersionRef.current;

      const t0 = performance.now();
      console.log(`⏱ [sidebar-sync] firing workspaceList() at ${t0.toFixed(1)}ms since page load`);
      const workspaceEntries = await workspaceList();
      console.log(`⏱ [sidebar-sync] workspaceList: ${(performance.now() - t0).toFixed(1)}ms (${workspaceEntries.length} workspaces)`);

      const nextDisplayCounts = Object.fromEntries(
        workspaceEntries.map((workspace) => [
          workspace.id,
          threadDisplayCountOverrides[workspace.id] ??
            threadStore.getState().displayCounts[workspace.id] ??
            WORKSPACE_THREAD_PAGE_SIZE,
        ]),
      );
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
        .map((workspace) => buildProjectOptionFromWorkspace(workspace, language))
        .filter((project): project is ProjectOption => project !== null);
      const nextBindings = buildWorkspaceBindings(workspaceEntries);
      const defaultWorkspace =
        workspaceEntries.find((workspace) => workspace.isDefault) ?? null;
      const defaultProject =
        defaultWorkspace === null
          ? null
          : (nextProjects.find(
              (project) =>
                project.id === defaultWorkspace.id
                || isSameWorkspacePath(project.path, defaultWorkspace.canonicalPath)
                || isSameWorkspacePath(project.path, defaultWorkspace.path),
            ) ?? null);

      setTerminalWorkspaceBindings((current) => {
        // Merge `nextBindings` with existing bindings instead of replacing
        // outright. Other code paths (e.g. the effect at L1498 and the
        // new-thread activation flow) can inject additional path aliases for
        // a workspace — typically the user-facing path when it differs from
        // `workspace.path` / `workspace.canonicalPath` after normalization
        // (e.g. symlinks, macOS `/private` prefix, case differences on
        // Windows). Blowing those aliases away every sync caused a feedback
        // loop: effect re-injects the alias, next sync wipes it, effect fires
        // again.
        //
        // Behaviour:
        //   1. Keys pointing to a workspace that still exists in the latest
        //      `workspaceEntries` are preserved (so user-injected aliases
        //      survive), *unless* `nextBindings` itself rebinds that key.
        //   2. Keys pointing to a workspace that has been removed are dropped.
        //   3. `nextBindings` wins on conflict (authoritative path → id map).
        const liveWorkspaceIds = new Set(
          workspaceEntries.map((workspace) => workspace.id),
        );
        const preservedAliases: Record<string, string> = {};
        for (const [pathKey, workspaceId] of Object.entries(current)) {
          if (liveWorkspaceIds.has(workspaceId)) {
            preservedAliases[pathKey] = workspaceId;
          }
        }
        const merged = { ...preservedAliases, ...nextBindings };

        // Avoid producing a new object reference if nothing actually changed.
        // This is critical for any effect that still compares bindings by
        // reference, and lets downstream memos stay stable.
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
          if (identical) {
            return current;
          }
        }

        return merged;
      });
      setRecentProjects(nextProjects);
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
      setSelectedProject((current) => {
        if (current) {
          const matchingProject =
            nextProjects.find(
              (project) =>
                project.id === current.id
                || isSameWorkspacePath(project.path, current.path),
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
      threadStore.setState((prev) => {
        const activeThreadId = getActiveThread(prev.workspaces)?.id ?? null;
        const syncedWorkspaces = buildWorkspaceItemsFromDtos(
          workspaceEntries,
          threadsByWorkspaceId,
          activeThreadId,
          language,
        );
        const mergedWithFallbacks = mergeLocalFallbackThreads({
          currentWorkspaces: prev.workspaces,
          syncedWorkspaces,
        });

        const nextWorkspaces = mergedWithFallbacks.map((workspace) => {
          const currentWorkspace =
            prev.workspaces.find((candidate) => candidate.id === workspace.id) ?? null;

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
                && currentTitle !== t("dashboard.newThread")
                && syncedTitle === t("dashboard.newThread")
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

        const nextOpenWorkspaces: Record<string, boolean> = {};
        for (const workspace of workspaceEntries) {
          nextOpenWorkspaces[workspace.id] =
            prev.openWorkspaces[workspace.id] ??
            (workspace.isDefault || workspaceEntries.length === 1);
        }

        return { workspaces: nextWorkspaces, openWorkspaces: nextOpenWorkspaces };
      });
      console.log(`⏱ [sidebar-sync] total: ${(performance.now() - syncStart).toFixed(1)}ms`);
    };

  // Public entry point. Coalesces concurrent callers and enforces a minimum
  // gap between fully independent runs. Keeping the callback identity stable
  // (empty deps) is important so hooks/effects that depend on it don't
  // retrigger every time state changes.
  const syncWorkspaceSidebar = useCallback(
    async (
      options: {
        preserveSelectedProjectIfMissing?: boolean;
        threadDisplayCountOverrides?: Record<string, number>;
      } = {},
    ): Promise<void> => {
      // Merge requested overrides into any pending options so a trailing run
      // sees the union of what every caller asked for.
      const existing = sidebarSyncPendingOptionsRef.current ?? {
        preserveSelectedProjectIfMissing: undefined,
        threadDisplayCountOverrides: {},
      };
      sidebarSyncPendingOptionsRef.current = {
        preserveSelectedProjectIfMissing:
          options.preserveSelectedProjectIfMissing
            ?? existing.preserveSelectedProjectIfMissing,
        threadDisplayCountOverrides: {
          ...existing.threadDisplayCountOverrides,
          ...(options.threadDisplayCountOverrides ?? {}),
        },
      };

      // If a run is currently in flight, or a trailing run is already queued,
      // join it instead of starting another one.
      if (sidebarSyncPendingPromiseRef.current) {
        return sidebarSyncPendingPromiseRef.current;
      }
      if (sidebarSyncInFlightPromiseRef.current) {
        // Schedule a trailing run to start after the current one completes,
        // once the minimum gap has elapsed.
        const trailing = sidebarSyncInFlightPromiseRef.current.then(async () => {
          const elapsed = Date.now() - sidebarSyncLastFinishedAtRef.current;
          const wait = Math.max(0, SIDEBAR_SYNC_MIN_GAP_MS - elapsed);
          if (wait > 0) {
            await new Promise<void>((resolve) => setTimeout(resolve, wait));
          }
          const optionsToRun = sidebarSyncPendingOptionsRef.current ?? {
            threadDisplayCountOverrides: {},
          };
          sidebarSyncPendingOptionsRef.current = null;
          sidebarSyncPendingPromiseRef.current = null;
          const run = runSyncWorkspaceSidebarRef.current({
            preserveSelectedProjectIfMissing:
              optionsToRun.preserveSelectedProjectIfMissing,
            threadDisplayCountOverrides:
              optionsToRun.threadDisplayCountOverrides,
          });
          sidebarSyncInFlightPromiseRef.current = run.finally(() => {
            sidebarSyncLastFinishedAtRef.current = Date.now();
            sidebarSyncInFlightPromiseRef.current = null;
          });
          return sidebarSyncInFlightPromiseRef.current;
        });
        sidebarSyncPendingPromiseRef.current = trailing;
        return trailing;
      }

      // No run in flight: honour minimum gap before starting.
      const elapsed = Date.now() - sidebarSyncLastFinishedAtRef.current;
      const wait = Math.max(0, SIDEBAR_SYNC_MIN_GAP_MS - elapsed);
      const start = async () => {
        const optionsToRun = sidebarSyncPendingOptionsRef.current ?? {
          threadDisplayCountOverrides: {},
        };
        sidebarSyncPendingOptionsRef.current = null;
        const run = runSyncWorkspaceSidebarRef.current({
          preserveSelectedProjectIfMissing:
            optionsToRun.preserveSelectedProjectIfMissing,
          threadDisplayCountOverrides:
            optionsToRun.threadDisplayCountOverrides,
        });
        sidebarSyncInFlightPromiseRef.current = run.finally(() => {
          sidebarSyncLastFinishedAtRef.current = Date.now();
          sidebarSyncInFlightPromiseRef.current = null;
        });
        return sidebarSyncInFlightPromiseRef.current;
      };
      if (wait > 0) {
        const delayed = new Promise<void>((resolve) =>
          setTimeout(resolve, wait),
        ).then(start);
        sidebarSyncPendingPromiseRef.current = delayed.finally(() => {
          sidebarSyncPendingPromiseRef.current = null;
        });
        return sidebarSyncPendingPromiseRef.current;
      }
      return start();
    },
    [],
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
          const { threadId, runId } = event.payload;
          setThreadStatus(threadId, "running", {
            runId,
            source: "tauri_event",
          });

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
          const { threadId, runId, status } = event.payload;
          setThreadStatus(
            threadId,
            mapRunFinishedStatusToThreadRunStatus(status),
            { runId, source: "tauri_event" },
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

          // Skip update for the thread currently being edited inline.
          if (editingThreadIdRef.current === threadId) {
            return;
          }

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
        const message = getInvokeErrorMessage(error, t("dashboard.error.refreshThreadList"));
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
      const current = uiLayoutStore.getState().terminalHeight;
      setTerminalHeight(Math.min(current, getMaxTerminalHeight()));
    };

    syncTerminalHeight();
    window.addEventListener("resize", syncTerminalHeight);

    return () => window.removeEventListener("resize", syncTerminalHeight);
  }, []);

  useEffect(() => {
    if (!isTauri()) {
      return;
    }

    console.log("⏱ [startup] syncWorkspaceSidebar initial useEffect fired");
    let cancelled = false;

    void (async () => {
      await waitForBackendReady();
      if (cancelled) return;

      await syncWorkspaceSidebar();
    })()
      .then(() => {
        if (cancelled) {
          return;
        }
        setStoreSidebarReady(true);
      })
      .catch((error) => {
        if (cancelled) {
          return;
        }

        const message = getInvokeErrorMessage(error, t("dashboard.error.workspaceInit"));
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

    if (hasRemovedWorkspacePath(removedWorkspacePathsRef.current, currentProject.path)) {
      return;
    }

    // Read latest bindings via ref so that `setTerminalWorkspaceBindings`
    // elsewhere (including from `syncWorkspaceSidebar` itself) does not
    // retrigger this effect — that was the source of a ~40ms infinite loop
    // where sync overwrote freshly-injected aliases and the effect kept
    // re-injecting them.
    if (getWorkspaceBindingId(terminalWorkspaceBindingsRef.current, currentProject.path)) {
      return;
    }

    let cancelled = false;
    setTerminalBootstrapError(null);

    void workspaceList()
      .then(async (workspaces) => {
        const existing = findWorkspaceByPath(workspaces, currentProject.path);
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
            getWorkspaceBindingId(current, currentProject.path)
            && getWorkspaceBindingId(current, workspace.canonicalPath)
          ) {
            return current;
          }

          return {
            ...current,
            ...buildWorkspaceBindingsForEntry(workspace, currentProject.path),
          };
        });
        void syncWorkspaceSidebar().catch((refreshError) => {
          if (cancelled) {
            return;
          }

          const message = getInvokeErrorMessage(
            refreshError,
            t("dashboard.error.refreshWorkspaceList"),
          );
          setTerminalBootstrapError(message);
        });
      })
      .catch((error) => {
        if (cancelled) {
          return;
        }

        const message = getInvokeErrorMessage(error, t("dashboard.error.addWorkspace"));
        setTerminalBootstrapError(message);
      });

    return () => {
      cancelled = true;
    };
  }, [currentProject, syncWorkspaceSidebar]);

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

    window.localStorage.removeItem("tiy-agent-auth-session");
  }, []);

  useEffect(() => {
    if (appUpdater.phase !== "upToDate" || typeof window === "undefined") {
      return;
    }

    const timeout = window.setTimeout(() => {
      appUpdater.dismiss();
    }, UPDATE_STATUS_DURATION);

    return () => window.clearTimeout(timeout);
  }, [appUpdater.phase, appUpdater.dismiss]);

  // ── Git snapshot subscription for branch display in thread title bar ──
  useEffect(() => {
    if (!isTauri() || !resolvedWorkspaceId) {
      setTopBarGitSnapshot(null);
      return;
    }

    // Clear stale snapshot immediately so the UI doesn't flash the previous
    // workspace's branch while the new subscription/fetch is in flight.
    setTopBarGitSnapshot(null);

    let cancelled = false;
    let unsubscribe: (() => Promise<void>) | null = null;

    // Subscribe first so we don't miss events during the initial fetch
    void gitSubscribe(resolvedWorkspaceId, (event) => {
      if (cancelled) return;
      if (event.type === "snapshot_updated") {
        setTopBarGitSnapshot(event.snapshot);
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

    // Then fetch the initial snapshot to fill the gap before subscription delivers
    void gitGetSnapshot(resolvedWorkspaceId)
      .then((snapshot) => {
        if (!cancelled && snapshot) {
          setTopBarGitSnapshot(snapshot);
        }
      })
      .catch(() => {});

    return () => {
      cancelled = true;
      if (unsubscribe) {
        void unsubscribe().catch(() => {});
      }
    };
  }, [resolvedWorkspaceId]);

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
    threadStore.setState((prev) => ({
      openWorkspaces: {
        ...prev.openWorkspaces,
        [workspaceId]: !prev.openWorkspaces[workspaceId],
      },
    }));
  };

  const handleWorkspaceShowMore = useCallback(
    (workspaceId: string) => {
      const nextDisplayCount =
        (threadStore.getState().displayCounts[workspaceId] ??
          WORKSPACE_THREAD_PAGE_SIZE) + WORKSPACE_THREAD_PAGE_SIZE;

      setStoreDisplayCount(workspaceId, nextDisplayCount);
      setStoreLoadMorePending(workspaceId, true);
      setTerminalBootstrapError(null);

      void syncWorkspaceSidebar({
        threadDisplayCountOverrides: {
          [workspaceId]: nextDisplayCount,
        },
      })
        .catch((error) => {
          const message = getInvokeErrorMessage(error, t("dashboard.error.loadMoreThreads"));
          setTerminalBootstrapError(message);
        })
        .finally(() => {
          setStoreLoadMorePending(workspaceId, false);
        });
    },
    [syncWorkspaceSidebar],
  );

  const handleEnterNewThreadMode = () => {
    if (selectedProjectWorkspaceId) {
      clearNewThreadBindingForWorkspace(selectedProjectWorkspaceId);
    }

    setActiveThreadProfileIdOverride(null);
    threadStore.setState({ isNewThreadMode: true });
    threadStore.setState((prev) => ({ workspaces: clearActiveThreads(prev.workspaces) }));
    setComposerError(null);
    setPendingDeleteThreadId(null);
    setTerminalBootstrapError(null);
  };

  const handleThreadSelect = (threadId: string) => {
    if (isNewThreadMode && selectedProjectWorkspaceId) {
      clearNewThreadBindingForWorkspace(selectedProjectWorkspaceId);
    }
    const nextActiveThread = workspaces
      .flatMap((workspace) => workspace.threads)
      .find((thread) => thread.id === threadId) ?? null;
    const resolvedProfileId = resolveThreadProfileId(
      nextActiveThread?.profileId ?? null,
      activeAgentProfileId,
    );
    setActiveThreadProfileIdOverride(resolvedProfileId);
    threadStore.setState({ isNewThreadMode: false });
    setActiveWorkspaceMenuId(null);
    setPendingDeleteThreadId(null);
    setTerminalBootstrapError(null);
    threadStore.setState((prev) => ({ workspaces: activateThread(prev.workspaces, threadId) }));

    // Align the selected project with the workspace that owns this thread so
    // the new-thread empty state and path bindings stay consistent when the
    // user toggles back to New Thread mode. Without this, selectedProject can
    // drift (e.g. it stays on a worktree after the user jumped back to a
    // thread belonging to the parent repo).
    const nextWorkspace = workspaces.find((workspace) =>
      workspace.threads.some((thread) => thread.id === threadId),
    );
    if (nextWorkspace && nextWorkspace.id !== selectedProject?.id) {
      const projectForWorkspace = recentProjects.find(
        (project) => project.id === nextWorkspace.id,
      );
      if (projectForWorkspace) {
        setSelectedProject({
          ...projectForWorkspace,
          lastOpenedLabel: t("time.justNow"),
        });
      }
    }

    // Thread selection should not overwrite the global default profile.
  };

  const handleProjectSelect = (project: ProjectOption) => {
    const nextProject = {
      ...project,
      lastOpenedLabel: t("time.justNow"),
    };

    deleteRemovedWorkspacePath(removedWorkspacePathsRef.current, nextProject.path);
    setSelectedProject(nextProject);
    setRecentProjects((current) => mergeRecentProjects(current, nextProject));
  };

  const activateWorkspaceAsNewThreadTarget = useCallback(
    (workspaceId: string, nextProject: ProjectOption) => {
      clearNewThreadBindingForWorkspace(workspaceId);
      deleteRemovedWorkspacePath(removedWorkspacePathsRef.current, nextProject.path);
      setSelectedProject(nextProject);
      setRecentProjects((current) => mergeRecentProjects(current, nextProject));
      setStoreOpenWorkspace(workspaceId, true);
      setActiveThreadProfileIdOverride(null);
      threadStore.setState({ isNewThreadMode: true });
      threadStore.setState((prev) => ({ workspaces: clearActiveThreads(prev.workspaces) }));
      setComposerError(null);
      setPendingDeleteThreadId(null);
      setTerminalBootstrapError(null);
      setActiveWorkspaceMenuId(null);
    },
    [clearNewThreadBindingForWorkspace],
  );

  const handleNewThreadForWorkspace = useCallback((workspace: WorkspaceItem) => {
    if (!workspace.path) {
      return;
    }

    const projectFromPath = buildProjectOptionFromPath(workspace.path, language);
    const nextProject: ProjectOption = {
      ...(projectFromPath ?? {
        id: workspace.id,
        name: workspace.name,
        path: workspace.path,
        lastOpenedLabel: t("time.justNow"),
      }),
      id: workspace.id,
      name: workspace.name,
      path: workspace.path,
      lastOpenedLabel: t("time.justNow"),
      // Preserve worktree-aware metadata so downstream effects (e.g. the
      // "set this workspace as default" auto-promotion) can correctly skip
      // worktree rows. Without this, selecting a worktree to start a new
      // thread would surface the backend error
      // "A worktree cannot be set as the default workspace" in the project
      // and git panels.
      kind: workspace.kind,
      parentWorkspaceId: workspace.parentWorkspaceId,
      worktreeHash: workspace.worktreeHash,
      branch: workspace.branch,
    };

    activateWorkspaceAsNewThreadTarget(workspace.id, nextProject);
  }, [activateWorkspaceAsNewThreadTarget, language, t]);

  const handleRuntimeThreadTitleChange = useCallback(
    (threadId: string, title: string) => {
      const trimmedTitle = title.trim();
      if (!trimmedTitle) {
        return;
      }

      threadStore.setState((prev) => ({
        workspaces: prev.workspaces.map((workspace) => ({
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
      }));

      void syncWorkspaceSidebar().catch((error) => {
        const message = getInvokeErrorMessage(error, t("dashboard.error.refreshThreadList"));
        setTerminalBootstrapError(message);
      });
    },
    [syncWorkspaceSidebar, t],
  );

  const handleRuntimeConsumeInitialPrompt = useCallback(
    (id: string) => {
      threadStore.setState((prev) => {
        const next = Object.fromEntries(
          Object.entries(prev.pendingRuns).filter(
            ([, pendingRun]) => pendingRun.id !== id,
          ),
        );
        if (Object.keys(next).length === Object.keys(prev.pendingRuns).length) {
          return {};
        }
        return { pendingRuns: next };
      });
    },
    [],
  );

  const handleThreadEditStart = useCallback(
    (threadId: string) => {
      setEditingThreadId(threadId);
      editingThreadIdRef.current = threadId;
    },
    [],
  );

  const handleThreadEditDone = useCallback(
    (threadId: string, newTitle: string | null, originalName: string) => {
      setEditingThreadId(null);

      if (!newTitle || newTitle === originalName) {
        editingThreadIdRef.current = null;
        return;
      }

      threadStore.setState((prev) => ({
        workspaces: prev.workspaces.map((workspace) => ({
          ...workspace,
          threads: workspace.threads.map((thread) =>
            thread.id === threadId
              ? { ...thread, name: newTitle }
              : thread,
          ),
        })),
      }));

      if (isTauri()) {
        void threadUpdateTitle(threadId, newTitle)
          .catch((error) => {
            console.warn("[thread] failed to update title:", error);
            // Rollback: restore the original name on failure.
            threadStore.setState((prev) => ({
              workspaces: prev.workspaces.map((workspace) => ({
                ...workspace,
                threads: workspace.threads.map((thread) =>
                  thread.id === threadId
                    ? { ...thread, name: originalName }
                    : thread,
                ),
              })),
            }));
          })
          .finally(() => {
            editingThreadIdRef.current = null;
          });
      } else {
        editingThreadIdRef.current = null;
      }
    },
    [],
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

          threadStore.setState((prev) => {
            const next = prev.workspaces.map((workspace) => ({
              ...workspace,
              threads: workspace.threads.filter(
                (thread) => thread.id !== threadId,
              ),
            }));

            return { workspaces: isDeletingActiveThread ? clearActiveThreads(next) : next };
          });
          threadStore.setState((prev) => {
            if (!(threadId in prev.pendingRuns)) {
              return {};
            }

            const next = { ...prev.pendingRuns };
            delete next[threadId];
            return { pendingRuns: next };
          });
          uiLayoutStore.setState((prev) => {
            if (!(threadId in prev.terminalCollapsedByThreadKey)) {
              return {};
            }
            const next = { ...prev.terminalCollapsedByThreadKey };
            delete next[threadId];
            return { terminalCollapsedByThreadKey: next };
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
            setActiveThreadProfileIdOverride(null);
            threadStore.setState({ isNewThreadMode: true });
            setComposerError(null);
          }

          if (isTauri()) {
            void syncWorkspaceSidebar().catch((error) => {
              const message = getInvokeErrorMessage(error, t("dashboard.error.refreshThreadList"));
              setTerminalBootstrapError(message);
            });
          }
        } catch (error) {
          const message = getInvokeErrorMessage(error, t("dashboard.error.deleteThread"));
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

    if (!effectivePrompt.trim()) {
      return;
    }

    if (isNewThreadMode) {
      if (commandBehavior === "clear" || commandBehavior === "compact") {
        clearNewThreadComposer();
        return;
      }

      void (async () => {
        let nextProject = selectedProject;
        let nextWorkspaceId = resolvedWorkspaceId;

        if (isTauri() && !nextWorkspaceId) {
          try {
            const projectToEnsure = nextProject;
            const ensuredWorkspace = projectToEnsure
              ? await workspaceList().then((workspaceEntries) => {
                  const existingWorkspace = findWorkspaceByPath(
                    workspaceEntries,
                    projectToEnsure.path,
                  );

                  return (
                    existingWorkspace ??
                    workspaceAdd(projectToEnsure.path, projectToEnsure.name)
                  );
                })
              : await workspaceEnsureDefault();
            const ensuredProject =
              buildProjectOptionFromWorkspace(ensuredWorkspace, language) ?? {
                id: ensuredWorkspace.id,
                name: ensuredWorkspace.name,
                path: ensuredWorkspace.canonicalPath || ensuredWorkspace.path,
                lastOpenedLabel: t("time.justNow"),
              };

            nextProject = ensuredProject;
            nextWorkspaceId = ensuredWorkspace.id;

            setSelectedProject(ensuredProject);
            setRecentProjects((current) =>
              mergeRecentProjects(current, {
                ...ensuredProject,
                lastOpenedLabel: t("time.justNow"),
              }),
            );
            setTerminalWorkspaceBindings((current) => ({
              ...current,
              ...buildWorkspaceBindingsForEntry(
                ensuredWorkspace,
                projectToEnsure?.path,
              ),
            }));
          } catch (error) {
            const message = getInvokeErrorMessage(
              error,
              t("dashboard.error.workspaceInit"),
            );
            setComposerError(message);
            return;
          }
        }

        if (!nextProject) {
          return;
        }

        deleteRemovedWorkspacePath(removedWorkspacePathsRef.current, nextProject.path);
        const project = {
          ...nextProject,
          lastOpenedLabel: t("time.justNow"),
        };
        // Two-pass lookup: prefer an exact ID match so a worktree is never
        // shadowed by its parent repo when they share the same name.
        const existingWorkspace =
          workspaces.find(
            (workspace) =>
              workspace.id === nextWorkspaceId
              || workspace.id === project.id,
          )
          ?? workspaces.find(
            (workspace) =>
              workspace.name === project.name
              || isSameWorkspacePath(workspace.path, project.path),
          )
          ?? null;
        const nextPendingRunId =
          typeof crypto !== "undefined" && "randomUUID" in crypto
            ? crypto.randomUUID()
            : `${Date.now()}`;

        let persistedThreadId =
          nextWorkspaceId === null
            ? null
            : (terminalThreadBindings[
                getNewThreadTerminalBindingKey(nextWorkspaceId)
              ] ?? null);
        // Intentional: new-thread mode always uses the global active profile as the
        // default for the new conversation; switching to an existing thread reads
        // its own persisted profile_id via resolveThreadProfileId instead.
        let persistedThreadProfileId = activeAgentProfileId;
        const nextThreadName = buildThreadTitle(trimmedValue || effectivePrompt);

        try {
          if (isTauri() && nextWorkspaceId) {
            if (!persistedThreadId) {
              persistedThreadId = await getOrCreateNewThreadId(nextWorkspaceId);
            }
          }
        } catch (error) {
          const message = getInvokeErrorMessage(error, t("dashboard.error.createThread"));
          setComposerError(message);
          return;
        }

        const nextThread = {
          id: persistedThreadId ?? `${project.id}-thread-${Date.now()}`,
          profileId: persistedThreadProfileId,
          name: nextThreadName,
          time: t("time.justNow"),
          active: true,
          status: "running" as const,
        };

        setSelectedProject(project);
        setRecentProjects((current) => mergeRecentProjects(current, project));
        threadStore.setState((prev) => {
          const cleared = clearActiveThreads(prev.workspaces);

          if (existingWorkspace) {
            return { workspaces: cleared.map((workspace) =>
              workspace.id === existingWorkspace.id
                ? {
                    ...workspace,
                    name: project.name,
                    path: project.path,
                    threads: [nextThread, ...workspace.threads],
                  }
                : workspace,
            ) };
          }

          return {
            workspaces: [
              {
                id: nextWorkspaceId ?? project.id,
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
        setStoreOpenWorkspace(
          existingWorkspace?.id ?? nextWorkspaceId ?? project.id,
          true,
        );

        if (activeTerminalStateKey) {
          uiLayoutStore.setState((prev) => {
            if (!(activeTerminalStateKey in prev.terminalCollapsedByThreadKey)) {
              return {};
            }

            const next = {
              ...prev.terminalCollapsedByThreadKey,
              [nextThread.id]: prev.terminalCollapsedByThreadKey[activeTerminalStateKey],
            };
            delete next[activeTerminalStateKey];
            return { terminalCollapsedByThreadKey: next };
          });
        }

        if (persistedThreadId) {
          setTerminalThreadBindings((current) => {
            if (!nextWorkspaceId || !persistedThreadId) {
              return current;
            }

            const bindingKey = getNewThreadTerminalBindingKey(nextWorkspaceId);

            if (current[bindingKey] === persistedThreadId) {
              return current;
            }

            return {
              ...current,
              [bindingKey]: persistedThreadId,
            };
          });

          threadStore.setState((prev) => ({
            pendingRuns: {
              ...prev.pendingRuns,
              [persistedThreadId]: {
              id: nextPendingRunId,
              displayText: submission.displayText,
              effectivePrompt,
              attachments: submission.attachments,
              metadata: submission.metadata ?? null,
              runMode: submission.runMode ?? newThreadRunMode,
              threadId: persistedThreadId,
            },
          },
          }));
        }

        // Bind the current profile to the newly created thread.
        if (persistedThreadId) {
          threadStore.setState((prev) => ({
            workspaces: prev.workspaces.map((workspace) => ({
              ...workspace,
              threads: workspace.threads.map((thread) =>
                thread.id === persistedThreadId
                  ? {
                      ...thread,
                      profileId: activeAgentProfileId,
                    }
                  : thread,
              ),
            })),
          }));
          setActiveThreadProfileIdOverride(activeAgentProfileId);
        }

        threadStore.setState({ isNewThreadMode: false });
        setNewThreadRunMode("default");
        setNewThreadValue("");
        setComposerError(null);
        if (nextWorkspaceId) {
          const bindingKey = getNewThreadTerminalBindingKey(nextWorkspaceId);
          setTerminalThreadBindings((current) => {
            if (!(bindingKey in current)) {
              return current;
            }

            const next = { ...current };
            delete next[bindingKey];
            return next;
          });
        }
      })();
      return;
    }

    setNewThreadValue("");
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

  // Wrapper: when user switches profile inside an active thread, keep the
  // change scoped to that thread. New-thread mode still updates the global
  // default profile because it defines the profile new conversations inherit.
  const handleSelectAgentProfileForThread = useCallback(
    async (profileId: string) => {
      if (isNewThreadMode || !activeThread?.id) {
        setActiveAgentProfile(profileId);
        return;
      }

      try {
        await threadUpdateProfile(activeThread.id, profileId);
      } catch (error) {
        const message = getInvokeErrorMessage(error, t("dashboard.error.switchProfile"));
        setComposerError(message);
        return;
      }

      threadStore.setState((prev) => ({
        workspaces: prev.workspaces.map((workspace) => ({
          ...workspace,
          threads: workspace.threads.map((thread) =>
            thread.id === activeThread.id
              ? {
                  ...thread,
                  profileId,
                }
              : thread,
          ),
        })),
      }));
      setActiveThreadProfileIdOverride(profileId);
    },
    [activeThread?.id, isNewThreadMode, setActiveAgentProfile],
  );

  const handleOpenSettings = (category: SettingsCategory = "general") => {
    openOverlay("settings", category);
  };

  const handleOpenMarketplace = () => {
    openOverlay("marketplace");
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

        const nextProject = buildProjectOptionFromPath(selectedPath, language);

        if (!nextProject) {
          return;
        }

        deleteRemovedWorkspacePath(removedWorkspacePathsRef.current, nextProject.path);
        const workspaceEntries = await workspaceList();
        const existingWorkspace = findWorkspaceByPath(
          workspaceEntries,
          selectedPath,
        );
        const workspace =
          existingWorkspace ??
          (await workspaceAdd(selectedPath, nextProject.name));
        const workspaceProject =
          buildProjectOptionFromWorkspace(workspace, language) ?? {
            ...nextProject,
            id: workspace.id,
            name: workspace.name,
            path: workspace.canonicalPath || workspace.path,
          };

        activateWorkspaceAsNewThreadTarget(workspace.id, workspaceProject);
        await syncWorkspaceSidebar();
      } catch (error) {
        const message = getInvokeErrorMessage(error, "Failed to add workspace");
        setTerminalBootstrapError(message);
      } finally {
        setAddingWorkspace(false);
      }
    })();
  }, [
    activateWorkspaceAsNewThreadTarget,
    isAddingWorkspace,
    language,
    syncWorkspaceSidebar,
  ]);

  const handleWorkspaceMenuToggle = (workspaceId: string) => {
    setActiveWorkspaceMenuId(
      activeWorkspaceMenuId === workspaceId ? null : workspaceId,
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

      if (workspace.kind === "worktree") {
        if (
          typeof window !== "undefined" &&
          !window.confirm(t("worktree.removeConfirm"))
        ) {
          return;
        }
      }

      void (async () => {
        const workspaceThreadIds = new Set(
          workspace.threads.map((thread) => thread.id),
        );
        const nextThreadBindingKey = getNewThreadTerminalBindingKey(
          workspace.id,
        );
        const isRemovingSelectedProject =
          selectedProject?.id === workspace.id
          || isSameWorkspacePath(selectedProject?.path, workspace.path);
        const fallbackSelectedProject = isRemovingSelectedProject
          ? (recentProjects.find(
              (project) =>
                project.id !== workspace.id
                && !isSameWorkspacePath(project.path, workspace.path),
            ) ?? null)
          : selectedProject;
        newThreadCreationRef.current = Object.fromEntries(
          Object.entries(newThreadCreationRef.current).filter(
            ([candidateWorkspaceId]) => candidateWorkspaceId !== workspace.id,
          ),
        );
        const isRemovingActiveWorkspace =
          activeThreadWorkspace?.id === workspace.id;
        const shouldPreserveSelectedProject =
          selectedProject?.id !== workspace.id
          && !isSameWorkspacePath(selectedProject?.path, workspace.path);

        setWorkspaceAction({
          workspaceId: workspace.id,
          kind: "remove",
        });
        setTerminalBootstrapError(null);

        try {
          if (workspace.path) {
            addRemovedWorkspacePath(removedWorkspacePathsRef.current, workspace.path);
          }
          await workspaceRemove(workspace.id, true);

          if (isRemovingActiveWorkspace) {
            threadStore.setState({ isNewThreadMode: true });
            threadStore.setState((prev) => ({ workspaces: clearActiveThreads(prev.workspaces) }));
            setComposerError(null);
            setSelectedDiffSelection(null);
          }

          if (isRemovingSelectedProject) {
            if (fallbackSelectedProject?.path) {
              deleteRemovedWorkspacePath(
                removedWorkspacePathsRef.current,
                fallbackSelectedProject.path,
              );
            }
            setSelectedProject(fallbackSelectedProject);
          }
          setRecentProjects((current) =>
            current.filter(
              (project) =>
                project.id !== workspace.id
                && !isSameWorkspacePath(project.path, workspace.path),
            ),
          );

          setPendingDeleteThreadId((current) =>
            current && workspaceThreadIds.has(current) ? null : current,
          );
          threadStore.setState((prev) => ({
            pendingRuns: Object.fromEntries(
              Object.entries(prev.pendingRuns).filter(
                ([threadId]) => !workspaceThreadIds.has(threadId),
              ),
            ),
          }));
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
          removeTerminalCollapsedForThreads(workspaceThreadIds);
          threadStore.setState((prev) => {
            if (!(workspace.id in prev.openWorkspaces)) {
              return {};
            }

            const next = { ...prev.openWorkspaces };
            delete next[workspace.id];
            return { openWorkspaces: next };
          });
          setActiveWorkspaceMenuId(null);

          await syncWorkspaceSidebar({
            preserveSelectedProjectIfMissing: shouldPreserveSelectedProject,
          });
        } catch (error) {
          if (workspace.path) {
            deleteRemovedWorkspacePath(removedWorkspacePathsRef.current, workspace.path);
          }
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
      recentProjects,
      selectedProject?.id,
      selectedProject?.path,
      syncWorkspaceSidebar,
      t,
      workspaceAction,
    ],
  );


  const handleCheckUpdates = appUpdater.checkForUpdates;

  const workspaceOpenLabel = t("sidebar.openInFileManager");
  const canOpenWorkspaceInSystem = isTauri() && (isMacOS || isWindows);
  const selectedThemeOption =
    THEME_OPTIONS.find((option) => option.value === theme) ?? THEME_OPTIONS[0];
  const selectedThemeSummary = t(selectedThemeOption.labelKey);
  const selectedLanguageOption =
    LANGUAGE_OPTIONS.find((option) => option.value === language) ??
    LANGUAGE_OPTIONS[1];
  const newThreadTerminalIdleMessage = !selectedProject
    ? t("dashboard.terminalDisabledHint")
    : !resolvedTerminalThreadId && !terminalBootstrapError
      ? "Preparing terminal…"
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
      handleOpenSettings("general");
    };

    window.addEventListener("keydown", handleKeyDown);
    return () => window.removeEventListener("keydown", handleKeyDown);
  }, [isMacOS]);

  return (
    <main className="h-screen overflow-hidden select-none bg-app-canvas text-app-foreground">
      <WorkbenchTopBar
        isMacOS={isMacOS}
        isWindows={isWindows}
        isTerminalCollapsed={isTerminalCollapsed}
        isCheckingUpdates={isCheckingUpdates}
        updateStatus={updateStatus}
        userMenuRef={userMenuRef}
        selectedLanguageLabel={selectedLanguageOption.label}
        selectedThemeSummary={selectedThemeSummary}
        language={language}
        theme={theme}
        onCheckUpdates={handleCheckUpdates}
        onOpenSettings={() => handleOpenSettings("general")}
        onSelectLanguage={handleLanguageSelect}
        onSelectTheme={handleThemeSelect}
        onToggleTerminal={() => {
          if (activeTerminalStateKey) {
            const cur = terminalCollapsedByThreadKey[activeTerminalStateKey] ?? DEFAULT_TERMINAL_COLLAPSED;
            setTerminalCollapsed(activeTerminalStateKey, !cur);
          }
        }}
      />

      <div className="flex h-full min-h-0 pt-9">
        <DashboardSidebar
          isSidebarOpen={isSidebarOpen}
          isMarketplaceOpen={isMarketplaceOpen}
          handleEnterNewThreadMode={handleEnterNewThreadMode}
          handleOpenMarketplace={handleOpenMarketplace}
          t={t}
          handleChooseWorkspaceFolder={handleChooseWorkspaceFolder}
          isAddingWorkspace={isAddingWorkspace}
          activeWorkspaceMenuId={activeWorkspaceMenuId}
          workspaceAction={workspaceAction}
          workspaceMenuRef={workspaceMenuRef}
          handleWorkspaceToggle={handleWorkspaceToggle}
          handleWorkspaceMenuToggle={handleWorkspaceMenuToggle}
          handleNewThreadForWorkspace={handleNewThreadForWorkspace}
          setActiveWorkspaceMenuId={setActiveWorkspaceMenuId}
          setWorktreeDialogContext={setWorktreeDialogContext}
          handleOpenWorkspaceInSystem={handleOpenWorkspaceInSystem}
          canOpenWorkspaceInSystem={canOpenWorkspaceInSystem}
          workspaceOpenLabel={workspaceOpenLabel}
          handleWorkspaceRemove={handleWorkspaceRemove}
          pendingDeleteThreadId={pendingDeleteThreadId}
          deletingThreadId={deletingThreadId}
          editingThreadId={editingThreadId}
          commitMessageModelPlan={commitMessageModelPlan}
          handleThreadEditDone={handleThreadEditDone}
          handleThreadSelect={handleThreadSelect}
          handleThreadEditStart={handleThreadEditStart}
          handleThreadDeleteConfirm={handleThreadDeleteConfirm}
          handleThreadDeleteRequest={handleThreadDeleteRequest}
          handleWorkspaceShowMore={handleWorkspaceShowMore}
        />

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
                        <div className="relative z-10 flex min-h-0 flex-1 items-center justify-center px-6 pb-8 pt-6">
                          <NewThreadEmptyState
                            recentProjects={recentProjects}
                            selectedProject={selectedProject}
                            isOverlayOpen={isOverlayOpen}
                            isLoading={!isSidebarReady}
                            onSelectProject={handleProjectSelect}
                            onRequestNewWorktree={(project) => {
                              setWorktreeDialogContext({
                                repo: {
                                  id: project.id,
                                  name: project.name,
                                  canonicalPath: project.path,
                                },
                              });
                            }}
                            branchSlot={
                              resolvedWorkspaceId &&
                              topBarGitSnapshot?.capabilities.repoAvailable &&
                              topBarGitSnapshot?.capabilities.gitCliAvailable ? (
                                <BranchSelector
                                  workspaceId={resolvedWorkspaceId}
                                  snapshot={branchSnapshot}
                                  modelPlan={commitMessageModelPlan}
                                  readOnly={currentProject?.kind === "worktree"}
                                />
                              ) : null
                            }
                          />
                        </div>

                        <div className="shrink-0 px-6 pb-6 pt-4">
                          <WorkbenchPromptComposer
                            activeAgentProfileId={workbenchActiveProfileId}
                            agentProfiles={agentProfiles}
                            canSubmitWhenAttachmentsOnly={false}
                            commands={composerCommands}
                            enabledSkills={enabledSkillEntries}
                            error={composerError}
                            onErrorMessageChange={setComposerError}
                            onRunModeChange={setNewThreadRunMode}
                            onOpenProfileSettings={() => handleOpenSettings("general")}
                            onSelectAgentProfile={handleSelectAgentProfileForThread}
                            onStop={() => undefined}
                            onSubmit={handleComposerSubmit}
                            placeholder={t("composer.placeholder")}
                            providers={providers}
                            runMode={newThreadRunMode}
                            showRunModeToggle
                            status="ready"
                            value={composerValue}
                            workspaceId={selectedProjectWorkspaceId}
                            onValueChange={setNewThreadValue}
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
                              {activeThread?.name ?? t("dashboard.newThread")}
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
                          <BranchSelector
                            workspaceId={resolvedWorkspaceId}
                            snapshot={branchSnapshot}
                            modelPlan={commitMessageModelPlan}
                            readOnly={currentProject?.kind === "worktree"}
                          />
                        </div>
                      </div>

                      <RuntimeThreadSurface
                        activeAgentProfileId={workbenchActiveProfileId}
                        agentProfiles={agentProfiles}
                        commands={composerCommands}
                        enabledSkills={enabledSkillEntries}
                        initialPromptRequest={
                          resolvedTerminalThreadId
                            ? (pendingThreadRuns[resolvedTerminalThreadId] ?? null)
                            : null
                        }
                        onConsumeInitialPrompt={handleRuntimeConsumeInitialPrompt}
                        onContextUsageChange={setRuntimeContextUsage}
                        onOpenProfileSettings={() => handleOpenSettings("general")}
                        onSelectAgentProfile={handleSelectAgentProfileForThread}
                        onThreadTitleChange={handleRuntimeThreadTitleChange}
                        providers={providers}
                        threadId={resolvedTerminalThreadId}
                        threadTitle={
                          activeThread?.name ?? t("dashboard.newThread")
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
                          label: t("dashboard.fileTree"),
                          title: t("dashboard.fileTreePanel"),
                          content: <FolderOpen className="size-4" />,
                        },
                        {
                          value: "git",
                          label: t("dashboard.sourceControl"),
                          title: t("dashboard.sourceControlPanel"),
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
                        isAutoRefreshActive={isProjectPanelAutoRefreshActive}
                      />
                    ) : (
                      <GitPanel
                        workspaceId={resolvedWorkspaceId}
                        currentProject={currentProject}
                        workspaceBootstrapError={terminalBootstrapError}
                        isAutoRefreshActive={isGitPanelAutoRefreshActive}
                        layoutResizeSignal={
                          isTerminalCollapsed ? 0 : terminalHeight
                        }
                        commitMessageLanguage={
                          workbenchActiveAgentProfile?.commitMessageLanguage ?? "English"
                        }
                        commitMessagePrompt={
                          workbenchActiveAgentProfile?.commitMessagePrompt ?? ""
                        }
                        commitMessageModelPlan={commitMessageModelPlan}
                        onOpenDiffPreview={setSelectedDiffSelection}
                      />
                    )}
                  </div>
                </div>
              </aside>
            </div>

            <DashboardTerminalOrchestrator
              active={!isTerminalCollapsed}
              bootstrapError={terminalBootstrapError}
              height={terminalHeight}
              idleMessage={newThreadTerminalIdleMessage}
              isPendingThread={isNewThreadMode}
              onCollapse={() => {
                if (activeTerminalStateKey) {
                  setTerminalCollapsed(activeTerminalStateKey, true);
                }
              }}
              onResizeStart={handleTerminalResizeStart}
              terminal={terminal}
              threadId={resolvedTerminalThreadId}
              threadTitle={activeThread?.name ?? t("dashboard.newThread")}
            />
          </div>
        </section>
      </div>

      <DashboardOverlays
        resolvedWorkspaceId={resolvedWorkspaceId}
        agentProfiles={agentProfiles}
        activeAgentProfileId={activeAgentProfileId}
        overlayContentRef={overlayContentRef}
        configDiagnostics={configDiagnostics}
        generalPreferences={generalPreferences}
        isCheckingUpdates={isCheckingUpdates}
        language={language}
        policy={policy}
        terminal={terminal}
        availableShells={availableShells}
        commands={commands}
        providerCatalog={providerCatalog}
        providers={providers}
        data={data}
        theme={theme}
        updateStatus={updateStatus}
        settingsWorkspaces={settingsWorkspaces}
        addAgentProfile={addAgentProfile}
        addAllowEntry={addAllowEntry}
        addCommand={addCommand}
        addDenyEntry={addDenyEntry}
        addProvider={addProvider}
        addWorkspace={addWorkspace}
        addWritableRoot={addWritableRoot}
        handleCheckUpdates={handleCheckUpdates}
        duplicateAgentProfile={duplicateAgentProfile}
        removeAgentProfile={removeAgentProfile}
        removeAllowEntry={removeAllowEntry}
        removeCommand={removeCommand}
        removeDenyEntry={removeDenyEntry}
        removeProvider={removeProvider}
        removeWorkspace={removeWorkspace}
        removeWritableRoot={removeWritableRoot}
        handleLanguageSelect={handleLanguageSelect}
        handleThemeSelect={handleThemeSelect}
        setActiveAgentProfile={setActiveAgentProfile}
        setDefaultWorkspace={setDefaultWorkspace}
        updateAgentProfile={updateAgentProfile}
        updateAllowEntry={updateAllowEntry}
        updateCommand={updateCommand}
        updateDenyEntry={updateDenyEntry}
        updateGeneralPreference={updateGeneralPreference}
        updatePolicySetting={updatePolicySetting}
        updateProvider={updateProvider}
        updateTerminalSetting={updateTerminalSetting}
        fetchProviderModels={fetchProviderModels}
        testProviderModelConnection={testProviderModelConnection}
        updateWritableRoot={updateWritableRoot}
        extensionDetailById={extensionDetailById}
        extensionsError={extensionsError}
        extensions={extensions}
        areExtensionsLoading={areExtensionsLoading}
        marketplaceItems={marketplaceItems}
        marketplaceSources={marketplaceSources}
        mcpServers={mcpServers}
        refreshExtensions={refreshExtensions}
        currentExtensionScope={currentExtensionScope}
        loadExtensionDetail={loadExtensionDetail}
        resolveItemScope={resolveItemScope}
        loadSkillPreview={loadSkillPreview}
        enableExtension={enableExtension}
        disableExtension={disableExtension}
        uninstallExtension={uninstallExtension}
        addMarketplaceSource={addMarketplaceSource}
        getMarketplaceSourceRemovePlan={getMarketplaceSourceRemovePlan}
        removeMarketplaceSource={removeMarketplaceSource}
        refreshMarketplaceSource={refreshMarketplaceSource}
        installMarketplaceItem={installMarketplaceItem}
        addMcpServer={addMcpServer}
        updateMcpServer={updateMcpServer}
        removeMcpServer={removeMcpServer}
        restartMcpServer={restartMcpServer}
        rescanSkills={rescanSkills}
        enableSkill={enableSkill}
        disableSkill={disableSkill}
        skillPreviewById={skillPreviewById}
        extensionSkills={extensionSkills}
        appUpdater={appUpdater}
        settingsHydrated={settingsHydrated}
        setLanguage={setLanguage}
        setTheme={setTheme}
        worktreeDialogContext={worktreeDialogContext}
        setWorktreeDialogContext={setWorktreeDialogContext}
        buildProjectOptionFromWorkspace={buildProjectOptionFromWorkspace}
        activateWorkspaceAsNewThreadTarget={activateWorkspaceAsNewThreadTarget}
        syncWorkspaceSidebar={syncWorkspaceSidebar}
        t={t}
      />
    </main>
  );
}
