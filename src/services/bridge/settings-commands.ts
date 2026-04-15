import { invoke, isTauri } from "@tauri-apps/api/core";
import type {
  AgentProfileDto,
  AgentProfileInput,
  CustomProviderCreateInput,
  PromptCommandDto,
  PromptCommandInput,
  ProviderCatalogEntryDto,
  ProviderModelConnectionTestResultDto,
  ProviderSettingsDto,
  ProviderSettingsUpdateInput,
  SettingDto,
} from "@/shared/types/api";

const requireTauri = (cmd: string) => {
  if (!isTauri()) throw new Error(`${cmd} requires Tauri runtime`);
};

// ---------------------------------------------------------------------------
// Settings KV
// ---------------------------------------------------------------------------

export async function settingsGet(key: string): Promise<SettingDto | null> {
  if (!isTauri()) return null;
  return invoke<SettingDto | null>("settings_get", { key });
}

export async function settingsGetAll(): Promise<SettingDto[]> {
  if (!isTauri()) return [];
  return invoke<SettingDto[]>("settings_get_all");
}

export async function settingsSet(key: string, value: string): Promise<void> {
  requireTauri("settings_set");
  return invoke("settings_set", { key, value });
}

// ---------------------------------------------------------------------------
// Policies KV
// ---------------------------------------------------------------------------

export async function policyGet(key: string): Promise<SettingDto | null> {
  if (!isTauri()) return null;
  return invoke<SettingDto | null>("policy_get", { key });
}

export async function policyGetAll(): Promise<SettingDto[]> {
  if (!isTauri()) return [];
  return invoke<SettingDto[]>("policy_get_all");
}

export async function policySet(key: string, value: string): Promise<void> {
  requireTauri("policy_set");
  return invoke("policy_set", { key, value });
}

// ---------------------------------------------------------------------------
// Providers
// ---------------------------------------------------------------------------

export async function providerCatalogList(): Promise<ProviderCatalogEntryDto[]> {
  if (!isTauri()) return [];
  return invoke<ProviderCatalogEntryDto[]>("provider_catalog_list");
}

export async function providerSettingsGetAll(): Promise<ProviderSettingsDto[]> {
  if (!isTauri()) return [];
  return invoke<ProviderSettingsDto[]>("provider_settings_get_all");
}

export async function providerSettingsFetchModels(id: string): Promise<ProviderSettingsDto> {
  requireTauri("provider_settings_fetch_models");
  return invoke<ProviderSettingsDto>("provider_settings_fetch_models", { id });
}

export async function providerSettingsUpsertBuiltin(
  providerKey: string,
  input: ProviderSettingsUpdateInput,
): Promise<ProviderSettingsDto> {
  requireTauri("provider_settings_upsert_builtin");
  return invoke<ProviderSettingsDto>("provider_settings_upsert_builtin", { providerKey, input });
}

export async function providerSettingsCreateCustom(
  input: CustomProviderCreateInput,
): Promise<ProviderSettingsDto> {
  requireTauri("provider_settings_create_custom");
  return invoke<ProviderSettingsDto>("provider_settings_create_custom", { input });
}

export async function providerSettingsUpdateCustom(
  id: string,
  input: ProviderSettingsUpdateInput,
): Promise<ProviderSettingsDto> {
  requireTauri("provider_settings_update_custom");
  return invoke<ProviderSettingsDto>("provider_settings_update_custom", { id, input });
}

export async function providerSettingsDeleteCustom(id: string): Promise<void> {
  requireTauri("provider_settings_delete_custom");
  return invoke("provider_settings_delete_custom", { id });
}

export async function providerModelTestConnection(
  providerId: string,
  modelId: string,
): Promise<ProviderModelConnectionTestResultDto> {
  requireTauri("provider_model_test_connection");
  return invoke<ProviderModelConnectionTestResultDto>("provider_model_test_connection", {
    providerId,
    modelId,
  });
}

// ---------------------------------------------------------------------------
// Agent Profiles
// ---------------------------------------------------------------------------

export async function profileList(): Promise<AgentProfileDto[]> {
  if (!isTauri()) return [];
  return invoke<AgentProfileDto[]>("profile_list");
}

export async function profileCreate(input: AgentProfileInput): Promise<AgentProfileDto> {
  requireTauri("profile_create");
  return invoke<AgentProfileDto>("profile_create", { input });
}

export async function profileUpdate(
  id: string,
  input: AgentProfileInput,
): Promise<AgentProfileDto> {
  requireTauri("profile_update");
  return invoke<AgentProfileDto>("profile_update", { id, input });
}

export async function profileDelete(id: string): Promise<void> {
  requireTauri("profile_delete");
  return invoke("profile_delete", { id });
}

// ---------------------------------------------------------------------------
// Prompt Commands
// ---------------------------------------------------------------------------

export async function promptCommandList(): Promise<PromptCommandDto[]> {
  if (!isTauri()) return [];
  return invoke<PromptCommandDto[]>("prompt_command_list");
}

export async function promptCommandCreate(input: PromptCommandInput): Promise<PromptCommandDto> {
  requireTauri("prompt_command_create");
  return invoke<PromptCommandDto>("prompt_command_create", { input });
}

export async function promptCommandUpdate(id: string, input: PromptCommandInput): Promise<PromptCommandDto> {
  requireTauri("prompt_command_update");
  return invoke<PromptCommandDto>("prompt_command_update", { id, input });
}

export async function promptCommandDelete(id: string): Promise<void> {
  requireTauri("prompt_command_delete");
  return invoke("prompt_command_delete", { id });
}
