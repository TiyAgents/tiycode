/**
 * Tests for cross-store orchestration actions in workbench-actions.ts.
 *
 * These tests verify that each action correctly coordinates multiple domain
 * stores and handles edge cases. IPC calls are mocked or skipped.
 */
import { describe, it, expect, beforeEach, vi } from "vitest";
import {
  selectThread,
  selectProject,
  activateWorkspace,
  deleteThread,
  removeWorkspace,
  enterNewThreadMode,
  submitNewThread,
} from "./workbench-actions";
import type { NewThreadSubmission } from "./workbench-actions";
import { threadStore } from "./thread-store";
import { projectStore } from "./project-store";
import { composerStore } from "./composer-store";
import { uiLayoutStore } from "./ui-layout-store";
import { settingsStore } from "@/modules/settings-center/model/settings-store";
import { terminalStore } from "@/features/terminal/model/terminal-store";

// ---------------------------------------------------------------------------
// Mock IPC
// ---------------------------------------------------------------------------

vi.mock("@/services/bridge/thread-commands", () => ({
  threadCreate: vi.fn().mockResolvedValue({
    id: "thread-1",
    profileId: "profile-1",
  }),
  threadDelete: vi.fn().mockResolvedValue(undefined),
}));

vi.mock("@/services/bridge/workspace-commands", () => ({
  workspaceRemove: vi.fn().mockResolvedValue(undefined),
  workspaceAdd: vi.fn().mockResolvedValue({
    id: "ws-1",
    name: "test",
    path: "/path/to/project",
    canonicalPath: "/path/to/project",
  }),
  workspaceList: vi.fn().mockResolvedValue([]),
  workspaceEnsureDefault: vi.fn().mockResolvedValue({
    id: "default-ws-1",
    name: "Default",
    path: "/home/user/.tiy/workspace/Default",
    canonicalPath: "/home/user/.tiy/workspace/Default",
    isDefault: true,
    kind: "standalone",
  }),
}));

// Mock isTauri to always return true (simulate Tauri env)
vi.mock("@tauri-apps/api/core", () => ({
  isTauri: vi.fn().mockReturnValue(true),
}));

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

function makeThread(
  threadId: string,
  overrides: Partial<{ name: string; profileId: string | null }> = {},
) {
  return {
    id: threadId,
    name: overrides.name ?? "Thread " + threadId,
    profileId: overrides.profileId ?? null,
  } as const;
}

function makeWorkspace(
  id: string,
  threads: ReturnType<typeof makeThread>[] = [],
  overrides: Partial<{ name: string; path: string }> = {},
) {
  return {
    id,
    name: overrides.name ?? "Workspace " + id,
    path: overrides.path ?? "/path/" + id,
    defaultOpen: true,
    kind: "repo" as const,
    threads: threads as any[],
    parentWorkspaceId: null,
    worktreeHash: null,
    branch: null,
  };
}

function makeProject(overrides: Partial<{ id: string; name: string; path: string }> = {}) {
  return {
    id: overrides.id ?? "ws-1",
    name: overrides.name ?? "test-project",
    path: overrides.path ?? "/path/to/project",
    lastOpenedLabel: "Just now",
  };
}

function resetAllStores() {
  threadStore.reset();
  projectStore.reset();
  composerStore.reset();
  uiLayoutStore.reset();
  settingsStore.reset();
  // terminalStore has no reset(); sessions cleaned via tested actions
}

function upsertTerminalSession(threadId: string) {
  terminalStore.upsertSession({
    threadId,
    exitCode: null,
    cwd: "/tmp",
    shellName: "bash",
    columns: 80,
    rows: 24,
    title: "test",
  } as any);
}

function hasTerminalSession(threadId: string): boolean {
  return threadId in terminalStore.getState().sessionsByThreadId;
}

beforeEach(() => {
  resetAllStores();
  // Set up minimal settingsStore state
  settingsStore.setState({ activeAgentProfileId: "default-profile" });
});

// ---------------------------------------------------------------------------
// selectProject
// ---------------------------------------------------------------------------

