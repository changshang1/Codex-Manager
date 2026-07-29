// 中文注释：aggregate_apis.rs 保留存储流程；这里集中维护基础 aggregate_apis SQL 片段。
pub(super) const AGGREGATE_API_SELECT_SQL: &str = "SELECT
    id,
    provider_type,
    supplier_name,
    sort,
    url,
    auth_type,
    auth_params_json,
    action,
    model_override,
    status,
    created_at,
    updated_at,
    last_test_at,
    last_test_status,
    last_test_error,
    balance_query_enabled,
    balance_query_template,
    balance_query_base_url,
    balance_query_user_id,
    balance_query_config_json,
    last_balance_at,
    last_balance_status,
    last_balance_error,
    last_balance_json,
    auto_toggle_enabled,
    consecutive_failures,
    auto_disabled,
    auto_disabled_at,
    auto_disabled_reason
 FROM aggregate_apis";
pub(super) const AGGREGATE_API_MODEL_SOURCE_KIND: &str = "aggregate_api";
pub(super) const AGGREGATE_API_ACTIVE_STATUS_CONDITION: &str =
    "LOWER(TRIM(COALESCE(status, ''))) = 'active' AND COALESCE(auto_disabled, 0) = 0";
pub(super) const AGGREGATE_API_NORMALIZED_PROVIDER_SQL: &str =
    "REPLACE(LOWER(TRIM(COALESCE(provider_type, ''))), '-', '_')";

pub(super) fn aggregate_api_by_id_sql() -> String {
    format!(
        "{AGGREGATE_API_SELECT_SQL}
         WHERE id = ?1
         LIMIT 1"
    )
}

pub(super) fn aggregate_api_with_secrets_by_id_sql() -> &'static str {
    "SELECT
        a.id,
        a.provider_type,
        a.supplier_name,
        a.sort,
        a.url,
        a.auth_type,
        a.auth_params_json,
        a.action,
        a.model_override,
        a.status,
        a.created_at,
        a.updated_at,
        a.last_test_at,
        a.last_test_status,
        a.last_test_error,
        a.balance_query_enabled,
        a.balance_query_template,
        a.balance_query_base_url,
        a.balance_query_user_id,
        a.balance_query_config_json,
        a.last_balance_at,
        a.last_balance_status,
        a.last_balance_error,
        a.last_balance_json,
        a.auto_toggle_enabled,
        a.consecutive_failures,
        a.auto_disabled,
        a.auto_disabled_at,
        a.auto_disabled_reason,
        s.secret_value,
        bs.access_token
     FROM aggregate_apis a
     LEFT JOIN aggregate_api_secrets s ON s.aggregate_api_id = a.id
     LEFT JOIN aggregate_api_balance_secrets bs ON bs.aggregate_api_id = a.id
     WHERE a.id = ?1
     LIMIT 1"
}

pub(super) fn aggregate_api_status_by_id_sql() -> &'static str {
    "SELECT status
     FROM aggregate_apis
     WHERE id = ?1
     LIMIT 1"
}

pub(super) fn aggregate_api_auth_type_by_id_sql() -> &'static str {
    "SELECT auth_type
     FROM aggregate_apis
     WHERE id = ?1
     LIMIT 1"
}

pub(super) fn aggregate_api_secret_config_by_id_sql() -> &'static str {
    "SELECT a.auth_type, s.secret_value
     FROM aggregate_apis a
     LEFT JOIN aggregate_api_secrets s ON s.aggregate_api_id = a.id
     WHERE a.id = ?1
     LIMIT 1"
}

pub(super) fn aggregate_api_update_config_by_id_sql() -> &'static str {
    "SELECT
        auth_type,
        balance_query_enabled,
        balance_query_template,
        balance_query_base_url,
        balance_query_user_id,
        balance_query_config_json
     FROM aggregate_apis
     WHERE id = ?1
     LIMIT 1"
}

pub(super) fn update_aggregate_api_url_sql() -> &'static str {
    "UPDATE aggregate_apis SET url = ?1, updated_at = ?2 WHERE id = ?3"
}

pub(super) fn update_aggregate_api_supplier_name_sql() -> &'static str {
    "UPDATE aggregate_apis SET supplier_name = ?1, updated_at = ?2 WHERE id = ?3"
}

pub(super) fn update_aggregate_api_sort_sql() -> &'static str {
    "UPDATE aggregate_apis SET sort = ?1, updated_at = ?2 WHERE id = ?3"
}

