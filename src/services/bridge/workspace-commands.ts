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
  return invoke<WorkspaceDto>("workspace_add", { path, name });
}

export async function workspaceRemove(id: string): Promise<void> {
  return invoke("workspace_remove", { id });
}

export async function workspaceSetDefault(id: string): Promise<void> {
  return invoke("workspace_set_default", { id });
}

export async function workspaceValidate(id: string): Promise<WorkspaceDto> {
  return invoke<WorkspaceDto>("workspace_validate", { id });
}
