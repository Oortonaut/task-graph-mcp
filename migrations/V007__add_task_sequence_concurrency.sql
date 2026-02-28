-- Add concurrency column to task_sequence table
-- Records how many timed tasks the same worker had open simultaneously
-- at the time of this transition. Used to normalize time_actual_ms
-- for parallel work (e.g., 3 tasks in "working" for 1 hour each
-- would each record concurrency=3, allowing normalization to 20min each).

ALTER TABLE task_sequence ADD COLUMN concurrency INTEGER DEFAULT 1;
