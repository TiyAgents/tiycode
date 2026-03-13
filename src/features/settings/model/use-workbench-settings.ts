import { useEffect, useState } from "react";

export type SettingsCategory = "account" | "general" | "workspace" | "providers" | "commands" | "policy" | "about";
export type PromptResponseStyle = "balanced" | "concise" | "guide";
export type ApprovalPolicy = "untrusted" | "on-request" | "never";
export type SandboxPolicy = "read-only" | "workspace-write" | "full-access";
export type NetworkAccessPolicy = "ask" | "block" | "allow";
export type PatternEntry = { id: string; pattern: string };
export type WritableRootEntry = { id: string; path: string };

export type WorkspaceEntry = {
  id: string;
  name: string;
  path: string;
  isDefault: boolean;
  isGit: boolean;
  autoWorkTree: boolean;
};

export type ApiProtocol = "chat-completions" | "responses" | "anthropic" | "gemini" | "ollama";

export type ProviderModel = {
  id: string;
  modelId: string;
  displayName: string;
  enabled: boolean;
  contextWindow?: string;
  isManual?: boolean;
};

export type ProviderEntry = {
  id: string;
  name: string;
  baseUrl: string;
  apiKey: string;
  apiProtocol: ApiProtocol;
  enabled: boolean;
  isCustom: boolean;
  models: Array<ProviderModel>;
};

export type CommandEntry = {
  id: string;
  name: string;
  path: string;
  argumentHint: string;
  description: string;
};

export type AgentProfile = {
  id: string;
  name: string;
  customInstructions: string;
  responseStyle: PromptResponseStyle;
  responseLanguage: string;
  primaryModel: string;
  assistantModel: string;
  liteModel: string;
};

export type CommandSettings = {
  commands: Array<CommandEntry>;
};

export type GeneralPreferences = {
  launchAtLogin: boolean;
  minimizeToTray: boolean;
};

export type PolicySettings = {
  approvalPolicy: ApprovalPolicy;
  allowList: Array<PatternEntry>;
  denyList: Array<PatternEntry>;
  sandboxPolicy: SandboxPolicy;
  networkAccess: NetworkAccessPolicy;
  writableRoots: Array<WritableRootEntry>;
};

type WorkbenchSettingsState = {
  general: GeneralPreferences;
  workspaces: Array<WorkspaceEntry>;
  providers: Array<ProviderEntry>;
  commands: CommandSettings;
  policy: PolicySettings;
  agentProfiles: Array<AgentProfile>;
  activeAgentProfileId: string;
};

const STORAGE_KEY = "tiy-agent-workbench-settings";

const DEFAULT_CUSTOM_INSTRUCTIONS =
  "You are Tiy Agent, a desktop coding partner. Keep answers crisp, grounded in the local workspace, and explicit about risks before taking action.";

const DEFAULT_AGENT_PROFILES: Array<AgentProfile> = [{
  id: "default-profile",
  name: "Default",
  customInstructions: DEFAULT_CUSTOM_INSTRUCTIONS,
  responseStyle: "balanced",
  responseLanguage: "English",
  primaryModel: "",
  assistantModel: "",
  liteModel: "",
}];

const DEFAULT_COMMAND_SETTINGS: CommandSettings = {
  commands: [
    {
      id: "cmd-commit",
      name: "commit",
      path: "/prompts:commit",
      argumentHint: "[--verify=yes|no] [--style=simple|full] [--type=feat|fix|docs|style|refactor|perf|test|chore|ci|build|revert] [--language=english|chinese]",
      description: "Create well-formatted commits with conventional commit messages",
    },
    {
      id: "cmd-create-pr",
      name: "create-pr",
      path: "/prompts:create-pr",
      argumentHint: "[--draft] [--base=main|master] [--style=simple|full] [--language=english|chinese]",
      description: "Create pull requests via GitHub MCP tools with well-formatted PR title and description",
    },
  ],
};

const DEFAULT_WORKSPACES: Array<WorkspaceEntry> = [
  {
    id: "tiy-desktop",
    name: "tiy-desktop",
    path: "/Users/jorben/Documents/Codespace/TiyAgents/tiy-desktop",
    isDefault: true,
    isGit: true,
    autoWorkTree: false,
  },
  {
    id: "default",
    name: "Default",
    path: "/Users/jorben/Library/Application Support/tiy/workspaces/default",
    isDefault: false,
    isGit: false,
    autoWorkTree: false,
  },
];

