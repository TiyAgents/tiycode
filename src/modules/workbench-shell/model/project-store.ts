import { createStore, useStore as useStoreBase, shallowEqual } from "@/shared/lib/create-store";
import type { ProjectOption } from "@/modules/workbench-shell/model/types";
import { mergeRecentProjects } from "@/modules/workbench-shell/model/helpers";

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

export interface ProjectStoreState {
  /** Currently selected project (workspace) for new-thread mode. */
  selectedProject: ProjectOption | null;
  /** Recent projects list (sidebar + new-thread empty state). */
  recentProjects: ProjectOption[];
  /** Workspace binding key → thread ID. Maps new-thread workspace keys
   *  (e.g. `${workspaceId}:new-thread`) to their bound terminal thread IDs. */
  terminalThreadBindings: Record<string, string>;
  /** Project path → workspace ID reverse mapping (includes aliases). */
  terminalWorkspaceBindings: Record<string, string>;
  /** Terminal/workbench initialization error message. */
  terminalBootstrapError: string | null;
}

// ---------------------------------------------------------------------------
// Store
// ---------------------------------------------------------------------------

export const projectStore = createStore<ProjectStoreState>({
  selectedProject: null,
  recentProjects: [],
  terminalThreadBindings: {},
  terminalWorkspaceBindings: {},
  terminalBootstrapError: null,
});

// ---------------------------------------------------------------------------
// React hook (re-export for convenience)
// ---------------------------------------------------------------------------

export { useStoreBase as useStore, shallowEqual };

// ---------------------------------------------------------------------------
// Actions — Project selection
// ---------------------------------------------------------------------------

/**
 * Select a project and add it to recent projects (deduplicated, capped).
 */
export function selectProject(project: ProjectOption | null): void {
  projectStore.setState((prev) => {
    const updates: Partial<ProjectStoreState> = { selectedProject: project };
    if (project) {
      updates.recentProjects = mergeRecentProjects(prev.recentProjects, {
        ...project,
        lastOpenedLabel: "Just now", // label will be localized by consumers
      });
    }
    return updates;
  });
}

// ---------------------------------------------------------------------------
// Actions — Recent projects
// ---------------------------------------------------------------------------

/** Add a project to recent projects (deduplicated, capped). */
export function addRecentProject(project: ProjectOption): void {
  projectStore.setState((prev) => ({
    recentProjects: mergeRecentProjects(prev.recentProjects, project),
  }));
}

// ---------------------------------------------------------------------------
// Actions — Terminal bindings
// ---------------------------------------------------------------------------

/** Set a terminal-thread binding for a workspace key. */
export function setTerminalBinding(
  workspaceKey: string,
  threadId: string,
): void {
  projectStore.setState((prev) => {
    if (prev.terminalThreadBindings[workspaceKey] === threadId) {
      return {};
    }
    return {
      terminalThreadBindings: {
        ...prev.terminalThreadBindings,
        [workspaceKey]: threadId,
      },
    };
  });
}

/** Remove a terminal-thread binding for a workspace key. */
export function removeTerminalBinding(workspaceKey: string): void {
  projectStore.setState((prev) => {
    if (!(workspaceKey in prev.terminalThreadBindings)) {
      return {};
    }
    const next = { ...prev.terminalThreadBindings };
    delete next[workspaceKey];
    return { terminalThreadBindings: next };
  });
}

/**
 * Remove all terminal bindings that reference a specific thread ID.
 * Used during thread deletion to clean up stale bindings.
 */
export function removeTerminalBindingForThread(threadId: string): void {
  projectStore.setState((prev) => {
    const next: Record<string, string> = {};
    let changed = false;
    for (const [key, tid] of Object.entries(prev.terminalThreadBindings)) {
      if (tid === threadId) {
        changed = true;
      } else {
        next[key] = tid;
      }
    }
    return changed ? { terminalThreadBindings: next } : {};
  });
}

// ---------------------------------------------------------------------------
// Actions — Workspace bindings (path → workspaceId)
// ---------------------------------------------------------------------------

/** Set a path → workspaceId binding (includes alias paths). */
export function setWorkspaceBinding(path: string, workspaceId: string): void {
  projectStore.setState((prev) => ({
    terminalWorkspaceBindings: {
      ...prev.terminalWorkspaceBindings,
      [path]: workspaceId,
    },
  }));
}

/**
 * Remove all workspace bindings that map to a specific workspace ID.
 * Used during workspace removal.
 */
export function removeWorkspaceBindingForWorkspace(workspaceId: string): void {
  projectStore.setState((prev) => {
    const next: Record<string, string> = {};
    let changed = false;
    for (const [path, wsId] of Object.entries(
      prev.terminalWorkspaceBindings,
    )) {
      if (wsId === workspaceId) {
        changed = true;
      } else {
        next[path] = wsId;
      }
    }
    return changed ? { terminalWorkspaceBindings: next } : {};
  });
}

// ---------------------------------------------------------------------------
// Actions — Error
// ---------------------------------------------------------------------------

export function setBootstrapError(error: string | null): void {
  projectStore.setState({ terminalBootstrapError: error });
}
