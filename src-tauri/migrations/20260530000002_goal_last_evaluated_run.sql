-- Track the latest terminal run consumed by goal evaluation.
-- This prevents duplicate post-run accounting and duplicate continuations when
-- terminal events are replayed or an older frontend path calls goal_evaluate.
ALTER TABLE goals ADD COLUMN last_evaluated_run_id TEXT;
