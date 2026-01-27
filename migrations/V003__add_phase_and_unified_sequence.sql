-- V003: Add phase field to tasks and create unified task_sequence table
--
-- Phase tracks the type of work being done (explore, design, implement, test, etc.)
-- The unified task_sequence table combines status and phase change tracking
-- into a single timeline for easier querying and reporting.

-- Add phase column to tasks
ALTER TABLE tasks ADD COLUMN phase TEXT;

-- Create the unified task_sequence table (replaces task_state_sequence)
-- Uses snapshot pattern: each row records the new status/phase values.
-- Previous values can be found by querying the previous row for the same task.
CREATE TABLE task_sequence (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    task_id TEXT NOT NULL,
    worker_id TEXT,
    -- Snapshot fields (NULL if unchanged from previous row)
    status TEXT,
    phase TEXT,
    -- Common fields
    reason TEXT,
    timestamp INTEGER NOT NULL,
    end_timestamp INTEGER,
    FOREIGN KEY (task_id) REFERENCES tasks(id) ON DELETE CASCADE
);

-- Migrate existing status transitions from task_state_sequence
INSERT INTO task_sequence (id, task_id, worker_id, status, phase, reason, timestamp, end_timestamp)
SELECT id, task_id, worker_id,
       event,  -- status (event column held the status value)
       NULL,   -- phase (not tracked in old schema)
       reason,
       timestamp,
       end_timestamp
FROM task_state_sequence;

-- Drop the old table
DROP TABLE IF EXISTS task_state_sequence;

-- Create indexes for the unified table
-- Note: composite (task_id, timestamp) covers task_id-only queries
CREATE INDEX idx_task_seq_task_timestamp ON task_sequence(task_id, timestamp);
CREATE INDEX idx_task_seq_timestamp ON task_sequence(timestamp);
CREATE INDEX idx_task_seq_status ON task_sequence(status) WHERE status IS NOT NULL;
CREATE INDEX idx_task_seq_phase ON task_sequence(phase) WHERE phase IS NOT NULL;

-- Add indexes for phase filtering on tasks
CREATE INDEX idx_tasks_phase ON tasks(phase);
CREATE INDEX idx_tasks_phase_status ON tasks(phase, status);

-- Rename 'in_progress' status to 'working'
UPDATE tasks SET status = 'working' WHERE status = 'in_progress';
UPDATE task_sequence SET status = 'working' WHERE status = 'in_progress';
