use codexmanager_core::rpc::types::{AggregateApiListResult, JsonRpcRequest, JsonRpcResponse};

use crate::{
    create_aggregate_api, delete_aggregate_api, list_aggregate_apis, read_aggregate_api_secret,
    refresh_aggregate_api_balance, test_aggregate_api_connection, update_aggregate_api,
    update_aggregate_api_sorts,
};

/// 函数 `api_id_param`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - req: 参数 req
///
/// # 返回
/// 返回函数执行结果
fn api_id_param(req: &JsonRpcRequest) -> Option<&str> {
    super::str_param(req, "id").or_else(|| super::str_param(req, "apiId"))
}

/// 函数 `try_handle`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - super: 参数 super
///
/// # 返回
/// 返回函数执行结果
pub(super) fn try_handle(req: &JsonRpcRequest) -> Option<JsonRpcResponse> {
    let result = match req.method.as_str() {
        "aggregateApi/list" => super::value_or_error(
            list_aggregate_apis().map(|items| AggregateApiListResult { items }),
        ),
        "aggregateApi/create" => {
            let provider_type = super::string_param(req, "providerType");
            let supplier_name = super::string_param(req, "supplierName");
            let sort = super::i64_param(req, "sort");
            let url = super::string_param(req, "url");
            let key = super::string_param(req, "key");
            let auth_type = super::string_param(req, "authType");
            let auth_custom_enabled = super::bool_param(req, "authCustomEnabled");
            let auth_params = req
                .params
                .as_ref()
                .and_then(|v| v.get("authParams"))
                .cloned();
            let action_custom_enabled = super::bool_param(req, "actionCustomEnabled");
            let action = super::string_param(req, "action");
            let model_override = super::string_param(req, "modelOverride");
            let username = super::string_param(req, "username");
            let password = super::string_param(req, "password");
            let balance_query_enabled = super::bool_param(req, "balanceQueryEnabled");
            let balance_query_template = super::string_param(req, "balanceQueryTemplate");
            let balance_query_base_url = super::string_param(req, "balanceQueryBaseUrl");
            let balance_query_access_token = super::string_param(req, "balanceQueryAccessToken");
            let balance_query_user_id = super::string_param(req, "balanceQueryUserId");
            let balance_query_config_json = super::string_param(req, "balanceQueryConfigJson");
            super::value_or_error(create_aggregate_api(
                url,
                key,
                provider_type,
                supplier_name,
                sort,
                auth_type,
                auth_custom_enabled,
                auth_params,
                action_custom_enabled,
                action,
                model_override,
                username,
                password,
                balance_query_enabled,
                balance_query_template,
                balance_query_base_url,
                balance_query_access_token,
                balance_query_user_id,
                balance_query_config_json,
            ))
        }
        "aggregateApi/update" => {
            let api_id = api_id_param(req).unwrap_or("");
            let provider_type = super::string_param(req, "providerType");
            let supplier_name = super::string_param(req, "supplierName");
            let sort = super::i64_param(req, "sort");
            let status = super::string_param(req, "status");
            let url = super::string_param(req, "url");
            let key = super::string_param(req, "key");
            let auth_type = super::string_param(req, "authType");
            let auth_custom_enabled = super::bool_param(req, "authCustomEnabled");
            let auth_params = req
                .params
                .as_ref()
                .and_then(|v| v.get("authParams"))
                .cloned();
            let action_custom_enabled = super::bool_param(req, "actionCustomEnabled");
            let action = super::string_param(req, "action");
            let model_override = super::string_param(req, "modelOverride");
            let username = super::string_param(req, "username");
            let password = super::string_param(req, "password");
            let balance_query_enabled = super::bool_param(req, "balanceQueryEnabled");
            let balance_query_template = super::string_param(req, "balanceQueryTemplate");
            let balance_query_base_url = super::string_param(req, "balanceQueryBaseUrl");
            let balance_query_access_token = super::string_param(req, "balanceQueryAccessToken");
            let balance_query_user_id = super::string_param(req, "balanceQueryUserId");
            let balance_query_config_json = super::string_param(req, "balanceQueryConfigJson");
            super::ok_or_error(update_aggregate_api(
                api_id,
                url,
                key,
                provider_type,
                supplier_name,
                sort,
                status,
                auth_type,
                auth_custom_enabled,
                auth_params,
                action_custom_enabled,
                action,
                model_override,
                username,
                password,
                balance_query_enabled,
                balance_query_template,
                balance_query_base_url,
                balance_query_access_token,
                balance_query_user_id,
                balance_query_config_json,
            ))
        }
        "aggregateApi/updateSorts" => super::value_or_error(
            aggregate_api_sort_updates_param(req).and_then(update_aggregate_api_sorts),
        ),
        "aggregateApi/readSecret" => {
            let api_id = api_id_param(req).unwrap_or("");
            super::value_or_error(read_aggregate_api_secret(api_id))
        }
        "aggregateApi/delete" => {
            let api_id = api_id_param(req).unwrap_or("");
            super::ok_or_error(delete_aggregate_api(api_id))
        }
        "aggregateApi/testConnection" => {
            let api_id = api_id_param(req).unwrap_or("");
            super::value_or_error(test_aggregate_api_connection(api_id))
        }
        "aggregateApi/refreshBalance" => {
            let api_id = api_id_param(req).unwrap_or("");
            super::value_or_error(refresh_aggregate_api_balance(api_id))
        }
        _ => return None,
    };

    Some(super::response(req, result))
}

fn aggregate_api_sort_updates_param(
    req: &JsonRpcRequest,
) -> Result<Vec<crate::aggregate_api::AggregateApiSortUpdate>, String> {
    let items = req
        .params
        .as_ref()
        .and_then(|params| params.get("updates"))
        .and_then(|value| value.as_array())
        .ok_or_else(|| "missing aggregate api sort updates".to_string())?;
    let mut updates = Vec::with_capacity(items.len());
    for (index, item) in items.iter().enumerate() {
        let source = item.as_object().ok_or_else(|| {
            format!("aggregate api sort update at index {index} must be an object")
        })?;
        let api_id = source
            .get("apiId")
            .or_else(|| source.get("api_id"))
            .or_else(|| source.get("id"))
            .and_then(|value| value.as_str())
            .ok_or_else(|| format!("aggregate api sort update at index {index} missing apiId"))?
            .to_string();
        let sort = source
            .get("sort")
            .and_then(|value| value.as_i64())
            .ok_or_else(|| format!("aggregate api sort update at index {index} missing sort"))?;
        updates.push(crate::aggregate_api::AggregateApiSortUpdate { api_id, sort });
    }
    Ok(updates)
}

#[cfg(test)]
#[path = "aggregate_api_tests.rs"]
mod tests;
