ALTER TABLE marketplace_offers
  ADD COLUMN source_included_at TEXT;

ALTER TABLE marketplace_offers
  ADD COLUMN source_shop_created_at TEXT;

UPDATE marketplace_offers
SET source_included_at = CASE
      WHEN json_valid(raw_json) THEN NULLIF(TRIM(json_extract(raw_json, '$.sourceIncludedAt')), '')
      ELSE NULL
    END,
    source_shop_created_at = CASE
      WHEN json_valid(raw_json) THEN NULLIF(TRIM(json_extract(raw_json, '$.sourceShopCreatedAt')), '')
      ELSE NULL
    END;