const DEFAULT_PROVIDERS: Array<ProviderEntry> = [
  {
    id: "zenmux",
    name: "ZenMux",
    baseUrl: "https://zenmux.ai/api/v1",
    apiKey: "sk-zenmux-xxxxxxxxxxxxxxxxxxxxxxxxxxxx",
    apiProtocol: "responses",
    enabled: true,
    isCustom: true,
    models: [
      { id: "m1", modelId: "openai/gpt-5.4", displayName: "GPT-5.4", enabled: true, isManual: true },
      { id: "m2", modelId: "stepfun/step-3.5-flash", displayName: "stepfun/step-3.5-flash", enabled: true, contextWindow: "256K" },
      { id: "m3", modelId: "anthropic/claude-3.5-haiku", displayName: "anthropic/claude-3.5-haiku", enabled: false, contextWindow: "200K" },
      { id: "m4", modelId: "anthropic/claude-3.7-sonnet", displayName: "anthropic/claude-3.7-sonnet", enabled: false, contextWindow: "200K" },
    ],
  },
  {
    id: "openai",
    name: "OpenAI",
    baseUrl: "https://api.openai.com/v1",
    apiKey: "",
    apiProtocol: "chat-completions",
    enabled: false,
    isCustom: false,
    models: [],
  },
  {
    id: "anthropic",
    name: "Anthropic",
    baseUrl: "https://api.anthropic.com/v1",
    apiKey: "",
    apiProtocol: "chat-completions",
    enabled: false,
    isCustom: false,
    models: [],
  },
  {
    id: "google-gemini",
    name: "Google Gemini",
    baseUrl: "https://generativelanguage.googleapis.com/v1beta",
    apiKey: "",
    apiProtocol: "chat-completions",
    enabled: false,
    isCustom: false,
    models: [],
  },
  {
    id: "deepseek",
    name: "DeepSeek",
    baseUrl: "https://api.deepseek.com/v1",
    apiKey: "",
    apiProtocol: "chat-completions",
    enabled: false,
    isCustom: false,
    models: [],
  },
  {
    id: "moonshot",
    name: "Moonshot",
    baseUrl: "https://api.moonshot.cn/v1",
    apiKey: "",
    apiProtocol: "chat-completions",
    enabled: false,
    isCustom: false,
    models: [],
  },
  {
    id: "openrouter",
    name: "OpenRouter",
    baseUrl: "https://openrouter.ai/api/v1",
    apiKey: "",
    apiProtocol: "chat-completions",
    enabled: false,
    isCustom: false,
    models: [],
  },
];

const DEFAULT_GENERAL_PREFERENCES: GeneralPreferences = {
  launchAtLogin: false,
  minimizeToTray: false,
};

const DEFAULT_POLICY_SETTINGS: PolicySettings = {
  approvalPolicy: "on-request",
  allowList: [],
  denyList: [],
  sandboxPolicy: "workspace-write",
  networkAccess: "ask",
  writableRoots: [],
};

const DEFAULT_SETTINGS: WorkbenchSettingsState = {
  general: DEFAULT_GENERAL_PREFERENCES,
  workspaces: DEFAULT_WORKSPACES,
  providers: DEFAULT_PROVIDERS,
  commands: DEFAULT_COMMAND_SETTINGS,
  policy: DEFAULT_POLICY_SETTINGS,
  agentProfiles: DEFAULT_AGENT_PROFILES,
  activeAgentProfileId: "default-profile",
};

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
    responseStyle: isPromptResponseStyle(raw.responseStyle) ? raw.responseStyle : defaultProfile.responseStyle,
    responseLanguage: typeof raw.responseLanguage === "string" ? raw.responseLanguage : defaultProfile.responseLanguage,
    primaryModel: typeof raw.primaryModel === "string" ? raw.primaryModel : defaultProfile.primaryModel,
    assistantModel: typeof raw.assistantModel === "string" ? raw.assistantModel : defaultProfile.assistantModel,
    liteModel: typeof raw.liteModel === "string" ? raw.liteModel : defaultProfile.liteModel,
  };
}

