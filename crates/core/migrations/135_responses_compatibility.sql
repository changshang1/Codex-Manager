ALTER TABLE aggregate_apis ADD COLUMN compatibility_config_json TEXT;
ALTER TABLE model_routes ADD COLUMN compatibility_override_json TEXT;

CREATE TABLE aggregate_api_response_affinity (
  response_id TEXT PRIMARY KEY,
  aggregate_api_id TEXT NOT NULL REFERENCES aggregate_apis(id) ON DELETE CASCADE,
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL
);

CREATE INDEX idx_aggregate_api_response_affinity_api
  ON aggregate_api_response_affinity(aggregate_api_id, updated_at DESC);
