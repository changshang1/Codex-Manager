use bytes::Bytes;
use codexmanager_core::storage::{AggregateApi, Storage};
use reqwest::header::{HeaderName, HeaderValue};
use serde::Deserialize;
use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};
use tiny_http::Request;

use super::super::response::GatewayStreamPrefetchTerminal;
use super::super::GatewayUpstreamResponse;
use crate::aggregate_api::{
    classify_aggregate_api_daily_limit_failure, classify_aggregate_api_daily_limit_hint,
    AGGREGATE_API_AUTH_APIKEY, AGGREGATE_API_AUTH_USERPASS, AGGREGATE_API_PROVIDER_CLAUDE,
    AGGREGATE_API_PROVIDER_CODEX, AGGREGATE_API_PROVIDER_COMPATIBLE, AGGREGATE_API_PROVIDER_GEMINI,
};
use crate::gateway::protocol_adapter::adapt_openai_responses_to_anthropic_messages;
use crate::gateway::request_log::RequestLogUsage;
use serde_json::Value;

const AGGREGATE_API_RETRY_ATTEMPTS_PER_CHANNEL: usize = 3;
const AGGREGATE_API_NON_STREAM_PREFLIGHT_MAX_BYTES: usize = 256 * 1024;
const AGGREGATE_API_STREAM_PREFLIGHT_MAX_BYTES: usize = 64 * 1024;
const AGGREGATE_API_STREAM_PREFLIGHT_WALL_CLOCK_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Debug, PartialEq, Eq)]
enum AggregateApiStreamPrefixDecision {
    NeedMore,
    Deliver,
    UpstreamError(String),
    DailyLimit {
        reason: &'static str,
        message: String,
    },
}

#[derive(Clone, Copy)]
enum AggregateApiStreamInspectionMode {
    CommitOnContent,
    CompleteBeforeDelivery,
}

enum AggregateApiStreamPreflightOutcome {
    Ready(GatewayUpstreamResponse),
    DailyLimit {
        reason: &'static str,
        message: String,
    },
    TransportFailure(String),
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ApiKeyAuthParams {
    location: String,
    name: String,
    #[serde(default)]
    header_value_format: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct UserPassAuthParams {
    mode: String,
    #[serde(default)]
    username_name: Option<String>,
    #[serde(default)]
    password_name: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct UserPassSecret {
    username: String,
    password: String,
}

#[derive(Debug, Clone)]
enum AggregateApiAuthConfig {
    ApiKeyDefaultBearer,
    ApiKeyHeader {
        name: String,
        format: String,
    },
    ApiKeyQuery {
        name: String,
    },
    UserPassBasic,
    UserPassHeaderPair {
        username_name: String,
        password_name: String,
    },
    UserPassQueryPair {
        username_name: String,
        password_name: String,
    },
}

fn normalize_header_key(name: &str) -> String {
    name.trim().to_ascii_lowercase()
}

fn normalize_action_path(action: &str) -> String {
    let action_trimmed = action.trim();
    if action_trimmed.is_empty() {
        return String::new();
    }
    if action_trimmed.starts_with('/') {
        action_trimmed.to_string()
    } else {
        format!("/{action_trimmed}")
    }
}

fn effective_action_path(candidate: &AggregateApi, path: &str) -> String {
    match candidate.action.as_deref().map(str::trim) {
        Some("") => String::new(),
        Some(value) => normalize_action_path(value),
        None => path.to_string(),
    }
}

fn build_upstream_url(base_url: &str, effective_path: &str) -> Result<reqwest::Url, ()> {
    let mut url = reqwest::Url::parse(base_url).map_err(|_| ())?;
    let trimmed_path = effective_path.trim();
    if trimmed_path.is_empty() {
        return Ok(url);
    }
    let (path_part, query_part) = trimmed_path
        .split_once('?')
        .map_or((trimmed_path, None), |(path, query)| (path, Some(query)));
    let raw_suffix = path_part.trim_start_matches('/');
    let base_path = url.path().trim_end_matches('/').to_string();
    let suffix = if (base_path == "/v1" || base_path.ends_with("/v1"))
        && (raw_suffix == "v1" || raw_suffix.starts_with("v1/"))
    {
        raw_suffix
            .strip_prefix("v1")
            .unwrap_or(raw_suffix)
            .trim_start_matches('/')
    } else {
        raw_suffix
    };
    let combined_path = if base_path.is_empty() || base_path == "/" {
        format!("/{}", suffix)
    } else if suffix.is_empty() {
        base_path
    } else {
        format!("{}/{}", base_path, suffix)
    };
    url.set_path(combined_path.as_str());
    url.set_query(query_part.filter(|query| !query.trim().is_empty()));
    Ok(url)
}

fn rewrite_body_model_override(body: &Bytes, model_override: Option<&str>) -> Bytes {
    let Some(model_override) = model_override
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        return body.clone();
    };
    let Ok(mut value) = serde_json::from_slice::<Value>(body.as_ref()) else {
        return body.clone();
    };
    let Some(object) = value.as_object_mut() else {
        return body.clone();
    };
    if object
        .get("model")
        .and_then(Value::as_str)
        .is_some_and(|current| current == model_override)
    {
        return body.clone();
    }
    object.insert(
        "model".to_string(),
        Value::String(model_override.to_string()),
    );
    serde_json::to_vec(&value)
        .map(Bytes::from)
        .unwrap_or_else(|_| body.clone())
}

fn rewrite_body_for_candidate_transport(
    body: &Bytes,
    candidate: &AggregateApi,
    path: &str,
    upstream_url: &str,
) -> Bytes {
    let rewritten = rewrite_body_model_override(body, candidate.model_override.as_deref());
    if normalize_provider_type_value(candidate.provider_type.as_str())
        == AGGREGATE_API_PROVIDER_CODEX
        && super::super::config::should_send_chatgpt_account_header(upstream_url)
    {
        return Bytes::from(super::super::super::apply_codex_candidate_transport_rules(
            path,
            rewritten.to_vec(),
        ));
    }
    rewritten
}

fn is_minimax_responses_request(base_url: &str, supplier_name: Option<&str>, path: &str) -> bool {
    let is_responses_path = path == "/v1/responses" || path.starts_with("/v1/responses?");
    if !is_responses_path {
        return false;
    }
    if supplier_name
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .is_some_and(|value| value.to_ascii_lowercase().contains("minimax"))
    {
        return true;
    }
    reqwest::Url::parse(base_url)
        .ok()
        .and_then(|url| url.host_str().map(|host| host.to_ascii_lowercase()))
        .is_some_and(|host| host == "minimax.io" || host.ends_with(".minimax.io"))
}

fn minimax_text_content(value: &Value) -> Option<String> {
    let Some(items) = value.as_array() else {
        return value.as_str().map(str::to_string);
    };
    let mut parts = Vec::new();
    for item in items {
        let Some(obj) = item.as_object() else {
            return None;
        };
        let item_type = obj
            .get("type")
            .and_then(Value::as_str)
            .map(str::trim)
            .unwrap_or_default();
        if !matches!(item_type, "input_text" | "output_text" | "text") {
            return None;
        }
        let Some(text) = obj.get("text").and_then(Value::as_str) else {
            return None;
        };
        parts.push(text);
    }
    if parts.is_empty() {
        return None;
    }
    Some(parts.join("\n"))
}

fn normalize_minimax_text_content(value: &mut Value) -> bool {
    let Some(text) = minimax_text_content(value) else {
        return false;
    };
    *value = Value::String(text);
    true
}

fn minimax_input_item_text(value: &Value) -> Option<String> {
    if let Some(text) = value.as_str() {
        return Some(text.to_string());
    }
    let obj = value.as_object()?;
    if obj
        .get("type")
        .and_then(Value::as_str)
        .map(str::trim)
        .is_some_and(|item_type| matches!(item_type, "input_text" | "output_text" | "text"))
    {
        return obj.get("text").and_then(Value::as_str).map(str::to_string);
    }
    obj.get("content").and_then(minimax_text_content)
}

fn normalize_minimax_responses_input(input: &mut Value) -> bool {
    let Some(items) = input.as_array() else {
        return false;
    };
    let mut parts = Vec::new();
    for item in items {
        if let Some(text) = minimax_input_item_text(item) {
            if !text.is_empty() {
                parts.push(text);
            }
        }
    }
    if parts.is_empty() {
        return false;
    }
    *input = Value::String(parts.join("\n\n"));
    true
}

fn rewrite_minimax_responses_body(
    body: &Bytes,
    base_url: &str,
    supplier_name: Option<&str>,
    path: &str,
) -> Bytes {
    if !is_minimax_responses_request(base_url, supplier_name, path) {
        return body.clone();
    }
    let Ok(mut value) = serde_json::from_slice::<Value>(body.as_ref()) else {
        return body.clone();
    };
    let Some(input) = value.get_mut("input") else {
        return body.clone();
    };

    let mut changed = false;
    if let Some(items) = input.as_array_mut() {
        for item in items {
            if let Some(content) = item.get_mut("content") {
                if normalize_minimax_text_content(content) {
                    changed = true;
                }
            }
        }
    }
    if normalize_minimax_responses_input(input) {
        changed = true;
    }

    if !changed {
        return body.clone();
    }
    serde_json::to_vec(&value)
        .map(Bytes::from)
        .unwrap_or_else(|_| body.clone())
}

fn aggregate_upstream_model_for_log<'a>(
    candidate: &'a AggregateApi,
    platform_model: Option<&'a str>,
) -> Option<&'a str> {
    candidate.model_override.as_deref().or(platform_model)
}

fn should_bridge_responses_to_anthropic(candidate: &AggregateApi, path: &str) -> bool {
    normalize_provider_type_value(candidate.provider_type.as_str()) == AGGREGATE_API_PROVIDER_CLAUDE
        && (path == "/v1/responses" || path.starts_with("/v1/responses?"))
}

fn responses_to_anthropic_messages_action_path(candidate: &AggregateApi, path: &str) -> String {
    if candidate.action.is_some() {
        return effective_action_path(candidate, path);
    }

    let base_path = reqwest::Url::parse(candidate.url.as_str())
        .ok()
        .map(|url| url.path().trim_end_matches('/').to_string())
        .unwrap_or_default();
    if base_path == "/v1" || base_path.ends_with("/v1") {
        "/messages".to_string()
    } else {
        "/v1/messages".to_string()
    }
}

fn replace_query_param(mut url: reqwest::Url, name: &str, value: &str) -> reqwest::Url {
    let name_trimmed = name.trim();
    if name_trimmed.is_empty() {
        return url;
    }
    let existing = url.query_pairs().into_owned().collect::<Vec<_>>();
    url.set_query(None);
    {
        let mut qp = url.query_pairs_mut();
        for (k, v) in existing {
            if k == name_trimmed {
                continue;
            }
            qp.append_pair(k.as_str(), v.as_str());
        }
        qp.append_pair(name_trimmed, value);
    }
    url
}

fn parse_auth_config(
    candidate: &AggregateApi,
) -> Result<(AggregateApiAuthConfig, HashSet<String>), String> {
    let auth_type = candidate.auth_type.trim().to_ascii_lowercase();
    let raw_params = candidate
        .auth_params_json
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());

    let mut injected_headers = HashSet::new();

    if raw_params.is_none() {
        if auth_type == AGGREGATE_API_AUTH_USERPASS {
            return Ok((AggregateApiAuthConfig::UserPassBasic, injected_headers));
        }
        return Ok((
            AggregateApiAuthConfig::ApiKeyDefaultBearer,
            injected_headers,
        ));
    }

    let value: serde_json::Value = serde_json::from_str(raw_params.unwrap())
        .map_err(|_| "invalid aggregate api authParams".to_string())?;

    if auth_type == AGGREGATE_API_AUTH_APIKEY {
        let parsed: ApiKeyAuthParams = serde_json::from_value(value)
            .map_err(|_| "invalid aggregate api authParams".to_string())?;
        let location = parsed.location.trim().to_ascii_lowercase();
        if location == "query" {
            return Ok((
                AggregateApiAuthConfig::ApiKeyQuery {
                    name: parsed.name.trim().to_string(),
                },
                injected_headers,
            ));
        }
        let header_name = parsed.name.trim().to_string();
        injected_headers.insert(normalize_header_key(header_name.as_str()));
        let format = parsed
            .header_value_format
            .as_deref()
            .unwrap_or("bearer")
            .trim()
            .to_ascii_lowercase();
        return Ok((
            AggregateApiAuthConfig::ApiKeyHeader {
                name: header_name,
                format,
            },
            injected_headers,
        ));
    }

    if auth_type == AGGREGATE_API_AUTH_USERPASS {
        let parsed: UserPassAuthParams = serde_json::from_value(value)
            .map_err(|_| "invalid aggregate api authParams".to_string())?;
        let mode = parsed.mode.trim().to_ascii_lowercase();
        match mode.as_str() {
            "basic" => return Ok((AggregateApiAuthConfig::UserPassBasic, injected_headers)),
            "headerpair" => {
                let username_name = parsed
                    .username_name
                    .as_deref()
                    .unwrap_or("username")
                    .trim()
                    .to_string();
                let password_name = parsed
                    .password_name
                    .as_deref()
                    .unwrap_or("password")
                    .trim()
                    .to_string();
                injected_headers.insert(normalize_header_key(username_name.as_str()));
                injected_headers.insert(normalize_header_key(password_name.as_str()));
                return Ok((
                    AggregateApiAuthConfig::UserPassHeaderPair {
                        username_name,
                        password_name,
                    },
                    injected_headers,
                ));
            }
            "querypair" => {
                let username_name = parsed
                    .username_name
                    .as_deref()
                    .unwrap_or("username")
                    .trim()
                    .to_string();
                let password_name = parsed
                    .password_name
                    .as_deref()
                    .unwrap_or("password")
                    .trim()
                    .to_string();
                return Ok((
                    AggregateApiAuthConfig::UserPassQueryPair {
                        username_name,
                        password_name,
                    },
                    injected_headers,
                ));
            }
            _ => return Err("invalid aggregate api authParams".to_string()),
        }
    }

    Ok((
        AggregateApiAuthConfig::ApiKeyDefaultBearer,
        injected_headers,
    ))
}

