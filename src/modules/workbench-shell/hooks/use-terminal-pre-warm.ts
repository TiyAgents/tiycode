/**
 * Pre-warms the terminal by creating a background thread for the selected
 * workspace when in new-thread mode. This ensures the terminal is ready
 * before the user submits their prompt.
 */
import { useEffect } from "react";
import { isTauri } from "@tauri-apps/api/core";
import { useStore } from "@/shared/lib/create-store";
import { threadStore } from "@/modules/workbench-shell/model/thread-store";
import { projectStore } from "@/modules/workbench-shell/model/project-store";
import { uiLayoutStore } from "@/modules/workbench-shell/model/ui-layout-store";
import { getInvokeErrorMessage } from "@/shared/lib/invoke-error";
import { getNewThreadTerminalBindingKey } from "@/modules/workbench-shell/ui/dashboard-workbench-logic";
import { getOrCreateNewThreadId } from "@/modules/workbench-shell/model/workbench-actions";

export function useTerminalPreWarm(): void {
  const isNewThreadMode = useStore(threadStore, (s) => s.isNewThreadMode);
  const selectedProject = useStore(projectStore, (s) => s.selectedProject);
  const terminalCollapsed = useStore(
    uiLayoutStore,
    (s) => {
      if (!selectedProject) return true;
      const key = getNewThreadTerminalBindingKey(selectedProject.id);
      return s.terminalCollapsedByThreadKey[key] ?? true;
    },
  );

  useEffect(() => {
    if (!isTauri() || !isNewThreadMode) return;
    if (!selectedProject) return;

    const workspaceId = selectedProject.id;
    if (!workspaceId) return;

    const bindingKey = getNewThreadTerminalBindingKey(workspaceId);
    const existingThreadId =
      projectStore.getState().terminalThreadBindings[bindingKey] ?? null;

    // Don't create if terminal is collapsed or a thread already exists
    if (terminalCollapsed || existingThreadId) return;

    getOrCreateNewThreadId(workspaceId).catch((error) => {
      projectStore.setState({
        terminalBootstrapError: getInvokeErrorMessage(
          error,
          "Failed to prepare terminal",
        ),
      });
    });
  }, [isNewThreadMode, terminalCollapsed, selectedProject]);
}
