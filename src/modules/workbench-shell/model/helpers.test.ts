import { describe, expect, it } from "vitest";
import type { WorkspaceDto } from "@/shared/types/api";
import {
  buildProjectOptionFromWorkspace,
  sortWorkspacesWithWorktrees,
} from "@/modules/workbench-shell/model/helpers";

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

function createWorkspace(overrides: Partial<WorkspaceDto> = {}): WorkspaceDto {
  return {
    id: "ws-1",
    name: "Repo",
    path: "/home/user/repo",
    canonicalPath: "/home/user/repo",
    displayPath: "~/repo",
    isDefault: false,
    isGit: true,
    autoWorkTree: false,
    status: "ready",
    lastValidatedAt: null,
    createdAt: "2026-04-12T00:00:00Z",
    updatedAt: "2026-04-12T00:00:00Z",
    kind: "repo",
    parentWorkspaceId: null,
    gitCommonDir: null,
    branch: "main",
    worktreeName: null,
    ...overrides,
  };
}

// ---------------------------------------------------------------------------
// buildProjectOptionFromWorkspace
// ---------------------------------------------------------------------------

describe("buildProjectOptionFromWorkspace", () => {
  it("maps basic workspace fields", () => {
    const ws = createWorkspace();
    const opt = buildProjectOptionFromWorkspace(ws);
    expect(opt.id).toBe(ws.id);
    expect(opt.name).toBe(ws.name);
    expect(opt.path).toBe(ws.canonicalPath);
    expect(opt.kind).toBe(ws.kind);
    expect(opt.parentWorkspaceId).toBe(ws.parentWorkspaceId);
    expect(opt.branch).toBe(ws.branch);
  });

  it("falls back to path when canonicalPath is empty", () => {
    const ws = createWorkspace({ canonicalPath: "", path: "/fallback/path" });
    const opt = buildProjectOptionFromWorkspace(ws);
    expect(opt.path).toBe("/fallback/path");
  });

  it("slices worktreeName to first 6 chars as worktreeHash", () => {
    const ws = createWorkspace({
      kind: "worktree",
      worktreeName: "abc123-feat-login",
    });
    const opt = buildProjectOptionFromWorkspace(ws);
    expect(opt.worktreeHash).toBe("abc123");
  });

  it("returns null worktreeHash when worktreeName is null", () => {
    const ws = createWorkspace({ worktreeName: null });
    const opt = buildProjectOptionFromWorkspace(ws);
    expect(opt.worktreeHash).toBeNull();
  });

  it("uses provided lastOpenedLabel", () => {
    const ws = createWorkspace();
    const opt = buildProjectOptionFromWorkspace(ws, "2 hours ago");
    expect(opt.lastOpenedLabel).toBe("2 hours ago");
  });
});

// ---------------------------------------------------------------------------
// sortWorkspacesWithWorktrees
// ---------------------------------------------------------------------------