fn resolve_passthrough_sse_protocol(
    path: &str,
    response_adapter: super::super::super::ResponseAdapter,
) -> Option<super::super::super::PassthroughSseProtocol> {
    if response_adapter != super::super::super::ResponseAdapter::Passthrough {
        return None;
    }
    if path == "/v1/messages" || path.starts_with("/v1/messages?") {
        return Some(super::super::super::PassthroughSseProtocol::AnthropicNative);
    }
    if path
        .split('?')
        .next()
        .is_some_and(|path| path.contains(":streamGenerateContent"))
    {
        return Some(super::super::super::PassthroughSseProtocol::GeminiNative);
    }
    None
}

/// 函数 `should_skip_forward_header`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - name: 参数 name
///
/// # 返回
/// 返回函数执行结果
fn should_skip_forward_header(name: &str) -> bool {
    matches!(
        name.to_ascii_lowercase().as_str(),
        "authorization"
            | "x-api-key"
            | "api-key"
            | "content-length"
            | "connection"
            | "proxy-authorization"
            | "proxy-authenticate"
            | "te"
            | "trailer"
            | "transfer-encoding"
            | "upgrade"
            | "host"
    )
}

fn should_skip_forward_header_with_overrides(name: &str, injected: &HashSet<String>) -> bool {
    if should_skip_forward_header(name) {
        return true;
    }
    injected.contains(normalize_header_key(name).as_str())
}

fn should_skip_forward_header_for_aggregate_request(
    name: &str,
    injected: &HashSet<String>,
    is_stream: bool,
) -> bool {
    if should_skip_forward_header_with_overrides(name, injected) {
        return true;
    }
    is_stream && normalize_header_key(name) == "accept"
}

/// 函数 `respond_error`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - request: 参数 request
/// - status: 参数 status
/// - message: 参数 message
/// - trace_id: 参数 trace_id
///
/// # 返回
/// 无
fn respond_error(request: Request, status: u16, message: &str, trace_id: Option<&str>) {
    let response_message = super::super::super::error_message_for_client(
        super::super::super::prefers_raw_errors_for_tiny_http_request(&request),
        message,
    );
    let response = super::super::super::error_response::terminal_text_response(
        status,
        response_message,
        trace_id,
    );
    let _ = request.respond(response);
}

/// 函数 `normalize_candidate_order`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - candidates: 参数 candidates
///
/// # 返回
/// 返回函数执行结果
fn normalize_candidate_order(mut candidates: Vec<AggregateApi>) -> Vec<AggregateApi> {
    // 连接顺序号只控制管理页展示；模型路由的优先级和权重决定运行时顺序。
    // 未携带模型路由时使用稳定的 ID 顺序，避免展示排序意外改变流量。
    candidates.sort_by(|left, right| left.id.cmp(&right.id));
    candidates
}

pub(in super::super) fn promote_preferred_aggregate_candidate(
    candidates: &mut Vec<AggregateApi>,
    preferred_id: &str,
) {
    let preferred_id = preferred_id.trim();
    if preferred_id.is_empty() {
        return;
    }
    let Some(index) = candidates.iter().position(|api| api.id == preferred_id) else {
        return;
    };
    if index == 0 {
        return;
    }
    let preferred = candidates.remove(index);
    candidates.insert(0, preferred);
}

/// 函数 `apply_gateway_route_strategy_to_aggregate_candidates`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - crate: 参数 crate
///
/// # 返回
/// 无
pub(crate) fn apply_gateway_route_strategy_to_aggregate_candidates(
    _candidates: &mut [AggregateApi],
    _key_id: &str,
    _model: Option<&str>,
    _preferred_aggregate_api_id: Option<&str>,
) {
    // 全局“顺序/均衡”仅控制账号池；聚合 API 已由模型路由独立调度。
}

#[cfg(test)]
pub(crate) fn preview_gateway_route_strategy_to_aggregate_candidates(
    _candidates: &mut [AggregateApi],
    _key_id: &str,
    _model: Option<&str>,
    _preferred_aggregate_api_id: Option<&str>,
) {
    // 与实际兼容钩子保持一致，预览也不改写模型路由顺序。
}

pub(crate) fn prepare_first_aggregate_candidate_client(
    candidates: &[AggregateApi],
    trace_id: &str,
) {
    if let Some(candidate) = candidates.first() {
        prepare_aggregate_candidate_client(candidate, trace_id, "first");
    }
}

fn prepare_next_aggregate_candidate_client(
    ordered_candidates: &[(String, String)],
    candidate_idx: usize,
    trace_id: &str,
) {
    let Some((candidate_id, candidate_url)) = ordered_candidates.get(candidate_idx + 1) else {
        return;
    };
    if let Err(err) = super::super::super::prepare_upstream_client_for_aggregate_api_candidate(
        candidate_id.as_str(),
        candidate_url.as_str(),
    ) {
        log::warn!(
            "event=gateway_aggregate_candidate_client_prepare_failed trace_id={} aggregate_api_id={} phase=next err={}",
            trace_id,
            candidate_id,
            err
        );
    }
}

fn prepare_aggregate_candidate_client(candidate: &AggregateApi, trace_id: &str, phase: &str) {
    if let Err(err) = super::super::super::prepare_upstream_client_for_aggregate_api_candidate(
        candidate.id.as_str(),
        candidate.url.as_str(),
    ) {
        log::warn!(
            "event=gateway_aggregate_candidate_client_prepare_failed trace_id={} aggregate_api_id={} phase={} err={}",
            trace_id,
            candidate.id,
            phase,
            err
        );
    }
}

/// 函数 `normalize_provider_type_value`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - value: 参数 value
///
/// # 返回
/// 返回函数执行结果
fn normalize_provider_type_value(value: &str) -> String {
    let normalized = value.trim().to_ascii_lowercase().replace('-', "_");
    match normalized.as_str() {
        "claude" | "anthropic" | "anthropic_native" | "claude_code" => {
            AGGREGATE_API_PROVIDER_CLAUDE.to_string()
        }
        "gemini" | "gemini_native" | "google" | "google_ai" | "google_gemini" => {
            AGGREGATE_API_PROVIDER_GEMINI.to_string()
        }
        "compatible" => AGGREGATE_API_PROVIDER_COMPATIBLE.to_string(),
        _ => AGGREGATE_API_PROVIDER_CODEX.to_string(),
    }
}

/// 函数 `first_upstream_header`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - headers: 参数 headers
/// - names: 参数 names
///
/// # 返回
/// 返回函数执行结果
fn first_upstream_header(headers: &reqwest::header::HeaderMap, names: &[&str]) -> Option<String> {
    names.iter().find_map(|name| {
        headers
            .get(*name)
            .and_then(|value| value.to_str().ok())
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
    })
}

/// 函数 `aggregate_api_failure_message`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - status_code: 参数 status_code
/// - body: 参数 body
/// - request_id: 参数 request_id
/// - cf_ray: 参数 cf_ray
/// - auth_error: 参数 auth_error
/// - identity_error_code: 参数 identity_error_code
///
/// # 返回
/// 返回函数执行结果
fn aggregate_api_failure_message(
    status_code: u16,
    body: &[u8],
    request_id: Option<&str>,
    cf_ray: Option<&str>,
    auth_error: Option<&str>,
    identity_error_code: Option<&str>,
) -> String {
    let mut parts =
        vec![
            crate::gateway::summarize_upstream_error_hint_from_body(status_code, body)
                .unwrap_or_else(|| format!("aggregate api upstream status={status_code}")),
        ];
    if let Some(request_id) = request_id.map(str::trim).filter(|value| !value.is_empty()) {
        parts.push(format!("request_id={request_id}"));
    }
    if let Some(cf_ray) = cf_ray.map(str::trim).filter(|value| !value.is_empty()) {
        parts.push(format!("cf_ray={cf_ray}"));
    }
    if let Some(auth_error) = auth_error.map(str::trim).filter(|value| !value.is_empty()) {
        parts.push(format!("auth_error={auth_error}"));
    }
    if let Some(identity_error_code) = identity_error_code
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        parts.push(format!("identity_error_code={identity_error_code}"));
    }
    if parts.len() == 1 {
        parts.remove(0)
    } else {
        format!("{} [{}]", parts.remove(0), parts.join(", "))
    }
}

fn record_aggregate_api_daily_limit_failure(
    storage: &Storage,
    candidate_id: &str,
    trace_id: &str,
    reason: &str,
) {
    match storage.record_aggregate_api_daily_quota_failure(candidate_id) {
        Ok(Some(update)) if update.counted => {
            log::warn!(
                "event=gateway_aggregate_daily_limit_failure trace_id={} aggregate_api_id={} reason={} consecutive_failures={} auto_disabled={}",
                trace_id,
                candidate_id,
                reason,
                update.consecutive_failures,
                update.auto_disabled,
            );
        }
        Ok(_) => {}
        Err(err) => {
            log::warn!(
                "event=gateway_aggregate_daily_limit_state_write_failed trace_id={} aggregate_api_id={} err={}",
                trace_id,
                candidate_id,
                err,
            );
        }
    }
}

fn reset_aggregate_api_consecutive_failures(storage: &Storage, candidate_id: &str, trace_id: &str) {
    if let Err(err) = storage.reset_aggregate_api_consecutive_failures(candidate_id) {
        log::warn!(
            "event=gateway_aggregate_daily_limit_reset_failed trace_id={} aggregate_api_id={} err={}",
            trace_id,
            candidate_id,
            err,
        );
    }
}

// A model can route to the same aggregate API more than once. Keep daily-limit
// failures request-local until routing finishes so a later success from that API
// cancels the pending failure instead of leaving it auto-disabled. A later
// transient failure does not erase an explicit daily-limit signal.
struct AggregateApiDailyLimitTracker<'a> {
    storage: &'a Storage,
    trace_id: &'a str,
    pending: HashMap<String, &'static str>,
}

impl<'a> AggregateApiDailyLimitTracker<'a> {
    fn new(storage: &'a Storage, trace_id: &'a str) -> Self {
        Self {
            storage,
            trace_id,
            pending: HashMap::new(),
        }
    }

    fn mark_failure(&mut self, candidate_id: &str, reason: &'static str) {
        self.pending
            .entry(candidate_id.to_string())
            .or_insert(reason);
    }

    fn has_pending_failure(&self, candidate_id: &str) -> bool {
        self.pending.contains_key(candidate_id)
    }

    fn mark_success(&mut self, candidate_id: &str) {
        self.pending.remove(candidate_id);
        reset_aggregate_api_consecutive_failures(self.storage, candidate_id, self.trace_id);
    }

    fn flush(&mut self) {
        for (candidate_id, reason) in std::mem::take(&mut self.pending) {
            record_aggregate_api_daily_limit_failure(
                self.storage,
                candidate_id.as_str(),
                self.trace_id,
                reason,
            );
        }
    }
}

impl Drop for AggregateApiDailyLimitTracker<'_> {
    fn drop(&mut self) {
        self.flush();
    }
}

fn aggregate_api_sse_frames(prefix: &[u8], include_incomplete: bool) -> Vec<String> {
    let normalized = String::from_utf8_lossy(prefix).replace("\r\n", "\n");
    let mut frames = normalized.split("\n\n").collect::<Vec<_>>();
    if !include_incomplete && !normalized.ends_with("\n\n") {
        let _ = frames.pop();
    }
    frames.into_iter().map(str::to_string).collect()
}

