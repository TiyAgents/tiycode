import { beforeEach, describe, expect, it, vi } from "vitest";

const { invokeMock, isTauriMock } = vi.hoisted(() => ({
  invokeMock: vi.fn(),
  isTauriMock: vi.fn(),
}));

vi.mock("@tauri-apps/api/core", () => ({
  invoke: invokeMock,
  isTauri: isTauriMock,
}));

import {
  workspaceAdd,
  workspaceCreateWorktree,
  workspaceEnsureDefault,
  workspaceList,
  workspaceListWorktrees,
  workspacePruneWorktrees,
  workspaceRemove,
  workspaceRemoveWorktree,
  workspaceSetDefault,
  workspaceValidate,
} from "./workspace-commands";

describe("workspace bridge commands", () => {
  beforeEach(() => {
    invokeMock.mockReset();
    isTauriMock.mockReset();
  });

  it("returns empty lists outside Tauri", async () => {
    isTauriMock.mockReturnValue(false);

    await expect(workspaceList()).resolves.toEqual([]);
    await expect(workspaceListWorktrees("workspace-1")).resolves.toEqual([]);
    expect(invokeMock).not.toHaveBeenCalled();
  });

  it("throws for mutating commands outside Tauri", async () => {
    isTauriMock.mockReturnValue(false);

    await expect(workspaceAdd("/repo")).rejects.toThrow("workspace_add requires Tauri runtime");
    await expect(workspaceEnsureDefault()).rejects.toThrow("workspace_ensure_default requires Tauri runtime");
    await expect(workspaceRemove("workspace-1")).rejects.toThrow("workspace_remove requires Tauri runtime");
    await expect(workspaceSetDefault("workspace-1")).rejects.toThrow("workspace_set_default requires Tauri runtime");
    await expect(workspaceValidate("workspace-1")).rejects.toThrow("workspace_validate requires Tauri runtime");
    await expect(workspaceCreateWorktree("workspace-1", { branch: "main", path: "/tmp/feature" })).rejects.toThrow("workspace_create_worktree requires Tauri runtime");
    await expect(workspaceRemoveWorktree("workspace-1")).rejects.toThrow("workspace_remove_worktree requires Tauri runtime");
    await expect(workspacePruneWorktrees("workspace-1")).rejects.toThrow("workspace_prune_worktrees requires Tauri runtime");
  });

  it("invokes workspace commands with normalized optional values", async () => {
    isTauriMock.mockReturnValue(true);
    invokeMock.mockResolvedValue({ id: "workspace-1" });

    await workspaceList();
    await workspaceAdd("/repo", "Repo");
    await workspaceRemove("workspace-1");
    await workspaceSetDefault("workspace-1");
    await workspaceValidate("workspace-1");
    await workspaceListWorktrees("workspace-1");
    await workspaceCreateWorktree("workspace-1", { branch: "main", path: "/tmp/feature" });
    await workspaceRemoveWorktree("worktree-1", true);
    await workspacePruneWorktrees("workspace-1");

    expect(invokeMock).toHaveBeenCalledWith("workspace_list");
    expect(invokeMock).toHaveBeenCalledWith("workspace_add", { path: "/repo", name: "Repo" });
    expect(invokeMock).toHaveBeenCalledWith("workspace_remove", { id: "workspace-1", force: false });
    expect(invokeMock).toHaveBeenCalledWith("workspace_set_default", { id: "workspace-1" });
    expect(invokeMock).toHaveBeenCalledWith("workspace_validate", { id: "workspace-1" });
    expect(invokeMock).toHaveBeenCalledWith("workspace_list_worktrees", { workspaceId: "workspace-1" });
    expect(invokeMock).toHaveBeenCalledWith("workspace_create_worktree", { workspaceId: "workspace-1", input: { branch: "main", path: "/tmp/feature" } });
    expect(invokeMock).toHaveBeenCalledWith("workspace_remove_worktree", { id: "worktree-1", force: true });
    expect(invokeMock).toHaveBeenCalledWith("workspace_prune_worktrees", { workspaceId: "workspace-1" });
  });
});