pub(super) fn update_aggregate_api_status_sql() -> &'static str {
    "UPDATE aggregate_apis SET status = ?1, updated_at = ?2 WHERE id = ?3"
}

pub(super) fn aggregate_api_auto_toggle_state_by_id_sql() -> &'static str {
    "SELECT
        auto_toggle_enabled,
        consecutive_failures,
        auto_disabled,
        auto_disabled_at,
        auto_disabled_reason
     FROM aggregate_apis
     WHERE id = ?1
     LIMIT 1"
}

pub(super) fn record_aggregate_api_daily_quota_failure_sql() -> &'static str {
    "UPDATE aggregate_apis
     SET consecutive_failures = COALESCE(consecutive_failures, 0) + 1,
         auto_disabled = CASE
             WHEN COALESCE(consecutive_failures, 0) + 1 >= ?2 THEN 1
             ELSE auto_disabled
         END,
         auto_disabled_at = CASE
             WHEN COALESCE(consecutive_failures, 0) + 1 >= ?2 THEN ?3
             ELSE auto_disabled_at
         END,
         auto_disabled_reason = CASE
             WHEN COALESCE(consecutive_failures, 0) + 1 >= ?2 THEN ?4
             ELSE auto_disabled_reason
         END
     WHERE id = ?1
       AND auto_toggle_enabled = 1
       AND LOWER(TRIM(COALESCE(status, ''))) = 'active'
       AND COALESCE(auto_disabled, 0) = 0"
}

pub(super) fn reset_aggregate_api_consecutive_failures_sql() -> &'static str {
    "UPDATE aggregate_apis
     SET consecutive_failures = 0
     WHERE id = ?1
       AND COALESCE(consecutive_failures, 0) <> 0"
}

pub(super) fn set_aggregate_api_auto_toggle_enabled_sql() -> &'static str {
    "UPDATE aggregate_apis
     SET auto_toggle_enabled = ?1,
         consecutive_failures = CASE WHEN ?1 = 0 THEN 0 ELSE consecutive_failures END,
         auto_disabled = CASE WHEN ?1 = 0 THEN 0 ELSE auto_disabled END
     WHERE id = ?2"
}

pub(super) fn recover_aggregate_api_auto_disable_sql() -> &'static str {
    "UPDATE aggregate_apis
     SET consecutive_failures = 0,
         auto_disabled = 0
     WHERE id = ?1
       AND (
           COALESCE(consecutive_failures, 0) <> 0
           OR COALESCE(auto_disabled, 0) <> 0
       )"
}

pub(super) fn recover_aggregate_apis_before_local_day_sql() -> &'static str {
    "UPDATE aggregate_apis
     SET consecutive_failures = 0,
         auto_disabled = 0
     WHERE COALESCE(auto_disabled, 0) = 1
       AND LOWER(TRIM(COALESCE(status, ''))) = 'active'
       AND auto_disabled_at IS NOT NULL
       AND date(auto_disabled_at, 'unixepoch', 'localtime')
           < date(?1, 'unixepoch', 'localtime')"
}

pub(super) fn has_recoverable_aggregate_apis_before_local_day_sql() -> &'static str {
    "SELECT EXISTS(
         SELECT 1
         FROM aggregate_apis
         WHERE COALESCE(auto_disabled, 0) = 1
           AND LOWER(TRIM(COALESCE(status, ''))) = 'active'
           AND auto_disabled_at IS NOT NULL
           AND date(auto_disabled_at, 'unixepoch', 'localtime')
               < date(?1, 'unixepoch', 'localtime')
     )"
}

pub(super) fn update_aggregate_api_provider_type_sql() -> &'static str {
    "UPDATE aggregate_apis SET provider_type = ?1, updated_at = ?2 WHERE id = ?3"
}

pub(super) fn update_aggregate_api_auth_type_sql() -> &'static str {
    "UPDATE aggregate_apis SET auth_type = ?1, updated_at = ?2 WHERE id = ?3"
}

pub(super) fn update_aggregate_api_auth_params_json_sql() -> &'static str {
    "UPDATE aggregate_apis SET auth_params_json = ?1, updated_at = ?2 WHERE id = ?3"
}

pub(super) fn update_aggregate_api_action_sql() -> &'static str {
    "UPDATE aggregate_apis SET action = ?1, updated_at = ?2 WHERE id = ?3"
}

