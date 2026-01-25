-- Migration V010: Add tags column for task categorization/discovery
-- This is separate from needed_tags/wanted_tags which are for claim-time requirements

-- Add tags column for categorization/discovery
ALTER TABLE tasks ADD COLUMN tags TEXT DEFAULT '[]';

-- Add index for tag-based queries
CREATE INDEX IF NOT EXISTS idx_tasks_tags ON tasks(tags);
