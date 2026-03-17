import {
  DEFAULT_AGENT_PROFILES,
  DEFAULT_COMMAND_SETTINGS,
  DEFAULT_GENERAL_PREFERENCES,
  DEFAULT_POLICY_SETTINGS,
  DEFAULT_PROVIDERS,
  DEFAULT_SETTINGS,
  DEFAULT_WORKSPACES,
  SETTINGS_STORAGE_KEY,
} from "@/modules/settings-center/model/defaults";
import type {
  AgentProfile,
  ApiProtocol,
  ProviderModelCapabilities,
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

function parseCustomHeaders(value: unknown): Record<string, string> {
  if (!isRecord(value)) {
    return {};
  }

  return Object.fromEntries(
    Object.entries(value).filter((entry): entry is [string, string] => typeof entry[1] === "string"),
  );
}

function parseProviderOptions(value: unknown): Record<string, unknown> {
  return isRecord(value) ? value : {};
}

function parseCapabilityOverrides(value: unknown): Partial<ProviderModelCapabilities> {
  if (!isRecord(value)) {
    return {};
  }

  const entries = Object.entries(value).filter(
    (entry): entry is [keyof ProviderModelCapabilities, boolean] =>
      ["vision", "imageOutput", "toolCalling", "reasoning", "embedding"].includes(entry[0]) && typeof entry[1] === "boolean",
  );

  return Object.fromEntries(entries);
}

function parseAgentProfileEntry(raw: Record<string, unknown>): AgentProfile {
  const defaultProfile = DEFAULT_AGENT_PROFILES[0];

  return {
    id: typeof raw.id === "string" ? raw.id : crypto.randomUUID(),
    name: typeof raw.name === "string" ? raw.name : "Unnamed",
    customInstructions: typeof raw.customInstructions === "string" ? raw.customInstructions : defaultProfile.customInstructions,
    responseStyle: isPromptResponseStyle(raw.responseStyle) ? raw.responseStyle : defaultProfile.responseStyle,
    responseLanguage: typeof raw.responseLanguage === "string" ? raw.responseLanguage : defaultProfile.responseLanguage,
    primaryModel: typeof raw.primaryModel === "string" ? raw.primaryModel : defaultProfile.primaryModel,
    assistantModel: typeof raw.assistantModel === "string" ? raw.assistantModel : defaultProfile.assistantModel,
    liteModel: typeof raw.liteModel === "string" ? raw.liteModel : defaultProfile.liteModel,
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

    const generalRaw = isRecord(parsed.general) ? parsed.general : {};
    const workspaces = Array.isArray(parsed.workspaces) ? parsed.workspaces : null;
    const providers = Array.isArray(parsed.providers) ? parsed.providers : null;
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
      providers: providers
        ? (providers as Array<unknown>).filter(isRecord).map((entry) => ({
            id: typeof entry.id === "string" ? entry.id : crypto.randomUUID(),
            name: typeof entry.name === "string" ? entry.name : "Unnamed",
            baseUrl: typeof entry.baseUrl === "string" ? entry.baseUrl : "",
            apiKey: typeof entry.apiKey === "string" ? entry.apiKey : "",
            apiProtocol: (["chat-completions", "responses", "anthropic", "gemini", "ollama"] as const).includes(entry.apiProtocol as ApiProtocol)
              ? (entry.apiProtocol as ApiProtocol)
              : "chat-completions",
            customHeaders: parseCustomHeaders(entry.customHeaders),
            enabled: typeof entry.enabled === "boolean" ? entry.enabled : false,
            isCustom: typeof entry.isCustom === "boolean" ? entry.isCustom : false,
            models: Array.isArray(entry.models)
              ? (entry.models as Array<unknown>).filter(isRecord).map((model) => ({
                  id: typeof model.id === "string" ? model.id : crypto.randomUUID(),
                  modelId: typeof model.modelId === "string" ? model.modelId : "",
                  displayName: typeof model.displayName === "string" ? model.displayName : "",
                  enabled: typeof model.enabled === "boolean" ? model.enabled : false,
                  contextWindow: typeof model.contextWindow === "string" ? model.contextWindow : undefined,
                  maxOutputTokens: typeof model.maxOutputTokens === "string" ? model.maxOutputTokens : undefined,
                  capabilityOverrides: parseCapabilityOverrides(model.capabilityOverrides),
                  providerOptions: parseProviderOptions(model.providerOptions),
                  isManual: typeof model.isManual === "boolean" ? model.isManual : undefined,
                }))
              : [],
          }))
        : DEFAULT_PROVIDERS,
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

  window.localStorage.setItem(SETTINGS_STORAGE_KEY, JSON.stringify(settings));
}
