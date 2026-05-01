/**
 * Ensures a backend workspace exists for the current project and injects
 * path → workspaceId bindings. Runs when currentProject changes.
 */
import { useEffect } from "react";
import { isTauri } from "@tauri-apps/api/core";
import { useStore } from "@/shared/lib/create-store";
import { projectStore } from "@/modules/workbench-shell/model/project-store";
import { getInvokeErrorMessage } from "@/shared/lib/invoke-error";
import { workspaceList, workspaceAdd } from "@/services/bridge/workspace-commands";
import {
  findWorkspaceByPath,
  buildWorkspaceBindingsForEntry,
} from "@/modules/workbench-shell/model/workspace-path-bindings";
import type { WorkspaceDto } from "@/shared/types/api";

export function useWorkspaceDiscovery(): void {
  const currentProject = useStore(projectStore, (s) => s.selectedProject);

  useEffect(() => {
    if (!isTauri() || !currentProject) return;

    // Check if a binding already exists
    const currentBindings = projectStore.getState().terminalWorkspaceBindings;
    if (currentBindings[currentProject.path]) return;

    let cancelled = false;
    projectStore.setState({ terminalBootstrapError: null });

    void workspaceList()
      .then(async (workspaces) => {
        const existing = findWorkspaceByPath(
          workspaces as unknown as WorkspaceDto[],
          currentProject.path,
        );
        if (existing) return existing;
        if (cancelled) return undefined;
        return workspaceAdd(currentProject.path, currentProject.name);
      })
      .then((workspace) => {
        if (cancelled || !workspace) return;
        projectStore.setState((prev) => ({
          terminalWorkspaceBindings: {
            ...prev.terminalWorkspaceBindings,
            ...buildWorkspaceBindingsForEntry(
              workspace as WorkspaceDto,
              currentProject.path,
            ),
          },
        }));
      })
      .catch((error) => {
        if (cancelled) return;
        projectStore.setState({
          terminalBootstrapError: getInvokeErrorMessage(
            error,
            "Failed to add workspace",
          ),
        });
      });

    return () => {
      cancelled = true;
    };
  }, [currentProject]);
}
