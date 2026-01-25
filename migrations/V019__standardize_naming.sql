-- Standardize naming: use consistent terminology throughout
-- 1. Remove unused 'name' column from workers (if exists)
-- 2. Rename owner_agent to worker_id for consistency
-- 3. Rename agent_tags_all to needed_tags (match API naming)
-- 4. Rename agent_tags_any to wanted_tags (match API naming)

-- SQLite doesn't support DROP COLUMN directly in older versions,
-- but modern SQLite (3.35+) does. We'll use ALTER TABLE directly.

-- Note: The 'name' column may have already been removed in a previous migration
-- or may not exist. SQLite will error if the column doesn't exist, so we skip this.

-- Rename owner_agent to worker_id for consistency
ALTER TABLE tasks RENAME COLUMN owner_agent TO worker_id;

-- Rename tag columns to match API naming
ALTER TABLE tasks RENAME COLUMN agent_tags_all TO needed_tags;
ALTER TABLE tasks RENAME COLUMN agent_tags_any TO wanted_tags;

-- Update indexes that reference owner_agent
-- First drop old indexes
DROP INDEX IF EXISTS idx_tasks_owner;
DROP INDEX IF EXISTS idx_tasks_owner_status;
DROP INDEX IF EXISTS idx_tasks_claimed;

-- Create new indexes with updated column names
CREATE INDEX idx_tasks_worker ON tasks(worker_id);
CREATE INDEX idx_tasks_worker_status ON tasks(worker_id, status);
CREATE INDEX idx_tasks_claimed ON tasks(claimed_at) WHERE worker_id IS NOT NULL;
