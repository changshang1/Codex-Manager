use codexmanager_core::storage::{
    now_ts, MarketplaceAlertRule, MarketplaceAlertRuleInput, MarketplaceAlertState,
    MarketplaceFavoriteMerchant, MarketplaceFavoriteMerchantInput, MarketplaceOffer,
    MarketplaceOfferInput, MarketplaceSource, MarketplaceSourceInput, Storage,
};
use reqwest::blocking::Client;
use reqwest::redirect::Policy;
use serde_json::{json, Value};
use std::collections::HashSet;
use std::io::Read;
use std::net::{IpAddr, Ipv4Addr};
use std::sync::{Mutex, OnceLock};
use std::thread;
use std::time::Duration;

const PRICEAI_BASE: &str = "https://priceai.cc";
// 商品池的产品范围是当前产品决策，不是运行时用户配置。页面和自动轮询
// 都只处理这个默认源；历史数据库中的其它源/报价仍保留，便于追溯。
const MARKETPLACE_PRODUCT_ID: &str = "chatgpt-plus";
const MARKETPLACE_SOURCE_ID: &str = "default-chatgpt-plus";
const MARKETPLACE_DEFAULT_TAGS_JSON: &str = "[\"account_verified\"]";
const POLL_INTERVAL: Duration = Duration::from_secs(60 * 60);
const VERIFY_LIMIT: usize = 20;
const PRICEAI_PAGE_LIMIT: usize = 200;
const PRICEAI_MAX_OFFSET: usize = 5_000;
const VERIFY_RESPONSE_LIMIT: u64 = 512 * 1024;

static NOTIFICATION_HANDLER: OnceLock<Mutex<Option<Box<dyn Fn(String) + Send + Sync>>>> =
    OnceLock::new();
static POLLING_STARTED: OnceLock<()> = OnceLock::new();
static REFRESH_LOCK: Mutex<()> = Mutex::new(());

fn notification_handler() -> &'static Mutex<Option<Box<dyn Fn(String) + Send + Sync>>> {
    NOTIFICATION_HANDLER.get_or_init(|| Mutex::new(None))
}

pub fn set_notification_handler<F>(handler: F)
where
    F: Fn(String) + Send + Sync + 'static,
{
    *notification_handler()
        .lock()
        .unwrap_or_else(|e| e.into_inner()) = Some(Box::new(handler));
}

pub fn ensure_polling() {
    POLLING_STARTED.get_or_init(|| {
        thread::spawn(|| {
            let _ = refresh(true);
            loop {
                thread::sleep(POLL_INTERVAL);
                let _ = refresh(true);
            }
        });
    });
}

fn storage() -> Result<crate::storage_helpers::StorageHandle, String> {
    crate::storage_helpers::open_storage().ok_or_else(|| "storage unavailable".to_string())
}

fn as_str(value: &Value) -> Option<String> {
    value
        .as_str()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
}

fn as_f64(value: &Value) -> Option<f64> {
    value
        .as_f64()
        .or_else(|| value.as_str().and_then(|raw| raw.parse().ok()))
}

fn as_i64(value: &Value) -> Option<i64> {
    value
        .as_i64()
        .or_else(|| value.as_str().and_then(|raw| raw.parse().ok()))
}

fn tags_from_json(raw: &str) -> Vec<String> {
    serde_json::from_str::<Vec<String>>(raw).unwrap_or_default()
}

fn fetch_source(
    client: &Client,
    source: &MarketplaceSource,
    tags: &[String],
) -> Result<Vec<Value>, String> {
    let joined_tags = tags.join(",");
    let mut all = Vec::new();
    let mut seen_ids = HashSet::new();
    let requested_tags = tags.iter().map(String::as_str).collect::<HashSet<_>>();
    let mut expected_total = None;
    let mut offset = 0_usize;
    loop {
        if offset > PRICEAI_MAX_OFFSET {
            return Err(format!(
                "PriceAI 下一页 offset {offset} 超过公开接口上限，已拒绝保存不完整快照"
            ));
        }
        let mut request = client
            .get(format!(
                "{PRICEAI_BASE}/api/products/{}/offers",
                source.product_id
            ))
            .query(&[("limit", PRICEAI_PAGE_LIMIT)])
            .query(&[("offset", offset)]);
        if !joined_tags.is_empty() {
            request = request.query(&[("tags", joined_tags.as_str())]);
        }
        let payload: Value = request
            .send()
            .map_err(|e| format!("PriceAI 请求失败: {e}"))?
            .error_for_status()
            .map_err(|e| format!("PriceAI HTTP 错误: {e}"))?
            .json()
            .map_err(|e| format!("PriceAI 响应解析失败: {e}"))?;
        if payload.get("degraded").and_then(Value::as_bool) == Some(true) {
            return Err("PriceAI 返回降级数据，已拒绝保存不完整快照".to_string());
        }
        let active_tags = payload
            .get("activeFilterTags")
            .and_then(Value::as_array)
            .ok_or_else(|| "PriceAI 响应缺少 activeFilterTags，无法确认抓取范围".to_string())?
            .iter()
            .filter_map(Value::as_str)
            .collect::<HashSet<_>>();
        if active_tags != requested_tags {
            return Err(format!(
                "PriceAI 实际标签范围与请求不一致（请求 {} 个，返回 {} 个），已拒绝保存",
                requested_tags.len(),
                active_tags.len()
            ));
        }
        let page = payload
            .get("offers")
            .and_then(Value::as_array)
            .cloned()
            .ok_or_else(|| "PriceAI 响应缺少 offers 数组，已拒绝保存不完整快照".to_string())?;
        let count = page.len();
        let total = payload
            .get("total")
            .and_then(Value::as_u64)
            .and_then(|value| usize::try_from(value).ok())
            .ok_or_else(|| "PriceAI 响应缺少 total，已拒绝保存不完整快照".to_string())?;
        match expected_total {
            Some(expected) if expected != total => {
                return Err(format!(
                    "PriceAI 分页期间 total 从 {expected} 变为 {total}，已拒绝保存不稳定快照"
                ));
            }
            None => expected_total = Some(total),
            _ => {}
        }
        for offer in page {
            let offer_id = as_str(offer.get("id").unwrap_or(&Value::Null))
                .ok_or_else(|| "PriceAI 报价缺少 id，已拒绝保存不完整快照".to_string())?;
            if !seen_ids.insert(offer_id.clone()) {
                return Err(format!(
                    "PriceAI 分页重复返回报价 {offer_id}，已拒绝保存不完整快照"
                ));
            }
            all.push(offer);
        }
        offset += count;
        if offset >= total {
            break;
        }
        if count == 0 {
            return Err(format!(
                "PriceAI 在 offset {offset} 提前返回空页，已拒绝保存不完整快照"
            ));
        }
    }
    let total = expected_total.unwrap_or(0);
    if all.len() != total {
        return Err(format!(
            "PriceAI 完整性校验失败：期望 {total} 条，实际得到 {} 条",
            all.len()
        ));
    }
    Ok(all)
}

