ALTER TABLE marketplace_sources
  ADD COLUMN last_successful_sync_id INTEGER NOT NULL DEFAULT 0;

ALTER TABLE marketplace_sources
  ADD COLUMN last_sync_at INTEGER;

ALTER TABLE marketplace_sources
  ADD COLUMN last_sync_error TEXT;

ALTER TABLE marketplace_offers
  ADD COLUMN freshness_status TEXT;

ALTER TABLE marketplace_offers
  ADD COLUMN expires_at TEXT;

ALTER TABLE marketplace_offers
  ADD COLUMN last_seen_sync_id INTEGER NOT NULL DEFAULT 0;

UPDATE marketplace_offers
SET local_status = 'unknown',
    local_checked_at = NULL,
    local_error = NULL;

CREATE INDEX IF NOT EXISTS idx_marketplace_offers_snapshot
  ON marketplace_offers(source_config_id, last_seen_sync_id);
