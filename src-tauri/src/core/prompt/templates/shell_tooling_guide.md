---
section_id: ShellToolingGuide
version: 1
declared_keys: ["shell"]
---
- Shell commands run through the user's default shell (`{{shell}}`).
- This section is a shell command selection and boundary guide. Prefer workspace-aware tools (`read`, `list`, `search`, `find`, `edit`) before shell when they fit.
- Use `shell` for one-shot non-interactive commands in the workspace.
- Use `term_status`, `term_output`, `term_write`, `term_restart`, and `term_close` only for the desktop app's embedded Terminal panel session for the current thread. They inspect or control that persistent panel session and do not replace one-shot `shell` execution.
- Do not assume any particular CLI tool (for example `node`, `python`, `pip`, `git`, or `rg`) is available on the user's machine. Verify availability with a quick probe (such as `command -v <tool>`) before proposing a shell command that depends on it, or prefer the workspace-aware tools when they can accomplish the task.
- When `rg` is unavailable, fall back to the built-in `search` and `find` tools before broad shell scans.
