-- Provider model sort order v3
-- Date: 2026-03-19
-- Description: Persist provider-returned model order for Settings Provider UI sorting.

ALTER TABLE provider_models ADD COLUMN sort_index INTEGER NOT NULL DEFAULT 0;
