import { createStore } from "@/shared/lib/create-store";
import {
  DEFAULT_SETTINGS,
} from "@/modules/settings-center/model/defaults";
import type {
  AgentProfile,
  CommandEntry,
  GeneralPreferences,
  PolicySettings,
  ProviderCatalogEntry,
  ProviderEntry,
  TerminalSettings,
  WorkspaceEntry,
} from "@/modules/settings-center/model/types";

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

export type HydrationPhase =
  | "uninitialized"
  | "loading_phase1"
  | "phase1_ready"
  | "loading_phase2"
  | "hydrated"
  | "error";

export interface SettingsStoreState {
  [key: string]: unknown;
  providers: Array<ProviderEntry>;
  agentProfiles: Array<AgentProfile>;
  activeAgentProfileId: string;
  workspaces: Array<WorkspaceEntry>;
  providerCatalog: Array<ProviderCatalogEntry>;
  general: GeneralPreferences;
  terminal: TerminalSettings;
  policy: PolicySettings;
  commands: Array<CommandEntry>;
  availableShells: Array<{ path: string; name: string }>;
  hydrationPhase: HydrationPhase;
}

// ---------------------------------------------------------------------------
// Store
// ---------------------------------------------------------------------------

export const settingsStore = createStore<SettingsStoreState>({
  providers: DEFAULT_SETTINGS.providers,
  agentProfiles: DEFAULT_SETTINGS.agentProfiles,
  activeAgentProfileId: DEFAULT_SETTINGS.activeAgentProfileId,
  workspaces: DEFAULT_SETTINGS.workspaces,
  providerCatalog: [],
  general: DEFAULT_SETTINGS.general,
  terminal: DEFAULT_SETTINGS.terminal,
  policy: DEFAULT_SETTINGS.policy,
  commands: DEFAULT_SETTINGS.commands.commands,
  availableShells: [],
  hydrationPhase: "uninitialized",
});
