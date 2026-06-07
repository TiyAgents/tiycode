-- Drop goal-level time accounting. Time-tracking moved to thread_runs.elapsed_running_secs
-- (added by 20260604000000_run_elapsed_tracking.sql), which is summed across all of a thread's
-- runs (planning + implementation) and rendered by the workbench-shell timer. The goal-level
-- time_used_seconds column was write-only with no readers in budget enforcement, UI, or logging.
ALTER TABLE goals DROP COLUMN time_used_seconds;