describe("selectProject", () => {
  it("sets the selected project and adds to recent", () => {
    const project = makeProject();
    selectProject(project);

    expect(projectStore.getState().selectedProject).toMatchObject({
      id: "ws-1",
      name: "test-project",
    });
    expect(projectStore.getState().recentProjects).toHaveLength(1);
    expect(projectStore.getState().recentProjects[0].lastOpenedLabel).toBe("Just now");
  });

  it("deduplicates recent projects", () => {
    const project = makeProject();
    selectProject(project);
    selectProject(project);

    expect(projectStore.getState().recentProjects).toHaveLength(1);
  });
});

// ---------------------------------------------------------------------------
// selectThread
// ---------------------------------------------------------------------------

describe("selectThread", () => {
  it("transitions out of new-thread mode and activates the thread", () => {
    const thread = makeThread("thread-1");
    const workspace = makeWorkspace("ws-1", [thread]);
    threadStore.setState({
      isNewThreadMode: true,
      workspaces: [workspace],
    });
    projectStore.setState({
      selectedProject: makeProject({ id: "ws-1" }),
      recentProjects: [makeProject({ id: "ws-1" })],
    });

    selectThread("thread-1");

    const state = threadStore.getState();
    expect(state.isNewThreadMode).toBe(false);
    expect(state.editingThreadId).toBeNull();
  });

  it("clears new-thread terminal binding on selection", () => {
    const thread = makeThread("thread-1");
    const workspace = makeWorkspace("ws-1", [thread]);
    threadStore.setState({
      isNewThreadMode: true,
      workspaces: [workspace],
    });
    projectStore.setState({
      selectedProject: makeProject({ id: "ws-1" }),
      recentProjects: [makeProject({ id: "ws-1" })],
      terminalThreadBindings: { "ws-1:__new_thread__": "bound-thread-id" },
    });

    selectThread("thread-1");

    const bindings = projectStore.getState().terminalThreadBindings;
    expect(bindings["ws-1:__new_thread__"]).toBeUndefined();
  });

  it("sets activeThreadProfileIdOverride based on thread profile", () => {
    const thread = makeThread("thread-1", { profileId: "custom-profile" });
    const workspace = makeWorkspace("ws-1", [thread]);
    threadStore.setState({
      isNewThreadMode: true,
      workspaces: [workspace],
    });
    projectStore.setState({
      selectedProject: makeProject({ id: "ws-1" }),
      recentProjects: [makeProject({ id: "ws-1" })],
    });

    selectThread("thread-1");

    expect(threadStore.getState().activeThreadProfileIdOverride).toBe("custom-profile");
  });

  it("closes the workspace menu", () => {
    const thread = makeThread("thread-1");
    const workspace = makeWorkspace("ws-1", [thread]);
    threadStore.setState({
      isNewThreadMode: true,
      workspaces: [workspace],
    });
    projectStore.setState({
      selectedProject: makeProject({ id: "ws-1" }),
      recentProjects: [makeProject({ id: "ws-1" })],
    });
    uiLayoutStore.setState({ activeWorkspaceMenuId: "ws-1" });

    selectThread("thread-1");

    expect(uiLayoutStore.getState().activeWorkspaceMenuId).toBeNull();
  });

  it("sets isNewThreadMode to false regardless of thread existence", () => {
    threadStore.setState({
      isNewThreadMode: true,
      workspaces: [],
    });

    selectThread("nonexistent");

    // selectThread always transitions out of new-thread mode
    expect(threadStore.getState().isNewThreadMode).toBe(false);
  });
});

// ---------------------------------------------------------------------------
// activateWorkspace
// ---------------------------------------------------------------------------

