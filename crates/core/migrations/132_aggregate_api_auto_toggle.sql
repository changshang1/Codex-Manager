ALTER TABLE aggregate_apis
  ADD COLUMN auto_toggle_enabled INTEGER NOT NULL DEFAULT 0;

ALTER TABLE aggregate_apis
  ADD COLUMN consecutive_failures INTEGER NOT NULL DEFAULT 0;

ALTER TABLE aggregate_apis
  ADD COLUMN auto_disabled INTEGER NOT NULL DEFAULT 0;

ALTER TABLE aggregate_apis
  ADD COLUMN auto_disabled_at INTEGER;

ALTER TABLE aggregate_apis
  ADD COLUMN auto_disabled_reason TEXT;
