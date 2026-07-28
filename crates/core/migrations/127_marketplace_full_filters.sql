ALTER TABLE marketplace_offers
  ADD COLUMN priceai_updated_at TEXT;

UPDATE marketplace_sources
SET tags_json = '[]',
    updated_at = unixepoch()
WHERE id = 'default-chatgpt-plus'
  AND product_id = 'chatgpt-plus'
  AND tags_json = '["account_verified"]'
  AND merchant IS NULL;
