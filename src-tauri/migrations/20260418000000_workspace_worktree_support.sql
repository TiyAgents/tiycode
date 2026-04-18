-- Tiy Agent: add Git worktree support columns to `workspaces` table.
-- Date: 2026-04-18
-- Description: Extends `workspaces` so that a worktree can be modeled as a
--              workspace row with a pointer to its parent repo workspace.
--              Historical rows keep `kind='standalone'`; they are upgraded to
--              `kind='repo'` by WorkspaceManager when detected as a Git repo.

ALTER TABLE workspaces ADD COLUMN kind TEXT NOT NULL DEFAULT 'standalone';
ALTER TABLE workspaces ADD COLUMN parent_workspace_id TEXT NULL REFERENCES workspaces(id);
ALTER TABLE workspaces ADD COLUMN git_common_dir TEXT NULL;
ALTER TABLE workspaces ADD COLUMN branch TEXT NULL;
ALTER TABLE workspaces ADD COLUMN worktree_name TEXT NULL;

CREATE INDEX IF NOT EXISTS idx_workspaces_parent
    ON workspaces(parent_workspace_id);

CREATE INDEX IF NOT EXISTS idx_workspaces_kind
    ON workspaces(kind);
