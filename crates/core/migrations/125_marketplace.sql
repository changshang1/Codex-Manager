CREATE TABLE IF NOT EXISTS marketplace_sources (
  id TEXT PRIMARY KEY,
  product_id TEXT NOT NULL,
  tags_json TEXT NOT NULL DEFAULT '[]',
  merchant TEXT,
  enabled INTEGER NOT NULL DEFAULT 1,
  verify_enabled INTEGER NOT NULL DEFAULT 1,
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS marketplace_offers (
  offer_key TEXT PRIMARY KEY,
  source_config_id TEXT NOT NULL,
  offer_id TEXT NOT NULL,
  product_id TEXT NOT NULL,
  source_id TEXT,
  source_name TEXT,
  collector_kind TEXT,
  title TEXT,
  price REAL,
  listed_price REAL,
  currency TEXT NOT NULL DEFAULT 'CNY',
  raw_status TEXT,
  effective_status TEXT,
  url TEXT,
  tags_json TEXT NOT NULL DEFAULT '[]',
  filter_tags_json TEXT NOT NULL DEFAULT '[]',
  stock_count INTEGER,
  raw_json TEXT NOT NULL,
  local_status TEXT NOT NULL DEFAULT 'unknown',
  local_checked_at INTEGER,
  local_error TEXT,
  first_seen_at INTEGER NOT NULL,
  last_seen_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL,
  UNIQUE(source_config_id, offer_id)
);

CREATE INDEX IF NOT EXISTS idx_marketplace_offers_filter
  ON marketplace_offers(product_id, currency, price, last_seen_at);

CREATE TABLE IF NOT EXISTS marketplace_offer_changes (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  offer_key TEXT NOT NULL,
  change_type TEXT NOT NULL,
  summary_json TEXT NOT NULL,
  created_at INTEGER NOT NULL,
  notified_at INTEGER
);

CREATE INDEX IF NOT EXISTS idx_marketplace_changes_created
  ON marketplace_offer_changes(created_at DESC);

CREATE TABLE IF NOT EXISTS marketplace_alert_rules (
  id TEXT PRIMARY KEY,
  name TEXT NOT NULL,
  source_config_id TEXT,
  product_id TEXT,
  tags_json TEXT NOT NULL DEFAULT '[]',
  merchant TEXT,
  currency TEXT NOT NULL DEFAULT 'CNY',
  max_price REAL,
  drop_amount REAL,
  drop_percent REAL,
  notify_restock INTEGER NOT NULL DEFAULT 1,
  notify_verified INTEGER NOT NULL DEFAULT 1,
  notify_invalid_link INTEGER NOT NULL DEFAULT 0,
  enabled INTEGER NOT NULL DEFAULT 1,
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS marketplace_alert_state (
  rule_id TEXT NOT NULL,
  offer_key TEXT NOT NULL,
  signature TEXT NOT NULL DEFAULT '',
  condition_active INTEGER NOT NULL DEFAULT 0,
  baseline_ready INTEGER NOT NULL DEFAULT 0,
  updated_at INTEGER NOT NULL,
  PRIMARY KEY(rule_id, offer_key)
);

CREATE TABLE IF NOT EXISTS marketplace_settings (
  key TEXT PRIMARY KEY,
  value TEXT NOT NULL,
  updated_at INTEGER NOT NULL
);
