-- Replace individual token fields with 8 generic metric columns
-- metric_0..metric_7 are integer counters that get aggregated

-- Add new metric columns
ALTER TABLE tasks ADD COLUMN metric_0 INTEGER NOT NULL DEFAULT 0;
ALTER TABLE tasks ADD COLUMN metric_1 INTEGER NOT NULL DEFAULT 0;
ALTER TABLE tasks ADD COLUMN metric_2 INTEGER NOT NULL DEFAULT 0;
ALTER TABLE tasks ADD COLUMN metric_3 INTEGER NOT NULL DEFAULT 0;
ALTER TABLE tasks ADD COLUMN metric_4 INTEGER NOT NULL DEFAULT 0;
ALTER TABLE tasks ADD COLUMN metric_5 INTEGER NOT NULL DEFAULT 0;
ALTER TABLE tasks ADD COLUMN metric_6 INTEGER NOT NULL DEFAULT 0;
ALTER TABLE tasks ADD COLUMN metric_7 INTEGER NOT NULL DEFAULT 0;

-- Migrate existing token data to new columns:
-- metric_0 = tokens_in
-- metric_1 = tokens_cached
-- metric_2 = tokens_out
-- metric_3 = tokens_thinking
-- metric_4 = tokens_image
-- metric_5 = tokens_audio
-- metric_6, metric_7 = reserved (0)
UPDATE tasks SET
    metric_0 = tokens_in,
    metric_1 = tokens_cached,
    metric_2 = tokens_out,
    metric_3 = tokens_thinking,
    metric_4 = tokens_image,
    metric_5 = tokens_audio;

-- Note: SQLite doesn't support DROP COLUMN in older versions,
-- so we keep the old columns but they become unused.
-- In a production system with SQLite 3.35+, you could drop them:
-- ALTER TABLE tasks DROP COLUMN tokens_in;
-- etc.
