import { invoke, isTauri } from "@tauri-apps/api/core";
import type { CustomSubagent } from "@/modules/settings-center/model/types";

const requireTauri = (cmd: string) => {
  if (!isTauri()) throw new Error(`${cmd} requires Tauri runtime`);
};

// ---------------------------------------------------------------------------
// Custom Subagents
// ---------------------------------------------------------------------------

export type CustomSubagentInput = {
  name: string;
  slug: string;
  systemPrompt: string;
  invocationDescription: string;
  allowedTools: string[];
  isEnabled?: boolean;
};

export async function customSubagentList(): Promise<CustomSubagent[]> {
  requireTauri("custom_subagent_list");
  return invoke<CustomSubagent[]>("custom_subagent_list");
}

export async function customSubagentCreate(input: CustomSubagentInput): Promise<CustomSubagent> {
  requireTauri("custom_subagent_create");
  return invoke<CustomSubagent>("custom_subagent_create", { input });
}

export async function customSubagentUpdate(id: string, input: CustomSubagentInput): Promise<CustomSubagent> {
  requireTauri("custom_subagent_update");
  return invoke<CustomSubagent>("custom_subagent_update", { id, input });
}

export async function customSubagentDelete(id: string): Promise<void> {
  requireTauri("custom_subagent_delete");
  return invoke<void>("custom_subagent_delete", { id });
}

// ---------------------------------------------------------------------------
// Profile ↔ Subagent Access
// ---------------------------------------------------------------------------

export async function profileSubagentAccessGet(profileId: string): Promise<string[]> {
  requireTauri("profile_subagent_access_get");
  return invoke<string[]>("profile_subagent_access_get", { profileId });
}

export async function profileSubagentAccessSet(profileId: string, subagentIds: string[]): Promise<void> {
  requireTauri("profile_subagent_access_set");
  return invoke<void>("profile_subagent_access_set", { profileId, subagentIds });
}
