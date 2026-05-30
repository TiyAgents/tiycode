-- Add UNIQUE constraint on goals.thread_id to prevent duplicate goals
-- for the same thread from concurrent create_goal calls.
DROP INDEX IF EXISTS idx_goals_thread_id;
CREATE UNIQUE INDEX IF NOT EXISTS idx_goals_thread_id ON goals(thread_id);
