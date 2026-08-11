ALTER TABLE aggregate_apis ADD COLUMN upstream_wire TEXT NOT NULL DEFAULT 'passthrough';
UPDATE aggregate_apis SET upstream_wire = 'passthrough' WHERE upstream_wire IS NULL OR TRIM(upstream_wire) = '';
