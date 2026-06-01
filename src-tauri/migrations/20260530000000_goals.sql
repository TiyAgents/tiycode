-- Goals: persisted thread goals for autonomous cross-turn task execution.
-- One goal per thread. Tracks objective, status, usage counters, and completion evidence.
CREATE TABLE IF NOT EXISTS goals (
    id TEXT PRIMARY KEY NOT NULL,
    thread_id TEXT NOT NULL REFERENCES threads(id) ON DELETE CASCADE,
    objective TEXT NOT NULL,
    status TEXT NOT NULL CHECK(status IN ('active', 'paused', 'budget_limited', 'complete')),
    token_budget INTEGER,
    tokens_used INTEGER NOT NULL DEFAULT 0,
    time_used_seconds INTEGER NOT NULL DEFAULT 0,
    turns_used INTEGER NOT NULL DEFAULT 0,
    max_turns INTEGER NOT NULL DEFAULT 50,
    pause_reason TEXT,           -- 'clarify_pending' | 'plan_pending' | 'idle_blocked' | 'user_requested' | 'budget_exhausted' | 'interrupted'
    pause_detail TEXT,           -- contextual detail (e.g. the clarify question text)
    evidence TEXT,               -- completion evidence
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);

CREATE INDEX IF NOT EXISTS idx_goals_thread_id ON goals(thread_id);
