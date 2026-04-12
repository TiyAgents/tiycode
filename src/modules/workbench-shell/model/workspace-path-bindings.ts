import { buildProjectOptionFromPath } from "@/modules/workbench-shell/model/helpers";
import type {
  ProjectOption,
  WorkspaceItem,
} from "@/modules/workbench-shell/model/types";
import { buildWorkspacePathKeys, isSameWorkspacePath, normalizeWorkspacePath } from "@/shared/lib/workspace-path";
import type { WorkspaceDto } from "@/shared/types/api";

type WorkspacePathLike = {
  path?: string | null;
  canonicalPath?: string | null;
};

export function buildWorkspaceBindingEntries(
  workspaceId: string,
  ...paths: Array<string | null | undefined>
) {
  return Object.fromEntries(
    buildWorkspacePathKeys(...paths).map((pathKey) => [pathKey, workspaceId]),
  );
}

export function buildWorkspaceBindings(
  workspaceEntries: ReadonlyArray<Pick<WorkspaceDto, "id" | "path" | "canonicalPath">>,
) {
  const bindings: Record<string, string> = {};

  for (const workspace of workspaceEntries) {
    Object.assign(bindings, buildWorkspaceBindingsForEntry(workspace));
  }

  return bindings;
}

export function buildWorkspaceBindingsForEntry(
  workspace: Pick<WorkspaceDto, "id" | "path" | "canonicalPath">,
  ...additionalPaths: Array<string | null | undefined>
) {
  return buildWorkspaceBindingEntries(
    workspace.id,
    workspace.path,
    workspace.canonicalPath,
    ...additionalPaths,
  );
}

export function findWorkspaceByPath<T extends WorkspacePathLike>(
  workspaces: ReadonlyArray<T>,
  path: string | null | undefined,
) {
  return (
    workspaces.find(
      (workspace) =>
        isSameWorkspacePath(workspace.path, path)
        || isSameWorkspacePath(workspace.canonicalPath, path),
    ) ?? null
  );
}

export function getWorkspaceBindingId(
  bindings: Readonly<Record<string, string>>,
  path: string | null | undefined,
) {
  return bindings[normalizeWorkspacePath(path)] ?? null;
}

export function hasRemovedWorkspacePath(
  removedPaths: ReadonlySet<string>,
  path: string | null | undefined,
) {
  return removedPaths.has(normalizeWorkspacePath(path));
}

export function addRemovedWorkspacePath(
  removedPaths: Set<string>,
  path: string | null | undefined,
) {
  const pathKey = normalizeWorkspacePath(path);
  if (!pathKey) {
    return;
  }

  removedPaths.add(pathKey);
}

export function deleteRemovedWorkspacePath(
  removedPaths: Set<string>,
  path: string | null | undefined,
) {
  const pathKey = normalizeWorkspacePath(path);
  if (!pathKey) {
    return;
  }

  removedPaths.delete(pathKey);
}

export function resolveProjectForWorkspace(
  workspace: Pick<WorkspaceItem, "id" | "name" | "path"> | null,
  recentProjects: ReadonlyArray<ProjectOption>,
) {
  if (!workspace) {
    return null;
  }

  const matchedProject = recentProjects.find(
    (project) =>
      isSameWorkspacePath(project.path, workspace.path)
      || project.id === workspace.id
      || project.name === workspace.name,
  );

  if (matchedProject) {
    return matchedProject;
  }

  if (!workspace.path) {
    return null;
  }

  return buildProjectOptionFromPath(workspace.path);
}
