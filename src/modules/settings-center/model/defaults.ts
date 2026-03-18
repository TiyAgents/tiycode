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
export const SETTINGS_STORAGE_SCHEMA_VERSION = 2;
export const GENERAL_PREVENT_SLEEP_WHILE_RUNNING_SETTING_KEY = "general.prevent_sleep_while_running";

const DEFAULT_CUSTOM_INSTRUCTIONS =
  "You are Tiy Agent, a desktop coding partner. Keep answers crisp, grounded in the local workspace, and explicit about risks before taking action.";

export const DEFAULT_AGENT_PROFILES: Array<AgentProfile> = [{
  id: "default-profile",
  name: "Default",
  customInstructions: DEFAULT_CUSTOM_INSTRUCTIONS,
  responseStyle: "balanced",
  responseLanguage: "English",
  primaryProviderId: "",
  primaryModelId: "",
  assistantProviderId: "",
  assistantModelId: "",
  liteProviderId: "",
  liteModelId: "",
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

export const DEFAULT_WORKSPACES: Array<WorkspaceEntry> = [];

export const DEFAULT_PROVIDERS: Array<ProviderEntry> = [];

export const DEFAULT_GENERAL_PREFERENCES: GeneralPreferences = {
  launchAtLogin: false,
  preventSleepWhileRunning: false,
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