fn aggregate_api_sse_event_and_data(frame: &str) -> (Option<String>, Option<String>) {
    let mut event_type = None;
    let mut data = String::new();
    for line in frame.lines() {
        let line = line.trim_end_matches('\r');
        if let Some(value) = line.strip_prefix("event:") {
            event_type = Some(value.trim().to_string());
        } else if let Some(value) = line.strip_prefix("data:") {
            if !data.is_empty() {
                data.push('\n');
            }
            data.push_str(value.trim_start());
        }
    }
    (event_type, (!data.is_empty()).then_some(data))
}

fn aggregate_api_metadata_only_event(event_type: &str) -> bool {
    matches!(
        event_type.trim().to_ascii_lowercase().as_str(),
        "response.created"
            | "response.in_progress"
            | "response.queued"
            | "response.output_item.added"
            | "response.content_part.added"
            | "response.reasoning_summary_part.added"
            | "message_start"
            | "content_block_start"
            | "ping"
    )
}

fn aggregate_api_chat_chunk_is_metadata_only(value: &Value) -> bool {
    let Some(choices) = value.get("choices").and_then(Value::as_array) else {
        return false;
    };
    if choices.is_empty() {
        return true;
    }
    choices.iter().all(|choice| {
        if choice
            .get("finish_reason")
            .is_some_and(|value| !value.is_null())
        {
            return false;
        }
        let Some(delta) = choice.get("delta").and_then(Value::as_object) else {
            return false;
        };
        let has_text = ["content", "refusal"]
            .into_iter()
            .filter_map(|key| delta.get(key).and_then(Value::as_str))
            .any(|text| !text.is_empty());
        let has_call = ["tool_calls", "function_call"]
            .into_iter()
            .any(|key| delta.get(key).is_some_and(|value| !value.is_null()));
        !has_text && !has_call
    })
}

fn aggregate_api_sse_event_is_error(
    declared_event_type: Option<&str>,
    parsed: Option<&Value>,
) -> bool {
    let payload_event_type = parsed
        .and_then(|value| value.get("type"))
        .and_then(Value::as_str);
    declared_event_type
        .into_iter()
        .chain(payload_event_type)
        .any(|event_type| {
            let normalized = event_type.trim().to_ascii_lowercase().replace('-', "_");
            matches!(
                normalized.as_str(),
                "error" | "response.failed" | "message.error" | "message.failed"
            ) || normalized.ends_with("_error")
                || normalized.ends_with(".error")
        })
        || parsed.is_some_and(|value| {
            value
                .get("error")
                .is_some_and(aggregate_api_error_value_is_present)
                || value
                    .pointer("/response/error")
                    .is_some_and(aggregate_api_error_value_is_present)
                || value
                    .pointer("/response/status_details/error")
                    .is_some_and(aggregate_api_error_value_is_present)
                || value
                    .get("code")
                    .is_some_and(aggregate_api_failure_code_is_present)
                || aggregate_api_explicit_daily_limit_code_is_present(value)
        })
}

fn aggregate_api_error_value_is_present(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(value) => *value,
        Value::Number(value) => value.as_f64().is_some_and(|value| value != 0.0),
        Value::String(value) => !value.trim().is_empty(),
        Value::Array(value) => !value.is_empty(),
        Value::Object(value) => !value.is_empty(),
    }
}

fn aggregate_api_failure_code_is_present(value: &Value) -> bool {
    match value {
        Value::Number(value) => value
            .as_i64()
            .is_some_and(|value| value < 0 || value >= 400),
        Value::String(value) => {
            let normalized = value
                .trim()
                .to_ascii_lowercase()
                .replace(['-', '.', ' '], "_");
            if normalized.is_empty() {
                return false;
            }
            if let Ok(value) = normalized.parse::<i64>() {
                return value < 0 || value >= 400;
            }
            if matches!(
                normalized.as_str(),
                "ok" | "success"
                    | "succeeded"
                    | "complete"
                    | "completed"
                    | "no_error"
                    | "no_errors"
                    | "noerror"
                    | "error_free"
                    | "none"
            ) {
                return false;
            }
            [
                "error",
                "fail",
                "exceeded",
                "denied",
                "unauthorized",
                "forbidden",
                "invalid",
                "insufficient",
                "unavailable",
                "overload",
                "timeout",
                "timed_out",
                "not_found",
                "exhausted",
            ]
            .into_iter()
            .any(|marker| normalized.contains(marker))
        }
        _ => false,
    }
}

fn aggregate_api_explicit_daily_limit_code_is_present(value: &Value) -> bool {
    ["type", "status", "reason"]
        .into_iter()
        .filter_map(|key| value.get(key).and_then(Value::as_str))
        .any(|value| {
            value
                .trim()
                .replace(['-', ' '], "_")
                .eq_ignore_ascii_case("DAILY_LIMIT_EXCEEDED")
        })
}

fn classify_aggregate_api_stream_prefix(
    prefix: &[u8],
    include_incomplete: bool,
    mode: AggregateApiStreamInspectionMode,
) -> AggregateApiStreamPrefixDecision {
    let frames = aggregate_api_sse_frames(prefix, include_incomplete);
    if frames.is_empty() {
        return AggregateApiStreamPrefixDecision::NeedMore;
    }
    let frame_count = frames.len();
    let mut saw_deliverable_content = false;
    for (index, frame) in frames.into_iter().enumerate() {
        let (declared_event_type, data) = aggregate_api_sse_event_and_data(frame.as_str());
        let Some(data) = data else {
            continue;
        };
        if data.trim() == "[DONE]" {
            saw_deliverable_content = true;
            if matches!(mode, AggregateApiStreamInspectionMode::CommitOnContent) {
                return AggregateApiStreamPrefixDecision::Deliver;
            }
            continue;
        }
        let parsed = serde_json::from_str::<Value>(data.as_str()).ok();
        let is_incomplete_trailing_frame =
            include_incomplete && index + 1 == frame_count && !data.ends_with('}');
        if is_incomplete_trailing_frame
            && parsed.is_none()
            && data
                .trim_start()
                .as_bytes()
                .first()
                .is_some_and(|byte| matches!(*byte, b'{' | b'['))
        {
            return AggregateApiStreamPrefixDecision::NeedMore;
        }
        let payload_event_type = parsed
            .as_ref()
            .and_then(|value| value.get("type"))
            .and_then(Value::as_str);
        let event_type = payload_event_type.or(declared_event_type.as_deref());
        if aggregate_api_sse_event_is_error(declared_event_type.as_deref(), parsed.as_ref()) {
            if let Some(reason) = classify_aggregate_api_daily_limit_failure(200, data.as_bytes())
                .or_else(|| classify_aggregate_api_daily_limit_hint(data.as_str()))
            {
                return AggregateApiStreamPrefixDecision::DailyLimit {
                    reason,
                    message: aggregate_api_failure_message(
                        429,
                        data.as_bytes(),
                        None,
                        None,
                        None,
                        None,
                    ),
                };
            }
            return AggregateApiStreamPrefixDecision::UpstreamError(aggregate_api_failure_message(
                502,
                data.as_bytes(),
                None,
                None,
                None,
                None,
            ));
        }
        if event_type.is_some_and(aggregate_api_metadata_only_event) {
            continue;
        }
        if parsed
            .as_ref()
            .is_some_and(aggregate_api_chat_chunk_is_metadata_only)
        {
            continue;
        }
        saw_deliverable_content = true;
        if matches!(mode, AggregateApiStreamInspectionMode::CommitOnContent) {
            return AggregateApiStreamPrefixDecision::Deliver;
        }
    }
    if saw_deliverable_content {
        AggregateApiStreamPrefixDecision::Deliver
    } else {
        AggregateApiStreamPrefixDecision::NeedMore
    }
}

fn is_aggregate_api_sse_response(response: &GatewayUpstreamResponse) -> bool {
    response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.split(';').next())
        .is_some_and(|value| value.trim().eq_ignore_ascii_case("text/event-stream"))
}

fn aggregate_api_prefix_looks_like_sse(prefix: &[u8]) -> bool {
    prefix.split(|byte| *byte == b'\n').any(|line| {
        let line = line
            .iter()
            .position(|byte| !matches!(*byte, b' ' | b'\t' | b'\r'))
            .map(|start| &line[start..])
            .unwrap_or_default();
        line.starts_with(b"data:")
            || line.starts_with(b"event:")
            || line.starts_with(b"id:")
            || line.starts_with(b"retry:")
            || line.starts_with(b":")
    })
}

fn aggregate_api_json_has_error_context(body: &[u8]) -> bool {
    serde_json::from_slice::<Value>(body)
        .ok()
        .is_some_and(|value| aggregate_api_sse_event_is_error(None, Some(&value)))
}

fn classify_complete_aggregate_api_non_stream_body(
    body: &[u8],
    response: GatewayUpstreamResponse,
) -> AggregateApiStreamPreflightOutcome {
    if let Some(reason) = classify_aggregate_api_daily_limit_failure(200, body) {
        return AggregateApiStreamPreflightOutcome::DailyLimit {
            reason,
            message: aggregate_api_failure_message(429, body, None, None, None, None),
        };
    }
    if aggregate_api_json_has_error_context(body) {
        return AggregateApiStreamPreflightOutcome::TransportFailure(
            aggregate_api_failure_message(502, body, None, None, None, None),
        );
    }
    AggregateApiStreamPreflightOutcome::Ready(response)
}

fn classify_prefetched_aggregate_api_sse(
    prefix: &[u8],
    response: GatewayUpstreamResponse,
    terminal: GatewayStreamPrefetchTerminal,
    mode: AggregateApiStreamInspectionMode,
) -> AggregateApiStreamPreflightOutcome {
    let include_incomplete = matches!(
        terminal,
        GatewayStreamPrefetchTerminal::Eof
            | GatewayStreamPrefetchTerminal::Error(_)
            | GatewayStreamPrefetchTerminal::Disconnected
    );
    match classify_aggregate_api_stream_prefix(prefix, include_incomplete, mode) {
        AggregateApiStreamPrefixDecision::DailyLimit { reason, message } => {
            AggregateApiStreamPreflightOutcome::DailyLimit { reason, message }
        }
        AggregateApiStreamPrefixDecision::UpstreamError(message) => {
            AggregateApiStreamPreflightOutcome::TransportFailure(message)
        }
        AggregateApiStreamPrefixDecision::Deliver
            if matches!(
                mode,
                AggregateApiStreamInspectionMode::CompleteBeforeDelivery
            ) =>
        {
            match terminal {
                GatewayStreamPrefetchTerminal::IdleTimeout => {
                    AggregateApiStreamPreflightOutcome::TransportFailure(
                        "aggregate api response body idle timeout before delivery".to_string(),
                    )
                }
                GatewayStreamPrefetchTerminal::Error(err) => {
                    AggregateApiStreamPreflightOutcome::TransportFailure(format!(
                        "aggregate api response body failed before delivery: {err}"
                    ))
                }
                GatewayStreamPrefetchTerminal::Disconnected => {
                    AggregateApiStreamPreflightOutcome::TransportFailure(
                        "aggregate api response body disconnected before delivery".to_string(),
                    )
                }
                _ => AggregateApiStreamPreflightOutcome::Ready(response),
            }
        }
        AggregateApiStreamPrefixDecision::Deliver => {
            AggregateApiStreamPreflightOutcome::Ready(response)
        }
        AggregateApiStreamPrefixDecision::NeedMore => match terminal {
            GatewayStreamPrefetchTerminal::IdleTimeout => {
                AggregateApiStreamPreflightOutcome::TransportFailure(
                    "aggregate api stream idle timeout before producing content".to_string(),
                )
            }
            GatewayStreamPrefetchTerminal::Eof
                if serde_json::from_slice::<Value>(prefix).is_ok() =>
            {
                classify_complete_aggregate_api_non_stream_body(prefix, response)
            }
            GatewayStreamPrefetchTerminal::Eof => {
                AggregateApiStreamPreflightOutcome::TransportFailure(
                    "aggregate api stream ended before producing content".to_string(),
                )
            }
            GatewayStreamPrefetchTerminal::Error(err) => {
                AggregateApiStreamPreflightOutcome::TransportFailure(format!(
                    "aggregate api stream failed before producing content: {err}"
                ))
            }
            GatewayStreamPrefetchTerminal::Disconnected => {
                AggregateApiStreamPreflightOutcome::TransportFailure(
                    "aggregate api stream disconnected before producing content".to_string(),
                )
            }
            GatewayStreamPrefetchTerminal::Open
            | GatewayStreamPrefetchTerminal::PrefixLimit
            | GatewayStreamPrefetchTerminal::WallClockTimeout => {
                AggregateApiStreamPreflightOutcome::Ready(response)
            }
        },
    }
}

