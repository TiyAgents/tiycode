-- Track cumulative active running time per run, excluding pauses
-- (waiting_approval, needs_reply, etc.).
--
-- elapsed_running_secs: seconds the run has actually been in "running" status.
-- running_since:        ISO timestamp marking the start of the current running
--                       segment; NULL when the run is not actively running.

ALTER TABLE thread_runs ADD COLUMN elapsed_running_secs INTEGER NOT NULL DEFAULT 0;
ALTER TABLE thread_runs ADD COLUMN running_since TEXT;

-- Back-fill: runs that are currently in "running" status were running since
-- their started_at. This is imprecise (ignores prior pauses) but is the best
-- approximation for a one-time upgrade.
UPDATE thread_runs
   SET running_since = started_at
 WHERE status = 'running';
