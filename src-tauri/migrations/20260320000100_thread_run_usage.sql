ALTER TABLE thread_runs
    ADD COLUMN input_tokens INTEGER NOT NULL DEFAULT 0;

ALTER TABLE thread_runs
    ADD COLUMN output_tokens INTEGER NOT NULL DEFAULT 0;

ALTER TABLE thread_runs
    ADD COLUMN cache_read_tokens INTEGER NOT NULL DEFAULT 0;

ALTER TABLE thread_runs
    ADD COLUMN cache_write_tokens INTEGER NOT NULL DEFAULT 0;

ALTER TABLE thread_runs
    ADD COLUMN total_tokens INTEGER NOT NULL DEFAULT 0;