fn classify_complete_aggregate_api_response(
    body: &[u8],
    response: GatewayUpstreamResponse,
) -> AggregateApiStreamPreflightOutcome {
    if is_aggregate_api_sse_response(&response) || aggregate_api_prefix_looks_like_sse(body) {
        classify_prefetched_aggregate_api_sse(
            body,
            response,
            GatewayStreamPrefetchTerminal::Eof,
            AggregateApiStreamInspectionMode::CompleteBeforeDelivery,
        )
    } else {
        classify_complete_aggregate_api_non_stream_body(body, response)
    }
}

fn preflight_aggregate_api_stream(
    response: GatewayUpstreamResponse,
) -> AggregateApiStreamPreflightOutcome {
    let declared_sse = is_aggregate_api_sse_response(&response);
    let content_length = response
        .headers()
        .get(reqwest::header::CONTENT_LENGTH)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<usize>().ok());
    if !declared_sse
        && content_length
            .is_some_and(|length| length > AGGREGATE_API_NON_STREAM_PREFLIGHT_MAX_BYTES)
    {
        return AggregateApiStreamPreflightOutcome::Ready(response);
    }
    let max_bytes = if declared_sse {
        AGGREGATE_API_STREAM_PREFLIGHT_MAX_BYTES
    } else {
        AGGREGATE_API_NON_STREAM_PREFLIGHT_MAX_BYTES
    };
    let (prefix, response, terminal) = response.prefetch_stream_prefix(
        max_bytes,
        crate::gateway::upstream_stream_timeout(),
        Some(AGGREGATE_API_STREAM_PREFLIGHT_WALL_CLOCK_TIMEOUT),
        |prefix| {
            (declared_sse || aggregate_api_prefix_looks_like_sse(prefix))
                && !matches!(
                    classify_aggregate_api_stream_prefix(
                        prefix,
                        false,
                        AggregateApiStreamInspectionMode::CommitOnContent,
                    ),
                    AggregateApiStreamPrefixDecision::NeedMore
                )
        },
    );
    if declared_sse || aggregate_api_prefix_looks_like_sse(prefix.as_ref()) {
        return classify_prefetched_aggregate_api_sse(
            prefix.as_ref(),
            response,
            terminal,
            AggregateApiStreamInspectionMode::CommitOnContent,
        );
    }

    match terminal {
        GatewayStreamPrefetchTerminal::Eof => {
            classify_complete_aggregate_api_non_stream_body(prefix.as_ref(), response)
        }
        GatewayStreamPrefetchTerminal::IdleTimeout => {
            AggregateApiStreamPreflightOutcome::TransportFailure(
                "aggregate api response body idle timeout before delivery".to_string(),
            )
        }
        GatewayStreamPrefetchTerminal::Error(err) => {
            AggregateApiStreamPreflightOutcome::TransportFailure(format!(
                "aggregate api response body failed before delivery: {err}"
            ))
        }
        GatewayStreamPrefetchTerminal::Disconnected => {
            AggregateApiStreamPreflightOutcome::TransportFailure(
                "aggregate api response body disconnected before delivery".to_string(),
            )
        }
        GatewayStreamPrefetchTerminal::Open
        | GatewayStreamPrefetchTerminal::PrefixLimit
        | GatewayStreamPrefetchTerminal::WallClockTimeout => {
            AggregateApiStreamPreflightOutcome::Ready(response)
        }
    }
}

fn classify_prefetched_aggregate_api_non_stream(
    body: &[u8],
    response: GatewayUpstreamResponse,
    terminal: GatewayStreamPrefetchTerminal,
) -> AggregateApiStreamPreflightOutcome {
    if is_aggregate_api_sse_response(&response) || aggregate_api_prefix_looks_like_sse(body) {
        return classify_prefetched_aggregate_api_sse(
            body,
            response,
            terminal,
            AggregateApiStreamInspectionMode::CompleteBeforeDelivery,
        );
    }

    match terminal {
        GatewayStreamPrefetchTerminal::Eof => {
            classify_complete_aggregate_api_response(body, response)
        }
        GatewayStreamPrefetchTerminal::IdleTimeout => {
            AggregateApiStreamPreflightOutcome::TransportFailure(
                "aggregate api response body idle timeout before delivery".to_string(),
            )
        }
        GatewayStreamPrefetchTerminal::Error(err) => {
            AggregateApiStreamPreflightOutcome::TransportFailure(format!(
                "aggregate api response body failed before delivery: {err}"
            ))
        }
        GatewayStreamPrefetchTerminal::Disconnected => {
            AggregateApiStreamPreflightOutcome::TransportFailure(
                "aggregate api response body disconnected before delivery".to_string(),
            )
        }
        GatewayStreamPrefetchTerminal::Open
        | GatewayStreamPrefetchTerminal::PrefixLimit
        | GatewayStreamPrefetchTerminal::WallClockTimeout => {
            AggregateApiStreamPreflightOutcome::Ready(response)
        }
    }
}

fn preflight_aggregate_api_non_stream(
    response: GatewayUpstreamResponse,
) -> AggregateApiStreamPreflightOutcome {
    let declared_sse = is_aggregate_api_sse_response(&response);
    let content_length = response
        .headers()
        .get(reqwest::header::CONTENT_LENGTH)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<usize>().ok());
    if !declared_sse
        && content_length
            .is_some_and(|length| length > AGGREGATE_API_NON_STREAM_PREFLIGHT_MAX_BYTES)
    {
        return AggregateApiStreamPreflightOutcome::Ready(response);
    }

    let max_bytes = if declared_sse {
        AGGREGATE_API_STREAM_PREFLIGHT_MAX_BYTES
    } else {
        AGGREGATE_API_NON_STREAM_PREFLIGHT_MAX_BYTES
    };
    let (body, response, terminal) = response.prefetch_stream_prefix(
        max_bytes,
        crate::gateway::upstream_stream_timeout(),
        Some(AGGREGATE_API_STREAM_PREFLIGHT_WALL_CLOCK_TIMEOUT),
        |prefix| {
            if !declared_sse && !aggregate_api_prefix_looks_like_sse(prefix) {
                return false;
            }
            matches!(
                classify_aggregate_api_stream_prefix(
                    prefix,
                    false,
                    AggregateApiStreamInspectionMode::CompleteBeforeDelivery,
                ),
                AggregateApiStreamPrefixDecision::DailyLimit { .. }
                    | AggregateApiStreamPrefixDecision::UpstreamError(_)
            )
        },
    );
    classify_prefetched_aggregate_api_non_stream(body.as_ref(), response, terminal)
}

/// 函数 `build_aggregate_api_request`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - client: 参数 client
/// - request: 参数 request
/// - method: 参数 method
/// - url: 参数 url
/// - body: 参数 body
/// - secret: 参数 secret
/// - request_deadline: 参数 request_deadline
/// - is_stream: 参数 is_stream
///
/// # 返回
/// 返回函数执行结果
fn build_aggregate_api_request(
    client: &reqwest::blocking::Client,
    request: &Request,
    method: &reqwest::Method,
    url: reqwest::Url,
    body: &Bytes,
    secret: &str,
    auth_config: &AggregateApiAuthConfig,
    injected_headers: &HashSet<String>,
    request_deadline: Option<Instant>,
    is_stream: bool,
) -> Result<reqwest::blocking::RequestBuilder, String> {
    let mut builder = client.request(method.clone(), url);
    if let Some(timeout) =
        super::super::support::deadline::send_timeout(request_deadline, is_stream)
    {
        builder = builder.timeout(timeout);
    }
    let request_headers = request.headers().to_vec();
    for header in &request_headers {
        if should_skip_forward_header_for_aggregate_request(
            header.field.as_str().into(),
            injected_headers,
            is_stream,
        ) {
            continue;
        }
        if let (Ok(name), Ok(value)) = (
            HeaderName::from_bytes(header.field.as_str().as_bytes()),
            HeaderValue::from_str(header.value.as_str()),
        ) {
            builder = builder.header(name, value);
        }
    }
    if is_stream {
        builder = builder.header(
            HeaderName::from_static("accept"),
            HeaderValue::from_static("text/event-stream"),
        );
    }

    let secret_trimmed = secret.trim();
    match auth_config {
        AggregateApiAuthConfig::ApiKeyDefaultBearer => {
            builder = builder.header(
                HeaderName::from_static("authorization"),
                HeaderValue::from_str(format!("Bearer {}", secret_trimmed).as_str())
                    .map_err(|_| "invalid aggregate api secret".to_string())?,
            );
        }
        AggregateApiAuthConfig::ApiKeyHeader { name, format } => {
            let header_name = HeaderName::from_bytes(name.as_bytes())
                .map_err(|_| "invalid aggregate api auth header".to_string())?;
            let value = if format == "raw" {
                secret_trimmed.to_string()
            } else {
                format!("Bearer {}", secret_trimmed)
            };
            builder = builder.header(
                header_name,
                HeaderValue::from_str(value.as_str())
                    .map_err(|_| "invalid aggregate api secret".to_string())?,
            );
        }
        AggregateApiAuthConfig::ApiKeyQuery { .. } => {}
        AggregateApiAuthConfig::UserPassBasic
        | AggregateApiAuthConfig::UserPassHeaderPair { .. }
        | AggregateApiAuthConfig::UserPassQueryPair { .. } => {
            let parsed: UserPassSecret = serde_json::from_str(secret_trimmed)
                .map_err(|_| "invalid aggregate api secret".to_string())?;
            match auth_config {
                AggregateApiAuthConfig::UserPassBasic => {
                    builder = builder.basic_auth(parsed.username, Some(parsed.password));
                }
                AggregateApiAuthConfig::UserPassHeaderPair {
                    username_name,
                    password_name,
                } => {
                    let user_header = HeaderName::from_bytes(username_name.as_bytes())
                        .map_err(|_| "invalid aggregate api auth header".to_string())?;
                    let pass_header = HeaderName::from_bytes(password_name.as_bytes())
                        .map_err(|_| "invalid aggregate api auth header".to_string())?;
                    builder = builder.header(
                        user_header,
                        HeaderValue::from_str(parsed.username.as_str())
                            .map_err(|_| "invalid aggregate api secret".to_string())?,
                    );
                    builder = builder.header(
                        pass_header,
                        HeaderValue::from_str(parsed.password.as_str())
                            .map_err(|_| "invalid aggregate api secret".to_string())?,
                    );
                }
                AggregateApiAuthConfig::UserPassQueryPair { .. } => {}
                _ => {}
            }
        }
    }
    if !body.is_empty() {
        builder = builder.body(body.clone());
    }
    Ok(builder)
}

fn build_anthropic_bridge_aggregate_api_request(
    client: &reqwest::blocking::Client,
    request: &Request,
    method: &reqwest::Method,
    url: reqwest::Url,
    body: &Bytes,
    secret: &str,
    auth_config: &AggregateApiAuthConfig,
    injected_headers: &HashSet<String>,
    request_deadline: Option<Instant>,
    is_stream: bool,
) -> Result<reqwest::blocking::RequestBuilder, String> {
    let mut builder = build_aggregate_api_request(
        client,
        request,
        method,
        url,
        body,
        secret,
        auth_config,
        injected_headers,
        request_deadline,
        is_stream,
    )?;
    builder = builder.header(
        HeaderName::from_static("anthropic-version"),
        HeaderValue::from_static("2023-06-01"),
    );
    if matches!(auth_config, AggregateApiAuthConfig::ApiKeyDefaultBearer) {
        builder = builder.header(
            HeaderName::from_static("x-api-key"),
            HeaderValue::from_str(secret.trim())
                .map_err(|_| "invalid aggregate api secret".to_string())?,
        );
    }
    Ok(builder)
}

/// 函数 `resolve_aggregate_api_rotation_candidates`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - crate: 参数 crate
///
/// # 返回
/// 返回函数执行结果
pub(crate) fn resolve_aggregate_api_rotation_candidates(
    storage: &Storage,
    protocol_type: &str,
    aggregate_api_id: Option<&str>,
) -> Result<Vec<AggregateApi>, String> {
    let provider_type = match protocol_type {
        "anthropic_native" => AGGREGATE_API_PROVIDER_CLAUDE,
        "gemini_native" => AGGREGATE_API_PROVIDER_GEMINI,
        _ => AGGREGATE_API_PROVIDER_CODEX,
    };

    let mut candidates = storage
        .list_active_aggregate_apis_by_provider_type(provider_type)
        .map_err(|err| err.to_string())?
        .into_iter()
        .collect::<Vec<_>>();
    candidates = normalize_candidate_order(candidates);

    if let Some(api_id) = aggregate_api_id
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        promote_preferred_aggregate_candidate(&mut candidates, api_id);
    }

    if candidates.is_empty() {
        Err(format!(
            "aggregate api not found for provider {provider_type}"
        ))
    } else {
        Ok(candidates)
    }
}

