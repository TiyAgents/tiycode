import { describe, it, expect, beforeEach } from "vitest";
import {
  uiLayoutStore,
  openOverlay,
  closeOverlay,
  toggleSidebar,
  toggleDrawer,
  setSidebarOpen,
  setDrawerOpen,
  setTerminalCollapsed,
  setTerminalHeight,
  setUserMenuOpen,
  toggleUserMenu,
  setActiveWorkspaceMenuId,
  setShowOnboarding,
  setSelectedDiffSelection,
  removeTerminalCollapsedForThreads,
} from "./ui-layout-store";

// Helper: reset store to initial state before each test
beforeEach(() => {
  uiLayoutStore.reset();
});

describe("uiLayoutStore", () => {
  // -----------------------------------------------------------------------
  // Overlay
  // -----------------------------------------------------------------------
  describe("overlay", () => {
    it("should open overlay and close menus", () => {
      // Set menus open first
      setUserMenuOpen(true);
      setActiveWorkspaceMenuId("ws-1");

      openOverlay("settings", "general");

      const state = uiLayoutStore.getState();
      expect(state.activeOverlay).toBe("settings");
      expect(state.activeSettingsCategory).toBe("general");
      expect(state.isUserMenuOpen).toBe(false);
      expect(state.activeWorkspaceMenuId).toBeNull();
      expect(state.openSettingsSection).toBeNull();
    });

    it("should close overlay", () => {
      openOverlay("settings");
      closeOverlay();

      const state = uiLayoutStore.getState();
      expect(state.activeOverlay).toBeNull();
      expect(state.activeSettingsCategory).toBe("general");
      expect(state.openSettingsSection).toBeNull();
    });
  });

  // -----------------------------------------------------------------------
  // Panel visibility
  // -----------------------------------------------------------------------
  describe("panel visibility", () => {
    it("should toggle sidebar", () => {
      const initial = uiLayoutStore.getState().panelVisibility.isSidebarOpen;
      toggleSidebar();
      expect(uiLayoutStore.getState().panelVisibility.isSidebarOpen).toBe(!initial);
    });

    it("should toggle drawer", () => {
      const initial = uiLayoutStore.getState().panelVisibility.isDrawerOpen;
      toggleDrawer();
      expect(uiLayoutStore.getState().panelVisibility.isDrawerOpen).toBe(!initial);
    });

    it("should set sidebar and drawer open explicitly", () => {
      setSidebarOpen(true);
      expect(uiLayoutStore.getState().panelVisibility.isSidebarOpen).toBe(true);
      setSidebarOpen(false);
      expect(uiLayoutStore.getState().panelVisibility.isSidebarOpen).toBe(false);

      setDrawerOpen(true);
      expect(uiLayoutStore.getState().panelVisibility.isDrawerOpen).toBe(true);
      setDrawerOpen(false);
      expect(uiLayoutStore.getState().panelVisibility.isDrawerOpen).toBe(false);
    });
  });

  // -----------------------------------------------------------------------
  // Terminal layout
  // -----------------------------------------------------------------------
  describe("terminal layout", () => {
    it("should set terminal collapsed state per thread key", () => {
      setTerminalCollapsed("thread-1", false);
      expect(uiLayoutStore.getState().terminalCollapsedByThreadKey["thread-1"]).toBe(false);

      // Setting to true (default) removes the key
      setTerminalCollapsed("thread-1", true);
      expect(uiLayoutStore.getState().terminalCollapsedByThreadKey["thread-1"]).toBeUndefined();
    });

    it("should set terminal height", () => {
      setTerminalHeight(400);
      expect(uiLayoutStore.getState().terminalHeight).toBe(400);
    });

    it("should remove terminal collapsed entries for threads", () => {
      setTerminalCollapsed("thread-a", false);
      setTerminalCollapsed("thread-b", false);
      setTerminalCollapsed("thread-c", false);

      removeTerminalCollapsedForThreads(new Set(["thread-a", "thread-c"]));

      const state = uiLayoutStore.getState();
      expect(state.terminalCollapsedByThreadKey["thread-a"]).toBeUndefined();
      expect(state.terminalCollapsedByThreadKey["thread-b"]).toBe(false);
      expect(state.terminalCollapsedByThreadKey["thread-c"]).toBeUndefined();
    });
  });

  // -----------------------------------------------------------------------
  // Menu state
  // -----------------------------------------------------------------------
  describe("menu state", () => {
    it("should toggle user menu and clear settings section", () => {
      toggleUserMenu();
      expect(uiLayoutStore.getState().isUserMenuOpen).toBe(true);
      expect(uiLayoutStore.getState().openSettingsSection).toBeNull();

      toggleUserMenu();
      expect(uiLayoutStore.getState().isUserMenuOpen).toBe(false);
    });

    it("should set user menu open explicitly", () => {
      setUserMenuOpen(true);
      expect(uiLayoutStore.getState().isUserMenuOpen).toBe(true);
      setUserMenuOpen(false);
      expect(uiLayoutStore.getState().isUserMenuOpen).toBe(false);
    });

    it("should set workspace menu id", () => {
      setActiveWorkspaceMenuId("ws-1");
      expect(uiLayoutStore.getState().activeWorkspaceMenuId).toBe("ws-1");
      setActiveWorkspaceMenuId(null);
      expect(uiLayoutStore.getState().activeWorkspaceMenuId).toBeNull();
    });
  });

  // -----------------------------------------------------------------------
  // Onboarding / Diff
  // -----------------------------------------------------------------------
  describe("onboarding and diff selection", () => {
    it("should set showOnboarding", () => {
      setShowOnboarding(true);
      expect(uiLayoutStore.getState().showOnboarding).toBe(true);
      setShowOnboarding(false);
      expect(uiLayoutStore.getState().showOnboarding).toBe(false);
    });

    it("should set selectedDiffSelection", () => {
      // GitDiffSelection is a complex union type from source-control-panels.
      // Verify the store accepts null correctly.
      setSelectedDiffSelection(null);
      expect(uiLayoutStore.getState().selectedDiffSelection).toBeNull();
    });
  });

  // -----------------------------------------------------------------------
  // Auto-persist: panelVisibility → localStorage
  // -----------------------------------------------------------------------
  describe("auto-persist", () => {
    it("should persist panelVisibility to localStorage", () => {
      // jsdom or similar DOM environment needed for localStorage
      if (typeof window === "undefined" || !window.localStorage) {
        return; // Skip in environments without localStorage
      }

      // Trigger a change to fire the subscriber
      toggleSidebar();

      const stored = window.localStorage.getItem("tiy-agent-panel-visibility");
      expect(stored).not.toBeNull();
      const parsed = JSON.parse(stored!);
      expect(typeof parsed.isSidebarOpen).toBe("boolean");
      expect(typeof parsed.isDrawerOpen).toBe("boolean");
    });
  });
});
