-- Task Graph MCP Server - Initial Schema

-- Workers (session-based agents)
CREATE TABLE workers (
    id TEXT PRIMARY KEY,
    tags TEXT,                                    -- JSON array of capability tags
    max_claims INTEGER NOT NULL DEFAULT 5,
    registered_at INTEGER NOT NULL,
    last_heartbeat INTEGER NOT NULL,
    last_claim_sequence INTEGER NOT NULL DEFAULT 0
);

-- Tasks
CREATE TABLE tasks (
    id TEXT PRIMARY KEY,
    title TEXT NOT NULL,
    description TEXT,
    status TEXT NOT NULL DEFAULT 'pending',
    priority TEXT NOT NULL DEFAULT 'medium',
    worker_id TEXT REFERENCES workers(id),
    claimed_at INTEGER,

    -- Claim affinity (tag-based filtering)
    needed_tags TEXT,                             -- JSON array: worker must have ALL
    wanted_tags TEXT,                             -- JSON array: worker must have ANY

    -- Categorization
    tags TEXT DEFAULT '[]',                       -- JSON array for discovery/filtering

    -- Estimation & tracking
    points INTEGER,
    time_estimate_ms INTEGER,
    time_actual_ms INTEGER,
    started_at INTEGER,
    completed_at INTEGER,

    -- Live status
    current_thought TEXT,

    -- Cost accounting (8 generic metrics)
    metric_0 INTEGER NOT NULL DEFAULT 0,
    metric_1 INTEGER NOT NULL DEFAULT 0,
    metric_2 INTEGER NOT NULL DEFAULT 0,
    metric_3 INTEGER NOT NULL DEFAULT 0,
    metric_4 INTEGER NOT NULL DEFAULT 0,
    metric_5 INTEGER NOT NULL DEFAULT 0,
    metric_6 INTEGER NOT NULL DEFAULT 0,
    metric_7 INTEGER NOT NULL DEFAULT 0,
    cost_usd REAL NOT NULL DEFAULT 0.0,
    user_metrics TEXT,                            -- JSON object for custom metrics

    -- Soft delete
    deleted_at INTEGER,
    deleted_by TEXT,
    deleted_reason TEXT,

    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);

-- Attachments (outputs, logs, artifacts)
CREATE TABLE attachments (
    task_id TEXT NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
    order_index INTEGER NOT NULL,
    name TEXT NOT NULL,
    mime_type TEXT NOT NULL DEFAULT 'text/plain',
    content TEXT NOT NULL,                        -- text or base64
    file_path TEXT,                               -- if set, content is in media dir
    created_at INTEGER NOT NULL,
    PRIMARY KEY (task_id, order_index)
);

-- Dependencies (DAG with typed edges)
CREATE TABLE dependencies (
    from_task_id TEXT NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
    to_task_id TEXT NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
    dep_type TEXT NOT NULL DEFAULT 'blocks',      -- blocks, contains, follows
    PRIMARY KEY (from_task_id, to_task_id, dep_type)
);

-- File locks (advisory)
CREATE TABLE file_locks (
    file_path TEXT PRIMARY KEY,
    worker_id TEXT NOT NULL REFERENCES workers(id),
    task_id TEXT REFERENCES tasks(id),
    reason TEXT,
    locked_at INTEGER NOT NULL
);

-- File claim sequence (audit log for coordination)
CREATE TABLE claim_sequence (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    file_path TEXT NOT NULL,
    worker_id TEXT NOT NULL,
    event TEXT NOT NULL,                          -- 'claimed' or 'released'
    reason TEXT,
    claim_id INTEGER,                             -- for releases: ID of corresponding claim
    timestamp INTEGER NOT NULL,
    end_timestamp INTEGER                         -- when this claim period ended
);

-- Task state sequence (audit log for time tracking)
CREATE TABLE task_state_sequence (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    task_id TEXT NOT NULL,
    worker_id TEXT,
    event TEXT NOT NULL,                          -- target state
    reason TEXT,
    timestamp INTEGER NOT NULL,
    end_timestamp INTEGER
);

-- Junction tables for efficient tag queries
CREATE TABLE task_tags (
    task_id TEXT NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
    tag TEXT NOT NULL,
    PRIMARY KEY (task_id, tag)
);