/// 函数 `proxy_aggregate_request`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - in super: 参数 in super
///
/// # 返回
/// 返回函数执行结果
pub(in super::super) struct AggregateProxyRequest<'a> {
    pub request: Request,
    pub storage: &'a Storage,
    pub trace_id: &'a str,
    pub key_id: &'a str,
    pub original_path: &'a str,
    pub path: &'a str,
    pub request_method: &'a str,
    pub method: &'a reqwest::Method,
    pub body: &'a Bytes,
    pub is_stream: bool,
    pub response_adapter: super::super::super::ResponseAdapter,
    pub gateway_mode_for_log: Option<&'a str>,
    pub route_strategy_for_log: Option<&'a str>,
    pub route_source_for_log: Option<&'a str>,
    pub client_model_for_log: Option<&'a str>,
    pub model_for_log: Option<&'a str>,
    pub model_source_for_log: Option<&'a str>,
    pub client_reasoning_for_log: Option<&'a str>,
    pub reasoning_for_log: Option<&'a str>,
    pub reasoning_source_for_log: Option<&'a str>,
    pub service_tier_for_log: Option<&'a str>,
    pub effective_service_tier_for_log: Option<&'a str>,
    pub service_tier_source_for_log: Option<&'a str>,
    pub aggregate_api_candidates: Vec<AggregateApi>,
    pub defer_exhaustion_response: bool,
    pub request_deadline: Option<Instant>,
    pub started_at: Instant,
}

pub(in super::super) enum AggregateProxyResult {
    Handled,
    Exhausted {
        request: Box<Request>,
        message: String,
    },
}

