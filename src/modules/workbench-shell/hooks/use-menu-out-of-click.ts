/**
 * Click-away handlers for the user menu and workspace context menu.
 *
 * Subscribes to uiLayoutStore for menu-open state and closes the
 * corresponding menu when a mousedown lands outside its ref container.
 * Replaces two useEffect blocks in DashboardWorkbench.
 */
import { useEffect, type RefObject } from "react";
import { useStore } from "@/shared/lib/create-store";
import {
  uiLayoutStore,
  setUserMenuOpen,
  setActiveWorkspaceMenuId,
  setOpenSettingsSection,
} from "@/modules/workbench-shell/model/ui-layout-store";

export interface MenuOutOfClickRefs {
  userMenu: RefObject<HTMLDivElement | null>;
  workspaceMenu: RefObject<HTMLDivElement | null>;
}

export function useMenuOutOfClick(refs: MenuOutOfClickRefs): void {
  const isUserMenuOpen = useStore(uiLayoutStore, (s) => s.isUserMenuOpen);
  const activeWorkspaceMenuId = useStore(uiLayoutStore, (s) => s.activeWorkspaceMenuId);

  // ── User menu click-away ──
  useEffect(() => {
    if (!isUserMenuOpen || typeof window === "undefined") {
      return;
    }

    const handlePointerDown = (event: MouseEvent) => {
      const target = event.target as Node | null;
      if (target && refs.userMenu.current?.contains(target)) {
        return;
      }
      setUserMenuOpen(false);
      setOpenSettingsSection(null);
    };

    window.addEventListener("mousedown", handlePointerDown);
    return () => window.removeEventListener("mousedown", handlePointerDown);
  }, [isUserMenuOpen, refs.userMenu]);

  // ── Workspace menu click-away ──
  useEffect(() => {
    if (!activeWorkspaceMenuId || typeof window === "undefined") {
      return;
    }

    const handlePointerDown = (event: MouseEvent) => {
      const target = event.target as Node | null;
      if (target && refs.workspaceMenu.current?.contains(target)) {
        return;
      }
      setActiveWorkspaceMenuId(null);
    };

    window.addEventListener("mousedown", handlePointerDown);
    return () => window.removeEventListener("mousedown", handlePointerDown);
  }, [activeWorkspaceMenuId, refs.workspaceMenu]);
}