pub(super) fn update_aggregate_api_model_override_sql() -> &'static str {
    "UPDATE aggregate_apis SET model_override = ?1, updated_at = ?2 WHERE id = ?3"
}

pub(super) fn update_aggregate_api_balance_query_sql() -> &'static str {
    "UPDATE aggregate_apis
     SET balance_query_enabled = ?1,
         balance_query_template = ?2,
         balance_query_base_url = ?3,
         balance_query_user_id = ?4,
         balance_query_config_json = ?5,
         updated_at = ?6
     WHERE id = ?7"
}

pub(super) fn update_aggregate_api_balance_result_sql() -> &'static str {
    "UPDATE aggregate_apis
     SET last_balance_at = ?1,
         last_balance_status = ?2,
         last_balance_error = ?3,
         last_balance_json = ?4,
         updated_at = ?1
     WHERE id = ?5"
}

pub(super) fn update_aggregate_api_test_result_sql() -> &'static str {
    "UPDATE aggregate_apis
     SET last_test_at = ?1,
         last_test_status = ?2,
         last_test_error = ?3,
         updated_at = ?1
     WHERE id = ?4"
}

pub(super) fn update_aggregate_api_last_test_error_sql() -> &'static str {
    "UPDATE aggregate_apis SET last_test_error = ?1 WHERE id = ?2"
}

pub(super) fn delete_aggregate_api_by_id_sql() -> &'static str {
    "DELETE FROM aggregate_apis WHERE id = ?1"
}

pub(super) fn delete_aggregate_api_secret_by_id_sql() -> &'static str {
    "DELETE FROM aggregate_api_secrets WHERE aggregate_api_id = ?1"
}

pub(super) fn delete_aggregate_api_balance_secret_by_id_sql() -> &'static str {
    "DELETE FROM aggregate_api_balance_secrets WHERE aggregate_api_id = ?1"
}

pub(super) fn aggregate_api_supplier_models_list_sql(
    has_supplier_key: bool,
    has_provider_type: bool,
) -> String {
    let mut clauses = Vec::new();
    if has_supplier_key {
        clauses.push("supplier_key = ?");
    }
    if has_provider_type {
        clauses.push("provider_type = ?");
    }
    let where_clause = if clauses.is_empty() {
        String::new()
    } else {
        format!(" WHERE {}", clauses.join(" AND "))
    };
    format!(
        "SELECT supplier_key, provider_type, upstream_model, display_name,
                status, created_at, updated_at
         FROM aggregate_api_supplier_models{where_clause}
         ORDER BY supplier_key ASC, provider_type ASC, upstream_model ASC"
    )
}

pub(super) fn delete_aggregate_api_supplier_model_sql() -> &'static str {
    "DELETE FROM aggregate_api_supplier_models
     WHERE supplier_key = ?1 AND provider_type = ?2 AND upstream_model = ?3"
}

pub(super) fn aggregate_api_exists_sql() -> &'static str {
    "SELECT EXISTS(SELECT 1 FROM aggregate_apis WHERE id = ?1)"
}

pub(super) fn aggregate_api_overview_stats_sql() -> &'static str {
    "SELECT
        COUNT(1) AS source_count,
        IFNULL(SUM(CASE WHEN balance_query_enabled = 1 THEN 1 ELSE 0 END), 0)
            AS enabled_balance_query_count,
        IFNULL(SUM(CASE WHEN last_balance_status = 'success' THEN 1 ELSE 0 END), 0)
            AS ok_count,
        IFNULL(SUM(CASE WHEN last_balance_status IN ('error', 'failed') THEN 1 ELSE 0 END), 0)
            AS error_count,
        MAX(last_balance_at) AS last_refreshed_at
     FROM aggregate_apis"
}

pub(super) fn aggregate_api_supplier_identity_by_id_sql() -> &'static str {
    "SELECT id, provider_type, supplier_name, url
     FROM aggregate_apis
     WHERE id = ?1
     LIMIT 1"
}

pub(super) fn aggregate_api_secret_by_id_sql() -> &'static str {
    "SELECT secret_value FROM aggregate_api_secrets WHERE aggregate_api_id = ?1 LIMIT 1"
}

pub(super) fn aggregate_api_balance_secret_by_id_sql() -> &'static str {
    "SELECT access_token FROM aggregate_api_balance_secrets WHERE aggregate_api_id = ?1 LIMIT 1"
}
