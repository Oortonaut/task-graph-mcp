-- Migration V018: Junction tables for efficient tag-based queries
--
-- Replaces JSON array columns with proper relational tables that can be indexed.
-- This enables fast lookups like "find highest priority task with tag X that agent Y can claim"

-- Task categorization tags (for filtering: "show me all 'urgent' tasks")
CREATE TABLE task_tags (
    task_id TEXT NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
    tag TEXT NOT NULL,
    PRIMARY KEY (task_id, tag)
);
CREATE INDEX idx_task_tags_tag ON task_tags(tag);

-- Agent requirement: ALL (agent must have ALL of these tags to claim)
CREATE TABLE task_needed_tags (
    task_id TEXT NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
    tag TEXT NOT NULL,
    PRIMARY KEY (task_id, tag)
);
CREATE INDEX idx_task_needed_tags_tag ON task_needed_tags(tag);

-- Agent requirement: ANY (agent must have at least ONE of these tags to claim)  
CREATE TABLE task_wanted_tags (
    task_id TEXT NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
    tag TEXT NOT NULL,
    PRIMARY KEY (task_id, tag)
);
CREATE INDEX idx_task_wanted_tags_tag ON task_wanted_tags(tag);

-- Migrate existing JSON data to junction tables
INSERT OR IGNORE INTO task_tags (task_id, tag)
SELECT t.id, j.value
FROM tasks t, json_each(t.tags) j
WHERE t.tags IS NOT NULL AND t.tags != '[]';

INSERT OR IGNORE INTO task_needed_tags (task_id, tag)
SELECT t.id, j.value
FROM tasks t, json_each(t.agent_tags_all) j
WHERE t.agent_tags_all IS NOT NULL AND t.agent_tags_all != '[]';

INSERT OR IGNORE INTO task_wanted_tags (task_id, tag)
SELECT t.id, j.value
FROM tasks t, json_each(t.agent_tags_any) j
WHERE t.agent_tags_any IS NOT NULL AND t.agent_tags_any != '[]';

-- Drop the now-unused JSON index
DROP INDEX IF EXISTS idx_tasks_tags;
