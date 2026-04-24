import { describe, it, expect, vi, beforeEach } from "vitest";
import { invoke, isTauri, Channel } from "@tauri-apps/api/core";

import * as gitCommands from "./git-commands";

describe("git-commands", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  // ---------------------------------------------------------------------------
  // gitGetSnapshot
  // ---------------------------------------------------------------------------
  describe("gitGetSnapshot", () => {
    it("returns null when not in Tauri", async () => {
      vi.mocked(isTauri).mockReturnValue(false);
      const result = await gitCommands.gitGetSnapshot("ws1");
      expect(result).toBeNull();
      expect(invoke).not.toHaveBeenCalled();
    });

    it("calls git_get_snapshot with workspaceId", async () => {
      vi.mocked(isTauri).mockReturnValue(true);
      const snapshot = { branch: "main", commit: "abc123" } as any;
      vi.mocked(invoke).mockResolvedValue(snapshot);

      const result = await gitCommands.gitGetSnapshot("ws1");
      expect(result).toEqual(snapshot);
      expect(invoke).toHaveBeenCalledWith("git_get_snapshot", { workspaceId: "ws1" });
    });
  });

  // ---------------------------------------------------------------------------
  // gitGetHistory
  // ---------------------------------------------------------------------------
  describe("gitGetHistory", () => {
    it("returns empty array when not in Tauri", async () => {
      vi.mocked(isTauri).mockReturnValue(false);
      const result = await gitCommands.gitGetHistory("ws1");
      expect(result).toEqual([]);
    });

    it("calls git_get_history with null limit by default", async () => {
      vi.mocked(isTauri).mockReturnValue(true);
      const history = [{ sha: "a1b2c3" }] as any;
      vi.mocked(invoke).mockResolvedValue(history);

      const result = await gitCommands.gitGetHistory("ws1");
      expect(result).toEqual(history);
      expect(invoke).toHaveBeenCalledWith("git_get_history", {
        workspaceId: "ws1",
        limit: null,
      });
    });

    it("passes limit when provided", async () => {
      vi.mocked(isTauri).mockReturnValue(true);
      vi.mocked(invoke).mockResolvedValue([] as any);

      await gitCommands.gitGetHistory("ws1", 10);
      expect(invoke).toHaveBeenCalledWith("git_get_history", {
        workspaceId: "ws1",
        limit: 10,
      });
    });
  });

  // ---------------------------------------------------------------------------
  // gitGetDiff
  // ---------------------------------------------------------------------------
  describe("gitGetDiff", () => {
    it("throws when not in Tauri", async () => {
      vi.mocked(isTauri).mockReturnValue(false);
      await expect(
        gitCommands.gitGetDiff("ws1", "src/main.ts"),
      ).rejects.toThrow("git_get_diff requires Tauri runtime");
    });

    it("calls git_get_diff with null staged by default", async () => {
      vi.mocked(isTauri).mockReturnValue(true);
      const diff = { diff: "+line" } as any;
      vi.mocked(invoke).mockResolvedValue(diff);

      const result = await gitCommands.gitGetDiff("ws1", "src/main.ts");
      expect(result).toEqual(diff);
      expect(invoke).toHaveBeenCalledWith("git_get_diff", {
        workspaceId: "ws1",
        path: "src/main.ts",
        staged: null,
      });
    });

    it("passes staged option", async () => {
      vi.mocked(isTauri).mockReturnValue(true);
      vi.mocked(invoke).mockResolvedValue({} as any);

      await gitCommands.gitGetDiff("ws1", "file.rs", true);
      expect(invoke).toHaveBeenCalledWith("git_get_diff", {
        workspaceId: "ws1",
        path: "file.rs",
        staged: true,
      });
    });
  });

  // ---------------------------------------------------------------------------
  // gitGetFileStatus
  // ---------------------------------------------------------------------------
  describe("gitGetFileStatus", () => {
    it("throws when not in Tauri", async () => {
      vi.mocked(isTauri).mockReturnValue(false);
      await expect(gitCommands.gitGetFileStatus("ws1", "f")).rejects.toThrow(
        "git_get_file_status requires Tauri runtime",
      );
    });

    it("calls git_get_file_status", async () => {
      vi.mocked(isTauri).mockReturnValue(true);
      const status = { status: "M" } as any;
      vi.mocked(invoke).mockResolvedValue(status);

      const result = await gitCommands.gitGetFileStatus("ws1", "f.ts");
      expect(result).toEqual(status);
      expect(invoke).toHaveBeenCalledWith("git_get_file_status", {
        workspaceId: "ws1",
        path: "f.ts",
      });
    });
  });

  // ---------------------------------------------------------------------------
  // gitGetConflictDiff
  // ---------------------------------------------------------------------------
  describe("gitGetConflictDiff", () => {
    it("throws when not in Tauri", async () => {
      vi.mocked(isTauri).mockReturnValue(false);
      await expect(gitCommands.gitGetConflictDiff("ws1", "f")).rejects.toThrow(
        "git_get_conflict_diff requires Tauri runtime",
      );
    });

    it("calls git_get_conflict_diff", async () => {
      vi.mocked(isTauri).mockReturnValue(true);
      const diff = { conflictContent: "<<<<<<" } as any;
      vi.mocked(invoke).mockResolvedValue(diff);

      const result = await gitCommands.gitGetConflictDiff("ws1", "f");
      expect(result).toEqual(diff);
      expect(invoke).toHaveBeenCalledWith("git_get_conflict_diff", {
        workspaceId: "ws1",
        path: "f",
      });
    });
  });

  // ---------------------------------------------------------------------------
  // gitSubscribe / gitUnsubscribe
  // ---------------------------------------------------------------------------
  describe("gitSubscribe", () => {
    it("throws when not in Tauri", async () => {
      vi.mocked(isTauri).mockReturnValue(false);
      await expect(
        gitCommands.gitSubscribe("ws1", vi.fn()),
      ).rejects.toThrow("git_subscribe requires Tauri runtime");
    });

    it("subscribes and returns unsubscribe function", async () => {
      vi.mocked(isTauri).mockReturnValue(true);
      vi.mocked(invoke).mockResolvedValue(undefined);

      const onEvent = vi.fn();
      const unsub = await gitCommands.gitSubscribe("ws1", onEvent);

      expect(invoke).toHaveBeenCalledWith("git_subscribe", {
        workspaceId: "ws1",
        onEvent: expect.any(Channel),
      });

      expect(unsub).toBeInstanceOf(Function);
    });

    it("unsubscribe calls git_unsubscribe with channel id", async () => {
      vi.mocked(isTauri).mockReturnValue(true);
      vi.mocked(invoke).mockResolvedValue(undefined);

      const unsub = await gitCommands.gitSubscribe("ws1", vi.fn());
      await unsub();

      // Second call is for unsubscribe
      expect(invoke).toHaveBeenCalledTimes(2);
      expect(invoke).toHaveBeenNthCalledWith(2, "git_unsubscribe", {
        workspaceId: "ws1",
        subscriptionId: expect.any(String),
      });
    });

    it("ignores double unsubscription", async () => {
      vi.mocked(isTauri).mockReturnValue(true);
      vi.mocked(invoke).mockResolvedValue(undefined);

      const unsub = await gitCommands.gitSubscribe("ws1", vi.fn());
      await unsub();
      await unsub(); // should be no-op

      // Only subscribe + one unsubscribe call
      expect(invoke).toHaveBeenCalledTimes(2);
    });
  });

  // ---------------------------------------------------------------------------
  // gitRefresh
  // ---------------------------------------------------------------------------
  describe("gitRefresh", () => {
    it("throws when not in Tauri", async () => {
      vi.mocked(isTauri).mockReturnValue(false);
      await expect(gitCommands.gitRefresh("ws1")).rejects.toThrow(
        "git_refresh requires Tauri runtime",
      );
    });

    it("calls git_refresh", async () => {
      vi.mocked(isTauri).mockReturnValue(true);
      const snap = { branch: "main" } as any;
      vi.mocked(invoke).mockResolvedValue(snap);

      const result = await gitCommands.gitRefresh("ws1");
      expect(result).toEqual(snap);
      expect(invoke).toHaveBeenCalledWith("git_refresh", { workspaceId: "ws1" });
    });
  });

  // ---------------------------------------------------------------------------
  // gitStage
  // ---------------------------------------------------------------------------
  describe("gitStage", () => {
    it("throws when not in Tauri", async () => {
      vi.mocked(isTauri).mockReturnValue(false);
      await expect(gitCommands.gitStage("ws1", ["a.ts"])).rejects.toThrow(
        "git_stage requires Tauri runtime",
      );
    });

    it("calls git_stage with paths array", async () => {
      vi.mocked(isTauri).mockReturnValue(true);
      const snap = { branch: "main" } as any;
      vi.mocked(invoke).mockResolvedValue(snap);

      const paths = ["src/a.ts", "src/b.ts"];
      const result = await gitCommands.gitStage("ws1", paths);
      expect(result).toEqual(snap);
      expect(invoke).toHaveBeenCalledWith("git_stage", { workspaceId: "ws1", paths });
    });
  });

  // ---------------------------------------------------------------------------
  // gitUnstage
  // ---------------------------------------------------------------------------
  describe("gitUnstage", () => {
    it("throws when not in Tauri", async () => {
      vi.mocked(isTauri).mockReturnValue(false);
      await expect(gitCommands.gitUnstage("ws1", ["a.ts"])).rejects.toThrow(
        "git_unstage requires Tauri runtime",
      );
    });

    it("calls git_unstage with paths array", async () => {
      vi.mocked(isTauri).mockReturnValue(true);
      const snap = { branch: "main" } as any;
      vi.mocked(invoke).mockResolvedValue(snap);

      const result = await gitCommands.gitUnstage("ws1", ["a.ts"]);
      expect(invoke).toHaveBeenCalledWith("git_unstage", { workspaceId: "ws1", paths: ["a.ts"] });
      expect(result).toEqual(snap);
    });
  });

  // ---------------------------------------------------------------------------
  // gitCommit
  // ---------------------------------------------------------------------------
  describe("gitCommit", () => {
    it("throws when not in Tauri", async () => {
      vi.mocked(isTauri).mockReturnValue(false);
      await expect(gitCommands.gitCommit("ws1", "fix bug")).rejects.toThrow(
        "git_commit requires Tauri runtime",
      );
    });

    it("calls git_commit with null approved by default", async () => {
      vi.mocked(isTauri).mockReturnValue(true);
      const resp = { success: true } as any;
      vi.mocked(invoke).mockResolvedValue(resp);

      const result = await gitCommands.gitCommit("ws1", "fix bug");
      expect(result).toEqual(resp);
      expect(invoke).toHaveBeenCalledWith("git_commit", {
        workspaceId: "ws1",
        message: "fix bug",
        approved: null,
      });
    });

    it("passes approved flag", async () => {
      vi.mocked(isTauri).mockReturnValue(true);
      vi.mocked(invoke).mockResolvedValue({} as any);

      await gitCommands.gitCommit("ws1", "feat x", true);
      expect(invoke).toHaveBeenCalledWith("git_commit", {
        workspaceId: "ws1",
        message: "feat x",
        approved: true,
      });
    });
  });

  // ---------------------------------------------------------------------------
  // gitGenerateCommitMessage
  // ---------------------------------------------------------------------------
  describe("gitGenerateCommitMessage", () => {
    it("throws when not in Tauri", async () => {
      vi.mocked(isTauri).mockReturnValue(false);
      await expect(
        gitCommands.gitGenerateCommitMessage("ws1", {} as any, "en", "changes"),
      ).rejects.toThrow("git_generate_commit_message requires Tauri runtime");
    });

    it("calls git_generate_commit_message with all args", async () => {
      vi.mocked(isTauri).mockReturnValue(true);
      const modelPlan = { modelKey: "gpt-4" } as any;
      vi.mocked(invoke).mockResolvedValue("feat: add new feature");

      const result = await gitCommands.gitGenerateCommitMessage("ws1", modelPlan, "en", "changes");
      expect(result).toBe("feat: add new feature");
      expect(invoke).toHaveBeenCalledWith("git_generate_commit_message", {
        workspaceId: "ws1",
        modelPlan,
        language: "en",
        prompt: "changes",
      });
    });
  });

  // ---------------------------------------------------------------------------
  // gitFetch
  // ---------------------------------------------------------------------------
  describe("gitFetch", () => {
    it("throws when not in Tauri", async () => {
      vi.mocked(isTauri).mockReturnValue(false);
      await expect(gitCommands.gitFetch("ws1")).rejects.toThrow(
        "git_fetch requires Tauri runtime",
      );
    });

    it("calls git_fetch with null approved by default", async () => {
      vi.mocked(isTauri).mockReturnValue(true);
      const resp = { success: true } as any;
      vi.mocked(invoke).mockResolvedValue(resp);

      const result = await gitCommands.gitFetch("ws1");
      expect(invoke).toHaveBeenCalledWith("git_fetch", { workspaceId: "ws1", approved: null });
      expect(result).toEqual(resp);
    });

    it("passes approved flag", async () => {
      vi.mocked(isTauri).mockReturnValue(true);
      vi.mocked(invoke).mockResolvedValue({} as any);

      await gitCommands.gitFetch("ws1", true);
      expect(invoke).toHaveBeenCalledWith("git_fetch", { workspaceId: "ws1", approved: true });
    });
  });

  // ---------------------------------------------------------------------------
  // gitPull
  // ---------------------------------------------------------------------------
  describe("gitPull", () => {
    it("throws when not in Tauri", async () => {
      vi.mocked(isTauri).mockReturnValue(false);
      await expect(gitCommands.gitPull("ws1")).rejects.toThrow(
        "git_pull requires Tauri runtime",
      );
    });

    it("calls git_pull with null approved default, passes approved", async () => {
      vi.mocked(isTauri).mockReturnValue(true);
      vi.mocked(invoke).mockResolvedValue({} as any);

      await gitCommands.gitPull("ws1", true);
      expect(invoke).toHaveBeenCalledWith("git_pull", { workspaceId: "ws1", approved: true });
    });
  });

  // ---------------------------------------------------------------------------
  // gitPush
  // ---------------------------------------------------------------------------
  describe("gitPush", () => {
    it("throws when not in Tauri", async () => {
      vi.mocked(isTauri).mockReturnValue(false);
      await expect(gitCommands.gitPush("ws1")).rejects.toThrow(
        "git_push requires Tauri runtime",
      );
    });

    it("calls git_push with null approved default, passes approved", async () => {
      vi.mocked(isTauri).mockReturnValue(true);
      vi.mocked(invoke).mockResolvedValue({} as any);

      await gitCommands.gitPush("ws1", false);
      expect(invoke).toHaveBeenCalledWith("git_push", { workspaceId: "ws1", approved: false });
    });
  });

  // ---------------------------------------------------------------------------
  // gitListBranches
  // ---------------------------------------------------------------------------
  describe("gitListBranches", () => {
    it("returns empty array when not in Tauri", async () => {
      vi.mocked(isTauri).mockReturnValue(false);
      const result = await gitCommands.gitListBranches("ws1");
      expect(result).toEqual([]);
    });

    it("calls git_list_branches", async () => {
      vi.mocked(isTauri).mockReturnValue(true);
      const branches = [{ name: "main" }, { name: "dev" }] as any;
      vi.mocked(invoke).mockResolvedValue(branches);

      const result = await gitCommands.gitListBranches("ws1");
      expect(result).toEqual(branches);
      expect(invoke).toHaveBeenCalledWith("git_list_branches", { workspaceId: "ws1" });
    });
  });

  // ---------------------------------------------------------------------------
  // gitCheckoutBranch
  // ---------------------------------------------------------------------------
  describe("gitCheckoutBranch", () => {
    it("throws when not in Tauri", async () => {
      vi.mocked(isTauri).mockReturnValue(false);
      await expect(gitCommands.gitCheckoutBranch("ws1", "feat-x")).rejects.toThrow(
        "git_checkout_branch requires Tauri runtime",
      );
    });

    it("calls git_checkout_branch with null approved by default", async () => {
      vi.mocked(isTauri).mockReturnValue(true);
      const resp = { success: true } as any;
      vi.mocked(invoke).mockResolvedValue(resp);

      const result = await gitCommands.gitCheckoutBranch("ws1", "feat-x");
      expect(result).toEqual(resp);
      expect(invoke).toHaveBeenCalledWith("git_checkout_branch", {
        workspaceId: "ws1",
        branch: "feat-x",
        approved: null,
      });
    });

    it("passes approved flag", async () => {
      vi.mocked(isTauri).mockReturnValue(true);
      vi.mocked(invoke).mockResolvedValue({} as any);

      await gitCommands.gitCheckoutBranch("ws1", "main", true);
      expect(invoke).toHaveBeenCalledWith("git_checkout_branch", {
        workspaceId: "ws1",
        branch: "main",
        approved: true,
      });
    });
  });

  // ---------------------------------------------------------------------------
  // gitCreateBranch
  // ---------------------------------------------------------------------------
  describe("gitCreateBranch", () => {
    it("throws when not in Tauri", async () => {
      vi.mocked(isTauri).mockReturnValue(false);
      await expect(gitCommands.gitCreateBranch("ws1", "new-br")).rejects.toThrow(
        "git_create_branch requires Tauri runtime",
      );
    });

    it("calls git_create_branch with null approved by default", async () => {
      vi.mocked(isTauri).mockReturnValue(true);
      const resp = { success: true } as any;
      vi.mocked(invoke).mockResolvedValue(resp);

      const result = await gitCommands.gitCreateBranch("ws1", "new-br");
      expect(invoke).toHaveBeenCalledWith("git_create_branch", {
        workspaceId: "ws1",
        branch: "new-br",
        approved: null,
      });
      expect(result).toEqual(resp);
    });
  });

  // ---------------------------------------------------------------------------
  // gitGenerateBranchName
  // ---------------------------------------------------------------------------
  describe("gitGenerateBranchName", () => {
    it("throws when not in Tauri", async () => {
      vi.mocked(isTauri).mockReturnValue(false);
      await expect(
        gitCommands.gitGenerateBranchName("ws1", {} as any),
      ).rejects.toThrow("git_generate_branch_name requires Tauri runtime");
    });

    it("calls git_generate_branch_name", async () => {
      vi.mocked(isTauri).mockReturnValue(true);
      const modelPlan = { modelKey: "gpt-4" } as any;
      vi.mocked(invoke).mockResolvedValue("feat/add-login-page");

      const result = await gitCommands.gitGenerateBranchName("ws1", modelPlan);
      expect(result).toBe("feat/add-login-page");
      expect(invoke).toHaveBeenCalledWith("git_generate_branch_name", {
        workspaceId: "ws1",
        modelPlan,
      });
    });
  });
});