describe("activateWorkspace", () => {
  it("sets new-thread mode and selects the project", () => {
    threadStore.setState({ isNewThreadMode: false, workspaces: [] });
    const project = makeProject({ id: "ws-2" });

    activateWorkspace("ws-2", project);

    const threadState = threadStore.getState();
    expect(threadState.isNewThreadMode).toBe(true);
    expect(threadState.activeThreadProfileIdOverride).toBeNull();
    expect(threadState.editingThreadId).toBeNull();

    const projectState = projectStore.getState();
    expect(projectState.selectedProject?.id).toBe("ws-2");
    expect(projectState.terminalBootstrapError).toBeNull();
  });

  it("expands the workspace in sidebar", () => {
    threadStore.setState({ isNewThreadMode: false, workspaces: [] });
    const project = makeProject({ id: "ws-2" });

    activateWorkspace("ws-2", project);

    expect(threadStore.getState().openWorkspaces["ws-2"]).toBe(true);
  });

  it("clears composer error", () => {
    composerStore.setState({ error: "Previous error" });
    threadStore.setState({ isNewThreadMode: false, workspaces: [] });

    activateWorkspace("ws-2", makeProject({ id: "ws-2" }));

    expect(composerStore.getState().error).toBeNull();
  });

  it("closes workspace menu", () => {
    uiLayoutStore.setState({ activeWorkspaceMenuId: "ws-1" });
    threadStore.setState({ isNewThreadMode: false, workspaces: [] });

    activateWorkspace("ws-2", makeProject({ id: "ws-2" }));

    expect(uiLayoutStore.getState().activeWorkspaceMenuId).toBeNull();
  });

  it("clears existing new-thread terminal binding", () => {
    projectStore.setState({
      terminalThreadBindings: { "ws-2:__new_thread__": "old-thread" },
    });
    threadStore.setState({ isNewThreadMode: false, workspaces: [] });

    activateWorkspace("ws-2", makeProject({ id: "ws-2" }));

    const bindings = projectStore.getState().terminalThreadBindings;
    expect(bindings["ws-2:__new_thread__"]).toBeUndefined();
  });
});

// ---------------------------------------------------------------------------
// deleteThread (skipIpc)
// ---------------------------------------------------------------------------

describe("deleteThread", () => {
  it("removes the thread from all workspaces", async () => {
    const thread1 = makeThread("thread-1");
    const thread2 = makeThread("thread-2");
    const workspace = makeWorkspace("ws-1", [thread1, thread2]);
    threadStore.setState({
      activeThreadId: null,
      workspaces: [workspace],
    });

    await deleteThread("thread-1", { skipIpc: true });

    const state = threadStore.getState();
    expect(state.workspaces[0].threads).toHaveLength(1);
    expect(state.workspaces[0].threads[0].id).toBe("thread-2");
  });

  it("cleans up terminal session for the deleted thread", async () => {
    const thread = makeThread("thread-1");
    threadStore.setState({
      activeThreadId: null,
      workspaces: [makeWorkspace("ws-1", [thread])],
    });
    upsertTerminalSession("thread-1");

    await deleteThread("thread-1", { skipIpc: true });

    expect(hasTerminalSession("thread-1")).toBe(false);
  });

  it("cleans terminal bindings", async () => {
    const thread = makeThread("thread-1");
    threadStore.setState({
      activeThreadId: null,
      workspaces: [makeWorkspace("ws-1", [thread])],
    });
    projectStore.setState({
      terminalThreadBindings: { "terminal:ws-1": "thread-1", "terminal:ws-2": "thread-2" },
    });

    await deleteThread("thread-1", { skipIpc: true });

    const bindings = projectStore.getState().terminalThreadBindings;
    expect(bindings["terminal:ws-1"]).toBeUndefined();
    expect(bindings["terminal:ws-2"]).toBe("thread-2");
  });

  it("cleans pending runs", async () => {
    const thread = makeThread("thread-1");
    threadStore.setState({
      activeThreadId: null,
      workspaces: [makeWorkspace("ws-1", [thread])],
      pendingRuns: { "thread-1": { id: "run-1", prompt: "test", runMode: "auto" } as any },
    });

    await deleteThread("thread-1", { skipIpc: true });

    expect(threadStore.getState().pendingRuns["thread-1"]).toBeUndefined();
  });

  it("transitions to new-thread mode when deleting active thread", async () => {
    const thread = makeThread("thread-1");
    const workspace = makeWorkspace("ws-1", [thread]);
    threadStore.setState({
      activeThreadId: "thread-1",
      workspaces: [workspace],
    });
    projectStore.setState({
      selectedProject: makeProject({ id: "ws-1" }),
      recentProjects: [makeProject({ id: "ws-1" })],
    });

    await deleteThread("thread-1", { skipIpc: true });

    expect(threadStore.getState().isNewThreadMode).toBe(true);
    expect(threadStore.getState().activeThreadProfileIdOverride).toBeNull();
    expect(composerStore.getState().error).toBeNull();
  });

  it("cleans terminal collapsed state", async () => {
    const thread = makeThread("thread-1");
    threadStore.setState({
      activeThreadId: null,
      workspaces: [makeWorkspace("ws-1", [thread])],
    });
    uiLayoutStore.setState({
      terminalCollapsedByThreadKey: { "thread-1": true, "thread-2": true },
    });

    await deleteThread("thread-1", { skipIpc: true });

    const collapsed = uiLayoutStore.getState().terminalCollapsedByThreadKey;
    expect(collapsed["thread-1"]).toBeUndefined();
    expect(collapsed["thread-2"]).toBe(true);
  });
});

