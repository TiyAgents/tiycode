import { useEffect, useRef, useState, type MouseEvent as ReactMouseEvent } from "react";
import {
  ArrowUp,
  Bot,
  Boxes,
  ChevronDown,
  Folder,
  FolderOpen,
  GitBranch,
  MessageSquarePlus,
  MoreHorizontal,
  PanelBottom,
  Plus,
  RefreshCw,
  Sparkles,
  TerminalSquare,
} from "lucide-react";
import { useLanguage, type LanguagePreference } from "@/app/providers/language-provider";
import { useTheme, type ThemePreference } from "@/app/providers/theme-provider";
import { useSettingsController, type SettingsCategory } from "@/modules/settings-center/model/use-settings-controller";
import { SettingsCenterOverlay } from "@/modules/settings-center/ui/settings-center-overlay";
import {
  CONTEXT_WINDOW_INFO,
  CONTEXT_WINDOW_USAGE_DETAIL,
  DEFAULT_TERMINAL_HEIGHT,
  DRAWER_LIST_LABEL_CLASS,
  DRAWER_LIST_ROW_CLASS,
  DRAWER_LIST_STACK_CLASS,
  GIT_CHANGE_FILES,
  LANGUAGE_OPTIONS,
  MESSAGE_SECTIONS,
  MIN_TERMINAL_HEIGHT,
  MIN_WORKBENCH_HEIGHT,
  MOCK_USER_SESSION,
  PANEL_VISIBILITY_STORAGE_KEY,
  RECENT_PROJECTS,
  TERMINAL_LINES,
  THEME_OPTIONS,
  TOPBAR_HEIGHT,
  UPDATE_STATUS_DURATION,
  WORKSPACE_ITEMS,
  DRAWER_OVERFLOW_ACTION_CLASS,
} from "@/modules/workbench-shell/model/fixtures";
import {
  activateThread,
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
import type { DrawerPanel, PanelVisibilityState, ProjectOption, WorkspaceItem } from "@/modules/workbench-shell/model/types";
import { InspectorItem } from "@/modules/workbench-shell/ui/inspector-item";
import { NewThreadEmptyState } from "@/modules/workbench-shell/ui/new-thread-empty-state";
import { ProjectPanel } from "@/modules/workbench-shell/ui/project-panel";
import { GitDiffPreviewPanel, GitPanel } from "@/modules/workbench-shell/ui/source-control-panels";
import { ThreadStatusIndicator } from "@/modules/workbench-shell/ui/thread-status-indicator";
import { WorkbenchTopBar } from "@/modules/workbench-shell/ui/workbench-top-bar";
import { useSystemMetadata } from "@/features/system-info/model/use-system-metadata";
import { cn } from "@/shared/lib/utils";
import { Button } from "@/shared/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/shared/ui/card";
import { WorkbenchSegmentedControl } from "@/shared/ui/workbench-segmented-control";

export function DashboardWorkbench() {
  const { data, error, isLoading, refetch } = useSystemMetadata();
  const { theme, setTheme } = useTheme();
  const { language, setLanguage } = useLanguage();
  const {
    general: generalPreferences,
    workspaces: settingsWorkspaces,
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
  const [recentProjects, setRecentProjects] = useState<Array<ProjectOption>>(() => [...RECENT_PROJECTS]);
  const [selectedProject, setSelectedProject] = useState<ProjectOption | null>(() => RECENT_PROJECTS[0] ?? null);
  const [isNewThreadMode, setNewThreadMode] = useState(true);
  const [isSettingsOpen, setSettingsOpen] = useState(false);
  const [activeSettingsCategory, setActiveSettingsCategory] = useState<SettingsCategory>("account");
  const [panelVisibilityState, setPanelVisibilityState] = useState<PanelVisibilityState>(() => readPanelVisibilityState());
  const [terminalHeight, setTerminalHeight] = useState(DEFAULT_TERMINAL_HEIGHT);
  const [terminalResize, setTerminalResize] = useState<{ startY: number; startHeight: number } | null>(null);
  const [composerValue, setComposerValue] = useState("");
  const [openSettingsSection, setOpenSettingsSection] = useState<"theme" | "language" | null>(null);
  const [isUserMenuOpen, setUserMenuOpen] = useState(false);
  const [userSession, setUserSession] = useState(() => readStoredUserSession());
  const [isCheckingUpdates, setCheckingUpdates] = useState(false);
  const [updateStatus, setUpdateStatus] = useState<string | null>(null);
  const [isComposerProfileMenuOpen, setComposerProfileMenuOpen] = useState(false);
  const [openWorkspaces, setOpenWorkspaces] = useState<Record<string, boolean>>(
    () => Object.fromEntries(WORKSPACE_ITEMS.map((workspace) => [workspace.id, workspace.defaultOpen])),
  );
  const [activeDrawerPanel, setActiveDrawerPanel] = useState<DrawerPanel>("project");
  const [selectedDiffFilePreview, setSelectedDiffFilePreview] = useState<{ fileId: string; isStaged: boolean } | null>(null);
  const composerRef = useRef<HTMLTextAreaElement | null>(null);
  const composerProfileMenuRef = useRef<HTMLDivElement | null>(null);
  const mainContentRef = useRef<HTMLElement | null>(null);
  const settingsContentRef = useRef<HTMLDivElement | null>(null);
  const userMenuRef = useRef<HTMLDivElement | null>(null);

  const selectedDiffFile = GIT_CHANGE_FILES.find((file) => file.id === selectedDiffFilePreview?.fileId) ?? null;
  const activeThread = getActiveThread(workspaces);
  const activeComposerProfile = agentProfiles.find((profile) => profile.id === activeAgentProfileId) ?? agentProfiles[0];
  const { isSidebarOpen, isDrawerOpen, isTerminalCollapsed } = panelVisibilityState;

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

  const handleTerminalResizeStart = (event: ReactMouseEvent<HTMLDivElement>) => {
    if (event.button !== 0) {
      return;
    }

    event.preventDefault();
    setTerminalResize({ startY: event.clientY, startHeight: terminalHeight });
  };

  useEffect(() => {
    const textarea = composerRef.current;
    if (!textarea) {
      return;
    }

    textarea.style.height = "0px";
    textarea.style.height = `${Math.min(textarea.scrollHeight, 176)}px`;
  }, [composerValue]);

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
    if (!isComposerProfileMenuOpen || typeof window === "undefined") {
      return;
    }

    const handlePointerDown = (event: MouseEvent) => {
      const target = event.target as Node | null;

      if (target && composerProfileMenuRef.current?.contains(target)) {
        return;
      }

      setComposerProfileMenuOpen(false);
    };

    const handleKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        setComposerProfileMenuOpen(false);
      }
    };

    window.addEventListener("mousedown", handlePointerDown);
    window.addEventListener("keydown", handleKeyDown);

    return () => {
      window.removeEventListener("mousedown", handlePointerDown);
      window.removeEventListener("keydown", handleKeyDown);
    };
  }, [isComposerProfileMenuOpen]);

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
    if (!selectedDiffFile || typeof window === "undefined") {
      return;
    }

    const handleKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        setSelectedDiffFilePreview(null);
      }
    };

    window.addEventListener("keydown", handleKeyDown);
    return () => window.removeEventListener("keydown", handleKeyDown);
  }, [selectedDiffFile]);

  useEffect(() => {
    if (!isSettingsOpen || typeof window === "undefined") {
      return;
    }

    const handleKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        setSettingsOpen(false);
      }
    };

    window.addEventListener("keydown", handleKeyDown);
    return () => window.removeEventListener("keydown", handleKeyDown);
  }, [isSettingsOpen]);

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
      const selectionInsideSettingsContent =
        isNodeInsideContainer(settingsContentRef.current, selection?.anchorNode ?? null) ||
        isNodeInsideContainer(settingsContentRef.current, selection?.focusNode ?? null);
      const targetInsideSettingsContent = isNodeInsideContainer(
        settingsContentRef.current,
        event.target instanceof Node ? event.target : null,
      );
      const selectionInsideMainContent =
        isNodeInsideContainer(mainContentRef.current, selection?.anchorNode ?? null) ||
        isNodeInsideContainer(mainContentRef.current, selection?.focusNode ?? null);
      const targetInsideMainContent = isNodeInsideContainer(
        mainContentRef.current,
        event.target instanceof Node ? event.target : null,
      );

      if (settingsContentRef.current && (targetInsideSettingsContent || selectionInsideSettingsContent)) {
        event.preventDefault();
        selectContainerContents(settingsContentRef.current);
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

  const handleComposerSubmit = () => {
    const trimmedValue = composerValue.trim();

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
      setNewThreadMode(false);
      setComposerValue("");
      return;
    }

    setComposerValue("");
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
    setSettingsOpen(true);
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
    setUserMenuOpen(!isSettingsOpen);
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

  return (
    <main className="h-screen overflow-hidden select-none bg-app-canvas text-app-foreground">
      <WorkbenchTopBar
        isMacOS={isMacOS}
        isWindows={isWindows}
        isSidebarOpen={isSidebarOpen}
        isDrawerOpen={isDrawerOpen}
        isTerminalCollapsed={isTerminalCollapsed}
        isUserMenuOpen={isUserMenuOpen}
        isSettingsOpen={isSettingsOpen}
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
                className="group flex w-full items-center gap-2.5 rounded-xl border border-transparent bg-transparent px-3 py-2.5 text-left text-app-muted transition-[transform,box-shadow,background-color,border-color,color] duration-200 hover:border-app-border hover:bg-app-surface-hover hover:text-app-foreground hover:shadow-[0_4px_14px_rgba(15,23,42,0.08)] active:scale-[0.99]"
              >
                <Boxes className="size-4 shrink-0 text-app-subtle transition-colors duration-200 group-hover:text-app-foreground" />
                <span className="truncate text-sm font-medium">Marketplace</span>
              </button>
            </div>

            <div className="mt-6 flex items-center justify-between px-3">
              <span className="text-xs uppercase tracking-[0.14em] text-app-subtle">Threads</span>
              <FolderOpen className="size-3.5 text-app-subtle" />
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
                    <div className="relative min-h-0 flex-1 overflow-hidden">
                      <div className="flex h-full items-center justify-center px-6 pb-8 pt-6">
                        <NewThreadEmptyState
                          recentProjects={recentProjects}
                          selectedProject={selectedProject}
                          onSelectProject={handleProjectSelect}
                        />
                      </div>
                      <div className="pointer-events-none absolute inset-x-0 bottom-0 h-14 bg-gradient-to-b from-transparent via-app-overlay via-55% to-app-canvas" />
                    </div>
                  ) : (
                    <>
                      <div className="flex h-12 items-center gap-3 border-b border-app-border px-5">
                        <div className="min-w-0 flex-1">
                          <div className="flex min-w-0 items-center gap-2">
                            {activeThread ? <ThreadStatusIndicator status={activeThread.status} /> : null}
                            <p className="truncate text-sm font-semibold text-app-foreground">
                              {activeThread?.name ?? "创建 Tauri 2 React+TS+shadcn/ui 模块化脚手架"}
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

                      <div className="relative min-h-0 flex-1">
                        <div className="h-full overflow-auto overscroll-contain [scrollbar-width:none] [&::-webkit-scrollbar]:hidden">
                          <div className="mx-auto flex max-w-4xl flex-col gap-6 px-6 pb-28 pt-6">
                            <div className="rounded-2xl border border-app-border bg-app-surface p-5">
                              <div className="flex items-center gap-2 text-app-muted">
                                <Sparkles className="size-4 text-app-success" />
                                <span className="text-sm font-medium">Jorben，这版布局已经收敛到更接近 Codex app 的工作台结构。</span>
                              </div>
                              <p className="mt-3 text-sm leading-7 text-app-muted">
                                左右侧边栏现在都是真正隐藏而不是缩窄；顶部仅保留应用名，且中部区域继续承担拖动窗口的能力。
                              </p>
                            </div>

                            <div className="space-y-5 pb-6">
                              {MESSAGE_SECTIONS.map((section) => (
                                <div key={section.title} className="rounded-2xl border border-app-border bg-app-surface-muted p-5">
                                  <h3 className="text-sm font-semibold text-app-foreground">{section.title}</h3>
                                  <ul className="mt-4 space-y-3 text-sm text-app-muted">
                                    {section.bullets.map((bullet) => (
                                      <li key={bullet} className="flex items-start gap-3">
                                        <span className="mt-2 size-1.5 shrink-0 rounded-full bg-app-subtle" />
                                        <code className="rounded bg-app-code px-2 py-1 text-[13px] text-app-foreground">{bullet}</code>
                                      </li>
                                    ))}
                                  </ul>
                                </div>
                              ))}
                            </div>

                            <Card className="border-app-border bg-app-surface text-app-foreground shadow-none">
                              <CardHeader>
                                <CardTitle className="text-base">Runtime Probe</CardTitle>
                                <CardDescription className="text-app-muted">确认桌面端命令桥接与应用元信息已经接通。</CardDescription>
                              </CardHeader>
                              <CardContent className="space-y-3 text-sm">
                                <div className="flex gap-3">
                                  <Button className="gap-2" onClick={() => void refetch()}>
                                    <RefreshCw className="size-4" />
                                    Refresh runtime info
                                  </Button>
                                </div>
                                {isLoading ? <p className="text-app-subtle">正在读取运行时信息...</p> : null}
                                {error ? <p className="text-app-danger">{error}</p> : null}
                                {data ? (
                                  <div className="grid gap-3 md:grid-cols-2 xl:grid-cols-3">
                                    <InspectorItem label="应用名" value={data.appName} />
                                    <InspectorItem label="版本" value={data.version} />
                                    <InspectorItem label="平台" value={data.platform} />
                                    <InspectorItem label="架构" value={data.arch} />
                                    <InspectorItem label="运行时" value={data.runtime} />
                                  </div>
                                ) : null}
                              </CardContent>
                            </Card>
                          </div>
                        </div>

                        <div className="pointer-events-none absolute inset-x-0 bottom-0 h-14 bg-gradient-to-b from-transparent via-app-overlay via-55% to-app-canvas" />
                      </div>
                    </>
                  )}

                  <div className={cn("shrink-0 px-6 pb-5", isNewThreadMode ? "relative z-30 pt-0" : "pt-3")}>
                    <div className="mx-auto max-w-4xl rounded-2xl border border-app-border bg-app-surface px-4 pb-3 pt-3 text-app-muted transition-colors focus-within:border-app-border-strong">
                      <textarea
                        ref={composerRef}
                        value={composerValue}
                        onChange={(event) => setComposerValue(event.target.value)}
                        rows={3}
                        placeholder={
                          isNewThreadMode
                            ? "Ask Tiy anything, @ to add files, / for commands, $ for skills"
                            : "Ask for follow-up changes"
                        }
                        className="max-h-44 min-h-[72px] w-full resize-none select-text overflow-y-auto bg-transparent text-sm leading-6 text-app-foreground outline-none placeholder:text-app-subtle [scrollbar-width:none] [&::-webkit-scrollbar]:hidden"
                      />
                      <div className="mt-3 flex items-end justify-between gap-3">
                        <div className="flex min-w-0 items-center gap-1.5">
                          <button type="button" className="-ml-1 mt-1 rounded-lg p-2 text-app-subtle transition-colors hover:bg-app-surface-hover hover:text-app-foreground">
                            <Plus className="size-4" />
                          </button>

                          {activeComposerProfile ? (
                            <div ref={composerProfileMenuRef} className="relative">
                              <button
                                type="button"
                                className={cn(
                                  "group inline-flex h-9 max-w-[220px] items-center gap-2 rounded-xl border border-app-border/80 bg-app-canvas/55 px-2.5 text-[12px] font-medium text-app-foreground shadow-[inset_0_1px_0_rgba(255,255,255,0.35)] backdrop-blur-sm transition-[border-color,background-color,box-shadow,transform] duration-200 hover:border-app-border-strong hover:bg-app-surface hover:shadow-[0_8px_18px_rgba(15,23,42,0.08)]",
                                  isComposerProfileMenuOpen && "border-app-border-strong bg-app-surface shadow-[0_10px_24px_rgba(15,23,42,0.12)]",
                                )}
                                aria-haspopup="menu"
                                aria-expanded={isComposerProfileMenuOpen}
                                aria-label={`Active profile: ${activeComposerProfile.name}`}
                                onClick={() => setComposerProfileMenuOpen((current) => !current)}
                              >
                                <span className="flex size-6 shrink-0 items-center justify-center rounded-lg bg-app-surface text-app-subtle ring-1 ring-app-border/70 transition-colors group-hover:text-app-foreground">
                                  <Bot className="size-3.5" />
                                </span>
                                <span className="truncate">{activeComposerProfile.name}</span>
                                <ChevronDown
                                  className={cn(
                                    "ml-auto size-3.5 shrink-0 text-app-subtle transition-transform duration-200",
                                    isComposerProfileMenuOpen && "rotate-180",
                                  )}
                                />
                              </button>

                              {isComposerProfileMenuOpen ? (
                                <div className="absolute bottom-[calc(100%+10px)] left-0 z-30 min-w-[240px] overflow-hidden rounded-2xl border border-app-border/80 bg-app-surface/95 p-1.5 shadow-[0_20px_48px_rgba(15,23,42,0.16)] backdrop-blur-xl">
                                  <div className="px-2.5 pb-1.5 pt-1">
                                    <div className="text-[10px] font-semibold uppercase tracking-[0.18em] text-app-subtle">Profiles</div>
                                  </div>
                                  <div className="space-y-1">
                                    {agentProfiles.map((profile) => {
                                      const isActive = profile.id === activeAgentProfileId;

                                      return (
                                        <button
                                          key={profile.id}
                                          type="button"
                                          className={cn(
                                            "flex w-full items-center gap-2 rounded-xl px-2.5 py-2 text-left transition-colors",
                                            isActive
                                              ? "bg-app-surface-hover text-app-foreground"
                                              : "text-app-muted hover:bg-app-surface-hover hover:text-app-foreground",
                                          )}
                                          onClick={() => {
                                            setActiveAgentProfile(profile.id);
                                            setComposerProfileMenuOpen(false);
                                          }}
                                        >
                                          <span className="flex size-7 shrink-0 items-center justify-center rounded-lg bg-app-canvas text-app-subtle ring-1 ring-app-border/70">
                                            <Bot className="size-3.5" />
                                          </span>
                                          <span className="min-w-0 flex-1 truncate text-[12px] font-medium">{profile.name}</span>
                                          {isActive ? <span className="text-app-foreground">•</span> : null}
                                        </button>
                                      );
                                    })}
                                  </div>
                                </div>
                              ) : null}
                            </div>
                          ) : null}
                        </div>
                        <button
                          type="button"
                          onClick={handleComposerSubmit}
                          className="flex size-8 items-center justify-center rounded-full bg-primary text-primary-foreground shadow-[0_1px_2px_rgba(15,23,42,0.18)] transition-[transform,box-shadow,background-color] duration-200 hover:scale-[1.02] hover:bg-primary/90 hover:shadow-[0_4px_10px_rgba(15,23,42,0.18)] active:scale-[0.98] disabled:cursor-not-allowed disabled:opacity-60 disabled:hover:scale-100 disabled:hover:shadow-[0_1px_2px_rgba(15,23,42,0.18)]"
                          disabled={!composerValue.trim()}
                        >
                          <ArrowUp className="size-3.5" />
                        </button>
                      </div>
                    </div>
                  </div>
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
                      <ProjectPanel />
                    ) : (
                      <GitPanel onOpenDiffPreview={(fileId, isStaged) => setSelectedDiffFilePreview({ fileId, isStaged })} />
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
                <div className="flex h-[38px] shrink-0 items-center justify-between px-4 text-xs text-app-muted">
                  <div className="flex items-center gap-2">
                    <TerminalSquare className="size-3.5" />
                    <span>Terminal</span>
                  </div>
                  <Button
                    size="icon"
                    variant="ghost"
                    className="size-7 text-app-subtle hover:bg-app-surface-hover hover:text-app-foreground"
                    aria-label="收起 terminal"
                    title="收起 terminal"
                    onClick={() => setTerminalCollapsed(true)}
                  >
                    <PanelBottom className="size-4" />
                  </Button>
                </div>
                <div className="min-h-0 flex-1 overflow-auto overscroll-contain px-4 py-3 font-mono text-[12px] leading-6 text-app-muted [scrollbar-width:none] [&::-webkit-scrollbar]:hidden">
                  {TERMINAL_LINES.map((line, index) => (
                    <div key={line} className="flex gap-3">
                      <span className={cn("text-app-subtle", index === 0 ? "text-app-info" : "")}>›</span>
                      <span>{line}</span>
                    </div>
                  ))}
                </div>
              </div>
            </section>
          </div>
        </section>
      </div>

      {selectedDiffFile ? (
        <GitDiffPreviewPanel
          file={selectedDiffFile}
          isStaged={Boolean(selectedDiffFilePreview?.isStaged)}
          onClose={() => setSelectedDiffFilePreview(null)}
        />
      ) : null}

      {isSettingsOpen ? (
        <SettingsCenterOverlay
          activeCategory={activeSettingsCategory}
          agentProfiles={agentProfiles}
          activeAgentProfileId={activeAgentProfileId}
          contentRef={settingsContentRef}
          generalPreferences={generalPreferences}
          isCheckingUpdates={isCheckingUpdates}
          language={language}
          policy={policy}
          commands={commands}
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
          onClose={() => setSettingsOpen(false)}
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
          onUpdateWorkspace={updateWorkspace}
          onUpdateWritableRoot={updateWritableRoot}
        />
      ) : null}
    </main>
  );
}
