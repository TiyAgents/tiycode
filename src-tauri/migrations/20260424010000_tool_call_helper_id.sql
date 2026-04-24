-- Mark tool calls that were executed inside helper/subagent runs.
-- Parent LLM history reconstruction must only replay top-level model tool calls;
-- helper-internal calls remain persisted for audit/UI but are excluded from that
-- parent-visible history path.

ALTER TABLE tool_calls ADD COLUMN helper_id TEXT;

-- Keep this partial-index predicate aligned with the parent-visible query in
-- tool_call_repo::list_parent_visible_by_run_ids. The colon filter is a legacy
-- compatibility guard for helper-internal rows created before helper_id existed.
CREATE INDEX IF NOT EXISTS idx_tool_calls_parent_visible_run_started
    ON tool_calls(run_id, started_at, id)
    WHERE helper_id IS NULL AND instr(COALESCE(tool_call_id, id), ':') = 0;
