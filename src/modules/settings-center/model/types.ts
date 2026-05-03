export type SettingsCategory = "account" | "general" | "workspace" | "providers" | "commands" | "terminal" | "policy" | "about";
export type PromptResponseStyle = "balanced" | "concise" | "guide";
export type ThinkingLevel = "off" | "minimal" | "low" | "medium" | "high" | "xhigh";
export type ApprovalPolicy = "untrusted" | "on-request" | "never";
export type PatternEntry = { id: string; pattern: string };
export type WritableRootEntry = { id: string; path: string };

export type WorkspaceEntry = {
  id: string;
  name: string;
  path: string;
  isDefault: boolean;
  isGit: boolean;
  autoWorkTree: boolean;
  kind?: "standalone" | "repo" | "worktree";
  parentWorkspaceId?: string | null;
  worktreeHash?: string | null;
};

export type ProviderKind = "builtin" | "custom";
export type ProviderType =
  | "openai"
  | "openai-compatible"
  | "openai-responses"
  | "anthropic"
  | "google"
  | "ollama"
  | "xai"
  | "groq"
  | "openrouter"
  | "minimax"
  | "kimi-coding"
  | "zai"
  | "deepseek"
  | "zenmux"
  | "bai";

export type CustomProviderType =
  | "openai-compatible"
  | "openai-responses"
  | "anthropic"
  | "google"
  | "ollama";

export type ProviderCatalogEntry = {
  providerKey: ProviderType;
  providerType: ProviderType;
  displayName: string;
  builtin: boolean;
  supportsCustom: boolean;
  defaultBaseUrl: string;
};

export type ProviderModelCapabilities = {
  vision: boolean;
  imageOutput: boolean;
  toolCalling: boolean;
  reasoning: boolean;
  embedding: boolean;
  reasoningContentConstrained?: boolean;
};

export type ProviderModel = {
  id: string;
  modelId: string;
  sortIndex: number;
  displayName: string;
  enabled: boolean;
  contextWindow?: string;
  maxOutputTokens?: string;
  capabilityOverrides: Partial<ProviderModelCapabilities>;
  providerOptions: Record<string, unknown>;
  isManual?: boolean;
};

export type ProviderEntry = {
  id: string;
  kind: ProviderKind;
  providerKey: string;
  providerType: ProviderType;
  displayName: string;
  baseUrl: string;
  apiKey: string;
  hasApiKey: boolean;
  lockedMapping: boolean;
  customHeaders: Record<string, string>;
  enabled: boolean;
  models: Array<ProviderModel>;
};

export type CommandEntry = {
  id: string;
  name: string;
  path: string;
  argumentHint: string;
  description: string;
  prompt: string;
  source?: string;
  enabled?: boolean;
  version?: number;
  fileName?: string;
  /** Client-only flag: entry exists locally but hasn't been persisted to the backend yet. */
  pendingCreate?: boolean;
};

export type AgentProfile = {
  id: string;
  name: string;
  customInstructions: string;
  commitMessagePrompt: string;
  responseStyle: PromptResponseStyle;
  thinkingLevel: ThinkingLevel;
  responseLanguage: string;
  commitMessageLanguage: string;
  primaryProviderId: string;
  primaryModelId: string;
  assistantProviderId: string;
  assistantModelId: string;
  liteProviderId: string;
  liteModelId: string;
};

export type CommandSettings = {
  commands: Array<CommandEntry>;
};

export type GeneralPreferences = {
  launchAtLogin: boolean;
  preventSleepWhileRunning: boolean;
  minimizeToTray: boolean;
};

export type PolicySettings = {
  approvalPolicy: ApprovalPolicy;
  allowList: Array<PatternEntry>;
  denyList: Array<PatternEntry>;
  writableRoots: Array<WritableRootEntry>;
};

export type TerminalCursorStyle = "block" | "underline" | "bar";

export type TerminalSettings = {
  shellPath: string;
  shellArgs: string;
  fontFamily: string;
  fontSize: number;
  lineHeight: number;
  cursorStyle: TerminalCursorStyle;
  cursorBlink: boolean;
  scrollback: number;
  copyOnSelect: boolean;
  termEnv: string;
};

export type LocalUiSettingsState = {
  general: GeneralPreferences;
  terminal: TerminalSettings;
};

export type SettingsState = {
  general: GeneralPreferences;
  workspaces: Array<WorkspaceEntry>;
  providers: Array<ProviderEntry>;
  commands: CommandSettings;
  terminal: TerminalSettings;
  policy: PolicySettings;
  agentProfiles: Array<AgentProfile>;
  activeAgentProfileId: string;
};
