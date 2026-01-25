-- Initial schema for Task Graph MCP Server

-- Workers (session-based)
CREATE TABLE workers (
    id TEXT PRIMARY KEY,
    name TEXT,
    tags TEXT,  -- JSON array of freeform tags
    max_claims INTEGER NOT NULL DEFAULT 5,
    registered_at INTEGER NOT NULL,
    last_heartbeat INTEGER NOT NULL
);

-- Tasks with hierarchy, estimation, tracking, and accounting
CREATE TABLE tasks (
    id TEXT PRIMARY KEY,
    parent_id TEXT REFERENCES tasks(id) ON DELETE CASCADE,
    title TEXT NOT NULL,
    description TEXT,
    status TEXT NOT NULL DEFAULT 'pending',
    priority TEXT NOT NULL DEFAULT 'medium',
    join_mode TEXT NOT NULL DEFAULT 'then',  -- 'then' or 'also'
    sibling_order INTEGER NOT NULL DEFAULT 0,  -- position among siblings
    owner_agent TEXT REFERENCES workers(id),
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

    metadata TEXT,  -- JSON
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);

-- Attachments (outputs, logs, artifacts)
CREATE TABLE attachments (
    id TEXT PRIMARY KEY,
    task_id TEXT NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    mime_type TEXT NOT NULL DEFAULT 'text/plain',
    content TEXT NOT NULL,  -- text or base64
    created_at INTEGER NOT NULL
);

-- Dependencies (DAG)
CREATE TABLE dependencies (
    from_task_id TEXT NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
    to_task_id TEXT NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
    PRIMARY KEY (from_task_id, to_task_id)
);

-- File locks (advisory)
CREATE TABLE file_locks (
    file_path TEXT PRIMARY KEY,
    worker_id TEXT NOT NULL REFERENCES workers(id),
    locked_at INTEGER NOT NULL
);

-- Subscriptions for pub/sub
CREATE TABLE subscriptions (
    id TEXT PRIMARY KEY,
    worker_id TEXT NOT NULL REFERENCES workers(id),
    target_type TEXT NOT NULL,  -- 'task', 'file', or 'agent'
    target_id TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    UNIQUE(worker_id, target_type, target_id)
);

-- Inbox for pub/sub messages
CREATE TABLE inbox (
    id TEXT PRIMARY KEY,
    worker_id TEXT NOT NULL REFERENCES workers(id),
    event_type TEXT NOT NULL,
    payload TEXT NOT NULL,  -- JSON
    created_at INTEGER NOT NULL,
    read INTEGER NOT NULL DEFAULT 0
);

-- Indexes
CREATE INDEX idx_tasks_parent ON tasks(parent_id);
CREATE INDEX idx_tasks_owner ON tasks(owner_agent);
CREATE INDEX idx_tasks_status ON tasks(status);
CREATE INDEX idx_inbox_worker ON inbox(worker_id, read, created_at);
CREATE INDEX idx_deps_to ON dependencies(to_task_id);
CREATE INDEX idx_file_locks_worker ON file_locks(worker_id);
CREATE INDEX idx_attachments_task ON attachments(task_id);
