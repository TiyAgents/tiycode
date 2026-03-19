import { useEffect, useRef, useState, type MouseEvent as ReactMouseEvent } from "react";
import { isTauri } from "@tauri-apps/api/core";
import {
  Boxes,
  ChevronDown,
  Folder,
  FolderOpen,
  FolderPlus,
  GitBranch,
  MessageSquarePlus,
  MoreHorizontal,
} from "lucide-react";
import type { PromptInputMessage } from "@/components/ai-elements/prompt-input";
import { useLanguage, type LanguagePreference } from "@/app/providers/language-provider";
import { useTheme, type ThemePreference } from "@/app/providers/theme-provider";
import { useMarketplaceController } from "@/modules/marketplace-center/model/use-marketplace-controller";
import { MarketplaceOverlay } from "@/modules/marketplace-center/ui/marketplace-overlay";
import { useSettingsController, type SettingsCategory } from "@/modules/settings-center/model/use-settings-controller";
import { AI_ELEMENTS_THREAD_TITLE } from "@/modules/workbench-shell/model/ai-elements-task-demo";
import { SettingsCenterOverlay } from "@/modules/settings-center/ui/settings-center-overlay";
import { ThreadTerminalPanel } from "@/features/terminal/ui/thread-terminal-panel";
import {
  threadCreate,
  threadList,
  threadUpdateTitle,
  workspaceAdd,
  workspaceList,
  workspaceSetDefault,
} from "@/services/bridge";
import {
  CONTEXT_WINDOW_INFO,
  CONTEXT_WINDOW_USAGE_DETAIL,
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
  WorkbenchOverlay,
  WorkspaceItem,
} from "@/modules/workbench-shell/model/types";
import { AiElementsTaskDemo } from "@/modules/workbench-shell/ui/ai-elements-task-demo";
import { NewThreadEmptyState } from "@/modules/workbench-shell/ui/new-thread-empty-state";
import { ProjectPanel } from "@/modules/workbench-shell/ui/project-panel";
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
import { WorkbenchSegmentedControl } from "@/shared/ui/workbench-segmented-control";

const NEW_THREAD_TERMINAL_KEY_SUFFIX = "__new_thread__";

function getNewThreadTerminalBindingKey(workspaceId: string) {
  return `${workspaceId}:${NEW_THREAD_TERMINAL_KEY_SUFFIX}`;
}

