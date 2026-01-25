-- FTS5 full-text search virtual table for tasks
-- Indexes task titles and descriptions for fast text search

-- Create FTS5 virtual table for tasks
CREATE VIRTUAL TABLE tasks_fts USING fts5(
    task_id UNINDEXED,  -- Store task_id but don't index it for search
    title,
    description
);

-- Populate FTS table with existing tasks
INSERT INTO tasks_fts(task_id, title, description)
SELECT id, title, COALESCE(description, '') FROM tasks;

-- Trigger: Keep FTS in sync on INSERT
CREATE TRIGGER tasks_fts_insert AFTER INSERT ON tasks BEGIN
    INSERT INTO tasks_fts(task_id, title, description)
    VALUES (NEW.id, NEW.title, COALESCE(NEW.description, ''));
END;

-- Trigger: Keep FTS in sync on UPDATE
CREATE TRIGGER tasks_fts_update AFTER UPDATE ON tasks BEGIN
    DELETE FROM tasks_fts WHERE task_id = OLD.id;
    INSERT INTO tasks_fts(task_id, title, description)
    VALUES (NEW.id, NEW.title, COALESCE(NEW.description, ''));
END;

-- Trigger: Keep FTS in sync on DELETE
CREATE TRIGGER tasks_fts_delete AFTER DELETE ON tasks BEGIN
    DELETE FROM tasks_fts WHERE task_id = OLD.id;
END;

-- FTS5 table for attachment content search
CREATE VIRTUAL TABLE attachments_fts USING fts5(
    attachment_id UNINDEXED,
    task_id UNINDEXED,
    name,
    content
);

-- Populate FTS table with existing text attachments (skip binary/base64)
INSERT INTO attachments_fts(attachment_id, task_id, name, content)
SELECT id, task_id, name, content
FROM attachments
WHERE mime_type LIKE 'text/%';

-- Trigger: Keep attachment FTS in sync on INSERT
CREATE TRIGGER attachments_fts_insert AFTER INSERT ON attachments
WHEN NEW.mime_type LIKE 'text/%' BEGIN
    INSERT INTO attachments_fts(attachment_id, task_id, name, content)
    VALUES (NEW.id, NEW.task_id, NEW.name, NEW.content);
END;

-- Trigger: Keep attachment FTS in sync on UPDATE
CREATE TRIGGER attachments_fts_update AFTER UPDATE ON attachments BEGIN
    DELETE FROM attachments_fts WHERE attachment_id = OLD.id;
    INSERT INTO attachments_fts(attachment_id, task_id, name, content)
    SELECT NEW.id, NEW.task_id, NEW.name, NEW.content
    WHERE NEW.mime_type LIKE 'text/%';
END;

-- Trigger: Keep attachment FTS in sync on DELETE
CREATE TRIGGER attachments_fts_delete AFTER DELETE ON attachments BEGIN
    DELETE FROM attachments_fts WHERE attachment_id = OLD.id;
END;
