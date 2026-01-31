-- Add overlays column to workers table
-- This tracks which overlay files (overlay-{name}.yaml) are applied to the worker's workflow
-- Stored as a JSON array of overlay names (e.g., '["git","user-request"]')
-- NULL means no overlays applied

ALTER TABLE workers ADD COLUMN overlays TEXT;
