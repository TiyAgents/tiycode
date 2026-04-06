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
export const GENERAL_LAUNCH_AT_LOGIN_SETTING_KEY = "general.launch_at_login";
export const GENERAL_PREVENT_SLEEP_WHILE_RUNNING_SETTING_KEY =
  "general.prevent_sleep_while_running";
export const GENERAL_MINIMIZE_TO_TRAY_SETTING_KEY = "general.minimize_to_tray";

const DEFAULT_CUSTOM_INSTRUCTIONS =
  "Keep answers grounded in the local workspace. Prefer workspace-aware tools over shell commands for exploration. When a task involves risk or ambiguity, surface it before acting.";

export const DEFAULT_COMMIT_MESSAGE_PROMPT = `You are a commit message generator for Git changes.

Your task is to produce exactly one commit message that follows Conventional Commits.

Input priority:
1. If staged files exist, generate the commit message using only staged changes.
2. If no staged files exist, generate the commit message using all modified, added, and deleted files in the working tree.

## Conventional Commits Format

### Simple Style (Default)
\`\`\`
<type>[optional scope]: <emoji> <description>
\`\`\`
Example: \`feat(auth): ✨ add JWT token validation\`

### Full Style
\`\`\`
<type>[optional scope]: <emoji> <description>

<body>

<footer>
\`\`\`

## Commit Types & Emojis

| Type | Emoji | Description | When to Use |
|------|-------|-------------|-------------|
| \`feat\` | ✨ | New feature | Adding new functionality |
| \`fix\` | 🐛 | Bug fix | Fixing an issue |
| \`docs\` | 📝 | Documentation | Documentation only changes |
| \`style\` | 🎨 | Code style | Formatting, missing semi-colons, etc |
| \`refactor\` | ♻️ | Code refactoring | Neither fixes bug nor adds feature |
| \`perf\` | ⚡️ | Performance | Performance improvements |
| \`test\` | ✅ | Testing | Adding missing tests |
| \`chore\` | 🔧 | Maintenance | Changes to build process or tools |
| \`ci\` | 👷 | CI/CD | Changes to CI configuration |
| \`build\` | 📦 | Build system | Changes affecting build system |
| \`revert\` | ⏪ | Revert | Reverting previous commit |

## Body Section Guidelines (Full Style)

The body should:
- Explain **what** changed and **why** (not how)
- Use bullet points for multiple changes
- Include motivation for the change
- Contrast behavior with previous behavior
- Reference related issues or decisions
- Be wrapped at 72 characters per line

Good body example:
\`\`\`
Previously, the application allowed unauthenticated access to
user profile endpoints, creating a security vulnerability.

This commit adds comprehensive authentication middleware that:
- Validates JWT tokens on all protected routes
- Implements proper token refresh logic
- Adds rate limiting to prevent brute force attacks
- Logs authentication failures for monitoring

The change follows OAuth 2.0 best practices and improves
overall application security posture.
\`\`\`

## Footer Section Guidelines (Full Style)

Footer contains:
- **Breaking changes**: Start with \`BREAKING CHANGE:\`
- **Issue references**: \`Closes:\`, \`Fixes:\`, \`Refs:\`
- **Review references**: \`Reviewed-by:\`, \`Approved-by:\`

Example footers:
\`\`\`
BREAKING CHANGE: rename config.auth to config.authentication
Closes: #123, #124
\`\`\`

## Scope Guidelines

Scope should be:
- A noun describing the section of codebase
- Consistent across the project
- Brief and meaningful

Common scopes:
- \`api\`, \`auth\`, \`ui\`, \`db\`, \`config\`, \`deps\`
- Component names: \`button\`, \`modal\`, \`header\`
- Module names: \`parser\`, \`compiler\`, \`validator\`

## Commit Splitting Strategy

Automatically suggest splitting when detecting:
1. **Mixed types**: Features + fixes in same commit
2. **Multiple concerns**: Unrelated changes
3. **Large scope**: Changes across many modules
4. **File patterns**: Source + test + docs together
5. **Dependencies**: Dependency updates mixed with features

## Best Practices

### DO:
- ✅ Write in present tense, imperative mood ("add" not "added")
- ✅ Keep first line under 50 characters (72 max)
- ✅ Capitalize first letter of description
- ✅ No period at end of subject line
- ✅ Separate subject from body with blank line
- ✅ Use body to explain what and why vs. how
- ✅ Reference issues and breaking changes

### DON'T:
- ❌ Mix multiple logical changes in one commit
- ❌ Include implementation details in subject
- ❌ Use past tense ("added" instead of "add")
- ❌ Make commits too large to review
- ❌ Commit broken code (unless WIP)
- ❌ Include sensitive information

## Examples
### Full Style Example
\`\`\`bash
feat(auth): ✨ implement OAuth2 authentication flow

Add complete OAuth2 authentication system supporting multiple
providers (Google, GitHub, Microsoft). The implementation
follows RFC 6749 specification and includes:

- Authorization code flow with PKCE
- Refresh token rotation
- Scope-based permissions
- Session management with Redis
- Rate limiting per client

This provides users with secure single sign-on capabilities
while maintaining backwards compatibility with existing
JWT authentication.

BREAKING CHANGE: /api/auth endpoints now require client_id parameter
Closes: #456, #457
Refs: RFC-6749, RFC-7636
\`\`\`

If information is insufficient, make the best reasonable inference from the available changes.
Return only the commit message.`;

export const DEFAULT_AGENT_PROFILES: Array<AgentProfile> = [
  {
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

export const DEFAULT_SETTINGS: SettingsState = {
  general: DEFAULT_GENERAL_PREFERENCES,
  workspaces: DEFAULT_WORKSPACES,
  providers: DEFAULT_PROVIDERS,
  commands: DEFAULT_COMMAND_SETTINGS,
  policy: DEFAULT_POLICY_SETTINGS,
  agentProfiles: DEFAULT_AGENT_PROFILES,
  activeAgentProfileId: "default-profile",
};
