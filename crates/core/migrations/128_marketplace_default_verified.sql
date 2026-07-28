UPDATE marketplace_sources
SET tags_json = '["account_verified"]',
    updated_at = CAST(strftime('%s', 'now') AS INTEGER)
WHERE id = 'default-chatgpt-plus'
  AND product_id = 'chatgpt-plus'
  AND merchant IS NULL
  AND tags_json = '[]';
