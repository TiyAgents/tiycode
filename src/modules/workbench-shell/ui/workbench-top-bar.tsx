import { useEffect, useState } from "react";
import {
  Check,
  CircleUserRound,
  Copy,
  Globe,
  LoaderCircle,
  LogIn,
  LogOut,
  Minus,
  MoreHorizontal,
  Moon,
  Palette,
  PanelBottom,
  PanelLeft,
  PanelRight,
  RefreshCw,
  Square,
  Sun,
  X,
} from "lucide-react";
import { isTauri } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";
import type { LanguagePreference } from "@/app/providers/language-provider";
import type { ThemePreference } from "@/app/providers/theme-provider";
import { Button } from "@/shared/ui/button";
import { cn } from "@/shared/lib/utils";
import {
  LANGUAGE_OPTIONS,
  MAC_USER_MENU_OFFSET,
  MAC_USER_MENU_POPOVER_OFFSET,
  MENU_SUBMENU_GROUP_CLASS,
  MENU_SUBMENU_ICON_CLASS,
  MENU_SUBMENU_LABEL_CLASS,
  MENU_SUBMENU_OPTION_CLASS,
  MENU_TRIGGER_CLASS,
  MENU_TRIGGER_ICON_CLASS,
  MENU_TRIGGER_LABEL_CLASS,
  THEME_OPTIONS,
} from "@/modules/workbench-shell/model/fixtures";
import type { MockUserSession } from "@/modules/workbench-shell/model/types";

