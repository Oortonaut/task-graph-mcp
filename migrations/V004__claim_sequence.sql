-- Migration V004: File claim tracking for coordination
-- Replaces pub/sub with simpler claim_updates mechanism

-- Create claim_sequence table for tracking file claim/release events
CREATE TABLE claim_sequence (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    file_path TEXT NOT NULL,
    agent_id TEXT NOT NULL,
    event TEXT NOT NULL,  -- 'claimed' or 'released'
    reason TEXT,          -- Optional reason for claim/release
    timestamp INTEGER NOT NULL
);

-- Index for efficient polling by files
CREATE INDEX idx_claim_sequence_file ON claim_sequence(file_path, id);

-- Add last_sequence column to agents for tracking poll position
ALTER TABLE agents ADD COLUMN last_claim_sequence INTEGER NOT NULL DEFAULT 0;

-- Add reason column to file_locks
ALTER TABLE file_locks ADD COLUMN reason TEXT;

-- Drop pub/sub tables (no longer needed)
DROP TABLE IF EXISTS inbox;
DROP TABLE IF EXISTS subscriptions;
