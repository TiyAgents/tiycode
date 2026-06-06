---
section_id: SandboxPermissions
version: 1
declared_keys: ["workspace_path", "approval_policy", "run_mode_line", "writable_roots_line"]
---
- Effective runtime sandbox: workspace-scoped tool execution with policy checks.
- Workspace boundary: file and path-aware tools are restricted to the current workspace (`{{workspace_path}}`).
- Approval policy: {{approval_policy}}.
- Read-only tools are generally auto-allowed; mutating tools may require approval.
- {{run_mode_line}}{{writable_roots_line}}
- Outer host sandbox metadata is not exposed here; rely on these effective runtime constraints.
