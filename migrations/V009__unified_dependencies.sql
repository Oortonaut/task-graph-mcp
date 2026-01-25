-- Migration V009: Unified dependency system
-- Consolidates parent_id, sibling_order, and join_mode into typed dependencies

-- Step 1: Add dep_type column to dependencies table
ALTER TABLE dependencies ADD COLUMN dep_type TEXT NOT NULL DEFAULT 'blocks';

-- Step 2: Add new indexes for query patterns
CREATE INDEX IF NOT EXISTS idx_deps_type ON dependencies(dep_type);
CREATE INDEX IF NOT EXISTS idx_deps_type_to ON dependencies(dep_type, to_task_id);
CREATE INDEX IF NOT EXISTS idx_tasks_claimed ON tasks(claimed_at) WHERE owner_agent IS NOT NULL;
CREATE INDEX IF NOT EXISTS idx_agents_heartbeat ON agents(last_heartbeat);

-- Step 3: Migrate parent_id relationships to 'contains' edges
-- from_task_id = parent, to_task_id = child (parent contains child)
INSERT OR IGNORE INTO dependencies (from_task_id, to_task_id, dep_type)
SELECT parent_id, id, 'contains'
FROM tasks WHERE parent_id IS NOT NULL;

-- Step 4: Migrate join_mode='then' siblings to 'follows' edges
-- A task with join_mode='then' depends on the previous sibling completing
-- from_task_id = previous sibling, to_task_id = current task (current follows previous)
INSERT OR IGNORE INTO dependencies (from_task_id, to_task_id, dep_type)
SELECT prev.id, curr.id, 'follows'
FROM tasks curr
JOIN tasks prev ON (curr.parent_id IS NOT DISTINCT FROM prev.parent_id 
                    OR (curr.parent_id IS NULL AND prev.parent_id IS NULL))
WHERE curr.join_mode = 'then'
  AND prev.sibling_order = curr.sibling_order - 1
  AND curr.sibling_order > 0;

-- Step 5: Recreate tasks table without parent_id, sibling_order, join_mode
-- SQLite requires table recreation for column removal

-- Create new tasks table without the removed columns
CREATE TABLE tasks_new (
    id TEXT PRIMARY KEY,
    title TEXT NOT NULL,
    description TEXT,
    status TEXT NOT NULL DEFAULT 'pending',
    priority TEXT NOT NULL DEFAULT 'medium',
    owner_agent TEXT REFERENCES agents(id),
    claimed_at INTEGER,

    -- Affinity (tag-based)
    needed_tags TEXT,  -- JSON array, agent must have ALL (AND)
    wanted_tags TEXT,  -- JSON array, agent must have AT LEAST ONE (OR)

    -- Estimation & tracking
    points INTEGER,
    time_estimate_ms INTEGER,
    time_actual_ms INTEGER,
    started_at INTEGER,
    completed_at INTEGER,

    -- Live status
    current_thought TEXT,

    -- Cost accounting (fixed categories)
    tokens_in INTEGER NOT NULL DEFAULT 0,
    tokens_cached INTEGER NOT NULL DEFAULT 0,
    tokens_out INTEGER NOT NULL DEFAULT 0,
    tokens_thinking INTEGER NOT NULL DEFAULT 0,
    tokens_image INTEGER NOT NULL DEFAULT 0,
    tokens_audio INTEGER NOT NULL DEFAULT 0,
    cost_usd REAL NOT NULL DEFAULT 0.0,
    user_metrics TEXT,  -- JSON object for custom metrics

    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);

-- Copy data from old table (excluding removed columns)
INSERT INTO tasks_new (
    id, title, description, status, priority, owner_agent, claimed_at,
    needed_tags, wanted_tags, points, time_estimate_ms, time_actual_ms,
    started_at, completed_at, current_thought,
    tokens_in, tokens_cached, tokens_out, tokens_thinking, tokens_image, tokens_audio,
    cost_usd, user_metrics, created_at, updated_at
)
SELECT
    id, title, description, status, priority, owner_agent, claimed_at,
    needed_tags, wanted_tags, points, time_estimate_ms, time_actual_ms,
    started_at, completed_at, current_thought,
    tokens_in, tokens_cached, tokens_out, tokens_thinking, tokens_image, tokens_audio,
    cost_usd, user_metrics, created_at, updated_at
FROM tasks;

-- Drop old table and rename new one
DROP TABLE tasks;
ALTER TABLE tasks_new RENAME TO tasks;

-- Step 6: Recreate task indexes
CREATE INDEX idx_tasks_owner ON tasks(owner_agent);
CREATE INDEX idx_tasks_status ON tasks(status);
CREATE INDEX idx_tasks_owner_status ON tasks(owner_agent, status);
CREATE INDEX idx_tasks_claimed ON tasks(claimed_at) WHERE owner_agent IS NOT NULL;

-- Update dependencies primary key to include dep_type
-- (need to recreate table since SQLite can't modify primary key)
CREATE TABLE dependencies_new (
    from_task_id TEXT NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
    to_task_id TEXT NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
    dep_type TEXT NOT NULL DEFAULT 'blocks',
    PRIMARY KEY (from_task_id, to_task_id, dep_type)
);

INSERT INTO dependencies_new (from_task_id, to_task_id, dep_type)
SELECT from_task_id, to_task_id, dep_type FROM dependencies;

DROP TABLE dependencies;
ALTER TABLE dependencies_new RENAME TO dependencies;

-- Recreate dependency indexes
CREATE INDEX idx_deps_to ON dependencies(to_task_id);
CREATE INDEX idx_deps_from ON dependencies(from_task_id);
CREATE INDEX idx_deps_type ON dependencies(dep_type);
CREATE INDEX idx_deps_type_to ON dependencies(dep_type, to_task_id);
