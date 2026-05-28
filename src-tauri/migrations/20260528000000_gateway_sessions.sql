-- Gateway user sessions: persists the per-user workspace/thread binding for IM channels.
CREATE TABLE IF NOT EXISTS gateway_sessions (
    user_id              TEXT NOT NULL,
    platform             TEXT NOT NULL,  -- 'weixin' | 'wecom'
    current_workspace_id TEXT REFERENCES workspaces(id) ON DELETE SET NULL,
    current_thread_id    TEXT REFERENCES threads(id) ON DELETE SET NULL,
    state                TEXT NOT NULL DEFAULT 'idle',
    updated_at           TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%f', 'now')),
    PRIMARY KEY (user_id, platform)
);
