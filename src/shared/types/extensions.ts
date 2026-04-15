export type ExtensionKind = "plugin" | "mcp" | "skill";

export type ExtensionInstallState = "discovered" | "installed" | "enabled" | "disabled" | "error";
export type ExtensionHealth = "unknown" | "healthy" | "degraded" | "error";
export type ConfigDiagnosticSeverity = "warning" | "error";
export type ConfigDiagnosticKind = "invalid_json" | "invalid_structure" | "read_failed";

export type ConfigDiagnostic = {
  id: string;
  scope: string;
  area: string;
  filePath: string;
  severity: ConfigDiagnosticSeverity;
  kind: ConfigDiagnosticKind;
  summary: string;
  detail: string;
  suggestion: string;
};

export type ExtensionSource =
  | { type: "builtin" }
  | { type: "local-dir"; path: string }
  | { type: "marketplace"; listingId: string };

export type ExtensionSummary = {
  id: string;
  kind: ExtensionKind;
  name: string;
  version: string;
  description?: string | null;
  source: ExtensionSource;
  installState: ExtensionInstallState;
  health: ExtensionHealth;
  permissions: string[];
  tags: string[];
};

export type PluginTool = {
  name: string;
  description: string;
  command: string;
  args: string[];
  cwd?: string | null;
  timeoutMs?: number | null;
  requiredPermission: string;
};

export type PluginCommand = {
  name: string;
  description: string;
  promptTemplate?: string | null;
};

export type PluginHookGroup = {
  event: string;
  handlers: string[];
};

export type PluginDetail = {
  id: string;
  path: string;
  author?: string | null;
  homepage?: string | null;
  defaultEnabled: boolean;
  enabled: boolean;
  capabilities: string[];
  permissions: string[];
  hooks: PluginHookGroup[];
  tools: PluginTool[];
  commands: PluginCommand[];
  bundledSkills: string[];
  bundledMcpServers: string[];
  timeoutMs?: number | null;
  skillsDir?: string | null;
  configSchemaPath?: string | null;
  lastError?: string | null;
};

export type McpServerConfigInput = {
  id: string;
  label: string;
  transport: "stdio" | "streamable-http";
  enabled: boolean;
  autoStart: boolean;
  command?: string;
  args?: string[];
  env?: Record<string, string>;
  cwd?: string;
  url?: string;
  headers?: Record<string, string>;
  timeoutMs?: number;
};

export type McpServerConfig = {
  id: string;
  label: string;
  transport: string;
  enabled: boolean;
  autoStart: boolean;
  command?: string | null;
  args: string[];
  env: Record<string, string>;
  cwd?: string | null;
  url?: string | null;
  headers: Record<string, string>;
  timeoutMs?: number | null;
};

export type McpToolSummary = {
  name: string;
  qualifiedName: string;
  description?: string | null;
  inputSchema?: unknown;
};

export type McpResourceSummary = {
  uri: string;
  name: string;
  description?: string | null;
  mimeType?: string | null;
};

export type McpServerState = {
  id: string;
  label: string;
  scope: string;
  status: string;
  phase: string;
  tools: McpToolSummary[];
  resources: McpResourceSummary[];
  staleSnapshot: boolean;
  lastError?: string | null;
  updatedAt: string;
  config: McpServerConfig;
};

export type SkillRecord = {
  id: string;
  name: string;
  description?: string | null;
  tags: string[];
  triggers: string[];
  tools: string[];
  priority?: string | null;
  source: string;
  path: string;
  enabled: boolean;
  pinned: boolean;
  scope: string;
  contentPreview: string;
  promptBudgetChars: number;
};

export type SkillPreview = {
  record: SkillRecord;
  content: string;
};

export type ExtensionDetail = {
  summary: ExtensionSummary;
  plugin?: PluginDetail | null;
  mcp?: McpServerState | null;
  skill?: SkillRecord | null;
};

export type ExtensionCommand = {
  pluginId: string;
  name: string;
  description: string;
  promptTemplate: string;
};

export type ExtensionActivityEvent = {
  id: string;
  source: string;
  action: string;
  targetType?: string | null;
  targetId?: string | null;
  result?: unknown;
  createdAt: string;
};

export type MarketplaceSource = {
  id: string;
  name: string;
  url: string;
  builtin: boolean;
  kind: string;
  status: string;
  lastSyncedAt?: string | null;
  lastError?: string | null;
  pluginCount: number;
};

export type MarketplaceSourceInput = {
  name: string;
  url: string;
};

export type MarketplaceSourcePluginRef = {
  id: string;
  name: string;
  version: string;
  enabled: boolean;
  path: string;
};

export type MarketplaceRemoveSourcePlan = {
  source: MarketplaceSource;
  canRemove: boolean;
  blockingPlugins: MarketplaceSourcePluginRef[];
  removableInstalledPlugins: MarketplaceSourcePluginRef[];
  summary: string;
};

export type MarketplaceItem = {
  id: string;
  sourceId: string;
  sourceName: string;
  kind: string;
  name: string;
  version: string;
  summary: string;
  description: string;
  publisher: string;
  tags: string[];
  hooks: PluginHookGroup[];
  commandNames: string[];
  mcpServers: string[];
  skillNames: string[];
  path: string;
  installable: boolean;
  installed: boolean;
  enabled: boolean;
};
