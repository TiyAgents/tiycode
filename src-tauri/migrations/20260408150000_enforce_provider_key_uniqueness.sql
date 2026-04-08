-- Provider key uniqueness enforcement
-- Date: 2026-04-08
-- Description: Add unique constraint on provider_key to prevent duplicates.
--              First, clean up any duplicate providers by keeping only the one
--              with the most recent updated_at timestamp (or ID as tiebreaker).

-- Step 1: Identify and delete duplicate providers (keep the most recent one)
-- We use a CTE to find duplicates, keeping the most recent by updated_at, then by ID
DELETE FROM providers WHERE id IN (
    WITH ranked_providers AS (
        SELECT 
            id,
            ROW_NUMBER() OVER (
                PARTITION BY provider_key 
                ORDER BY updated_at DESC, id DESC
            ) as rn
        FROM providers
    )
    SELECT id FROM ranked_providers WHERE rn > 1
);

-- Step 2: Add unique constraint on provider_key
-- SQLite doesn't support ALTER TABLE ADD CONSTRAINT, so we use CREATE UNIQUE INDEX
CREATE UNIQUE INDEX IF NOT EXISTS idx_providers_key_unique ON providers(provider_key);
