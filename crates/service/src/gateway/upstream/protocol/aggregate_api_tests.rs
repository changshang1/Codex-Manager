use codexmanager_core::storage::{now_ts, AggregateApi, Storage};

use super::{
    apply_compatibility_static_headers, apply_previous_response_affinity,
    build_anthropic_bridge_aggregate_api_request, build_upstream_url, effective_action_path,
    merge_compatibility_config_json, resolve_aggregate_api_rotation_candidates,
    resolve_passthrough_sse_protocol, responses_to_anthropic_messages_action_path,
    rewrite_body_for_compatibility, rewrite_body_model_override,
    should_bridge_responses_to_anthropic,
};
use crate::aggregate_api::{
    AGGREGATE_API_AUTH_APIKEY, AGGREGATE_API_PROVIDER_CLAUDE, AGGREGATE_API_PROVIDER_CODEX,
    AGGREGATE_API_PROVIDER_COMPATIBLE, AGGREGATE_API_PROVIDER_GEMINI,
    AGGREGATE_API_PROVIDER_RESPONSES,
};
use crate::gateway::{PassthroughSseProtocol, ResponseAdapter};
use bytes::Bytes;

fn aggregate_api_with_action(action: Option<&str>) -> AggregateApi {
    AggregateApi {
        id: "agg-path-test".to_string(),
        provider_type: "claude".to_string(),
        supplier_name: Some("test".to_string()),
        sort: 0,
        url: "https://open.bigmodel.cn/api/anthropic".to_string(),
        auth_type: "apikey".to_string(),
        auth_params_json: None,
        action: action.map(str::to_string),
        model_override: None,
        compatibility_config_json: None,
        status: "active".to_string(),
        auto_toggle_enabled: false,
        consecutive_failures: 0,
        auto_disabled: false,
        auto_disabled_at: None,
        auto_disabled_reason: None,
        created_at: 0,
        updated_at: 0,
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
fn empty_custom_action_uses_base_url_without_original_path() {
    let api = aggregate_api_with_action(Some(""));
    let path = effective_action_path(&api, "/v1/messages?beta=true");
    assert_eq!(path, "");
}

#[test]
fn messages_passthrough_uses_anthropic_native_terminal_rules_without_provider_gate() {
    let protocol =
        resolve_passthrough_sse_protocol("/v1/messages?beta=true", ResponseAdapter::Passthrough);
    assert_eq!(protocol, Some(PassthroughSseProtocol::AnthropicNative));
}

#[test]
fn messages_passthrough_protocol_still_requires_passthrough_adapter() {
    let protocol = resolve_passthrough_sse_protocol(
        "/v1/messages?beta=true",
        ResponseAdapter::AnthropicMessagesFromResponses,
    );
    assert_eq!(protocol, None);
}

#[test]
fn deepseek_compatibility_preserves_supported_responses_fields() {
    let mut api = aggregate_api_with_action(None);
    api.provider_type = "responses".to_string();
    api.compatibility_config_json = Some(r#"{"profile":"deepseek"}"#.to_string());
    let body = Bytes::from_static(
        br#"{"model":"deepseek-v4-flash","input":"hi","reasoning":{"effort":"high"},"include":["reasoning.encrypted_content"],"background":true,"service_tier":"flex"}"#,
    );
    let rewritten = rewrite_body_for_compatibility(&body, &api).expect("rewrite body");
    let value: serde_json::Value = serde_json::from_slice(&rewritten).expect("json body");
    assert_eq!(value["reasoning"]["effort"], "high");
    assert_eq!(value["background"], true);
    assert_eq!(value["include"][0], "reasoning.encrypted_content");
    assert_eq!(value["service_tier"], "flex");
}

#[test]
fn deepseek_compatibility_rejects_required_tool_choice() {
    let mut api = aggregate_api_with_action(None);
    api.provider_type = "responses".to_string();
    api.compatibility_config_json = Some(r#"{"profile":"deepseek"}"#.to_string());
    let body = Bytes::from_static(
        br#"{"model":"deepseek-v4-flash","input":"hi","tool_choice":"required"}"#,
    );
    let error = rewrite_body_for_compatibility(&body, &api).expect_err("reject tool choice");
    assert!(error.contains("tool_choice=required"));
}

#[test]
fn compatibility_field_policy_can_reject_or_replace_fields() {
    let mut api = aggregate_api_with_action(None);
    api.provider_type = "responses".to_string();
    api.compatibility_config_json = Some(
        r#"{"fieldPolicies":{"store":"reject","service_tier":{"strategy":"replace","value":"default"}}}"#
            .to_string(),
    );
    let body = Bytes::from_static(br#"{"model":"x","input":"hi","store":true}"#);
    let error = rewrite_body_for_compatibility(&body, &api).expect_err("reject unsupported field");
    assert!(error.contains("store"));
}

#[test]
fn compatibility_field_policy_supports_nested_paths_and_value_maps() {
    let mut api = aggregate_api_with_action(None);
    api.provider_type = "responses".to_string();
    api.compatibility_config_json = Some(
        r#"{"fieldPolicies":{"reasoning.effort":{"strategy":"map","value":{"max":"high"}},"metadata.private":"drop","text.verbosity":{"strategy":"replace","value":"low"}}}"#
            .to_string(),
    );
    let body = Bytes::from_static(
        br#"{"model":"x","reasoning":{"effort":"max"},"metadata":{"private":true,"keep":1}}"#,
    );
    let rewritten = rewrite_body_for_compatibility(&body, &api).expect("rewrite nested fields");
    let value: serde_json::Value = serde_json::from_slice(&rewritten).expect("json body");
    assert_eq!(value["reasoning"]["effort"], "high");
    assert!(value["metadata"].get("private").is_none());
    assert_eq!(value["metadata"]["keep"], 1);
    assert_eq!(value["text"]["verbosity"], "low");
}

#[test]
fn route_compatibility_override_deep_merges_aggregate_config() {
    let merged = merge_compatibility_config_json(
        Some(r#"{"profile":"deepseek","staticHeaders":{"x-base":"1"},"fieldPolicies":{"store":"drop"}}"#),
        Some(r#"{"staticHeaders":{"x-route":"2"},"fieldPolicies":{"store":"pass","background":"drop"}}"#),
    )
    .expect("merge compatibility config")
    .expect("merged json");
    let value: serde_json::Value = serde_json::from_str(&merged).expect("json");
    assert_eq!(value["profile"], "deepseek");
    assert_eq!(value["staticHeaders"]["x-base"], "1");
    assert_eq!(value["staticHeaders"]["x-route"], "2");
    assert_eq!(value["fieldPolicies"]["store"], "pass");
    assert_eq!(value["fieldPolicies"]["background"], "drop");
}

#[test]
fn compatibility_static_headers_render_allowed_placeholders() {
    let mut api = aggregate_api_with_action(None);
    api.id = "agg-header".to_string();
    api.supplier_name = Some("DeepSeek".to_string());
    api.compatibility_config_json = Some(
        r#"{"staticHeaders":{"x-provider-context":"${supplier}:${model}:${secret}"}}"#.to_string(),
    );
    let request = apply_compatibility_static_headers(
        reqwest::blocking::Client::new().post("https://example.com/v1/responses"),
        &api,
        "secret-value",
        Some("deepseek-v4-flash"),
    )
    .expect("apply static headers")
    .build()
    .expect("build request");
    assert_eq!(
        request
            .headers()
            .get("x-provider-context")
            .and_then(|value| value.to_str().ok()),
        Some("DeepSeek:deepseek-v4-flash:secret-value")
    );
}

#[test]
fn previous_response_affinity_selects_bound_supplier_and_rejects_unknown_multi_supplier() {
    let storage = Storage::open_in_memory().expect("open storage");
    storage.init().expect("init storage");
    let mut first = aggregate_api_with_action(None);
    first.id = "agg-affinity-first".to_string();
    let mut second = aggregate_api_with_action(None);
    second.id = "agg-affinity-second".to_string();
    storage
        .insert_aggregate_api(&first)
        .expect("insert first aggregate api");
    storage
        .insert_aggregate_api(&second)
        .expect("insert second aggregate api");
    storage
        .upsert_aggregate_api_response_affinity("resp-bound", second.id.as_str())
        .expect("save response affinity");

    let mut candidates = vec![first.clone(), second.clone()];
    apply_previous_response_affinity(
        &storage,
        &Bytes::from_static(br#"{"previous_response_id":"resp-bound"}"#),
        &mut candidates,
        false,
    )
    .expect("apply bound affinity");
    assert_eq!(candidates.len(), 1);
    assert_eq!(candidates[0].id, second.id);

    let mut unknown_candidates = vec![first, second];
    let error = apply_previous_response_affinity(
        &storage,
        &Bytes::from_static(br#"{"previous_response_id":"resp-unknown"}"#),
        &mut unknown_candidates,
        false,
    )
    .expect_err("unknown response id must not cross suppliers");
    assert!(error.contains("禁止跨供应商"));
}

#[test]
fn gemini_stream_passthrough_uses_native_terminal_rules() {
    let protocol = resolve_passthrough_sse_protocol(
        "/v1beta/models/gemini-2.5-pro:streamGenerateContent?alt=sse",
        ResponseAdapter::Passthrough,
    );
    assert_eq!(protocol, Some(PassthroughSseProtocol::GeminiNative));
}

#[test]
fn build_upstream_url_preserves_base_path_prefix() {
    let url = build_upstream_url(
        "https://open.bigmodel.cn/api/anthropic",
        "/v1/messages?beta=true",
    )
    .expect("build upstream url");
    assert_eq!(
        url.as_str(),
        "https://open.bigmodel.cn/api/anthropic/v1/messages?beta=true"
    );
}

#[test]
fn build_upstream_url_keeps_root_base_behavior() {
    let url = build_upstream_url("https://api.example.com", "/v1/messages?beta=true")
        .expect("build upstream url");
    assert_eq!(
        url.as_str(),
        "https://api.example.com/v1/messages?beta=true"
    );
}

#[test]
fn build_upstream_url_deduplicates_v1_base_path() {
    let url = build_upstream_url("https://api.minimax.io/v1", "/v1/responses")
        .expect("build upstream url");

    assert_eq!(url.as_str(), "https://api.minimax.io/v1/responses");
}

#[test]
fn responses_bridge_uses_messages_suffix_for_anthropic_v1_base_url() {
    let mut api = aggregate_api_with_action(None);
    api.url = "https://api.anthropic.com/v1".to_string();

    let path = responses_to_anthropic_messages_action_path(&api, "/v1/responses");
    let url = build_upstream_url(api.url.as_str(), path.as_str()).expect("build upstream url");

    assert_eq!(url.as_str(), "https://api.anthropic.com/v1/messages");
}

#[test]
fn responses_bridge_keeps_v1_messages_for_deepseek_anthropic_base_url() {
    let mut api = aggregate_api_with_action(None);
    api.url = "https://api.deepseek.com/anthropic".to_string();

    let path = responses_to_anthropic_messages_action_path(&api, "/v1/responses");
    let url = build_upstream_url(api.url.as_str(), path.as_str()).expect("build upstream url");

    assert_eq!(
        url.as_str(),
        "https://api.deepseek.com/anthropic/v1/messages"
    );
}

#[test]
fn responses_bridge_respects_custom_action_path() {
    let mut api = aggregate_api_with_action(Some("/messages?beta=true"));
    api.url = "https://api.anthropic.com/v1".to_string();

    let path = responses_to_anthropic_messages_action_path(&api, "/v1/responses");
    let url = build_upstream_url(api.url.as_str(), path.as_str()).expect("build upstream url");

    assert_eq!(
        path.as_str(),
        "/messages?beta=true",
        "custom action should remain the upstream bridge action"
    );
    assert_eq!(
        url.as_str(),
        "https://api.anthropic.com/v1/messages?beta=true"
    );
}

#[test]
fn anthropic_bridge_request_adds_required_messages_headers_with_default_auth() {
    let request: tiny_http::Request = tiny_http::TestRequest::new()
        .with_header(
            tiny_http::Header::from_bytes("Authorization", "Bearer client-key")
                .expect("auth header"),
        )
        .into();
    let client = reqwest::blocking::Client::new();
    let builder = build_anthropic_bridge_aggregate_api_request(
        &client,
        &request,
        &reqwest::Method::POST,
        reqwest::Url::parse("https://api.anthropic.com/v1/messages").expect("url"),
        &Bytes::from_static(br#"{"model":"claude-sonnet","messages":[]}"#),
        "sk-ant-test",
        &crate::gateway::upstream::protocol::aggregate_api::AggregateApiAuthConfig::ApiKeyDefaultBearer,
        &std::collections::HashSet::new(),
        None,
        true,
    )
    .expect("build request")
    .build()
    .expect("finalize request");

    assert_eq!(
        builder
            .headers()
            .get("authorization")
            .and_then(|value| value.to_str().ok()),
        Some("Bearer sk-ant-test")
    );
    assert_eq!(
        builder
            .headers()
            .get("x-api-key")
            .and_then(|value| value.to_str().ok()),
        Some("sk-ant-test")
    );
    assert_eq!(
        builder
            .headers()
            .get("anthropic-version")
            .and_then(|value| value.to_str().ok()),
        Some("2023-06-01")
    );
    assert_eq!(
        builder
            .headers()
            .get("accept")
            .and_then(|value| value.to_str().ok()),
        Some("text/event-stream")
    );
}

#[test]
fn rewrite_body_model_override_replaces_json_model() {
    let body = Bytes::from_static(br#"{"model":"claude-sonnet","messages":[]}"#);

    let rewritten = rewrite_body_model_override(&body, Some("qwen3.5-plus"));

    let value: serde_json::Value =
        serde_json::from_slice(rewritten.as_ref()).expect("parse rewritten body");
    assert_eq!(value["model"], "qwen3.5-plus");
    assert_eq!(value["messages"].as_array().map(Vec::len), Some(0));
}

#[test]
fn gemini_native_candidates_resolve_to_gemini_provider_only() {
    let storage = Storage::open_in_memory().expect("open storage");
    storage.init().expect("init storage");
    let now = now_ts();
    for (id, provider_type) in [
        ("agg-codex", AGGREGATE_API_PROVIDER_CODEX),
        ("agg-claude", AGGREGATE_API_PROVIDER_CLAUDE),
        ("agg-gemini", AGGREGATE_API_PROVIDER_GEMINI),
    ] {
        storage
            .insert_aggregate_api(&AggregateApi {
                id: id.to_string(),
                provider_type: provider_type.to_string(),
                supplier_name: Some(id.to_string()),
                sort: 0,
                url: format!("https://{id}.example.com"),
                auth_type: AGGREGATE_API_AUTH_APIKEY.to_string(),
                auth_params_json: None,
                action: None,
                model_override: None,
                compatibility_config_json: None,
                status: "active".to_string(),
                auto_toggle_enabled: false,
                consecutive_failures: 0,
                auto_disabled: false,
                auto_disabled_at: None,
                auto_disabled_reason: None,
                created_at: now,
                updated_at: now,
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
            })
            .expect("insert aggregate api");
    }

    let candidates = resolve_aggregate_api_rotation_candidates(
        &storage,
        "gemini_native",
        "/v1beta/models/gemini:generateContent",
        None,
    )
    .expect("resolve gemini candidates");
    let candidate_ids = candidates
        .iter()
        .map(|item| item.id.as_str())
        .collect::<Vec<_>>();
    assert_eq!(candidate_ids, vec!["agg-gemini"]);
}

#[test]
fn compatible_candidate_resolves_for_codex_and_anthropic_without_protocol_bridge() {
    let storage = Storage::open_in_memory().expect("open storage");
    storage.init().expect("init storage");
    let now = now_ts();
    let mut compatible = aggregate_api_with_action(None);
    compatible.id = "agg-compatible".to_string();
    compatible.provider_type = AGGREGATE_API_PROVIDER_COMPATIBLE.to_string();
    compatible.url = "https://multi-protocol.example.com".to_string();
    compatible.created_at = now;
    compatible.updated_at = now;
    storage
        .insert_aggregate_api(&compatible)
        .expect("insert compatible aggregate api");

    for protocol_type in ["openai", "anthropic_native"] {
        let candidates = resolve_aggregate_api_rotation_candidates(
            &storage,
            protocol_type,
            "/v1/responses",
            None,
        )
        .expect("resolve compatible candidate");
        assert_eq!(
            candidates
                .iter()
                .map(|candidate| candidate.id.as_str())
                .collect::<Vec<_>>(),
            vec!["agg-compatible"]
        );
    }
    assert!(resolve_aggregate_api_rotation_candidates(
        &storage,
        "gemini_native",
        "/v1beta/models/gemini:generateContent",
        None,
    )
    .is_err());
    assert!(!should_bridge_responses_to_anthropic(
        &compatible,
        "/v1/responses"
    ));
}

#[test]
fn responses_provider_only_resolves_for_client_responses_path() {
    let storage = Storage::open_in_memory().expect("open storage");
    storage.init().expect("init storage");
    let now = now_ts();
    let mut responses = aggregate_api_with_action(None);
    responses.id = "agg-responses-only".to_string();
    responses.provider_type = AGGREGATE_API_PROVIDER_RESPONSES.to_string();
    responses.url = "https://api.deepseek.com".to_string();
    responses.created_at = now;
    responses.updated_at = now;
    storage
        .insert_aggregate_api(&responses)
        .expect("insert responses aggregate api");

    let responses_candidates =
        resolve_aggregate_api_rotation_candidates(&storage, "openai", "/v1/responses", None)
            .expect("resolve responses candidates");
    assert!(responses_candidates
        .iter()
        .any(|candidate| candidate.id == "agg-responses-only"));

    assert!(resolve_aggregate_api_rotation_candidates(
        &storage,
        "openai",
        "/v1/chat/completions",
        None,
    )
    .is_err());
}

#[test]
fn explicit_aggregate_api_id_promotes_matching_active_provider_candidate_only() {
    let storage = Storage::open_in_memory().expect("open storage");
    storage.init().expect("init storage");
    let now = now_ts();
    for (id, provider_type, sort) in [
        ("agg-first", AGGREGATE_API_PROVIDER_CODEX, 0),
        ("agg-preferred", AGGREGATE_API_PROVIDER_CODEX, 10),
        ("agg-claude", AGGREGATE_API_PROVIDER_CLAUDE, -1),
    ] {
        storage
            .insert_aggregate_api(&AggregateApi {
                id: id.to_string(),
                provider_type: provider_type.to_string(),
                supplier_name: Some(id.to_string()),
                sort,
                url: format!("https://{id}.example.com"),
                auth_type: AGGREGATE_API_AUTH_APIKEY.to_string(),
                auth_params_json: None,
                action: None,
                model_override: None,
                compatibility_config_json: None,
                status: "active".to_string(),
                auto_toggle_enabled: false,
                consecutive_failures: 0,
                auto_disabled: false,
                auto_disabled_at: None,
                auto_disabled_reason: None,
                created_at: now,
                updated_at: now,
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
            })
            .expect("insert aggregate api");
    }

    let candidates = resolve_aggregate_api_rotation_candidates(
        &storage,
        "openai",
        "/v1/chat/completions",
        Some("agg-preferred"),
    )
    .expect("resolve codex candidates");
    let candidate_ids = candidates
        .iter()
        .map(|item| item.id.as_str())
        .collect::<Vec<_>>();
    assert_eq!(candidate_ids, vec!["agg-preferred", "agg-first"]);

    let candidates = resolve_aggregate_api_rotation_candidates(
        &storage,
        "openai",
        "/v1/chat/completions",
        Some("agg-claude"),
    )
    .expect("resolve codex candidates with mismatched preferred");
    let candidate_ids = candidates
        .iter()
        .map(|item| item.id.as_str())
        .collect::<Vec<_>>();
    assert_eq!(candidate_ids, vec!["agg-first", "agg-preferred"]);
}

/// 函数 `final_error_promotes_success_status_to_bad_gateway`
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
fn final_error_promotes_success_status_to_bad_gateway() {
    let status_code = bridge_status_code(Some(200), true, Some("unsupported model"));
    assert_eq!(status_code, 502);
}

/// 函数 `successful_bridge_keeps_success_status`
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
fn successful_bridge_keeps_success_status() {
    let status_code = bridge_status_code(Some(200), true, None);
    assert_eq!(status_code, 200);
}

/// 函数 `incomplete_bridge_without_status_defaults_to_bad_gateway`
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
fn incomplete_bridge_without_status_defaults_to_bad_gateway() {
    let status_code = bridge_status_code(None, false, None);
    assert_eq!(status_code, 502);
}

/// 函数 `bridge_status_code`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - delivered_status_code: 参数 delivered_status_code
/// - bridge_ok: 参数 bridge_ok
/// - final_error: 参数 final_error
///
/// # 返回
/// 返回函数执行结果
fn bridge_status_code(
    delivered_status_code: Option<u16>,
    bridge_ok: bool,
    final_error: Option<&str>,
) -> u16 {
    let status_code = delivered_status_code.unwrap_or_else(|| if bridge_ok { 200 } else { 502 });
    if final_error.is_some() && status_code < 400 {
        502
    } else {
        status_code
    }
}
