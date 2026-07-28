use codexmanager_core::rpc::types::{JsonRpcRequest, JsonRpcResponse};
use codexmanager_core::storage::{MarketplaceAlertRuleInput, MarketplaceSourceInput};
use serde_json::Value;

fn optional_string(req: &JsonRpcRequest, key: &str) -> Option<String> {
    super::str_param(req, key)
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .map(ToOwned::to_owned)
}
fn optional_f64(req: &JsonRpcRequest, key: &str) -> Option<f64> {
    req.params
        .as_ref()
        .and_then(|v| v.get(key))
        .and_then(Value::as_f64)
}
fn bool_or(req: &JsonRpcRequest, key: &str, default: bool) -> bool {
    super::bool_param(req, key).unwrap_or(default)
}
fn array_json(req: &JsonRpcRequest, key: &str) -> String {
    req.params
        .as_ref()
        .and_then(|v| v.get(key))
        .filter(|v| v.is_array())
        .cloned()
        .unwrap_or_else(|| Value::Array(Vec::new()))
        .to_string()
}

pub(super) fn try_handle(req: &JsonRpcRequest) -> Option<JsonRpcResponse> {
    let result = match req.method.as_str() {
        "marketplace/sourceList" => super::value_or_error(crate::marketplace::list_sources()),
        "marketplace/sourceUpsert" => {
            let id = optional_string(req, "id")
                .unwrap_or_else(|| format!("source-{}", codexmanager_core::storage::now_ts()));
            super::value_or_error(crate::marketplace::upsert_source(MarketplaceSourceInput {
                id,
                product_id: optional_string(req, "productId")
                    .unwrap_or_else(|| "chatgpt-plus".to_string()),
                tags_json: array_json(req, "tags"),
                merchant: optional_string(req, "merchant"),
                enabled: bool_or(req, "enabled", true),
                verify_enabled: bool_or(req, "verifyEnabled", true),
            }))
        }
        "marketplace/sourceDelete" => super::value_or_error(crate::marketplace::delete_source(
            super::str_param(req, "id").unwrap_or(""),
        )),
        "marketplace/offerList" => super::value_or_error(crate::marketplace::list_offers(
            optional_string(req, "sourceConfigId").as_deref(),
            optional_string(req, "productId").as_deref(),
            super::i64_param(req, "limit").unwrap_or(500),
        )),
        "marketplace/offerVerify" => super::value_or_error(crate::marketplace::verify_offer(
            super::str_param(req, "offerKey").unwrap_or(""),
        )),
        "marketplace/favoriteMerchantList" => {
            super::value_or_error(crate::marketplace::list_favorite_merchants())
        }
        "marketplace/favoriteMerchantSet" => super::value_or_error(
            super::bool_param(req, "favorite")
                .ok_or_else(|| "favorite 必须是布尔值".to_string())
                .and_then(|favorite| {
                    crate::marketplace::set_favorite_merchant(
                        super::str_param(req, "offerKey").unwrap_or(""),
                        favorite,
                    )
                }),
        ),
        // Automatic verification and desktop notifications are reserved for the
        // service scheduler; RPC callers can only request a manual refresh.
        "marketplace/refresh" => super::value_or_error(crate::marketplace::refresh(false)),
        "marketplace/changeList" => super::value_or_error(crate::marketplace::list_changes(
            super::i64_param(req, "limit").unwrap_or(200),
        )),
        "marketplace/alertList" => super::value_or_error(crate::marketplace::list_rules()),
        "marketplace/alertUpsert" => {
            let id = optional_string(req, "id")
                .unwrap_or_else(|| format!("rule-{}", codexmanager_core::storage::now_ts()));
            super::value_or_error(crate::marketplace::upsert_rule(MarketplaceAlertRuleInput {
                id,
                name: optional_string(req, "name").unwrap_or_else(|| "商品池提醒".to_string()),
                source_config_id: optional_string(req, "sourceConfigId"),
                product_id: optional_string(req, "productId"),
                tags_json: array_json(req, "tags"),
                merchant: optional_string(req, "merchant"),
                currency: optional_string(req, "currency").unwrap_or_else(|| "CNY".to_string()),
                max_price: optional_f64(req, "maxPrice"),
                drop_amount: optional_f64(req, "dropAmount"),
                drop_percent: optional_f64(req, "dropPercent"),
                notify_restock: bool_or(req, "notifyRestock", true),
                notify_verified: bool_or(req, "notifyVerified", true),
                notify_invalid_link: bool_or(req, "notifyInvalidLink", false),
                enabled: bool_or(req, "enabled", true),
            }))
        }
        "marketplace/alertDelete" => super::value_or_error(crate::marketplace::delete_rule(
            super::str_param(req, "id").unwrap_or(""),
        )),
        "marketplace/notificationGet" => {
            super::value_or_error(crate::marketplace::notification_enabled())
        }
        "marketplace/notificationSet" => super::value_or_error(
            crate::marketplace::set_notification_enabled(bool_or(req, "enabled", false)),
        ),
        _ => return None,
    };
    Some(super::response(req, result))
}
