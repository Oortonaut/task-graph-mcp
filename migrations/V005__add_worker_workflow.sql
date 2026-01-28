-- Add workflow column to workers table
-- This tracks which named workflow file (workflow-{name}.yaml) the worker is using

ALTER TABLE workers ADD COLUMN workflow TEXT;
-- workflow column stores the name (e.g., "swarm" for workflow-swarm.yaml)
-- NULL means use the default workflows.yaml
