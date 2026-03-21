import {
  DEFAULT_AGENT_PROFILES,
  DEFAULT_COMMAND_SETTINGS,
  DEFAULT_GENERAL_PREFERENCES,
  DEFAULT_POLICY_SETTINGS,
  DEFAULT_SETTINGS,
  DEFAULT_WORKSPACES,
  SETTINGS_STORAGE_KEY,
  SETTINGS_STORAGE_SCHEMA_VERSION,
} from "@/modules/settings-center/model/defaults";
import type {
  AgentProfile,
  SettingsState,
} from "@/modules/settings-center/model/types";
import {
  type ApprovalPolicy,
  type NetworkAccessPolicy,
  type PromptResponseStyle,
  type SandboxPolicy,
} from "@/modules/settings-center/model/types";

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null;
}

function isPromptResponseStyle(value: unknown): value is PromptResponseStyle {
  return value === "balanced" || value === "concise" || value === "guide";
}

function isApprovalPolicy(value: unknown): value is ApprovalPolicy {
  return value === "untrusted" || value === "on-request" || value === "never";
}

function isSandboxPolicy(value: unknown): value is SandboxPolicy {
  return value === "read-only" || value === "workspace-write" || value === "full-access";
}

function isNetworkAccessPolicy(value: unknown): value is NetworkAccessPolicy {
  return value === "ask" || value === "block" || value === "allow";
}

function parseAgentProfileEntry(raw: Record<string, unknown>): AgentProfile {
  const defaultProfile = DEFAULT_AGENT_PROFILES[0];

  return {
    id: typeof raw.id === "string" ? raw.id : crypto.randomUUID(),
    name: typeof raw.name === "string" ? raw.name : "Unnamed",
    customInstructions: typeof raw.customInstructions === "string" ? raw.customInstructions : defaultProfile.customInstructions,
    commitMessagePrompt: typeof raw.commitMessagePrompt === "string" ? raw.commitMessagePrompt : defaultProfile.commitMessagePrompt,
    responseStyle: isPromptResponseStyle(raw.responseStyle) ? raw.responseStyle : defaultProfile.responseStyle,
    responseLanguage: typeof raw.responseLanguage === "string" ? raw.responseLanguage : defaultProfile.responseLanguage,
    commitMessageLanguage:
      typeof raw.commitMessageLanguage === "string"
        ? raw.commitMessageLanguage
        : defaultProfile.commitMessageLanguage,
    primaryProviderId: typeof raw.primaryProviderId === "string" ? raw.primaryProviderId : "",
    primaryModelId: typeof raw.primaryModelId === "string" ? raw.primaryModelId : "",
    assistantProviderId: typeof raw.assistantProviderId === "string" ? raw.assistantProviderId : "",
    assistantModelId: typeof raw.assistantModelId === "string" ? raw.assistantModelId : "",
    liteProviderId: typeof raw.liteProviderId === "string" ? raw.liteProviderId : "",
    liteModelId: typeof raw.liteModelId === "string" ? raw.liteModelId : "",
  };
}

function parseAgentProfiles(parsed: Record<string, unknown>): { activeAgentProfileId: string; agentProfiles: Array<AgentProfile> } {
  if (Array.isArray(parsed.agentProfiles) && parsed.agentProfiles.length > 0) {
    const profiles = (parsed.agentProfiles as Array<unknown>).filter(isRecord).map(parseAgentProfileEntry);
    const activeId = typeof parsed.activeAgentProfileId === "string" ? parsed.activeAgentProfileId : profiles[0]?.id ?? "default-profile";

    return {
      agentProfiles: profiles.length > 0 ? profiles : DEFAULT_AGENT_PROFILES,
      activeAgentProfileId: profiles.some((profile) => profile.id === activeId) ? activeId : profiles[0]?.id ?? "default-profile",
    };
  }

  return {
    agentProfiles: DEFAULT_AGENT_PROFILES,
    activeAgentProfileId: DEFAULT_AGENT_PROFILES[0]?.id ?? "default-profile",
  };
}

