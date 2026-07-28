use crate::commands::shared::rpc_call_in_background;

/// 函数 `service_usage_read`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - addr: 参数 addr
/// - account_id: 参数 account_id
///
/// # 返回
/// 返回函数执行结果
#[tauri::command]
pub async fn service_usage_read(
    addr: Option<String>,
    account_id: Option<String>,
) -> Result<serde_json::Value, String> {
    let params = account_id.map(|id| serde_json::json!({ "accountId": id }));
    rpc_call_in_background("account/usage/read", addr, params).await
}

/// 函数 `service_usage_list`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - addr: 参数 addr
///
/// # 返回
/// 返回函数执行结果
#[tauri::command]
pub async fn service_usage_list(addr: Option<String>) -> Result<serde_json::Value, String> {
    rpc_call_in_background("account/usage/list", addr, None).await
}

/// 函数 `service_usage_aggregate`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - addr: 参数 addr
///
/// # 返回
/// 返回函数执行结果
#[tauri::command]
pub async fn service_usage_aggregate(addr: Option<String>) -> Result<serde_json::Value, String> {
    rpc_call_in_background("account/usage/aggregate", addr, None).await
}

/// 函数 `service_usage_refresh`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - addr: 参数 addr
/// - account_id: 参数 account_id
/// - mark_unavailable_on_failure: 快速开启校验失败时将账号健康状态标记为不可用
///
/// # 返回
/// 返回函数执行结果
#[tauri::command]
pub async fn service_usage_refresh(
    addr: Option<String>,
    account_id: Option<String>,
    mark_unavailable_on_failure: Option<bool>,
) -> Result<serde_json::Value, String> {
    let mut params = serde_json::Map::new();
    if let Some(id) = account_id {
        params.insert("accountId".to_string(), serde_json::json!(id));
    }
    if let Some(mark_unavailable_on_failure) = mark_unavailable_on_failure {
        params.insert(
            "markUnavailableOnFailure".to_string(),
            serde_json::json!(mark_unavailable_on_failure),
        );
    }
    let params = (!params.is_empty()).then(|| serde_json::Value::Object(params));
    rpc_call_in_background("account/usage/refresh", addr, params).await
}

#[tauri::command]
pub async fn service_usage_reset_credits(
    addr: Option<String>,
    account_id: String,
) -> Result<serde_json::Value, String> {
    rpc_call_in_background(
        "account/usage/resetCredits",
        addr,
        Some(serde_json::json!({ "accountId": account_id })),
    )
    .await
}

#[tauri::command]
pub async fn service_usage_reset_credit_consume(
    addr: Option<String>,
    account_id: String,
) -> Result<serde_json::Value, String> {
    rpc_call_in_background(
        "account/usage/resetCredit/consume",
        addr,
        Some(serde_json::json!({ "accountId": account_id })),
    )
    .await
}
