import { useEffect, useState } from "react";

export type SettingsCategory = "account" | "general" | "workspace" | "providers" | "prompts" | "policy";
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

export type PromptSettings = {
  customInstructions: string;
  responseStyle: PromptResponseStyle;
  responseLanguage: string;
  commands: Array<CommandEntry>;
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
  workspaces: Array<WorkspaceEntry>;
  providers: Array<ProviderEntry>;
  prompts: PromptSettings;
  policy: PolicySettings;
};

const STORAGE_KEY = "tiy-agent-workbench-settings";

const DEFAULT_PROMPT_SETTINGS: PromptSettings = {
  customInstructions:
    "You are Tiy Agent, a desktop coding partner. Keep answers crisp, grounded in the local workspace, and explicit about risks before taking action.",
  responseStyle: "balanced",
  responseLanguage: "English",
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

const DEFAULT_POLICY_SETTINGS: PolicySettings = {
  approvalPolicy: "on-request",
  allowList: [],
  denyList: [],
  sandboxPolicy: "workspace-write",
  networkAccess: "ask",
  writableRoots: [],
};

const DEFAULT_SETTINGS: WorkbenchSettingsState = {
  workspaces: DEFAULT_WORKSPACES,
  providers: DEFAULT_PROVIDERS,
  prompts: DEFAULT_PROMPT_SETTINGS,
  policy: DEFAULT_POLICY_SETTINGS,
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

    const workspaces = Array.isArray(parsed.workspaces) ? parsed.workspaces : null;
    const providers = Array.isArray(parsed.providers) ? parsed.providers : null;
    const prompts = isRecord(parsed.prompts) ? parsed.prompts : {};
    const policyRaw = isRecord(parsed.policy) ? parsed.policy : isRecord(parsed.approvalPolicy) ? parsed.approvalPolicy : {};

    return {
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
      prompts: {
        customInstructions:
          typeof prompts.customInstructions === "string"
            ? prompts.customInstructions
            : typeof prompts.systemPrompt === "string"
              ? (prompts.systemPrompt as string)
              : DEFAULT_PROMPT_SETTINGS.customInstructions,
        responseStyle: isPromptResponseStyle(prompts.responseStyle)
          ? prompts.responseStyle
          : DEFAULT_PROMPT_SETTINGS.responseStyle,
        responseLanguage:
          typeof prompts.responseLanguage === "string"
            ? prompts.responseLanguage
            : DEFAULT_PROMPT_SETTINGS.responseLanguage,
        commands: Array.isArray(prompts.commands)
          ? (prompts.commands as Array<unknown>).filter(isRecord).map((cmd) => ({
              id: typeof cmd.id === "string" ? cmd.id : crypto.randomUUID(),
              name: typeof cmd.name === "string" ? cmd.name : "",
              path: typeof cmd.path === "string" ? cmd.path : "",
              argumentHint: typeof cmd.argumentHint === "string" ? cmd.argumentHint : "",
              description: typeof cmd.description === "string" ? cmd.description : "",
            }))
          : DEFAULT_PROMPT_SETTINGS.commands,
      },
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

  const updatePromptSetting = <Key extends keyof PromptSettings>(key: Key, value: PromptSettings[Key]) => {
    setSettings((current) => ({
      ...current,
      prompts: {
        ...current.prompts,
        [key]: value,
      },
    }));
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
      prompts: {
        ...current.prompts,
        commands: [...current.prompts.commands, { ...entry, id: crypto.randomUUID() }],
      },
    }));
  };

  const removeCommand = (id: string) => {
    setSettings((current) => ({
      ...current,
      prompts: {
        ...current.prompts,
        commands: current.prompts.commands.filter((cmd) => cmd.id !== id),
      },
    }));
  };

  const updateCommand = (id: string, patch: Partial<Omit<CommandEntry, "id">>) => {
    setSettings((current) => ({
      ...current,
      prompts: {
        ...current.prompts,
        commands: current.prompts.commands.map((cmd) =>
          cmd.id === id ? { ...cmd, ...patch } : cmd,
        ),
      },
    }));
  };

  return {
    workspaces: settings.workspaces,
    providers: settings.providers,
    prompts: settings.prompts,
    policy: settings.policy,
    addWorkspace,
    removeWorkspace,
    updateWorkspace,
    setDefaultWorkspace,
    addProvider,
    removeProvider,
    updateProvider,
    updatePromptSetting,
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
