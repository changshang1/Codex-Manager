use codexmanager_core::rpc::types::{
    JsonRpcRequest, JsonRpcResponse, UsageListResult, UsageReadResult,
};

use crate::{usage_aggregate, usage_list, usage_read, usage_refresh};

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
        "account/usage/read" => {
            let account_id =
                super::str_param(req, "accountId").or_else(|| super::str_param(req, "account_id"));
            super::as_json(UsageReadResult {
                snapshot: usage_read::read_usage_snapshot(account_id),
            })
        }
        "account/usage/list" => {
            let limit = super::i64_param(req, "limit").map(|value| value.max(0) as usize);
            super::value_or_error(
                usage_list::read_usage_snapshots_limited(limit)
                    .map(|items| UsageListResult { items }),
            )
        }
        "account/usage/aggregate" => {
            super::value_or_error(usage_aggregate::read_usage_aggregate_summary())
        }
        "account/usage/refresh" => {
            let account_id =
                super::str_param(req, "accountId").or_else(|| super::str_param(req, "account_id"));
            let mark_unavailable_on_failure = mark_unavailable_on_failure_param(req);
            let result = match account_id {
                Some(account_id) => usage_refresh::refresh_usage_for_account_result_with_policy(
                    account_id,
                    mark_unavailable_on_failure,
                ),
                None => usage_refresh::refresh_usage_for_all_accounts_result(),
            };
            super::value_or_error(result)
        }
        "account/usage/resetCredits" => {
            let account_id =
                super::str_param(req, "accountId").or_else(|| super::str_param(req, "account_id"));
            super::value_or_error(
                account_id
                    .ok_or_else(|| "accountId is required".to_string())
                    .and_then(crate::usage_reset_credits::read_reset_credits),
            )
        }
        "account/usage/resetCredit/consume" => {
            let account_id =
                super::str_param(req, "accountId").or_else(|| super::str_param(req, "account_id"));
            super::value_or_error(
                account_id
                    .ok_or_else(|| "accountId is required".to_string())
                    .and_then(crate::usage_reset_credits::consume_reset_credit),
            )
        }
        _ => return None,
    };

    Some(super::response(req, result))
}

fn mark_unavailable_on_failure_param(req: &JsonRpcRequest) -> bool {
    super::bool_param(req, "markUnavailableOnFailure")
        .or_else(|| super::bool_param(req, "mark_unavailable_on_failure"))
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::mark_unavailable_on_failure_param;
    use codexmanager_core::rpc::types::JsonRpcRequest;

    fn request(params: Option<serde_json::Value>) -> JsonRpcRequest {
        JsonRpcRequest {
            id: 1.into(),
            method: "account/usage/refresh".to_string(),
            params,
            trace: None,
        }
    }

    #[test]
    fn usage_refresh_failure_policy_defaults_off_and_accepts_both_param_styles() {
        assert!(!mark_unavailable_on_failure_param(&request(None)));
        assert!(mark_unavailable_on_failure_param(&request(Some(
            serde_json::json!({ "markUnavailableOnFailure": true }),
        ))));
        assert!(mark_unavailable_on_failure_param(&request(Some(
            serde_json::json!({ "mark_unavailable_on_failure": true }),
        ))));
    }
}