pub(in super::super) fn proxy_aggregate_request(
    params: AggregateProxyRequest<'_>,
) -> Result<AggregateProxyResult, String> {
    let AggregateProxyRequest {
        request,
        storage,
        trace_id,
        key_id,
        original_path,
        path,
        request_method,
        method,
        body,
        is_stream,
        response_adapter,
        gateway_mode_for_log,
        route_strategy_for_log,
        route_source_for_log,
        client_model_for_log,
        model_for_log,
        model_source_for_log,
        client_reasoning_for_log,
        reasoning_for_log,
        reasoning_source_for_log,
        service_tier_for_log,
        effective_service_tier_for_log,
        service_tier_source_for_log,
        aggregate_api_candidates,
        defer_exhaustion_response,
        request_deadline,
        started_at,
    } = params;
    let estimated_input_tokens =
        super::super::super::request_log::estimate_input_tokens_from_body(body.as_ref());
    if aggregate_api_candidates.is_empty() {
        let message = "aggregate api not found".to_string();
        if defer_exhaustion_response {
            return Ok(AggregateProxyResult::Exhausted {
                request: Box::new(request),
                message,
            });
        }
        super::super::super::record_gateway_request_outcome(path, 404, Some("aggregate_api"));
        super::super::super::trace_log::log_request_final(
            trace_id,
            404,
            Some(key_id),
            None,
            Some(message.as_str()),
            started_at.elapsed().as_millis(),
        );
        let request = request;
        respond_error(request, 404, message.as_str(), Some(trace_id));
        return Ok(AggregateProxyResult::Handled);
    }

    let mut request = Some(request);
    let mut attempted_aggregate_api_ids = Vec::new();
    let mut daily_limit_tracker = AggregateApiDailyLimitTracker::new(storage, trace_id);
    let mut last_attempt_url: Option<String> = None;
    let mut last_attempt_id: Option<String> = None;
    let mut last_attempt_upstream_model: Option<String> = None;
    let mut last_attempt_supplier_name: Option<String> = None;
    let mut last_attempt_error: Option<String> = None;
    let mut last_failure_status = 502u16;

    let total_candidates = aggregate_api_candidates.len();
    let secrets_by_candidate_id =
        aggregate_api_secrets_by_candidate_id(storage, &aggregate_api_candidates)?;
    let ordered_candidates = aggregate_api_candidates
        .iter()
        .map(|candidate| (candidate.id.clone(), candidate.url.clone()))
        .collect::<Vec<_>>();
    for (candidate_idx, candidate) in aggregate_api_candidates.into_iter().enumerate() {
        if daily_limit_tracker.has_pending_failure(candidate.id.as_str()) {
            log::warn!(
                "event=gateway_aggregate_daily_limit_duplicate_skipped trace_id={} aggregate_api_id={}",
                trace_id,
                candidate.id,
            );
            continue;
        }
        prepare_next_aggregate_candidate_client(
            ordered_candidates.as_slice(),
            candidate_idx,
            trace_id,
        );
        attempted_aggregate_api_ids.push(candidate.id.clone());
        let candidate_id = candidate.id.clone();
        let candidate_upstream_model =
            aggregate_upstream_model_for_log(&candidate, model_for_log).map(str::to_string);
        let candidate_supplier_name = candidate.supplier_name.clone();
        let candidate_url = candidate.url.clone();
        let client = super::super::super::upstream_client_for_aggregate_api_candidate(
            candidate_id.as_str(),
            candidate_url.as_str(),
        );
        last_attempt_id = Some(candidate_id.clone());
        last_attempt_upstream_model = candidate_upstream_model.clone();
        let Some(secret) = secrets_by_candidate_id.get(candidate.id.as_str()) else {
            last_attempt_url = Some(candidate_url.clone());
            last_attempt_supplier_name = candidate_supplier_name.clone();
            last_attempt_error = Some("aggregate api secret not found".to_string());
            last_failure_status = 403;
            continue;
        };

        let bridge_responses_to_anthropic = should_bridge_responses_to_anthropic(&candidate, path);
        let effective_path = if bridge_responses_to_anthropic {
            responses_to_anthropic_messages_action_path(&candidate, path)
        } else {
            effective_action_path(&candidate, path)
        };
        let response_adapter_for_candidate = if bridge_responses_to_anthropic {
            super::super::super::ResponseAdapter::ResponsesFromAnthropicMessages
        } else {
            response_adapter
        };
        let (auth_config, injected_headers) = match parse_auth_config(&candidate) {
            Ok(value) => value,
            Err(err) => {
                last_attempt_url = Some(candidate_url.clone());
                last_attempt_supplier_name = candidate_supplier_name.clone();
                last_attempt_error = Some(err);
                last_failure_status = 502;
                continue;
            }
        };

        let base_upstream_url =
            match build_upstream_url(candidate_url.as_str(), effective_path.as_str()) {
                Ok(url) => url,
                Err(_) => {
                    last_attempt_url = Some(candidate_url.clone());
                    last_attempt_supplier_name = candidate_supplier_name.clone();
                    last_attempt_error = Some("invalid aggregate api url".to_string());
                    last_failure_status = 502;
                    continue;
                }
            };
        let candidate_body = rewrite_body_for_candidate_transport(
            body,
            &candidate,
            path,
            base_upstream_url.as_str(),
        );
        let candidate_body = rewrite_minimax_responses_body(
            &candidate_body,
            candidate.url.as_str(),
            candidate.supplier_name.as_deref(),
            path,
        );
        let upstream_body = if bridge_responses_to_anthropic {
            match adapt_openai_responses_to_anthropic_messages(
                candidate_body.as_ref(),
                candidate.model_override.as_deref(),
            ) {
                Ok(body) => Bytes::from(body),
                Err(err) => {
                    last_attempt_url = Some(base_upstream_url.to_string());
                    last_attempt_supplier_name = candidate_supplier_name.clone();
                    last_attempt_error = Some(err);
                    last_failure_status = 502;
                    continue;
                }
            }
        } else {
            candidate_body
        };

        let mut succeeded = false;
        for attempt_idx in 0..=AGGREGATE_API_RETRY_ATTEMPTS_PER_CHANNEL {
            if super::super::support::deadline::is_expired(request_deadline) {
                let message = "aggregate api request timeout".to_string();
                let request = request.take().ok_or_else(|| {
                    "aggregate api request already consumed before timeout response".to_string()
                })?;
                if defer_exhaustion_response {
                    return Ok(AggregateProxyResult::Exhausted {
                        request: Box::new(request),
                        message,
                    });
                }
                super::super::super::record_gateway_request_outcome(
                    path,
                    504,
                    Some("aggregate_api"),
                );
                super::super::super::trace_log::log_request_final(
                    trace_id,
                    504,
                    Some(key_id),
                    Some(candidate_url.as_str()),
                    Some(message.as_str()),
                    started_at.elapsed().as_millis(),
                );
                super::super::super::write_request_log(
                    storage,
                    super::super::super::request_log::RequestLogTraceContext {
                        trace_id: Some(trace_id),
                        original_path: Some(original_path),
                        adapted_path: Some(path),
                        gateway_mode: gateway_mode_for_log,
                        route_strategy: route_strategy_for_log,
                        route_source: route_source_for_log,
                        client_model: client_model_for_log,
                        model_source: model_source_for_log,
                        client_reasoning_effort: client_reasoning_for_log,
                        reasoning_source: reasoning_source_for_log,
                        response_adapter: Some(response_adapter),
                        service_tier: service_tier_for_log,
                        effective_service_tier: effective_service_tier_for_log,
                        service_tier_source: service_tier_source_for_log,
                        aggregate_api_supplier_name: candidate_supplier_name.as_deref(),
                        aggregate_api_url: Some(candidate_url.as_str()),
                        attempted_aggregate_api_ids: Some(attempted_aggregate_api_ids.as_slice()),
                        upstream_model: candidate_upstream_model.as_deref(),
                        actual_source_kind: Some("aggregate_api"),
                        actual_source_id: Some(candidate_id.as_str()),
                        ..Default::default()
                    },
                    Some(key_id),
                    None,
                    path,
                    request_method,
                    model_for_log,
                    reasoning_for_log,
                    Some(candidate_url.as_str()),
                    Some(504),
                    RequestLogUsage {
                        estimated_input_tokens: Some(estimated_input_tokens),
                        ..Default::default()
                    },
                    Some(message.as_str()),
                    Some(started_at.elapsed().as_millis()),
                );
                respond_error(request, 504, message.as_str(), Some(trace_id));
                return Ok(AggregateProxyResult::Handled);
            }

            let mut url = base_upstream_url.clone();

            match &auth_config {
                AggregateApiAuthConfig::ApiKeyQuery { name } => {
                    url = replace_query_param(url, name.as_str(), secret.trim());
                }
                AggregateApiAuthConfig::UserPassQueryPair {
                    username_name,
                    password_name,
                } => {
                    let parsed: UserPassSecret = match serde_json::from_str(secret.trim()) {
                        Ok(parsed) => parsed,
                        Err(_) => {
                            last_attempt_url = Some(url.as_str().to_string());
                            last_attempt_supplier_name = candidate_supplier_name.clone();
                            last_attempt_error = Some("invalid aggregate api secret".to_string());
                            last_failure_status = 502;
                            break;
                        }
                    };
                    url =
                        replace_query_param(url, username_name.as_str(), parsed.username.as_str());
                    url =
                        replace_query_param(url, password_name.as_str(), parsed.password.as_str());
                }
                _ => {}
            }

            let request_ref = request.as_ref().ok_or_else(|| {
                "aggregate api request already consumed before upstream attempt".to_string()
            })?;
            let builder = if bridge_responses_to_anthropic {
                build_anthropic_bridge_aggregate_api_request(
                    &client,
                    request_ref,
                    method,
                    url.clone(),
                    &upstream_body,
                    secret.as_str(),
                    &auth_config,
                    &injected_headers,
                    request_deadline,
                    is_stream,
                )
            } else {
                build_aggregate_api_request(
                    &client,
                    request_ref,
                    method,
                    url.clone(),
                    &upstream_body,
                    secret.as_str(),
                    &auth_config,
                    &injected_headers,
                    request_deadline,
                    is_stream,
                )
            };
            let builder = match builder {
                Ok(builder) => builder,
                Err(err) => {
                    last_attempt_url = Some(url.as_str().to_string());
                    last_attempt_supplier_name = candidate_supplier_name.clone();
                    last_attempt_error = Some(err);
                    last_failure_status = 502;
                    break;
                }
            };

            let attempt_started_at = Instant::now();
            let upstream = match builder.send() {
                Ok(resp) => {
                    let duration_ms =
                        super::super::super::duration_to_millis(attempt_started_at.elapsed());
                    super::super::super::metrics::record_gateway_upstream_attempt(
                        duration_ms,
                        false,
                    );
                    resp
                }
                Err(err) => {
                    let duration_ms =
                        super::super::super::duration_to_millis(attempt_started_at.elapsed());
                    super::super::super::metrics::record_gateway_upstream_attempt(
                        duration_ms,
                        true,
                    );
                    let message = format!("aggregate api upstream error: {err}");
                    last_attempt_url = Some(url.as_str().to_string());
                    last_attempt_supplier_name = candidate_supplier_name.clone();
                    last_attempt_error = Some(message);
                    last_failure_status = 502;
                    if attempt_idx < AGGREGATE_API_RETRY_ATTEMPTS_PER_CHANNEL {
                        continue;
                    }
                    break;
                }
            };

            if !upstream.status().is_success() {
                let status_code = upstream.status().as_u16();
                let upstream_request_id = first_upstream_header(
                    upstream.headers(),
                    &["x-request-id", "x-oai-request-id"],
                );
                let upstream_cf_ray = first_upstream_header(upstream.headers(), &["cf-ray"]);
                let upstream_auth_error =
                    first_upstream_header(upstream.headers(), &["x-openai-authorization-error"]);
                let upstream_identity_error_code =
                    crate::gateway::extract_identity_error_code_from_headers(upstream.headers());
                let upstream_body = match upstream.bytes() {
                    Ok(body) => body,
                    Err(err) => {
                        last_attempt_url = Some(url.as_str().to_string());
                        last_attempt_supplier_name = candidate_supplier_name.clone();
                        last_attempt_error =
                            Some(format!("read aggregate api upstream body failed: {err}"));
                        last_failure_status = 502;
                        if attempt_idx < AGGREGATE_API_RETRY_ATTEMPTS_PER_CHANNEL {
                            continue;
                        }
                        break;
                    }
                };
                if candidate.auto_toggle_enabled {
                    if let Some(reason) = classify_aggregate_api_daily_limit_failure(
                        status_code,
                        upstream_body.as_ref(),
                    ) {
                        daily_limit_tracker.mark_failure(&candidate_id, reason);
                    }
                }
                let message = aggregate_api_failure_message(
                    status_code,
                    upstream_body.as_ref(),
                    upstream_request_id.as_deref(),
                    upstream_cf_ray.as_deref(),
                    upstream_auth_error.as_deref(),
                    upstream_identity_error_code.as_deref(),
                );
                last_attempt_url = Some(url.as_str().to_string());
                last_attempt_supplier_name = candidate_supplier_name.clone();
                last_attempt_error = Some(message);
                last_failure_status = 502;
                if attempt_idx < AGGREGATE_API_RETRY_ATTEMPTS_PER_CHANNEL {
                    continue;
                }
                break;
            }

            let upstream = GatewayUpstreamResponse::Blocking(upstream);
            let upstream = if !is_stream && defer_exhaustion_response {
                match upstream.into_buffered() {
                    Ok((buffered_body, buffered)) => {
                        if candidate.auto_toggle_enabled {
                            match classify_complete_aggregate_api_response(
                                buffered_body.as_ref(),
                                buffered,
                            ) {
                                AggregateApiStreamPreflightOutcome::Ready(buffered) => buffered,
                                AggregateApiStreamPreflightOutcome::DailyLimit {
                                    reason,
                                    message,
                                } => {
                                    daily_limit_tracker.mark_failure(&candidate_id, reason);
                                    last_attempt_url = Some(url.as_str().to_string());
                                    last_attempt_supplier_name = candidate_supplier_name.clone();
                                    last_attempt_error = Some(message);
                                    last_failure_status = 502;
                                    if attempt_idx < AGGREGATE_API_RETRY_ATTEMPTS_PER_CHANNEL {
                                        continue;
                                    }
                                    break;
                                }
                                AggregateApiStreamPreflightOutcome::TransportFailure(message) => {
                                    last_attempt_url = Some(url.as_str().to_string());
                                    last_attempt_supplier_name = candidate_supplier_name.clone();
                                    last_attempt_error = Some(message);
                                    last_failure_status = 502;
                                    if attempt_idx < AGGREGATE_API_RETRY_ATTEMPTS_PER_CHANNEL {
                                        continue;
                                    }
                                    break;
                                }
                            }
                        } else {
                            buffered
                        }
                    }
                    Err(err) => {
                        last_attempt_url = Some(url.as_str().to_string());
                        last_attempt_supplier_name = candidate_supplier_name.clone();
                        last_attempt_error = Some(err);
                        last_failure_status = 502;
                        if attempt_idx < AGGREGATE_API_RETRY_ATTEMPTS_PER_CHANNEL {
                            continue;
                        }
                        break;
                    }
                }
            } else if !is_stream && candidate.auto_toggle_enabled {
                match preflight_aggregate_api_non_stream(upstream) {
                    AggregateApiStreamPreflightOutcome::Ready(upstream) => upstream,
                    AggregateApiStreamPreflightOutcome::DailyLimit { reason, message } => {
                        daily_limit_tracker.mark_failure(&candidate_id, reason);
                        last_attempt_url = Some(url.as_str().to_string());
                        last_attempt_supplier_name = candidate_supplier_name.clone();
                        last_attempt_error = Some(message);
                        last_failure_status = 502;
                        if attempt_idx < AGGREGATE_API_RETRY_ATTEMPTS_PER_CHANNEL {
                            continue;
                        }
                        break;
                    }
                    AggregateApiStreamPreflightOutcome::TransportFailure(message) => {
                        last_attempt_url = Some(url.as_str().to_string());
                        last_attempt_supplier_name = candidate_supplier_name.clone();
                        last_attempt_error = Some(message);
                        last_failure_status = 502;
                        if attempt_idx < AGGREGATE_API_RETRY_ATTEMPTS_PER_CHANNEL {
                            continue;
                        }
                        break;
                    }
                }
            } else {
                upstream
            };

            let upstream = if is_stream && candidate.auto_toggle_enabled {
                match preflight_aggregate_api_stream(upstream) {
                    AggregateApiStreamPreflightOutcome::Ready(upstream) => upstream,
                    AggregateApiStreamPreflightOutcome::DailyLimit { reason, message } => {
                        daily_limit_tracker.mark_failure(&candidate_id, reason);
                        last_attempt_url = Some(url.as_str().to_string());
                        last_attempt_supplier_name = candidate_supplier_name.clone();
                        last_attempt_error = Some(message);
                        last_failure_status = 502;
                        if attempt_idx < AGGREGATE_API_RETRY_ATTEMPTS_PER_CHANNEL {
                            continue;
                        }
                        break;
                    }
                    AggregateApiStreamPreflightOutcome::TransportFailure(message) => {
                        last_attempt_url = Some(url.as_str().to_string());
                        last_attempt_supplier_name = candidate_supplier_name.clone();
                        last_attempt_error = Some(message);
                        last_failure_status = 502;
                        if attempt_idx < AGGREGATE_API_RETRY_ATTEMPTS_PER_CHANNEL {
                            continue;
                        }
                        break;
                    }
                }
            } else {
                upstream
            };

            let inflight_guard = super::super::super::acquire_account_inflight(key_id);
            let passthrough_sse_protocol =
                resolve_passthrough_sse_protocol(path, response_adapter_for_candidate);
            let request = request.take().ok_or_else(|| {
                "aggregate api request already consumed before bridge".to_string()
            })?;
            let bridge = super::super::super::respond_with_upstream(
                request,
                upstream,
                inflight_guard,
                response_adapter_for_candidate,
                passthrough_sse_protocol,
                None,
                path,
                None,
                is_stream,
                false,
                Some(trace_id),
                None,
                started_at,
            )?;
            let bridge_output_text_len = bridge
                .usage
                .output_text
                .as_deref()
                .map(str::trim)
                .map(str::len)
                .unwrap_or(0);
            super::super::super::trace_log::log_bridge_result(
                super::super::super::trace_log::BridgeResultLog {
                    trace_id,
                    adapter: format!("{response_adapter_for_candidate:?}").as_str(),
                    path,
                    is_stream,
                    stream_terminal_seen: bridge.stream_terminal_seen,
                    stream_terminal_error: bridge.stream_terminal_error.as_deref(),
                    delivery_error: bridge.delivery_error.as_deref(),
                    output_text_len: bridge_output_text_len,
                    output_tokens: bridge.usage.output_tokens,
                    first_response_ms: bridge.usage.first_response_ms,
                    delivered_status_code: bridge.delivered_status_code,
                    upstream_error_hint: bridge.upstream_error_hint.as_deref(),
                    upstream_request_id: bridge.upstream_request_id.as_deref(),
                    upstream_cf_ray: bridge.upstream_cf_ray.as_deref(),
                    upstream_auth_error: bridge.upstream_auth_error.as_deref(),
                    upstream_identity_error_code: bridge.upstream_identity_error_code.as_deref(),
                    upstream_content_type: bridge.upstream_content_type.as_deref(),
                    last_sse_event_type: bridge.last_sse_event_type.as_deref(),
                },
            );
            let bridge_ok = bridge.is_ok(is_stream);
            let mut final_error = bridge.upstream_error_hint.clone();
            if final_error.is_none() && !bridge_ok {
                final_error =
                    Some(bridge.error_message(is_stream).unwrap_or_else(|| {
                        "aggregate api upstream response incomplete".to_string()
                    }));
            }
            let status_code =
                bridge
                    .delivered_status_code
                    .unwrap_or(if bridge_ok { 200 } else { 502 });
            let status_code = if final_error.is_some() && status_code < 400 {
                502
            } else {
                status_code
            };
            let usage = bridge.usage;

            let daily_limit_reason = final_error
                .as_deref()
                .and_then(classify_aggregate_api_daily_limit_hint);
            if bridge_ok && final_error.is_none() {
                daily_limit_tracker.mark_success(&candidate_id);
            } else if candidate.auto_toggle_enabled {
                if let Some(reason) = daily_limit_reason {
                    daily_limit_tracker.mark_failure(&candidate_id, reason);
                }
            }

            super::super::super::record_gateway_request_outcome(
                path,
                status_code,
                Some("aggregate_api"),
            );
            super::super::super::trace_log::log_request_final(
                trace_id,
                status_code,
                Some(key_id),
                Some(url.as_str()),
                final_error.as_deref(),
                started_at.elapsed().as_millis(),
            );
            super::super::super::write_request_log(
                storage,
                super::super::super::request_log::RequestLogTraceContext {
                    trace_id: Some(trace_id),
                    original_path: Some(original_path),
                    adapted_path: Some(path),
                    gateway_mode: gateway_mode_for_log,
                    route_strategy: route_strategy_for_log,
                    route_source: route_source_for_log,
                    client_model: client_model_for_log,
                    model_source: model_source_for_log,
                    client_reasoning_effort: client_reasoning_for_log,
                    reasoning_source: reasoning_source_for_log,
                    response_adapter: Some(response_adapter_for_candidate),
                    service_tier: service_tier_for_log,
                    effective_service_tier: effective_service_tier_for_log,
                    service_tier_source: service_tier_source_for_log,
                    aggregate_api_supplier_name: candidate_supplier_name.as_deref(),
                    aggregate_api_url: Some(candidate_url.as_str()),
                    attempted_aggregate_api_ids: Some(attempted_aggregate_api_ids.as_slice()),
                    upstream_model: candidate_upstream_model.as_deref(),
                    actual_source_kind: Some("aggregate_api"),
                    actual_source_id: Some(candidate_id.as_str()),
                    ..Default::default()
                },
                Some(key_id),
                None,
                path,
                request_method,
                model_for_log,
                reasoning_for_log,
                Some(url.as_str()),
                Some(status_code),
                RequestLogUsage {
                    input_tokens: usage.input_tokens,
                    cached_input_tokens: usage.cached_input_tokens,
                    output_tokens: usage.output_tokens,
                    total_tokens: usage.total_tokens,
                    reasoning_output_tokens: usage.reasoning_output_tokens,
                    first_response_ms: usage.first_response_ms,
                    estimated_input_tokens: Some(estimated_input_tokens),
                },
                final_error.as_deref(),
                Some(started_at.elapsed().as_millis()),
            );
            succeeded = true;
            break;
        }

        if succeeded {
            return Ok(AggregateProxyResult::Handled);
        }

        if candidate_idx + 1 < total_candidates {
            super::super::super::record_gateway_failover_attempt();
        }
    }

    let message =
        last_attempt_error.unwrap_or_else(|| "aggregate api upstream response failed".to_string());
    let status_code = last_failure_status;
    let request = request.take().ok_or_else(|| {
        "aggregate api request already consumed before failure response".to_string()
    })?;
    if defer_exhaustion_response {
        return Ok(AggregateProxyResult::Exhausted {
            request: Box::new(request),
            message,
        });
    }
    super::super::super::record_gateway_request_outcome(path, status_code, Some("aggregate_api"));
    super::super::super::trace_log::log_request_final(
        trace_id,
        status_code,
        Some(key_id),
        last_attempt_url.as_deref(),
        Some(message.as_str()),
        started_at.elapsed().as_millis(),
    );
    super::super::super::write_request_log(
        storage,
        super::super::super::request_log::RequestLogTraceContext {
            trace_id: Some(trace_id),
            original_path: Some(original_path),
            adapted_path: Some(path),
            gateway_mode: gateway_mode_for_log,
            route_strategy: route_strategy_for_log,
            route_source: route_source_for_log,
            client_model: client_model_for_log,
            model_source: model_source_for_log,
            client_reasoning_effort: client_reasoning_for_log,
            reasoning_source: reasoning_source_for_log,
            response_adapter: Some(response_adapter),
            service_tier: service_tier_for_log,
            effective_service_tier: effective_service_tier_for_log,
            service_tier_source: service_tier_source_for_log,
            aggregate_api_supplier_name: last_attempt_supplier_name.as_deref(),
            aggregate_api_url: last_attempt_url.as_deref(),
            attempted_aggregate_api_ids: Some(attempted_aggregate_api_ids.as_slice()),
            upstream_model: last_attempt_upstream_model.as_deref(),
            actual_source_kind: last_attempt_id.as_deref().map(|_| "aggregate_api"),
            actual_source_id: last_attempt_id.as_deref(),
            ..Default::default()
        },
        Some(key_id),
        None,
        path,
        request_method,
        model_for_log,
        reasoning_for_log,
        last_attempt_url.as_deref(),
        Some(status_code),
        RequestLogUsage {
            estimated_input_tokens: Some(estimated_input_tokens),
            ..Default::default()
        },
        Some(message.as_str()),
        Some(started_at.elapsed().as_millis()),
    );
    respond_error(request, status_code, message.as_str(), Some(trace_id));
    Ok(AggregateProxyResult::Handled)
}

