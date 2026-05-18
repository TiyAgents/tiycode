import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const { fileReadMock, fileWriteMock } = vi.hoisted(() => ({
  fileReadMock: vi.fn(),
  fileWriteMock: vi.fn(),
}));

vi.mock("@/services/bridge/file-commands", () => ({
  fileRead: fileReadMock,
  fileWrite: fileWriteMock,
}));

import {
  closeAllTabs,
  closeTab,
  detectLanguage,
  type FileTab,
  fileEditorStore,
  getFileTabKey,
  isImageFile,
  isPreviewable,
  openFile,
  pinTab,
  reloadTab,
  saveFile,
  setPreviewMode,
  updateContent,
} from "./file-editor-store";

const makeTab = (overrides: Partial<FileTab> = {}): FileTab => ({
  workspaceId: "workspace-1",
  path: "file.txt",
  content: "content",
  originalContent: "content",
  language: "plaintext",
  isDirty: false,
  isLoading: false,
  isPinned: false,
  isBinary: false,
  previewMode: "editor",
  error: null,
  ...overrides,
});

describe("file-editor-store", () => {
  beforeEach(() => {
    vi.useRealTimers();
    closeAllTabs();
    fileReadMock.mockReset();
    fileWriteMock.mockReset();
  });

  afterEach(() => {
    closeAllTabs();
    vi.useRealTimers();
  });

  it("detects language and preview helpers", () => {
    expect(detectLanguage("src/app.tsx")).toBe("typescript");
    expect(detectLanguage("data.json")).toBe("json");
    expect(detectLanguage("unknown.bin")).toBe("plaintext");
    expect(isPreviewable("README.md")).toBe(true);
    expect(isPreviewable("src/main.ts")).toBe(false);
    expect(isImageFile("photo.PNG")).toBe(true);
    expect(isImageFile("notes.txt")).toBe(false);
  });

  it("opens a file as an unpinned preview tab", async () => {
    fileReadMock.mockResolvedValueOnce({ content: "hello", isBinary: false, sizeBytes: 5 });

    await openFile("workspace-1", "README.md");

    expect(fileReadMock).toHaveBeenCalledWith("workspace-1", "README.md");
    expect(fileEditorStore.getState()).toMatchObject({
      activeTabKey: getFileTabKey("workspace-1", "README.md"),
    });
    expect(fileEditorStore.getState().tabs).toEqual([
      expect.objectContaining({
        workspaceId: "workspace-1",
        path: "README.md",
        content: "hello",
        originalContent: "hello",
        isDirty: false,
        isPinned: false,
        previewMode: "preview",
      }),
    ]);
  });

  it("keeps same-path files isolated across workspaces", async () => {
    fileReadMock
      .mockResolvedValueOnce({ content: "one", isBinary: false, sizeBytes: 3 })
      .mockResolvedValueOnce({ content: "two", isBinary: false, sizeBytes: 3 });

    await openFile("workspace-1", "README.md", true);
    await openFile("workspace-2", "README.md", true);

    expect(fileReadMock).toHaveBeenCalledTimes(2);
    expect(fileReadMock).toHaveBeenNthCalledWith(1, "workspace-1", "README.md");
    expect(fileReadMock).toHaveBeenNthCalledWith(2, "workspace-2", "README.md");

    const state = fileEditorStore.getState();
    expect(state.activeTabKey).toBe(getFileTabKey("workspace-2", "README.md"));
    expect(state.tabs).toHaveLength(2);
    expect(state.tabs).toEqual([
      expect.objectContaining({ workspaceId: "workspace-1", path: "README.md", content: "one" }),
      expect.objectContaining({ workspaceId: "workspace-2", path: "README.md", content: "two" }),
    ]);
  });

  it("saves same-path files only in the matching workspace", async () => {
    fileReadMock
      .mockResolvedValueOnce({ content: "one", isBinary: false, sizeBytes: 3 })
      .mockResolvedValueOnce({ content: "two", isBinary: false, sizeBytes: 3 });
    fileWriteMock.mockResolvedValueOnce(undefined);

    await openFile("workspace-1", "README.md", true);
    await openFile("workspace-2", "README.md", true);
    updateContent("workspace-2", "README.md", "two changed");
    await saveFile("workspace-2", "README.md");

    expect(fileWriteMock).toHaveBeenCalledTimes(1);
    expect(fileWriteMock).toHaveBeenCalledWith("workspace-2", "README.md", "two changed");
    expect(fileEditorStore.getState().tabs).toEqual([
      expect.objectContaining({ workspaceId: "workspace-1", path: "README.md", content: "one", isDirty: false }),
      expect.objectContaining({ workspaceId: "workspace-2", path: "README.md", content: "two changed", isDirty: false }),
    ]);
  });

  it("replaces an unpinned clean preview tab in the same workspace when opening another preview", async () => {
    fileReadMock
      .mockResolvedValueOnce({ content: "first", isBinary: false, sizeBytes: 5 })
      .mockResolvedValueOnce({ content: "other workspace", isBinary: false, sizeBytes: 15 })
      .mockResolvedValueOnce({ content: "second", isBinary: false, sizeBytes: 6 });

    await openFile("workspace-1", "first.txt");
    await openFile("workspace-2", "keep.txt");
    await openFile("workspace-1", "second.txt");

    const state = fileEditorStore.getState();
    expect(state.activeTabKey).toBe(getFileTabKey("workspace-1", "second.txt"));
    expect(state.tabs).toHaveLength(2);
    expect(state.tabs).toEqual([
      expect.objectContaining({ workspaceId: "workspace-1", path: "second.txt", content: "second" }),
      expect.objectContaining({ workspaceId: "workspace-2", path: "keep.txt", content: "other workspace" }),
    ]);
  });

  it("keeps pinned tabs when opening another file", async () => {
    fileReadMock
      .mockResolvedValueOnce({ content: "first", isBinary: false, sizeBytes: 5 })
      .mockResolvedValueOnce({ content: "second", isBinary: false, sizeBytes: 6 });

    await openFile("workspace-1", "first.txt", true);
    await openFile("workspace-1", "second.txt");

    const state = fileEditorStore.getState();
    expect(state.tabs.map((tab) => tab.path)).toEqual(["first.txt", "second.txt"]);
    expect(state.tabs[0].isPinned).toBe(true);
  });

  it("marks content dirty and saves it", async () => {
    fileReadMock.mockResolvedValueOnce({ content: "hello", isBinary: false, sizeBytes: 5 });
    fileWriteMock.mockResolvedValueOnce(undefined);

    await openFile("workspace-1", "note.txt");
    updateContent("workspace-1", "note.txt", "hello world");

    let tab = fileEditorStore.getState().tabs[0];
    expect(tab).toMatchObject({ content: "hello world", isDirty: true });

    await saveFile("workspace-1", "note.txt");

    expect(fileWriteMock).toHaveBeenCalledWith("workspace-1", "note.txt", "hello world");
    tab = fileEditorStore.getState().tabs[0];
    expect(tab).toMatchObject({ originalContent: "hello world", isDirty: false, error: null });
  });

  it("records save failures without clearing dirty content", async () => {
    fileReadMock.mockResolvedValueOnce({ content: "hello", isBinary: false, sizeBytes: 5 });
    fileWriteMock.mockRejectedValueOnce(new Error("disk full"));

    await openFile("workspace-1", "note.txt");
    updateContent("workspace-1", "note.txt", "changed");
    await saveFile("workspace-1", "note.txt");

    const tab = fileEditorStore.getState().tabs[0];
    expect(tab.content).toBe("changed");
    expect(tab.isDirty).toBe(true);
    expect(tab.error).toBe("Save failed: disk full");
  });

  it("selects another tab in the same workspace when closing the active tab", async () => {
    fileReadMock
      .mockResolvedValueOnce({ content: "one", isBinary: false, sizeBytes: 3 })
      .mockResolvedValueOnce({ content: "other", isBinary: false, sizeBytes: 5 })
      .mockResolvedValueOnce({ content: "two", isBinary: false, sizeBytes: 3 });

    await openFile("workspace-1", "one.txt", true);
    await openFile("workspace-2", "other.txt", true);
    await openFile("workspace-1", "two.txt", true);
    closeTab("workspace-1", "two.txt");

    expect(fileEditorStore.getState().activeTabKey).toBe(getFileTabKey("workspace-1", "one.txt"));
  });

  it("clears the active tab when closing the last tab in that workspace", async () => {
    fileReadMock
      .mockResolvedValueOnce({ content: "one", isBinary: false, sizeBytes: 3 })
      .mockResolvedValueOnce({ content: "other", isBinary: false, sizeBytes: 5 });

    await openFile("workspace-2", "other.txt", true);
    await openFile("workspace-1", "one.txt", true);
    closeTab("workspace-1", "one.txt");

    expect(fileEditorStore.getState().activeTabKey).toBeNull();
    expect(fileEditorStore.getState().tabs).toEqual([
      expect.objectContaining({ workspaceId: "workspace-2", path: "other.txt" }),
    ]);
  });

  it("evicts the oldest clean unpinned tab when max tabs is exceeded", async () => {
    fileEditorStore.setState({
      activeTabKey: getFileTabKey("workspace-1", "tab-9.txt"),
      tabs: Array.from({ length: 10 }, (_, index) => makeTab({
        path: `tab-${index}.txt`,
        content: `tab ${index}`,
        originalContent: `tab ${index}`,
      })),
    });
    fileReadMock.mockResolvedValueOnce({ content: "new", isBinary: false, sizeBytes: 3 });

    await openFile("workspace-1", "new.txt", true);

    const paths = fileEditorStore.getState().tabs.map((tab) => tab.path);
    expect(paths).not.toContain("tab-0.txt");
    expect(paths).toContain("new.txt");
    expect(paths).toHaveLength(10);
  });

  it("keeps all tabs when max tabs are pinned and no eviction candidate exists", async () => {
    fileEditorStore.setState({
      activeTabKey: getFileTabKey("workspace-1", "pinned-9.txt"),
      tabs: Array.from({ length: 10 }, (_, index) => makeTab({
        path: `pinned-${index}.txt`,
        content: `tab ${index}`,
        originalContent: `tab ${index}`,
        isPinned: true,
      })),
    });
    fileReadMock.mockResolvedValueOnce({ content: "new", isBinary: false, sizeBytes: 3 });

    await openFile("workspace-1", "new.txt", true);

    expect(fileEditorStore.getState().tabs).toHaveLength(11);
    expect(fileEditorStore.getState().tabs.some((tab) => tab.path === "new.txt")).toBe(true);
  });

  it("auto-saves dirty content after the debounce delay", async () => {
    vi.useFakeTimers();
    fileReadMock.mockResolvedValueOnce({ content: "hello", isBinary: false, sizeBytes: 5 });
    fileWriteMock.mockResolvedValueOnce(undefined);

    await openFile("workspace-1", "auto.txt");
    updateContent("workspace-1", "auto.txt", "changed");

    await vi.advanceTimersByTimeAsync(9_999);
    expect(fileWriteMock).not.toHaveBeenCalled();

    await vi.advanceTimersByTimeAsync(1);
    expect(fileWriteMock).toHaveBeenCalledWith("workspace-1", "auto.txt", "changed");

    closeAllTabs();
    vi.useRealTimers();
  });

  it("keeps auto-save timers isolated across workspaces", async () => {
    vi.useFakeTimers();
    fileReadMock
      .mockResolvedValueOnce({ content: "one", isBinary: false, sizeBytes: 3 })
      .mockResolvedValueOnce({ content: "two", isBinary: false, sizeBytes: 3 });
    fileWriteMock.mockResolvedValue(undefined);

    await openFile("workspace-1", "auto.txt", true);
    await openFile("workspace-2", "auto.txt", true);
    updateContent("workspace-1", "auto.txt", "one changed");
    updateContent("workspace-2", "auto.txt", "two changed");

    await vi.advanceTimersByTimeAsync(10_000);

    expect(fileWriteMock).toHaveBeenCalledTimes(2);
    expect(fileWriteMock).toHaveBeenCalledWith("workspace-1", "auto.txt", "one changed");
    expect(fileWriteMock).toHaveBeenCalledWith("workspace-2", "auto.txt", "two changed");

    closeAllTabs();
    vi.useRealTimers();
  });

  it("cancels pending auto-save when closing tabs", async () => {
    vi.useFakeTimers();
    fileReadMock.mockResolvedValueOnce({ content: "hello", isBinary: false, sizeBytes: 5 });

    await openFile("workspace-1", "close-me.txt");
    updateContent("workspace-1", "close-me.txt", "changed");
    closeTab("workspace-1", "close-me.txt");

    await vi.advanceTimersByTimeAsync(10_000);
    expect(fileWriteMock).not.toHaveBeenCalled();

    closeAllTabs();
    vi.useRealTimers();
  });

  it("handles binary files and read failures when opening", async () => {
    fileReadMock
      .mockResolvedValueOnce({ content: "", isBinary: true, sizeBytes: 7 })
      .mockRejectedValueOnce(new Error("read failed"));

    await openFile("workspace-1", "binary.bin");
    let tab = fileEditorStore.getState().tabs[0];
    expect(tab).toMatchObject({
      workspaceId: "workspace-1",
      path: "binary.bin",
      isBinary: true,
      isLoading: false,
      content: "",
      originalContent: null,
    });

    await openFile("workspace-1", "broken.txt", true);
    tab = fileEditorStore.getState().tabs.find((item) => item.path === "broken.txt")!;
    expect(tab).toMatchObject({ isLoading: false, isBinary: false, error: "read failed" });
  });

  it("pins tabs and updates preview mode", async () => {
    fileReadMock.mockResolvedValueOnce({ content: "# hello", isBinary: false, sizeBytes: 7 });

    await openFile("workspace-1", "README.md");
    pinTab("workspace-1", "README.md");
    setPreviewMode("workspace-1", "README.md", "editor");

    expect(fileEditorStore.getState().tabs[0]).toMatchObject({
      isPinned: true,
      previewMode: "editor",
    });
  });

  it("reloads clean tabs and records reload errors", async () => {
    fileReadMock
      .mockResolvedValueOnce({ content: "initial", isBinary: false, sizeBytes: 7 })
      .mockResolvedValueOnce({ content: "reloaded", isBinary: false, sizeBytes: 8 })
      .mockRejectedValueOnce(new Error("reload failed"));

    await openFile("workspace-1", "reload.txt");
    await reloadTab("workspace-1", "reload.txt");

    let tab = fileEditorStore.getState().tabs[0];
    expect(tab).toMatchObject({
      content: "reloaded",
      originalContent: "reloaded",
      isDirty: false,
      isLoading: false,
      error: null,
    });

    await reloadTab("workspace-1", "reload.txt");
    tab = fileEditorStore.getState().tabs[0];
    expect(tab).toMatchObject({ isLoading: false, error: "reload failed" });

    await reloadTab("workspace-1", "missing.txt");
    expect(fileReadMock).toHaveBeenCalledTimes(3);
  });

  it("reloads only the matching workspace for same-path tabs", async () => {
    fileReadMock
      .mockResolvedValueOnce({ content: "one", isBinary: false, sizeBytes: 3 })
      .mockResolvedValueOnce({ content: "two", isBinary: false, sizeBytes: 3 })
      .mockResolvedValueOnce({ content: "two reloaded", isBinary: false, sizeBytes: 12 });

    await openFile("workspace-1", "reload.txt", true);
    await openFile("workspace-2", "reload.txt", true);
    await reloadTab("workspace-2", "reload.txt");

    expect(fileEditorStore.getState().tabs).toEqual([
      expect.objectContaining({ workspaceId: "workspace-1", path: "reload.txt", content: "one" }),
      expect.objectContaining({ workspaceId: "workspace-2", path: "reload.txt", content: "two reloaded" }),
    ]);
  });

  it("skips save when there is no dirty text content and clears all tabs", async () => {
    fileReadMock.mockResolvedValueOnce({ content: "hello", isBinary: false, sizeBytes: 5 });

    await openFile("workspace-1", "clean.txt", true);
    await saveFile("workspace-1", "clean.txt");
    expect(fileWriteMock).not.toHaveBeenCalled();

    fileEditorStore.setState((prev) => ({
      tabs: prev.tabs.map((tab) => ({ ...tab, content: null, isDirty: true })),
    }));
    await saveFile("workspace-1", "clean.txt");
    expect(fileWriteMock).not.toHaveBeenCalled();

    closeAllTabs();
    expect(fileEditorStore.getState()).toMatchObject({ tabs: [], activeTabKey: null });
  });
});