function parseAgentProfiles(
  parsed: Record<string, unknown>,
  prompts: Record<string, unknown>,
): { agentProfiles: Array<AgentProfile>; activeAgentProfileId: string } {
  if (Array.isArray(parsed.agentProfiles) && parsed.agentProfiles.length > 0) {
    const profiles = (parsed.agentProfiles as Array<unknown>).filter(isRecord).map(parseAgentProfileEntry);
    const activeId = typeof parsed.activeAgentProfileId === "string" ? parsed.activeAgentProfileId : profiles[0]?.id ?? "default-profile";
    return {
      agentProfiles: profiles.length > 0 ? profiles : DEFAULT_AGENT_PROFILES,
      activeAgentProfileId: profiles.some((p) => p.id === activeId) ? activeId : profiles[0]?.id ?? "default-profile",
    };
  }

  // Migration from old format: prompts.customInstructions / prompts.responseStyle / prompts.responseLanguage / prompts.modelDefaults
  const oldModelDefaults = isRecord(prompts.modelDefaults) ? prompts.modelDefaults : {};
  const migratedProfile: AgentProfile = {
    id: "default-profile",
    name: "Default",
    customInstructions: typeof prompts.customInstructions === "string"
      ? prompts.customInstructions
      : typeof prompts.systemPrompt === "string"
        ? (prompts.systemPrompt as string)
        : DEFAULT_AGENT_PROFILES[0].customInstructions,
    responseStyle: isPromptResponseStyle(prompts.responseStyle) ? prompts.responseStyle : DEFAULT_AGENT_PROFILES[0].responseStyle,
    responseLanguage: typeof prompts.responseLanguage === "string" ? prompts.responseLanguage : DEFAULT_AGENT_PROFILES[0].responseLanguage,
    primaryModel: typeof oldModelDefaults.primaryModel === "string" ? oldModelDefaults.primaryModel : "",
    assistantModel: typeof oldModelDefaults.assistantModel === "string" ? oldModelDefaults.assistantModel : "",
    liteModel: typeof oldModelDefaults.liteModel === "string" ? oldModelDefaults.liteModel : "",
  };

  return {
    agentProfiles: [migratedProfile],
    activeAgentProfileId: "default-profile",
  };
}

function getStoredSettings(): WorkbenchSettingsState {
  if (typeof window === "undefined") {
    return DEFAULT_SETTINGS;
  }

  const rawValue = window.localStorage.getItem(STORAGE_KEY);

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
    // Read from both old "prompts" key and new "commands" key for migration
    const promptsRaw = isRecord(parsed.prompts) ? parsed.prompts : {};
    const commandsRaw = isRecord(parsed.commands) ? parsed.commands : {};
    const policyRaw = isRecord(parsed.policy) ? parsed.policy : isRecord(parsed.approvalPolicy) ? parsed.approvalPolicy : {};

    return {
      general: {
        launchAtLogin: typeof generalRaw.launchAtLogin === "boolean" ? generalRaw.launchAtLogin : DEFAULT_GENERAL_PREFERENCES.launchAtLogin,
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
              : "chat-completions" as const,
            enabled: typeof entry.enabled === "boolean" ? entry.enabled : false,
            isCustom: typeof entry.isCustom === "boolean" ? entry.isCustom : false,
            models: Array.isArray(entry.models)
              ? (entry.models as Array<unknown>).filter(isRecord).map((model) => ({
                  id: typeof model.id === "string" ? model.id : crypto.randomUUID(),
                  modelId: typeof model.modelId === "string" ? model.modelId : "",
                  displayName: typeof model.displayName === "string" ? model.displayName : "",
                  enabled: typeof model.enabled === "boolean" ? model.enabled : false,
                  contextWindow: typeof model.contextWindow === "string" ? model.contextWindow : undefined,
                  isManual: typeof model.isManual === "boolean" ? model.isManual : undefined,
                }))
              : [],
          }))
        : DEFAULT_PROVIDERS,
      commands: {
        commands: (() => {
          // Try new "commands.commands" first, then fall back to old "prompts.commands"
          const rawCommands = Array.isArray(commandsRaw.commands) ? commandsRaw.commands : Array.isArray(promptsRaw.commands) ? promptsRaw.commands : null;
          return rawCommands
            ? (rawCommands as Array<unknown>).filter(isRecord).map((cmd) => ({
                id: typeof cmd.id === "string" ? cmd.id : crypto.randomUUID(),
                name: typeof cmd.name === "string" ? cmd.name : "",
                path: typeof cmd.path === "string" ? cmd.path : "",
                argumentHint: typeof cmd.argumentHint === "string" ? cmd.argumentHint : "",
                description: typeof cmd.description === "string" ? cmd.description : "",
              }))
            : DEFAULT_COMMAND_SETTINGS.commands;
        })(),
      },
      ...parseAgentProfiles(parsed, promptsRaw),
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

