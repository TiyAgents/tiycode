-- Migration: add run_helpers and migrate legacy run_subtasks summaries

CREATE TABLE IF NOT EXISTS run_helpers (
    id                   TEXT PRIMARY KEY,
    run_id               TEXT NOT NULL REFERENCES thread_runs(id),
    thread_id            TEXT NOT NULL REFERENCES threads(id),
    helper_kind          TEXT NOT NULL,
    parent_tool_call_id  TEXT,
    status               TEXT NOT NULL DEFAULT 'created',
    model_role           TEXT NOT NULL DEFAULT 'assistant',
    provider_id          TEXT,
    model_id             TEXT,
    input_summary        TEXT,
    output_summary       TEXT,
    error_summary        TEXT,
    started_at           TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    finished_at          TEXT
);

CREATE INDEX IF NOT EXISTS idx_run_helpers_run
    ON run_helpers(run_id);

CREATE INDEX IF NOT EXISTS idx_run_helpers_thread
    ON run_helpers(thread_id);

INSERT INTO run_helpers (
    id,
    run_id,
    thread_id,
    helper_kind,
    parent_tool_call_id,
    status,
    model_role,
    provider_id,
    model_id,
    input_summary,
    output_summary,
    error_summary,
    started_at,
    finished_at
)
SELECT
    legacy.id,
    legacy.run_id,
    legacy.thread_id,
    legacy.subtask_type,
    NULL,
    legacy.status,
    legacy.role,
    legacy.provider_id,
    legacy.model_id,
    NULL,
    legacy.summary,
    legacy.error_message,
    legacy.started_at,
    legacy.finished_at
FROM run_subtasks AS legacy
WHERE NOT EXISTS (
    SELECT 1
    FROM run_helpers AS helpers
    WHERE helpers.id = legacy.id
);
