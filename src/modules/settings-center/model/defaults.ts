import type {
  AgentProfile,
  CommandSettings,
  GeneralPreferences,
  PolicySettings,
  ProviderEntry,
  SettingsState,
  TerminalSettings,
  WorkspaceEntry,
} from "@/modules/settings-center/model/types";

export const SETTINGS_STORAGE_KEY = "tiy-agent-workbench-settings";
export const SETTINGS_STORAGE_SCHEMA_VERSION = 2;
export const GENERAL_LAUNCH_AT_LOGIN_SETTING_KEY = "general.launch_at_login";
export const GENERAL_PREVENT_SLEEP_WHILE_RUNNING_SETTING_KEY =
  "general.prevent_sleep_while_running";
export const GENERAL_MINIMIZE_TO_TRAY_SETTING_KEY = "general.minimize_to_tray";

export const TERMINAL_SHELL_PATH_SETTING_KEY = "terminal.shell_path";
export const TERMINAL_SHELL_ARGS_SETTING_KEY = "terminal.shell_args";
export const TERMINAL_FONT_FAMILY_SETTING_KEY = "terminal.font_family";
export const TERMINAL_FONT_SIZE_SETTING_KEY = "terminal.font_size";
export const TERMINAL_LINE_HEIGHT_SETTING_KEY = "terminal.line_height";
export const TERMINAL_CURSOR_STYLE_SETTING_KEY = "terminal.cursor_style";
export const TERMINAL_CURSOR_BLINK_SETTING_KEY = "terminal.cursor_blink";
export const TERMINAL_SCROLLBACK_SETTING_KEY = "terminal.scrollback";
export const TERMINAL_COPY_ON_SELECT_SETTING_KEY = "terminal.copy_on_select";
export const TERMINAL_TERM_ENV_SETTING_KEY = "terminal.term_env";

export const DEFAULT_AGENT_PROFILES: Array<AgentProfile> = [
  {
    id: "default-profile",
    name: "Default",
    customInstructions: "",
    commitMessagePrompt: "",
    responseStyle: "balanced",
    thinkingLevel: "off",
    responseLanguage: "English",
    commitMessageLanguage: "English",
    primaryProviderId: "",
    primaryModelId: "",
    assistantProviderId: "",
    assistantModelId: "",
    liteProviderId: "",
    liteModelId: "",
  },
];

export const DEFAULT_COMMAND_SETTINGS: CommandSettings = {
  commands: [
    {
      id: "cmd-commit",
      name: "commit",
      path: "/prompts:commit",
      argumentHint:
        "[--verify=yes|no] [--style=simple|full] [--type=feat|fix|docs|style|refactor|perf|test|chore|ci|build|revert] [--language=english|chinese]",
      description:
        "Create well-formatted commits with conventional commit messages",
      prompt: "Generate a conventional commit message using the current Git changes and the configured commit message guidance. Use any provided command arguments to refine style, language, verification behavior, or commit type.",
    },
    {
      id: "cmd-create-pr",
      name: "create-pr",
      path: "/prompts:create-pr",
      argumentHint:
        "[--draft] [--base=main|master] [--style=simple|full] [--language=english|chinese]",
      description:
        "Create pull requests via GitHub MCP tools with well-formatted PR title and description",
      prompt: "Create a pull request using the current branch changes and any supplied command arguments. Produce a clear PR title and description, and use available GitHub tooling when appropriate.",
    },
  ],
};

export const DEFAULT_WORKSPACES: Array<WorkspaceEntry> = [];

export const DEFAULT_PROVIDERS: Array<ProviderEntry> = [];

export const DEFAULT_GENERAL_PREFERENCES: GeneralPreferences = {
  launchAtLogin: false,
  preventSleepWhileRunning: false,
  minimizeToTray: true,
};

export const DEFAULT_POLICY_SETTINGS: PolicySettings = {
  approvalPolicy: "on-request",
  allowList: [],
  denyList: [
    { id: "default-deny-rm-root", pattern: "shell:rm -rf /" },
    { id: "default-deny-rm-literal-star", pattern: "shell:rm -rf \\*" },
  ],
  writableRoots: [],
};

export const DEFAULT_TERMINAL_SETTINGS: TerminalSettings = {
  shellPath: "",
  shellArgs: "",
  fontFamily: '"SFMono-Regular", "JetBrains Mono", "Menlo", monospace',
  fontSize: 12,
  lineHeight: 1.35,
  cursorStyle: "block",
  cursorBlink: true,
  scrollback: 5000,
  copyOnSelect: false,
  termEnv: "xterm-256color",
};

export const DEFAULT_SETTINGS: SettingsState = {
  general: DEFAULT_GENERAL_PREFERENCES,
  workspaces: DEFAULT_WORKSPACES,
  providers: DEFAULT_PROVIDERS,
  commands: DEFAULT_COMMAND_SETTINGS,
  terminal: DEFAULT_TERMINAL_SETTINGS,
  policy: DEFAULT_POLICY_SETTINGS,
  agentProfiles: DEFAULT_AGENT_PROFILES,
  activeAgentProfileId: "default-profile",
};
