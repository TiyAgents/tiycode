import type {
  AgentProfile,
  CommandSettings,
  GeneralPreferences,
  PolicySettings,
  ProviderEntry,
  SettingsState,
  WorkspaceEntry,
} from "@/modules/settings-center/model/types";

export const SETTINGS_STORAGE_KEY = "tiy-agent-workbench-settings";

const DEFAULT_CUSTOM_INSTRUCTIONS =
  "You are Tiy Agent, a desktop coding partner. Keep answers crisp, grounded in the local workspace, and explicit about risks before taking action.";

export const DEFAULT_AGENT_PROFILES: Array<AgentProfile> = [{
  id: "default-profile",
  name: "Default",
  customInstructions: DEFAULT_CUSTOM_INSTRUCTIONS,
  responseStyle: "balanced",
  responseLanguage: "English",
  primaryModel: "",
  assistantModel: "",
  liteModel: "",
}];

export const DEFAULT_COMMAND_SETTINGS: CommandSettings = {
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

export const DEFAULT_WORKSPACES: Array<WorkspaceEntry> = [
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

export const DEFAULT_PROVIDERS: Array<ProviderEntry> = [
  {
    id: "zenmux",
    name: "ZenMux",
    baseUrl: "https://zenmux.ai/api/v1",
    apiKey: "sk-zenmux-xxxxxxxxxxxxxxxxxxxxxxxxxxxx",
    apiProtocol: "responses",
    customHeaders: {},
    enabled: true,
    isCustom: true,
    models: [
      {
        id: "m1",
        modelId: "openai/gpt-5.4",
        displayName: "GPT-5.4",
        enabled: true,
        capabilityOverrides: {},
        providerOptions: {},
        isManual: true,
      },
      {
        id: "m2",
        modelId: "stepfun/step-3.5-flash",
        displayName: "stepfun/step-3.5-flash",
        enabled: true,
        contextWindow: "256K",
        capabilityOverrides: {},
        providerOptions: {},
      },
      {
        id: "m3",
        modelId: "anthropic/claude-3.5-haiku",
        displayName: "anthropic/claude-3.5-haiku",
        enabled: false,
        contextWindow: "200K",
        capabilityOverrides: {},
        providerOptions: {},
      },
      {
        id: "m4",
        modelId: "anthropic/claude-3.7-sonnet",
        displayName: "anthropic/claude-3.7-sonnet",
        enabled: false,
        contextWindow: "200K",
        capabilityOverrides: {},
        providerOptions: {},
      },
    ],
  },
  {
    id: "openai",
    name: "OpenAI",
    baseUrl: "https://api.openai.com/v1",
    apiKey: "",
    apiProtocol: "chat-completions",
    customHeaders: {},
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
    customHeaders: {},
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
    customHeaders: {},
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
    customHeaders: {},
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
    customHeaders: {},
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
    customHeaders: {},
    enabled: false,
    isCustom: false,
    models: [],
  },
];

export const DEFAULT_GENERAL_PREFERENCES: GeneralPreferences = {
  launchAtLogin: false,
  minimizeToTray: false,
};

export const DEFAULT_POLICY_SETTINGS: PolicySettings = {
  approvalPolicy: "on-request",
  allowList: [],
  denyList: [],
  sandboxPolicy: "workspace-write",
  networkAccess: "ask",
  writableRoots: [],
};

export const DEFAULT_SETTINGS: SettingsState = {
  general: DEFAULT_GENERAL_PREFERENCES,
  workspaces: DEFAULT_WORKSPACES,
  providers: DEFAULT_PROVIDERS,
  commands: DEFAULT_COMMAND_SETTINGS,
  policy: DEFAULT_POLICY_SETTINGS,
  agentProfiles: DEFAULT_AGENT_PROFILES,
  activeAgentProfileId: "default-profile",
};
