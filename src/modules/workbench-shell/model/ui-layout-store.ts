import { createStore, useStore as useStoreBase, shallowEqual } from "@/shared/lib/create-store";
import type { DrawerPanel, PanelVisibilityState, WorkbenchOverlay } from "@/modules/workbench-shell/model/types";
import type { GitDiffSelection } from "@/modules/workbench-shell/ui/source-control-panels";
import { DEFAULT_TERMINAL_HEIGHT, PANEL_VISIBILITY_STORAGE_KEY } from "@/modules/workbench-shell/model/fixtures";
import { readPanelVisibilityState } from "@/modules/workbench-shell/model/helpers";
import type { SettingsCategory } from "@/modules/settings-center/model/use-settings-controller";

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

export interface TerminalResizeState {
  startY: number;
  startHeight: number;
}

export interface UILayoutStoreState {
  [key: string]: unknown;
  // Overlay
  activeOverlay: WorkbenchOverlay;
  activeSettingsCategory: SettingsCategory;
  openSettingsSection: "theme" | "language" | null;

  // Panel visibility
  panelVisibility: PanelVisibilityState;
  activeDrawerPanel: DrawerPanel;
  selectedDiffSelection: GitDiffSelection | null;

  // Terminal layout
  terminalCollapsedByThreadKey: Record<string, boolean>;
  terminalHeight: number;
  terminalResize: TerminalResizeState | null;

  // Menu state
  isUserMenuOpen: boolean;
  activeWorkspaceMenuId: string | null;

  // Onboarding
  showOnboarding: boolean;
}

// ---------------------------------------------------------------------------
// Initial state
// ---------------------------------------------------------------------------

function getInitialState(): UILayoutStoreState {
  return {
    activeOverlay: null,
    activeSettingsCategory: "general",
    openSettingsSection: null,
    panelVisibility: readPanelVisibilityState(),
    activeDrawerPanel: "project",
    selectedDiffSelection: null,
    terminalCollapsedByThreadKey: {},
    terminalHeight: DEFAULT_TERMINAL_HEIGHT,
    terminalResize: null,
    isUserMenuOpen: false,
    activeWorkspaceMenuId: null,
    showOnboarding: false,
  };
}

// ---------------------------------------------------------------------------
// Store
// ---------------------------------------------------------------------------

export const uiLayoutStore = createStore<UILayoutStoreState>(getInitialState());

// Auto-persist panelVisibility to localStorage
let lastPersistedPanelVisibility: string | null = null;
uiLayoutStore.subscribe(() => {
  const { panelVisibility } = uiLayoutStore.getState();
  const serialized = JSON.stringify(panelVisibility);
  if (serialized !== lastPersistedPanelVisibility) {
    lastPersistedPanelVisibility = serialized;
    try {
      window.localStorage.setItem(PANEL_VISIBILITY_STORAGE_KEY, serialized);
    } catch {
      // localStorage may be unavailable (SSR / sandboxed iframe)
    }
  }
});

// ---------------------------------------------------------------------------
// React hook (re-export for convenience)
// ---------------------------------------------------------------------------

export { useStoreBase as useStore, shallowEqual };

// ---------------------------------------------------------------------------
// Actions — Overlay
// ---------------------------------------------------------------------------

export function openOverlay(type: NonNullable<WorkbenchOverlay>, settingsCategory?: SettingsCategory): void {
  uiLayoutStore.setState({
    activeOverlay: type,
    activeSettingsCategory: settingsCategory ?? "general" as SettingsCategory,
    // Close menus when opening an overlay
    isUserMenuOpen: false,
    activeWorkspaceMenuId: null,
    openSettingsSection: null,
  });
}

export function closeOverlay(): void {
  uiLayoutStore.setState({
    activeOverlay: null,
    activeSettingsCategory: "general" as SettingsCategory,
    openSettingsSection: null,
  });
}

