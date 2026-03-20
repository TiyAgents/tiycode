ALTER TABLE run_helpers
    ADD COLUMN input_tokens INTEGER NOT NULL DEFAULT 0;

ALTER TABLE run_helpers
    ADD COLUMN output_tokens INTEGER NOT NULL DEFAULT 0;

ALTER TABLE run_helpers
    ADD COLUMN cache_read_tokens INTEGER NOT NULL DEFAULT 0;

ALTER TABLE run_helpers
    ADD COLUMN cache_write_tokens INTEGER NOT NULL DEFAULT 0;

ALTER TABLE run_helpers
    ADD COLUMN total_tokens INTEGER NOT NULL DEFAULT 0;
