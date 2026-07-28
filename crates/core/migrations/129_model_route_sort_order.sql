ALTER TABLE model_routes ADD COLUMN sort_order INTEGER NOT NULL DEFAULT 0;

WITH ranked AS (
  SELECT
    id,
    ROW_NUMBER() OVER (
      PARTITION BY model_id
      ORDER BY priority DESC, id ASC
    ) * 10 AS sort_order
  FROM model_routes
)
UPDATE model_routes
SET sort_order = (
  SELECT ranked.sort_order
  FROM ranked
  WHERE ranked.id = model_routes.id
);

CREATE INDEX IF NOT EXISTS idx_model_routes_model_sort_order
  ON model_routes(model_id, sort_order, source_kind, source_id, id);
