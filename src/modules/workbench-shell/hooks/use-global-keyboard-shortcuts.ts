/**
 * Global keyboard shortcuts:
 * - Cmd+A / Ctrl+A: select all in the main content or overlay area
 * - Cmd+, / Ctrl+,: open settings (macOS only)
 */
import { useEffect, RefObject } from "react";
import {
  isEditableSelectionTarget,
  isNodeInsideContainer,
  selectContainerContents,
} from "@/modules/workbench-shell/model/helpers";

export interface GlobalKeyboardShortcutRefs {
  mainContent: RefObject<HTMLElement | null>;
  overlay: RefObject<HTMLElement | null>;
}

export interface GlobalKeyboardShortcutOptions {
  /** Whether we're on macOS (Cmd+, shortcut). */
  isMacOS: boolean;
  /** Callback to open settings overlay. */
  onOpenSettings: () => void;
}

export function useGlobalKeyboardShortcuts(
  refs: GlobalKeyboardShortcutRefs,
  options: GlobalKeyboardShortcutOptions,
): void {
  useEffect(() => {
    if (typeof window === "undefined") return;

    const handleKeyDown = (event: KeyboardEvent) => {
      const hasModifier = event.metaKey || event.ctrlKey;
      if (!hasModifier || event.altKey) return;

      // Cmd+A / Ctrl+A: select all in content area
      if (event.key.toLowerCase() === "a") {
        if (isEditableSelectionTarget(event.target)) return;

        const selection = window.getSelection();
        const selectionInsideOverlay =
          isNodeInsideContainer(
            refs.overlay.current,
            selection?.anchorNode ?? null,
          ) ||
          isNodeInsideContainer(
            refs.overlay.current,
            selection?.focusNode ?? null,
          );

        if (selectionInsideOverlay) {
          event.preventDefault();
          if (refs.overlay.current) {
            selectContainerContents(refs.overlay.current);
          }
          return;
        }

        const selectionInsideMain =
          isNodeInsideContainer(
            refs.mainContent.current,
            selection?.anchorNode ?? null,
          ) ||
          isNodeInsideContainer(
            refs.mainContent.current,
            selection?.focusNode ?? null,
          );

        if (selectionInsideMain) {
          event.preventDefault();
          if (refs.mainContent.current) {
            selectContainerContents(refs.mainContent.current);
          }
        }
        return;
      }

      // Cmd+, / Ctrl+,: open settings (macOS convention)
      if (event.key === "," && options.isMacOS) {
        event.preventDefault();
        options.onOpenSettings();
        return;
      }
    };

    window.addEventListener("keydown", handleKeyDown);
    return () => window.removeEventListener("keydown", handleKeyDown);
  }, [refs.mainContent, refs.overlay, options.isMacOS, options.onOpenSettings]);
}
