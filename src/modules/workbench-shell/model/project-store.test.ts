import { describe, it, expect, beforeEach } from "vitest";
import {
  projectStore,
  selectProject,
  addRecentProject,
  setTerminalBinding,
  removeTerminalBinding,
  removeTerminalBindingForThread,
  setWorkspaceBinding,
  removeWorkspaceBindingForWorkspace,
  setBootstrapError,
} from "./project-store";
import type { ProjectOption } from "@/modules/workbench-shell/model/types";

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

function makeProject(overrides: Partial<ProjectOption> = {}): ProjectOption {
  return {
    id: "ws-1",
    name: "test-project",
    path: "/path/to/project",
    lastOpenedLabel: "2h ago",
    ...overrides,
  };
}

beforeEach(() => {
  projectStore.reset();
});

// ---------------------------------------------------------------------------
// projectStore
// ---------------------------------------------------------------------------

describe("projectStore", () => {
  describe("selectProject", () => {
    it("sets the selected project", () => {
      const project = makeProject();
      selectProject(project);
      expect(projectStore.getState().selectedProject).toEqual(project);
    });

    it("adds the project to recent projects", () => {
      const project = makeProject();
      selectProject(project);
      const recent = projectStore.getState().recentProjects;
      expect(recent).toHaveLength(1);
      expect(recent[0].id).toBe("ws-1");
    });

    it("deduplicates by id in recent projects", () => {
      const project = makeProject();
      selectProject(project);
      selectProject(project);
      expect(projectStore.getState().recentProjects).toHaveLength(1);
    });

    it("caps recent projects at 6 entries", () => {
      for (let i = 0; i < 10; i++) {
        selectProject(makeProject({ id: `ws-${i}`, path: `/p${i}` }));
      }
      expect(projectStore.getState().recentProjects.length).toBeLessThanOrEqual(6);
    });

    it("sets selectedProject to null when passed null", () => {
      const project = makeProject();
      selectProject(project);
      selectProject(null);
      expect(projectStore.getState().selectedProject).toBeNull();
    });
  });

  describe("addRecentProject", () => {
    it("adds a project to recent projects without changing selection", () => {
      const project = makeProject();
      addRecentProject(project);
      expect(projectStore.getState().recentProjects).toHaveLength(1);
      expect(projectStore.getState().selectedProject).toBeNull();
    });
  });

  describe("terminal bindings", () => {
    it("setTerminalBinding sets a binding", () => {
      setTerminalBinding("key-1", "thread-1");
      expect(projectStore.getState().terminalThreadBindings).toEqual({
        "key-1": "thread-1",
      });
    });

    it("setTerminalBinding is idempotent for same value", () => {
      setTerminalBinding("key-1", "thread-1");
      setTerminalBinding("key-1", "thread-1");
      expect(projectStore.getState().terminalThreadBindings).toEqual({
        "key-1": "thread-1",
      });
    });

    it("setTerminalBinding updates an existing key", () => {
      setTerminalBinding("key-1", "thread-1");
      setTerminalBinding("key-1", "thread-2");
      expect(projectStore.getState().terminalThreadBindings).toEqual({
        "key-1": "thread-2",
      });
    });

    it("removeTerminalBinding removes a key", () => {
      setTerminalBinding("key-1", "thread-1");
      setTerminalBinding("key-2", "thread-2");
      removeTerminalBinding("key-1");
      expect(projectStore.getState().terminalThreadBindings).toEqual({
        "key-2": "thread-2",
      });
    });

    it("removeTerminalBinding is a no-op for missing key", () => {
      setTerminalBinding("key-1", "thread-1");
      removeTerminalBinding("key-2");
      expect(projectStore.getState().terminalThreadBindings).toEqual({
        "key-1": "thread-1",
      });
    });

    it("removeTerminalBindingForThread removes all bindings for a thread", () => {
      setTerminalBinding("key-a", "thread-1");
      setTerminalBinding("key-b", "thread-1");
      setTerminalBinding("key-c", "thread-2");
      removeTerminalBindingForThread("thread-1");
      expect(projectStore.getState().terminalThreadBindings).toEqual({
        "key-c": "thread-2",
      });
    });

    it("removeTerminalBindingForThread is a no-op for nonexistent thread", () => {
      setTerminalBinding("key-a", "thread-1");
      removeTerminalBindingForThread("thread-x");
      expect(projectStore.getState().terminalThreadBindings).toEqual({
        "key-a": "thread-1",
      });
    });
  });

  describe("workspace bindings", () => {
    it("setWorkspaceBinding sets a path→workspaceId binding", () => {
      setWorkspaceBinding("/path/a", "ws-1");
      expect(projectStore.getState().terminalWorkspaceBindings).toEqual({
        "/path/a": "ws-1",
      });
    });

    it("setWorkspaceBinding can set multiple paths for same workspace", () => {
      setWorkspaceBinding("/path/a", "ws-1");
      setWorkspaceBinding("/path/alias", "ws-1");
      expect(projectStore.getState().terminalWorkspaceBindings).toEqual({
        "/path/a": "ws-1",
        "/path/alias": "ws-1",
      });
    });

    it("removeWorkspaceBindingForWorkspace removes all bindings for a workspace", () => {
      setWorkspaceBinding("/path/a", "ws-1");
      setWorkspaceBinding("/path/b", "ws-2");
      removeWorkspaceBindingForWorkspace("ws-1");
      expect(projectStore.getState().terminalWorkspaceBindings).toEqual({
        "/path/b": "ws-2",
      });
    });
  });

  describe("bootstrap error", () => {
    it("setBootstrapError sets and clears the error", () => {
      expect(projectStore.getState().terminalBootstrapError).toBeNull();
      setBootstrapError("Something went wrong");
      expect(projectStore.getState().terminalBootstrapError).toBe("Something went wrong");
      setBootstrapError(null);
      expect(projectStore.getState().terminalBootstrapError).toBeNull();
    });
  });
});
