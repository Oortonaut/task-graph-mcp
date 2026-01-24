-- Migration V006: Task state transition tracking
-- Enables automatic time tracking by recording all state transitions

-- Task state transition tracking (append-only audit log)
CREATE TABLE task_state_sequence (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    task_id TEXT NOT NULL,
    agent_id TEXT,
    event TEXT NOT NULL,          -- Target state: pending, in_progress, completed, etc.
    reason TEXT,
    timestamp INTEGER NOT NULL,
    end_timestamp INTEGER
);

CREATE INDEX idx_task_state_seq_task ON task_state_sequence(task_id, id);
CREATE INDEX idx_task_state_seq_open ON task_state_sequence(task_id)
    WHERE end_timestamp IS NULL;

-- Add end_timestamp to file claim_sequence for duration tracking
ALTER TABLE claim_sequence ADD COLUMN end_timestamp INTEGER;
CREATE INDEX idx_claim_seq_open ON claim_sequence(file_path)
    WHERE end_timestamp IS NULL;
