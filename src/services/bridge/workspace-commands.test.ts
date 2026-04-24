import { describe, it, expect, vi, beforeEach } from "vitest";
import { invoke, isTauri } from "@tauri-apps/api/core";

// Import after mocks are set up by test-setup.ts
import * as workspaceCommands from "./workspace-commands";

describe("workspace-commands", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  // ---------------------------------------------------------------------------
  // workspaceList
  // ---------------------------------------------------------------------------
  describe("workspaceList", () => {
    it("returns empty array when not in Tauri", async () => {
      vi.mocked(isTauri).mockReturnValue(false);
      const result = await workspaceCommands.workspaceList();
      expect(result).toEqual([]);
      expect(invoke).not.toHaveBeenCalled();
    });

    it("calls workspace_list command when in Tauri", async () => {
      const mockWorkspaces = [{ id: "w1", path: "/tmp/project" }];
      vi.mocked(isTauri).mockReturnValue(true);
      vi.mocked(invoke).mockResolvedValue(mockWorkspaces);

      const result = await workspaceCommands.workspaceList();
      expect(result).toEqual(mockWorkspaces);
      expect(invoke).toHaveBeenCalledWith("workspace_list");
    });
  });

  // ---------------------------------------------------------------------------
  // workspaceAdd
  // ---------------------------------------------------------------------------
  describe("workspaceAdd", () => {
    it("throws when not in Tauri", async () => {
      vi.mocked(isTauri).mockReturnValue(false);
      await expect(workspaceCommands.workspaceAdd("/path")).rejects.toThrow(
        "workspace_add requires Tauri runtime",
      );
    });

    it("calls workspace_add with path and name", async () => {
      const mockWorkspace = { id: "w2", path: "/new/path", name: "my-proj" };
      vi.mocked(isTauri).mockReturnValue(true);
      vi.mocked(invoke).mockResolvedValue(mockWorkspace);

      const result = await workspaceCommands.workspaceAdd("/new/path", "my-proj");
      expect(result).toEqual(mockWorkspace);
      expect(invoke).toHaveBeenCalledWith("workspace_add", { path: "/new/path", name: "my-proj" });
    });

    it("calls workspace_add with null name when omitted", async () => {
      vi.mocked(isTauri).mockReturnValue(true);
      vi.mocked(invoke).mockResolvedValue({ id: "w3", path: "/p" });

      await workspaceCommands.workspaceAdd("/p");
      expect(invoke).toHaveBeenCalledWith("workspace_add", { path: "/p", name: undefined });
    });
  });

  // ---------------------------------------------------------------------------
  // workspaceEnsureDefault
  // ---------------------------------------------------------------------------
  describe("workspaceEnsureDefault", () => {
    it("throws when not in Tauri", async () => {
      vi.mocked(isTauri).mockReturnValue(false);
      await expect(workspaceCommands.workspaceEnsureDefault()).rejects.toThrow(
        "workspace_ensure_default requires Tauri runtime",
      );
    });

    it("calls workspace_ensure_default", async () => {
      const ws = { id: "default", path: "/home/user" };
      vi.mocked(isTauri).mockReturnValue(true);
      vi.mocked(invoke).mockResolvedValue(ws);

      const result = await workspaceCommands.workspaceEnsureDefault();
      expect(result).toEqual(ws);
      expect(invoke).toHaveBeenCalledWith("workspace_ensure_default");
    });
  });

  // ---------------------------------------------------------------------------
  // workspaceRemove
  // ---------------------------------------------------------------------------
  describe("workspaceRemove", () => {
    it("throws when not in Tauri", async () => {
      vi.mocked(isTauri).mockReturnValue(false);
      await expect(workspaceCommands.workspaceRemove("w1")).rejects.toThrow(
        "workspace_remove requires Tauri runtime",
      );
    });

    it("calls workspace_remove with force defaulting to false", async () => {
      vi.mocked(isTauri).mockReturnValue(true);
      vi.mocked(invoke).mockResolvedValue(undefined);

      await workspaceCommands.workspaceRemove("w1");
      expect(invoke).toHaveBeenCalledWith("workspace_remove", { id: "w1", force: false });
    });

    it("passes force=true when specified", async () => {
      vi.mocked(isTauri).mockReturnValue(true);
      vi.mocked(invoke).mockResolvedValue(undefined);

      await workspaceCommands.workspaceRemove("w1", true);
      expect(invoke).toHaveBeenCalledWith("workspace_remove", { id: "w1", force: true });
    });
  });

  // ---------------------------------------------------------------------------
  // workspaceSetDefault
  // ---------------------------------------------------------------------------
  describe("workspaceSetDefault", () => {
    it("throws when not in Tauri", async () => {
      vi.mocked(isTauri).mockReturnValue(false);
      await expect(workspaceCommands.workspaceSetDefault("w1")).rejects.toThrow(
        "workspace_set_default requires Tauri runtime",
      );
    });

    it("calls workspace_set_default", async () => {
      vi.mocked(isTauri).mockReturnValue(true);
      vi.mocked(invoke).mockResolvedValue(undefined);

      await workspaceCommands.workspaceSetDefault("w1");
      expect(invoke).toHaveBeenCalledWith("workspace_set_default", { id: "w1" });
    });
  });

  // ---------------------------------------------------------------------------
  // workspaceValidate
  // ---------------------------------------------------------------------------
  describe("workspaceValidate", () => {
    it("throws when not in Tauri", async () => {
      vi.mocked(isTauri).mockReturnValue(false);
      await expect(workspaceCommands.workspaceValidate("w1")).rejects.toThrow(
        "workspace_validate requires Tauri runtime",
      );
    });

    it("calls workspace_validate and returns workspace dto", async () => {
      const validWs = { id: "w1", path: "/valid" };
      vi.mocked(isTauri).mockReturnValue(true);
      vi.mocked(invoke).mockResolvedValue(validWs);

      const result = await workspaceCommands.workspaceValidate("w1");
      expect(result).toEqual(validWs);
      expect(invoke).toHaveBeenCalledWith("workspace_validate", { id: "w1" });
    });
  });

  // ---------------------------------------------------------------------------
  // Worktree commands
  // ---------------------------------------------------------------------------
  describe("workspaceListWorktrees", () => {
    it("returns empty array when not in Tauri", async () => {
      vi.mocked(isTauri).mockReturnValue(false);
      const result = await workspaceCommands.workspaceListWorktrees("w1");
      expect(result).toEqual([]);
      expect(invoke).not.toHaveBeenCalled();
    });

    it("calls workspace_list_worktrees", async () => {
      const trees = [{ id: "wt1", branch: "main" }];
      vi.mocked(isTauri).mockReturnValue(true);
      vi.mocked(invoke).mockResolvedValue(trees);

      const result = await workspaceCommands.workspaceListWorktrees("w1");
      expect(result).toEqual(trees);
      expect(invoke).toHaveBeenCalledWith("workspace_list_worktrees", { workspaceId: "w1" });
    });
  });

  describe("workspaceCreateWorktree", () => {
    it("throws when not in Tauri", async () => {
      vi.mocked(isTauri).mockReturnValue(false);
      await expect(
        workspaceCommands.workspaceCreateWorktree("w1", { baseBranch: "main" }),
      ).rejects.toThrow("workspace_create_worktree requires Tauri runtime");
    });

    it("calls workspace_create_worktree with input", async () => {
      const input = { baseBranch: "feat-x" };
      const newWs = { id: "w2", path: "/worktrees/feat-x" };
      vi.mocked(isTauri).mockReturnValue(true);
      vi.mocked(invoke).mockResolvedValue(newWs);

      const result = await workspaceCommands.workspaceCreateWorktree("w1", input as any);
      expect(result).toEqual(newWs);
      expect(invoke).toHaveBeenCalledWith("workspace_create_worktree", { workspaceId: "w1", input });
    });
  });

  describe("workspaceRemoveWorktree", () => {
    it("throws when not in Tauri", async () => {
      vi.mocked(isTauri).mockReturnValue(false);
      await expect(workspaceCommands.workspaceRemoveWorktree("wt1")).rejects.toThrow(
        "workspace_remove_worktree requires Tauri runtime",
      );
    });

    it("calls workspace_remove_worktree with force default false", async () => {
      vi.mocked(isTauri).mockReturnValue(true);
      vi.mocked(invoke).mockResolvedValue(undefined);

      await workspaceCommands.workspaceRemoveWorktree("wt1");
      expect(invoke).toHaveBeenCalledWith("workspace_remove_worktree", { id: "wt1", force: false });
    });

    it("passes force option through", async () => {
      vi.mocked(isTauri).mockReturnValue(true);
      vi.mocked(invoke).mockResolvedValue(undefined);

      await workspaceCommands.workspaceRemoveWorktree("wt1", true);
      expect(invoke).toHaveBeenCalledWith("workspace_remove_worktree", { id: "wt1", force: true });
    });
  });

  describe("workspacePruneWorktrees", () => {
    it("throws when not in Tauri", async () => {
      vi.mocked(isTauri).mockReturnValue(false);
      await expect(workspaceCommands.workspacePruneWorktrees("w1")).rejects.toThrow(
        "workspace_prune_worktrees requires Tauri runtime",
      );
    });

    it("calls workspace_prune_worktrees", async () => {
      vi.mocked(isTauri).mockReturnValue(true);
      vi.mocked(invoke).mockResolvedValue(undefined);

      await workspaceCommands.workspacePruneWorktrees("w1");
      expect(invoke).toHaveBeenCalledWith("workspace_prune_worktrees", { workspaceId: "w1" });
    });
  });
});