fn offer_input(
    source: &MarketplaceSource,
    value: &Value,
    local_status: &str,
    checked_at: Option<i64>,
    local_error: Option<String>,
    sync_id: i64,
) -> Option<MarketplaceOfferInput> {
    let offer_id = as_str(value.get("id")?)?;
    let offer_key = format!("{}:{offer_id}", source.id);
    let tags = value.get("tags").cloned().unwrap_or_else(|| json!([]));
    let filter_tags = value
        .get("filterTags")
        .cloned()
        .unwrap_or_else(|| json!([]));
    let priceai_updated_at = ["verifiedAt", "lastSeenAt", "capturedAt", "sourceUpdatedAt"]
        .iter()
        .find_map(|key| as_str(value.get(*key).unwrap_or(&Value::Null)));
    Some(MarketplaceOfferInput {
        offer_key,
        source_config_id: source.id.clone(),
        offer_id,
        product_id: source.product_id.clone(),
        source_id: as_str(value.get("sourceId").unwrap_or(&Value::Null)),
        source_name: as_str(value.get("sourceStoreName").unwrap_or(&Value::Null))
            .or_else(|| as_str(value.get("sourceName").unwrap_or(&Value::Null))),
        source_included_at: as_str(value.get("sourceIncludedAt").unwrap_or(&Value::Null)),
        source_shop_created_at: as_str(value.get("sourceShopCreatedAt").unwrap_or(&Value::Null)),
        collector_kind: as_str(value.get("collectorKind").unwrap_or(&Value::Null)),
        title: as_str(value.get("sourceTitle").unwrap_or(&Value::Null))
            .or_else(|| as_str(value.get("title").unwrap_or(&Value::Null))),
        price: value.get("price").and_then(as_f64),
        listed_price: value.get("listedPrice").and_then(as_f64),
        currency: as_str(value.get("currency").unwrap_or(&Value::Null))
            .unwrap_or_else(|| "CNY".to_string()),
        raw_status: as_str(value.get("status").unwrap_or(&Value::Null)),
        effective_status: as_str(value.get("effectiveStatus").unwrap_or(&Value::Null)),
        freshness_status: as_str(value.get("freshnessStatus").unwrap_or(&Value::Null)),
        priceai_updated_at,
        expires_at: as_str(value.get("expiresAt").unwrap_or(&Value::Null)),
        url: as_str(value.get("url").unwrap_or(&Value::Null)),
        tags_json: tags.to_string(),
        filter_tags_json: filter_tags.to_string(),
        stock_count: value.get("stockCount").and_then(as_i64),
        raw_json: value.to_string(),
        local_status: local_status.to_string(),
        local_checked_at: checked_at,
        local_error,
        last_seen_sync_id: sync_id,
    })
}

fn status_text(value: Option<&Value>) -> String {
    value
        .and_then(|value| {
            value
                .as_str()
                .map(ToOwned::to_owned)
                .or_else(|| value.as_i64().map(|value| value.to_string()))
                .or_else(|| value.as_bool().map(|value| value.to_string()))
        })
        .unwrap_or_default()
}

fn is_unavailable_status(value: &str) -> bool {
    matches!(
        value.trim().to_ascii_lowercase().as_str(),
        "0" | "false" | "unavailable" | "offline" | "hidden" | "disabled"
    )
}

fn is_available_status(value: &str) -> bool {
    matches!(
        value.trim().to_ascii_lowercase().as_str(),
        "1" | "true" | "available" | "online" | "上架"
    )
}

fn is_removed_message(value: &str) -> bool {
    [
        "商品未上架",
        "商品暂未上架",
        "暂未上架",
        "已下架",
        "商品不存在",
        "商品已删除",
        "已删除",
        "停售",
        "not found",
    ]
    .iter()
    .any(|marker| value.to_ascii_lowercase().contains(marker))
}

fn is_shop_closed_message(value: &str) -> bool {
    [
        "店铺已打烊",
        "店铺打烊",
        "已打烊",
        "暂停营业",
        "停止营业",
        "暂不营业",
    ]
    .iter()
    .any(|marker| value.contains(marker))
}

