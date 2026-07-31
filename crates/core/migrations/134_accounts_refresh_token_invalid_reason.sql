ALTER TABLE accounts ADD COLUMN refresh_token_invalid_reason TEXT;

WITH refresh_token_invalid_events AS (
  SELECT
    e.account_id,
    CASE
      WHEN e.type = 'account_status_update' THEN
        TRIM(SUBSTR(e.message, INSTR(e.message, ' reason=') + LENGTH(' reason=')))
      WHEN LOWER(e.message) LIKE '%refresh_token_expired%'
        OR LOWER(e.message) LIKE '%refresh token has expired%' THEN
        'refresh_token_invalid:refresh_token_expired'
      WHEN LOWER(e.message) LIKE '%refresh_token_reused%'
        OR LOWER(e.message) LIKE '%refresh token was already used%' THEN
        'refresh_token_invalid:refresh_token_reused'
      WHEN LOWER(e.message) LIKE '%refresh_token_invalidated%'
        OR LOWER(e.message) LIKE '%refresh token was revoked%' THEN
        'refresh_token_invalid:refresh_token_invalidated'
      WHEN LOWER(e.message) LIKE '%app_session_terminated%'
        OR LOWER(e.message) LIKE '%your session has ended%' THEN
        'refresh_token_invalid:app_session_terminated'
      WHEN LOWER(e.message) LIKE '%invalid_grant%'
        OR LOWER(e.message) LIKE '%refresh token is no longer valid%' THEN
        'refresh_token_invalid:invalid_grant'
      WHEN LOWER(e.message) LIKE '%refresh token failed with status 401%' THEN
        'refresh_token_invalid:refresh_token_unknown_401'
      ELSE NULL
    END AS reason,
    e.created_at,
    e.id
  FROM events e
  WHERE (
      e.type = 'account_status_update'
      AND INSTR(e.message, ' reason=') > 0
      AND LOWER(TRIM(SUBSTR(e.message, INSTR(e.message, ' reason=') + LENGTH(' reason='))))
          LIKE 'refresh_token_invalid:%'
    ) OR (
      e.type = 'usage_refresh_failed'
      AND (
        LOWER(e.message) LIKE '%refresh token failed with status 401%'
        OR (
          LOWER(e.message) LIKE '%refresh token failed with status 400%'
          AND (
            LOWER(e.message) LIKE '%invalid_grant%'
            OR LOWER(e.message) LIKE '%refresh token is no longer valid%'
            OR LOWER(e.message) LIKE '%app_session_terminated%'
            OR LOWER(e.message) LIKE '%your session has ended%'
          )
        )
      )
    )
)
UPDATE accounts
SET refresh_token_invalid_reason = (
  SELECT candidate.reason
  FROM refresh_token_invalid_events candidate
  WHERE candidate.account_id = accounts.id
    AND candidate.reason IS NOT NULL
    AND candidate.created_at >= COALESCE(
      (SELECT t.last_refresh FROM tokens t WHERE t.account_id = accounts.id),
      0
    )
  ORDER BY candidate.created_at DESC, candidate.id DESC
  LIMIT 1
)
WHERE refresh_token_invalid_reason IS NULL;
