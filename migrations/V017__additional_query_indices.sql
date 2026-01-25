-- Migration V017: Additional indices for query optimization
--
-- Identified slow query patterns:
-- 1. Dependency queries filtering by (from_task_id, dep_type)
-- 2. Claim sequence lookups by (file_path, worker_id)

-- Index for dependency queries that filter by from_task_id and dep_type.
-- Used in: get_completion_blockers, would_create_cycle, propagate_unblock_effects
-- These queries filter by from_task_id = ? AND dep_type IN (...)
-- The existing idx_deps_from only covers from_task_id, requiring additional
-- filtering through the result set for dep_type.
CREATE INDEX IF NOT EXISTS idx_deps_from_type ON dependencies(from_task_id, dep_type);

-- Index for claim_sequence queries during unlock operations.
-- Used in: unlock_file, unlock_files_verbose
-- Query: SELECT MAX(id) FROM claim_sequence WHERE file_path = ? AND worker_id = ? AND event = 'claimed'
-- The existing idx_claim_sequence_file on (file_path, id) doesn't cover worker_id,
-- requiring a scan through all events for a file to find ones for a specific worker.
CREATE INDEX IF NOT EXISTS idx_claim_seq_file_worker ON claim_sequence(file_path, worker_id);
