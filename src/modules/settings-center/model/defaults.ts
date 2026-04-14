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
      prompt: `TiyCode Command: Commit

This command helps you create well-formatted commits following the Conventional Commits specification.

## Usage

Basic usage:
\`\`\`
/prompts:commit
\`\`\`

With options:
\`\`\`
/prompts:commit --verify=no
/prompts:commit --style=full
/prompts:commit --style=full --type=feat
\`\`\`

## Command Options

- \`--verify\`: Pre-commit checks (lint, build, generate:docs)
  - \`no\` (default): Skip pre-commit checks
  - \`yes\`: Perform pre-commit checks
- \`--style=simple|full\`:
  - \`simple\` (default): Creates concise single-line commit messages
  - \`full\`: Creates detailed commit messages with body and footer sections
- \`--type=<type>\`: Specify the commit type (overrides automatic detection)
- \`--language=english|chinese\`:
   - \`english\` (default): Generates commit messages in English
   - \`chinese\`: Generates commit messages in Simplified Chinese

## What This Command Does

1. **Merge latest remote main branch**:
   - Detect the default branch name (master or main) from remote
   - Fetch latest changes from remote: \`git fetch origin\`
   - Merge the latest remote main branch: \`git merge origin/<main|master>\`
   - Resolve any merge conflicts if necessary before proceeding

2. **Pre-commit checks** (when \`--verify=yes\`):
   - **Auto-detect project build tool first**: Check for package.json, pyproject.toml, Makefile, Cargo.toml, go.mod, etc.
   - **Determine the package manager**: For Node.js projects, check lock files (package-lock.json → npm, pnpm-lock.yaml → pnpm, yarn.lock → yarn)
   - **Read available scripts/targets**: Check package.json scripts, Makefile targets, or equivalent before executing
   - **Execute appropriate commands based on detected configuration**:
     - Lint: Run the project's lint command if available
     - Test: Run the project's test command if available
     - Build: Run the project's build command if available
   - **IMPORTANT**: Do NOT hardcode or assume commands like \`npm run test\`. Always verify the exact commands from the project configuration first.

3. **File staging**:
   - Check staged files with \`git status\`
   - If no files staged, **do NOT automatically add files**. Instead, remind the user that there are no staged changes and ask them to stage the desired files before committing.

4. **Change analysis**:
   - Run \`git diff\` to understand changes
   - Detect if multiple logical changes should be split
   - Suggest atomic commits when appropriate

5. **Commit message creation**:
   - **CRITICAL: Language Detection** - First, check if \`--language=chinese\` is present in the command arguments. If so, generate ALL commit messages in Simplified Chinese. If not specified or \`--language=english\`, generate in English.
   - Generate messages following Conventional Commits specification
   - Apply appropriate emoji prefixes
   - Add detailed body/footer in full style mode

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

Example:
\`\`\`
feat(auth): ✨ add JWT token validation

Implement JWT token validation middleware that:
- Validates token signature and expiration
- Extracts user claims from payload
- Adds user context to request object
- Handles refresh token rotation

This change improves security by ensuring all protected
routes validate authentication tokens properly.

BREAKING CHANGE: API now requires Bearer token for all authenticated endpoints
Closes: #123
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
- **Co-authors**: \`Co-authored-by: name <email>\`
- **Review references**: \`Reviewed-by:\`, \`Approved-by:\`

Example footers:
\`\`\`
BREAKING CHANGE: rename config.auth to config.authentication
Closes: #123, #124
Co-authored-by: Jane Doe <jane@example.com>
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

### Simple Style Examples
\`\`\`bash
feat: ✨ add user registration flow
fix: 🐛 resolve memory leak in event handler
docs: 📝 update API endpoints documentation
refactor: ♻️ simplify authentication logic
perf: ⚡️ optimize database query performance
chore: 🔧 update build dependencies
\`\`\`

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

## Workflow

1. **Check language parameter**: Determine the language for commit messages by checking command arguments. If \`--language=chinese\` is present, use Simplified Chinese. Otherwise, use English.
2. **Merge latest remote main branch**: Fetch and merge the latest changes from remote main/master branch to ensure the local branch is up-to-date.
3. **Run pre-commit checks** (if \`--verify=yes\`): Auto-detect project build tools and execute appropriate lint/test/build commands.
4. Analyze changes to determine commit type and scope
5. Check if changes should be split into multiple commits
6. For each commit:
   - Verify files are staged (do not auto-stage; ask the user if nothing is staged)
   - Generate commit message based on style setting
   - If full style, create detailed body and footer
   - Execute git commit with generated message
7. Provide summary of committed changes

## Important Notes

- Default style is \`simple\` for quick, everyday commits
- Default language is \`english\` for commit messages
- Use \`--language=chinese\` to generate Simplified Chinese commit messages
- Use \`full\` style for:
  - Breaking changes
  - Complex features
  - Bug fixes requiring explanation
  - Changes affecting multiple systems
- The tool will intelligently detect when full style might be beneficial and suggest it
- Always review the generated message before confirming
- Pre-commit checks help maintain code quality`,
    },
    {
      id: "cmd-create-pr",
      name: "create-pr",
      path: "/prompts:create-pr",
      argumentHint:
        "[--draft] [--base=main|master] [--style=simple|full] [--language=english|chinese]",
      description:
        "Create pull requests with well-formatted PR title and description",
      prompt: `# TiyCode Command: Create PR

This command helps you create well-formatted pull requests using GitHub CLI, with automatic fallback to GitHub MCP tools.

## Usage

Basic usage:
\`\`\`
/prompts:create-pr
\`\`\`

With options:
\`\`\`
/prompts:create-pr --draft
/prompts:create-pr --base=main
/prompts:create-pr --style=full
/prompts:create-pr --language=chinese
\`\`\`

## Command Options

- \`--draft\`: Create the pull request as a draft
- \`--base=<branch>\`: Specify the base branch (default: auto-detect main/master from remote)
- \`--style=simple|full\`:
  - \`simple\` (default): Creates concise PR title and brief description
  - \`full\`: Creates detailed PR description with comprehensive sections
- \`--language=english|chinese\`:
  - \`english\` (default): Generates PR content in English
  - \`chinese\`: Generates PR content in Simplified Chinese

## What This Command Does

1. **Pre-flight checks**:
   - Verify current directory is a git repository
   - Check for uncommitted changes and warn user
   - Ensure current branch is not the main/master branch
   - Verify branch has commits ahead of base branch

2. **Detect base branch**:
   - If \`--base\` specified, use that branch
   - Otherwise, auto-detect default branch from remote (main or master)
   - Fetch latest changes: \`git fetch origin\`

3. **Analyze changes**:
   - Get all commits between base branch and current branch: \`git log <base>..HEAD\`
   - Get full diff against base branch: \`git diff <base>...HEAD\`
   - Identify changed files and their categories
   - Determine the overall nature of changes (feature, fix, refactor, etc.)

4. **Generate PR content**:
   - Create PR title following Conventional Commits style
   - Generate PR description based on style setting
   - Include summary of changes, test plan, and relevant metadata

5. **Push branch to remote**:
   - Check if branch exists on remote
   - Push with upstream tracking: \`git push -u origin <branch>\`

6. **Create pull request**:
   - **Primary**: use GitHub CLI \`gh pr create\`
   - **Fallback**: If CLI unavailable, Use GitHub MCP tool \`mcp__github__create_pull_request\`
   - Return the PR URL to user

## Tool Selection Strategy

### Primary: GitHub CLI

Use \`gh pr create\` when available:
\`\`\`bash
gh pr create --title "<pr-title>" --body "<pr-description>" --base "<base-branch>" [--draft]
\`\`\`

### Fallback: GitHub MCP Tools

If GitHub CLI tools are unavailable, use \`mcp__github__create_pull_request\`:
\`\`\`
mcp__github__create_pull_request({
  owner: "<repo-owner>",
  repo: "<repo-name>",
  title: "<pr-title>",
  body: "<pr-description>",
  head: "<current-branch>",
  base: "<base-branch>",
  draft: <true|false>
})
\`\`\`

## PR Title Format

### Simple Style (Default)
\`\`\`
<type>[optional scope]: <emoji> <description>
\`\`\`
Example: \`feat(auth): ✨ Add OAuth2 authentication flow\`

### Characteristics
- Use present tense, imperative mood ("Add" not "Added")
- Keep under 72 characters
- Capitalize first letter of description
- No period at end

## PR Description Format

### Simple Style
\`\`\`markdown
## Summary
<1-3 bullet points describing the changes>

## Test Plan
<Brief testing checklist>

🤖 Generated with [TiyCode](https://github.com/TiyAgents/tiycode)
\`\`\`

### Full Style
\`\`\`markdown
## Summary
<Comprehensive description of what this PR does>

## Changes
<Detailed list of changes organized by category>

## Motivation
<Why these changes are needed>

## Testing
<Detailed test plan with checkboxes>

## Screenshots (if applicable)
<Add screenshots for UI changes>

## Breaking Changes
<List any breaking changes, or "None">

## Related Issues
<Reference related issues: Closes #123, Refs #456>

## Checklist
- [ ] Code follows project style guidelines
- [ ] Tests have been added/updated
- [ ] Documentation has been updated
- [ ] No new warnings or errors introduced

🤖 Generated with [TiyCode](https://github.com/TiyAgents/tiycode)
\`\`\`

## PR Types & Emojis

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

## Workflow

1. **Check language parameter**: Determine the language for PR content by checking command arguments.

2. **Validate environment**:
   \`\`\`bash
   # Check if in git repo
   git rev-parse --is-inside-work-tree

   # Get current branch
   git branch --show-current

   # Check for uncommitted changes
   git status --porcelain
   \`\`\`

3. **Detect repository info**:
   \`\`\`bash
   # Get remote URL and parse owner/repo
   git remote get-url origin

   # Detect default branch
   git remote show origin | grep 'HEAD branch'
   \`\`\`

4. **Analyze changes**:
   \`\`\`bash
   # Fetch latest
   git fetch origin

   # Get commit log
   git log origin/<base>..HEAD --oneline

   # Get diff summary
   git diff origin/<base>...HEAD --stat

   # Get full diff for analysis
   git diff origin/<base>...HEAD
   \`\`\`

5. **Generate PR content**:
   - Analyze commits to determine PR type
   - Summarize changes across all commits
   - Create title following conventional format
   - Generate description based on style setting

6. **Push branch**:
   \`\`\`bash
   git push -u origin <current-branch>
   \`\`\`

7. **Create PR**:
   - Primary: use GitHub CLI \`gh pr create\`
   - If CLI unavailable, fallback to MCP tool: \`mcp__github__create_pull_request\`

8. **Output result**:
   - Display PR URL to user
   - Show PR number and title

## Best Practices

### DO:
- ✅ Ensure all commits are pushed before creating PR
- ✅ Write clear, descriptive PR titles
- ✅ Include test plan in description
- ✅ Reference related issues
- ✅ Keep PRs focused on single concern
- ✅ Add screenshots for UI changes
- ✅ Document breaking changes clearly

### DON'T:
- ❌ Create PR from main/master branch
- ❌ Create PR with uncommitted changes
- ❌ Use vague titles like "Fix bug" or "Update code"
- ❌ Mix unrelated changes in single PR
- ❌ Forget to push branch before creating PR
- ❌ Include sensitive information in PR description

## Error Handling

### Common Issues and Solutions

1. **Not a git repository**:
   - Error: "fatal: not a git repository"
   - Solution: Navigate to a git repository first

2. **No remote configured**:
   - Error: "No remote 'origin' found"
   - Solution: Add remote with \`git remote add origin <url>\`

3. **Branch already has PR**:
   - Error: "A pull request already exists"
   - Solution: Update existing PR or create new branch

4. **No commits to create PR**:
   - Error: "No commits between base and head"
   - Solution: Make commits before creating PR

5. **GitHub CLI unavailable**:
   - Automatically fallback to MCP tool \`mcp__github__create_pull_request\`
   - Ensure MCP GitHub tools are configured

6. **GitHub CLI not authenticated**:
   - Error: "gh: Not logged in"
   - Solution: Run \`gh auth login\` first

## Examples

### Simple Feature PR
\`\`\`
Title: feat(api): ✨ Add user profile endpoint

## Summary
- Add GET /api/users/:id/profile endpoint
- Include user preferences and settings in response
- Add caching for improved performance

## Test Plan
- [ ] Verify endpoint returns correct user data
- [ ] Test with invalid user ID
- [ ] Verify cache behavior

🤖 Generated with [TiyCode](https://github.com/TiyAgents/tiycode)
\`\`\`

### Full Style Bug Fix PR
\`\`\`
Title: fix(auth): 🐛 Resolve session timeout issue

## Summary
Fix an issue where user sessions were expiring prematurely due to incorrect
timestamp comparison in the session validation middleware.

## Changes
### Backend
- Fix timestamp comparison in \`src/middleware/auth.js\`
- Update session refresh logic in \`src/services/session.js\`

### Tests
- Add unit tests for session timeout scenarios
- Add integration tests for session refresh flow

## Motivation
Users reported being logged out unexpectedly after short periods of
inactivity. Investigation revealed the session timeout calculation was
using local time instead of UTC, causing premature expiration.

## Testing
- [ ] Verify session persists for expected duration
- [ ] Test across different timezones
- [ ] Verify refresh token rotation works correctly
- [ ] Run full auth test suite

## Breaking Changes
None

## Related Issues
Closes #234
Refs #198

## Checklist
- [x] Code follows project style guidelines
- [x] Tests have been added/updated
- [x] Documentation has been updated
- [x] No new warnings or errors introduced

🤖 Generated with [TiyCode](https://github.com/TiyAgents/tiycode)
\`\`\`

## Important Notes

- Default style is \`simple\` for quick PR creation
- Default language is \`english\` for PR content
- Use \`--language=chinese\` to generate Simplified Chinese PR content
- Use \`full\` style for:
  - Complex features
  - Bug fixes requiring explanation
  - Breaking changes
  - Changes affecting multiple systems
- Always review the generated PR content before confirming
- The tool will intelligently suggest \`full\` style when appropriate
- PR URL will be displayed upon successful creation`,
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
