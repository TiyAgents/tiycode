-- Provider settings v2
-- Date: 2026-03-19
-- Description: Align provider persistence with tiy-core provider catalog and
--              preserve richer provider model settings.

ALTER TABLE providers ADD COLUMN provider_kind TEXT NOT NULL DEFAULT 'custom';
ALTER TABLE providers ADD COLUMN provider_key TEXT NOT NULL DEFAULT '';
ALTER TABLE providers ADD COLUMN mapping_locked INTEGER NOT NULL DEFAULT 0;

UPDATE providers
SET provider_key = id
WHERE provider_key = '';

ALTER TABLE provider_models ADD COLUMN context_window TEXT;
ALTER TABLE provider_models ADD COLUMN max_output_tokens TEXT;
ALTER TABLE provider_models ADD COLUMN provider_options_json TEXT;
ALTER TABLE provider_models ADD COLUMN is_manual INTEGER NOT NULL DEFAULT 0;
