-- Tiy Agent Initial Schema
-- Date: 2026-03-16
-- Description: Creates all core tables for the Tiy Agent desktop application.
-- Storage location: $HOME/.tiy/db/tiycode.db

--------------------------------------------------------------------------------
-- 0. Pragmas (applied at connection time, not in migration)
--    PRAGMA journal_mode = WAL;
--    PRAGMA synchronous = NORMAL;
--    PRAGMA foreign_keys = ON;
--    PRAGMA busy_timeout = 5000;
--    PRAGMA cache_size = -8000;
--------------------------------------------------------------------------------

--------------------------------------------------------------------------------
-- 1. workspaces
--------------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS workspaces (
    id                TEXT PRIMARY KEY,
    name              TEXT NOT NULL,
    path              TEXT NOT NULL,
    canonical_path    TEXT NOT NULL UNIQUE,
    display_path      TEXT NOT NULL,
    is_default        INTEGER NOT NULL DEFAULT 0,
    is_git            INTEGER NOT NULL DEFAULT 0,
    auto_work_tree    INTEGER NOT NULL DEFAULT 0,
    status            TEXT NOT NULL DEFAULT 'ready',
    last_validated_at TEXT,
    created_at        TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    updated_at        TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);

CREATE INDEX IF NOT EXISTS idx_workspaces_is_default
    ON workspaces(is_default) WHERE is_default = 1;

CREATE INDEX IF NOT EXISTS idx_workspaces_status
    ON workspaces(status);

--------------------------------------------------------------------------------
-- 2. threads
--------------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS threads (
    id              TEXT PRIMARY KEY,
    workspace_id    TEXT NOT NULL REFERENCES workspaces(id),
    title           TEXT NOT NULL DEFAULT '',
    status          TEXT NOT NULL DEFAULT 'idle',
    summary         TEXT,
    last_active_at  TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    created_at      TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    updated_at      TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);

CREATE INDEX IF NOT EXISTS idx_threads_workspace
    ON threads(workspace_id);

CREATE INDEX IF NOT EXISTS idx_threads_workspace_active
    ON threads(workspace_id, last_active_at DESC);

CREATE INDEX IF NOT EXISTS idx_threads_status
    ON threads(status) WHERE status != 'archived';

--------------------------------------------------------------------------------
-- 3. providers
--------------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS providers (
    id                  TEXT PRIMARY KEY,
    name                TEXT NOT NULL,
    protocol_type       TEXT NOT NULL DEFAULT 'openai',
    base_url            TEXT NOT NULL,
    api_key_encrypted   TEXT,
    enabled             INTEGER NOT NULL DEFAULT 1,
    custom_headers_json TEXT,
    created_at          TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    updated_at          TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);

--------------------------------------------------------------------------------
-- 4. provider_models
--------------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS provider_models (
    id                TEXT PRIMARY KEY,
    provider_id       TEXT NOT NULL REFERENCES providers(id) ON DELETE CASCADE,
    model_name        TEXT NOT NULL,
    display_name      TEXT,
    enabled           INTEGER NOT NULL DEFAULT 1,
    capabilities_json TEXT,
    created_at        TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);

CREATE INDEX IF NOT EXISTS idx_provider_models_provider
    ON provider_models(provider_id);

CREATE UNIQUE INDEX IF NOT EXISTS idx_provider_models_unique
    ON provider_models(provider_id, model_name);

--------------------------------------------------------------------------------
-- 5. agent_profiles
--------------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS agent_profiles (
    id                        TEXT PRIMARY KEY,
    name                      TEXT NOT NULL,
    custom_instructions       TEXT,
    response_style            TEXT,
    response_language         TEXT,
    primary_provider_id       TEXT,
    primary_model_id          TEXT,
    auxiliary_provider_id     TEXT,
    auxiliary_model_id        TEXT,
    lightweight_provider_id   TEXT,
    lightweight_model_id      TEXT,
    is_default                INTEGER NOT NULL DEFAULT 0,
    created_at                TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    updated_at                TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);

--------------------------------------------------------------------------------
-- 6. thread_runs
--------------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS thread_runs (
    id                          TEXT PRIMARY KEY,
    thread_id                   TEXT NOT NULL REFERENCES threads(id),
    profile_id                  TEXT,
    run_mode                    TEXT NOT NULL DEFAULT 'default',
    execution_strategy          TEXT,
    source_plan_run_id          TEXT REFERENCES thread_runs(id),
    provider_id                 TEXT,
    model_id                    TEXT,
    effective_model_plan_json   TEXT,
    status                      TEXT NOT NULL DEFAULT 'created',
    error_message               TEXT,
    started_at                  TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    finished_at                 TEXT
);

