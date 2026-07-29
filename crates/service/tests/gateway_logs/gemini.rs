use super::*;
use codexmanager_core::storage::AggregateApi;

#[test]
fn gateway_aggregate_gemini_native_stream_survives_auto_toggle_preflight_and_resets_failures() {
    let _lock = test_env_guard();
    let dir = new_test_dir("codexmanager-gateway-aggregate-gemini-native-stream-auto-toggle");
    let db_path: PathBuf = dir.join("codexmanager.db");
    let _db_guard = EnvGuard::set("CODEXMANAGER_DB_PATH", db_path.to_string_lossy().as_ref());

    let gemini_sse = concat!(
        "data: {\"candidates\":[{\"content\":{\"role\":\"model\",\"parts\":[{\"text\":\"hello from gemini\"}]},\"finishReason\":\"STOP\",\"index\":0}],",
        "\"usageMetadata\":{\"promptTokenCount\":2,\"candidatesTokenCount\":3,\"totalTokenCount\":5},",
        "\"modelVersion\":\"gemini-2.5-flash\",\"responseId\":\"gemini-aggregate-stream\"}\n\n"
    );
    let (upstream_addr, upstream_rx, upstream_join) =
        start_mock_upstream_sequence_lenient_with_content_types(
            vec![(200, gemini_sse.to_string(), "text/event-stream".to_string())],
            Duration::from_secs(3),
        );

    let storage = Storage::open(&db_path).expect("open db");
    storage.init().expect("init db");
    let now = now_ts();
    let aggregate_id = "agg_gemini_native_stream_auto_toggle";
    storage
        .insert_aggregate_api(&AggregateApi {
            id: aggregate_id.to_string(),
            provider_type: "gemini".to_string(),
            supplier_name: Some("gemini-native-stream".to_string()),
            sort: 0,
            url: format!("http://{upstream_addr}"),
            auth_type: "apikey".to_string(),
            auth_params_json: None,
            action: None,
            model_override: None,
            status: "active".to_string(),
            auto_toggle_enabled: true,
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
        .expect("insert aggregate API");
    storage
        .upsert_aggregate_api_secret(aggregate_id, "upstream-secret")
        .expect("insert aggregate secret");
    for _ in 0..2 {
        storage
            .record_aggregate_api_daily_quota_failure(aggregate_id)
            .expect("seed prior daily quota failure");
    }
    seed_model_catalog_route(
        &storage,
        "gemini-2.5-flash",
        "aggregate_api",
        aggregate_id,
        "gemini-2.5-flash",
        10,
    );

    let platform_key = "pk_gemini_native_stream_auto_toggle";
    storage
        .insert_api_key(&ApiKey {
            id: "gk_gemini_native_stream_auto_toggle".to_string(),
            name: Some("gemini-native-stream-auto-toggle".to_string()),
            model_slug: Some("gemini-2.5-flash".to_string()),
            reasoning_effort: None,
            service_tier: None,
            rotation_strategy: "aggregate_api_rotation".to_string(),
            aggregate_api_id: Some(aggregate_id.to_string()),
            account_plan_filter: None,
            aggregate_api_url: None,
            client_type: "codex".to_string(),
            protocol_type: "openai_compat".to_string(),
            auth_scheme: "authorization_bearer".to_string(),
            upstream_base_url: None,
            static_headers_json: None,
            key_hash: hash_platform_key_for_test(platform_key),
            status: "active".to_string(),
            created_at: now,
            last_used_at: None,
        })
        .expect("insert API key");

    let path = "/v1beta/models/gemini-2.5-flash:streamGenerateContent?alt=sse";
    let request_body = serde_json::json!({
        "contents": [{
            "role": "user",
            "parts": [{ "text": "hello" }]
        }]
    });
    let request_body = serde_json::to_string(&request_body).expect("serialize request");
    let server = codexmanager_service::start_one_shot_server().expect("start server");
    let (status, response_body) = post_http_raw(
        &server.addr,
        path,
        &request_body,
        &[
            ("Content-Type", "application/json"),
            ("Authorization", &format!("Bearer {platform_key}")),
        ],
    );
    server.join();

    assert_eq!(status, 200, "gateway response: {response_body}");
    assert!(
        response_body.contains("\"text\":\"hello from gemini\""),
        "gateway response: {response_body}"
    );
    assert!(response_body.contains("\"finishReason\":\"STOP\""));

    let captured = upstream_rx
        .recv_timeout(Duration::from_secs(3))
        .expect("receive upstream request");
    upstream_join.join().expect("join mock upstream");
    assert_eq!(captured.path, path);
    assert_eq!(
        captured.headers.get("authorization").map(String::as_str),
        Some("Bearer upstream-secret")
    );
    let upstream_payload: serde_json::Value =
        serde_json::from_slice(&captured.body).expect("parse Gemini request body");
    assert_eq!(upstream_payload["contents"][0]["role"], "user");
    assert_eq!(upstream_payload["contents"][0]["parts"][0]["text"], "hello");

    let aggregate = storage
        .find_aggregate_api_by_id(aggregate_id)
        .expect("read aggregate API")
        .expect("aggregate API exists");
    assert_eq!(aggregate.consecutive_failures, 0);
    assert!(!aggregate.auto_disabled);
}
