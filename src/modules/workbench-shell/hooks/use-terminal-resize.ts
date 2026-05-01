/**
 * Manages terminal height: clamps on window resize and handles drag-to-resize.
 */
import { useEffect, useCallback } from "react";
import { useStore } from "@/shared/lib/create-store";
import { uiLayoutStore } from "@/modules/workbench-shell/model/ui-layout-store";
import {
  DEFAULT_TERMINAL_HEIGHT,
  MIN_TERMINAL_HEIGHT,
} from "@/modules/workbench-shell/model/fixtures";

function getMaxTerminalHeight(): number {
  if (typeof window === "undefined") return DEFAULT_TERMINAL_HEIGHT;
  // Reserve space for top bar + prompt + minimum surface height
  const reserved = 300;
  return Math.max(MIN_TERMINAL_HEIGHT, window.innerHeight - reserved);
}

export function useTerminalResize(): {
  handleTerminalResizeStart: (
    event: React.MouseEvent<HTMLDivElement>,
  ) => void;
} {
  const terminalResize = useStore(uiLayoutStore, (s) => s.terminalResize);

  // Clamp terminal height on window resize
  useEffect(() => {
    if (typeof window === "undefined") return;

    const syncTerminalHeight = () => {
      const current = uiLayoutStore.getState().terminalHeight;
      const maxHeight = getMaxTerminalHeight();
      if (current > maxHeight) {
        uiLayoutStore.setState({ terminalHeight: maxHeight });
      }
    };

    syncTerminalHeight();
    window.addEventListener("resize", syncTerminalHeight);
    return () => window.removeEventListener("resize", syncTerminalHeight);
  }, []);

  // Handle drag-to-resize
  useEffect(() => {
    if (!terminalResize || typeof window === "undefined") return;

    const handleMouseMove = (event: MouseEvent) => {
      const deltaY = terminalResize.startY - event.clientY;
      const nextHeight = terminalResize.startHeight + deltaY;
      const clampedHeight = Math.min(
        getMaxTerminalHeight(),
        Math.max(MIN_TERMINAL_HEIGHT, nextHeight),
      );
      uiLayoutStore.setState({ terminalHeight: clampedHeight });
    };

    const handleMouseUp = () => {
      uiLayoutStore.setState({ terminalResize: null });
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

  const handleTerminalResizeStart = useCallback(
    (event: React.MouseEvent<HTMLDivElement>) => {
      if (event.button !== 0) return;
      event.preventDefault();
      const height = uiLayoutStore.getState().terminalHeight;
      uiLayoutStore.setState({
        terminalResize: { startY: event.clientY, startHeight: height },
      });
    },
    [],
  );

  return { handleTerminalResizeStart };
}
