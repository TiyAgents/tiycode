import { invoke, isTauri } from "@tauri-apps/api/core";
import type { WorkspaceDto } from "@/shared/types/api";

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