fn aggregate_api_secrets_by_candidate_id(
    storage: &Storage,
    candidates: &[AggregateApi],
) -> Result<HashMap<String, String>, String> {
    let candidate_ids = candidates
        .iter()
        .map(|candidate| candidate.id.clone())
        .collect::<Vec<_>>();
    storage
        .list_aggregate_api_secrets_for_ids(&candidate_ids)
        .map_err(|err| err.to_string())
}

#[cfg(test)]
mod bridge_tests {
    use super::*;
    use crate::gateway::upstream::response::{GatewayByteStream, GatewayStreamResponse};

    fn buffered_upstream_response(
        body: &'static [u8],
        content_type: &'static str,
    ) -> GatewayUpstreamResponse {
        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert(
            reqwest::header::CONTENT_TYPE,
            HeaderValue::from_static(content_type),
        );
        GatewayUpstreamResponse::Stream(GatewayStreamResponse::new(
            reqwest::StatusCode::OK,
            headers,
            GatewayByteStream::from_bytes(Bytes::from_static(body)),
        ))
    }

    /// 函数 `candidate`
    ///
    /// 作者: gaohongshun
    ///
    /// 时间: 2026-04-02
    ///
    /// # 参数
    /// - id: 参数 id
    /// - sort: 参数 sort
    ///
    /// # 返回
    /// 返回函数执行结果
    fn candidate(id: &str, sort: i64) -> AggregateApi {
        AggregateApi {
            id: id.to_string(),
            provider_type: AGGREGATE_API_PROVIDER_CODEX.to_string(),
            supplier_name: None,
            sort,
            url: format!("https://{id}.example.com"),
            auth_type: AGGREGATE_API_AUTH_APIKEY.to_string(),
            auth_params_json: None,
            action: None,
            model_override: None,
            status: "active".to_string(),
            auto_toggle_enabled: false,
            consecutive_failures: 0,
            auto_disabled: false,
            auto_disabled_at: None,
            auto_disabled_reason: None,
            created_at: sort,
            updated_at: sort,
            last_test_at: None,
            last_test_status: None,
            last_test_error: None,
            balance_query_enabled: false,
            balance_query_template: None,
            balance_query_base_url: None,
            balance_query_user_id: None,
            balance_query_config_json: None,
            last_balance_at: None,
            last_balance_status: None,
            last_balance_error: None,
            last_balance_json: None,
        }
    }

    #[test]
    fn daily_limit_failure_is_counted_once_per_api_id_within_one_request() {
        let storage = Storage::open_in_memory().expect("open storage");
        storage.init().expect("init storage");
        let mut api = candidate("agg-deduplicated", 0);
        api.auto_toggle_enabled = true;
        storage.insert_aggregate_api(&api).expect("insert API");
        {
            let mut tracker = AggregateApiDailyLimitTracker::new(&storage, "trace-deduplicated");
            for _ in 0..2 {
                tracker.mark_failure(&api.id, "daily_quota_exceeded");
            }
        }

        let stored = storage
            .find_aggregate_api_by_id(&api.id)
            .expect("read API")
            .expect("API exists");
        assert_eq!(stored.consecutive_failures, 1);
    }

    #[test]
    fn later_success_for_same_api_cancels_pending_daily_limit_failure() {
        let storage = Storage::open_in_memory().expect("open storage");
        storage.init().expect("init storage");
        let mut api = candidate("agg-eventual-success", 0);
        api.auto_toggle_enabled = true;
        storage.insert_aggregate_api(&api).expect("insert API");

        {
            let mut tracker =
                AggregateApiDailyLimitTracker::new(&storage, "trace-eventual-success");
            tracker.mark_failure(&api.id, "daily_quota_exceeded");
            tracker.mark_success(&api.id);
        }

        let stored = storage
            .find_aggregate_api_by_id(&api.id)
            .expect("read API")
            .expect("API exists");
        assert_eq!(stored.consecutive_failures, 0);
        assert!(!stored.auto_disabled);
    }

    #[test]
    fn stream_preflight_detects_daily_limit_before_content() {
        let prefix = br#"event: response.created
data: {"type":"response.created","response":{"id":"resp_1"}}

event: response.failed
data: {"type":"response.failed","response":{"error":{"code":"DAILY_LIMIT_EXCEEDED","message":"daily usage limit exceeded"}}}

"#;
        assert!(matches!(
            classify_aggregate_api_stream_prefix(
                prefix,
                false,
                AggregateApiStreamInspectionMode::CommitOnContent,
            ),
            AggregateApiStreamPrefixDecision::DailyLimit { .. }
        ));
    }

    #[test]
    fn stream_preflight_commits_when_bounded_inspection_stops() {
        let metadata = b"event: response.created\ndata: {\"type\":\"response.created\"}\n\n";
        for terminal in [
            GatewayStreamPrefetchTerminal::PrefixLimit,
            GatewayStreamPrefetchTerminal::WallClockTimeout,
        ] {
            assert!(matches!(
                classify_prefetched_aggregate_api_sse(
                    metadata,
                    buffered_upstream_response(metadata, "text/event-stream"),
                    terminal,
                    AggregateApiStreamInspectionMode::CommitOnContent,
                ),
                AggregateApiStreamPreflightOutcome::Ready(_)
            ));
        }
    }

    #[test]
    fn non_stream_preflight_commits_when_bounded_inspection_stops() {
        let partial_json = br#"{"id":"response-1""#;
        for terminal in [
            GatewayStreamPrefetchTerminal::PrefixLimit,
            GatewayStreamPrefetchTerminal::WallClockTimeout,
        ] {
            assert!(matches!(
                classify_prefetched_aggregate_api_non_stream(
                    partial_json,
                    buffered_upstream_response(partial_json, "application/json"),
                    terminal,
                ),
                AggregateApiStreamPreflightOutcome::Ready(_)
            ));
        }
    }

    #[test]
    fn stream_preflight_commits_after_content_and_fails_over_transient_errors() {
        let content_then_error = br#"event: response.output_text.delta
data: {"type":"response.output_text.delta","delta":"hello"}

event: response.failed
data: {"type":"response.failed","response":{"error":{"code":"DAILY_LIMIT_EXCEEDED"}}}

"#;
        assert_eq!(
            classify_aggregate_api_stream_prefix(
                content_then_error,
                false,
                AggregateApiStreamInspectionMode::CommitOnContent,
            ),
            AggregateApiStreamPrefixDecision::Deliver
        );
        assert!(matches!(
            classify_aggregate_api_stream_prefix(
                content_then_error,
                false,
                AggregateApiStreamInspectionMode::CompleteBeforeDelivery,
            ),
            AggregateApiStreamPrefixDecision::DailyLimit { .. }
        ));

        let concurrency_limit = br#"event: error
data: {"type":"error","error":{"code":"rate_limit_exceeded","message":"Concurrency limit exceeded"}}

"#;
        assert!(matches!(
            classify_aggregate_api_stream_prefix(
                concurrency_limit,
                false,
                AggregateApiStreamInspectionMode::CommitOnContent,
            ),
            AggregateApiStreamPrefixDecision::UpstreamError(_)
        ));

        let root_code_concurrency_limit =
            br#"data: {"code":"rate_limit_exceeded","message":"Concurrency limit exceeded"}

"#;
        assert!(matches!(
            classify_aggregate_api_stream_prefix(
                root_code_concurrency_limit,
                false,
                AggregateApiStreamInspectionMode::CommitOnContent,
            ),
            AggregateApiStreamPrefixDecision::UpstreamError(_)
        ));
    }

    #[test]
    fn chat_stream_role_metadata_does_not_hide_a_daily_limit_error() {
        let prefix = br#"data: {"choices":[{"delta":{"role":"assistant"},"finish_reason":null}]}

data: {"error":{"type":"billing_error","message":"daily usage limit exceeded"}}

"#;
        assert!(matches!(
            classify_aggregate_api_stream_prefix(
                prefix,
                false,
                AggregateApiStreamInspectionMode::CommitOnContent,
            ),
            AggregateApiStreamPrefixDecision::DailyLimit { .. }
        ));
    }

    #[test]
    fn stream_preflight_requires_error_context_for_daily_limit_text() {
        let ordinary_message = br#"data: {"message":"daily usage limit exceeded"}

"#;
        assert_eq!(
            classify_aggregate_api_stream_prefix(
                ordinary_message,
                false,
                AggregateApiStreamInspectionMode::CommitOnContent,
            ),
            AggregateApiStreamPrefixDecision::Deliver
        );

        let plain_error = b"event: error\ndata: daily usage limit exceeded\n\n";
        assert!(matches!(
            classify_aggregate_api_stream_prefix(
                plain_error,
                false,
                AggregateApiStreamInspectionMode::CommitOnContent,
            ),
            AggregateApiStreamPrefixDecision::DailyLimit { .. }
        ));

        let billing_error =
            b"data: {\"type\":\"billing_error\",\"message\":\"daily usage limit exceeded\"}\n\n";
        assert!(matches!(
            classify_aggregate_api_stream_prefix(
                billing_error,
                false,
                AggregateApiStreamInspectionMode::CommitOnContent,
            ),
            AggregateApiStreamPrefixDecision::DailyLimit { .. }
        ));

        for explicit_code in [
            b"data: {\"type\":\"DAILY_LIMIT_EXCEEDED\"}\n\n".as_slice(),
            b"data: {\"status\":\"daily-limit-exceeded\"}\n\n".as_slice(),
            b"data: {\"reason\":\"daily limit exceeded\"}\n\n".as_slice(),
        ] {
            assert!(matches!(
                classify_aggregate_api_stream_prefix(
                    explicit_code,
                    false,
                    AggregateApiStreamInspectionMode::CommitOnContent,
                ),
                AggregateApiStreamPrefixDecision::DailyLimit { .. }
            ));
        }
    }

    #[test]
    fn complete_before_delivery_rejects_failed_stream_after_content() {
        let content = b"event: response.output_text.delta\ndata: {\"type\":\"response.output_text.delta\",\"delta\":\"hello\"}\n\n";
        for terminal in [
            GatewayStreamPrefetchTerminal::IdleTimeout,
            GatewayStreamPrefetchTerminal::Error("upstream reset".to_string()),
            GatewayStreamPrefetchTerminal::Disconnected,
        ] {
            assert!(matches!(
                classify_prefetched_aggregate_api_sse(
                    content,
                    buffered_upstream_response(content, "text/event-stream"),
                    terminal,
                    AggregateApiStreamInspectionMode::CompleteBeforeDelivery,
                ),
                AggregateApiStreamPreflightOutcome::TransportFailure(_)
            ));
        }

        assert!(matches!(
            classify_prefetched_aggregate_api_sse(
                content,
                buffered_upstream_response(content, "text/event-stream"),
                GatewayStreamPrefetchTerminal::Error("upstream reset".to_string()),
                AggregateApiStreamInspectionMode::CommitOnContent,
            ),
            AggregateApiStreamPreflightOutcome::Ready(_)
        ));
    }

