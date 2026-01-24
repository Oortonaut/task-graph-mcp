-- Migration V003: Composite primary key for attachments
-- Changes attachment primary key from UUID to composite (task_id, order_index)

-- Recreate attachments table with composite key
CREATE TABLE attachments_new (
    task_id TEXT NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
    order_index INTEGER NOT NULL,
    name TEXT NOT NULL,
    mime_type TEXT NOT NULL DEFAULT 'text/plain',
    content TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    PRIMARY KEY (task_id, order_index)
);

-- Copy existing data
INSERT INTO attachments_new (task_id, order_index, name, mime_type, content, created_at)
SELECT task_id, order_index, name, mime_type, content, created_at FROM attachments;

-- Drop old table
DROP TABLE attachments;

-- Rename new table
ALTER TABLE attachments_new RENAME TO attachments;

-- Recreate index
CREATE INDEX idx_attachments_task ON attachments(task_id);
