import { describe, it, expect, vi, beforeEach } from "vitest";
import { invoke, isTauri } from "@tauri-apps/api/core";

import * as threadCommands from "./thread-commands";

describe("thread-commands", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  // ---------------------------------------------------------------------------
  // threadList
  // ---------------------------------------------------------------------------
  describe("threadList", () => {
    it("returns empty array when not in Tauri", async () => {
      vi.mocked(isTauri).mockReturnValue(false);
      const result = await threadCommands.threadList("ws1");
      expect(result).toEqual([]);
      expect(invoke).not.toHaveBeenCalled();
    });

    it("calls thread_list with workspaceId and null limit/offset by default", async () => {
      vi.mocked(isTauri).mockReturnValue(true);
      const threads = [{ id: "t1", title: "Hello" }] as any;
      vi.mocked(invoke).mockResolvedValue(threads);

      const result = await threadCommands.threadList("ws1");
      expect(result).toEqual(threads);
      expect(invoke).toHaveBeenCalledWith("thread_list", {
        workspaceId: "ws1",
        limit: null,
        offset: null,
      });
    });

    it("passes limit and offset when provided", async () => {
      vi.mocked(isTauri).mockReturnValue(true);
      vi.mocked(invoke).mockResolvedValue([]);

      await threadCommands.threadList("ws1", 20, 40);
      expect(invoke).toHaveBeenCalledWith("thread_list", {
        workspaceId: "ws1",
        limit: 20,
        offset: 40,
      });
    });
  });

  // ---------------------------------------------------------------------------
  // threadCreate
  // ---------------------------------------------------------------------------
  describe("threadCreate", () => {
    it("throws when not in Tauri", async () => {
      vi.mocked(isTauri).mockReturnValue(false);
      await expect(threadCommands.threadCreate("ws1")).rejects.toThrow(
        "thread_create requires Tauri runtime",
      );
    });

    it("calls thread_create with workspaceId and null title/profileId by default", async () => {
      vi.mocked(isTauri).mockReturnValue(true);
      const thread = { id: "t2", title: "Untitled" } as any;
      vi.mocked(invoke).mockResolvedValue(thread);

      const result = await threadCommands.threadCreate("ws1");
      expect(result).toEqual(thread);
      expect(invoke).toHaveBeenCalledWith("thread_create", {
        workspaceId: "ws1",
        title: null,
        profileId: null,
      });
    });

    it("passes title and profileId when provided", async () => {
      vi.mocked(isTauri).mockReturnValue(true);
      const thread = { id: "t3", title: "My Thread" } as any;
      vi.mocked(invoke).mockResolvedValue(thread);

      const result = await threadCommands.threadCreate("ws1", "My Thread", "prof-1");
      expect(result).toEqual(thread);
      expect(invoke).toHaveBeenCalledWith("thread_create", {
        workspaceId: "ws1",
        title: "My Thread",
        profileId: "prof-1",
      });
    });
  });

  // ---------------------------------------------------------------------------
  // threadLoad
  // ---------------------------------------------------------------------------
  describe("threadLoad", () => {
    it("throws when not in Tauri", async () => {
      vi.mocked(isTauri).mockReturnValue(false);
      await expect(threadCommands.threadLoad("t1")).rejects.toThrow(
        "thread_load requires Tauri runtime",
      );
    });

    it("calls thread_load with id and null cursor/limit by default", async () => {
      vi.mocked(isTauri).mockReturnValue(true);
      const snapshot = { id: "t1", messages: [] } as any;
      vi.mocked(invoke).mockResolvedValue(snapshot);

      const result = await threadCommands.threadLoad("t1");
      expect(result).toEqual(snapshot);
      expect(invoke).toHaveBeenCalledWith("thread_load", {
        id: "t1",
        messageCursor: null,
        messageLimit: null,
      });
    });

    it("passes messageCursor and messageLimit when provided", async () => {
      vi.mocked(isTauri).mockReturnValue(true);
      vi.mocked(invoke).mockResolvedValue({} as any);

      await threadCommands.threadLoad("t1", "cursor-abc", 50);
      expect(invoke).toHaveBeenCalledWith("thread_load", {
        id: "t1",
        messageCursor: "cursor-abc",
        messageLimit: 50,
      });
    });
  });

  // ---------------------------------------------------------------------------
  // threadUpdateTitle
  // ---------------------------------------------------------------------------
  describe("threadUpdateTitle", () => {
    it("throws when not in Tauri", async () => {
      vi.mocked(isTauri).mockReturnValue(false);
      await expect(threadCommands.threadUpdateTitle("t1", "New Title")).rejects.toThrow(
        "thread_update_title requires Tauri runtime",
      );
    });

    it("calls thread_update_title with id and title", async () => {
      vi.mocked(isTauri).mockReturnValue(true);
      vi.mocked(invoke).mockResolvedValue(undefined);

      await threadCommands.threadUpdateTitle("t1", "New Title");
      expect(invoke).toHaveBeenCalledWith("thread_update_title", { id: "t1", title: "New Title" });
    });
  });

  // ---------------------------------------------------------------------------
  // threadUpdateProfile
  // ---------------------------------------------------------------------------
  describe("threadUpdateProfile", () => {
    it("throws when not in Tauri", async () => {
      vi.mocked(isTauri).mockReturnValue(false);
      await expect(
        threadCommands.threadUpdateProfile("t1", "prof-1"),
      ).rejects.toThrow("thread_update_profile requires Tauri runtime");
    });

    it("calls thread_update_profile with id and profileId (including null)", async () => {
      vi.mocked(isTauri).mockReturnValue(true);
      vi.mocked(invoke).mockResolvedValue(undefined);

      await threadCommands.threadUpdateProfile("t1", null);
      expect(invoke).toHaveBeenCalledWith("thread_update_profile", { id: "t1", profileId: null });
    });
  });

  // ---------------------------------------------------------------------------
  // threadDelete
  // ---------------------------------------------------------------------------
  describe("threadDelete", () => {
    it("throws when not in Tauri", async () => {
      vi.mocked(isTauri).mockReturnValue(false);
      await expect(threadCommands.threadDelete("t1")).rejects.toThrow(
        "thread_delete requires Tauri runtime",
      );
    });

    it("calls thread_delete with id", async () => {
      vi.mocked(isTauri).mockReturnValue(true);
      vi.mocked(invoke).mockResolvedValue(undefined);

      await threadCommands.threadDelete("t1");
      expect(invoke).toHaveBeenCalledWith("thread_delete", { id: "t1" });
    });
  });

  // ---------------------------------------------------------------------------
  // threadAddMessage
  // ---------------------------------------------------------------------------
  describe("threadAddMessage", () => {
    it("throws when not in Tauri", async () => {
      vi.mocked(isTauri).mockReturnValue(false);
      await expect(
        threadCommands.threadAddMessage("t1", {} as any),
      ).rejects.toThrow("thread_add_message requires Tauri runtime");
    });

    it("calls thread_add_message with threadId and input", async () => {
      vi.mocked(isTauri).mockReturnValue(true);
      const input = { role: "user", content: "Hello" } as any;
      const msg = { id: "m1", content: "Hello" } as any;
      vi.mocked(invoke).mockResolvedValue(msg);

      const result = await threadCommands.threadAddMessage("t1", input);
      expect(result).toEqual(msg);
      expect(invoke).toHaveBeenCalledWith("thread_add_message", { threadId: "t1", input });
    });
  });

  // ---------------------------------------------------------------------------
  // threadRegenerateTitle
  // ---------------------------------------------------------------------------
  describe("threadRegenerateTitle", () => {
    it("throws when not in Tauri", async () => {
      vi.mocked(isTauri).mockReturnValue(false);
      await expect(
        threadCommands.threadRegenerateTitle("t1", {} as any),
      ).rejects.toThrow("thread_regenerate_title requires Tauri runtime");
    });

    it("calls thread_regenerate_title with threadId and modelPlan", async () => {
      vi.mocked(isTauri).mockReturnValue(true);
      const modelPlan = { modelKey: "gpt-4" } as any;
      vi.mocked(invoke).mockResolvedValue("New Generated Title");

      const result = await threadCommands.threadRegenerateTitle("t1", modelPlan);
      expect(result).toBe("New Generated Title");
      expect(invoke).toHaveBeenCalledWith("thread_regenerate_title", {
        threadId: "t1",
        modelPlan,
      });
    });
  });
});
