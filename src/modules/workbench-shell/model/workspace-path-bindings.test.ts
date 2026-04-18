import { describe, expect, it } from "vitest";
import type { WorkspaceItem } from "@/modules/workbench-shell/model/types";
import {
  addRemovedWorkspacePath,
  buildWorkspaceBindings,
  deleteRemovedWorkspacePath,
  findWorkspaceByPath,
  getWorkspaceBindingId,
  hasRemovedWorkspacePath,
  resolveProjectForWorkspace,
} from "@/modules/workbench-shell/model/workspace-path-bindings";
import type { WorkspaceDto } from "@/shared/types/api";

function createWorkspace(overrides: Partial<WorkspaceDto> = {}): WorkspaceDto {
  return {
    id: "workspace-1",
    name: "Repo",
    path: "C:\\Users\\Buddy\\Repo",
    canonicalPath: "c:/users/buddy/repo",
    displayPath: "~/Repo",
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
    branch: null,
    worktreeName: null,
    ...overrides,
  };
}

function createWorkspaceItem(
  overrides: Partial<WorkspaceItem> = {},
): WorkspaceItem {
  return {
    id: "workspace-1",
    name: "Repo",
    defaultOpen: false,
    threads: [],
    path: "C:\\Users\\Buddy\\Repo",
    ...overrides,
  };
}

describe("workspace path bindings", () => {
  it("normalizes path variants when building and resolving workspace bindings", () => {
    const bindings = buildWorkspaceBindings([
      createWorkspace(),
      createWorkspace({
        id: "workspace-2",
        name: "Share",
        path: "//SERVER/Share/Docs",
        canonicalPath: "//server/share/docs",
      }),
    ]);

    expect(getWorkspaceBindingId(bindings, "c:/Users/BUDDY/Repo/./"))
      .toBe("workspace-1");
    expect(getWorkspaceBindingId(bindings, "///server/share/docs"))
      .toBe("workspace-2");
  });

  it("finds workspaces by either display path or canonical path", () => {
    const workspace = findWorkspaceByPath(
      [
        createWorkspace({
          path: "/Users/buddy/repo",
          canonicalPath: "/private/Users/buddy/repo",
        }),
      ],
      "/private/Users/buddy/repo/./",
    );

    expect(workspace?.id).toBe("workspace-1");
  });

  it("tracks removed workspace paths using normalized keys", () => {
    const removedPaths = new Set<string>();

    addRemovedWorkspacePath(removedPaths, "//SERVER/Share//Docs");
    expect(hasRemovedWorkspacePath(removedPaths, "///server/share/docs"))
      .toBe(true);

    deleteRemovedWorkspacePath(removedPaths, "//server/share/docs/./");
    expect(hasRemovedWorkspacePath(removedPaths, "//SERVER/Share/Docs"))
      .toBe(false);
  });

  it("matches recent projects by normalized workspace path before falling back", () => {
    const project = resolveProjectForWorkspace(
      createWorkspaceItem(),
      [
        {
          id: "project-1",
          name: "Repo Alias",
          path: "c:/users/buddy/repo/",
          lastOpenedLabel: "Today",
        },
      ],
    );

    expect(project?.id).toBe("project-1");
  });

  it("builds a fallback project when no recent project matches", () => {
    const project = resolveProjectForWorkspace(
      createWorkspaceItem({
        id: "workspace-2",
        name: "Docs",
        path: "/Users/buddy/docs",
      }),
      [],
    );

    expect(project).not.toBeNull();
    expect(project?.path).toBe("/Users/buddy/docs");
    expect(project?.name).toBe("docs");
    expect(project?.id).toContain("docs");
  });
});
