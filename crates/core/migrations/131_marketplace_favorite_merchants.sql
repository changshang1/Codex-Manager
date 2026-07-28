CREATE TABLE IF NOT EXISTS marketplace_favorite_merchants (
  merchant_key TEXT PRIMARY KEY,
  source_id TEXT,
  source_name TEXT,
  collector_kind TEXT,
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_marketplace_favorite_merchants_updated
  ON marketplace_favorite_merchants(updated_at DESC);