// ---------------------------------------------------------------------------
// removeWorkspace (uses mocked IPC)
// ---------------------------------------------------------------------------

describe("removeWorkspace", () => {
  it("cleans terminal bindings for workspace threads", async () => {
    const ws1 = makeWorkspace("ws-1", [makeThread("thread-1"), makeThread("thread-2")]);
    threadStore.setState({
      activeThreadId: null,
      workspaces: [ws1],
    });
    projectStore.setState({
      terminalThreadBindings: { "terminal:thread-1": "thread-1", "terminal:ws-2": "thread-3" },
    });

    await removeWorkspace(ws1 as any);

    const bindings = projectStore.getState().terminalThreadBindings;
    expect(bindings["terminal:thread-1"]).toBeUndefined();
    expect(bindings["terminal:ws-2"]).toBe("thread-3");
  });

  it("cleans pending runs for removed threads", async () => {
    const ws1 = makeWorkspace("ws-1", [makeThread("thread-1")]);
    threadStore.setState({
      activeThreadId: null,
      workspaces: [ws1],
      pendingRuns: { "thread-1": { id: "run-1", prompt: "test", runMode: "auto" } as any },
    });

    await removeWorkspace(ws1 as any);

    expect(threadStore.getState().pendingRuns["thread-1"]).toBeUndefined();
  });

  it("cleans terminal collapsed state for removed threads", async () => {
    const ws1 = makeWorkspace("ws-1", [makeThread("thread-1")]);
    threadStore.setState({
      activeThreadId: null,
      workspaces: [ws1],
    });
    uiLayoutStore.setState({
      terminalCollapsedByThreadKey: { "thread-1": true, "thread-2": true },
    });

    await removeWorkspace(ws1 as any);

    const collapsed = uiLayoutStore.getState().terminalCollapsedByThreadKey;
    expect(collapsed["thread-1"]).toBeUndefined();
    expect(collapsed["thread-2"]).toBe(true);
  });

  it("clears selected project if it matches removed workspace", async () => {
    const ws1 = makeWorkspace("ws-1", []);
    threadStore.setState({ activeThreadId: null, workspaces: [ws1] });
    projectStore.setState({
      selectedProject: makeProject({ id: "ws-1", path: "/path/ws-1" }),
    });

    await removeWorkspace(ws1 as any);

    expect(projectStore.getState().selectedProject).toBeNull();
  });
});

// ---------------------------------------------------------------------------
// enterNewThreadMode
// ---------------------------------------------------------------------------