export function setActiveSettingsCategory(category: SettingsCategory): void {
  uiLayoutStore.setState({ activeSettingsCategory: category });
}

export function setOpenSettingsSection(section: "theme" | "language" | null): void {
  uiLayoutStore.setState({ openSettingsSection: section });
}

// ---------------------------------------------------------------------------
// Actions — Panel visibility
// ---------------------------------------------------------------------------

export function setSidebarOpen(open: boolean): void {
  uiLayoutStore.setState((prev) => ({
    panelVisibility: { ...prev.panelVisibility, isSidebarOpen: open },
  }));
}

export function setDrawerOpen(open: boolean): void {
  uiLayoutStore.setState((prev) => ({
    panelVisibility: { ...prev.panelVisibility, isDrawerOpen: open },
  }));
}

export function toggleSidebar(): void {
  uiLayoutStore.setState((prev) => ({
    panelVisibility: {
      ...prev.panelVisibility,
      isSidebarOpen: !prev.panelVisibility.isSidebarOpen,
    },
  }));
}

export function toggleDrawer(): void {
  uiLayoutStore.setState((prev) => ({
    panelVisibility: {
      ...prev.panelVisibility,
      isDrawerOpen: !prev.panelVisibility.isDrawerOpen,
    },
  }));
}

export function setActiveDrawerPanel(panel: DrawerPanel): void {
  uiLayoutStore.setState({ activeDrawerPanel: panel });
}

export function setSelectedDiffSelection(selection: GitDiffSelection | null): void {
  uiLayoutStore.setState({ selectedDiffSelection: selection });
}

// ---------------------------------------------------------------------------
// Actions — Terminal layout
// ---------------------------------------------------------------------------

export function setTerminalCollapsed(threadKey: string, collapsed: boolean): void {
  uiLayoutStore.setState((prev) => {
    const next = { ...prev.terminalCollapsedByThreadKey };

    if (collapsed) {
      // `true` is the default — remove the key to avoid storing redundant entries
      delete next[threadKey];
    } else {
      next[threadKey] = collapsed;
    }

    return { terminalCollapsedByThreadKey: next };
  });
}

export function setTerminalHeight(height: number): void {
  uiLayoutStore.setState({ terminalHeight: height });
}

export function setTerminalResize(resize: TerminalResizeState | null): void {
  uiLayoutStore.setState({ terminalResize: resize });
}

// ---------------------------------------------------------------------------
// Actions — Menu state
// ---------------------------------------------------------------------------

export function setUserMenuOpen(open: boolean): void {
  uiLayoutStore.setState({
    isUserMenuOpen: open,
    openSettingsSection: null,
  });
}

export function toggleUserMenu(): void {
  uiLayoutStore.setState((prev) => ({
    isUserMenuOpen: !prev.isUserMenuOpen,
    openSettingsSection: null,
  }));
}

export function setActiveWorkspaceMenuId(id: string | null): void {
  uiLayoutStore.setState({ activeWorkspaceMenuId: id });
}

// ---------------------------------------------------------------------------
// Actions — Onboarding
// ---------------------------------------------------------------------------

export function setShowOnboarding(show: boolean): void {
  uiLayoutStore.setState({ showOnboarding: show });
}

// ---------------------------------------------------------------------------
// Actions — Compound / bulk
// ---------------------------------------------------------------------------

/**
 * Remove all terminal-collapsed entries that reference threads within a set of
 * workspace thread IDs (used during workspace removal).
 */
export function removeTerminalCollapsedForThreads(threadIds: Set<string>): void {
  uiLayoutStore.setState((prev) => {
    const next = { ...prev.terminalCollapsedByThreadKey };
    let changed = false;
    for (const key of Object.keys(next)) {
      if (threadIds.has(key)) {
        delete next[key];
        changed = true;
      }
    }
    return changed ? { terminalCollapsedByThreadKey: next } : {};
  });
}