CREATE INDEX IF NOT EXISTS idx_runs_thread
    ON thread_runs(thread_id, started_at DESC);

CREATE INDEX IF NOT EXISTS idx_runs_thread_active
    ON thread_runs(thread_id, status)
    WHERE status NOT IN ('completed', 'failed', 'denied', 'interrupted', 'cancelled');

CREATE INDEX IF NOT EXISTS idx_runs_status
    ON thread_runs(status)
    WHERE status NOT IN ('completed', 'failed', 'denied', 'interrupted', 'cancelled');

--------------------------------------------------------------------------------
-- 7. messages
--------------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS messages (
    id               TEXT PRIMARY KEY,
    thread_id        TEXT NOT NULL REFERENCES threads(id),
    run_id           TEXT REFERENCES thread_runs(id),
    role             TEXT NOT NULL,
    content_markdown TEXT NOT NULL DEFAULT '',
    message_type     TEXT NOT NULL DEFAULT 'plain_message',
    status           TEXT NOT NULL DEFAULT 'completed',
    metadata_json    TEXT,
    created_at       TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);

CREATE INDEX IF NOT EXISTS idx_messages_thread
    ON messages(thread_id, created_at);

CREATE INDEX IF NOT EXISTS idx_messages_thread_page
    ON messages(thread_id, id DESC);

CREATE INDEX IF NOT EXISTS idx_messages_run
    ON messages(run_id) WHERE run_id IS NOT NULL;

CREATE INDEX IF NOT EXISTS idx_messages_type
    ON messages(thread_id, message_type)
    WHERE message_type IN ('plan', 'approval_prompt', 'summary_marker');

--------------------------------------------------------------------------------
-- 8. run_subtasks
--------------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS run_subtasks (
    id              TEXT PRIMARY KEY,
    run_id          TEXT NOT NULL REFERENCES thread_runs(id),
    thread_id       TEXT NOT NULL REFERENCES threads(id),
    subtask_type    TEXT NOT NULL,
    role            TEXT NOT NULL DEFAULT 'auxiliary',
    provider_id     TEXT,
    model_id        TEXT,
    status          TEXT NOT NULL DEFAULT 'created',
    started_at      TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    finished_at     TEXT,
    summary         TEXT,
    error_message   TEXT
);

CREATE INDEX IF NOT EXISTS idx_subtasks_run
    ON run_subtasks(run_id);

CREATE INDEX IF NOT EXISTS idx_subtasks_thread
    ON run_subtasks(thread_id);

--------------------------------------------------------------------------------
-- 9. tool_calls
--------------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS tool_calls (
    id                  TEXT PRIMARY KEY,
    run_id              TEXT NOT NULL REFERENCES thread_runs(id),
    thread_id           TEXT NOT NULL REFERENCES threads(id),
    tool_name           TEXT NOT NULL,
    tool_input_json     TEXT NOT NULL DEFAULT '{}',
    tool_output_json    TEXT,
    status              TEXT NOT NULL DEFAULT 'requested',
    approval_status     TEXT,
    policy_verdict_json TEXT,
    started_at          TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    finished_at         TEXT
);

CREATE INDEX IF NOT EXISTS idx_tool_calls_run
    ON tool_calls(run_id);

CREATE INDEX IF NOT EXISTS idx_tool_calls_thread
    ON tool_calls(thread_id);

CREATE INDEX IF NOT EXISTS idx_tool_calls_pending
    ON tool_calls(status)
    WHERE status IN ('requested', 'waiting_approval', 'running');

CREATE INDEX IF NOT EXISTS idx_tool_calls_tool
    ON tool_calls(tool_name, thread_id);

--------------------------------------------------------------------------------
-- 10. settings
--------------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS settings (
    key         TEXT PRIMARY KEY,
    value_json  TEXT NOT NULL,
    updated_at  TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);

--------------------------------------------------------------------------------
-- 11. policies
--------------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS policies (
    key         TEXT PRIMARY KEY,
    value_json  TEXT NOT NULL,
    updated_at  TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);

--------------------------------------------------------------------------------
-- 12. commands
--------------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS commands (
    id              TEXT PRIMARY KEY,
    name            TEXT NOT NULL UNIQUE,
    path            TEXT,
    args_hint       TEXT,
    description     TEXT,
    prompt_template TEXT,
    created_at      TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    updated_at      TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);

