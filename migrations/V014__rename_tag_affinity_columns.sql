-- Rename tag affinity columns for clarity:
-- needed_tags (AND logic) -> agent_tags_all
-- wanted_tags (OR logic) -> agent_tags_any

ALTER TABLE tasks RENAME COLUMN needed_tags TO agent_tags_all;
ALTER TABLE tasks RENAME COLUMN wanted_tags TO agent_tags_any;
