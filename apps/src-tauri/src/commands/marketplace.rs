use crate::commands::shared::rpc_call_in_background;
use serde_json::{json, Value};

#[tauri::command]
pub async fn service_marketplace_source_list(addr: Option<String>) -> Result<Value, String> {
    rpc_call_in_background("marketplace/sourceList", addr, None).await
}

#[tauri::command]
pub async fn service_marketplace_source_upsert(
    payload: Value,
    addr: Option<String>,
) -> Result<Value, String> {
    rpc_call_in_background("marketplace/sourceUpsert", addr, Some(payload)).await
}

#[tauri::command]
pub async fn service_marketplace_source_delete(
    id: String,
    addr: Option<String>,
) -> Result<Value, String> {
    rpc_call_in_background("marketplace/sourceDelete", addr, Some(json!({"id": id}))).await
}

#[tauri::command]
pub async fn service_marketplace_offer_list(
    payload: Value,
    addr: Option<String>,
) -> Result<Value, String> {
    rpc_call_in_background("marketplace/offerList", addr, Some(payload)).await
}

#[tauri::command]
pub async fn service_marketplace_offer_verify(
    offer_key: String,
    addr: Option<String>,
) -> Result<Value, String> {
    rpc_call_in_background(
        "marketplace/offerVerify",
        addr,
        Some(json!({"offerKey": offer_key})),
    )
    .await
}

#[tauri::command]
pub async fn service_marketplace_favorite_merchant_list(
    addr: Option<String>,
) -> Result<Value, String> {
    rpc_call_in_background("marketplace/favoriteMerchantList", addr, None).await
}

#[tauri::command]
pub async fn service_marketplace_favorite_merchant_set(
    offer_key: String,
    favorite: bool,
    addr: Option<String>,
) -> Result<Value, String> {
    rpc_call_in_background(
        "marketplace/favoriteMerchantSet",
        addr,
        Some(json!({"offerKey": offer_key, "favorite": favorite})),
    )
    .await
}

#[tauri::command]
pub async fn service_marketplace_refresh(addr: Option<String>) -> Result<Value, String> {
    rpc_call_in_background("marketplace/refresh", addr, None).await
}

#[tauri::command]
pub async fn service_marketplace_change_list(
    payload: Value,
    addr: Option<String>,
) -> Result<Value, String> {
    rpc_call_in_background("marketplace/changeList", addr, Some(payload)).await
}

#[tauri::command]
pub async fn service_marketplace_alert_list(addr: Option<String>) -> Result<Value, String> {
    rpc_call_in_background("marketplace/alertList", addr, None).await
}

#[tauri::command]
pub async fn service_marketplace_alert_upsert(
    payload: Value,
    addr: Option<String>,
) -> Result<Value, String> {
    rpc_call_in_background("marketplace/alertUpsert", addr, Some(payload)).await
}

#[tauri::command]
pub async fn service_marketplace_alert_delete(
    id: String,
    addr: Option<String>,
) -> Result<Value, String> {
    rpc_call_in_background("marketplace/alertDelete", addr, Some(json!({"id": id}))).await
}

#[tauri::command]
pub async fn service_marketplace_notification_get(addr: Option<String>) -> Result<Value, String> {
    rpc_call_in_background("marketplace/notificationGet", addr, None).await
}

#[tauri::command]
pub async fn service_marketplace_notification_set(
    enabled: bool,
    addr: Option<String>,
) -> Result<Value, String> {
    rpc_call_in_background(
        "marketplace/notificationSet",
        addr,
        Some(json!({"enabled": enabled})),
    )
    .await
}
