import { describe, it, expect, vi, beforeEach } from "vitest";
import { invoke, isTauri } from "@tauri-apps/api/core";

import * as indexCommands from "./index-commands";

describe("index-commands", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  // ---------------------------------------------------------------------------
  // indexGetTree
  // ---------------------------------------------------------------------------
  describe("indexGetTree", () => {
    it("returns null when not in Tauri", async () => {
      vi.mocked(isTauri).mockReturnValue(false);
      const result = await indexCommands.indexGetTree("ws1");
      expect(result).toBeNull();
      expect(invoke).not.toHaveBeenCalled();
    });

    it("calls index_get_tree when in Tauri", async () => {
      vi.mocked(isTauri).mockReturnValue(true);
      const tree = { repoAvailable: true, tree: { name: "root", path: "/", isDir: true, children: [] } };
      vi.mocked(invoke).mockResolvedValue(tree);

      const result = await indexCommands.indexGetTree("ws1");
      expect(result).toEqual(tree);
      expect(invoke).toHaveBeenCalledWith("index_get_tree", { workspaceId: "ws1" });
    });
  });

  // ---------------------------------------------------------------------------
  // indexGetChildren
  // ---------------------------------------------------------------------------
  describe("indexGetChildren", () => {
    it("calls index_get_children with default null offset/maxResults", async () => {
      const resp = { children: [], hasMore: false };
      vi.mocked(invoke).mockResolvedValue(resp);

      const result = await indexCommands.indexGetChildren("ws1", "/src");
      expect(result).toEqual(resp);
      expect(invoke).toHaveBeenCalledWith("index_get_children", {
        workspaceId: "ws1",
        directoryPath: "/src",
        offset: null,
        maxResults: null,
      });
    });

    it("passes offset and maxResults when provided", async () => {
      vi.mocked(invoke).mockResolvedValue({} as any);

      await indexCommands.indexGetChildren("ws1", "/src", 50, 100);
      expect(invoke).toHaveBeenCalledWith("index_get_children", {
        workspaceId: "ws1",
        directoryPath: "/src",
        offset: 50,
        maxResults: 100,
      });
    });
  });

  // ---------------------------------------------------------------------------
  // indexFilterFiles
  // ---------------------------------------------------------------------------
  describe("indexFilterFiles", () => {
    it("calls index_filter_files with default null maxResults", async () => {
      const resp = { query: "test", results: [], count: 0 };
      vi.mocked(invoke).mockResolvedValue(resp);

      const result = await indexCommands.indexFilterFiles("ws1", "test");
      expect(result).toEqual(resp);
      expect(invoke).toHaveBeenCalledWith("index_filter_files", {
        workspaceId: "ws1",
        query: "test",
        maxResults: null,
      });
    });

    it("passes maxResults when provided", async () => {
      vi.mocked(invoke).mockResolvedValue({} as any);

      await indexCommands.indexFilterFiles("ws1", "ts", 20);
      expect(invoke).toHaveBeenCalledWith("index_filter_files", {
        workspaceId: "ws1",
        query: "ts",
        maxResults: 20,
      });
    });
  });

  // ---------------------------------------------------------------------------
  // indexRevealPath
  // ---------------------------------------------------------------------------
  describe("indexRevealPath", () => {
    it("calls index_reveal_path", async () => {
      const resp = { targetPath: "/src/main.ts", segments: [] };
      vi.mocked(invoke).mockResolvedValue(resp);

      const result = await indexCommands.indexRevealPath("ws1", "/src/main.ts");
      expect(result).toEqual(resp);
      expect(invoke).toHaveBeenCalledWith("index_reveal_path", {
        workspaceId: "ws1",
        targetPath: "/src/main.ts",
      });
    });
  });

  // ---------------------------------------------------------------------------
  // indexSearch
  // ---------------------------------------------------------------------------
  describe("indexSearch", () => {
    it("calls index_search with all defaults null when no options", async () => {
      const resp = { query: "foo", results: [], count: 0, completed: true, elapsedMs: 10, searchedFiles: 5 };
      vi.mocked(invoke).mockResolvedValue(resp);

      const result = await indexCommands.indexSearch("ws1", "foo");
      expect(result).toEqual(resp);
      expect(invoke).toHaveBeenCalledWith("index_search", {
        workspaceId: "ws1",
        query: "foo",
        filePattern: null,
        fileType: null,
        maxResults: null,
        queryMode: null,
        outputMode: null,
        caseInsensitive: null,
        multiline: null,
        timeoutMs: null,
      });
    });

    it("passes all search options through", async () => {
      vi.mocked(invoke).mockResolvedValue({} as any);

      const options = {
        filePattern: "*.ts",
        fileType: "rust",
        maxResults: 50,
        queryMode: "regex" as const,
        outputMode: "files_with_matches" as const,
        caseInsensitive: true,
        multiline: false,
        timeoutMs: 5000,
      };

      await indexCommands.indexSearch("ws1", "pattern", options);
      expect(invoke).toHaveBeenCalledWith("index_search", {
        workspaceId: "ws1",
        query: "pattern",
        filePattern: "*.ts",
        fileType: "rust",
        maxResults: 50,
        queryMode: "regex",
        outputMode: "files_with_matches",
        caseInsensitive: true,
        multiline: false,
        timeoutMs: 5000,
      });
    });
  });

  // ---------------------------------------------------------------------------
  // indexSearchStream
  // ---------------------------------------------------------------------------
  describe("indexSearchStream", () => {
    it("returns no-op cancel function when not in Tauri", async () => {
      vi.mocked(isTauri).mockReturnValue(false);
      const onEvent = vi.fn();

      const cancel = await indexCommands.indexSearchStream("ws1", "q", onEvent);
      expect(cancel).toBeInstanceOf(Function);
      expect(invoke).not.toHaveBeenCalled();

      // Calling cancel should not throw or call invoke
      await cancel();
      expect(invoke).not.toHaveBeenCalled();
    });

    it("cancels previous active search for same workspaceId", async () => {
      vi.mocked(isTauri).mockReturnValue(true);
      vi.mocked(invoke).mockResolvedValue(undefined);

      const onEvent = vi.fn();
      // First search
      await indexCommands.indexSearchStream("ws1", "query1", onEvent);

      // Second search should trigger cancellation of first channel's id
      await indexCommands.indexSearchStream("ws1", "query2", onEvent);

      // The first call to index_cancel_search_stream should have been made
      const calls = vi.mocked(invoke).mock.calls;
      const cancelCalls = calls.filter((c) => c[0] === "index_cancel_search_stream");
      expect(cancelCalls.length).toBeGreaterThanOrEqual(1);
    });

    it("calls invoke with correct params including channel and options", async () => {
      vi.mocked(isTauri).mockReturnValue(true);
      vi.mocked(invoke).mockResolvedValue(undefined);

      const options = {
        filePattern: "*.ts",
        maxResults: 30,
        queryMode: "regex" as const,
        outputMode: "content" as const,
        caseInsensitive: true,
      };

      const cancel = await indexCommands.indexSearchStream(
        "ws1", "test query", vi.fn(), options,
      );
      expect(cancel).toBeInstanceOf(Function);
      expect(invoke).toHaveBeenCalledWith("index_search_stream", expect.objectContaining({
        workspaceId: "ws1",
        query: "test query",
        filePattern: "*.ts",
        maxResults: 30,
        queryMode: "regex",
        outputMode: "content",
        caseInsensitive: true,
        onEvent: expect.any(Object),
      }));
    });

    it("returns a working cancel function that calls index_cancel_search_stream", async () => {
      vi.mocked(isTauri).mockReturnValue(true);
      vi.mocked(invoke).mockResolvedValue(undefined);

      const cancel = await indexCommands.indexSearchStream("ws-unique-cancel", "q", vi.fn());

      // Call cancel - should trigger index_cancel_search_stream
      await cancel();

      // Verify that index_cancel_search_stream was called (the last invoke call)
      const lastCall = vi.mocked(invoke).mock.calls[vi.mocked(invoke).mock.calls.length - 1];
      expect(lastCall[0]).toBe("index_cancel_search_stream");
      expect(lastCall[1]).toEqual({ searchId: expect.any(String) });
    });
  });
});
