use super::*;
use codexmanager_core::storage::AggregateApi;

const MODEL: &str = "gpt-hybrid-route-test";
const UPSTREAM_MODEL: &str = "gpt-hybrid-route-upstream";

fn response_json(id: &str) -> String {
    serde_json::to_string(&serde_json::json!({
        "id": id,
        "model": MODEL,
        "output": [{
            "type": "message",
            "role": "assistant",
            "content": [{ "type": "output_text", "text": "ok" }]
        }],
        "usage": { "input_tokens": 2, "output_tokens": 1, "total_tokens": 3 }
    }))
    .expect("serialize upstream response")
}

fn insert_active_account(storage: &Storage, account_id: &str, now: i64) {
    storage
        .insert_account(&Account {
            id: account_id.to_string(),
            label: account_id.to_string(),
            issuer: "https://auth.openai.com".to_string(),
            chatgpt_account_id: Some(format!("chatgpt_{account_id}")),
            workspace_id: None,
            group_name: None,
            sort: 0,
            status: "active".to_string(),
            created_at: now,
            updated_at: now,
        })
        .expect("insert account");
    storage
        .insert_token(&Token {
            account_id: account_id.to_string(),
            id_token: String::new(),
            access_token: format!("access_{account_id}"),
            refresh_token: String::new(),
            api_key_access_token: Some(format!("api_access_{account_id}")),
            last_refresh: now,
        })
        .expect("insert token");
}

