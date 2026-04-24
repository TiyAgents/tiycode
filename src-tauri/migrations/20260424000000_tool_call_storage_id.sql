-- Split tool_calls storage identity from provider/runtime tool call identity.
-- Existing rows used tool_calls.id for both purposes; keep those rows compatible
-- by backfilling tool_call_id from id. New rows use id as an internal storage
-- primary key and tool_call_id as the raw model/runtime correlation id.

ALTER TABLE tool_calls ADD COLUMN tool_call_id TEXT;

UPDATE tool_calls
SET tool_call_id = id
WHERE tool_call_id IS NULL OR TRIM(tool_call_id) = '';

CREATE UNIQUE INDEX IF NOT EXISTS idx_tool_calls_run_tool_call_id_unique
    ON tool_calls(run_id, tool_call_id);
