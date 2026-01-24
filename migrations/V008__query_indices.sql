-- Migration V008: Add indices for common query patterns

-- Speed up queries filtering by owner and status together
CREATE INDEX idx_tasks_owner_status ON tasks(owner_agent, status);

-- Speed up queries filtering by status alone (list_tasks with status filter)
CREATE INDEX idx_tasks_status ON tasks(status);

-- Speed up dependency lookups (get_blockers, cycle detection)
CREATE INDEX idx_deps_from ON dependencies(from_task_id);
