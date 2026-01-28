-- V004: Split attachment type and name
--
-- Changes the attachment primary key from (task_id, order_index) to (task_id, attachment_type, sequence).
-- - attachment_type: the category (e.g., "commit", "note", "changelist")
-- - name: arbitrary label for the attachment
-- - sequence: auto-increment per (task_id, attachment_type)
--
-- Replace operations now delete by (task_id, attachment_type) rather than exact name match.

-- Create new attachments table with the new schema
CREATE TABLE attachments_new (
    task_id TEXT NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
    attachment_type TEXT NOT NULL,
    sequence INTEGER NOT NULL,
    name TEXT NOT NULL DEFAULT '',
    mime_type TEXT NOT NULL DEFAULT 'text/plain',
    content TEXT NOT NULL,
    file_path TEXT,
    created_at INTEGER NOT NULL,
    PRIMARY KEY (task_id, attachment_type, sequence)
);

-- Migrate existing data: use existing 'name' as 'attachment_type', keep sequence from order_index
INSERT INTO attachments_new (task_id, attachment_type, sequence, name, mime_type, content, file_path, created_at)
SELECT task_id, name, order_index, '', mime_type, content, file_path, created_at
FROM attachments;

-- Drop old table and rename
DROP TABLE attachments;
ALTER TABLE attachments_new RENAME TO attachments;

-- Recreate FTS table with new schema
DROP TABLE IF EXISTS attachments_fts;
CREATE VIRTUAL TABLE attachments_fts USING fts5(
    task_id UNINDEXED,
    attachment_type UNINDEXED,
    sequence UNINDEXED,
    name,
    content
);

-- Populate FTS from migrated data
INSERT INTO attachments_fts(task_id, attachment_type, sequence, name, content)
SELECT task_id, attachment_type, sequence, name, content
FROM attachments
WHERE mime_type LIKE 'text/%';

-- Recreate triggers for FTS
DROP TRIGGER IF EXISTS attachments_fts_insert;
DROP TRIGGER IF EXISTS attachments_fts_update;
DROP TRIGGER IF EXISTS attachments_fts_delete;

CREATE TRIGGER attachments_fts_insert AFTER INSERT ON attachments
WHEN NEW.mime_type LIKE 'text/%' BEGIN
    INSERT INTO attachments_fts(task_id, attachment_type, sequence, name, content)
    VALUES (NEW.task_id, NEW.attachment_type, NEW.sequence, NEW.name, NEW.content);
END;

CREATE TRIGGER attachments_fts_update AFTER UPDATE ON attachments BEGIN
    DELETE FROM attachments_fts WHERE task_id = OLD.task_id AND attachment_type = OLD.attachment_type AND sequence = OLD.sequence;
    INSERT INTO attachments_fts(task_id, attachment_type, sequence, name, content)
    SELECT NEW.task_id, NEW.attachment_type, NEW.sequence, NEW.name, NEW.content
    WHERE NEW.mime_type LIKE 'text/%';
END;

CREATE TRIGGER attachments_fts_delete AFTER DELETE ON attachments BEGIN
    DELETE FROM attachments_fts WHERE task_id = OLD.task_id AND attachment_type = OLD.attachment_type AND sequence = OLD.sequence;
END;

-- Recreate indexes
DROP INDEX IF EXISTS idx_attachments_task;
CREATE INDEX idx_attachments_task ON attachments(task_id);
CREATE INDEX idx_attachments_task_type ON attachments(task_id, attachment_type);