fn item_id_from_url(parsed: &url::Url) -> Option<String> {
    let from_path = parsed.path_segments().and_then(|segments| {
        let segments = segments.collect::<Vec<_>>();
        segments
            .windows(2)
            .find(|pair| pair[0].eq_ignore_ascii_case("item"))
            .map(|pair| pair[1].to_string())
    });
    from_path
        .or_else(|| {
            parsed
                .query_pairs()
                .find(|(key, _)| key == "commodity")
                .map(|(_, value)| value.into_owned())
        })
        .or_else(|| {
            parsed
                .query_pairs()
                .find(|(key, _)| key == "id")
                .map(|(_, value)| value.into_owned())
        })
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn read_limited_text(response: reqwest::blocking::Response) -> Result<String, String> {
    let mut body = String::new();
    response
        .take(VERIFY_RESPONSE_LIMIT)
        .read_to_string(&mut body)
        .map_err(|error| error.to_string())?;
    Ok(body)
}

fn has_positive_stock_text(body: &str) -> bool {
    ["库存"]
        .iter()
        .filter_map(|marker| body.find(marker).map(|index| (marker, index)))
        .any(|(marker, index)| {
            let tail = &body[index + marker.len()..];
            let digits = tail
                .chars()
                .skip_while(|value| !value.is_ascii_digit())
                .take_while(char::is_ascii_digit)
                .collect::<String>();
            digits.parse::<u64>().map_or(false, |value| value > 0)
        })
}

fn has_zero_stock_text(body: &str) -> bool {
    ["库存"]
        .iter()
        .filter_map(|marker| body.find(marker).map(|index| (marker, index)))
        .any(|(marker, index)| {
            let tail = &body[index + marker.len()..];
            let digits = tail
                .chars()
                .skip_while(|value| !value.is_ascii_digit())
                .take_while(char::is_ascii_digit)
                .collect::<String>();
            digits.parse::<u64>() == Ok(0)
        })
}

fn classify_generic_body(body: &str, status_code: u16) -> (String, Option<String>) {
    let lower = body.to_lowercase();
    if [
        "下架",
        "售罄",
        "缺货",
        "库存不足",
        "sold out",
        "out of stock",
        "out_of_stock",
        r#""is_available":false"#,
        r#""stock":0"#,
        r#""stock_count":0"#,
    ]
    .iter()
    .any(|marker| lower.contains(marker))
        || has_zero_stock_text(&lower)
    {
        return (
            "unavailable".to_string(),
            Some("页面明确标记缺货或下架".to_string()),
        );
    }
    if [
        "有货",
        "现货",
        "in stock",
        "available now",
        r#""is_available":true"#,
    ]
    .iter()
    .any(|marker| lower.contains(marker))
        || has_positive_stock_text(&lower)
    {
        return ("available".to_string(), None);
    }
    (
        "unknown".to_string(),
        Some(format!("HTTP {status_code}，页面未明确库存")),
    )
}

fn verify_shop_api(
    client: &Client,
    parsed: &url::Url,
    goods_key: &str,
) -> (String, Option<String>) {
    let base_url = parsed.origin().ascii_serialization();
    let response = client
        .post(format!("{base_url}/shopApi/Shop/goodsInfo"))
        .header("accept", "application/json, text/plain, */*")
        .header("origin", &base_url)
        .header("referer", parsed.as_str())
        .json(&json!({"goods_key": goods_key, "trade_no": ""}))
        .send();
    let response = match response {
        Ok(response) => response,
        Err(error) => return ("unknown".to_string(), Some(error.to_string())),
    };
    let http_status = response.status().as_u16();
    if matches!(http_status, 404 | 410) {
        return ("invalid".to_string(), Some(format!("HTTP {http_status}")));
    }
    if !response.status().is_success() {
        return ("unknown".to_string(), Some(format!("HTTP {http_status}")));
    }
    let body = match read_limited_text(response) {
        Ok(body) => body,
        Err(error) => return ("unknown".to_string(), Some(error)),
    };
    let payload: Value = match serde_json::from_str(&body) {
        Ok(payload) => payload,
        Err(_) => return classify_generic_body(&body, http_status),
    };
    let message = as_str(payload.get("msg").unwrap_or(&Value::Null))
        .or_else(|| as_str(payload.get("message").unwrap_or(&Value::Null)))
        .unwrap_or_default();
    let Some(data) = payload.get("data").and_then(Value::as_object) else {
        if is_shop_closed_message(&message) {
            return ("unavailable".to_string(), Some(message));
        }
        if is_removed_message(&message) {
            return ("invalid".to_string(), Some(message));
        }
        return (
            "unknown".to_string(),
            Some(if message.is_empty() {
                "商品接口未返回商品详情".to_string()
            } else {
                message
            }),
        );
    };
    let item_status = status_text(data.get("status").or_else(|| data.get("state")));
    if is_unavailable_status(&item_status) || is_removed_message(&message) {
        return (
            "invalid".to_string(),
            Some(if message.is_empty() {
                format!("商品状态为 {item_status}")
            } else {
                message
            }),
        );
    }
    let stock = data
        .get("extend")
        .and_then(Value::as_object)
        .and_then(|extend| extend.get("stock_count"))
        .and_then(as_i64)
        .or_else(|| data.get("stock_count").and_then(as_i64))
        .or_else(|| data.get("stockCount").and_then(as_i64))
        .or_else(|| data.get("stock").and_then(as_i64))
        .or_else(|| data.get("inventory").and_then(as_i64));
    if stock == Some(0) {
        return ("unavailable".to_string(), Some("商品库存为 0".to_string()));
    }
    if stock.is_some_and(|value| value > 0) {
        return ("available".to_string(), None);
    }
    (
        "unknown".to_string(),
        Some(if is_available_status(&item_status) {
            "商品已上架，但接口未返回明确库存".to_string()
        } else {
            "商品接口返回了详情，但库存字段不明确".to_string()
        }),
    )
}

fn verify_kami(client: &Client, parsed: &url::Url, item_id: &str) -> (String, Option<String>) {
    let base_url = parsed.origin().ascii_serialization();
    for page in 1..=10 {
        let response = client
            .get(format!(
                "{base_url}/user/api/index/commodity?limit=100&page={page}"
            ))
            .header("accept", "application/json, text/plain, */*")
            .send();
        let response = match response {
            Ok(response) => response,
            Err(error) => return ("unknown".to_string(), Some(error.to_string())),
        };
        let http_status = response.status().as_u16();
        if matches!(http_status, 403 | 429) {
            return ("unknown".to_string(), Some(format!("HTTP {http_status}")));
        }
        if matches!(http_status, 404 | 410) {
            return ("invalid".to_string(), Some(format!("HTTP {http_status}")));
        }
        if !response.status().is_success() {
            return ("unknown".to_string(), Some(format!("HTTP {http_status}")));
        }
        let body = match read_limited_text(response) {
            Ok(body) => body,
            Err(error) => return ("unknown".to_string(), Some(error)),
        };
        let payload: Value = match serde_json::from_str(&body) {
            Ok(payload) => payload,
            Err(error) => return ("unknown".to_string(), Some(error.to_string())),
        };
        let items = payload
            .get("data")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        if let Some(item) = items
            .iter()
            .find(|item| status_text(item.get("id")) == item_id)
        {
            let hidden = item
                .get("hide")
                .and_then(|value| {
                    value
                        .as_bool()
                        .or_else(|| as_i64(value).map(|value| value != 0))
                })
                .unwrap_or(false);
            let item_status = status_text(item.get("status"));
            if hidden || is_unavailable_status(&item_status) {
                return ("invalid".to_string(), Some("商品已隐藏或停用".to_string()));
            }
            let stock = item
                .get("stock")
                .and_then(as_i64)
                .or_else(|| item.get("inventory").and_then(as_i64));
            if stock == Some(0) {
                return ("unavailable".to_string(), Some("商品库存为 0".to_string()));
            }
            if stock.is_some_and(|value| value > 0) {
                return ("available".to_string(), None);
            }
            return (
                "unknown".to_string(),
                Some("商品仍在列表中，但接口未返回明确库存".to_string()),
            );
        }
        if items.len() < 100 {
            break;
        }
    }
    (
        "invalid".to_string(),
        Some("商品列表已不再返回该商品".to_string()),
    )
}

fn host_is_private(parsed: &url::Url) -> bool {
    let Some(host) = parsed.host_str() else {
        return true;
    };
    let host = host.trim_end_matches('.');
    if host.eq_ignore_ascii_case("localhost") || host.ends_with(".localhost") {
        return true;
    }
    host.parse::<IpAddr>()
        .map_or(false, |address| match address {
            IpAddr::V4(address) => private_ipv4(address),
            IpAddr::V6(address) => {
                address.is_loopback()
                    || address.is_unspecified()
                    || address.is_unique_local()
                    || address.to_ipv4_mapped().is_some_and(private_ipv4)
            }
        })
}

fn private_ipv4(address: Ipv4Addr) -> bool {
    address.is_private()
        || address.is_loopback()
        || address.is_link_local()
        || address.is_unspecified()
}

fn verify_url(
    client: &Client,
    url: &str,
    collector_kind: Option<&str>,
) -> (String, Option<String>) {
    let parsed = match url::Url::parse(url) {
        Ok(parsed) if matches!(parsed.scheme(), "http" | "https") => parsed,
        _ => return ("invalid".to_string(), Some("商品链接格式无效".to_string())),
    };
    if host_is_private(&parsed) {
        return (
            "unknown".to_string(),
            Some("拒绝验证本地或私有网络地址".to_string()),
        );
    }
    if collector_kind == Some("shopApi") {
        return match item_id_from_url(&parsed) {
            Some(goods_key) => verify_shop_api(client, &parsed, &goods_key),
            None => (
                "unknown".to_string(),
                Some("无法从链接识别 ShopApi 商品编号".to_string()),
            ),
        };
    }
    if collector_kind == Some("kami") {
        return match item_id_from_url(&parsed) {
            Some(item_id) => verify_kami(client, &parsed, &item_id),
            None => (
                "unknown".to_string(),
                Some("无法从链接识别 Kami 商品编号".to_string()),
            ),
        };
    }
    let response = match client.get(parsed).send() {
        Ok(response) => response,
        Err(error) => return ("unknown".to_string(), Some(error.to_string())),
    };
    let http_status = response.status().as_u16();
    if matches!(http_status, 404 | 410) {
        return ("invalid".to_string(), Some(format!("HTTP {http_status}")));
    }
    if !response.status().is_success() {
        return ("unknown".to_string(), Some(format!("HTTP {http_status}")));
    }
    match read_limited_text(response) {
        Ok(body) => classify_generic_body(&body, http_status),
        Err(error) => ("unknown".to_string(), Some(error)),
    }
}

fn priceai_status(offer: &MarketplaceOffer) -> &'static str {
    if offer.raw_status.as_deref() == Some("out_of_stock") {
        return "out_of_stock";
    }
    if !offer.price.is_some_and(f64::is_finite)
        || offer
            .url
            .as_deref()
            .map(str::trim)
            .unwrap_or_default()
            .is_empty()
    {
        return "invalid";
    }
    if offer.effective_status.as_deref() == Some("failed")
        || offer.freshness_status.as_deref() == Some("failed")
    {
        return "invalid";
    }
    if matches!(
        offer.effective_status.as_deref(),
        Some("unavailable" | "stale")
    ) || offer.freshness_status.as_deref() == Some("expired")
        || offer.expires_at.as_deref().is_some_and(|value| {
            chrono::DateTime::parse_from_rfc3339(value)
                .is_ok_and(|expires_at| expires_at.timestamp() <= chrono::Utc::now().timestamp())
        })
    {
        return "unavailable";
    }
    "available"
}

