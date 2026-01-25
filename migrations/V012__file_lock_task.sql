-- Add task_id column to file_locks for auto-cleanup when tasks complete
ALTER TABLE file_locks ADD COLUMN task_id TEXT REFERENCES tasks(id);

-- Index for efficient task-based cleanup
CREATE INDEX idx_file_locks_task ON file_locks(task_id);
