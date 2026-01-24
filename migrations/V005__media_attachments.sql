-- Migration V005: Add file_path support for reference attachments
-- Attachments can now reference files stored in .task-graph/media/ directory

-- Add file_path column (nullable - null means content is inline, set means content is in file)
ALTER TABLE attachments ADD COLUMN file_path TEXT;