export function WorkbenchTopBar({
  isMacOS,
  isWindows,
  isSidebarOpen,
  isDrawerOpen,
  isTerminalCollapsed,
  isUserMenuOpen,
  isOverlayOpen,
  isLoggedIn,
  userSession,
  isCheckingUpdates,
  updateStatus,
  openSettingsSection,
  userMenuRef,
  selectedLanguageLabel,
  selectedThemeSummary,
  language,
  theme,
  onToggleUserMenu,
  onLogin,
  onLogout,
  onCheckUpdates,
  onOpenSettings,
  onSelectLanguage,
  onSelectTheme,
  onToggleSettingsSection,
  onToggleSidebar,
  onToggleDrawer,
  onToggleTerminal,
}: {
  isMacOS: boolean;
  isWindows: boolean;
  isSidebarOpen: boolean;
  isDrawerOpen: boolean;
  isTerminalCollapsed: boolean;
  isUserMenuOpen: boolean;
  isOverlayOpen: boolean;
  isLoggedIn: boolean;
  userSession: MockUserSession | null;
  isCheckingUpdates: boolean;
  updateStatus: string | null;
  openSettingsSection: "theme" | "language" | null;
  userMenuRef: { current: HTMLDivElement | null };
  selectedLanguageLabel: string;
  selectedThemeSummary: string;
  language: LanguagePreference;
  theme: ThemePreference;
  onToggleUserMenu: () => void;
  onLogin: () => void;
  onLogout: () => void;
  onCheckUpdates: () => void;
  onOpenSettings: () => void;
  onSelectLanguage: (language: LanguagePreference) => void;
  onSelectTheme: (theme: ThemePreference) => void;
  onToggleSettingsSection: React.Dispatch<React.SetStateAction<"theme" | "language" | null>>;
  onToggleSidebar: () => void;
  onToggleDrawer: () => void;
  onToggleTerminal: () => void;
}) {
  const [isMaximized, setIsMaximized] = useState(false);
  const canUseDesktopWindowControls = isWindows && isTauri();

  useEffect(() => {
    if (!canUseDesktopWindowControls) {
      setIsMaximized(false);
      return;
    }

    const appWindow = getCurrentWindow();
    let unlisten: (() => void) | undefined;

    const setup = async () => {
      setIsMaximized(await appWindow.isMaximized());
      unlisten = await appWindow.onResized(async () => {
        setIsMaximized(await appWindow.isMaximized());
      });
    };

    void setup();
    return () => unlisten?.();
  }, [canUseDesktopWindowControls]);

  const handleWindowMinimize = () => {
    if (!canUseDesktopWindowControls) {
      return;
    }

    void getCurrentWindow().minimize();
  };

  const handleWindowToggleMaximize = () => {
    if (!canUseDesktopWindowControls) {
      return;
    }

    void getCurrentWindow().toggleMaximize();
  };

  const handleWindowClose = () => {
    if (!canUseDesktopWindowControls) {
      return;
    }

    void getCurrentWindow().close();
  };

  const panelToggleButtonClass =
    "relative size-7 text-app-subtle transition-[color,background-color] duration-200 hover:bg-app-surface-hover hover:text-app-foreground";

  return (
    <header className="fixed inset-x-0 top-0 z-30 h-9 border-b border-app-border bg-app-chrome backdrop-blur-xl">
      <div className={cn("grid h-full grid-cols-[auto_1fr_auto] items-center gap-2 px-2.5", isWindows && "pr-0")}>
        <div className={cn("relative z-10 flex h-full shrink-0 items-center", isMacOS ? "w-[150px]" : "w-[132px]")} ref={userMenuRef}>
          <Button
            size="icon"
            variant="ghost"
            className={cn(
              "size-7 rounded-full text-app-subtle transition-[color,background-color,border-color] duration-200 hover:bg-app-surface-hover hover:text-app-foreground",
              isMacOS ? MAC_USER_MENU_OFFSET : "ml-2",
              isOverlayOpen && "pointer-events-none invisible",
              isUserMenuOpen && "bg-app-surface-hover text-app-foreground",
            )}
            aria-label={isLoggedIn ? "打开用户菜单" : "打开登录菜单"}
            title={isLoggedIn ? "打开用户菜单" : "打开登录菜单"}
            aria-expanded={isUserMenuOpen}
            aria-haspopup="menu"
            onClick={onToggleUserMenu}
          >
            <CircleUserRound className="size-4" />
          </Button>

          {isUserMenuOpen ? (
            <div className={cn("absolute left-0 top-full z-30 mt-2 w-[248px] rounded-2xl border border-app-border bg-app-menu p-1.5 shadow-[0_20px_48px_rgba(15,23,42,0.18)] dark:shadow-[0_20px_48px_rgba(0,0,0,0.42)]", isMacOS ? MAC_USER_MENU_POPOVER_OFFSET : "left-2")}>
              {userSession ? (
                <div className="mb-1 flex items-center gap-3 rounded-xl px-3 py-3 text-left">
                  <div className="flex size-10 shrink-0 items-center justify-center rounded-full bg-app-surface-active text-sm font-semibold text-app-foreground">
                    {userSession.avatar}
                  </div>
                  <div className="min-w-0">
                    <p className="truncate text-sm font-medium text-app-foreground">{userSession.name}</p>
                    <p className="truncate text-xs text-app-subtle">{userSession.email}</p>
                  </div>
                </div>
              ) : null}

              <button
                type="button"
                className={cn(MENU_TRIGGER_CLASS, "text-app-foreground")}
                aria-expanded={openSettingsSection === "theme"}
                onClick={() => onToggleSettingsSection((current) => (current === "theme" ? null : "theme"))}
              >
                <Palette className={MENU_TRIGGER_ICON_CLASS} />
                <span className={MENU_TRIGGER_LABEL_CLASS}>主题</span>
                <span className="shrink-0 text-xs text-app-subtle">{selectedThemeSummary}</span>
              </button>

              {openSettingsSection === "theme" ? (
                <div className={MENU_SUBMENU_GROUP_CLASS}>
                  <div className="space-y-0.5">
                    {THEME_OPTIONS.map((option) => {
                      const OptionIcon = option.icon === Moon ? Moon : option.icon === Sun ? Sun : option.icon;
                      const isSelected = theme === option.value;

                      return (
                        <button
                          key={option.value}
                          type="button"
                          className={cn(
                            MENU_SUBMENU_OPTION_CLASS,
                            isSelected
                              ? "bg-app-surface-hover/80 text-app-foreground"
                              : "text-app-muted hover:bg-app-surface-hover hover:text-app-foreground",
                          )}
                          onClick={() => onSelectTheme(option.value)}
                        >
                          <OptionIcon className={MENU_SUBMENU_ICON_CLASS} />
                          <span className={MENU_SUBMENU_LABEL_CLASS}>{option.label}</span>
                          {isSelected ? <Check className="size-3.5 shrink-0 text-app-foreground" /> : null}
                        </button>
                      );
                    })}
                  </div>
                </div>
              ) : null}

              <button
                type="button"
                className={cn(MENU_TRIGGER_CLASS, "mt-1 text-app-foreground")}
                aria-expanded={openSettingsSection === "language"}
                onClick={() => onToggleSettingsSection((current) => (current === "language" ? null : "language"))}
              >
                <Globe className={MENU_TRIGGER_ICON_CLASS} />
                <span className={MENU_TRIGGER_LABEL_CLASS}>语言</span>
                <span className="shrink-0 text-xs text-app-subtle">{selectedLanguageLabel}</span>
              </button>

              {openSettingsSection === "language" ? (
                <div className={MENU_SUBMENU_GROUP_CLASS}>
                  <div className="space-y-0.5">
                    {LANGUAGE_OPTIONS.map((option) => {
                      const OptionIcon = option.icon;
                      const isSelected = language === option.value;

                      return (
                        <button
                          key={option.value}
                          type="button"
                          className={cn(
                            MENU_SUBMENU_OPTION_CLASS,
                            isSelected
                              ? "bg-app-surface-hover/80 text-app-foreground"
                              : "text-app-muted hover:bg-app-surface-hover hover:text-app-foreground",
                          )}
                          onClick={() => onSelectLanguage(option.value)}
                        >
                          <OptionIcon className={MENU_SUBMENU_ICON_CLASS} />
                          <span className={MENU_SUBMENU_LABEL_CLASS}>{option.label}</span>
                          {isSelected ? <Check className="size-3.5 shrink-0 text-app-foreground" /> : null}
                        </button>
                      );
                    })}
                  </div>
                </div>
              ) : null}

              <button
                type="button"
                className={cn(MENU_TRIGGER_CLASS, "mt-1 text-app-foreground", isCheckingUpdates && "cursor-wait")}
                onClick={onCheckUpdates}
              >
                {isCheckingUpdates ? (
                  <LoaderCircle className={cn(MENU_TRIGGER_ICON_CLASS, "animate-spin")} />
                ) : (
                  <RefreshCw className={MENU_TRIGGER_ICON_CLASS} />
                )}
                <span className={MENU_TRIGGER_LABEL_CLASS}>检查更新</span>
              </button>

              <button
                type="button"
                className={cn(MENU_TRIGGER_CLASS, "mt-1 text-app-foreground", isOverlayOpen && "bg-app-surface-hover")}
                onClick={onOpenSettings}
              >
                <MoreHorizontal className={MENU_TRIGGER_ICON_CLASS} />
                <span className={MENU_TRIGGER_LABEL_CLASS}>更多设置</span>
              </button>

              {userSession ? (
                <button type="button" className={cn(MENU_TRIGGER_CLASS, "mt-1 text-app-foreground")} onClick={onLogout}>
                  <LogOut className={MENU_TRIGGER_ICON_CLASS} />
                  <span className={MENU_TRIGGER_LABEL_CLASS}>退出登录</span>
                </button>
              ) : (
                <button type="button" className={cn(MENU_TRIGGER_CLASS, "mt-1 text-app-foreground")} onClick={onLogin}>
                  <LogIn className={MENU_TRIGGER_ICON_CLASS} />
                  <span className={MENU_TRIGGER_LABEL_CLASS}>登录</span>
                </button>
              )}

              {updateStatus ? (
                <div className="px-3 pb-1 pt-2 text-xs text-app-subtle">{updateStatus}</div>
              ) : null}
            </div>
          ) : null}
        </div>

        <div
          className="relative z-10 flex h-full items-center justify-center"
          data-tauri-drag-region=""
        >
          <img src="/app-icon.png" alt="" className="mr-1.5 size-4 shrink-0 select-none" draggable={false} data-tauri-drag-region="" />
          <span className="select-none text-[13px] font-semibold tracking-[0.02em] text-app-foreground" data-tauri-drag-region="">Tiy Agent</span>
        </div>

        <div className="relative z-10 flex items-center justify-end gap-0.5">
          <Button
            size="icon"
            variant="ghost"
            className={cn(panelToggleButtonClass, isSidebarOpen && "text-app-foreground", isOverlayOpen && "pointer-events-none invisible")}
            aria-label={isSidebarOpen ? "收拢 sidebar" : "展开 sidebar"}
            title={isSidebarOpen ? "收拢 sidebar" : "展开 sidebar"}
            onClick={onToggleSidebar}
          >
            <PanelLeft className="size-4" />
          </Button>
          <Button
            size="icon"
            variant="ghost"
            className={cn(panelToggleButtonClass, !isTerminalCollapsed && "text-app-foreground", isOverlayOpen && "pointer-events-none invisible")}
            aria-label={isTerminalCollapsed ? "展开 terminal 面板" : "收起 terminal 面板"}
            title={isTerminalCollapsed ? "展开 terminal 面板" : "收起 terminal 面板"}
            onClick={onToggleTerminal}
          >
            <PanelBottom className="size-4" />
          </Button>
          <Button
            size="icon"
            variant="ghost"
            className={cn(panelToggleButtonClass, isDrawerOpen && "text-app-foreground", isOverlayOpen && "pointer-events-none invisible")}
            aria-label={isDrawerOpen ? "收拢右侧面板" : "展开右侧面板"}
            title={isDrawerOpen ? "收拢右侧面板" : "展开右侧面板"}
            onClick={onToggleDrawer}
          >
            <PanelRight className="size-4" />
          </Button>

          {canUseDesktopWindowControls ? (
            <>
              <div className="mx-1 h-4 w-px bg-app-border" />
              <button
                type="button"
                className="flex size-7 items-center justify-center text-app-subtle transition-colors hover:bg-app-surface-hover hover:text-app-foreground"
                aria-label="最小化"
                title="最小化"
                onClick={handleWindowMinimize}
              >
                <Minus className="size-3.5" />
              </button>
              <button
                type="button"
                className="flex size-7 items-center justify-center text-app-subtle transition-colors hover:bg-app-surface-hover hover:text-app-foreground"
                aria-label={isMaximized ? "还原" : "最大化"}
                title={isMaximized ? "还原" : "最大化"}
                onClick={handleWindowToggleMaximize}
              >
                {isMaximized ? <Copy className="size-3" /> : <Square className="size-3" />}
              </button>
              <button
                type="button"
                className="flex size-7 items-center justify-center text-app-subtle transition-colors hover:bg-red-500/90 hover:text-white"
                aria-label="关闭"
                title="关闭"
                onClick={handleWindowClose}
              >
                <X className="size-3.5" />
              </button>
            </>
          ) : null}
        </div>
      </div>
    </header>
  );
}