fn priceai_in_stock(offer: &MarketplaceOffer) -> bool {
    priceai_status(offer) == "available"
}

fn offer_is_available(offer: &MarketplaceOffer) -> bool {
    offer.local_status == "available" || priceai_in_stock(offer)
}

fn merchant_matches_offer(offer: &MarketplaceOffer, merchant: &str) -> bool {
    [
        offer.source_name.as_deref(),
        offer.source_id.as_deref(),
        offer.collector_kind.as_deref(),
    ]
    .into_iter()
    .flatten()
    .any(|candidate| candidate.trim().eq_ignore_ascii_case(merchant))
}

fn raw_tags(raw: &Value) -> Vec<String> {
    let mut tags = raw
        .get("tags")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .map(ToOwned::to_owned)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    tags.extend(
        raw.get("filterTags")
            .and_then(Value::as_array)
            .map(|items| {
                items
                    .iter()
                    .filter_map(Value::as_str)
                    .map(ToOwned::to_owned)
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default(),
    );
    tags
}

fn offer_matches_source_merchant(raw: &Value, merchant: Option<&str>) -> bool {
    let Some(merchant) = merchant.map(str::trim).filter(|value| !value.is_empty()) else {
        return true;
    };
    ["sourceStoreName", "sourceName", "sourceId", "collectorKind"]
        .iter()
        .filter_map(|key| as_str(raw.get(*key).unwrap_or(&Value::Null)))
        .any(|candidate| candidate.eq_ignore_ascii_case(merchant))
}

fn rule_wants_verification(
    rule: &MarketplaceAlertRule,
    source: &MarketplaceSource,
    raw: &Value,
) -> bool {
    if !rule.enabled
        || rule
            .product_id
            .as_deref()
            .is_some_and(|value| value != source.product_id)
        || rule
            .source_config_id
            .as_deref()
            .is_some_and(|value| value != source.id)
        || rule.currency
            != as_str(raw.get("currency").unwrap_or(&Value::Null))
                .unwrap_or_else(|| "CNY".to_string())
        || rule
            .merchant
            .as_deref()
            .is_some_and(|merchant| !offer_matches_source_merchant(raw, Some(merchant)))
    {
        return false;
    }
    if rule.max_price.is_some_and(|max_price| {
        raw.get("price")
            .and_then(as_f64)
            .map_or(true, |price| price > max_price)
    }) {
        return false;
    }
    let tags = raw_tags(raw);
    tags_from_json(&rule.tags_json)
        .iter()
        .all(|tag| tags.iter().any(|candidate| candidate == tag))
}

fn rule_scope_matches(rule: &MarketplaceAlertRule, offer: &MarketplaceOffer) -> bool {
    if !rule.enabled
        || rule.currency != offer.currency
        || rule
            .product_id
            .as_deref()
            .is_some_and(|value| value != offer.product_id)
        || rule
            .source_config_id
            .as_deref()
            .is_some_and(|value| value != offer.source_config_id)
        || rule
            .merchant
            .as_deref()
            .is_some_and(|merchant| !merchant_matches_offer(offer, merchant))
    {
        return false;
    }
    let mut tags = tags_from_json(&offer.tags_json);
    tags.extend(tags_from_json(&offer.filter_tags_json));
    tags_from_json(&rule.tags_json)
        .iter()
        .all(|tag| tags.iter().any(|candidate| candidate == tag))
}

fn drop_threshold_matches(
    rule: &MarketplaceAlertRule,
    offer: &MarketplaceOffer,
    previous: Option<&MarketplaceOffer>,
) -> bool {
    let has_drop_threshold = rule.drop_amount.is_some() || rule.drop_percent.is_some();
    if !has_drop_threshold {
        return true;
    }
    let (Some(previous_price), Some(price)) = (previous.and_then(|value| value.price), offer.price)
    else {
        return false;
    };
    if rule
        .drop_amount
        .is_some_and(|drop_amount| previous_price - price < drop_amount)
    {
        return false;
    }
    if rule.drop_percent.is_some_and(|drop_percent| {
        previous_price <= 0.0 || (previous_price - price) / previous_price * 100.0 < drop_percent
    }) {
        return false;
    }
    true
}

fn rule_active(
    rule: &MarketplaceAlertRule,
    offer: &MarketplaceOffer,
    previous: Option<&MarketplaceOffer>,
) -> bool {
    if !rule_scope_matches(rule, offer) {
        return false;
    }
    let under_price_ceiling = rule.max_price.map_or(true, |max_price| {
        offer.price.is_some_and(|price| price <= max_price)
    });
    let has_price_condition =
        rule.max_price.is_some() || rule.drop_amount.is_some() || rule.drop_percent.is_some();
    let price_active = has_price_condition
        && offer_is_available(offer)
        && under_price_ceiling
        && drop_threshold_matches(rule, offer, previous);
    let restocked = previous.is_some_and(|old| !priceai_in_stock(old) && priceai_in_stock(offer));
    let verification_changed = previous.is_some_and(|old| old.local_status != offer.local_status);
    price_active
        || (rule.notify_restock && restocked && under_price_ceiling)
        || (rule.notify_verified && verification_changed && under_price_ceiling)
        || (rule.notify_invalid_link && offer.local_status == "invalid")
}

fn rule_signature(rule: &MarketplaceAlertRule, offer: &MarketplaceOffer) -> String {
    let mut parts = Vec::new();
    if rule.max_price.is_some() || rule.drop_amount.is_some() || rule.drop_percent.is_some() {
        parts.push(format!(
            "price:{}",
            offer
                .price
                .map(|value| format!("{value:.4}"))
                .unwrap_or_default()
        ));
    }
    if rule.notify_restock {
        parts.push(format!("stock:{}", priceai_in_stock(offer)));
    }
    if rule.notify_verified {
        parts.push(format!("local:{}", offer.local_status));
    }
    if rule.notify_invalid_link {
        parts.push(format!("invalid:{}", offer.local_status == "invalid"));
    }
    parts.join("|")
}

fn process_rules(
    storage: &Storage,
    rules: &[MarketplaceAlertRule],
    offer: &MarketplaceOffer,
    previous: Option<&MarketplaceOffer>,
    notify: bool,
    notifications: &mut Vec<String>,
) -> Result<(), String> {
    for rule in rules {
        let active = rule_active(rule, offer, previous);
        let signature = rule_signature(rule, offer);
        let state = storage
            .marketplace_alert_state_get(&rule.id, &offer.offer_key)
            .map_err(|error| error.to_string())?;
        let should_notify = state.as_ref().is_some_and(|previous_state| {
            notify
                && active
                && (!previous_state.condition_active || previous_state.signature != signature)
        });
        storage
            .marketplace_alert_state_put(&MarketplaceAlertState {
                rule_id: rule.id.clone(),
                offer_key: offer.offer_key.clone(),
                signature,
                condition_active: active,
                baseline_ready: true,
                updated_at: now_ts(),
            })
            .map_err(|error| error.to_string())?;
        if should_notify {
            notifications.push(format!(
                "{}: {}",
                rule.name,
                offer.title.as_deref().unwrap_or(&offer.offer_key)
            ));
        }
    }
    Ok(())
}

fn marketplace_client() -> Result<Client, String> {
    Client::builder()
        .timeout(Duration::from_secs(15))
        .user_agent("CodexManager/marketplace")
        .redirect(Policy::custom(|attempt| {
            if host_is_private(attempt.url()) {
                attempt.stop()
            } else {
                attempt.follow()
            }
        }))
        .build()
        .map_err(|error| error.to_string())
}

fn record_offer_changes(
    storage: &Storage,
    previous: &MarketplaceOffer,
    current: &MarketplaceOffer,
) -> Result<(), String> {
    let key = &current.offer_key;
    if previous.price != current.price {
        storage
            .marketplace_change_insert(
                key,
                "price",
                &json!({"before":previous.price,"after":current.price}).to_string(),
            )
            .map_err(|error| error.to_string())?;
    }
    if previous.raw_status != current.raw_status
        || previous.effective_status != current.effective_status
        || previous.freshness_status != current.freshness_status
        || previous.expires_at != current.expires_at
        || previous.stock_count != current.stock_count
    {
        storage
            .marketplace_change_insert(
                key,
                "priceai_status",
                &json!({
                    "before": priceai_status(previous),
                    "after": priceai_status(current),
                    "rawBefore": previous.raw_status,
                    "rawAfter": current.raw_status,
                    "effectiveBefore": previous.effective_status,
                    "effectiveAfter": current.effective_status,
                    "stockBefore": previous.stock_count,
                    "stockAfter": current.stock_count
                })
                .to_string(),
            )
            .map_err(|error| error.to_string())?;
    }
    if !priceai_in_stock(previous) && priceai_in_stock(current) {
        storage
            .marketplace_change_insert(key, "restock", &json!({}).to_string())
            .map_err(|error| error.to_string())?;
    }
    if previous.local_status != current.local_status {
        storage
            .marketplace_change_insert(
                key,
                "verification",
                &json!({"before":previous.local_status,"after":current.local_status}).to_string(),
            )
            .map_err(|error| error.to_string())?;
    }
    if previous.local_status != "invalid" && current.local_status == "invalid" {
        storage
            .marketplace_change_insert(
                key,
                "invalid_link",
                &json!({"reason":current.local_error}).to_string(),
            )
            .map_err(|error| error.to_string())?;
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum MarketplaceSyncMode {
    Automatic,
    Manual,
}

impl MarketplaceSyncMode {
    fn is_automatic(self) -> bool {
        self == Self::Automatic
    }

    fn commits_full_snapshot(self) -> bool {
        self == Self::Manual
    }
}

fn sync_source(
    storage: &Storage,
    client: &Client,
    source: &MarketplaceSource,
    rules: &[MarketplaceAlertRule],
    tags: &[String],
    mode: MarketplaceSyncMode,
    notifications: &mut Vec<String>,
) -> Result<usize, String> {
    let automatic = mode.is_automatic();
    // Automatic tag syncs update only the returned subset. Reusing the last
    // committed full-snapshot marker keeps offers outside that tag scope current.
    let sync_id = if mode.commits_full_snapshot() {
        chrono::Utc::now()
            .timestamp_millis()
            .max(source.last_successful_sync_id.saturating_add(1))
    } else {
        source.last_successful_sync_id
    };
    let mut offers = fetch_source(client, source, tags)?;
    offers.sort_by(|left, right| {
        match (
            left.get("price").and_then(as_f64),
            right.get("price").and_then(as_f64),
        ) {
            (Some(left), Some(right)) => left.total_cmp(&right),
            (Some(_), None) => std::cmp::Ordering::Less,
            (None, Some(_)) => std::cmp::Ordering::Greater,
            (None, None) => std::cmp::Ordering::Equal,
        }
    });

    let mut count = 0_usize;
    for (index, raw) in offers.into_iter().enumerate() {
        let offer_id = as_str(raw.get("id").unwrap_or(&Value::Null))
            .ok_or_else(|| "PriceAI 报价缺少 id，已拒绝保存不完整快照".to_string())?;
        let key = format!("{}:{offer_id}", source.id);
        let previous = storage
            .marketplace_offer_by_key(&key)
            .map_err(|error| error.to_string())?;
        let rule_match_hint = automatic
            && rules
                .iter()
                .any(|rule| rule_wants_verification(rule, source, &raw));
        let should_verify =
            automatic && source.verify_enabled && (index < VERIFY_LIMIT || rule_match_hint);
        let (local_status, checked_at, local_error) = if should_verify {
            thread::sleep(Duration::from_millis(120));
            match as_str(raw.get("url").unwrap_or(&Value::Null)) {
                Some(url) => {
                    let (status, error) = verify_url(
                        client,
                        &url,
                        as_str(raw.get("collectorKind").unwrap_or(&Value::Null)).as_deref(),
                    );
                    (status, Some(now_ts()), error)
                }
                None => (
                    "invalid".to_string(),
                    Some(now_ts()),
                    Some("缺少商品链接".to_string()),
                ),
            }
        } else {
            (
                previous
                    .as_ref()
                    .map(|offer| offer.local_status.clone())
                    .unwrap_or_else(|| "unknown".to_string()),
                previous.as_ref().and_then(|offer| offer.local_checked_at),
                previous
                    .as_ref()
                    .and_then(|offer| offer.local_error.clone()),
            )
        };
        let input = offer_input(
            source,
            &raw,
            &local_status,
            checked_at,
            local_error,
            sync_id,
        )
        .ok_or_else(|| "PriceAI 报价字段不完整，已拒绝保存不完整快照".to_string())?;
        let old = storage
            .marketplace_offer_upsert(&input)
            .map_err(|error| error.to_string())?;
        let current = storage
            .marketplace_offer_by_key(&key)
            .map_err(|error| error.to_string())?
            .ok_or_else(|| "报价写入后读取失败".to_string())?;
        if let Some(previous) = old.as_ref() {
            record_offer_changes(storage, previous, &current)?;
        }
        process_rules(
            storage,
            rules,
            &current,
            old.as_ref(),
            automatic,
            notifications,
        )?;
        count += 1;
    }
    if mode.commits_full_snapshot() {
        storage
            .marketplace_source_sync_succeeded(&source.id, sync_id)
            .map_err(|error| error.to_string())?;
    } else {
        storage
            .marketplace_source_partial_sync_succeeded(&source.id)
            .map_err(|error| error.to_string())?;
    }
    Ok(count)
}

fn ensure_default_source(storage: &Storage) -> Result<MarketplaceSource, String> {
    let existing = storage
        .marketplace_sources()
        .map_err(|error| error.to_string())?
        .into_iter()
        .find(|source| source.id == MARKETPLACE_SOURCE_ID);
    let Some(existing) = existing else {
        return storage
            .marketplace_source_upsert(&MarketplaceSourceInput {
                id: MARKETPLACE_SOURCE_ID.to_string(),
                product_id: MARKETPLACE_PRODUCT_ID.to_string(),
                tags_json: MARKETPLACE_DEFAULT_TAGS_JSON.to_string(),
                merchant: None,
                enabled: true,
                verify_enabled: true,
            })
            .map_err(|error| error.to_string());
    };

    if existing.product_id == MARKETPLACE_PRODUCT_ID && existing.merchant.is_none() {
        return Ok(existing);
    }

    // Product and merchant scope are code-owned. The three scheduler settings
    // remain user-owned and must survive service restarts and source listing.
    storage
        .marketplace_source_upsert(&MarketplaceSourceInput {
            id: MARKETPLACE_SOURCE_ID.to_string(),
            product_id: MARKETPLACE_PRODUCT_ID.to_string(),
            tags_json: existing.tags_json,
            merchant: None,
            enabled: existing.enabled,
            verify_enabled: existing.verify_enabled,
        })
        .map_err(|error| error.to_string())
}

pub fn refresh(automatic: bool) -> Result<Value, String> {
    let _refresh_guard = REFRESH_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let storage = storage()?;
    let source = ensure_default_source(&storage)?;
    let mode = if automatic {
        MarketplaceSyncMode::Automatic
    } else {
        MarketplaceSyncMode::Manual
    };
    if mode.is_automatic() && !source.enabled {
        return Ok(json!({
            "count": 0,
            "notified": 0,
            "errors": [],
            "skipped": true,
            "completedAt": now_ts()
        }));
    }
    let tags = if mode.is_automatic() {
        serde_json::from_str::<Vec<String>>(&source.tags_json)
            .map_err(|_| "定时同步标签配置损坏，请重新保存商品池设置".to_string())?
    } else {
        // Manual refresh is the only full reconciliation. It never carries the
        // scheduled tag scope, even when automatic sync is disabled.
        Vec::new()
    };
    let rules = storage
        .marketplace_rules()
        .map_err(|error| error.to_string())?;
    let client = marketplace_client()?;
    let mut count = 0_usize;
    let mut notifications = Vec::new();
    let mut errors = Vec::new();
    match sync_source(
        &storage,
        &client,
        &source,
        &rules,
        &tags,
        mode,
        &mut notifications,
    ) {
        Ok(source_count) => count += source_count,
        Err(error) => {
            log::warn!("marketplace source {} failed: {}", source.id, error);
            if let Err(storage_error) = storage.marketplace_source_sync_failed(&source.id, &error) {
                log::warn!(
                    "marketplace source {} failure state could not be saved: {}",
                    source.id,
                    storage_error
                );
            }
            errors.push(format!("{}: {error}", source.id));
        }
    }
    let desktop_notifications = storage
        .marketplace_setting_get("desktop_notifications")
        .map_err(|e| e.to_string())?
        .as_deref()
        == Some("true");
    if automatic && desktop_notifications && !notifications.is_empty() {
        let summary = format!(
            "商品池有 {} 条提醒：{}",
            notifications.len(),
            notifications
                .iter()
                .take(5)
                .cloned()
                .collect::<Vec<_>>()
                .join("；")
        );
        if let Some(handler) = notification_handler()
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .as_ref()
        {
            handler(summary);
        }
    }
    Ok(
        json!({"count": count, "notified": notifications.len(), "errors": errors, "completedAt": now_ts()}),
    )
}

pub fn list_sources() -> Result<Vec<MarketplaceSource>, String> {
    let handle = storage()?;
    let source = ensure_default_source(&handle)?;
    Ok(vec![source])
}

fn normalize_tags_json(raw: &str) -> Result<String, String> {
    let values =
        serde_json::from_str::<Vec<String>>(raw).map_err(|_| "标签必须是字符串数组".to_string())?;
    let mut normalized = Vec::new();
    for value in values {
        let value = value.trim();
        if !value.is_empty() && !normalized.iter().any(|existing| existing == value) {
            normalized.push(value.to_string());
        }
    }
    serde_json::to_string(&normalized).map_err(|error| error.to_string())
}

fn normalize_optional(value: Option<String>) -> Option<String> {
    value
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn trimmed(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|value| !value.is_empty())
}

fn marketplace_merchant_key(offer: &MarketplaceOffer) -> Option<String> {
    if let (Some(collector), Some(source_id)) = (
        trimmed(offer.collector_kind.as_deref()),
        trimmed(offer.source_id.as_deref()),
    ) {
        return Some(format!(
            "source:{}:{source_id}",
            collector.to_ascii_lowercase()
        ));
    }

    let source_name = trimmed(offer.source_name.as_deref());
    let origin = offer.url.as_deref().and_then(|value| {
        let parsed = url::Url::parse(value).ok()?;
        if !matches!(parsed.scheme(), "http" | "https") || parsed.host_str().is_none() {
            return None;
        }
        Some(parsed.origin().ascii_serialization().to_ascii_lowercase())
    });

    // Some PriceAI rows omit sourceId. Pairing the origin with the merchant name
    // avoids merging different stores that share a hosted storefront domain.
    origin
        .zip(source_name)
        .map(|(origin, source_name)| format!("fallback:{origin}:{source_name}"))
}

fn attach_merchant_key(mut offer: MarketplaceOffer) -> MarketplaceOffer {
    offer.merchant_key = marketplace_merchant_key(&offer);
    offer
}

pub fn upsert_source(mut input: MarketplaceSourceInput) -> Result<MarketplaceSource, String> {
    input.id = input.id.trim().to_string();
    input.product_id = input.product_id.trim().to_string();
    if input.id != MARKETPLACE_SOURCE_ID || input.product_id != MARKETPLACE_PRODUCT_ID {
        return Err("商品池产品固定为 ChatGPT Plus 试用订阅".to_string());
    }
    input.tags_json = normalize_tags_json(&input.tags_json)?;
    input.merchant = None;
    storage()?
        .marketplace_source_upsert(&input)
        .map_err(|error| error.to_string())
}

pub fn delete_source(id: &str) -> Result<(), String> {
    let id = id.trim();
    if id.is_empty() {
        return Err("商品源 ID 不能为空".to_string());
    }
    storage()?
        .marketplace_source_delete(id)
        .map_err(|error| error.to_string())
}
pub fn list_offers(
    source: Option<&str>,
    product: Option<&str>,
    limit: i64,
) -> Result<Vec<MarketplaceOffer>, String> {
    let offers = storage()?
        .marketplace_offers(source, product, limit)
        .map_err(|e| e.to_string())?;
    Ok(offers.into_iter().map(attach_merchant_key).collect())
}

pub fn verify_offer(key: &str) -> Result<MarketplaceOffer, String> {
    let key = key.trim();
    if key.is_empty() {
        return Err("商品标识不能为空".to_string());
    }
    let _refresh_guard = REFRESH_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let storage = storage()?;
    let previous = storage
        .marketplace_offer_by_key(key)
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "商品不存在".to_string())?;
    let client = marketplace_client()?;
    let (local_status, local_error) = match previous.url.as_deref() {
        Some(url) => verify_url(&client, url, previous.collector_kind.as_deref()),
        None => ("invalid".to_string(), Some("缺少商品链接".to_string())),
    };
    let current = storage
        .marketplace_offer_verification_update(key, &local_status, local_error.as_deref())
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "验证结果写入后读取失败".to_string())?;
    record_offer_changes(&storage, &previous, &current)?;

    let rules = storage
        .marketplace_rules()
        .map_err(|error| error.to_string())?;
    let mut ignored_notifications = Vec::new();
    process_rules(
        &storage,
        &rules,
        &current,
        Some(&previous),
        false,
        &mut ignored_notifications,
    )?;
    Ok(attach_merchant_key(current))
}

pub fn list_changes(
    limit: i64,
) -> Result<Vec<codexmanager_core::storage::MarketplaceOfferChange>, String> {
    storage()?
        .marketplace_changes(limit)
        .map_err(|e| e.to_string())
}
pub fn list_rules() -> Result<Vec<MarketplaceAlertRule>, String> {
    storage()?.marketplace_rules().map_err(|e| e.to_string())
}

pub fn list_favorite_merchants() -> Result<Vec<MarketplaceFavoriteMerchant>, String> {
    storage()?
        .marketplace_favorite_merchants()
        .map_err(|error| error.to_string())
}

pub fn set_favorite_merchant(
    offer_key: &str,
    favorite: bool,
) -> Result<Vec<MarketplaceFavoriteMerchant>, String> {
    let offer_key = offer_key.trim();
    if offer_key.is_empty() {
        return Err("商品标识不能为空".to_string());
    }
    let storage = storage()?;
    let offer = storage
        .marketplace_offer_by_key(offer_key)
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "商品不存在".to_string())?;
    let merchant_key = marketplace_merchant_key(&offer)
        .ok_or_else(|| "当前商品缺少可用于收藏的商家标识".to_string())?;
    let input = MarketplaceFavoriteMerchantInput {
        merchant_key,
        source_id: normalize_optional(offer.source_id),
        source_name: normalize_optional(offer.source_name),
        collector_kind: normalize_optional(offer.collector_kind),
    };
    storage
        .marketplace_favorite_merchant_set(&input, favorite)
        .map_err(|error| error.to_string())?;
    storage
        .marketplace_favorite_merchants()
        .map_err(|error| error.to_string())
}
pub fn upsert_rule(mut input: MarketplaceAlertRuleInput) -> Result<MarketplaceAlertRule, String> {
    input.id = input.id.trim().to_string();
    input.name = input.name.trim().to_string();
    input.source_config_id = normalize_optional(input.source_config_id);
    input.product_id = normalize_optional(input.product_id);
    input.merchant = normalize_optional(input.merchant);
    input.currency = input.currency.trim().to_ascii_uppercase();
    input.tags_json = normalize_tags_json(&input.tags_json)?;
    if input.id.is_empty() || input.name.is_empty() {
        return Err("提醒规则 ID 和名称不能为空".to_string());
    }
    if input
        .product_id
        .as_deref()
        .is_some_and(|product| product != MARKETPLACE_PRODUCT_ID)
    {
        return Err("不支持的 PriceAI 产品范围".to_string());
    }
    if input.currency.is_empty() || input.currency.len() > 12 {
        return Err("币种不能为空且不能超过 12 个字符".to_string());
    }
    if [input.max_price, input.drop_amount]
        .into_iter()
        .flatten()
        .any(|value| !value.is_finite() || value < 0.0)
        || input
            .drop_percent
            .is_some_and(|value| !value.is_finite() || !(0.0..=100.0).contains(&value))
    {
        return Err("价格阈值必须为有效的非负数，下降比例需在 0 到 100 之间".to_string());
    }
    storage()?
        .marketplace_rule_upsert(&input)
        .map_err(|error| error.to_string())
}

pub fn delete_rule(id: &str) -> Result<(), String> {
    let id = id.trim();
    if id.is_empty() {
        return Err("提醒规则 ID 不能为空".to_string());
    }
    storage()?
        .marketplace_rule_delete(id)
        .map_err(|error| error.to_string())
}
pub fn notification_enabled() -> Result<bool, String> {
    Ok(storage()?
        .marketplace_setting_get("desktop_notifications")
        .map_err(|e| e.to_string())?
        .as_deref()
        == Some("true"))
}
pub fn set_notification_enabled(enabled: bool) -> Result<(), String> {
    storage()?
        .marketplace_setting_set(
            "desktop_notifications",
            if enabled { "true" } else { "false" },
        )
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn offer(
        key: &str,
        price: f64,
        priceai_available: bool,
        local_status: &str,
    ) -> MarketplaceOffer {
        MarketplaceOffer {
            offer_key: key.to_string(),
            source_config_id: "source-1".to_string(),
            offer_id: key.to_string(),
            product_id: "chatgpt-plus".to_string(),
            source_id: Some("merchant-id".to_string()),
            source_name: Some("测试商家".to_string()),
            source_included_at: None,
            source_shop_created_at: None,
            collector_kind: Some("shopApi".to_string()),
            title: Some("测试商品".to_string()),
            price: Some(price),
            listed_price: None,
            currency: "CNY".to_string(),
            raw_status: Some(
                if priceai_available {
                    "in_stock"
                } else {
                    "out_of_stock"
                }
                .to_string(),
            ),
            effective_status: Some(
                if priceai_available {
                    "available"
                } else {
                    "unavailable"
                }
                .to_string(),
            ),
            freshness_status: Some("fresh".to_string()),
            priceai_updated_at: None,
            expires_at: None,
            url: Some("https://example.com/item/1".to_string()),
            tags_json: "[]".to_string(),
            filter_tags_json: "[\"account_verified\"]".to_string(),
            stock_count: None,
            raw_json: "{}".to_string(),
            local_status: local_status.to_string(),
            local_checked_at: Some(1),
            local_error: None,
            first_seen_at: 1,
            last_seen_at: 1,
            updated_at: 1,
            is_current: true,
            merchant_key: None,
        }
    }

    #[test]
    fn merchant_key_uses_stable_source_pair_and_safe_fallback() {
        let first = offer("first", 10.0, true, "unknown");
        let mut second = offer("second", 20.0, true, "unknown");
        second.url = Some("https://example.com/item/2".to_string());
        assert_eq!(
            marketplace_merchant_key(&first),
            marketplace_merchant_key(&second)
        );
        assert_eq!(
            marketplace_merchant_key(&first).as_deref(),
            Some("source:shopapi:merchant-id")
        );

        let mut fallback_first = first.clone();
        fallback_first.source_id = None;
        fallback_first.collector_kind = None;
        fallback_first.source_name = Some("GPT专卖-cw".to_string());
        fallback_first.url = Some("https://caowo.store/item/68".to_string());
        let mut fallback_second = fallback_first.clone();
        fallback_second.url = Some("https://caowo.store/item/69".to_string());
        assert_eq!(
            marketplace_merchant_key(&fallback_first),
            marketplace_merchant_key(&fallback_second)
        );

        let mut different_merchant = fallback_first.clone();
        different_merchant.source_name = Some("另一商家".to_string());
        assert_ne!(
            marketplace_merchant_key(&fallback_first),
            marketplace_merchant_key(&different_merchant)
        );

        fallback_first.source_name = None;
        assert_eq!(marketplace_merchant_key(&fallback_first), None);
    }

    fn rule(id: &str) -> MarketplaceAlertRule {
        MarketplaceAlertRule {
            id: id.to_string(),
            name: "测试提醒".to_string(),
            source_config_id: Some("source-1".to_string()),
            product_id: Some("chatgpt-plus".to_string()),
            tags_json: "[\"account_verified\"]".to_string(),
            merchant: Some("测试商家".to_string()),
            currency: "CNY".to_string(),
            max_price: Some(10.0),
            drop_amount: None,
            drop_percent: None,
            notify_restock: false,
            notify_verified: false,
            notify_invalid_link: false,
            enabled: true,
            created_at: 1,
            updated_at: 1,
        }
    }

    fn initialized_storage() -> Storage {
        let storage = Storage::open_in_memory().expect("open marketplace test storage");
        storage.init().expect("initialize marketplace test storage");
        storage
    }

    #[test]
    fn default_source_repair_preserves_user_scheduler_settings() {
        let storage = initialized_storage();
        let created = ensure_default_source(&storage).expect("create default source");
        assert_eq!(created.tags_json, MARKETPLACE_DEFAULT_TAGS_JSON);
        assert!(created.enabled);
        assert!(created.verify_enabled);

        storage
            .marketplace_source_upsert(&MarketplaceSourceInput {
                id: MARKETPLACE_SOURCE_ID.to_string(),
                product_id: "legacy-product".to_string(),
                tags_json: "[\"warranty_long\"]".to_string(),
                merchant: Some("legacy-merchant".to_string()),
                enabled: false,
                verify_enabled: false,
            })
            .expect("write legacy source shape");

        let repaired = ensure_default_source(&storage).expect("repair fixed source scope");
        assert_eq!(repaired.product_id, MARKETPLACE_PRODUCT_ID);
        assert_eq!(repaired.tags_json, "[\"warranty_long\"]");
        assert!(repaired.merchant.is_none());
        assert!(!repaired.enabled);
        assert!(!repaired.verify_enabled);
    }

    #[test]
    fn first_sync_builds_baseline_and_active_condition_does_not_repeat() {
        let storage = initialized_storage();
        let rules = vec![rule("rule-baseline")];
        let initial = offer("offer-baseline", 8.0, true, "available");
        let mut notifications = Vec::new();

        process_rules(&storage, &rules, &initial, None, true, &mut notifications)
            .expect("build first baseline");
        assert!(notifications.is_empty());

        process_rules(
            &storage,
            &rules,
            &initial,
            Some(&initial),
            true,
            &mut notifications,
        )
        .expect("process unchanged condition");
        assert!(notifications.is_empty());

        let changed = offer("offer-baseline", 7.0, true, "available");
        process_rules(
            &storage,
            &rules,
            &changed,
            Some(&initial),
            true,
            &mut notifications,
        )
        .expect("process changed price");
        assert_eq!(notifications.len(), 1);

        notifications.clear();
        process_rules(
            &storage,
            &rules,
            &changed,
            Some(&changed),
            true,
            &mut notifications,
        )
        .expect("process unchanged price again");
        assert!(notifications.is_empty());
    }

    #[test]
    fn manual_refresh_updates_baseline_without_delayed_notification_and_reentry_notifies() {
        let storage = initialized_storage();
        let rules = vec![rule("rule-manual")];
        let initial = offer("offer-manual", 8.0, true, "available");
        let manual_change = offer("offer-manual", 7.0, true, "available");
        let mut notifications = Vec::new();

        process_rules(&storage, &rules, &initial, None, true, &mut notifications)
            .expect("build baseline");
        process_rules(
            &storage,
            &rules,
            &manual_change,
            Some(&initial),
            false,
            &mut notifications,
        )
        .expect("process manual change");
        process_rules(
            &storage,
            &rules,
            &manual_change,
            Some(&manual_change),
            true,
            &mut notifications,
        )
        .expect("process next automatic refresh");
        assert!(notifications.is_empty());

        let outside = offer("offer-manual", 12.0, true, "available");
        process_rules(
            &storage,
            &rules,
            &outside,
            Some(&manual_change),
            true,
            &mut notifications,
        )
        .expect("leave condition");
        let reentered = offer("offer-manual", 9.0, true, "available");
        process_rules(
            &storage,
            &rules,
            &reentered,
            Some(&outside),
            true,
            &mut notifications,
        )
        .expect("reenter condition");
        assert_eq!(notifications.len(), 1);
    }

    #[test]
    fn event_rules_respect_scope_restock_and_verification_flags() {
        let mut event_rule = rule("rule-events");
        event_rule.max_price = None;
        event_rule.notify_restock = true;
        event_rule.notify_verified = true;

        let unavailable = offer("offer-events", 8.0, false, "unknown");
        let restocked = offer("offer-events", 8.0, true, "unknown");
        assert!(rule_active(&event_rule, &restocked, Some(&unavailable)));

        let verified = offer("offer-events", 8.0, true, "available");
        assert!(rule_active(&event_rule, &verified, Some(&restocked)));

        let mut wrong_merchant = verified.clone();
        wrong_merchant.source_name = Some("其他商家".to_string());
        assert!(!rule_active(&event_rule, &wrong_merchant, Some(&restocked)));

        let mut wrong_tags = verified.clone();
        wrong_tags.filter_tags_json = "[]".to_string();
        assert!(!rule_active(&event_rule, &wrong_tags, Some(&restocked)));
    }

    #[test]
    fn priceai_status_matches_official_availability_fields() {
        let mut offer = offer("priceai-status", 8.0, true, "unknown");
        offer.effective_status = Some("available".to_string());
        offer.raw_status = Some("out_of_stock".to_string());
        offer.stock_count = Some(0);
        assert_eq!(priceai_status(&offer), "out_of_stock");
        assert!(!priceai_in_stock(&offer));

        offer.raw_status = Some("in_stock".to_string());
        offer.stock_count = Some(0);
        assert_eq!(priceai_status(&offer), "available");
        assert!(priceai_in_stock(&offer));

        offer.freshness_status = Some("expired".to_string());
        assert_eq!(priceai_status(&offer), "unavailable");
        assert!(!priceai_in_stock(&offer));
    }

    #[test]
    fn generic_http_200_without_stock_evidence_stays_unknown() {
        assert_eq!(
            classify_generic_body("<html><body>商品详情</body></html>", 200).0,
            "unknown"
        );
        assert_eq!(classify_generic_body("当前库存 3", 200).0, "available");
        assert_eq!(classify_generic_body("当前库存 0", 200).0, "unavailable");
    }
}