export function useWorkbenchSettings() {
  const [settings, setSettings] = useState<WorkbenchSettingsState>(() => getStoredSettings());

  useEffect(() => {
    if (typeof window === "undefined") {
      return;
    }

    window.localStorage.setItem(STORAGE_KEY, JSON.stringify(settings));
  }, [settings]);

  const updateGeneralPreference = <Key extends keyof GeneralPreferences>(key: Key, value: GeneralPreferences[Key]) => {
    setSettings((current) => ({
      ...current,
      general: {
        ...current.general,
        [key]: value,
      },
    }));
  };

  const updateCommandSetting = <Key extends keyof CommandSettings>(key: Key, value: CommandSettings[Key]) => {
    setSettings((current) => ({
      ...current,
      commands: {
        ...current.commands,
        [key]: value,
      },
    }));
  };

  const addAgentProfile = (entry: Omit<AgentProfile, "id">) => {
    const id = crypto.randomUUID();
    setSettings((current) => ({
      ...current,
      agentProfiles: [...current.agentProfiles, { ...entry, id }],
      activeAgentProfileId: id,
    }));
  };

  const removeAgentProfile = (id: string) => {
    setSettings((current) => {
      const remaining = current.agentProfiles.filter((p) => p.id !== id);
      if (remaining.length === 0) return current;
      const activeId = current.activeAgentProfileId === id ? remaining[0].id : current.activeAgentProfileId;
      return { ...current, agentProfiles: remaining, activeAgentProfileId: activeId };
    });
  };

  const updateAgentProfile = (id: string, patch: Partial<Omit<AgentProfile, "id">>) => {
    setSettings((current) => ({
      ...current,
      agentProfiles: current.agentProfiles.map((p) =>
        p.id === id ? { ...p, ...patch } : p,
      ),
    }));
  };

  const setActiveAgentProfile = (id: string) => {
    setSettings((current) => ({ ...current, activeAgentProfileId: id }));
  };

  const duplicateAgentProfile = (id: string) => {
    setSettings((current) => {
      const source = current.agentProfiles.find((p) => p.id === id);
      if (!source) return current;
      const newId = crypto.randomUUID();
      const copy: AgentProfile = { ...source, id: newId, name: `${source.name} Copy` };
      return {
        ...current,
        agentProfiles: [...current.agentProfiles, copy],
        activeAgentProfileId: newId,
      };
    });
  };

  const updatePolicySetting = <Key extends keyof PolicySettings>(key: Key, value: PolicySettings[Key]) => {
    setSettings((current) => ({
      ...current,
      policy: {
        ...current.policy,
        [key]: value,
      },
    }));
  };

  const addAllowEntry = (entry: Omit<PatternEntry, "id">) => {
    setSettings((current) => ({
      ...current,
      policy: {
        ...current.policy,
        allowList: [...current.policy.allowList, { ...entry, id: crypto.randomUUID() }],
      },
    }));
  };

  const removeAllowEntry = (id: string) => {
    setSettings((current) => ({
      ...current,
      policy: {
        ...current.policy,
        allowList: current.policy.allowList.filter((entry) => entry.id !== id),
      },
    }));
  };

  const updateAllowEntry = (id: string, patch: Partial<Omit<PatternEntry, "id">>) => {
    setSettings((current) => ({
      ...current,
      policy: {
        ...current.policy,
        allowList: current.policy.allowList.map((entry) =>
          entry.id === id ? { ...entry, ...patch } : entry,
        ),
      },
    }));
  };

  const addDenyEntry = (entry: Omit<PatternEntry, "id">) => {
    setSettings((current) => ({
      ...current,
      policy: {
        ...current.policy,
        denyList: [...current.policy.denyList, { ...entry, id: crypto.randomUUID() }],
      },
    }));
  };

  const removeDenyEntry = (id: string) => {
    setSettings((current) => ({
      ...current,
      policy: {
        ...current.policy,
        denyList: current.policy.denyList.filter((entry) => entry.id !== id),
      },
    }));
  };

  const updateDenyEntry = (id: string, patch: Partial<Omit<PatternEntry, "id">>) => {
    setSettings((current) => ({
      ...current,
      policy: {
        ...current.policy,
        denyList: current.policy.denyList.map((entry) =>
          entry.id === id ? { ...entry, ...patch } : entry,
        ),
      },
    }));
  };

  const addWritableRoot = (entry: Omit<WritableRootEntry, "id">) => {
    setSettings((current) => ({
      ...current,
      policy: {
        ...current.policy,
        writableRoots: [...current.policy.writableRoots, { ...entry, id: crypto.randomUUID() }],
      },
    }));
  };

  const removeWritableRoot = (id: string) => {
    setSettings((current) => ({
      ...current,
      policy: {
        ...current.policy,
        writableRoots: current.policy.writableRoots.filter((entry) => entry.id !== id),
      },
    }));
  };

  const updateWritableRoot = (id: string, patch: Partial<Omit<WritableRootEntry, "id">>) => {
    setSettings((current) => ({
      ...current,
      policy: {
        ...current.policy,
        writableRoots: current.policy.writableRoots.map((entry) =>
          entry.id === id ? { ...entry, ...patch } : entry,
        ),
      },
    }));
  };

  const addWorkspace = (entry: Omit<WorkspaceEntry, "id">) => {
    setSettings((current) => ({
      ...current,
      workspaces: [
        ...current.workspaces,
        { ...entry, id: crypto.randomUUID() },
      ],
    }));
  };

  const removeWorkspace = (id: string) => {
    setSettings((current) => ({
      ...current,
      workspaces: current.workspaces.filter((workspace) => workspace.id !== id),
    }));
  };

  const updateWorkspace = (id: string, patch: Partial<Omit<WorkspaceEntry, "id">>) => {
    setSettings((current) => ({
      ...current,
      workspaces: current.workspaces.map((workspace) =>
        workspace.id === id ? { ...workspace, ...patch } : workspace,
      ),
    }));
  };

  const setDefaultWorkspace = (id: string) => {
    setSettings((current) => ({
      ...current,
      workspaces: current.workspaces.map((workspace) => ({
        ...workspace,
        isDefault: workspace.id === id,
      })),
    }));
  };

  const addProvider = (entry: Omit<ProviderEntry, "id">) => {
    setSettings((current) => ({
      ...current,
      providers: [...current.providers, { ...entry, id: crypto.randomUUID() }],
    }));
  };

  const removeProvider = (id: string) => {
    setSettings((current) => ({
      ...current,
      providers: current.providers.filter((provider) => provider.id !== id),
    }));
  };

  const updateProvider = (id: string, patch: Partial<Omit<ProviderEntry, "id">>) => {
    setSettings((current) => ({
      ...current,
      providers: current.providers.map((provider) =>
        provider.id === id ? { ...provider, ...patch } : provider,
      ),
    }));
  };

  const addCommand = (entry: Omit<CommandEntry, "id">) => {
    setSettings((current) => ({
      ...current,
      commands: {
        ...current.commands,
        commands: [...current.commands.commands, { ...entry, id: crypto.randomUUID() }],
      },
    }));
  };

  const removeCommand = (id: string) => {
    setSettings((current) => ({
      ...current,
      commands: {
        ...current.commands,
        commands: current.commands.commands.filter((cmd) => cmd.id !== id),
      },
    }));
  };

  const updateCommand = (id: string, patch: Partial<Omit<CommandEntry, "id">>) => {
    setSettings((current) => ({
      ...current,
      commands: {
        ...current.commands,
        commands: current.commands.commands.map((cmd) =>
          cmd.id === id ? { ...cmd, ...patch } : cmd,
        ),
      },
    }));
  };

  return {
    general: settings.general,
    workspaces: settings.workspaces,
    providers: settings.providers,
    commands: settings.commands,
    policy: settings.policy,
    updateGeneralPreference,
    addWorkspace,
    removeWorkspace,
    updateWorkspace,
    setDefaultWorkspace,
    addProvider,
    removeProvider,
    updateProvider,
    updateCommandSetting,
    agentProfiles: settings.agentProfiles,
    activeAgentProfileId: settings.activeAgentProfileId,
    addAgentProfile,
    removeAgentProfile,
    updateAgentProfile,
    setActiveAgentProfile,
    duplicateAgentProfile,
    updatePolicySetting,
    addAllowEntry,
    removeAllowEntry,
    updateAllowEntry,
    addDenyEntry,
    removeDenyEntry,
    updateDenyEntry,
    addWritableRoot,
    removeWritableRoot,
    updateWritableRoot,
    addCommand,
    removeCommand,
    updateCommand,
  };
}