--------------------------------------------------------------------------------
-- 13. marketplace_items
--------------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS marketplace_items (
    item_id       TEXT PRIMARY KEY,
    category      TEXT NOT NULL,
    name          TEXT NOT NULL,
    description   TEXT,
    source        TEXT,
    version       TEXT,
    installed     INTEGER NOT NULL DEFAULT 0,
    enabled       INTEGER NOT NULL DEFAULT 0,
    metadata_json TEXT,
    updated_at    TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);

CREATE INDEX IF NOT EXISTS idx_marketplace_category
    ON marketplace_items(category);

CREATE INDEX IF NOT EXISTS idx_marketplace_installed
    ON marketplace_items(installed, enabled);

--------------------------------------------------------------------------------
-- 14. audit_events
--------------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS audit_events (
    id                TEXT PRIMARY KEY,
    actor_type        TEXT NOT NULL,
    actor_id          TEXT,
    source            TEXT NOT NULL,
    workspace_id      TEXT REFERENCES workspaces(id),
    thread_id         TEXT REFERENCES threads(id),
    run_id            TEXT REFERENCES thread_runs(id),
    tool_call_id      TEXT REFERENCES tool_calls(id),
    action            TEXT NOT NULL,
    target_type       TEXT,
    target_id         TEXT,
    policy_check_json TEXT,
    result_json       TEXT,
    created_at        TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);

CREATE INDEX IF NOT EXISTS idx_audit_thread
    ON audit_events(thread_id, created_at DESC);

CREATE INDEX IF NOT EXISTS idx_audit_run
    ON audit_events(run_id) WHERE run_id IS NOT NULL;

CREATE INDEX IF NOT EXISTS idx_audit_workspace
    ON audit_events(workspace_id, created_at DESC);

CREATE INDEX IF NOT EXISTS idx_audit_action
    ON audit_events(action, created_at DESC);

CREATE INDEX IF NOT EXISTS idx_audit_tool_call
    ON audit_events(tool_call_id) WHERE tool_call_id IS NOT NULL;

--------------------------------------------------------------------------------
-- 15. automation_runs (Phase 3 reserved)
--------------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS automation_runs (
    id              TEXT PRIMARY KEY,
    automation_id   TEXT NOT NULL REFERENCES marketplace_items(item_id),
    status          TEXT NOT NULL DEFAULT 'pending',
    started_at      TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    finished_at     TEXT,
    result_summary  TEXT
);

CREATE INDEX IF NOT EXISTS idx_automation_runs_automation
    ON automation_runs(automation_id, started_at DESC);

--------------------------------------------------------------------------------
-- 16. thread_summaries
--------------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS thread_summaries (
    id                  TEXT PRIMARY KEY,
    thread_id           TEXT NOT NULL REFERENCES threads(id),
    source_range_start  TEXT NOT NULL,
    source_range_end    TEXT NOT NULL,
    summary_text        TEXT NOT NULL,
    model_id            TEXT,
    status              TEXT NOT NULL DEFAULT 'active',
    created_at          TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);

CREATE INDEX IF NOT EXISTS idx_thread_summaries_thread
    ON thread_summaries(thread_id, status);

--------------------------------------------------------------------------------
-- 17. terminal_sessions
--------------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS terminal_sessions (
    id            TEXT PRIMARY KEY,
    thread_id     TEXT NOT NULL REFERENCES threads(id),
    workspace_id  TEXT NOT NULL REFERENCES workspaces(id),
    shell_path    TEXT,
    cwd           TEXT,
    status        TEXT NOT NULL DEFAULT 'created',
    pid           INTEGER,
    exit_code     INTEGER,
    created_at    TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    exited_at     TEXT
);

CREATE INDEX IF NOT EXISTS idx_terminal_sessions_thread
    ON terminal_sessions(thread_id);

CREATE UNIQUE INDEX IF NOT EXISTS idx_terminal_sessions_active
    ON terminal_sessions(thread_id)
    WHERE status IN ('created', 'running');

--------------------------------------------------------------------------------
-- 18. Seed default settings
--------------------------------------------------------------------------------
INSERT OR IGNORE INTO settings (key, value_json) VALUES
    ('theme', '"system"'),
    ('language', '"zh-CN"'),
    ('startup_behavior', '{"restore_last_workspace": true}');

INSERT OR IGNORE INTO policies (key, value_json) VALUES
    ('approval_policy', '{"mode": "require_for_mutations"}'),
    ('sandbox_policy', '{"enabled": false}'),
    ('network_access', '{"allowed": true, "blocked_domains": []}'),
    ('allow_list', '[]'),
    ('deny_list', '[]'),
    ('writable_roots', '[]');
