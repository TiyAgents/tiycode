import { useEffect, useState } from "react";
import {
  Check,
  Copy,
  Globe,
  LoaderCircle,
  Minus,
  MoreHorizontal,
  Settings,
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
import { useT } from "@/i18n";
import { Button } from "@/shared/ui/button";
import { cn } from "@/shared/lib/utils";
import {
  LANGUAGE_OPTIONS,
  MENU_SUBMENU_GROUP_CLASS,
  MENU_SUBMENU_ICON_CLASS,
  MENU_SUBMENU_LABEL_CLASS,
  MENU_SUBMENU_OPTION_CLASS,
  MENU_TRIGGER_CLASS,
  MENU_TRIGGER_ICON_CLASS,
  MENU_TRIGGER_LABEL_CLASS,
  THEME_OPTIONS,
} from "@/modules/workbench-shell/model/fixtures";
import {
  uiLayoutStore,
  toggleSidebar,
  toggleDrawer,
  toggleUserMenu,
  setOpenSettingsSection,
} from "@/modules/workbench-shell/model/ui-layout-store";
import { useStore } from "@/shared/lib/create-store";

export function WorkbenchTopBar({
  isMacOS,
  isWindows,
  isTerminalCollapsed,
  isCheckingUpdates,
  updateStatus,
  userMenuRef,
  selectedLanguageLabel,
  selectedThemeSummary,
  language,
  theme,
  onCheckUpdates,
  onOpenSettings,
  onSelectLanguage,
  onSelectTheme,
  onToggleTerminal,
}: {
  isMacOS: boolean;
  isWindows: boolean;
  isTerminalCollapsed: boolean;
  isCheckingUpdates: boolean;
  updateStatus: string | null;
  userMenuRef: { current: HTMLDivElement | null };
  selectedLanguageLabel: string;
  selectedThemeSummary: string;
  language: LanguagePreference;
  theme: ThemePreference;
  onCheckUpdates: () => void;
  onOpenSettings: () => void;
  onSelectLanguage: (language: LanguagePreference) => void;
  onSelectTheme: (theme: ThemePreference) => void;
  onToggleTerminal: () => void;
}) {
  const t = useT();

  // ── Phase 2: subscribe to uiLayoutStore for panel/menu/overlay state ──
  const isSidebarOpen = useStore(uiLayoutStore, (s) => s.panelVisibility.isSidebarOpen);
  const isDrawerOpen = useStore(uiLayoutStore, (s) => s.panelVisibility.isDrawerOpen);
  const isUserMenuOpen = useStore(uiLayoutStore, (s) => s.isUserMenuOpen);
  const activeOverlay = useStore(uiLayoutStore, (s) => s.activeOverlay);
  const openSettingsSection = useStore(uiLayoutStore, (s) => s.openSettingsSection);
  const isOverlayOpen = activeOverlay !== null;
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
        <div className={cn("relative z-10 flex h-full shrink-0 items-center", isMacOS && "w-[150px]")}>
        </div>

        <div
          className={cn("relative z-10 flex h-full items-center", isWindows ? "justify-start" : "justify-center")}
          data-tauri-drag-region=""
        >
          <img src="/app-icon.png" alt="" className="mr-1.5 size-4 shrink-0 select-none" draggable={false} data-tauri-drag-region="" />
          <span className="select-none text-[13px] font-semibold tracking-[0.02em] text-app-foreground" data-tauri-drag-region="">TiyCode</span>
        </div>

        <div className="relative z-10 flex items-center justify-end gap-0.5" ref={userMenuRef}>
          <div className="relative">
            <Button
              size="icon"
              variant="ghost"
              className={cn(
                panelToggleButtonClass,
                "rounded-full",
                isOverlayOpen && "pointer-events-none invisible",
                isUserMenuOpen && "bg-app-surface-hover text-app-foreground",
              )}
              aria-label={t("topBar.openMenu")}
              title={t("topBar.openMenu")}
              aria-expanded={isUserMenuOpen}
              aria-haspopup="menu"
              onClick={toggleUserMenu}
            >
              <Settings className="size-4" />
            </Button>

            {isUserMenuOpen ? (
              <div className="absolute right-0 top-full z-30 mt-2 w-[248px] rounded-2xl border border-app-border bg-app-menu p-1.5 shadow-[0_20px_48px_rgba(15,23,42,0.18)] dark:shadow-[0_20px_48px_rgba(0,0,0,0.42)]">

                <button
                  type="button"
                  className={cn(MENU_TRIGGER_CLASS, "text-app-foreground")}
                  aria-expanded={openSettingsSection === "theme"}
                  onClick={() => {
                    const current = uiLayoutStore.getState().openSettingsSection;
                    setOpenSettingsSection(current === "theme" ? null : "theme");
                  }}
                >
                  <Palette className={MENU_TRIGGER_ICON_CLASS} />
                  <span className={MENU_TRIGGER_LABEL_CLASS}>{t("topBar.theme")}</span>
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
                            <span className={MENU_SUBMENU_LABEL_CLASS}>{t(option.labelKey)}</span>
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
                  onClick={() => {
                    const current = uiLayoutStore.getState().openSettingsSection;
                    setOpenSettingsSection(current === "language" ? null : "language");
                  }}
                >
                  <Globe className={MENU_TRIGGER_ICON_CLASS} />
                  <span className={MENU_TRIGGER_LABEL_CLASS}>{t("topBar.language")}</span>
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
                  <span className={MENU_TRIGGER_LABEL_CLASS}>{t("topBar.checkUpdates")}</span>
                </button>

                <button
                  type="button"
                  className={cn(MENU_TRIGGER_CLASS, "mt-1 text-app-foreground", isOverlayOpen && "bg-app-surface-hover")}
                  onClick={onOpenSettings}
                >
                  <MoreHorizontal className={MENU_TRIGGER_ICON_CLASS} />
                  <span className={MENU_TRIGGER_LABEL_CLASS}>{t("topBar.moreSettings")}</span>
                </button>

                {updateStatus ? (
                  <div className="px-3 pb-1 pt-2 text-xs text-app-subtle">{updateStatus}</div>
                ) : null}
              </div>
            ) : null}
          </div>

          <Button
            size="icon"
            variant="ghost"
            className={cn(panelToggleButtonClass, isSidebarOpen && "text-app-foreground", isOverlayOpen && "pointer-events-none invisible")}
            aria-label={isSidebarOpen ? t("topBar.collapseSidebar") : t("topBar.expandSidebar")}
            title={isSidebarOpen ? t("topBar.collapseSidebar") : t("topBar.expandSidebar")}
            onClick={toggleSidebar}
          >
            <PanelLeft className="size-4" />
          </Button>
          <Button
            size="icon"
            variant="ghost"
            className={cn(panelToggleButtonClass, !isTerminalCollapsed && "text-app-foreground", isOverlayOpen && "pointer-events-none invisible")}
            aria-label={isTerminalCollapsed ? t("topBar.expandTerminal") : t("topBar.collapseTerminal")}
            title={isTerminalCollapsed ? t("topBar.expandTerminal") : t("topBar.collapseTerminal")}
            onClick={onToggleTerminal}
          >
            <PanelBottom className="size-4" />
          </Button>
          <Button
            size="icon"
            variant="ghost"
            className={cn(panelToggleButtonClass, isDrawerOpen && "text-app-foreground", isOverlayOpen && "pointer-events-none invisible")}
            aria-label={isDrawerOpen ? t("topBar.collapseDrawer") : t("topBar.expandDrawer")}
            title={isDrawerOpen ? t("topBar.collapseDrawer") : t("topBar.expandDrawer")}
            onClick={toggleDrawer}
          >
            <PanelRight className="size-4" />
          </Button>

          {canUseDesktopWindowControls ? (
            <>
              <div className="mx-1 h-4 w-px bg-app-border" />
              <button
                type="button"
                className="flex size-7 items-center justify-center text-app-subtle transition-colors hover:bg-app-surface-hover hover:text-app-foreground"
                aria-label={t("topBar.minimize")}
                title={t("topBar.minimize")}
                onClick={handleWindowMinimize}
              >
                <Minus className="size-3.5" />
              </button>
              <button
                type="button"
                className="flex size-7 items-center justify-center text-app-subtle transition-colors hover:bg-app-surface-hover hover:text-app-foreground"
                aria-label={isMaximized ? t("topBar.restore") : t("topBar.maximize")}
                title={isMaximized ? t("topBar.restore") : t("topBar.maximize")}
                onClick={handleWindowToggleMaximize}
              >
                {isMaximized ? <Copy className="size-3" /> : <Square className="size-3" />}
              </button>
              <button
                type="button"
                className="flex size-7 items-center justify-center text-app-subtle transition-colors hover:bg-red-500/90 hover:text-white"
                aria-label={t("topBar.close")}
                title={t("topBar.close")}
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
