use super::{aggregate_api_sort_updates_param, try_handle};
use codexmanager_core::rpc::types::JsonRpcRequest;

/// 函数 `rpc_request`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - method: 参数 method
/// - params: 参数 params
///
/// # 返回
/// 返回函数执行结果
fn rpc_request(method: &str, params: serde_json::Value) -> JsonRpcRequest {
    JsonRpcRequest {
        id: 1.into(),
        method: method.to_string(),
        params: Some(params),
        trace: None,
    }
}

/// 函数 `error_message`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - resp: 参数 resp
///
/// # 返回
/// 返回函数执行结果
fn error_message(resp: &codexmanager_core::rpc::types::JsonRpcResponse) -> String {
    resp.result
        .get("error")
        .and_then(|value| value.as_str())
        .unwrap_or("")
        .to_string()
}

/// 函数 `aggregate_api_update_accepts_id_and_api_id`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// 无
///
/// # 返回
/// 无
#[test]
fn aggregate_api_update_accepts_id_and_api_id() {
    let missing = try_handle(&rpc_request(
        "aggregateApi/update",
        serde_json::json!({ "supplierName": "codex" }),
    ))
    .expect("response");
    assert_eq!(error_message(&missing), "aggregate api id required");

    let with_id = try_handle(&rpc_request(
        "aggregateApi/update",
        serde_json::json!({ "id": "ag_test", "supplierName": "codex" }),
    ))
    .expect("response");
    assert_ne!(error_message(&with_id), "aggregate api id required");

    let with_api_id = try_handle(&rpc_request(
        "aggregateApi/update",
        serde_json::json!({ "apiId": "ag_test", "supplierName": "codex" }),
    ))
    .expect("response");
    assert_ne!(error_message(&with_api_id), "aggregate api id required");
}

#[test]
fn aggregate_api_sort_updates_accept_supported_ids_and_reject_bad_rows() {
    let parsed = aggregate_api_sort_updates_param(&rpc_request(
        "aggregateApi/updateSorts",
        serde_json::json!({
            "updates": [
                { "apiId": "agg-a", "sort": 0 },
                { "id": "agg-b", "sort": 5 }
            ]
        }),
    ))
    .expect("parse sort updates");
    assert_eq!(parsed.len(), 2);
    assert_eq!(parsed[0].api_id, "agg-a");
    assert_eq!(parsed[1].api_id, "agg-b");

    let error = aggregate_api_sort_updates_param(&rpc_request(
        "aggregateApi/updateSorts",
        serde_json::json!({ "updates": [{ "apiId": "agg-a" }] }),
    ))
    .expect_err("missing sort should fail");
    assert!(error.contains("missing sort"));
}

/// 函数 `aggregate_api_test_connection_accepts_id_and_api_id`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// 无
///
/// # 返回
/// 无
#[test]
fn aggregate_api_test_connection_accepts_id_and_api_id() {
    let missing = try_handle(&rpc_request(
        "aggregateApi/testConnection",
        serde_json::json!({}),
    ))
    .expect("response");
    assert_eq!(error_message(&missing), "aggregate api id required");

    let with_id = try_handle(&rpc_request(
        "aggregateApi/testConnection",
        serde_json::json!({ "id": "ag_test" }),
    ))
    .expect("response");
    assert_ne!(error_message(&with_id), "aggregate api id required");

    let with_api_id = try_handle(&rpc_request(
        "aggregateApi/testConnection",
        serde_json::json!({ "apiId": "ag_test" }),
    ))
    .expect("response");
    assert_ne!(error_message(&with_api_id), "aggregate api id required");
}