CREATE TABLE task_needed_tags (
    task_id TEXT NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
    tag TEXT NOT NULL,
    PRIMARY KEY (task_id, tag)
);

CREATE TABLE task_wanted_tags (
    task_id TEXT NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
    tag TEXT NOT NULL,
    PRIMARY KEY (task_id, tag)
);

-- FTS5 full-text search for tasks
CREATE VIRTUAL TABLE tasks_fts USING fts5(
    task_id UNINDEXED,
    title,
    description
);

CREATE TRIGGER tasks_fts_insert AFTER INSERT ON tasks BEGIN
    INSERT INTO tasks_fts(task_id, title, description)
    VALUES (NEW.id, NEW.title, COALESCE(NEW.description, ''));
END;

CREATE TRIGGER tasks_fts_update AFTER UPDATE ON tasks BEGIN
    DELETE FROM tasks_fts WHERE task_id = OLD.id;
    INSERT INTO tasks_fts(task_id, title, description)
    VALUES (NEW.id, NEW.title, COALESCE(NEW.description, ''));
END;

CREATE TRIGGER tasks_fts_delete AFTER DELETE ON tasks BEGIN
    DELETE FROM tasks_fts WHERE task_id = OLD.id;
END;

-- FTS5 full-text search for attachments
CREATE VIRTUAL TABLE attachments_fts USING fts5(
    task_id UNINDEXED,
    order_index UNINDEXED,
    name,
    content
);

CREATE TRIGGER attachments_fts_insert AFTER INSERT ON attachments
WHEN NEW.mime_type LIKE 'text/%' BEGIN
    INSERT INTO attachments_fts(task_id, order_index, name, content)
    VALUES (NEW.task_id, NEW.order_index, NEW.name, NEW.content);
END;

CREATE TRIGGER attachments_fts_update AFTER UPDATE ON attachments BEGIN
    DELETE FROM attachments_fts WHERE task_id = OLD.task_id AND order_index = OLD.order_index;
    INSERT INTO attachments_fts(task_id, order_index, name, content)
    SELECT NEW.task_id, NEW.order_index, NEW.name, NEW.content
    WHERE NEW.mime_type LIKE 'text/%';
END;

CREATE TRIGGER attachments_fts_delete AFTER DELETE ON attachments BEGIN
    DELETE FROM attachments_fts WHERE task_id = OLD.task_id AND order_index = OLD.order_index;
END;

-- Indexes
CREATE INDEX idx_tasks_worker ON tasks(worker_id);
CREATE INDEX idx_tasks_worker_status ON tasks(worker_id, status);
CREATE INDEX idx_tasks_status ON tasks(status);
CREATE INDEX idx_tasks_claimed ON tasks(claimed_at) WHERE worker_id IS NOT NULL;
CREATE INDEX idx_tasks_deleted ON tasks(deleted_at);

CREATE INDEX idx_attachments_task ON attachments(task_id);

CREATE INDEX idx_deps_to ON dependencies(to_task_id);
CREATE INDEX idx_deps_from ON dependencies(from_task_id);
CREATE INDEX idx_deps_type ON dependencies(dep_type);
CREATE INDEX idx_deps_type_to ON dependencies(dep_type, to_task_id);
CREATE INDEX idx_deps_from_type ON dependencies(from_task_id, dep_type);

CREATE INDEX idx_file_locks_worker ON file_locks(worker_id);
CREATE INDEX idx_file_locks_task ON file_locks(task_id);

CREATE INDEX idx_claim_sequence_file ON claim_sequence(file_path, id);
CREATE INDEX idx_claim_seq_file_worker ON claim_sequence(file_path, worker_id);
CREATE INDEX idx_claim_seq_open ON claim_sequence(file_path) WHERE end_timestamp IS NULL;

CREATE INDEX idx_task_state_seq_task ON task_state_sequence(task_id, id);
CREATE INDEX idx_task_state_seq_open ON task_state_sequence(task_id) WHERE end_timestamp IS NULL;

CREATE INDEX idx_task_tags_tag ON task_tags(tag);
CREATE INDEX idx_task_needed_tags_tag ON task_needed_tags(tag);
CREATE INDEX idx_task_wanted_tags_tag ON task_wanted_tags(tag);

CREATE INDEX idx_workers_heartbeat ON workers(last_heartbeat);