describe("sortWorkspacesWithWorktrees", () => {
  it("sorts standalone items by name then id deterministically", () => {
    const items = [
      { id: "c", kind: "standalone" as const, parentWorkspaceId: null, name: "Zeta" },
      { id: "a", kind: "standalone" as const, parentWorkspaceId: null, name: "Alpha" },
      { id: "b", kind: "standalone" as const, parentWorkspaceId: null, name: "Alpha" },
    ];
    const sorted = sortWorkspacesWithWorktrees(items);
    // name "Alpha" first (id a < b), then "Zeta"
    expect(sorted.map((i) => i.id)).toEqual(["a", "b", "c"]);
  });

  it("sorts by default workspace first, then name", () => {
    const items = [
      { id: "a", kind: "standalone" as const, parentWorkspaceId: null, name: "Beta", defaultOpen: false },
      { id: "b", kind: "standalone" as const, parentWorkspaceId: null, name: "Alpha", defaultOpen: true },
    ];
    const sorted = sortWorkspacesWithWorktrees(items);
    // default first, then name
    expect(sorted.map((i) => i.id)).toEqual(["b", "a"]);
  });

  it("sorts repo/standalone before worktree by kind priority", () => {
    const items = [
      { id: "2", kind: "worktree" as const, parentWorkspaceId: "1", name: "feature" },
      { id: "1", kind: "repo" as const, parentWorkspaceId: null, name: "Repo" },
    ];
    const sorted = sortWorkspacesWithWorktrees(items);
    // repo first, then its worktree
    expect(sorted.map((i) => i.id)).toEqual(["1", "2"]);
  });

  it("places worktree children immediately after their parent repo", () => {
    const items = [
      { id: "repo-1", kind: "repo" as const, parentWorkspaceId: null, name: "repo-1" },
      { id: "standalone-1", kind: "standalone" as const, parentWorkspaceId: null, name: "standalone-1" },
      { id: "wt-1", kind: "worktree" as const, parentWorkspaceId: "repo-1", name: "wt-1" },
      { id: "wt-2", kind: "worktree" as const, parentWorkspaceId: "repo-1", name: "wt-2" },
    ];
    const sorted = sortWorkspacesWithWorktrees(items);
    expect(sorted.map((i) => i.id)).toEqual([
      "repo-1",
      "wt-1",
      "wt-2",
      "standalone-1",
    ]);
  });

  it("sorts by createdAt descending when names and kinds tie", () => {
    const items = [
      { id: "older", kind: "standalone" as const, parentWorkspaceId: null, name: "Project", createdAt: "2025-01-01T00:00:00Z" },
      { id: "newer", kind: "standalone" as const, parentWorkspaceId: null, name: "Project", createdAt: "2026-01-01T00:00:00Z" },
    ];
    const sorted = sortWorkspacesWithWorktrees(items);
    // Newer (higher createdAt) comes first
    expect(sorted.map((i) => i.id)).toEqual(["newer", "older"]);
  });

  it("handles orphan worktrees (parent missing) gracefully", () => {
    const items = [
      { id: "standalone-1", kind: "standalone" as const, parentWorkspaceId: null },
      { id: "wt-orphan", kind: "worktree" as const, parentWorkspaceId: "missing-repo" },
    ];
    const sorted = sortWorkspacesWithWorktrees(items);
    // Orphans should still appear — not be lost
    expect(sorted.map((i) => i.id)).toContain("wt-orphan");
    expect(sorted).toHaveLength(2);
  });

  it("handles multiple repos each with worktrees", () => {
    const items = [
      { id: "repo-a", kind: "repo" as const, parentWorkspaceId: null, name: "A" },
      { id: "repo-b", kind: "repo" as const, parentWorkspaceId: null, name: "B" },
      { id: "wt-b1", kind: "worktree" as const, parentWorkspaceId: "repo-b", name: "b1" },
      { id: "wt-a1", kind: "worktree" as const, parentWorkspaceId: "repo-a", name: "a1" },
    ];
    const sorted = sortWorkspacesWithWorktrees(items);
    const ids = sorted.map((i) => i.id);
    // Each worktree must come after its parent
    expect(ids.indexOf("wt-a1")).toBeGreaterThan(ids.indexOf("repo-a"));
    expect(ids.indexOf("wt-b1")).toBeGreaterThan(ids.indexOf("repo-b"));
  });

  it("returns empty array for empty input", () => {
    expect(sortWorkspacesWithWorktrees([])).toEqual([]);
  });

  it("sorts items without name/kind/defaultOpen/createdAt by id as fallback", () => {
    const items = [
      { id: "y", parentWorkspaceId: null },
      { id: "x", parentWorkspaceId: null },
    ];
    const sorted = sortWorkspacesWithWorktrees(items);
    expect(sorted.map((i) => i.id)).toEqual(["x", "y"]);
  });
});
