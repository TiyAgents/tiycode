import { invoke, isTauri } from "@tauri-apps/api/core";
import type {
  SettingDto,
  ProviderDto,
  ProviderInput,
  ProviderModelDto,
  ProviderModelInput,
  AgentProfileDto,
  AgentProfileInput,
} from "@/shared/types/api";

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

export async function settingsSet(
  key: string,
  value: string,
): Promise<void> {
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
  return invoke("policy_set", { key, value });
}

// ---------------------------------------------------------------------------
// Providers
// ---------------------------------------------------------------------------

export async function providerList(): Promise<ProviderDto[]> {
  if (!isTauri()) return [];
  return invoke<ProviderDto[]>("provider_list");
}

export async function providerCreate(
  input: ProviderInput,
): Promise<ProviderDto> {
  return invoke<ProviderDto>("provider_create", { input });
}

export async function providerUpdate(
  id: string,
  input: ProviderInput,
): Promise<ProviderDto> {
  return invoke<ProviderDto>("provider_update", { id, input });
}

export async function providerDelete(id: string): Promise<void> {
  return invoke("provider_delete", { id });
}

// ---------------------------------------------------------------------------
// Provider Models
// ---------------------------------------------------------------------------

export async function providerModelList(
  providerId: string,
): Promise<ProviderModelDto[]> {
  if (!isTauri()) return [];
  return invoke<ProviderModelDto[]>("provider_model_list", { providerId });
}

export async function providerModelAdd(
  providerId: string,
  input: ProviderModelInput,
): Promise<ProviderModelDto> {
  return invoke<ProviderModelDto>("provider_model_add", { providerId, input });
}

export async function providerModelRemove(id: string): Promise<void> {
  return invoke("provider_model_remove", { id });
}

// ---------------------------------------------------------------------------
// Agent Profiles
// ---------------------------------------------------------------------------

export async function profileList(): Promise<AgentProfileDto[]> {
  if (!isTauri()) return [];
  return invoke<AgentProfileDto[]>("profile_list");
}

export async function profileCreate(
  input: AgentProfileInput,
): Promise<AgentProfileDto> {
  return invoke<AgentProfileDto>("profile_create", { input });
}

export async function profileUpdate(
  id: string,
  input: AgentProfileInput,
): Promise<AgentProfileDto> {
  return invoke<AgentProfileDto>("profile_update", { id, input });
}

export async function profileDelete(id: string): Promise<void> {
  return invoke("profile_delete", { id });
}
