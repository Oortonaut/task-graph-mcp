-- Track which task the worker last transitioned on
ALTER TABLE workers ADD COLUMN last_task_id TEXT;