export function readStoredSettings(): SettingsState {
  if (typeof window === "undefined") {
    return DEFAULT_SETTINGS;
  }

  const rawValue = window.localStorage.getItem(SETTINGS_STORAGE_KEY);

  if (!rawValue) {
    return DEFAULT_SETTINGS;
  }

  try {
    const parsed = JSON.parse(rawValue) as unknown;

    if (!isRecord(parsed)) {
      return DEFAULT_SETTINGS;
    }

    const schemaVersion = typeof parsed.schemaVersion === "number" ? parsed.schemaVersion : 0;
    if (schemaVersion < SETTINGS_STORAGE_SCHEMA_VERSION) {
      return DEFAULT_SETTINGS;
    }

    const generalRaw = isRecord(parsed.general) ? parsed.general : {};
    const workspaces = Array.isArray(parsed.workspaces) ? parsed.workspaces : null;
    const commandsRaw = isRecord(parsed.commands) ? parsed.commands : {};
    const policyRaw = isRecord(parsed.policy) ? parsed.policy : {};

    return {
      general: {
        launchAtLogin: typeof generalRaw.launchAtLogin === "boolean" ? generalRaw.launchAtLogin : DEFAULT_GENERAL_PREFERENCES.launchAtLogin,
        preventSleepWhileRunning:
          typeof generalRaw.preventSleepWhileRunning === "boolean"
            ? generalRaw.preventSleepWhileRunning
            : DEFAULT_GENERAL_PREFERENCES.preventSleepWhileRunning,
        minimizeToTray: typeof generalRaw.minimizeToTray === "boolean" ? generalRaw.minimizeToTray : DEFAULT_GENERAL_PREFERENCES.minimizeToTray,
      },
      workspaces: workspaces
        ? (workspaces as Array<unknown>).filter(isRecord).map((entry) => ({
            id: typeof entry.id === "string" ? entry.id : crypto.randomUUID(),
            name: typeof entry.name === "string" ? entry.name : "Unnamed",
            path: typeof entry.path === "string" ? entry.path : "",
            isDefault: typeof entry.isDefault === "boolean" ? entry.isDefault : false,
            isGit: typeof entry.isGit === "boolean" ? entry.isGit : false,
            autoWorkTree: typeof entry.autoWorkTree === "boolean" ? entry.autoWorkTree : false,
          }))
        : DEFAULT_WORKSPACES,
      providers: [],
      commands: {
        commands: (() => {
          const rawCommands = Array.isArray(commandsRaw.commands) ? commandsRaw.commands : null;

          return rawCommands
            ? (rawCommands as Array<unknown>).filter(isRecord).map((command) => ({
                id: typeof command.id === "string" ? command.id : crypto.randomUUID(),
                name: typeof command.name === "string" ? command.name : "",
                path: typeof command.path === "string" ? command.path : "",
                argumentHint: typeof command.argumentHint === "string" ? command.argumentHint : "",
                description: typeof command.description === "string" ? command.description : "",
              }))
            : DEFAULT_COMMAND_SETTINGS.commands;
        })(),
      },
      ...parseAgentProfiles(parsed),
      policy: {
        approvalPolicy: isApprovalPolicy(policyRaw.approvalPolicy)
          ? policyRaw.approvalPolicy
          : DEFAULT_POLICY_SETTINGS.approvalPolicy,
        allowList: Array.isArray(policyRaw.allowList)
          ? (policyRaw.allowList as Array<unknown>).filter(isRecord).map((entry) => ({
              id: typeof entry.id === "string" ? entry.id : crypto.randomUUID(),
              pattern: typeof entry.pattern === "string" ? entry.pattern : "",
            }))
          : DEFAULT_POLICY_SETTINGS.allowList,
        denyList: Array.isArray(policyRaw.denyList)
          ? (policyRaw.denyList as Array<unknown>).filter(isRecord).map((entry) => ({
              id: typeof entry.id === "string" ? entry.id : crypto.randomUUID(),
              pattern: typeof entry.pattern === "string" ? entry.pattern : "",
            }))
          : DEFAULT_POLICY_SETTINGS.denyList,
        sandboxPolicy: isSandboxPolicy(policyRaw.sandboxPolicy)
          ? policyRaw.sandboxPolicy
          : DEFAULT_POLICY_SETTINGS.sandboxPolicy,
        networkAccess: isNetworkAccessPolicy(policyRaw.networkAccess)
          ? policyRaw.networkAccess
          : DEFAULT_POLICY_SETTINGS.networkAccess,
        writableRoots: Array.isArray(policyRaw.writableRoots)
          ? (policyRaw.writableRoots as Array<unknown>).filter(isRecord).map((entry) => ({
              id: typeof entry.id === "string" ? entry.id : crypto.randomUUID(),
              path: typeof entry.path === "string" ? entry.path : "",
            }))
          : DEFAULT_POLICY_SETTINGS.writableRoots,
      },
    };
  } catch {
    return DEFAULT_SETTINGS;
  }
}

export function persistSettings(settings: SettingsState) {
  if (typeof window === "undefined") {
    return;
  }

  window.localStorage.setItem(SETTINGS_STORAGE_KEY, JSON.stringify({
    schemaVersion: SETTINGS_STORAGE_SCHEMA_VERSION,
    ...settings,
    providers: [],
  }));
}

export function persistLocalUiSettings(settings: SettingsState) {
  if (typeof window === "undefined") {
    return;
  }

  window.localStorage.setItem(SETTINGS_STORAGE_KEY, JSON.stringify({
    schemaVersion: SETTINGS_STORAGE_SCHEMA_VERSION,
    general: settings.general,
    workspaces: settings.workspaces,
    commands: settings.commands,
    providers: [],
  }));
}