fn insert_aggregate_api(storage: &Storage, aggregate_id: &str, addr: &str, action: &str, now: i64) {
    storage
        .insert_aggregate_api(&AggregateApi {
            id: aggregate_id.to_string(),
            provider_type: "codex".to_string(),
            supplier_name: Some("hybrid route test".to_string()),
            sort: 0,
            url: format!("http://{addr}/backend-api/codex"),
            auth_type: "apikey".to_string(),
            auth_params_json: None,
            action: Some(action.to_string()),
            model_override: None,
            compatibility_config_json: None,
            upstream_wire: None,
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
        .expect("insert aggregate API");
    storage
        .upsert_aggregate_api_secret(aggregate_id, "aggregate-secret")
        .expect("insert aggregate API secret");
}

fn replace_with_aggregate_only_route(storage: &Storage, aggregate_id: &str) {
    seed_model_catalog_models(storage, &[MODEL]);
    let mut model = storage
        .get_managed_model_v2(MODEL)
        .expect("get V2 model")
        .expect("V2 model exists");
    model.routes = vec![ModelRouteV2 {
        id: String::new(),
        source_kind: "aggregate_api".to_string(),
        source_id: aggregate_id.to_string(),
        upstream_model: UPSTREAM_MODEL.to_string(),
        enabled: true,
        priority: 0,
        weight: 1,
        sort_order: 0,
        compatibility_override_json: None,
    }];
    storage
        .upsert_managed_model_v2(&ManagedModelV2Upsert {
            previous_slug: None,
            model,
        })
        .expect("replace V2 model routes");
}

fn seed_dual_routes(storage: &Storage, aggregate_id: &str) {
    seed_model_catalog_models(storage, &[MODEL]);
    seed_model_catalog_route(
        storage,
        MODEL,
        "aggregate_api",
        aggregate_id,
        UPSTREAM_MODEL,
        0,
    );
}

fn insert_hybrid_key(storage: &Storage, key_id: &str, platform_key: &str, now: i64) {
    insert_hybrid_key_with_rotation(storage, key_id, platform_key, "hybrid_rotation", now);
}

fn insert_hybrid_key_with_rotation(
    storage: &Storage,
    key_id: &str,
    platform_key: &str,
    rotation_strategy: &str,
    now: i64,
) {
    storage
        .insert_api_key(&ApiKey {
            id: key_id.to_string(),
            name: Some(key_id.to_string()),
            model_slug: Some(MODEL.to_string()),
            reasoning_effort: None,
            service_tier: None,
            rotation_strategy: rotation_strategy.to_string(),
            aggregate_api_id: None,
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
        .expect("insert hybrid API key");
}

#[test]
fn hybrid_aggregate_only_skips_active_account_and_uses_aggregate_api() {
    let _lock = test_env_guard();
    let dir = new_test_dir("codexmanager-hybrid-aggregate-only");
    let db_path: PathBuf = dir.join("codexmanager.db");
    let _db_guard = EnvGuard::set("CODEXMANAGER_DB_PATH", db_path.to_string_lossy().as_ref());

    let (local_addr, local_rx, local_join) = start_mock_upstream_sequence_lenient(
        vec![(200, response_json("resp_local_should_not_run"))],
        Duration::from_secs(2),
    );
    let local_base = format!("http://{local_addr}/backend-api/codex");
    let _upstream_guard = EnvGuard::set("CODEXMANAGER_UPSTREAM_BASE_URL", &local_base);
    let (aggregate_addr, aggregate_rx, aggregate_join) = start_mock_upstream_sequence_lenient(
        vec![(200, response_json("resp_aggregate_only"))],
        Duration::from_secs(2),
    );

    let storage = Storage::open(&db_path).expect("open db");
    storage.init().expect("init db");
    let now = now_ts();
    let aggregate_id = "agg_hybrid_aggregate_only";
    let key_id = "gk_hybrid_aggregate_only";
    let platform_key = "pk_hybrid_aggregate_only";
    insert_active_account(&storage, "acc_hybrid_aggregate_only", now);
    insert_aggregate_api(&storage, aggregate_id, &aggregate_addr, "/responses", now);
    replace_with_aggregate_only_route(&storage, aggregate_id);
    insert_hybrid_key(&storage, key_id, platform_key, now);

    let server = codexmanager_service::start_one_shot_server().expect("start server");
    let request = serde_json::json!({
        "model": MODEL,
        "input": "hello",
        "stream": false
    });
    let request = serde_json::to_string(&request).expect("serialize request");
    let (status, response_body) = post_http_raw(
        &server.addr,
        "/v1/responses",
        &request,
        &[
            ("Content-Type", "application/json"),
            ("Authorization", &format!("Bearer {platform_key}")),
        ],
    );
    server.join();
    local_join.join().expect("join local upstream");
    aggregate_join.join().expect("join aggregate upstream");

    assert_eq!(status, 200, "gateway response: {response_body}");
    assert!(response_body.contains("resp_aggregate_only"));
    assert_eq!(
        local_rx.try_iter().count(),
        0,
        "local account must be skipped"
    );
    let aggregate_requests = aggregate_rx.try_iter().collect::<Vec<_>>();
    assert_eq!(aggregate_requests.len(), 1, "aggregate API request count");
    assert_eq!(aggregate_requests[0].path, "/backend-api/codex/responses");
    let aggregate_body: serde_json::Value =
        serde_json::from_slice(&decode_upstream_request_body(&aggregate_requests[0]))
            .expect("parse aggregate request body");
    assert_eq!(aggregate_body["model"], UPSTREAM_MODEL);

    let log = storage
        .list_request_logs(Some(&format!("key:={key_id}")), 10)
        .expect("list request logs")
        .into_iter()
        .find(|item| item.request_path == "/v1/responses")
        .expect("request log");
    assert_eq!(log.status_code, Some(200));
    assert_eq!(log.actual_source_kind.as_deref(), Some("aggregate_api"));
    assert_eq!(log.actual_source_id.as_deref(), Some(aggregate_id));
}

#[test]
fn hybrid_aggregate_first_streams_chat_tool_calls_and_clears_prior_failures() {
    let _lock = test_env_guard();
    let dir = new_test_dir("codexmanager-hybrid-aggregate-only-chat-tools");
    let db_path: PathBuf = dir.join("codexmanager.db");
    let _db_guard = EnvGuard::set("CODEXMANAGER_DB_PATH", db_path.to_string_lossy().as_ref());

    let (local_addr, local_rx, local_join) = start_mock_upstream_sequence_lenient(
        vec![(200, response_json("resp_local_should_not_run"))],
        Duration::from_secs(2),
    );
    let local_base = format!("http://{local_addr}/backend-api/codex");
    let _upstream_guard = EnvGuard::set("CODEXMANAGER_UPSTREAM_BASE_URL", &local_base);
    let tool_call_sse = concat!(
        "data: {\"id\":\"chatcmpl_hybrid_tool\",\"object\":\"chat.completion.chunk\",\"created\":1775900000,\"model\":\"gpt-hybrid-route-test\",\"choices\":[{\"index\":0,\"delta\":{\"role\":\"assistant\",\"tool_calls\":[{\"index\":0,\"id\":\"call_hybrid_1\",\"type\":\"function\",\"function\":{\"name\":\"get_answer\",\"arguments\":\"{\\\"question\\\":\\\"2+2\\\"}\"}}]},\"finish_reason\":null}]}\n\n",
        "data: {\"id\":\"chatcmpl_hybrid_tool\",\"object\":\"chat.completion.chunk\",\"created\":1775900000,\"model\":\"gpt-hybrid-route-test\",\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"tool_calls\"}]}\n\n",
        "data: [DONE]\n\n"
    );
    let (aggregate_addr, aggregate_rx, aggregate_join) =
        start_mock_upstream_sequence_lenient_with_content_types(
            vec![(
                200,
                tool_call_sse.to_string(),
                "text/event-stream".to_string(),
            )],
            Duration::from_secs(2),
        );

    let storage = Storage::open(&db_path).expect("open db");
    storage.init().expect("init db");
    let now = now_ts();
    let aggregate_id = "agg_hybrid_aggregate_only_chat_tools";
    let key_id = "gk_hybrid_aggregate_only_chat_tools";
    let platform_key = "pk_hybrid_aggregate_only_chat_tools";
    insert_active_account(&storage, "acc_hybrid_aggregate_only_chat_tools", now);
    insert_aggregate_api(
        &storage,
        aggregate_id,
        &aggregate_addr,
        "/chat/completions",
        now,
    );
    storage
        .set_aggregate_api_auto_toggle_enabled(aggregate_id, true)
        .expect("enable aggregate API auto toggle");
    for _ in 0..2 {
        storage
            .record_aggregate_api_daily_quota_failure(aggregate_id)
            .expect("seed prior daily quota failure");
    }
    seed_dual_routes(&storage, aggregate_id);
    insert_hybrid_key_with_rotation(
        &storage,
        key_id,
        platform_key,
        "hybrid_aggregate_first_rotation",
        now,
    );

    let server = codexmanager_service::start_one_shot_server().expect("start server");
    let request = serde_json::json!({
        "model": MODEL,
        "messages": [{ "role": "user", "content": "answer with a tool" }],
        "stream": true,
        "tools": [{
            "type": "function",
            "function": {
                "name": "get_answer",
                "description": "Return an answer",
                "parameters": {
                    "type": "object",
                    "properties": { "question": { "type": "string" } },
                    "required": ["question"]
                }
            }
        }]
    });
    let request = serde_json::to_string(&request).expect("serialize request");
    let (status, response_body) = post_http_raw(
        &server.addr,
        "/v1/chat/completions",
        &request,
        &[
            ("Content-Type", "application/json"),
            ("Authorization", &format!("Bearer {platform_key}")),
        ],
    );
    server.join();
    local_join.join().expect("join local upstream");
    aggregate_join.join().expect("join aggregate upstream");

    assert_eq!(status, 200, "gateway response: {response_body}");
    assert_eq!(
        local_rx.try_iter().count(),
        0,
        "local account must be skipped"
    );
    let aggregate_requests = aggregate_rx.try_iter().collect::<Vec<_>>();
    assert_eq!(aggregate_requests.len(), 1, "aggregate API request count");
    assert_eq!(
        aggregate_requests[0].path,
        "/backend-api/codex/chat/completions"
    );
    let aggregate_body: serde_json::Value =
        serde_json::from_slice(&decode_upstream_request_body(&aggregate_requests[0]))
            .expect("parse aggregate request body");
    assert_eq!(aggregate_body["model"], UPSTREAM_MODEL);
    assert!(aggregate_body["messages"].is_array());
    assert!(aggregate_body["tools"].is_array());
    assert!(
        response_body.contains("\"tool_calls\""),
        "chat completions response: {response_body}"
    );
    assert!(response_body.contains("\"id\":\"call_hybrid_1\""));
    assert!(response_body.contains("\"name\":\"get_answer\""));
    assert!(response_body.contains("{\\\"question\\\":\\\"2+2\\\"}"));
    assert!(response_body.contains("\"finish_reason\":\"tool_calls\""));
    assert!(response_body.contains("data: [DONE]"));
    let aggregate = storage
        .find_aggregate_api_by_id(aggregate_id)
        .expect("read aggregate API")
        .expect("aggregate API exists");
    assert_eq!(aggregate.consecutive_failures, 0);
    assert!(!aggregate.auto_disabled);
}

#[test]
fn hybrid_aggregate_first_chat_monthly_limit_falls_back_and_counts_once() {
    let _lock = test_env_guard();
    let dir = new_test_dir("codexmanager-hybrid-aggregate-first-chat-daily-limit");
    let db_path: PathBuf = dir.join("codexmanager.db");
    let _db_guard = EnvGuard::set("CODEXMANAGER_DB_PATH", db_path.to_string_lossy().as_ref());

    let (local_addr, local_rx, local_join) = start_mock_upstream_sequence_lenient(
        vec![(200, response_json("resp_account_chat_daily_limit_fallback"))],
        Duration::from_secs(2),
    );
    let local_base = format!("http://{local_addr}/backend-api/codex");
    let _upstream_guard = EnvGuard::set("CODEXMANAGER_UPSTREAM_BASE_URL", &local_base);
    let monthly_limit = serde_json::json!({
        "code": "USAGE_LIMIT_EXCEEDED",
        "message": "error: code=429 reason=\"MONTHLY_LIMIT_EXCEEDED\" message=\"monthly usage limit exceeded\" metadata=map[]"
    })
    .to_string();
    let (aggregate_addr, aggregate_rx, aggregate_join) =
        start_mock_upstream_sequence_lenient(vec![(429, monthly_limit); 4], Duration::from_secs(2));

    let storage = Storage::open(&db_path).expect("open db");
    storage.init().expect("init db");
    let now = now_ts();
    let aggregate_id = "agg_hybrid_aggregate_first_chat_daily_limit";
    let key_id = "gk_hybrid_aggregate_first_chat_daily_limit";
    let platform_key = "pk_hybrid_aggregate_first_chat_daily_limit";
    insert_active_account(&storage, "acc_hybrid_aggregate_first_chat_daily_limit", now);
    insert_aggregate_api(
        &storage,
        aggregate_id,
        &aggregate_addr,
        "/chat/completions",
        now,
    );
    storage
        .set_aggregate_api_auto_toggle_enabled(aggregate_id, true)
        .expect("enable aggregate API auto toggle");
    seed_dual_routes(&storage, aggregate_id);
    insert_hybrid_key_with_rotation(
        &storage,
        key_id,
        platform_key,
        "hybrid_aggregate_first_rotation",
        now,
    );

    let server = codexmanager_service::start_one_shot_server().expect("start server");
    let request = serde_json::json!({
        "model": MODEL,
        "messages": [{ "role": "user", "content": "hello" }],
        "stream": false
    });
    let request = serde_json::to_string(&request).expect("serialize request");
    let (status, response_body) = post_http_raw(
        &server.addr,
        "/v1/chat/completions",
        &request,
        &[
            ("Content-Type", "application/json"),
            ("Authorization", &format!("Bearer {platform_key}")),
        ],
    );
    server.join();
    local_join.join().expect("join local upstream");
    aggregate_join.join().expect("join aggregate upstream");

    assert_eq!(status, 200, "gateway response: {response_body}");
    let response: serde_json::Value =
        serde_json::from_str(&response_body).expect("parse chat completions response");
    assert_eq!(response["id"], "resp_account_chat_daily_limit_fallback");
    assert_eq!(response["choices"][0]["message"]["content"], "ok");
    let local_requests = local_rx.try_iter().collect::<Vec<_>>();
    assert_eq!(local_requests.len(), 1, "account fallback request count");
    assert_eq!(local_requests[0].path, "/backend-api/codex/responses");
    let aggregate_requests = aggregate_rx.try_iter().collect::<Vec<_>>();
    assert_eq!(
        aggregate_requests.len(),
        4,
        "aggregate API retry request count"
    );
    assert!(aggregate_requests
        .iter()
        .all(|item| item.path == "/backend-api/codex/chat/completions"));
    let aggregate_body: serde_json::Value =
        serde_json::from_slice(&decode_upstream_request_body(&aggregate_requests[0]))
            .expect("parse aggregate request body");
    assert_eq!(aggregate_body["model"], UPSTREAM_MODEL);
    assert!(aggregate_body["messages"].is_array());
    let aggregate = storage
        .find_aggregate_api_by_id(aggregate_id)
        .expect("read aggregate API")
        .expect("aggregate API exists");
    assert_eq!(aggregate.consecutive_failures, 1);
    assert!(!aggregate.auto_disabled);
}

#[test]
fn hybrid_dual_route_prefers_active_account_for_streaming_responses() {
    let _lock = test_env_guard();
    let dir = new_test_dir("codexmanager-hybrid-dual-account-first");
    let db_path: PathBuf = dir.join("codexmanager.db");
    let _db_guard = EnvGuard::set("CODEXMANAGER_DB_PATH", db_path.to_string_lossy().as_ref());

    let account_sse = concat!(
        "data: {\"type\":\"response.output_text.delta\",\"delta\":\"account ok\"}\n\n",
        "data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp_hybrid_account\",\"model\":\"gpt-hybrid-route-test\",\"usage\":{\"input_tokens\":3,\"output_tokens\":1,\"total_tokens\":4}}}\n\n",
        "data: [DONE]\n\n"
    );
    let (local_addr, local_rx, local_join) =
        start_mock_upstream_sequence_lenient_with_content_types(
            vec![(
                200,
                account_sse.to_string(),
                "text/event-stream".to_string(),
            )],
            Duration::from_secs(2),
        );
    let local_base = format!("http://{local_addr}/backend-api/codex");
    let _upstream_guard = EnvGuard::set("CODEXMANAGER_UPSTREAM_BASE_URL", &local_base);
    let (aggregate_addr, aggregate_rx, aggregate_join) = start_mock_upstream_sequence_lenient(
        vec![(200, response_json("resp_aggregate_should_not_run"))],
        Duration::from_secs(2),
    );

    let storage = Storage::open(&db_path).expect("open db");
    storage.init().expect("init db");
    let now = now_ts();
    let aggregate_id = "agg_hybrid_dual_account_first";
    let key_id = "gk_hybrid_dual_account_first";
    let platform_key = "pk_hybrid_dual_account_first";
    insert_active_account(&storage, "acc_hybrid_dual_account_first", now);
    insert_aggregate_api(&storage, aggregate_id, &aggregate_addr, "/responses", now);
    seed_dual_routes(&storage, aggregate_id);
    insert_hybrid_key(&storage, key_id, platform_key, now);

    let server = codexmanager_service::start_one_shot_server().expect("start server");
    let request = serde_json::json!({
        "model": MODEL,
        "input": "hello",
        "stream": true
    });
    let request = serde_json::to_string(&request).expect("serialize request");
    let (status, response_body) = post_http_raw(
        &server.addr,
        "/v1/responses",
        &request,
        &[
            ("Content-Type", "application/json"),
            ("Authorization", &format!("Bearer {platform_key}")),
        ],
    );
    server.join();
    local_join.join().expect("join local upstream");
    aggregate_join.join().expect("join aggregate upstream");

    assert_eq!(status, 200, "gateway response: {response_body}");
    assert!(response_body.contains("resp_hybrid_account"));
    let local_requests = local_rx.try_iter().collect::<Vec<_>>();
    assert_eq!(local_requests.len(), 1, "local account request count");
    assert_eq!(local_requests[0].path, "/backend-api/codex/responses");
    let local_body: serde_json::Value =
        serde_json::from_slice(&decode_upstream_request_body(&local_requests[0]))
            .expect("parse local account request body");
    assert_eq!(local_body["model"], MODEL);
    assert_eq!(
        aggregate_rx.try_iter().count(),
        0,
        "aggregate API must remain idle after account success"
    );
}

#[test]
fn hybrid_dual_route_falls_back_to_aggregate_when_account_pool_is_empty() {
    let _lock = test_env_guard();
    let dir = new_test_dir("codexmanager-hybrid-dual-empty-account-pool");
    let db_path: PathBuf = dir.join("codexmanager.db");
    let _db_guard = EnvGuard::set("CODEXMANAGER_DB_PATH", db_path.to_string_lossy().as_ref());

    let (local_addr, local_rx, local_join) = start_mock_upstream_sequence_lenient(
        vec![(200, response_json("resp_local_should_not_run"))],
        Duration::from_secs(2),
    );
    let local_base = format!("http://{local_addr}/backend-api/codex");
    let _upstream_guard = EnvGuard::set("CODEXMANAGER_UPSTREAM_BASE_URL", &local_base);
    let (aggregate_addr, aggregate_rx, aggregate_join) = start_mock_upstream_sequence_lenient(
        vec![(200, response_json("resp_hybrid_empty_account_fallback"))],
        Duration::from_secs(2),
    );

    let storage = Storage::open(&db_path).expect("open db");
    storage.init().expect("init db");
    let now = now_ts();
    let aggregate_id = "agg_hybrid_dual_empty_account_pool";
    let key_id = "gk_hybrid_dual_empty_account_pool";
    let platform_key = "pk_hybrid_dual_empty_account_pool";
    insert_aggregate_api(&storage, aggregate_id, &aggregate_addr, "/responses", now);
    seed_dual_routes(&storage, aggregate_id);
    insert_hybrid_key(&storage, key_id, platform_key, now);

    let server = codexmanager_service::start_one_shot_server().expect("start server");
    let request = serde_json::json!({
        "model": MODEL,
        "input": "hello",
        "stream": false
    });
    let request = serde_json::to_string(&request).expect("serialize request");
    let (status, response_body) = post_http_raw(
        &server.addr,
        "/v1/responses",
        &request,
        &[
            ("Content-Type", "application/json"),
            ("Authorization", &format!("Bearer {platform_key}")),
        ],
    );
    server.join();
    local_join.join().expect("join local upstream");
    aggregate_join.join().expect("join aggregate upstream");

    assert_eq!(status, 200, "gateway response: {response_body}");
    assert!(response_body.contains("resp_hybrid_empty_account_fallback"));
    assert_eq!(
        local_rx.try_iter().count(),
        0,
        "local account request count"
    );
    let aggregate_requests = aggregate_rx.try_iter().collect::<Vec<_>>();
    assert_eq!(aggregate_requests.len(), 1, "aggregate API request count");
    assert_eq!(aggregate_requests[0].path, "/backend-api/codex/responses");
    let aggregate_body: serde_json::Value =
        serde_json::from_slice(&decode_upstream_request_body(&aggregate_requests[0]))
            .expect("parse aggregate request body");
    assert_eq!(aggregate_body["model"], UPSTREAM_MODEL);
}

#[test]
fn hybrid_account_only_uses_account_and_ignores_unbound_aggregate_api() {
    let _lock = test_env_guard();
    let dir = new_test_dir("codexmanager-hybrid-account-only");
    let db_path: PathBuf = dir.join("codexmanager.db");
    let _db_guard = EnvGuard::set("CODEXMANAGER_DB_PATH", db_path.to_string_lossy().as_ref());

    let (local_addr, local_rx, local_join) = start_mock_upstream_sequence_lenient(
        vec![(200, response_json("resp_hybrid_account_only"))],
        Duration::from_secs(2),
    );
    let local_base = format!("http://{local_addr}/backend-api/codex");
    let _upstream_guard = EnvGuard::set("CODEXMANAGER_UPSTREAM_BASE_URL", &local_base);
    let (aggregate_addr, aggregate_rx, aggregate_join) = start_mock_upstream_sequence_lenient(
        vec![(200, response_json("resp_aggregate_should_not_run"))],
        Duration::from_secs(2),
    );

    let storage = Storage::open(&db_path).expect("open db");
    storage.init().expect("init db");
    let now = now_ts();
    let aggregate_id = "agg_hybrid_account_only_unbound";
    let key_id = "gk_hybrid_account_only";
    let platform_key = "pk_hybrid_account_only";
    insert_active_account(&storage, "acc_hybrid_account_only", now);
    insert_aggregate_api(&storage, aggregate_id, &aggregate_addr, "/responses", now);
    seed_model_catalog_models(&storage, &[MODEL]);
    insert_hybrid_key(&storage, key_id, platform_key, now);

    let server = codexmanager_service::start_one_shot_server().expect("start server");
    let request = serde_json::json!({
        "model": MODEL,
        "messages": [{ "role": "user", "content": "hello" }],
        "stream": false
    });
    let request = serde_json::to_string(&request).expect("serialize request");
    let (status, response_body) = post_http_raw(
        &server.addr,
        "/v1/chat/completions",
        &request,
        &[
            ("Content-Type", "application/json"),
            ("Authorization", &format!("Bearer {platform_key}")),
        ],
    );
    server.join();
    local_join.join().expect("join local upstream");
    aggregate_join.join().expect("join aggregate upstream");

    assert_eq!(status, 200, "gateway response: {response_body}");
    let response: serde_json::Value =
        serde_json::from_str(&response_body).expect("parse chat completions response");
    assert_eq!(response["id"], "resp_hybrid_account_only");
    assert_eq!(response["choices"][0]["message"]["content"], "ok");
    let local_requests = local_rx.try_iter().collect::<Vec<_>>();
    assert_eq!(local_requests.len(), 1, "local account request count");
    assert_eq!(local_requests[0].path, "/backend-api/codex/responses");
    assert_eq!(
        aggregate_rx.try_iter().count(),
        0,
        "unbound aggregate API must remain idle"
    );
}

#[test]
fn hybrid_aggregate_first_uses_aggregate_api_even_when_reported_balance_is_zero() {
    let _lock = test_env_guard();
    let dir = new_test_dir("codexmanager-hybrid-aggregate-first-zero-balance");
    let db_path: PathBuf = dir.join("codexmanager.db");
    let _db_guard = EnvGuard::set("CODEXMANAGER_DB_PATH", db_path.to_string_lossy().as_ref());

    let (local_addr, local_rx, local_join) = start_mock_upstream_sequence_lenient(
        vec![(200, response_json("resp_local_should_not_run"))],
        Duration::from_secs(2),
    );
    let local_base = format!("http://{local_addr}/backend-api/codex");
    let _upstream_guard = EnvGuard::set("CODEXMANAGER_UPSTREAM_BASE_URL", &local_base);
    let (aggregate_addr, aggregate_rx, aggregate_join) = start_mock_upstream_sequence_lenient(
        vec![(200, response_json("resp_aggregate_first"))],
        Duration::from_secs(2),
    );

    let storage = Storage::open(&db_path).expect("open db");
    storage.init().expect("init db");
    let now = now_ts();
    let aggregate_id = "agg_hybrid_aggregate_first_zero_balance";
    let key_id = "gk_hybrid_aggregate_first_zero_balance";
    let platform_key = "pk_hybrid_aggregate_first_zero_balance";
    insert_active_account(&storage, "acc_hybrid_aggregate_first_zero_balance", now);
    insert_aggregate_api(&storage, aggregate_id, &aggregate_addr, "/responses", now);
    storage
        .update_aggregate_api_balance_result(
            aggregate_id,
            true,
            Some(r#"{"remaining":0,"unit":"USD"}"#),
            None,
        )
        .expect("record zero aggregate balance");
    seed_dual_routes(&storage, aggregate_id);
    insert_hybrid_key_with_rotation(
        &storage,
        key_id,
        platform_key,
        "hybrid_aggregate_first_rotation",
        now,
    );

    let server = codexmanager_service::start_one_shot_server().expect("start server");
    let request = serde_json::json!({
        "model": MODEL,
        "input": "hello",
        "stream": false
    });
    let request = serde_json::to_string(&request).expect("serialize request");
    let (status, response_body) = post_http_raw(
        &server.addr,
        "/v1/responses",
        &request,
        &[
            ("Content-Type", "application/json"),
            ("Authorization", &format!("Bearer {platform_key}")),
        ],
    );
    server.join();
    local_join.join().expect("join local upstream");
    aggregate_join.join().expect("join aggregate upstream");

    assert_eq!(status, 200, "gateway response: {response_body}");
    assert!(response_body.contains("resp_aggregate_first"));
    assert_eq!(
        local_rx.try_iter().count(),
        0,
        "account pool must remain idle after aggregate success"
    );
    assert_eq!(
        aggregate_rx.try_iter().count(),
        1,
        "aggregate API request count"
    );

    let log = storage
        .list_request_logs(Some(&format!("key:={key_id}")), 10)
        .expect("list request logs")
        .into_iter()
        .find(|item| item.request_path == "/v1/responses")
        .expect("request log");
    assert_eq!(log.actual_source_kind.as_deref(), Some("aggregate_api"));
    assert_eq!(log.actual_source_id.as_deref(), Some(aggregate_id));
}

#[test]
fn hybrid_aggregate_first_falls_back_to_account_after_aggregate_failure() {
    let _lock = test_env_guard();
    let dir = new_test_dir("codexmanager-hybrid-aggregate-first-fallback");
    let db_path: PathBuf = dir.join("codexmanager.db");
    let _db_guard = EnvGuard::set("CODEXMANAGER_DB_PATH", db_path.to_string_lossy().as_ref());

    let (local_addr, local_rx, local_join) = start_mock_upstream_sequence_lenient(
        vec![(200, response_json("resp_account_fallback"))],
        Duration::from_secs(2),
    );
    let local_base = format!("http://{local_addr}/backend-api/codex");
    let _upstream_guard = EnvGuard::set("CODEXMANAGER_UPSTREAM_BASE_URL", &local_base);
    let aggregate_failure = serde_json::json!({
        "code": "rate_limit_exceeded",
        "message": "Concurrency limit exceeded"
    })
    .to_string();
    let (aggregate_addr, aggregate_rx, aggregate_join) = start_mock_upstream_sequence_lenient(
        vec![(200, aggregate_failure); 4],
        Duration::from_secs(2),
    );

    let storage = Storage::open(&db_path).expect("open db");
    storage.init().expect("init db");
    let now = now_ts();
    let aggregate_id = "agg_hybrid_aggregate_first_fallback";
    let key_id = "gk_hybrid_aggregate_first_fallback";
    let platform_key = "pk_hybrid_aggregate_first_fallback";
    insert_active_account(&storage, "acc_hybrid_aggregate_first_fallback", now);
    insert_aggregate_api(&storage, aggregate_id, &aggregate_addr, "/responses", now);
    storage
        .set_aggregate_api_auto_toggle_enabled(aggregate_id, true)
        .expect("enable aggregate API auto toggle");
    storage
        .record_aggregate_api_daily_quota_failure(aggregate_id)
        .expect("seed prior daily quota failure");
    seed_dual_routes(&storage, aggregate_id);
    insert_hybrid_key_with_rotation(
        &storage,
        key_id,
        platform_key,
        "hybrid_aggregate_first_rotation",
        now,
    );

    let server = codexmanager_service::start_one_shot_server().expect("start server");
    let request = serde_json::json!({
        "model": MODEL,
        "input": "hello",
        "stream": false
    });
    let request = serde_json::to_string(&request).expect("serialize request");
    let (status, response_body) = post_http_raw(
        &server.addr,
        "/v1/responses",
        &request,
        &[
            ("Content-Type", "application/json"),
            ("Authorization", &format!("Bearer {platform_key}")),
        ],
    );
    server.join();
    local_join.join().expect("join local upstream");
    aggregate_join.join().expect("join aggregate upstream");

    assert_eq!(status, 200, "gateway response: {response_body}");
    assert!(response_body.contains("resp_account_fallback"));
    assert_eq!(
        local_rx.try_iter().count(),
        1,
        "account fallback request count"
    );
    assert_eq!(
        aggregate_rx.try_iter().count(),
        4,
        "aggregate API retry request count"
    );
    let aggregate = storage
        .find_aggregate_api_by_id(aggregate_id)
        .expect("read aggregate API")
        .expect("aggregate API exists");
    assert_eq!(aggregate.consecutive_failures, 1);

    let log = storage
        .list_request_logs(Some(&format!("key:={key_id}")), 10)
        .expect("list request logs")
        .into_iter()
        .find(|item| item.request_path == "/v1/responses")
        .expect("request log");
    assert_eq!(log.actual_source_kind.as_deref(), Some("openai_account"));
    assert_eq!(
        log.actual_source_id.as_deref(),
        Some("acc_hybrid_aggregate_first_fallback")
    );
}

#[test]
fn hybrid_aggregate_first_daily_limit_falls_back_and_counts_once_per_request() {
    let _lock = test_env_guard();
    let dir = new_test_dir("codexmanager-hybrid-aggregate-first-daily-limit");
    let db_path: PathBuf = dir.join("codexmanager.db");
    let _db_guard = EnvGuard::set("CODEXMANAGER_DB_PATH", db_path.to_string_lossy().as_ref());

    let (local_addr, local_rx, local_join) = start_mock_upstream_sequence_lenient(
        vec![(200, response_json("resp_account_daily_limit_fallback"))],
        Duration::from_secs(2),
    );
    let local_base = format!("http://{local_addr}/backend-api/codex");
    let _upstream_guard = EnvGuard::set("CODEXMANAGER_UPSTREAM_BASE_URL", &local_base);
    let daily_limit = serde_json::json!({
        "error": {
            "code": "DAILY_LIMIT_EXCEEDED",
            "message": "daily usage limit exceeded"
        }
    })
    .to_string();
    let (aggregate_addr, aggregate_rx, aggregate_join) =
        start_mock_upstream_sequence_lenient(vec![(200, daily_limit); 4], Duration::from_secs(2));

    let storage = Storage::open(&db_path).expect("open db");
    storage.init().expect("init db");
    let now = now_ts();
    let aggregate_id = "agg_hybrid_aggregate_first_daily_limit";
    let key_id = "gk_hybrid_aggregate_first_daily_limit";
    let platform_key = "pk_hybrid_aggregate_first_daily_limit";
    insert_active_account(&storage, "acc_hybrid_aggregate_first_daily_limit", now);
    insert_aggregate_api(&storage, aggregate_id, &aggregate_addr, "/responses", now);
    storage
        .set_aggregate_api_auto_toggle_enabled(aggregate_id, true)
        .expect("enable aggregate API auto toggle");
    seed_dual_routes(&storage, aggregate_id);
    insert_hybrid_key_with_rotation(
        &storage,
        key_id,
        platform_key,
        "hybrid_aggregate_first_rotation",
        now,
    );

    let server = codexmanager_service::start_one_shot_server().expect("start server");
    let request = serde_json::json!({
        "model": MODEL,
        "input": "hello",
        "stream": false
    });
    let request = serde_json::to_string(&request).expect("serialize request");
    let (status, response_body) = post_http_raw(
        &server.addr,
        "/v1/responses",
        &request,
        &[
            ("Content-Type", "application/json"),
            ("Authorization", &format!("Bearer {platform_key}")),
        ],
    );
    server.join();
    local_join.join().expect("join local upstream");
    aggregate_join.join().expect("join aggregate upstream");

    assert_eq!(status, 200, "gateway response: {response_body}");
    assert!(response_body.contains("resp_account_daily_limit_fallback"));
    assert_eq!(
        local_rx.try_iter().count(),
        1,
        "account fallback request count"
    );
    assert_eq!(
        aggregate_rx.try_iter().count(),
        4,
        "aggregate API retry request count"
    );
    let aggregate = storage
        .find_aggregate_api_by_id(aggregate_id)
        .expect("read aggregate API")
        .expect("aggregate API exists");
    assert_eq!(aggregate.consecutive_failures, 1);
    assert!(!aggregate.auto_disabled);
}

#[test]
fn hybrid_aggregate_first_third_daily_failure_trips_and_next_request_skips_aggregate() {
    let _lock = test_env_guard();
    let dir = new_test_dir("codexmanager-hybrid-aggregate-first-auto-disable");
    let db_path: PathBuf = dir.join("codexmanager.db");
    let _db_guard = EnvGuard::set("CODEXMANAGER_DB_PATH", db_path.to_string_lossy().as_ref());

    let account_responses = (1..=4)
        .map(|request_number| {
            (
                200,
                response_json(&format!("resp_account_auto_disable_{request_number}")),
            )
        })
        .collect::<Vec<_>>();
    let (local_addr, local_rx, local_join) =
        start_mock_upstream_sequence_lenient(account_responses, Duration::from_secs(2));
    let local_base = format!("http://{local_addr}/backend-api/codex");
    let _upstream_guard = EnvGuard::set("CODEXMANAGER_UPSTREAM_BASE_URL", &local_base);
    let daily_limit = serde_json::json!({
        "error": {
            "code": "DAILY_LIMIT_EXCEEDED",
            "message": "daily usage limit exceeded"
        }
    })
    .to_string();
    let aggregate_responses = (0..12)
        .map(|_| (200, daily_limit.clone()))
        .collect::<Vec<_>>();
    let (aggregate_addr, aggregate_rx, aggregate_join) =
        start_mock_upstream_sequence_lenient(aggregate_responses, Duration::from_secs(2));

    let storage = Storage::open(&db_path).expect("open db");
    storage.init().expect("init db");
    let now = now_ts();
    let aggregate_id = "agg_hybrid_aggregate_first_auto_disable";
    let key_id = "gk_hybrid_aggregate_first_auto_disable";
    let platform_key = "pk_hybrid_aggregate_first_auto_disable";
    insert_active_account(&storage, "acc_hybrid_aggregate_first_auto_disable", now);
    insert_aggregate_api(&storage, aggregate_id, &aggregate_addr, "/responses", now);
    storage
        .set_aggregate_api_auto_toggle_enabled(aggregate_id, true)
        .expect("enable aggregate API auto toggle");
    seed_dual_routes(&storage, aggregate_id);
    insert_hybrid_key_with_rotation(
        &storage,
        key_id,
        platform_key,
        "hybrid_aggregate_first_rotation",
        now,
    );

    let request = serde_json::json!({
        "model": MODEL,
        "input": "hello",
        "stream": false
    });
    let request = serde_json::to_string(&request).expect("serialize request");
    let authorization = format!("Bearer {platform_key}");
    for request_number in 1_i64..=4 {
        let server = codexmanager_service::start_one_shot_server().expect("start server");
        let (status, response_body) = post_http_raw(
            &server.addr,
            "/v1/responses",
            &request,
            &[
                ("Content-Type", "application/json"),
                ("Authorization", authorization.as_str()),
            ],
        );
        server.join();

        assert_eq!(status, 200, "gateway response: {response_body}");
        assert!(response_body.contains(&format!("resp_account_auto_disable_{request_number}")));
        let aggregate = storage
            .find_aggregate_api_by_id(aggregate_id)
            .expect("read aggregate API")
            .expect("aggregate API exists");
        assert_eq!(aggregate.consecutive_failures, request_number.min(3));
        assert_eq!(aggregate.status, "active");
        assert_eq!(aggregate.auto_disabled, request_number >= 3);
        if request_number >= 3 {
            assert!(aggregate.auto_disabled_at.is_some());
            assert_eq!(
                aggregate.auto_disabled_reason.as_deref(),
                Some("daily_quota_exceeded")
            );
        }
    }
    local_join.join().expect("join local upstream");
    aggregate_join.join().expect("join aggregate upstream");

    assert_eq!(
        local_rx.try_iter().count(),
        4,
        "every request must finish through the account pool"
    );
    let aggregate_requests = aggregate_rx.try_iter().collect::<Vec<_>>();
    assert_eq!(
        aggregate_requests.len(),
        12,
        "only the first three requests may reach the aggregate API"
    );
    assert!(aggregate_requests
        .iter()
        .all(|item| item.path == "/backend-api/codex/responses"));
}

#[test]
fn hybrid_aggregate_first_non_stream_sse_daily_limit_falls_back_before_delivery() {
    let _lock = test_env_guard();
    let dir = new_test_dir("codexmanager-hybrid-aggregate-first-non-stream-sse-daily-limit");
    let db_path: PathBuf = dir.join("codexmanager.db");
    let _db_guard = EnvGuard::set("CODEXMANAGER_DB_PATH", db_path.to_string_lossy().as_ref());

    let (local_addr, local_rx, local_join) = start_mock_upstream_sequence_lenient(
        vec![(
            200,
            response_json("resp_account_non_stream_sse_daily_limit_fallback"),
        )],
        Duration::from_secs(2),
    );
    let local_base = format!("http://{local_addr}/backend-api/codex");
    let _upstream_guard = EnvGuard::set("CODEXMANAGER_UPSTREAM_BASE_URL", &local_base);
    let aggregate_sse = concat!(
        "event: response.created\n",
        "data: {\"type\":\"response.created\",\"response\":{\"id\":\"resp_aggregate_non_stream_sse_daily_limit\"}}\n\n",
        "event: response.output_text.delta\n",
        "data: {\"type\":\"response.output_text.delta\",\"delta\":\"aggregate content must be discarded\"}\n\n",
        "event: response.failed\n",
        "data: {\"type\":\"response.failed\",\"response\":{\"error\":{\"code\":\"DAILY_LIMIT_EXCEEDED\",\"message\":\"daily usage limit exceeded\"}}}\n\n"
    );
    let (aggregate_addr, aggregate_rx, aggregate_join) =
        start_mock_upstream_sequence_lenient_with_content_types(
            vec![
                (
                    200,
                    aggregate_sse.to_string(),
                    "application/json".to_string(),
                );
                4
            ],
            Duration::from_secs(2),
        );

    let storage = Storage::open(&db_path).expect("open db");
    storage.init().expect("init db");
    let now = now_ts();
    let aggregate_id = "agg_hybrid_aggregate_first_non_stream_sse_daily_limit";
    let key_id = "gk_hybrid_aggregate_first_non_stream_sse_daily_limit";
    let platform_key = "pk_hybrid_aggregate_first_non_stream_sse_daily_limit";
    insert_active_account(
        &storage,
        "acc_hybrid_aggregate_first_non_stream_sse_daily_limit",
        now,
    );
    insert_aggregate_api(&storage, aggregate_id, &aggregate_addr, "/responses", now);
    storage
        .set_aggregate_api_auto_toggle_enabled(aggregate_id, true)
        .expect("enable aggregate API auto toggle");
    seed_dual_routes(&storage, aggregate_id);
    insert_hybrid_key_with_rotation(
        &storage,
        key_id,
        platform_key,
        "hybrid_aggregate_first_rotation",
        now,
    );

    let server = codexmanager_service::start_one_shot_server().expect("start server");
    let request = serde_json::json!({
        "model": MODEL,
        "input": "hello",
        "stream": false
    });
    let request = serde_json::to_string(&request).expect("serialize request");
    let (status, response_body) = post_http_raw(
        &server.addr,
        "/v1/responses",
        &request,
        &[
            ("Content-Type", "application/json"),
            ("Authorization", &format!("Bearer {platform_key}")),
        ],
    );
    server.join();
    local_join.join().expect("join local upstream");
    aggregate_join.join().expect("join aggregate upstream");

    assert_eq!(status, 200, "gateway response: {response_body}");
    assert!(response_body.contains("resp_account_non_stream_sse_daily_limit_fallback"));
    assert!(!response_body.contains("resp_aggregate_non_stream_sse_daily_limit"));
    assert!(!response_body.contains("aggregate content must be discarded"));
    assert_eq!(
        local_rx.try_iter().count(),
        1,
        "account fallback request count"
    );
    assert_eq!(
        aggregate_rx.try_iter().count(),
        4,
        "aggregate API retry request count"
    );
    let aggregate = storage
        .find_aggregate_api_by_id(aggregate_id)
        .expect("read aggregate API")
        .expect("aggregate API exists");
    assert_eq!(aggregate.consecutive_failures, 1);
    assert!(!aggregate.auto_disabled);
}

#[test]
fn hybrid_aggregate_first_stream_daily_limit_falls_back_before_delivery() {
    let _lock = test_env_guard();
    let dir = new_test_dir("codexmanager-hybrid-aggregate-first-stream-daily-limit");
    let db_path: PathBuf = dir.join("codexmanager.db");
    let _db_guard = EnvGuard::set("CODEXMANAGER_DB_PATH", db_path.to_string_lossy().as_ref());

    let account_sse = concat!(
        "data: {\"type\":\"response.output_text.delta\",\"delta\":\"account fallback\"}\n\n",
        "data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp_account_stream_daily_limit_fallback\",\"model\":\"gpt-hybrid-route-test\",\"usage\":{\"input_tokens\":3,\"output_tokens\":2,\"total_tokens\":5}}}\n\n",
        "data: [DONE]\n\n"
    );
    let (local_addr, local_rx, local_join) =
        start_mock_upstream_sequence_lenient_with_content_types(
            vec![(
                200,
                account_sse.to_string(),
                "text/event-stream".to_string(),
            )],
            Duration::from_secs(2),
        );
    let local_base = format!("http://{local_addr}/backend-api/codex");
    let _upstream_guard = EnvGuard::set("CODEXMANAGER_UPSTREAM_BASE_URL", &local_base);
    let aggregate_sse = concat!(
        "event: response.created\n",
        "data: {\"type\":\"response.created\",\"response\":{\"id\":\"resp_aggregate_daily_limit\"}}\n\n",
        "event: response.failed\n",
        "data: {\"type\":\"response.failed\",\"response\":{\"error\":{\"code\":\"DAILY_LIMIT_EXCEEDED\",\"message\":\"daily usage limit exceeded\"}}}\n\n"
    );
    let (aggregate_addr, aggregate_rx, aggregate_join) =
        start_mock_upstream_sequence_lenient_with_content_types(
            vec![
                (
                    200,
                    aggregate_sse.to_string(),
                    "application/json".to_string(),
                );
                4
            ],
            Duration::from_secs(2),
        );

    let storage = Storage::open(&db_path).expect("open db");
    storage.init().expect("init db");
    let now = now_ts();
    let aggregate_id = "agg_hybrid_aggregate_first_stream_daily_limit";
    let key_id = "gk_hybrid_aggregate_first_stream_daily_limit";
    let platform_key = "pk_hybrid_aggregate_first_stream_daily_limit";
    insert_active_account(
        &storage,
        "acc_hybrid_aggregate_first_stream_daily_limit",
        now,
    );
    insert_aggregate_api(&storage, aggregate_id, &aggregate_addr, "/responses", now);
    storage
        .set_aggregate_api_auto_toggle_enabled(aggregate_id, true)
        .expect("enable aggregate API auto toggle");
    seed_dual_routes(&storage, aggregate_id);
    insert_hybrid_key_with_rotation(
        &storage,
        key_id,
        platform_key,
        "hybrid_aggregate_first_rotation",
        now,
    );

    let server = codexmanager_service::start_one_shot_server().expect("start server");
    let request = serde_json::json!({
        "model": MODEL,
        "input": "hello",
        "stream": true
    });
    let request = serde_json::to_string(&request).expect("serialize request");
    let (status, response_body) = post_http_raw(
        &server.addr,
        "/v1/responses",
        &request,
        &[
            ("Content-Type", "application/json"),
            ("Authorization", &format!("Bearer {platform_key}")),
        ],
    );
    server.join();
    local_join.join().expect("join local upstream");
    aggregate_join.join().expect("join aggregate upstream");

    assert_eq!(status, 200, "gateway response: {response_body}");
    assert!(response_body.contains("account fallback"));
    assert!(response_body.contains("resp_account_stream_daily_limit_fallback"));
    assert!(!response_body.contains("resp_aggregate_daily_limit"));
    assert_eq!(
        local_rx.try_iter().count(),
        1,
        "account fallback request count"
    );
    assert_eq!(
        aggregate_rx.try_iter().count(),
        4,
        "aggregate API retry request count"
    );
    let aggregate = storage
        .find_aggregate_api_by_id(aggregate_id)
        .expect("read aggregate API")
        .expect("aggregate API exists");
    assert_eq!(aggregate.consecutive_failures, 1);
    assert!(!aggregate.auto_disabled);
}

#[test]
fn hybrid_aggregate_first_generic_upstream_failure_counts_when_fresh_balance_is_zero() {
    let _lock = test_env_guard();
    let dir = new_test_dir("codexmanager-hybrid-aggregate-first-zero-balance-generic-error");
    let db_path: PathBuf = dir.join("codexmanager.db");
    let _db_guard = EnvGuard::set("CODEXMANAGER_DB_PATH", db_path.to_string_lossy().as_ref());

    let account_sse = concat!(
        "data: {\"type\":\"response.output_text.delta\",\"delta\":\"account fallback\"}\n\n",
        "data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp_account_zero_balance_fallback\",\"model\":\"gpt-hybrid-route-test\",\"usage\":{\"input_tokens\":3,\"output_tokens\":2,\"total_tokens\":5}}}\n\n",
        "data: [DONE]\n\n"
    );
    let (local_addr, local_rx, local_join) =
        start_mock_upstream_sequence_lenient_with_content_types(
            vec![(
                200,
                account_sse.to_string(),
                "text/event-stream".to_string(),
            )],
            Duration::from_secs(2),
        );
    let local_base = format!("http://{local_addr}/backend-api/codex");
    let _upstream_guard = EnvGuard::set("CODEXMANAGER_UPSTREAM_BASE_URL", &local_base);
    let aggregate_sse = concat!(
        "event: response.created\n",
        "data: {\"type\":\"response.created\",\"response\":{\"id\":\"resp_aggregate_zero_balance\"}}\n\n",
        "event: response.failed\n",
        "data: {\"type\":\"response.failed\",\"response\":{\"error\":{\"code\":\"upstream_error\",\"message\":\"Upstream request failed\"}}}\n\n"
    );
    let (aggregate_addr, aggregate_rx, aggregate_join) =
        start_mock_upstream_sequence_lenient_with_content_types(
            vec![
                (
                    200,
                    aggregate_sse.to_string(),
                    "application/json".to_string(),
                );
                4
            ],
            Duration::from_secs(2),
        );

    let storage = Storage::open(&db_path).expect("open db");
    storage.init().expect("init db");
    let now = now_ts();
    let aggregate_id = "agg_hybrid_aggregate_first_zero_balance_generic_error";
    let key_id = "gk_hybrid_aggregate_first_zero_balance_generic_error";
    let platform_key = "pk_hybrid_aggregate_first_zero_balance_generic_error";
    insert_active_account(
        &storage,
        "acc_hybrid_aggregate_first_zero_balance_generic_error",
        now,
    );
    insert_aggregate_api(&storage, aggregate_id, &aggregate_addr, "/responses", now);
    storage
        .set_aggregate_api_auto_toggle_enabled(aggregate_id, true)
        .expect("enable aggregate API auto toggle");
    storage
        .update_aggregate_api_balance_query(aggregate_id, true, Some("generic"), None, None, None)
        .expect("enable aggregate API balance query");
    storage
        .update_aggregate_api_balance_result(
            aggregate_id,
            true,
            Some(r#"{"isValid":true,"remaining":0.0,"unit":"USD"}"#),
            None,
        )
        .expect("store depleted aggregate API balance");
    seed_dual_routes(&storage, aggregate_id);
    insert_hybrid_key_with_rotation(
        &storage,
        key_id,
        platform_key,
        "hybrid_aggregate_first_rotation",
        now,
    );

    let server = codexmanager_service::start_one_shot_server().expect("start server");
    let request = serde_json::json!({
        "model": MODEL,
        "input": "hello",
        "stream": true
    });
    let request = serde_json::to_string(&request).expect("serialize request");
    let (status, response_body) = post_http_raw(
        &server.addr,
        "/v1/responses",
        &request,
        &[
            ("Content-Type", "application/json"),
            ("Authorization", &format!("Bearer {platform_key}")),
        ],
    );
    server.join();
    local_join.join().expect("join local upstream");
    aggregate_join.join().expect("join aggregate upstream");

    assert_eq!(status, 200, "gateway response: {response_body}");
    assert!(response_body.contains("account fallback"));
    assert_eq!(local_rx.try_iter().count(), 1);
    assert_eq!(aggregate_rx.try_iter().count(), 4);
    let aggregate = storage
        .find_aggregate_api_by_id(aggregate_id)
        .expect("read aggregate API")
        .expect("aggregate API exists");
    assert_eq!(aggregate.consecutive_failures, 1);
    assert!(!aggregate.auto_disabled);
}

#[test]
fn aggregate_rotation_stream_preflights_daily_limit_on_final_attempt() {
    let _lock = test_env_guard();
    let dir = new_test_dir("codexmanager-aggregate-stream-final-daily-limit");
    let db_path: PathBuf = dir.join("codexmanager.db");
    let _db_guard = EnvGuard::set("CODEXMANAGER_DB_PATH", db_path.to_string_lossy().as_ref());

    let transient_sse = concat!(
        "event: error\n",
        "data: {\"type\":\"error\",\"error\":{\"code\":\"rate_limit_exceeded\",\"message\":\"Concurrency limit exceeded\"}}\n\n"
    );
    let daily_limit_sse = concat!("event: error\n", "data: daily usage limit exceeded\n\n");
    let (aggregate_addr, aggregate_rx, aggregate_join) =
        start_mock_upstream_sequence_lenient_with_content_types(
            vec![
                (
                    200,
                    transient_sse.to_string(),
                    "text/event-stream".to_string(),
                ),
                (
                    200,
                    transient_sse.to_string(),
                    "text/event-stream".to_string(),
                ),
                (
                    200,
                    transient_sse.to_string(),
                    "text/event-stream".to_string(),
                ),
                (
                    200,
                    daily_limit_sse.to_string(),
                    "text/event-stream".to_string(),
                ),
            ],
            Duration::from_secs(2),
        );

    let storage = Storage::open(&db_path).expect("open db");
    storage.init().expect("init db");
    let now = now_ts();
    let aggregate_id = "agg_rotation_stream_final_daily_limit";
    let key_id = "gk_rotation_stream_final_daily_limit";
    let platform_key = "pk_rotation_stream_final_daily_limit";
    insert_aggregate_api(&storage, aggregate_id, &aggregate_addr, "/responses", now);
    storage
        .set_aggregate_api_auto_toggle_enabled(aggregate_id, true)
        .expect("enable aggregate API auto toggle");
    replace_with_aggregate_only_route(&storage, aggregate_id);
    insert_hybrid_key_with_rotation(
        &storage,
        key_id,
        platform_key,
        "aggregate_api_rotation",
        now,
    );

    let server = codexmanager_service::start_one_shot_server().expect("start server");
    let request = serde_json::json!({
        "model": MODEL,
        "input": "hello",
        "stream": true
    });
    let request = serde_json::to_string(&request).expect("serialize request");
    let (status, response_body) = post_http_raw(
        &server.addr,
        "/v1/responses",
        &request,
        &[
            ("Content-Type", "application/json"),
            ("Authorization", &format!("Bearer {platform_key}")),
        ],
    );
    server.join();
    aggregate_join.join().expect("join aggregate upstream");

    assert_eq!(status, 502, "gateway response: {response_body}");
    assert!(!response_body.contains("event: error"));
    assert_eq!(
        aggregate_rx.try_iter().count(),
        4,
        "aggregate API retry request count"
    );
    let aggregate = storage
        .find_aggregate_api_by_id(aggregate_id)
        .expect("read aggregate API")
        .expect("aggregate API exists");
    assert_eq!(aggregate.consecutive_failures, 1);
    assert!(!aggregate.auto_disabled);
}
