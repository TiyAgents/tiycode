import { useEffect, useState } from "react";

export type SettingsCategory = "account" | "general" | "workspace" | "providers" | "prompts" | "approval-policy";
export type PromptResponseStyle = "balanced" | "concise" | "guide";
export type CommandExecutionPolicy = "ask-every-time" | "auto-safe" | "full-auto";
export type AccessPolicy = "ask-first" | "block" | "allow";
export type RiskyCommandConfirmationPolicy = "always-confirm" | "block";

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

export type ApprovalPolicySettings = {
  commandExecution: CommandExecutionPolicy;
  fileWriteOutsideWorkspace: AccessPolicy;
  networkAccess: AccessPolicy;
  riskyCommandConfirmation: RiskyCommandConfirmationPolicy;
};

type WorkbenchSettingsState = {
  workspaces: Array<WorkspaceEntry>;
  providers: Array<ProviderEntry>;
  prompts: PromptSettings;
  approvalPolicy: ApprovalPolicySettings;
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

const DEFAULT_APPROVAL_POLICY_SETTINGS: ApprovalPolicySettings = {
  commandExecution: "auto-safe",
  fileWriteOutsideWorkspace: "ask-first",
  networkAccess: "ask-first",
  riskyCommandConfirmation: "always-confirm",
};

const DEFAULT_SETTINGS: WorkbenchSettingsState = {
  workspaces: DEFAULT_WORKSPACES,
  providers: DEFAULT_PROVIDERS,
  prompts: DEFAULT_PROMPT_SETTINGS,
  approvalPolicy: DEFAULT_APPROVAL_POLICY_SETTINGS,
};

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null;
}

function isPromptResponseStyle(value: unknown): value is PromptResponseStyle {
  return value === "balanced" || value === "concise" || value === "guide";
}

function isCommandExecutionPolicy(value: unknown): value is CommandExecutionPolicy {
  return value === "ask-every-time" || value === "auto-safe" || value === "full-auto";
}

function isAccessPolicy(value: unknown): value is AccessPolicy {
  return value === "ask-first" || value === "block" || value === "allow";
}

function isRiskyCommandConfirmationPolicy(value: unknown): value is RiskyCommandConfirmationPolicy {
  return value === "always-confirm" || value === "block";
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
    const approvalPolicy = isRecord(parsed.approvalPolicy) ? parsed.approvalPolicy : {};

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
      approvalPolicy: {
        commandExecution: isCommandExecutionPolicy(approvalPolicy.commandExecution)
          ? approvalPolicy.commandExecution
          : DEFAULT_APPROVAL_POLICY_SETTINGS.commandExecution,
        fileWriteOutsideWorkspace: isAccessPolicy(approvalPolicy.fileWriteOutsideWorkspace)
          ? approvalPolicy.fileWriteOutsideWorkspace
          : DEFAULT_APPROVAL_POLICY_SETTINGS.fileWriteOutsideWorkspace,
        networkAccess: isAccessPolicy(approvalPolicy.networkAccess)
          ? approvalPolicy.networkAccess
          : DEFAULT_APPROVAL_POLICY_SETTINGS.networkAccess,
        riskyCommandConfirmation: isRiskyCommandConfirmationPolicy(approvalPolicy.riskyCommandConfirmation)
          ? approvalPolicy.riskyCommandConfirmation
          : DEFAULT_APPROVAL_POLICY_SETTINGS.riskyCommandConfirmation,
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

  const updateApprovalPolicySetting = <Key extends keyof ApprovalPolicySettings>(
    key: Key,
    value: ApprovalPolicySettings[Key],
  ) => {
    setSettings((current) => ({
      ...current,
      approvalPolicy: {
        ...current.approvalPolicy,
        [key]: value,
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
    approvalPolicy: settings.approvalPolicy,
    addWorkspace,
    removeWorkspace,
    updateWorkspace,
    setDefaultWorkspace,
    addProvider,
    removeProvider,
    updateProvider,
    updatePromptSetting,
    updateApprovalPolicySetting,
    addCommand,
    removeCommand,
    updateCommand,
  };
}
