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
  "Keep answers grounded in the local workspace. Prefer workspace-aware tools over shell commands for exploration. When a task involves risk or ambiguity, surface it before acting.";

export const DEFAULT_COMMIT_MESSAGE_PROMPT = `You are a commit message generator for Git changes.

Your task is to produce exactly one commit message that follows Conventional Commits.

Input priority:
1. If staged files exist, generate the commit message using only staged changes.
2. If no staged files exist, generate the commit message using all modified, added, and deleted files in the working tree.

Language rule:
- If the command arguments contain --language=chinese, output the entire commit message in Simplified Chinese.
- Otherwise, output the entire commit message in English.
- Do not mix languages within the same message.

Output rule:
- Output only the final commit message.
- Do not include explanations, analysis, labels, code fences, or extra text.

Format rules:
- Default to simple style:
  <type>[optional scope]: <emoji> <description>
- Use full style only when the change is complex and needs additional context:
  <type>[optional scope]: <emoji> <description>

  <body>

  <footer>

Commit type selection:
- feat: new feature
- fix: bug fix
- docs: documentation only
- style: formatting or style changes only
- refactor: code restructuring without behavior change
- perf: performance improvement
- test: test-related changes
- chore: maintenance, tooling, dependency updates
- ci: CI/CD changes
- build: build system changes
- revert: revert a previous commit

Writing rules:
- Use imperative mood and present tense.
- Keep the subject line concise.
- Do not end the subject line with a period.
- Use a meaningful and brief scope when appropriate.
- Prefer the main change type if multiple unrelated changes exist.
- If the change is clearly large or needs explanation, include a short body and optional footer.

Conventional Commits Format

Simple Style (Default)
<type>[optional scope]: <emoji> <description>

Full Style
<type>[optional scope]: <emoji> <description>

<body>

<footer>

Commit Types & Emojis
- feat => ✨
- fix => 🐛
- docs => 📝
- style => 🎨
- refactor => ♻️
- perf => ⚡️
- test => ✅
- chore => 🔧
- ci => 👷
- build => 📦
- revert => ⏪

If information is insufficient, make the best reasonable inference from the available changes.
Return only the commit message.`;

export const DEFAULT_AGENT_PROFILES: Array<AgentProfile> = [{
  id: "default-profile",
  name: "Default",
  customInstructions: DEFAULT_CUSTOM_INSTRUCTIONS,
  commitMessagePrompt: DEFAULT_COMMIT_MESSAGE_PROMPT,
  responseStyle: "balanced",
  responseLanguage: "English",
  commitMessageLanguage: "English",
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
