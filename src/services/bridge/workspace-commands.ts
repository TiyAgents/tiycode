import { invoke, isTauri } from "@tauri-apps/api/core";
import type {
  WorkspaceDto,
  WorktreeCreateInput,
  WorktreeInfoDto,
} from "@/shared/types/api";

export async function workspaceList(): Promise<WorkspaceDto[]> {
  if (!isTauri()) return [];
  return invoke<WorkspaceDto[]>("workspace_list");
}

export async function workspaceAdd(
  path: string,
  name?: string,
): Promise<WorkspaceDto> {
  if (!isTauri()) throw new Error("workspace_add requires Tauri runtime");
  return invoke<WorkspaceDto>("workspace_add", { path, name });
}

export async function workspaceEnsureDefault(): Promise<WorkspaceDto> {
  if (!isTauri()) {
    throw new Error("workspace_ensure_default requires Tauri runtime");
  }
  return invoke<WorkspaceDto>("workspace_ensure_default");
}

export async function workspaceRemove(id: string): Promise<void> {
  if (!isTauri()) throw new Error("workspace_remove requires Tauri runtime");
  return invoke("workspace_remove", { id });
}

export async function workspaceSetDefault(id: string): Promise<void> {
  if (!isTauri()) throw new Error("workspace_set_default requires Tauri runtime");
  return invoke("workspace_set_default", { id });
}

export async function workspaceValidate(id: string): Promise<WorkspaceDto> {
  if (!isTauri()) throw new Error("workspace_validate requires Tauri runtime");
  return invoke<WorkspaceDto>("workspace_validate", { id });
}

// ---------------------------------------------------------------------------
// Worktree bridge
// ---------------------------------------------------------------------------

export async function workspaceListWorktrees(
  workspaceId: string,
): Promise<WorktreeInfoDto[]> {
  if (!isTauri()) return [];
  return invoke<WorktreeInfoDto[]>("workspace_list_worktrees", { workspaceId });
}

export async function workspaceCreateWorktree(
  workspaceId: string,
  input: WorktreeCreateInput,
): Promise<WorkspaceDto> {
  if (!isTauri()) {
    throw new Error("workspace_create_worktree requires Tauri runtime");
  }
  return invoke<WorkspaceDto>("workspace_create_worktree", {
    workspaceId,
    input,
  });
}

export async function workspaceRemoveWorktree(
  id: string,
  force?: boolean,
): Promise<void> {
  if (!isTauri()) {
    throw new Error("workspace_remove_worktree requires Tauri runtime");
  }
  return invoke("workspace_remove_worktree", {
    id,
    force: force ?? true,
  });
}

export async function workspacePruneWorktrees(
  workspaceId: string,
): Promise<void> {
  if (!isTauri()) {
    throw new Error("workspace_prune_worktrees requires Tauri runtime");
  }
  return invoke("workspace_prune_worktrees", { workspaceId });
}