    #[test]
    fn non_stream_preflight_detects_daily_limit_and_replays_normal_body() {
        let daily_limit = buffered_upstream_response(
            br#"{"error":{"code":"DAILY_LIMIT_EXCEEDED"}}"#,
            "application/json",
        );
        assert!(matches!(
            preflight_aggregate_api_non_stream(daily_limit),
            AggregateApiStreamPreflightOutcome::DailyLimit { .. }
        ));

        let other_error = buffered_upstream_response(
            br#"{"error":{"code":"rate_limit_exceeded","message":"Concurrency limit exceeded"}}"#,
            "application/json",
        );
        assert!(matches!(
            preflight_aggregate_api_non_stream(other_error),
            AggregateApiStreamPreflightOutcome::TransportFailure(_)
        ));

        let root_code_error = buffered_upstream_response(
            br#"{"code":"rate_limit_exceeded","message":"Concurrency limit exceeded"}"#,
            "application/json",
        );
        assert!(matches!(
            preflight_aggregate_api_non_stream(root_code_error),
            AggregateApiStreamPreflightOutcome::TransportFailure(_)
        ));

        let sse_daily_limit = buffered_upstream_response(
            b"event: response.created\ndata: {\"type\":\"response.created\"}\n\nevent: response.failed\ndata: {\"type\":\"response.failed\",\"response\":{\"error\":{\"code\":\"DAILY_LIMIT_EXCEEDED\"}}}\n\n",
            "text/event-stream",
        );
        assert!(matches!(
            preflight_aggregate_api_non_stream(sse_daily_limit),
            AggregateApiStreamPreflightOutcome::DailyLimit { .. }
        ));

        let success_wrappers: &[&[u8]] = &[
            br#"{"code":0,"message":"ok","data":{"id":"response-1"}}"#,
            br#"{"code":1,"message":"ok","data":{"id":"response-1"}}"#,
            br#"{"code":200,"message":"ok","data":{"id":"response-1"}}"#,
            br#"{"code":"success","message":"ok","data":{"id":"response-1"}}"#,
            br#"{"code":"NO_ERROR","message":"ok","data":{"id":"response-1"}}"#,
            br#"{"code":"NO_ERRORS","message":"ok","data":{"id":"response-1"}}"#,
            br#"{"error":false,"data":{"id":"response-1"}}"#,
            br#"{"error":{},"data":{"id":"response-1"}}"#,
        ];
        for body in success_wrappers {
            assert!(matches!(
                preflight_aggregate_api_non_stream(buffered_upstream_response(
                    body,
                    "application/json"
                )),
                AggregateApiStreamPreflightOutcome::Ready(_)
            ));
        }

        let normal_body = br#"{"id":"response-1","output_text":"ok"}"#;
        let normal = buffered_upstream_response(normal_body, "application/json");
        let AggregateApiStreamPreflightOutcome::Ready(normal) =
            preflight_aggregate_api_non_stream(normal)
        else {
            panic!("normal JSON should be delivered");
        };
        let (replayed, _) = normal.into_buffered().expect("read replayed response");
        assert_eq!(replayed.as_ref(), normal_body);
    }

    #[test]
    fn stream_preflight_inspects_json_error_responses_and_sse_prefixes() {
        let json_error = buffered_upstream_response(
            br#"{"error":{"type":"billing_error","message":"daily usage limit exceeded"}}"#,
            "application/json",
        );
        assert!(matches!(
            preflight_aggregate_api_stream(json_error),
            AggregateApiStreamPreflightOutcome::DailyLimit { .. }
        ));

        let sse_error = buffered_upstream_response(
            b"event: response.created\ndata: {\"type\":\"response.created\"}\n\nevent: response.failed\ndata: {\"type\":\"response.failed\",\"response\":{\"error\":{\"code\":\"DAILY_LIMIT_EXCEEDED\"}}}\n\n",
            "text/event-stream",
        );
        assert!(matches!(
            preflight_aggregate_api_stream(sse_error),
            AggregateApiStreamPreflightOutcome::DailyLimit { .. }
        ));

        let mislabeled_sse_error = buffered_upstream_response(
            b"event: error\ndata: daily usage limit exceeded\n\n",
            "application/json",
        );
        assert!(matches!(
            preflight_aggregate_api_stream(mislabeled_sse_error),
            AggregateApiStreamPreflightOutcome::DailyLimit { .. }
        ));
    }

    #[test]
    fn candidate_transport_rewrite_isolated_between_codex_and_generic_upstreams() {
        let _guard = crate::test_env_guard();
        let body = Bytes::from_static(
            br#"{"model":"platform-model","input":"hello","stream":false,"service_tier":"fast"}"#,
        );
        let mut codex = candidate("codex", 0);
        codex.model_override = Some("gpt-5.4".to_string());
        let mut generic = candidate("generic", 1);
        generic.model_override = Some("MiniMax-M3".to_string());
        let mut claude = candidate("claude", 2);
        claude.provider_type = AGGREGATE_API_PROVIDER_CLAUDE.to_string();
        let mut compatible = candidate("compatible", 3);
        compatible.provider_type = AGGREGATE_API_PROVIDER_COMPATIBLE.to_string();

        let codex_body = rewrite_body_for_candidate_transport(
            &body,
            &codex,
            "/v1/responses",
            "https://chatgpt.com/backend-api/codex/responses",
        );
        let generic_body = rewrite_body_for_candidate_transport(
            &body,
            &generic,
            "/v1/responses",
            "https://api.example.com/v1/responses",
        );
        let claude_body = rewrite_body_for_candidate_transport(
            &body,
            &claude,
            "/v1/responses",
            "https://proxy.example.com/backend-api/codex/responses",
        );
        let compatible_body = rewrite_body_for_candidate_transport(
            &body,
            &compatible,
            "/v1/responses",
            "https://proxy.example.com/backend-api/codex/responses",
        );
        let codex_value: Value = serde_json::from_slice(codex_body.as_ref()).expect("codex body");
        let generic_value: Value =
            serde_json::from_slice(generic_body.as_ref()).expect("generic body");
        let claude_value: Value =
            serde_json::from_slice(claude_body.as_ref()).expect("claude body");
        let compatible_value: Value =
            serde_json::from_slice(compatible_body.as_ref()).expect("compatible body");

        assert_eq!(codex_value["model"], "gpt-5.4");
        assert_eq!(
            codex_value["instructions"],
            "Follow the user's instructions."
        );
        assert_eq!(codex_value["stream"], true);
        assert_eq!(codex_value["store"], false);
        assert_eq!(codex_value["service_tier"], "priority");

        assert_eq!(generic_value["model"], "MiniMax-M3");
        assert_eq!(generic_value["input"], "hello");
        assert_eq!(generic_value["stream"], false);
        assert_eq!(generic_value["service_tier"], "fast");
        assert!(generic_value.get("instructions").is_none());
        assert!(generic_value.get("store").is_none());
        assert!(generic_value.get("tool_choice").is_none());
        assert!(generic_value.get("include").is_none());

        assert_eq!(claude_value["model"], "platform-model");
        assert_eq!(claude_value["service_tier"], "fast");
        assert!(claude_value.get("instructions").is_none());
        assert!(claude_value.get("store").is_none());

        assert_eq!(compatible_value["model"], "platform-model");
        assert_eq!(compatible_value["input"], "hello");
        assert_eq!(compatible_value["stream"], false);
        assert_eq!(compatible_value["service_tier"], "fast");
        assert!(compatible_value.get("instructions").is_none());
        assert!(compatible_value.get("store").is_none());
    }

    /// 函数 `ids`
    ///
    /// 作者: gaohongshun
    ///
    /// 时间: 2026-04-02
    ///
    /// # 参数
    /// - items: 参数 items
    ///
    /// # 返回
    /// 返回函数执行结果
    fn ids(items: &[AggregateApi]) -> Vec<String> {
        items.iter().map(|item| item.id.clone()).collect()
    }

    #[test]
    fn connection_sort_does_not_control_runtime_candidate_order() {
        let candidates =
            normalize_candidate_order(vec![candidate("agg-z", -100), candidate("agg-a", 100)]);

        assert_eq!(ids(&candidates), vec!["agg-a", "agg-z"]);
    }

    /// 函数 `balanced_route_strategy_rotates_aggregate_candidates`
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
    fn balanced_account_strategy_does_not_rotate_aggregate_candidates() {
        let _guard = crate::test_env_guard();
        let previous = std::env::var("CODEXMANAGER_ROUTE_STRATEGY").ok();
        std::env::set_var("CODEXMANAGER_ROUTE_STRATEGY", "balanced");
        crate::gateway::reload_runtime_config_from_env();

        let mut candidates = vec![
            candidate("agg-a", 0),
            candidate("agg-b", 1),
            candidate("agg-c", 2),
        ];
        apply_gateway_route_strategy_to_aggregate_candidates(
            &mut candidates,
            "gk-aggregate-route-strategy",
            Some("gpt-5.4-mini"),
            None,
        );
        assert_eq!(ids(&candidates), vec!["agg-a", "agg-b", "agg-c"]);

        let mut second = vec![
            candidate("agg-a", 0),
            candidate("agg-b", 1),
            candidate("agg-c", 2),
        ];
        apply_gateway_route_strategy_to_aggregate_candidates(
            &mut second,
            "gk-aggregate-route-strategy",
            Some("gpt-5.4-mini"),
            None,
        );
        assert_eq!(ids(&second), vec!["agg-a", "agg-b", "agg-c"]);

        if let Some(value) = previous {
            std::env::set_var("CODEXMANAGER_ROUTE_STRATEGY", value);
        } else {
            std::env::remove_var("CODEXMANAGER_ROUTE_STRATEGY");
        }
        crate::gateway::reload_runtime_config_from_env();
    }

    #[test]
    fn aggregate_route_compatibility_hooks_are_both_noops() {
        let _guard = crate::test_env_guard();
        let previous = std::env::var("CODEXMANAGER_ROUTE_STRATEGY").ok();
        std::env::set_var("CODEXMANAGER_ROUTE_STRATEGY", "balanced");
        crate::gateway::reload_runtime_config_from_env();

        let key_id = "gk-aggregate-preview-route-strategy";
        let model = Some("gpt-5.4-mini");
        let mut preview = vec![
            candidate("agg-a", 0),
            candidate("agg-b", 1),
            candidate("agg-c", 2),
        ];
        preview_gateway_route_strategy_to_aggregate_candidates(&mut preview, key_id, model, None);
        assert_eq!(ids(&preview), vec!["agg-a", "agg-b", "agg-c"]);

        let mut first_apply = vec![
            candidate("agg-a", 0),
            candidate("agg-b", 1),
            candidate("agg-c", 2),
        ];
        apply_gateway_route_strategy_to_aggregate_candidates(&mut first_apply, key_id, model, None);
        assert_eq!(ids(&first_apply), vec!["agg-a", "agg-b", "agg-c"]);

        let mut second_apply = vec![
            candidate("agg-a", 0),
            candidate("agg-b", 1),
            candidate("agg-c", 2),
        ];
        apply_gateway_route_strategy_to_aggregate_candidates(
            &mut second_apply,
            key_id,
            model,
            None,
        );
        assert_eq!(ids(&second_apply), vec!["agg-a", "agg-b", "agg-c"]);

        if let Some(value) = previous {
            std::env::set_var("CODEXMANAGER_ROUTE_STRATEGY", value);
        } else {
            std::env::remove_var("CODEXMANAGER_ROUTE_STRATEGY");
        }
        crate::gateway::reload_runtime_config_from_env();
    }

    #[test]
    fn aggregate_stream_requests_override_forwarded_accept_header() {
        let injected_headers = HashSet::new();

        assert!(should_skip_forward_header_for_aggregate_request(
            "Accept",
            &injected_headers,
            true,
        ));
        assert!(!should_skip_forward_header_for_aggregate_request(
            "Accept",
            &injected_headers,
            false,
        ));
    }

    /// 函数 `balanced_route_strategy_preserves_explicit_preferred_aggregate_api`
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
    fn balanced_route_strategy_preserves_explicit_preferred_aggregate_api() {
        let _guard = crate::test_env_guard();
        let previous = std::env::var("CODEXMANAGER_ROUTE_STRATEGY").ok();
        std::env::set_var("CODEXMANAGER_ROUTE_STRATEGY", "balanced");
        crate::gateway::reload_runtime_config_from_env();

        let mut candidates = vec![
            candidate("agg-preferred", 0),
            candidate("agg-b", 1),
            candidate("agg-c", 2),
        ];
        apply_gateway_route_strategy_to_aggregate_candidates(
            &mut candidates,
            "gk-aggregate-route-strategy-preferred",
            Some("gpt-5.4-mini"),
            Some("agg-preferred"),
        );
        assert_eq!(ids(&candidates), vec!["agg-preferred", "agg-b", "agg-c"]);

        let mut second = vec![
            candidate("agg-preferred", 0),
            candidate("agg-b", 1),
            candidate("agg-c", 2),
        ];
        apply_gateway_route_strategy_to_aggregate_candidates(
            &mut second,
            "gk-aggregate-route-strategy-preferred",
            Some("gpt-5.4-mini"),
            Some("agg-preferred"),
        );
        assert_eq!(ids(&second), vec!["agg-preferred", "agg-b", "agg-c"]);

        if let Some(value) = previous {
            std::env::set_var("CODEXMANAGER_ROUTE_STRATEGY", value);
        } else {
            std::env::remove_var("CODEXMANAGER_ROUTE_STRATEGY");
        }
        crate::gateway::reload_runtime_config_from_env();
    }
}

#[cfg(test)]
#[path = "aggregate_api_tests.rs"]
mod tests;
