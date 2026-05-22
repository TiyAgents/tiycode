-- Custom subagents: user-defined sub-agent configurations
CREATE TABLE IF NOT EXISTS custom_subagents (
    id TEXT PRIMARY KEY NOT NULL,
    name TEXT NOT NULL,
    slug TEXT NOT NULL UNIQUE,
    system_prompt TEXT NOT NULL,
    invocation_description TEXT NOT NULL,
    allowed_tools TEXT NOT NULL DEFAULT '[]',
    is_enabled INTEGER NOT NULL DEFAULT 1,
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at TEXT NOT NULL DEFAULT (datetime('now'))
);

-- Profile ↔ Subagent access: many-to-many relationship
CREATE TABLE IF NOT EXISTS profile_subagent_access (
    profile_id TEXT NOT NULL REFERENCES agent_profiles(id) ON DELETE CASCADE,
    subagent_id TEXT NOT NULL REFERENCES custom_subagents(id) ON DELETE CASCADE,
    PRIMARY KEY (profile_id, subagent_id)
);
