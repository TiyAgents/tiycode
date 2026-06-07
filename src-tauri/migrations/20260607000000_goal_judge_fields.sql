-- Goal Judge verification fields: persist the most recent independent Judge
-- verdict for a goal. Acceptance is expressed as status='complete' AND
-- judge_passed=1 (the main agent can no longer self-attest completion).
ALTER TABLE goals ADD COLUMN judge_passed INTEGER NOT NULL DEFAULT 0;       -- bool
ALTER TABLE goals ADD COLUMN judge_completeness INTEGER;                    -- 0-100, nullable
ALTER TABLE goals ADD COLUMN judge_findings TEXT;                          -- JSON array, nullable
ALTER TABLE goals ADD COLUMN judge_summary TEXT;                           -- nullable
ALTER TABLE goals ADD COLUMN judge_evaluated_run_id TEXT;                  -- nullable

-- Backfill goals already completed via the legacy goal_scored path so that an
-- upgrade does not treat them as un-verified (which would otherwise let goal
-- continuation re-open them).
UPDATE goals
SET judge_passed = 1,
    judge_summary = COALESCE(judge_summary, evidence),
    judge_completeness = COALESCE(judge_completeness, 100)
WHERE status = 'complete';