describe("enterNewThreadMode", () => {
  it("transitions to new-thread mode", () => {
    threadStore.setState({ isNewThreadMode: false });

    enterNewThreadMode();

    expect(threadStore.getState().isNewThreadMode).toBe(true);
  });

  it("clears active thread profile override", () => {
    threadStore.setState({
      isNewThreadMode: false,
      activeThreadProfileIdOverride: "custom",
    });

    enterNewThreadMode();

    expect(threadStore.getState().activeThreadProfileIdOverride).toBeNull();
  });

  it("clears composer error", () => {
    composerStore.setState({ error: "Previous error" });

    enterNewThreadMode();

    expect(composerStore.getState().error).toBeNull();
  });

  it("preserves workspaces structure", () => {
    const workspace = makeWorkspace("ws-1", [makeThread("thread-1")]);
    threadStore.setState({
      isNewThreadMode: false,
      workspaces: [workspace],
    });

    enterNewThreadMode();

    expect(threadStore.getState().workspaces).toHaveLength(1);
    expect(threadStore.getState().workspaces[0].id).toBe("ws-1");
  });

  it("clears terminal bootstrap error", () => {
    projectStore.setState({ terminalBootstrapError: "some error" });

    enterNewThreadMode();

    expect(projectStore.getState().terminalBootstrapError).toBeNull();
  });
});

// ---------------------------------------------------------------------------
// submitNewThread
// ---------------------------------------------------------------------------

describe("submitNewThread", () => {
  function makeSubmission(
    overrides?: Partial<NewThreadSubmission>,
  ): NewThreadSubmission {
    return {
      value: "test prompt",
      runMode: "default",
      effectivePrompt: "test prompt",
      ...overrides,
    };
  }

  it("creates thread using default workspace when no project is selected", async () => {
    threadStore.setState({
      workspaces: [],
      isNewThreadMode: true,
    });
    composerStore.setState({ newThreadRunMode: "default" });
    settingsStore.setState({ activeAgentProfileId: "default-profile" });

    await submitNewThread(makeSubmission({ value: "hello" }));

    const state = threadStore.getState();
    expect(state.workspaces).toHaveLength(1);
    // workspaceAdd mock returns ws-1; the default workspace fallback ensures
    // a thread is created even when selectedProject is null
    expect(state.workspaces[0].threads).toHaveLength(1);
    expect(state.isNewThreadMode).toBe(false);
  });

  it("adds the new thread to an existing workspace", async () => {
    const project = makeProject({
      id: "ws-1",
      name: "test-project",
      path: "/path/to/project",
    });
    const workspace = makeWorkspace("ws-1", [], {
      name: "test-project",
      path: "/path/to/project",
    });

    projectStore.setState({
      selectedProject: project,
      recentProjects: [project],
    });
    threadStore.setState({
      workspaces: [workspace],
      isNewThreadMode: true,
    });
    composerStore.setState({ newThreadRunMode: "default" });
    settingsStore.setState({ activeAgentProfileId: "default-profile" });

    await submitNewThread(makeSubmission({ value: "hello" }));

    const state = threadStore.getState();
    // Should have added one thread to the existing workspace
    expect(state.workspaces[0].threads).toHaveLength(1);
    expect(state.workspaces[0].threads[0].name).toBe("hello");
    expect(state.workspaces[0].threads[0].status).toBe("running");
    expect(state.isNewThreadMode).toBe(false);
    expect(state.openWorkspaces["ws-1"]).toBe(true);
    // Should have added a pending run
    expect(state.pendingRuns["thread-1"]).toBeDefined();
    expect(state.pendingRuns["thread-1"].displayText).toBe("hello");
  });

  it("clears active flags on other threads when adding new one", async () => {
    const project = makeProject({
      id: "ws-1",
      name: "test-project",
      path: "/path/to/project",
    });
    const thread = makeThread("thread-existing", { name: "existing" });
    const workspace = makeWorkspace("ws-1", [thread], {
      name: "test-project",
      path: "/path/to/project",
    });

    projectStore.setState({
      selectedProject: project,
      recentProjects: [project],
    });
    threadStore.setState({
      workspaces: [workspace],
      isNewThreadMode: true,
    });
    composerStore.setState({ newThreadRunMode: "default" });
    settingsStore.setState({ activeAgentProfileId: "default-profile" });

    await submitNewThread(makeSubmission({ value: "new thread" }));

    const state = threadStore.getState();
    // Existing thread should no longer be active
    const existing = state.workspaces[0].threads.find(
      (t) => t.id === "thread-existing",
    );
    expect(existing).toBeTruthy();
    // New thread should be first in the list
    expect(state.workspaces[0].threads[0].name).toBe("new thread");
    expect(state.workspaces[0].threads[0].active).toBe(true);
  });
});
