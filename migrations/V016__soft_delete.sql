-- Add soft delete support to tasks table
-- deleted_at stores timestamp when task was soft-deleted, NULL if not deleted
-- deleted_by stores agent/worker ID who deleted the task
-- deleted_reason stores optional reason for deletion

ALTER TABLE tasks ADD COLUMN deleted_at INTEGER;
ALTER TABLE tasks ADD COLUMN deleted_by TEXT;
ALTER TABLE tasks ADD COLUMN deleted_reason TEXT;

-- Index for efficient filtering of non-deleted tasks
CREATE INDEX idx_tasks_deleted ON tasks(deleted_at);
