import { beforeEach, describe, expect, it, vi } from "vitest";

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
  saveFile,
  updateContent,
} from "./file-editor-store";

describe("file-editor-store", () => {
  beforeEach(() => {
    vi.useRealTimers();
    closeAllTabs();
    fileReadMock.mockReset();
    fileWriteMock.mockReset();
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
});
