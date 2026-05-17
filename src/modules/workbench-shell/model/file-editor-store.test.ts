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
  fileEditorStore,
  isImageFile,
  isPreviewable,
  openFile,
  pinTab,
  reloadTab,
  saveFile,
  setPreviewMode,
  updateContent,
} from "./file-editor-store";

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
    expect(fileEditorStore.getState()).toMatchObject({ activeTabPath: "README.md" });
    expect(fileEditorStore.getState().tabs).toEqual([
      expect.objectContaining({
        path: "README.md",
        content: "hello",
        originalContent: "hello",
        isDirty: false,
        isPinned: false,
        previewMode: "preview",
      }),
    ]);
  });

  it("replaces an unpinned clean preview tab when opening another preview", async () => {
    fileReadMock
      .mockResolvedValueOnce({ content: "first", isBinary: false, sizeBytes: 5 })
      .mockResolvedValueOnce({ content: "second", isBinary: false, sizeBytes: 6 });

    await openFile("workspace-1", "first.txt");
    await openFile("workspace-1", "second.txt");

    const state = fileEditorStore.getState();
    expect(state.activeTabPath).toBe("second.txt");
    expect(state.tabs).toHaveLength(1);
    expect(state.tabs[0]).toMatchObject({ path: "second.txt", content: "second" });
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

  it("selects the previous tab when closing the active tab", async () => {
    fileReadMock
      .mockResolvedValueOnce({ content: "one", isBinary: false, sizeBytes: 3 })
      .mockResolvedValueOnce({ content: "two", isBinary: false, sizeBytes: 3 });

    await openFile("workspace-1", "one.txt", true);
    await openFile("workspace-1", "two.txt", true);
    closeTab("two.txt");

    expect(fileEditorStore.getState().activeTabPath).toBe("one.txt");
  });

  it("evicts the oldest clean unpinned tab when max tabs is exceeded", async () => {
    fileEditorStore.setState({
      activeTabPath: "tab-9.txt",
      tabs: Array.from({ length: 10 }, (_, index) => ({
        path: `tab-${index}.txt`,
        content: `tab ${index}`,
        originalContent: `tab ${index}`,
        language: "plaintext",
        isDirty: false,
        isLoading: false,
        isPinned: false,
        isBinary: false,
        previewMode: "editor" as const,
        error: null,
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
      activeTabPath: "pinned-9.txt",
      tabs: Array.from({ length: 10 }, (_, index) => ({
        path: `pinned-${index}.txt`,
        content: `tab ${index}`,
        originalContent: `tab ${index}`,
        language: "plaintext",
        isDirty: false,
        isLoading: false,
        isPinned: true,
        isBinary: false,
        previewMode: "editor" as const,
        error: null,
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

  it("cancels pending auto-save when closing tabs", async () => {
    vi.useFakeTimers();
    fileReadMock.mockResolvedValueOnce({ content: "hello", isBinary: false, sizeBytes: 5 });

    await openFile("workspace-1", "close-me.txt");
    updateContent("workspace-1", "close-me.txt", "changed");
    closeTab("close-me.txt");

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
    pinTab("README.md");
    setPreviewMode("README.md", "editor");

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
    expect(fileEditorStore.getState()).toMatchObject({ tabs: [], activeTabPath: null });
  });
});
