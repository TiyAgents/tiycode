import {
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
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
import { useSettingsInit } from "@/modules/settings-center/model/use-settings-init";
import { settingsStore } from "@/modules/settings-center/model/settings-store";
import type { SettingsCategory } from "@/modules/settings-center/model/types";
import { setActiveAgentProfile } from "@/modules/settings-center/model/settings-ipc-actions";
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

  resolveActiveThreadWorkbenchProfileId,
} from "@/modules/workbench-shell/ui/dashboard-workbench-logic";
import { isOnboardingCompleted } from "@/modules/onboarding/model/use-onboarding";
import {
  threadUpdateProfile,
  threadUpdateTitle,
  workspaceAdd,
  workspaceList,
} from "@/services/bridge";
import {
  LANGUAGE_OPTIONS,
  RECENT_PROJECTS,
  THEME_OPTIONS,
  UPDATE_STATUS_DURATION,
  
} from "@/modules/workbench-shell/model/fixtures";
import {
  buildProjectOptionFromPath,
  
  getActiveThread,
} from "@/modules/workbench-shell/model/helpers";
import {
  addRemovedWorkspacePath,
  deleteRemovedWorkspacePath,
  findWorkspaceByPath,
  getWorkspaceBindingId,
  resolveProjectForWorkspace,
} from "@/modules/workbench-shell/model/workspace-path-bindings";
import type {
  DrawerPanel,
  ProjectOption,
  WorkspaceItem,
} from "@/modules/workbench-shell/model/types";
import type { ExtensionDetail, SkillPreview } from "@/shared/types/extensions";
import { NewThreadEmptyState } from "@/modules/workbench-shell/ui/new-thread-empty-state";
import { ProjectPanel } from "@/modules/workbench-shell/ui/project-panel";
import { BranchSelector } from "@/modules/workbench-shell/ui/branch-selector";
import {
  RuntimeThreadSurface,
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
import { waitForBackendReady } from "@/shared/lib/backend-ready";
import { WorkbenchSegmentedControl } from "@/shared/ui/workbench-segmented-control";
import {
  threadStore,
  useStore,
  updateThreadTitle as setStoreThreadTitle,
  setDisplayCount as setStoreDisplayCount,
  setLoadMorePending as setStoreLoadMorePending,
  setSidebarReady as setStoreSidebarReady,
  shallowEqual,
} from "@/modules/workbench-shell/model/thread-store";
import { createCoalescedAsyncRunner } from "@/modules/workbench-shell/model/sidebar-sync-runner";
import type { DeletePhase } from "@/modules/workbench-shell/model/delete-confirm-types";
import {
  dispatchGlobalEvent,
  dispatchRunFinishedEvent,
} from "@/modules/workbench-shell/model/run-event-dispatcher";
import {
  uiLayoutStore,
  openOverlay,
  setOpenSettingsSection,
  setActiveDrawerPanel,
  setSelectedDiffSelection,
  setTerminalCollapsed,
  setUserMenuOpen,
  setActiveWorkspaceMenuId,
  setShowOnboarding,
  setWorktreeDialogContext,
} from "@/modules/workbench-shell/model/ui-layout-store";
import {
  composerStore,
  setNewThreadValue,
  setNewThreadRunMode,
  setComposerError,
  clearNewThreadComposer,
} from "@/modules/workbench-shell/model/composer-store";
import { projectStore } from "@/modules/workbench-shell/model/project-store";
import {
  selectThread,
  selectProject as selectProjectAction,
  activateWorkspace,
  deleteThread,
  removeWorkspace,
  submitNewThread,
  enterNewThreadMode,
  performSidebarSync,
} from "@/modules/workbench-shell/model/workbench-actions";
import { useTerminalPreWarm } from "@/modules/workbench-shell/hooks/use-terminal-pre-warm";
import { useTerminalResize } from "@/modules/workbench-shell/hooks/use-terminal-resize";
import { useWorkspaceDiscovery } from "@/modules/workbench-shell/hooks/use-workspace-discovery";
import { useGitSnapshot } from "@/modules/workbench-shell/hooks/use-git-snapshot";
import { useGlobalKeyboardShortcuts } from "@/modules/workbench-shell/hooks/use-global-keyboard-shortcuts";


export function DashboardWorkbench() {
  const { data } = useSystemMetadata();
  const { theme, setTheme } = useTheme();
  const { language, setLanguage } = useLanguage();
  const t = useT();

  // ── Phase 6: projectStore subscriptions (replaces 5 useState) ──
  const selectedProject = useStore(projectStore, (s) => s.selectedProject);
  const recentProjects = useStore(projectStore, (s) => s.recentProjects, shallowEqual);
  const terminalThreadBindings = useStore(projectStore, (s) => s.terminalThreadBindings, shallowEqual);
  const terminalWorkspaceBindings = useStore(projectStore, (s) => s.terminalWorkspaceBindings, shallowEqual);

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
  // ── Phase 4: settings init (hydration + persistence + backend sync) ──
  useSettingsInit();

  // ── Settings store subscriptions (direct, replaces useSettingsController) ──
  const providers = useStore(settingsStore, (s) => s.providers, shallowEqual);
  const commandEntries = useStore(settingsStore, (s) => s.commands, shallowEqual);
  const terminal = useStore(settingsStore, (s) => s.terminal, shallowEqual);
  const agentProfiles = useStore(settingsStore, (s) => s.agentProfiles, shallowEqual);
  const activeAgentProfileId = useStore(settingsStore, (s) => s.activeAgentProfileId);

  // ── Phase 1: threadStore subscriptions (replaces 9 useState + 3 useRef) ──
  const workspaces = useStore(threadStore, (s) => s.workspaces, shallowEqual);
  const isNewThreadMode = useStore(threadStore, (s) => s.isNewThreadMode);
  const isSidebarReady = useStore(threadStore, (s) => s.sidebarReady);
  // Phase 6: threadStore additions (replaces 3 useState)
  const activeThreadProfileIdOverride = useStore(threadStore, (s) => s.activeThreadProfileIdOverride);
  const runtimeContextUsage = useStore(threadStore, (s) => s.runtimeContextUsage);
  const editingThreadId = useStore(threadStore, (s) => s.editingThreadId);

  // ── Phase 2: uiLayoutStore + composerStore subscriptions (replaces 16 useState) ──
  const activeOverlay = useStore(uiLayoutStore, (s) => s.activeOverlay);
  const panelVisibilityState = useStore(uiLayoutStore, (s) => s.panelVisibility, shallowEqual);
  const terminalCollapsedByThreadKey = useStore(uiLayoutStore, (s) => s.terminalCollapsedByThreadKey, shallowEqual);
  const terminalHeight = useStore(uiLayoutStore, (s) => s.terminalHeight);
  const isUserMenuOpen = useStore(uiLayoutStore, (s) => s.isUserMenuOpen);
  const activeWorkspaceMenuId = useStore(uiLayoutStore, (s) => s.activeWorkspaceMenuId);
  const activeDrawerPanel = useStore(uiLayoutStore, (s) => s.activeDrawerPanel);
  // Phase 6: uiLayoutStore addition (replaces 1 useState)
  const worktreeDialogContext = useStore(uiLayoutStore, (s) => s.worktreeDialogContext);

  const composerValue = useStore(composerStore, (s) => s.newThreadValue);
  const composerError = useStore(composerStore, (s) => s.error);
  const newThreadRunMode = useStore(composerStore, (s) => s.newThreadRunMode);

  // ── Init onboarding visibility (one-time, based on localStorage) ──
  useEffect(() => {
    if (!isOnboardingCompleted()) {
      setShowOnboarding(true);
    }
  }, []);

  // ── Phase 6: Initialize projectStore defaults (non-Tauri fallback) ──
  useEffect(() => {
    if (!isTauri() && projectStore.getState().recentProjects.length === 0) {
      projectStore.setState({
        recentProjects: [...RECENT_PROJECTS],
        selectedProject: RECENT_PROJECTS[0] ?? null,
      });
    }
  }, []);

  const appUpdater = useAppUpdater();
  const isCheckingUpdates = appUpdater.phase === "checking";
  const updateStatus =
    appUpdater.phase === "upToDate"
      ? t("dashboard.upToDate", { version: data?.version ?? "0.1.0" })
      : null;

  // ── Local-only state (not cross-domain) ──
  const [deletePhase, setDeletePhase] = useState<DeletePhase>({ kind: "idle" });
  const [isAddingWorkspace, setAddingWorkspace] = useState(false);
  const [workspaceAction, setWorkspaceAction] = useState<{
    workspaceId: string;
    kind: "open" | "remove";
  } | null>(null);

  // ── Store-derived state (no more compat wrappers — actions use stores directly) ──
  const terminalBootstrapError = useStore(projectStore, (s) => s.terminalBootstrapError);


  const composerCommands = useMemo(
    () => [...commandEntries, ...pluginCommandEntries],
    [commandEntries, pluginCommandEntries],
  );

  // ── DOM refs (4 UI-only refs) ──
  const mainContentRef = useRef<HTMLElement | null>(null);
  const overlayContentRef = useRef<HTMLDivElement | null>(null);
  const userMenuRef = useRef<HTMLDivElement | null>(null);
  const workspaceMenuRef = useRef<HTMLDivElement | null>(null);
  const sidebarAutoRefreshUntilRef = useRef(0);
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

  // ── Phase 6: Dedicated hooks (extract effects from component body) ──
  // NOTE: Must come after resolvedWorkspaceId derivation and before any
  // code that reads topBarGitSnapshot (branchSnapshot, etc.).
  useTerminalPreWarm();
  useTerminalResize();
  useWorkspaceDiscovery();
  const { snapshot: topBarGitSnapshot } = useGitSnapshot(resolvedWorkspaceId);
  const handleOpenSettingsShortcut = useCallback(() => {
    openOverlay("settings");
  }, []);

  useGlobalKeyboardShortcuts(
    { mainContent: mainContentRef, overlay: overlayContentRef },
    { isMacOS: (data?.platform === "macos") || false, onOpenSettings: handleOpenSettingsShortcut },
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
  // Coalesced async runner for sidebar sync. Delegates core sync logic to
  // performSidebarSync in workbench-actions.ts while keeping single-flight,
  // coalescing, and throttling.
  const sidebarSyncRunner = useMemo(
    () =>
      createCoalescedAsyncRunner({
        minGapMs: SIDEBAR_SYNC_MIN_GAP_MS,
        executeFn: async (options: {
          preserveSelectedProjectIfMissing?: boolean;
          threadDisplayCountOverrides: Record<string, number>;
        }) => {
          await performSidebarSync({
            language,
            preserveSelectedProjectIfMissing: options.preserveSelectedProjectIfMissing,
            threadDisplayCountOverrides: options.threadDisplayCountOverrides,
          });
        },
      }),
    [language],
  );

  // Public entry point. Delegates to the coalesced async runner which
  // handles single-flight, coalescing, and throttling.
  const syncWorkspaceSidebar = useCallback(
    async (
      options: {
        preserveSelectedProjectIfMissing?: boolean;
        threadDisplayCountOverrides?: Record<string, number>;
      } = {},
    ): Promise<void> => {
      return sidebarSyncRunner.request({
        preserveSelectedProjectIfMissing: options.preserveSelectedProjectIfMissing,
        threadDisplayCountOverrides: options.threadDisplayCountOverrides ?? {},
      });
    },
    [sidebarSyncRunner],
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
          dispatchGlobalEvent(threadId, "RUN_STARTED", { runId });

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
          dispatchRunFinishedEvent(threadId, runId, status);

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
          if (threadStore.getState().editingThreadId === threadId) {
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

      if (sidebarSyncRunner.isRunning()) {
        return;
      }

      void syncWorkspaceSidebar().catch((error) => {
        const message = getInvokeErrorMessage(error, t("dashboard.error.refreshThreadList"));
        projectStore.setState({ terminalBootstrapError: message });
      });
    }, SIDEBAR_AUTO_REFRESH_INTERVAL_MS);

    return () => window.clearInterval(interval);
  }, [hasSidebarLiveThreads, syncWorkspaceSidebar]);

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
        projectStore.setState({ terminalBootstrapError: message });
      });

    return () => {
      cancelled = true;
    };
  }, [syncWorkspaceSidebar]);

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
      projectStore.setState({ terminalBootstrapError: null });

      void syncWorkspaceSidebar({
        threadDisplayCountOverrides: {
          [workspaceId]: nextDisplayCount,
        },
      })
        .catch((error) => {
          const message = getInvokeErrorMessage(error, t("dashboard.error.loadMoreThreads"));
          projectStore.setState({ terminalBootstrapError: message });
        })
        .finally(() => {
          setStoreLoadMorePending(workspaceId, false);
        });
    },
    [syncWorkspaceSidebar],
  );

  const handleEnterNewThreadMode = () => {
    enterNewThreadMode();
    setDeletePhase({ kind: "idle" });
  };

  const handleThreadSelect = (threadId: string) => {
    selectThread(threadId);
    setDeletePhase({ kind: "idle" });
  };

  const handleProjectSelect = (project: ProjectOption) => {
    deleteRemovedWorkspacePath(removedWorkspacePathsRef.current, project.path);
    selectProjectAction(project);
  };

  const activateWorkspaceAsNewThreadTarget = useCallback(
    (workspaceId: string, nextProject: ProjectOption) => {
      deleteRemovedWorkspacePath(removedWorkspacePathsRef.current, nextProject.path);
      activateWorkspace(workspaceId, nextProject);
      setDeletePhase({ kind: "idle" });
    },
    [],
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



  const handleThreadEditStart = useCallback(
    (threadId: string) => {
      threadStore.setState({ editingThreadId: threadId });
    },
    [],
  );

  const handleThreadEditDone = useCallback(
    (threadId: string, newTitle: string | null, originalName: string) => {
      threadStore.setState({ editingThreadId: null });

      if (!newTitle || newTitle === originalName) {
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
            // editingThreadId already set to null via threadStore.setState({ editingThreadId: null }) above
          });
      } else {
        // Non-Tauri: editingThreadId already set to null via threadStore.setState({ editingThreadId: null })
      }
    },
    [],
  );

  const handleThreadDeleteRequest = useCallback((threadId: string) => {
    setDeletePhase({ kind: "confirming", threadId });
    projectStore.setState({ terminalBootstrapError: null });
  }, []);

  const handleThreadDeleteConfirm = useCallback(
    async (threadId: string) => {
      if (deletePhase.kind === "deleting") return;
      setDeletePhase({ kind: "deleting", threadId });
      projectStore.setState({ terminalBootstrapError: null });
      try {
        await deleteThread(threadId);
        if (isTauri()) {
          void syncWorkspaceSidebar().catch((error) => {
            const message = getInvokeErrorMessage(error, t("dashboard.error.refreshThreadList"));
            projectStore.setState({ terminalBootstrapError: message });
          });
        }
      } catch (error) {
        const message = getInvokeErrorMessage(error, t("dashboard.error.deleteThread"));
        projectStore.setState({ terminalBootstrapError: message });
      } finally {
        setDeletePhase({ kind: "idle" });
      }
    },
    [deletePhase, syncWorkspaceSidebar, t],
  );

  const handleComposerSubmit = (submission: ComposerSubmission) => {
    const trimmedValue = submission.displayText?.trim() ?? "";
    const commandBehavior = submission.command?.behavior ?? null;
    const effectivePrompt = submission.effectivePrompt;

    if (!effectivePrompt.trim()) {
      return;
    }

    if (!isNewThreadMode) {
      // Non-new-thread submissions are handled by RuntimeThreadSurface directly.
      return;
    }

    if (commandBehavior === "clear" || commandBehavior === "compact") {
      clearNewThreadComposer();
      return;
    }

    void submitNewThread({
      value: trimmedValue,
      runMode: newThreadRunMode,
      displayText: submission.displayText,
      effectivePrompt,
      attachments: submission.attachments,
      metadata: submission.metadata ?? null,
      commandBehavior: commandBehavior ?? "none",
    }).catch((error) => {
      const message = getInvokeErrorMessage(error, t("dashboard.error.createThread"));
      composerStore.setState({ error: message });
    });
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
        composerStore.setState({ error: message });
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
      threadStore.setState({ activeThreadProfileIdOverride: profileId });
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
      projectStore.setState({ terminalBootstrapError: null });

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
        projectStore.setState({ terminalBootstrapError: message });
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
    setActiveWorkspaceMenuId((current: string | null) =>
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
        projectStore.setState({ terminalBootstrapError: null });

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
          projectStore.setState({ terminalBootstrapError: message });
        } finally {
          setWorkspaceAction(null);
        }
      })();
    },
    [isWindows, workspaceAction],
  );

  const handleWorkspaceRemove = useCallback(
    async (workspace: WorkspaceItem) => {
      if (!isTauri() || workspaceAction) return;
      if (
        workspace.kind === "worktree"
        && typeof window !== "undefined"
        && !window.confirm(t("worktree.removeConfirm"))
      ) {
        return;
      }

      setWorkspaceAction({ workspaceId: workspace.id, kind: "remove" });
      projectStore.setState({ terminalBootstrapError: null });

      try {
        if (workspace.path) {
          addRemovedWorkspacePath(removedWorkspacePathsRef.current, workspace.path);
        }
        await removeWorkspace(workspace);
        setSelectedDiffSelection(null);
        setDeletePhase((current) =>
          current.kind !== "idle" && workspace.threads.some((t) => t.id === current.threadId)
            ? { kind: "idle" }
            : current,
        );
        await syncWorkspaceSidebar();
      } catch (error) {
        if (workspace.path) {
          deleteRemovedWorkspacePath(removedWorkspacePathsRef.current, workspace.path);
        }
        const message = getInvokeErrorMessage(error, `Failed to remove ${workspace.name}`);
        projectStore.setState({ terminalBootstrapError: message });
      } finally {
        setWorkspaceAction(null);
      }
    },
    [syncWorkspaceSidebar, t, workspaceAction],
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
          deletePhase={deletePhase}
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
                              uiLayoutStore.setState({ worktreeDialogContext: {
                                repo: {
                                  id: project.id,
                                  name: project.name,
                                  canonicalPath: project.path,
                                },
                              } });
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
                        commands={composerCommands}
                        enabledSkills={enabledSkillEntries}
                        threadId={resolvedTerminalThreadId}
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
              idleMessage={newThreadTerminalIdleMessage}
              isPendingThread={isNewThreadMode}
              onCollapse={() => {
                if (activeTerminalStateKey) {
                  setTerminalCollapsed(activeTerminalStateKey, true);
                }
              }}
              terminal={terminal}
              threadId={resolvedTerminalThreadId}
              threadTitle={activeThread?.name ?? t("dashboard.newThread")}
            />
          </div>
        </section>
      </div>

      <DashboardOverlays
        resolvedWorkspaceId={resolvedWorkspaceId}
        overlayContentRef={overlayContentRef}
        configDiagnostics={configDiagnostics}
        isCheckingUpdates={isCheckingUpdates}
        language={language}
        data={data}
        theme={theme}
        updateStatus={updateStatus}
        handleCheckUpdates={handleCheckUpdates}
        handleLanguageSelect={handleLanguageSelect}
        handleThemeSelect={handleThemeSelect}
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
