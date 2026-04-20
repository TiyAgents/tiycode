-- Persist thread-level profile selection.
-- Date: 2026-04-20
-- Description: Adds threads.profile_id and backfills it from latest run profile,
--              then legacy thread_profile_bindings, then active_profile_id.

ALTER TABLE threads ADD COLUMN profile_id TEXT;

UPDATE threads
SET profile_id = (
  SELECT tr.profile_id
  FROM thread_runs tr
  WHERE tr.thread_id = threads.id
    AND tr.profile_id IS NOT NULL
    AND TRIM(tr.profile_id) <> ''
  ORDER BY tr.started_at DESC
  LIMIT 1
)
WHERE profile_id IS NULL;

UPDATE threads
SET profile_id = (
  SELECT json_extract(s.value_json, '$.' || threads.id)
  FROM settings s
  WHERE s.key = 'thread_profile_bindings'
)
WHERE profile_id IS NULL
  AND EXISTS (
    SELECT 1
    FROM settings s
    WHERE s.key = 'thread_profile_bindings'
      AND json_extract(s.value_json, '$.' || threads.id) IS NOT NULL
      AND TRIM(json_extract(s.value_json, '$.' || threads.id)) <> ''
  );

UPDATE threads
SET profile_id = (
  SELECT json_extract(s.value_json, '$')
  FROM settings s
  WHERE s.key = 'active_profile_id'
)
WHERE profile_id IS NULL
  AND EXISTS (
    SELECT 1
    FROM settings s
    WHERE s.key = 'active_profile_id'
      AND json_extract(s.value_json, '$') IS NOT NULL
      AND TRIM(json_extract(s.value_json, '$')) <> ''
  );

CREATE INDEX IF NOT EXISTS idx_threads_profile_id ON threads(profile_id);
