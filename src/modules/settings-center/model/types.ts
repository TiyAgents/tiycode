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

export type ProviderModelCapabilities = {
  vision: boolean;
  imageOutput: boolean;
  toolCalling: boolean;
  reasoning: boolean;
  embedding: boolean;
};

export type ProviderModel = {
  id: string;
  modelId: string;
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
  name: string;
  baseUrl: string;
  apiKey: string;
  apiProtocol: ApiProtocol;
  customHeaders: Record<string, string>;
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
  preventSleepWhileRunning: boolean;
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

export type SettingsState = {
  general: GeneralPreferences;
  workspaces: Array<WorkspaceEntry>;
  providers: Array<ProviderEntry>;
  commands: CommandSettings;
  policy: PolicySettings;
  agentProfiles: Array<AgentProfile>;
  activeAgentProfileId: string;
};
