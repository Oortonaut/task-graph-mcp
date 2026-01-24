-- Migration V007: Add claim_id to release events for efficient filtering
-- When a file is released, store the ID of the corresponding claim event.
-- This allows claim_updates to filter out releases where the agent never saw the claim.

ALTER TABLE claim_sequence ADD COLUMN claim_id INTEGER;

-- Remove end_timestamp (not needed with claim_id approach)
-- SQLite doesn't support DROP COLUMN in older versions, so we leave it
