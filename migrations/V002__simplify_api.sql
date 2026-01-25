-- API Simplification Migration
-- 1. Remove metadata column from tasks (use attachments instead)
-- 2. Add order_index to attachments for auto-increment ordering

-- SQLite doesn't support DROP COLUMN easily, so we recreate the table
-- First, create a new tasks table without metadata

CREATE TABLE tasks_new (
    id TEXT PRIMARY KEY,
    parent_id TEXT REFERENCES tasks_new(id) ON DELETE CASCADE,
    title TEXT NOT NULL,
    description TEXT,
    status TEXT NOT NULL DEFAULT 'pending',
    priority TEXT NOT NULL DEFAULT 'medium',
    join_mode TEXT NOT NULL DEFAULT 'then',
    sibling_order INTEGER NOT NULL DEFAULT 0,
    owner_agent TEXT REFERENCES workers(id),
    claimed_at INTEGER,

    -- Affinity (tag-based)
    needed_tags TEXT,
    wanted_tags TEXT,

    -- Estimation & tracking
    points INTEGER,
    time_estimate_ms INTEGER,
    time_actual_ms INTEGER,
    started_at INTEGER,
    completed_at INTEGER,

    -- Live status
    current_thought TEXT,

    -- Cost accounting
    tokens_in INTEGER NOT NULL DEFAULT 0,
    tokens_cached INTEGER NOT NULL DEFAULT 0,
    tokens_out INTEGER NOT NULL DEFAULT 0,
    tokens_thinking INTEGER NOT NULL DEFAULT 0,
    tokens_image INTEGER NOT NULL DEFAULT 0,
    tokens_audio INTEGER NOT NULL DEFAULT 0,
    cost_usd REAL NOT NULL DEFAULT 0.0,
    user_metrics TEXT,

    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);

-- Migrate existing metadata to attachments before dropping
INSERT INTO attachments (id, task_id, name, mime_type, content, created_at)
SELECT 
    lower(hex(randomblob(4)) || '-' || hex(randomblob(2)) || '-4' || substr(hex(randomblob(2)),2) || '-' || substr('89ab',abs(random()) % 4 + 1, 1) || substr(hex(randomblob(2)),2) || '-' || hex(randomblob(6))),
    id,
    'meta',
    'application/json',
    metadata,
    created_at
FROM tasks
WHERE metadata IS NOT NULL AND metadata != 'null';

-- Copy data to new table (excluding metadata)
INSERT INTO tasks_new (
    id, parent_id, title, description, status, priority, join_mode, sibling_order,
    owner_agent, claimed_at, needed_tags, wanted_tags, points, time_estimate_ms,
    time_actual_ms, started_at, completed_at, current_thought, tokens_in, tokens_cached,
    tokens_out, tokens_thinking, tokens_image, tokens_audio, cost_usd, user_metrics,
    created_at, updated_at
)
SELECT 
    id, parent_id, title, description, status, priority, join_mode, sibling_order,
    owner_agent, claimed_at, needed_tags, wanted_tags, points, time_estimate_ms,
    time_actual_ms, started_at, completed_at, current_thought, tokens_in, tokens_cached,
    tokens_out, tokens_thinking, tokens_image, tokens_audio, cost_usd, user_metrics,
    created_at, updated_at
FROM tasks;

-- Drop old table and rename new one
DROP TABLE tasks;
ALTER TABLE tasks_new RENAME TO tasks;

-- Recreate indexes
CREATE INDEX idx_tasks_parent ON tasks(parent_id);
CREATE INDEX idx_tasks_owner ON tasks(owner_agent);
CREATE INDEX idx_tasks_status ON tasks(status);

-- Add order_index to attachments
ALTER TABLE attachments ADD COLUMN order_index INTEGER NOT NULL DEFAULT 0;