function findWorkspaceForThread(
  workspaces: ReadonlyArray<WorkspaceItem>,
  threadId: string | null,
) {
  if (!threadId) {
    return null;
  }

  return workspaces.find((workspace) => workspace.threads.some((thread) => thread.id === threadId)) ?? null;
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
    updateWorkspace,
    setDefaultWorkspace,
    addProvider,
    removeProvider,
    updateProvider,
    fetchProviderModels,
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
  const [workspaces, setWorkspaces] = useState<Array<WorkspaceItem>>(() => buildInitialWorkspaces());
  const [recentProjects, setRecentProjects] = useState<Array<ProjectOption>>(() => (isTauri() ? [] : [...RECENT_PROJECTS]));
  const [selectedProject, setSelectedProject] = useState<ProjectOption | null>(() => (isTauri() ? null : RECENT_PROJECTS[0] ?? null));
  const [isNewThreadMode, setNewThreadMode] = useState(true);
  const [activeOverlay, setActiveOverlay] = useState<WorkbenchOverlay>(null);
  const [activeSettingsCategory, setActiveSettingsCategory] = useState<SettingsCategory>("account");
  const [panelVisibilityState, setPanelVisibilityState] = useState<PanelVisibilityState>(() => readPanelVisibilityState());
  const [terminalHeight, setTerminalHeight] = useState(DEFAULT_TERMINAL_HEIGHT);
  const [terminalResize, setTerminalResize] = useState<{ startY: number; startHeight: number } | null>(null);
  const [terminalThreadBindings, setTerminalThreadBindings] = useState<Record<string, string>>({});
  const [composerValue, setComposerValue] = useState("");
  const [composerError, setComposerError] = useState<string | null>(null);
  const [openSettingsSection, setOpenSettingsSection] = useState<"theme" | "language" | null>(null);
  const [isUserMenuOpen, setUserMenuOpen] = useState(false);
  const [userSession, setUserSession] = useState(() => readStoredUserSession());
  const [isCheckingUpdates, setCheckingUpdates] = useState(false);
  const [updateStatus, setUpdateStatus] = useState<string | null>(null);
  const [terminalBootstrapError, setTerminalBootstrapError] = useState<string | null>(null);
  const [terminalWorkspaceBindings, setTerminalWorkspaceBindings] = useState<Record<string, string>>({});
  const [defaultWorkspaceId, setDefaultWorkspaceId] = useState<string | null>(null);
  const [openWorkspaces, setOpenWorkspaces] = useState<Record<string, boolean>>(
    () => Object.fromEntries(WORKSPACE_ITEMS.map((workspace) => [workspace.id, workspace.defaultOpen])),
  );
  const [activeDrawerPanel, setActiveDrawerPanel] = useState<DrawerPanel>("project");
  const [selectedDiffSelection, setSelectedDiffSelection] = useState<GitDiffSelection | null>(null);
  const mainContentRef = useRef<HTMLElement | null>(null);
  const overlayContentRef = useRef<HTMLDivElement | null>(null);
  const userMenuRef = useRef<HTMLDivElement | null>(null);

  const activeThread = getActiveThread(workspaces);
  const selectedProjectWorkspaceId =
    selectedProject === null ? null : terminalWorkspaceBindings[selectedProject.path] ?? null;
  const activeThreadWorkspace = findWorkspaceForThread(workspaces, activeThread?.id ?? null);
  const activeThreadProject = resolveProjectForWorkspace(activeThreadWorkspace, recentProjects);
  const currentProject = isNewThreadMode ? selectedProject : activeThreadProject;
  const resolvedWorkspaceId =
    currentProject === null ? null : terminalWorkspaceBindings[currentProject.path] ?? null;
  const newThreadTerminalBindingKey =
    selectedProjectWorkspaceId === null ? null : getNewThreadTerminalBindingKey(selectedProjectWorkspaceId);
  const terminalBindingKey =
    !isNewThreadMode && resolvedWorkspaceId && activeThread ? `${resolvedWorkspaceId}:${activeThread.name}` : null;
  const resolvedTerminalThreadId = isNewThreadMode
    ? (newThreadTerminalBindingKey === null ? null : terminalThreadBindings[newThreadTerminalBindingKey] ?? null)
    : terminalBindingKey === null
      ? null
      : terminalThreadBindings[terminalBindingKey] ?? null;
  const { isSidebarOpen, isDrawerOpen, isTerminalCollapsed } = panelVisibilityState;
  const isSettingsOpen = activeOverlay === "settings";
  const isMarketplaceOpen = activeOverlay === "marketplace";
  const isOverlayOpen = activeOverlay !== null;

  const setSidebarOpen = (nextState: boolean | ((current: boolean) => boolean)) => {
    setPanelVisibilityState((current) => ({
      ...current,
      isSidebarOpen: typeof nextState === "function" ? nextState(current.isSidebarOpen) : nextState,
    }));
  };

  const setDrawerOpen = (nextState: boolean | ((current: boolean) => boolean)) => {
    setPanelVisibilityState((current) => ({
      ...current,
      isDrawerOpen: typeof nextState === "function" ? nextState(current.isDrawerOpen) : nextState,
    }));
  };

  const setTerminalCollapsed = (nextState: boolean | ((current: boolean) => boolean)) => {
    setPanelVisibilityState((current) => ({
      ...current,
      isTerminalCollapsed: typeof nextState === "function" ? nextState(current.isTerminalCollapsed) : nextState,
    }));
  };

  const getMaxTerminalHeight = () => {
    if (typeof window === "undefined") {
      return DEFAULT_TERMINAL_HEIGHT;
    }

    return Math.max(MIN_TERMINAL_HEIGHT, window.innerHeight - TOPBAR_HEIGHT - MIN_WORKBENCH_HEIGHT);
  };

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

    void workspaceList()
      .then((workspaceEntries) => {
        if (cancelled) {
          return;
        }

        const nextProjects = workspaceEntries
          .map((workspace) => {
            const project = buildProjectOptionFromPath(workspace.canonicalPath || workspace.path);
            if (!project) {
              return null;
            }

            return {
              ...project,
              id: workspace.id,
              name: workspace.name,
            };
          })
          .filter((project): project is ProjectOption => project !== null);
        const defaultWorkspace = workspaceEntries.find((workspace) => workspace.isDefault) ?? null;
        const defaultProject =
          defaultWorkspace === null
            ? null
            : nextProjects.find((project) => project.id === defaultWorkspace.id || project.path === defaultWorkspace.canonicalPath)
              ?? null;

        setRecentProjects(nextProjects);
        setDefaultWorkspaceId(defaultWorkspace?.id ?? null);
        setSelectedProject((current) => {
          if (current) {
            return (
              nextProjects.find((project) => project.id === current.id || project.path === current.path) ?? current
            );
          }

          return defaultProject ?? nextProjects[0] ?? null;
        });
      })
      .catch((error) => {
        if (cancelled) {
          return;
        }

        const message = error instanceof Error ? error.message : String(error);
        setTerminalBootstrapError(message);
      });

    return () => {
      cancelled = true;
    };
  }, []);

  useEffect(() => {
    if (!terminalResize || typeof window === "undefined") {
      return;
    }

    const handleMouseMove = (event: MouseEvent) => {
      const deltaY = terminalResize.startY - event.clientY;
      const nextHeight = terminalResize.startHeight + deltaY;
      const clampedHeight = Math.min(getMaxTerminalHeight(), Math.max(MIN_TERMINAL_HEIGHT, nextHeight));

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
        const existing = workspaces.find((workspace) => workspace.path === currentProject.path || workspace.canonicalPath === currentProject.path);
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
          if (current[currentProject.path]) {
            return current;
          }

          return {
            ...current,
            [currentProject.path]: workspace.id,
          };
        });
      })
      .catch((error) => {
        if (cancelled) {
          return;
        }

        const message = error instanceof Error ? error.message : String(error);
        setTerminalBootstrapError(message);
      });

    return () => {
      cancelled = true;
    };
  }, [currentProject, terminalWorkspaceBindings]);

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

        const message = error instanceof Error ? error.message : String(error);
        setTerminalBootstrapError(message);
      });

    return () => {
      cancelled = true;
    };
  }, [defaultWorkspaceId, selectedProject, selectedProjectWorkspaceId]);

  useEffect(() => {
    if (!isTauri() || isNewThreadMode || !activeThread || !resolvedWorkspaceId || !terminalBindingKey) {
      return;
    }

    if (terminalThreadBindings[terminalBindingKey]) {
      return;
    }

    let cancelled = false;
    setTerminalBootstrapError(null);

    void threadList(resolvedWorkspaceId, 100)
      .then(async (threads) => {
        const existing = threads.find((thread) => thread.title === activeThread.name);
        if (existing) {
          return existing;
        }

        return threadCreate(resolvedWorkspaceId, activeThread.name);
      })
      .then((thread) => {
        if (cancelled) {
          return;
        }

        setTerminalThreadBindings((current) => {
          if (current[terminalBindingKey]) {
            return current;
          }

          return {
            ...current,
            [terminalBindingKey]: thread.id,
          };
        });
      })
      .catch((error) => {
        if (cancelled) {
          return;
        }

        const message = error instanceof Error ? error.message : String(error);
        setTerminalBootstrapError(message);
      });

    return () => {
      cancelled = true;
    };
  }, [
    activeThread,
    isNewThreadMode,
    terminalBindingKey,
    terminalThreadBindings,
    resolvedWorkspaceId,
  ]);

  useEffect(() => {
    if (!isTauri() || !isNewThreadMode || !resolvedWorkspaceId || !newThreadTerminalBindingKey) {
      return;
    }

    if (terminalThreadBindings[newThreadTerminalBindingKey]) {
      return;
    }

    let cancelled = false;
    setTerminalBootstrapError(null);

    void threadList(resolvedWorkspaceId, 100)
      .then(async (threads) => {
        const existing = threads.find((thread) => thread.title.trim().length === 0);
        if (existing) {
          return existing;
        }

        return threadCreate(resolvedWorkspaceId, "");
      })
      .then((thread) => {
        if (cancelled) {
          return;
        }

        setTerminalThreadBindings((current) => {
          if (current[newThreadTerminalBindingKey]) {
            return current;
          }

          return {
            ...current,
            [newThreadTerminalBindingKey]: thread.id,
          };
        });
      })
      .catch((error) => {
        if (cancelled) {
          return;
        }

        const message = error instanceof Error ? error.message : String(error);
        setTerminalBootstrapError(message);
      });

    return () => {
      cancelled = true;
    };
  }, [
    isNewThreadMode,
    newThreadTerminalBindingKey,
    resolvedWorkspaceId,
    terminalThreadBindings,
  ]);

  const handleTerminalResizeStart = (event: ReactMouseEvent<HTMLDivElement>) => {
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
    if (typeof window === "undefined") {
      return;
    }

    if (userSession) {
      window.localStorage.setItem("tiy-agent-auth-session", JSON.stringify(userSession));
      return;
    }

    window.localStorage.removeItem("tiy-agent-auth-session");
  }, [userSession]);

  useEffect(() => {
    if (typeof window === "undefined") {
      return;
    }

    window.localStorage.setItem(PANEL_VISIBILITY_STORAGE_KEY, JSON.stringify(panelVisibilityState));
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
    if (typeof window === "undefined") {
      return;
    }

    const handleKeyDown = (event: KeyboardEvent) => {
      if (!(event.metaKey || event.ctrlKey) || event.altKey || event.key.toLowerCase() !== "a") {
        return;
      }

      if (isEditableSelectionTarget(event.target)) {
        return;
      }

      const selection = window.getSelection();
      const selectionInsideOverlayContent =
        isNodeInsideContainer(overlayContentRef.current, selection?.anchorNode ?? null) ||
        isNodeInsideContainer(overlayContentRef.current, selection?.focusNode ?? null);
      const targetInsideOverlayContent = isNodeInsideContainer(
        overlayContentRef.current,
        event.target instanceof Node ? event.target : null,
      );
      const selectionInsideMainContent =
        isNodeInsideContainer(mainContentRef.current, selection?.anchorNode ?? null) ||
        isNodeInsideContainer(mainContentRef.current, selection?.focusNode ?? null);
      const targetInsideMainContent = isNodeInsideContainer(
        mainContentRef.current,
        event.target instanceof Node ? event.target : null,
      );

      if (overlayContentRef.current && (targetInsideOverlayContent || selectionInsideOverlayContent)) {
        event.preventDefault();
        selectContainerContents(overlayContentRef.current);
        return;
      }

      if (mainContentRef.current && (targetInsideMainContent || selectionInsideMainContent)) {
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

  const handleEnterNewThreadMode = () => {
    setNewThreadMode(true);
    setWorkspaces((current) => clearActiveThreads(current));
    setComposerError(null);
    setTerminalBootstrapError(null);
  };

  const handleThreadSelect = (threadId: string) => {
    setNewThreadMode(false);
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

  const handleComposerSubmit = (message: PromptInputMessage) => {
    const trimmedValue = message.text?.trim() ?? "";

    if (!trimmedValue) {
      return;
    }

    if (isNewThreadMode) {
      if (!selectedProject) {
        return;
      }

      const project = {
        ...selectedProject,
        lastOpenedLabel: "刚刚",
      };
      const existingWorkspace = workspaces.find(
        (workspace) =>
          workspace.id === project.id ||
          workspace.name === project.name ||
          (workspace.path && workspace.path === project.path),
      );
      const nextThread = {
        id: `${project.id}-thread-${Date.now()}`,
        name: buildThreadTitle(trimmedValue),
        time: "刚刚",
        active: true,
        status: "running" as const,
      };
      const draftThreadId =
        newThreadTerminalBindingKey === null ? null : terminalThreadBindings[newThreadTerminalBindingKey] ?? null;
      const promotedTerminalBindingKey =
        resolvedWorkspaceId === null ? null : `${resolvedWorkspaceId}:${nextThread.name}`;

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
            id: project.id,
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
        [existingWorkspace?.id ?? project.id]: true,
      }));
      if (draftThreadId && promotedTerminalBindingKey) {
        setTerminalThreadBindings((current) => {
          const next = {
            ...current,
            [promotedTerminalBindingKey]: draftThreadId,
          };

          if (newThreadTerminalBindingKey) {
            delete next[newThreadTerminalBindingKey];
          }

          return next;
        });

        void threadUpdateTitle(draftThreadId, nextThread.name).catch((error) => {
          const message = error instanceof Error ? error.message : String(error);
          setTerminalBootstrapError(message);
        });
      }
      setNewThreadMode(false);
      setComposerValue("");
      setComposerError(null);
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

  const isMacOS = data?.platform === "macos" || (typeof navigator !== "undefined" && navigator.userAgent.includes("Mac"));
  const isWindows = data?.platform === "windows" || (typeof navigator !== "undefined" && navigator.userAgent.includes("Windows"));
  const selectedThemeOption = THEME_OPTIONS.find((option) => option.value === theme) ?? THEME_OPTIONS[0];
  const selectedThemeSummary = theme === "system" ? "跟随系统" : selectedThemeOption.label;
  const selectedLanguageOption = LANGUAGE_OPTIONS.find((option) => option.value === language) ?? LANGUAGE_OPTIONS[1];
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
      if (event.defaultPrevented || !event.metaKey || event.ctrlKey || event.altKey || event.shiftKey) {
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
                    isNewThreadMode ? "text-app-foreground" : "text-app-subtle group-hover:text-app-foreground",
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
                    isMarketplaceOpen ? "text-app-foreground" : "text-app-subtle group-hover:text-app-foreground",
                  )}
                />
                <span className="truncate text-sm font-medium">Marketplace</span>
              </button>
            </div>

            <div className="mt-6 flex items-center justify-between px-3">
              <span className="text-xs uppercase tracking-[0.14em] text-app-subtle">WORKSPACE</span>
              <FolderPlus className="size-3.5 text-app-subtle" />
            </div>

            <div className="mx-1 mt-3 h-px shrink-0 bg-app-border" />

            <div className="mt-3 min-h-0 flex-1 overflow-auto overscroll-contain [scrollbar-width:none] [&::-webkit-scrollbar]:hidden">
              <div className="space-y-1.5">
                {workspaces.map((workspace) => {
                  const isOpen = openWorkspaces[workspace.id] ?? workspace.defaultOpen;
                  const FolderIcon = isOpen ? FolderOpen : Folder;

                  return (
                    <div key={workspace.id} className="space-y-1">
                      <div className="group px-1">
                        <div className="relative">
                          <button
                            type="button"
                            className={cn(
                              "flex items-center gap-2 pr-10 text-app-muted hover:bg-app-surface-hover hover:text-app-foreground",
                              DRAWER_LIST_ROW_CLASS,
                            )}
                            onClick={() => handleWorkspaceToggle(workspace.id)}
                          >
                            <FolderIcon className="size-4 shrink-0 text-app-muted" />
                            <span className={DRAWER_LIST_LABEL_CLASS}>{workspace.name}</span>
                          </button>
                          <button
                            type="button"
                            aria-label="更多操作"
                            title="更多操作"
                            className={DRAWER_OVERFLOW_ACTION_CLASS}
                          >
                            <MoreHorizontal className="size-4" />
                          </button>
                        </div>
                      </div>

                      {isOpen && workspace.threads.length > 0 ? (
                        <div className={cn(DRAWER_LIST_STACK_CLASS, "pl-2.5")}>
                          {workspace.threads.map((thread) => (
                            <div key={thread.id} className="group relative">
                              <button
                                type="button"
                                className={cn(
                                  `${DRAWER_LIST_ROW_CLASS} border pr-11`,
                                  thread.active
                                    ? "border-app-border-strong bg-app-surface-active text-app-foreground"
                                    : "border-transparent bg-transparent text-app-muted hover:bg-app-surface-hover hover:text-app-foreground",
                                )}
                                onClick={() => handleThreadSelect(thread.id)}
                              >
                                <div className="flex items-center gap-2">
                                  <ThreadStatusIndicator
                                    status={thread.status}
                                    emphasis={thread.active ? "default" : "subtle"}
                                  />
                                  <p className={DRAWER_LIST_LABEL_CLASS}>{thread.name}</p>
                                </div>
                              </button>
                              <span className="pointer-events-none absolute right-2.5 top-1/2 -translate-y-1/2 text-[11px] text-app-subtle transition-opacity duration-200 group-hover:opacity-0">
                                {thread.time}
                              </span>
                              <button
                                type="button"
                                aria-label="更多操作"
                                title="更多操作"
                                className={DRAWER_OVERFLOW_ACTION_CLASS}
                              >
                                <MoreHorizontal className="size-4" />
                              </button>
                            </div>
                          ))}
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
              <section ref={mainContentRef} className="min-h-0 min-w-0 flex-1 select-text bg-app-canvas">
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
                            error={composerError}
                            onErrorMessageChange={setComposerError}
                            onSelectAgentProfile={setActiveAgentProfile}
                            onStop={() => undefined}
                            onSubmit={handleComposerSubmit}
                            placeholder="Ask Tiy anything, @ to add files, / for commands, $ for skills"
                            providers={providers}
                            status="ready"
                            value={composerValue}
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
                            {activeThread ? <ThreadStatusIndicator status={activeThread.status} /> : null}
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
                                style={{ width: `${CONTEXT_WINDOW_INFO.usageRatio * 100}%` }}
                              />
                              <span className="relative inline-flex items-center gap-1.5 px-2 py-0.5">
                                <span className="text-app-subtle">{CONTEXT_WINDOW_INFO.label}</span>
                                <span className="font-semibold text-app-foreground">
                                  {CONTEXT_WINDOW_INFO.used} / {CONTEXT_WINDOW_INFO.total}
                                </span>
                              </span>
                            </span>
                            <div className="pointer-events-none absolute left-1/2 top-[calc(100%+0.5rem)] z-20 w-max min-w-[190px] -translate-x-1/2 translate-y-1 rounded-xl border border-app-border bg-app-menu px-3 py-2 text-center opacity-0 shadow-[0_14px_32px_rgba(15,23,42,0.14)] transition-[opacity,transform] duration-150 group-hover/context-window:translate-y-0 group-hover/context-window:opacity-100 group-focus-within/context-window:translate-y-0 group-focus-within/context-window:opacity-100 dark:shadow-[0_16px_36px_rgba(0,0,0,0.38)]">
                              <p className="whitespace-nowrap text-[11px] font-semibold text-app-foreground">
                                {CONTEXT_WINDOW_USAGE_DETAIL.usedPercent}% used
                                <span className="font-normal text-app-subtle"> ({CONTEXT_WINDOW_USAGE_DETAIL.leftPercent}% left)</span>
                              </p>
                              <p className="mt-1 whitespace-nowrap text-[11px] text-app-muted">
                                {CONTEXT_WINDOW_INFO.used} / {CONTEXT_WINDOW_INFO.total} tokens used
                              </p>
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

                      <AiElementsTaskDemo
                        activeAgentProfileId={activeAgentProfileId}
                        agentProfiles={agentProfiles}
                        onSelectAgentProfile={setActiveAgentProfile}
                        providers={providers}
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
                      onValueChange={(panel) => setActiveDrawerPanel(panel as DrawerPanel)}
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
                        layoutResizeSignal={isTerminalCollapsed ? 0 : terminalHeight}
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
                isTerminalCollapsed ? "border-t border-transparent opacity-0 pointer-events-none" : "border-t border-app-border opacity-100",
              )}
              style={{ height: isTerminalCollapsed ? 0 : terminalHeight }}
            >
              <div
                className={cn(
                  "group absolute inset-x-0 top-0 z-10 flex h-4 -translate-y-1/2 items-start justify-center transition-opacity duration-200",
                  isTerminalCollapsed ? "opacity-0" : "cursor-row-resize opacity-100",
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
          onUpdateWorkspace={updateWorkspace}
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
