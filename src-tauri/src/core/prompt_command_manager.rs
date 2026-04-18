use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::model::errors::{AppError, ErrorSource};
use crate::model::settings::{PromptCommandDto, PromptCommandInput};

const PROMPTS_DIR_NAME: &str = "prompts";
const BUILTIN_PROMPTS_DIR_NAME: &str = "builtin";
const USER_PROMPTS_DIR_NAME: &str = "user";
const DEFAULT_PROMPT_VERSION: u32 = 1;
const FRONTMATTER_DELIMITER: &str = "---";

const BUILTIN_PROMPT_COMMANDS: &[BuiltinPromptSeed<'_>] = &[
    BuiltinPromptSeed {
        id: "cmd-commit",
        name: "commit",
        path: "/prompts:commit",
        argument_hint:
            "[--verify=yes|no] [--style=simple|full] [--type=feat|fix|docs|style|refactor|perf|test|chore|ci|build|revert] [--language=english|chinese]",
        description: "Create well-formatted commits with conventional commit messages",
        prompt: r#"TiyCode Command: Commit

This command helps you create well-formatted commits following the Conventional Commits specification.

## Usage

Basic usage:
```
/prompts:commit
```

With options:
```
/prompts:commit --verify=no
/prompts:commit --style=full
/prompts:commit --style=full --type=feat
```

## Command Options

- `--verify`: Pre-commit checks (lint, build, generate:docs)
  - `no` (default): Skip pre-commit checks
  - `yes`: Perform pre-commit checks
- `--style=simple|full`:
  - `simple` (default): Creates concise single-line commit messages
  - `full`: Creates detailed commit messages with body and footer sections
- `--type=<type>`: Specify the commit type (overrides automatic detection)
- `--language=english|chinese`:
   - `english` (default): Generates commit messages in English
   - `chinese`: Generates commit messages in Simplified Chinese

## What This Command Does

1. **Pre-commit checks** (when `--verify=yes`):
   - **Auto-detect project build tool first**: Check for package.json, pyproject.toml, Makefile, Cargo.toml, go.mod, etc.
   - **Determine the package manager**: For Node.js projects, check lock files (package-lock.json → npm, pnpm-lock.yaml → pnpm, yarn.lock → yarn)
   - **Read available scripts/targets**: Check package.json scripts, Makefile targets, or equivalent before executing
   - **Execute appropriate commands based on detected configuration**:
     - Lint: Run the project's lint command if available
     - Test: Run the project's test command if available
     - Build: Run the project's build command if available
   - **IMPORTANT**: Do NOT hardcode or assume commands like `npm run test`. Always verify the exact commands from the project configuration first.

2. **File staging**:
   - Check staged files with `git status`
   - If no files staged, **do NOT automatically add files**. Instead, remind the user that there are no staged changes and ask them to stage the desired files before committing.

3. **Change analysis**:
   - Run `git diff` to understand changes
   - Detect if multiple logical changes should be split
   - Suggest atomic commits when appropriate

4. **Commit message creation**:
   - **CRITICAL: Language Detection** - First, check if `--language=chinese` is present in the command arguments. If so, generate ALL commit messages in Simplified Chinese. If not specified or `--language=english`, generate in English.
   - Generate messages following Conventional Commits specification
   - Apply appropriate emoji prefixes
   - Add detailed body/footer in full style mode

## Conventional Commits Format

### Simple Style (Default)
```
<type>[optional scope]: <emoji> <description>
```
Example: `feat(auth): ✨ add JWT token validation`

### Full Style
```
<type>[optional scope]: <emoji> <description>

<body>

<footer>
```

Example:
```
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
```

## Commit Types & Emojis

| Type | Emoji | Description | When to Use |
|------|-------|-------------|-------------|
| `feat` | ✨ | New feature | Adding new functionality |
| `fix` | 🐛 | Bug fix | Fixing an issue |
| `docs` | 📝 | Documentation | Documentation only changes |
| `style` | 🎨 | Code style | Formatting, missing semi-colons, etc |
| `refactor` | ♻️ | Code refactoring | Neither fixes bug nor adds feature |
| `perf` | ⚡️ | Performance | Performance improvements |
| `test` | ✅ | Testing | Adding missing tests |
| `chore` | 🔧 | Maintenance | Changes to build process or tools |
| `ci` | 👷 | CI/CD | Changes to CI configuration |
| `build` | 📦 | Build system | Changes affecting build system |
| `revert` | ⏪ | Revert | Reverting previous commit |

## Body Section Guidelines (Full Style)

The body should:
- Explain **what** changed and **why** (not how)
- Use bullet points for multiple changes
- Include motivation for the change
- Contrast behavior with previous behavior
- Reference related issues or decisions
- Be wrapped at 72 characters per line

Good body example:
```
Previously, the application allowed unauthenticated access to
user profile endpoints, creating a security vulnerability.

This commit adds comprehensive authentication middleware that:
- Validates JWT tokens on all protected routes
- Implements proper token refresh logic
- Adds rate limiting to prevent brute force attacks
- Logs authentication failures for monitoring

The change follows OAuth 2.0 best practices and improves
overall application security posture.
```

## Footer Section Guidelines (Full Style)

Footer contains:
- **Breaking changes**: Start with `BREAKING CHANGE:`
- **Issue references**: `Closes:`, `Fixes:`, `Refs:`
- **Co-authors**: `Co-authored-by: name <email>`
- **Review references**: `Reviewed-by:`, `Approved-by:`

Example footers:
```
BREAKING CHANGE: rename config.auth to config.authentication
Closes: #123, #124
Co-authored-by: Jane Doe <jane@example.com>
```

## Scope Guidelines

Scope should be:
- A noun describing the section of codebase
- Consistent across the project
- Brief and meaningful

Common scopes:
- `api`, `auth`, `ui`, `db`, `config`, `deps`
- Component names: `button`, `modal`, `header`
- Module names: `parser`, `compiler`, `validator`

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
```bash
feat: ✨ add user registration flow
fix: 🐛 resolve memory leak in event handler
docs: 📝 update API endpoints documentation
refactor: ♻️ simplify authentication logic
perf: ⚡️ optimize database query performance
chore: 🔧 update build dependencies
```

### Full Style Example
```bash
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
```

## Workflow

1. **Check language parameter**: Determine the language for commit messages by checking command arguments. If `--language=chinese` is present, use Simplified Chinese. Otherwise, use English.
2. **Run pre-commit checks** (if `--verify=yes`): Auto-detect project build tools and execute appropriate lint/test/build commands.
3. Analyze changes to determine commit type and scope
4. Check if changes should be split into multiple commits
5. For each commit:
   - Verify files are staged (do not auto-stage; ask the user if nothing is staged)
   - Generate commit message based on style setting
   - If full style, create detailed body and footer
   - Execute git commit with generated message
6. Provide summary of committed changes

## Important Notes

- Default style is `simple` for quick, everyday commits
- Default language is `english` for commit messages
- Use `--language=chinese` to generate Simplified Chinese commit messages
- Use `full` style for:
  - Breaking changes
  - Complex features
  - Bug fixes requiring explanation
  - Changes affecting multiple systems
- The tool will intelligently detect when full style might be beneficial and suggest it
- Always review the generated message before confirming
- Pre-commit checks help maintain code quality"#,
    },
    BuiltinPromptSeed {
        id: "cmd-create-pr",
        name: "create-pr",
        path: "/prompts:create-pr",
        argument_hint:
            "[--draft] [--base=main|master] [--style=simple|full] [--language=english|chinese]",
        description: "Create pull requests with well-formatted PR title and description",
        prompt: r#"# TiyCode Command: Create PR

This command helps you create well-formatted pull requests using GitHub CLI, with automatic fallback to GitHub MCP tools.

## Usage

Basic usage:
```
/prompts:create-pr
```

With options:
```
/prompts:create-pr --draft
/prompts:create-pr --base=main
/prompts:create-pr --style=full
/prompts:create-pr --language=chinese
```

## Command Options

- `--draft`: Create the pull request as a draft
- `--base=<branch>`: Specify the base branch (default: auto-detect main/master from remote)
- `--style=simple|full`:
  - `simple` (default): Creates concise PR title and brief description
  - `full`: Creates detailed PR description with comprehensive sections
- `--language=english|chinese`:
  - `english` (default): Generates PR content in English
  - `chinese`: Generates PR content in Simplified Chinese

## What This Command Does

1. **Pre-flight checks**:
   - Verify current directory is a git repository
   - Check for uncommitted changes and warn user
   - Ensure current branch is not the main/master branch
   - Verify branch has commits ahead of base branch

2. **Detect base branch**:
   - If `--base` specified, use that branch
   - Otherwise, auto-detect default branch from remote (main or master)
   - Fetch latest changes: `git fetch origin`

3. **Analyze changes**:
   - Get all commits between base branch and current branch: `git log <base>..HEAD`
   - Get full diff against base branch: `git diff <base>...HEAD`
   - Identify changed files and their categories
   - Determine the overall nature of changes (feature, fix, refactor, etc.)

4. **Generate PR content**:
   - Create PR title following Conventional Commits style
   - Generate PR description based on style setting
   - Include summary of changes, test plan, and relevant metadata

5. **Push branch to remote**:
   - Check if branch exists on remote
   - Push with upstream tracking: `git push -u origin <branch>`

6. **Create pull request**:
   - **Primary**: use GitHub CLI `gh pr create`
   - **Fallback**: If CLI unavailable, Use GitHub MCP tool `mcp__github__create_pull_request`
   - Return the PR URL to user

## Tool Selection Strategy

### Primary: GitHub CLI

Use `gh pr create` when available:
```bash
gh pr create --title "<pr-title>" --body "<pr-description>" --base "<base-branch>" [--draft]
```

### Fallback: GitHub MCP Tools

If GitHub CLI tools are unavailable, use `mcp__github__create_pull_request`:
```
mcp__github__create_pull_request({
  owner: "<repo-owner>",
  repo: "<repo-name>",
  title: "<pr-title>",
  body: "<pr-description>",
  head: "<current-branch>",
  base: "<base-branch>",
  draft: <true|false>
})
```

## PR Title Format

### Simple Style (Default)
```
<type>[optional scope]: <emoji> <description>
```
Example: `feat(auth): ✨ Add OAuth2 authentication flow`

### Characteristics
- Use present tense, imperative mood ("Add" not "Added")
- Keep under 72 characters
- Capitalize first letter of description
- No period at end

## PR Description Format

### Simple Style
```markdown
## Summary
<1-3 bullet points describing the changes>

## Test Plan
<Brief testing checklist>

🤖 Generated with [TiyCode](https://github.com/TiyAgents/tiycode)
```

### Full Style
```markdown
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
```

## PR Types & Emojis

| Type | Emoji | Description | When to Use |
|------|-------|-------------|-------------|
| `feat` | ✨ | New feature | Adding new functionality |
| `fix` | 🐛 | Bug fix | Fixing an issue |
| `docs` | 📝 | Documentation | Documentation only changes |
| `style` | 🎨 | Code style | Formatting, missing semi-colons, etc |
| `refactor` | ♻️ | Code refactoring | Neither fixes bug nor adds feature |
| `perf` | ⚡️ | Performance | Performance improvements |
| `test` | ✅ | Testing | Adding missing tests |
| `chore` | 🔧 | Maintenance | Changes to build process or tools |
| `ci` | 👷 | CI/CD | Changes to CI configuration |
| `build` | 📦 | Build system | Changes affecting build system |
| `revert` | ⏪ | Revert | Reverting previous commit |

## Workflow

1. **Check language parameter**: Determine the language for PR content by checking command arguments.

2. **Validate environment**:
   ```bash
   # Check if in git repo
   git rev-parse --is-inside-work-tree

   # Get current branch
   git branch --show-current

   # Check for uncommitted changes
   git status --porcelain
   ```

3. **Detect repository info**:
   ```bash
   # Get remote URL and parse owner/repo
   git remote get-url origin

   # Detect default branch
   git remote show origin | grep 'HEAD branch'
   ```

4. **Analyze changes**:
   ```bash
   # Fetch latest
   git fetch origin

   # Get commit log
   git log origin/<base>..HEAD --oneline

   # Get diff summary
   git diff origin/<base>...HEAD --stat

   # Get full diff for analysis
   git diff origin/<base>...HEAD
   ```

5. **Generate PR content**:
   - Analyze commits to determine PR type
   - Summarize changes across all commits
   - Create title following conventional format
   - Generate description based on style setting

6. **Push branch**:
   ```bash
   git push -u origin <current-branch>
   ```

7. **Create PR**:
   - Primary: use GitHub CLI `gh pr create`
   - If CLI unavailable, fallback to MCP tool: `mcp__github__create_pull_request`

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
   - Solution: Add remote with `git remote add origin <url>`

3. **Branch already has PR**:
   - Error: "A pull request already exists"
   - Solution: Update existing PR or create new branch

4. **No commits to create PR**:
   - Error: "No commits between base and head"
   - Solution: Make commits before creating PR

5. **GitHub CLI unavailable**:
   - Automatically fallback to MCP tool `mcp__github__create_pull_request`
   - Ensure MCP GitHub tools are configured

6. **GitHub CLI not authenticated**:
   - Error: "gh: Not logged in"
   - Solution: Run `gh auth login` first

## Examples

### Simple Feature PR
```
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
```

### Full Style Bug Fix PR
```
Title: fix(auth): 🐛 Resolve session timeout issue

## Summary
Fix an issue where user sessions were expiring prematurely due to incorrect
timestamp comparison in the session validation middleware.

## Changes
### Backend
- Fix timestamp comparison in `src/middleware/auth.js`
- Update session refresh logic in `src/services/session.js`

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
```

## Important Notes

- Default style is `simple` for quick PR creation
- Default language is `english` for PR content
- Use `--language=chinese` to generate Simplified Chinese PR content
- Use `full` style for:
  - Complex features
  - Bug fixes requiring explanation
  - Breaking changes
  - Changes affecting multiple systems
- Always review the generated PR content before confirming
- The tool will intelligently suggest `full` style when appropriate
- PR URL will be displayed upon successful creation

## Windows and Cross-Platform Notes

When running on Windows (cmd.exe), the default shell has limited support for multi-line strings, heredocs, and special character escaping. Follow these guidelines to avoid common pitfalls:

1. **Never pass multi-line PR body directly via --body**. Windows cmd does not support heredoc or `$'...'` syntax, and embedded newlines and quotes are easily mangled. Instead:
   - Create the PR with `gh pr create --fill` (which auto-generates a title from commits), or use `--title` with a single-line title only.
   - Then update the body from a file: `gh pr edit <PR-NUMBER> --body-file pr-description.md`
   - Write the PR description to a temporary markdown file first, then pass that file to `--body-file`.

2. **Two-step approach is the safest workflow on all platforms**:
   - Step 1: `gh pr create --fill` or `gh pr create --title "<title>" --body ""`
   - Step 2: `gh pr edit <PR-NUMBER> --body-file <path-to-description-file>`

3. **Avoid shell-special characters in inline strings**. Characters like `!`, `%`, `"`, `` ` ``, and `\n` are interpreted differently across cmd, PowerShell, and Unix shells. Writing content to a file and referencing it with `--body-file` bypasses all shell escaping issues.

4. **CRLF line endings**: If `git diff` output shows `^M` artifacts, ensure `git config core.autocrlf` is set appropriately for the project."#,
    },
];

#[derive(Debug, Clone)]
pub struct PromptCommandManager;

#[derive(Debug, Clone)]
struct PromptCommandRecord {
    id: String,
    name: String,
    path: String,
    argument_hint: String,
    description: String,
    prompt: String,
    source: String,
    enabled: bool,
    version: u32,
    file_name: String,
    file_path: PathBuf,
}

#[derive(Debug, Clone, Copy)]
struct BuiltinPromptSeed<'a> {
    id: &'a str,
    name: &'a str,
    path: &'a str,
    argument_hint: &'a str,
    description: &'a str,
    prompt: &'a str,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct PromptCommandFrontmatter {
    id: Option<String>,
    name: Option<String>,
    path: Option<String>,
    #[serde(rename = "argumentHint")]
    argument_hint: Option<String>,
    description: Option<String>,
    source: Option<String>,
    enabled: Option<bool>,
    version: Option<u32>,
}

impl PromptCommandManager {
    pub fn new() -> Self {
        Self
    }

    pub fn ensure_builtin_seeded(&self) -> Result<(), AppError> {
        let inputs = BUILTIN_PROMPT_COMMANDS
            .iter()
            .map(BuiltinPromptSeed::to_input)
            .collect::<Vec<_>>();
        self.ensure_builtin_commands(&inputs)
    }

    pub fn list_commands(&self) -> Result<Vec<PromptCommandDto>, AppError> {
        let mut records = self.load_prompt_command_records()?;
        records.sort_by(|left, right| left.name.to_lowercase().cmp(&right.name.to_lowercase()));
        Ok(records
            .into_iter()
            .map(PromptCommandRecord::into_dto)
            .collect())
    }

    pub fn create_command(&self, input: PromptCommandInput) -> Result<PromptCommandDto, AppError> {
        let mut records = self.load_prompt_command_records()?;
        let record = self.build_record_from_input(input, None)?;
        self.ensure_path_is_unique(&records, &record.path, None)?;
        self.ensure_name_is_unique(&records, &record.name, None)?;
        self.write_record(&record)?;
        records.push(record.clone());
        Ok(record.into_dto())
    }

    pub fn update_command(
        &self,
        id: &str,
        input: PromptCommandInput,
    ) -> Result<PromptCommandDto, AppError> {
        let records = self.load_prompt_command_records()?;
        let existing = records
            .iter()
            .find(|record| record.id == id)
            .cloned()
            .ok_or_else(|| {
                AppError::not_found(ErrorSource::Settings, format!("prompt command '{id}'"))
            })?;

        let next_record = self.build_record_from_input(input, Some(&existing))?;
        self.ensure_path_is_unique(&records, &next_record.path, Some(id))?;
        self.ensure_name_is_unique(&records, &next_record.name, Some(id))?;

        self.write_record(&next_record)?;

        if existing.file_path != next_record.file_path && existing.file_path.exists() {
            fs::remove_file(&existing.file_path).map_err(|error| {
                AppError::recoverable(
                    ErrorSource::Settings,
                    "settings.prompt_command.delete_failed",
                    format!(
                        "Unable to remove old prompt file '{}': {error}",
                        existing.file_path.display()
                    ),
                )
            })?;
        }

        Ok(next_record.into_dto())
    }

    pub fn delete_command(&self, id: &str) -> Result<(), AppError> {
        let records = self.load_prompt_command_records()?;
        let existing = records
            .iter()
            .find(|record| record.id == id)
            .ok_or_else(|| {
                AppError::not_found(ErrorSource::Settings, format!("prompt command '{id}'"))
            })?;

        if existing.file_path.exists() {
            fs::remove_file(&existing.file_path).map_err(|error| {
                AppError::recoverable(
                    ErrorSource::Settings,
                    "settings.prompt_command.delete_failed",
                    format!(
                        "Unable to remove prompt file '{}': {error}",
                        existing.file_path.display()
                    ),
                )
            })?;
        }

        Ok(())
    }

    pub fn ensure_builtin_commands(&self, inputs: &[PromptCommandInput]) -> Result<(), AppError> {
        for input in inputs {
            let record = self.build_record_from_input(input.clone(), None)?;
            if record.file_path.exists() {
                continue;
            }
            self.write_record(&record)?;
        }
        Ok(())
    }

    fn load_prompt_command_records(&self) -> Result<Vec<PromptCommandRecord>, AppError> {
        let mut records = Vec::new();
        for dir in [builtin_prompts_dir(), user_prompts_dir()] {
            if !dir.exists() {
                continue;
            }
            for entry in fs::read_dir(&dir)? {
                let entry = entry?;
                let path = entry.path();
                if !path.is_file()
                    || path.extension().and_then(|value| value.to_str()) != Some("md")
                {
                    continue;
                }
                let raw = fs::read_to_string(&path).map_err(|error| {
                    AppError::recoverable(
                        ErrorSource::Settings,
                        "settings.prompt_command.read_failed",
                        format!(
                            "Unable to read prompt command '{}': {error}",
                            path.display()
                        ),
                    )
                })?;
                records.push(self.parse_record(&raw, &path)?);
            }
        }
        Ok(records)
    }

    fn parse_record(&self, raw: &str, path: &Path) -> Result<PromptCommandRecord, AppError> {
        let (frontmatter, body) = split_frontmatter(raw).unwrap_or((None, raw));
        let meta = frontmatter
            .map(parse_frontmatter_map)
            .transpose()?
            .unwrap_or_default();
        let file_name = path
            .file_name()
            .and_then(|value| value.to_str())
            .ok_or_else(|| {
                AppError::validation(ErrorSource::Settings, "Prompt command file name is invalid")
            })?
            .to_string();
        let fallback_name = path
            .file_stem()
            .and_then(|value| value.to_str())
            .unwrap_or("prompt");
        let source = meta.source.unwrap_or_else(|| infer_source_from_path(path));
        let name = meta.name.unwrap_or_else(|| fallback_name.to_string());
        let command_path =
            normalize_command_path(meta.path.unwrap_or_else(|| format!("/prompts:{name}")));
        Ok(PromptCommandRecord {
            id: meta
                .id
                .unwrap_or_else(|| format!("cmd-{}", slugify_file_stem(&name))),
            name,
            path: command_path,
            argument_hint: meta.argument_hint.unwrap_or_default(),
            description: meta.description.unwrap_or_default(),
            prompt: body.trim().to_string(),
            source,
            enabled: meta.enabled.unwrap_or(true),
            version: meta.version.unwrap_or(DEFAULT_PROMPT_VERSION),
            file_name,
            file_path: path.to_path_buf(),
        })
    }

    fn build_record_from_input(
        &self,
        input: PromptCommandInput,
        existing: Option<&PromptCommandRecord>,
    ) -> Result<PromptCommandRecord, AppError> {
        let name = input.name.trim().to_string();
        if name.is_empty() {
            return Err(AppError::validation(
                ErrorSource::Settings,
                "Prompt command name is required",
            ));
        }
        let normalized_source = normalize_source(
            input.source.as_deref().unwrap_or(
                existing
                    .map(|record| record.source.as_str())
                    .unwrap_or("user"),
            ),
        );
        let id = input.id.unwrap_or_else(|| {
            existing
                .map(|record| record.id.clone())
                .unwrap_or_else(|| format!("cmd-{}", Uuid::now_v7()))
        });
        let file_stem = slugify_file_stem(&name);
        let file_name = format!("{file_stem}.md");
        let parent_dir = if normalized_source == "builtin" {
            builtin_prompts_dir()
        } else {
            user_prompts_dir()
        };
        let file_path = parent_dir.join(&file_name);
        Ok(PromptCommandRecord {
            id,
            name,
            path: normalize_command_path(input.path),
            argument_hint: input.argument_hint.unwrap_or_default(),
            description: input.description.unwrap_or_default(),
            prompt: input.prompt.trim().to_string(),
            source: normalized_source,
            enabled: input
                .enabled
                .unwrap_or(existing.map(|record| record.enabled).unwrap_or(true)),
            version: input.version.unwrap_or(
                existing
                    .map(|record| record.version)
                    .unwrap_or(DEFAULT_PROMPT_VERSION),
            ),
            file_name,
            file_path,
        })
    }

    fn ensure_path_is_unique(
        &self,
        records: &[PromptCommandRecord],
        command_path: &str,
        current_id: Option<&str>,
    ) -> Result<(), AppError> {
        if records
            .iter()
            .any(|record| record.path == command_path && current_id != Some(record.id.as_str()))
        {
            return Err(AppError::validation(
                ErrorSource::Settings,
                format!("Prompt command path '{command_path}' is already in use"),
            ));
        }
        Ok(())
    }

    fn ensure_name_is_unique(
        &self,
        records: &[PromptCommandRecord],
        name: &str,
        current_id: Option<&str>,
    ) -> Result<(), AppError> {
        if records.iter().any(|record| {
            record.name.eq_ignore_ascii_case(name) && current_id != Some(record.id.as_str())
        }) {
            return Err(AppError::validation(
                ErrorSource::Settings,
                format!("Prompt command name '{name}' is already in use"),
            ));
        }
        Ok(())
    }

    fn write_record(&self, record: &PromptCommandRecord) -> Result<(), AppError> {
        if let Some(parent) = record.file_path.parent() {
            fs::create_dir_all(parent)?;
        }
        let frontmatter = PromptCommandFrontmatter {
            id: Some(record.id.clone()),
            name: Some(record.name.clone()),
            path: Some(record.path.clone()),
            argument_hint: Some(record.argument_hint.clone()),
            description: Some(record.description.clone()),
            source: Some(record.source.clone()),
            enabled: Some(record.enabled),
            version: Some(record.version),
        };
        let yaml_like = serialize_frontmatter(&frontmatter);
        let content = format!(
            "{FRONTMATTER_DELIMITER}\n{yaml_like}\n{FRONTMATTER_DELIMITER}\n\n{}\n",
            record.prompt.trim()
        );
        fs::write(&record.file_path, content).map_err(|error| {
            AppError::recoverable(
                ErrorSource::Settings,
                "settings.prompt_command.write_failed",
                format!(
                    "Unable to write prompt command '{}': {error}",
                    record.file_path.display()
                ),
            )
        })
    }
}

impl PromptCommandRecord {
    fn into_dto(self) -> PromptCommandDto {
        PromptCommandDto {
            id: self.id,
            name: self.name,
            path: self.path,
            argument_hint: self.argument_hint,
            description: self.description,
            prompt: self.prompt,
            source: self.source,
            enabled: self.enabled,
            version: self.version,
            file_name: self.file_name,
        }
    }
}

impl BuiltinPromptSeed<'_> {
    fn to_input(&self) -> PromptCommandInput {
        PromptCommandInput {
            id: Some(self.id.to_string()),
            name: self.name.to_string(),
            path: self.path.to_string(),
            argument_hint: Some(self.argument_hint.to_string()),
            description: Some(self.description.to_string()),
            prompt: self.prompt.to_string(),
            source: Some("builtin".to_string()),
            enabled: Some(true),
            version: Some(DEFAULT_PROMPT_VERSION),
        }
    }
}

fn tiy_home() -> PathBuf {
    dirs::home_dir()
        .expect("cannot resolve HOME directory")
        .join(".tiy")
}

fn prompts_root() -> PathBuf {
    tiy_home().join(PROMPTS_DIR_NAME)
}

fn builtin_prompts_dir() -> PathBuf {
    prompts_root().join(BUILTIN_PROMPTS_DIR_NAME)
}

fn user_prompts_dir() -> PathBuf {
    prompts_root().join(USER_PROMPTS_DIR_NAME)
}

fn normalize_source(source: &str) -> String {
    if source.trim().eq_ignore_ascii_case("builtin") {
        "builtin".to_string()
    } else {
        "user".to_string()
    }
}

fn infer_source_from_path(path: &Path) -> String {
    path.parent()
        .and_then(|parent| parent.file_name())
        .and_then(|value| value.to_str())
        .map(normalize_source)
        .unwrap_or_else(|| "user".to_string())
}

fn normalize_command_path(path: String) -> String {
    let trimmed = path.trim();
    if trimmed.starts_with('/') {
        trimmed.to_string()
    } else {
        format!("/{trimmed}")
    }
}

fn slugify_file_stem(value: &str) -> String {
    let mut slug = String::new();
    let mut last_dash = false;
    for ch in value.chars() {
        let normalized = ch.to_ascii_lowercase();
        if normalized.is_ascii_alphanumeric() {
            slug.push(normalized);
            last_dash = false;
        } else if !last_dash {
            slug.push('-');
            last_dash = true;
        }
    }
    slug.trim_matches('-').to_string().if_empty_then("prompt")
}

fn split_frontmatter(raw: &str) -> Option<(Option<&str>, &str)> {
    let trimmed = raw.trim_start();
    let delimiter_prefix = format!("{FRONTMATTER_DELIMITER}\n");
    if !trimmed.starts_with(&delimiter_prefix) {
        return Some((None, raw));
    }
    let remainder = &trimmed[delimiter_prefix.len()..];
    let delimiter_suffix = format!("\n{FRONTMATTER_DELIMITER}\n");
    let end = remainder.find(&delimiter_suffix)?;
    let frontmatter = &remainder[..end];
    let body = &remainder[end + delimiter_suffix.len()..];
    Some((Some(frontmatter), body))
}

fn parse_frontmatter_map(frontmatter: &str) -> Result<PromptCommandFrontmatter, AppError> {
    let mut meta = PromptCommandFrontmatter::default();
    for line in frontmatter.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let Some((key, value)) = trimmed.split_once(':') else {
            continue;
        };
        let normalized_key = key.trim();
        let normalized_value = value.trim();
        match normalized_key {
            "id" => meta.id = Some(normalized_value.to_string()),
            "name" => meta.name = Some(normalized_value.to_string()),
            "path" => meta.path = Some(normalized_value.to_string()),
            "argumentHint" => meta.argument_hint = Some(normalized_value.to_string()),
            "description" => meta.description = Some(normalized_value.to_string()),
            "source" => meta.source = Some(normalized_value.to_string()),
            "enabled" => meta.enabled = Some(matches!(normalized_value, "true" | "yes" | "1")),
            "version" => {
                meta.version = Some(normalized_value.parse::<u32>().map_err(|error| {
                    AppError::recoverable(
                        ErrorSource::Settings,
                        "settings.prompt_command.invalid_frontmatter",
                        format!("Invalid prompt command version: {error}"),
                    )
                })?)
            }
            _ => {}
        }
    }
    Ok(meta)
}

fn serialize_frontmatter(frontmatter: &PromptCommandFrontmatter) -> String {
    let mut lines = Vec::new();
    if let Some(value) = &frontmatter.id {
        lines.push(format!("id: {}", sanitize_frontmatter_value(value)));
    }
    if let Some(value) = &frontmatter.name {
        lines.push(format!("name: {}", sanitize_frontmatter_value(value)));
    }
    if let Some(value) = &frontmatter.path {
        lines.push(format!("path: {}", sanitize_frontmatter_value(value)));
    }
    if let Some(value) = &frontmatter.argument_hint {
        lines.push(format!(
            "argumentHint: {}",
            sanitize_frontmatter_value(value)
        ));
    }
    if let Some(value) = &frontmatter.description {
        lines.push(format!(
            "description: {}",
            sanitize_frontmatter_value(value)
        ));
    }
    if let Some(value) = &frontmatter.source {
        lines.push(format!("source: {}", sanitize_frontmatter_value(value)));
    }
    if let Some(value) = frontmatter.enabled {
        lines.push(format!("enabled: {value}"));
    }
    if let Some(value) = frontmatter.version {
        lines.push(format!("version: {value}"));
    }
    lines.join("\n")
}

fn sanitize_frontmatter_value(value: &str) -> String {
    let sanitized = value
        .chars()
        .map(|ch| if ch.is_control() { ' ' } else { ch })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    sanitized.trim().to_string()
}

trait IfEmptyThen {
    fn if_empty_then(self, fallback: &str) -> String;
}

impl IfEmptyThen for String {
    fn if_empty_then(self, fallback: &str) -> String {
        if self.is_empty() {
            fallback.to_string()
        } else {
            self
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;
    use tempfile::tempdir;

    #[test]
    fn ensure_builtin_seeded_preserves_existing_builtin_prompt_files() {
        let temp_home = tempdir().expect("tempdir");
        let original_home = env::var("HOME").ok();
        unsafe {
            env::set_var("HOME", temp_home.path());
        }

        let existing_path = temp_home.path().join(".tiy/prompts/builtin/commit.md");
        fs::create_dir_all(existing_path.parent().expect("parent")).expect("create prompts dir");
        fs::write(
            &existing_path,
            "---\nname: commit\nsource: builtin\n---\n\ncustom body\n",
        )
        .expect("write existing prompt");

        let manager = PromptCommandManager::new();
        manager
            .ensure_builtin_seeded()
            .expect("seed builtin prompts");

        let content = fs::read_to_string(&existing_path).expect("read prompt");
        assert!(content.contains("custom body"));

        if let Some(home) = original_home {
            unsafe {
                env::set_var("HOME", home);
            }
        } else {
            unsafe {
                env::remove_var("HOME");
            }
        }
    }
}
