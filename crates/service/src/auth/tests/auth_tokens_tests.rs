use super::{
    auth_http_client_for_issuer, next_account_sort, normalized_device_poll_interval,
    openai_auth_loopback_http_client_build_count, persist_completed_oauth_login,
    poll_device_auth_token_async_with_timeout, resolve_existing_account_for_login, run_auth_future,
    DeviceLoginError, TokenResponse,
};
use crate::account_identity::{build_account_storage_id, pick_existing_account_id_by_identity};
use crate::auth_tokens::{
    build_api_key_exchange_request, build_exchange_code_request, ensure_workspace_allowed,
    format_api_key_exchange_status_error, format_token_endpoint_status_error,
    issuer_uses_loopback_host, parse_token_endpoint_error,
};
use codexmanager_core::auth::parse_id_token_claims;
use codexmanager_core::storage::{
    now_ts, Account, AccountAgentIdentity, LoginSession, Storage, Token,
};
use reqwest::header::{HeaderMap, HeaderValue};
use reqwest::Client;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::time::Duration;

#[test]
fn device_poll_interval_is_clamped_to_one_second() {
    assert_eq!(normalized_device_poll_interval(0), Duration::from_secs(1));
    assert_eq!(normalized_device_poll_interval(7), Duration::from_secs(7));
}

#[test]
fn device_poll_reports_expired_without_waiting_for_real_timeout() {
    let result = run_auth_future(poll_device_auth_token_async_with_timeout(
        "http://127.0.0.1:1",
        "device-auth-test",
        "CODE-TEST",
        0,
        Duration::ZERO,
        Arc::new(AtomicBool::new(false)),
    ));

    assert!(matches!(result, Err(DeviceLoginError::Expired)));
}

/// 函数 `build_account`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - id: 参数 id
/// - chatgpt_account_id: 参数 chatgpt_account_id
/// - workspace_id: 参数 workspace_id
///
/// # 返回
/// 返回函数执行结果
fn build_account(
    id: &str,
    chatgpt_account_id: Option<&str>,
    workspace_id: Option<&str>,
) -> Account {
    let now = now_ts();
    Account {
        id: id.to_string(),
        label: id.to_string(),
        issuer: "https://auth.openai.com".to_string(),
        chatgpt_account_id: chatgpt_account_id.map(|v| v.to_string()),
        workspace_id: workspace_id.map(|v| v.to_string()),
        group_name: None,
        sort: 0,
        status: "active".to_string(),
        created_at: now,
        updated_at: now,
    }
}

fn build_login_session(
    note: Option<&str>,
    tags: Option<&str>,
    group_name: Option<&str>,
) -> LoginSession {
    let now = now_ts();
    LoginSession {
        login_id: "login-test".to_string(),
        code_verifier: "verifier-test".to_string(),
        state: "state-test".to_string(),
        status: "completing".to_string(),
        error: None,
        workspace_id: None,
        note: note.map(str::to_string),
        tags: tags.map(str::to_string),
        group_name: group_name.map(str::to_string),
        created_at: now,
        updated_at: now,
    }
}

fn completed_oauth_tokens(suffix: &str) -> TokenResponse {
    TokenResponse {
        id_token: format!("id-{suffix}"),
        access_token: format!("access-{suffix}"),
        refresh_token: format!("refresh-{suffix}"),
    }
}

#[test]
fn completed_oauth_login_preserves_existing_account_details() {
    let mut storage = Storage::open_in_memory().expect("open in memory");
    storage.init().expect("init");
    let account_id = "subject-old::cgpt=cgpt-old|ws=ws-old";
    let created_at = now_ts().saturating_sub(100);
    storage
        .insert_account(&Account {
            id: account_id.to_string(),
            label: "本地名称".to_string(),
            issuer: "https://issuer.old".to_string(),
            chatgpt_account_id: Some("cgpt-old".to_string()),
            workspace_id: Some("ws-old".to_string()),
            group_name: Some("本地分组".to_string()),
            sort: 25,
            status: "disabled".to_string(),
            created_at,
            updated_at: created_at,
        })
        .expect("insert existing account");
    storage
        .upsert_account_metadata(account_id, Some("本地备注"), Some("本地标签"))
        .expect("insert metadata");
    storage
        .update_account_warranty_expires_on(account_id, Some("2027-08-12"))
        .expect("set warranty");
    storage
        .upsert_account_quota_capacity_override(account_id, Some(1234), Some(5678))
        .expect("set quota override");
    storage
        .upsert_account_subscription(
            account_id,
            true,
            Some("team"),
            Some("team"),
            Some(1_900_000_000),
            Some(1_800_000_000),
        )
        .expect("set subscription");
    storage
        .upsert_account_agent_identity(&AccountAgentIdentity {
            account_id: account_id.to_string(),
            agent_runtime_id: "runtime-old".to_string(),
            agent_private_key: "private-old".to_string(),
            task_id: Some("task-old".to_string()),
            chatgpt_user_id: "user-old".to_string(),
            chatgpt_account_is_fedramp: false,
            auth_mode: "agentIdentity".to_string(),
            workspace_id: Some("ws-old".to_string()),
            created_at,
            updated_at: created_at,
        })
        .expect("set agent identity");
    storage
        .insert_token(&Token {
            account_id: account_id.to_string(),
            id_token: "id-old".to_string(),
            access_token: "access-old".to_string(),
            refresh_token: "refresh-old".to_string(),
            api_key_access_token: Some("api-key-old".to_string()),
            last_refresh: created_at,
        })
        .expect("insert old token");
    assert!(storage
        .mark_account_refresh_token_invalid_if_current(
            account_id,
            "refresh-old",
            "refresh_token_invalid:refresh_token_invalidated",
        )
        .expect("mark old refresh token invalid"));
    storage
        .set_preferred_account(Some(account_id))
        .expect("set preferred account");

    persist_completed_oauth_login(
        &storage,
        &build_login_session(Some("本次备注"), Some("本次标签"), Some("本次分组")),
        account_id,
        "https://issuer.new",
        "subject-new",
        "授权返回名称".to_string(),
        Some("cgpt-new".to_string()),
        Some("ws-new".to_string()),
        completed_oauth_tokens("new"),
        None,
    )
    .expect("persist repeated OAuth login");

    let account = storage
        .find_account_by_id(account_id)
        .expect("find account")
        .expect("account exists");
    assert_eq!(account.label, "本地名称");
    assert_eq!(account.group_name.as_deref(), Some("本地分组"));
    assert_eq!(account.sort, 25);
    assert_eq!(account.status, "active");
    assert_eq!(account.created_at, created_at);
    assert_eq!(account.issuer, "https://issuer.new");
    assert_eq!(account.chatgpt_account_id.as_deref(), Some("cgpt-new"));
    assert_eq!(account.workspace_id.as_deref(), Some("ws-new"));

    let metadata = storage
        .find_account_metadata(account_id)
        .expect("find metadata")
        .expect("metadata exists");
    assert_eq!(metadata.note.as_deref(), Some("本地备注"));
    assert_eq!(metadata.tags.as_deref(), Some("本地标签"));
    let token = storage
        .find_token_by_account_id(account_id)
        .expect("find token")
        .expect("token exists");
    assert_eq!(token.id_token, "id-new");
    assert_eq!(token.access_token, "access-new");
    assert_eq!(token.refresh_token, "refresh-new");
    assert_eq!(token.api_key_access_token, None);
    assert_eq!(
        storage.preferred_account_id().expect("preferred account"),
        Some(account_id.to_string())
    );
    let summary = storage
        .list_account_summary_rows()
        .expect("list account summary");
    assert_eq!(
        summary[0].warranty_expires_on.as_deref(),
        Some("2027-08-12")
    );
    assert_eq!(summary[0].refresh_token_invalid_reason, None);
    let quota = storage
        .list_account_quota_capacity_overrides()
        .expect("list quota overrides");
    assert_eq!(quota.len(), 1);
    assert_eq!(quota[0].primary_window_tokens, Some(1234));
    assert_eq!(quota[0].secondary_window_tokens, Some(5678));
    let subscription = storage
        .find_account_subscription(account_id)
        .expect("find subscription")
        .expect("subscription exists");
    assert_eq!(subscription.plan_type.as_deref(), Some("team"));
    let identity = storage
        .find_account_agent_identity(account_id)
        .expect("find agent identity")
        .expect("agent identity exists");
    assert_eq!(identity.agent_runtime_id, "runtime-old");
    assert_eq!(identity.agent_private_key, "private-old");
    let subject_accounts = storage
        .list_account_workspace_identities_for_subject("subject-new")
        .expect("find updated subject identity");
    assert_eq!(subject_accounts.len(), 1);
    assert_eq!(subject_accounts[0].id, account_id);
}

#[test]
fn completed_oauth_login_preserves_empty_existing_group() {
    let storage = Storage::open_in_memory().expect("open in memory");
    storage.init().expect("init");
    let account_id = "existing-without-group";
    storage
        .insert_account(&build_account(account_id, Some("cgpt-old"), Some("ws-old")))
        .expect("insert existing account");

    persist_completed_oauth_login(
        &storage,
        &build_login_session(None, None, Some("本次分组")),
        account_id,
        "https://issuer.new",
        "subject-new",
        "授权返回名称".to_string(),
        Some("cgpt-new".to_string()),
        Some("ws-new".to_string()),
        completed_oauth_tokens("new"),
        Some("api-key-new".to_string()),
    )
    .expect("persist repeated OAuth login");

    let account = storage
        .find_account_by_id(account_id)
        .expect("find account")
        .expect("account exists");
    assert_eq!(account.group_name, None);
}

#[test]
fn completed_oauth_login_uses_session_details_for_new_account() {
    let storage = Storage::open_in_memory().expect("open in memory");
    storage.init().expect("init");
    let account_id = "subject-new::cgpt=cgpt-new|ws=ws-new";

    persist_completed_oauth_login(
        &storage,
        &build_login_session(Some("新增备注"), Some("新增标签"), Some("新增分组")),
        account_id,
        "https://issuer.new",
        "subject-new",
        "新增账号名称".to_string(),
        Some("cgpt-new".to_string()),
        Some("ws-new".to_string()),
        completed_oauth_tokens("new"),
        Some("api-key-new".to_string()),
    )
    .expect("persist new OAuth login");

    let account = storage
        .find_account_by_id(account_id)
        .expect("find account")
        .expect("account exists");
    assert_eq!(account.label, "新增账号名称");
    assert_eq!(account.group_name.as_deref(), Some("新增分组"));
    assert_eq!(account.status, "active");
    let metadata = storage
        .find_account_metadata(account_id)
        .expect("find metadata")
        .expect("metadata exists");
    assert_eq!(metadata.note.as_deref(), Some("新增备注"));
    assert_eq!(metadata.tags.as_deref(), Some("新增标签"));
    let token = storage
        .find_token_by_account_id(account_id)
        .expect("find token")
        .expect("token exists");
    assert_eq!(token.api_key_access_token.as_deref(), Some("api-key-new"));
}

/// 函数 `pick_existing_account_requires_exact_scope_when_workspace_present`
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
fn pick_existing_account_requires_exact_scope_when_workspace_present() {
    let storage = Storage::open_in_memory().expect("open in memory");
    storage.init().expect("init");
    storage
        .insert_account(&build_account("acc-ws-a", Some("cgpt-1"), Some("ws-a")))
        .expect("insert ws-a");

    let found = pick_existing_account_id_by_identity(
        storage.list_accounts().expect("list accounts").iter(),
        Some("cgpt-1"),
        Some("ws-b"),
        Some("sub-fallback"),
        None,
    );

    assert_eq!(found, None);
}

/// 函数 `pick_existing_account_matches_exact_workspace_scope`
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
fn pick_existing_account_matches_exact_workspace_scope() {
    let storage = Storage::open_in_memory().expect("open in memory");
    storage.init().expect("init");
    storage
        .insert_account(&build_account("acc-ws-a", Some("cgpt-1"), Some("ws-a")))
        .expect("insert ws-a");
    storage
        .insert_account(&build_account("acc-ws-b", Some("cgpt-1"), Some("ws-b")))
        .expect("insert ws-b");

    let found = pick_existing_account_id_by_identity(
        storage.list_accounts().expect("list accounts").iter(),
        Some("cgpt-1"),
        Some("ws-b"),
        Some("sub-fallback"),
        None,
    );

    assert_eq!(found.as_deref(), Some("acc-ws-b"));
}

/// 函数 `build_account_storage_id_keeps_login_scope_shape`
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
fn build_account_storage_id_keeps_login_scope_shape() {
    let account_id = build_account_storage_id("sub-1", Some("cgpt-1"), Some("ws-a"), None);
    assert_eq!(account_id, "sub-1::cgpt=cgpt-1|ws=ws-a");
}

/// 函数 `next_account_sort_uses_step_five`
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
fn next_account_sort_uses_step_five() {
    let storage = Storage::open_in_memory().expect("open in memory");
    storage.init().expect("init");
    storage
        .insert_account(&build_account("acc-1", Some("cgpt-1"), Some("ws-1")))
        .expect("insert account 1");
    storage
        .update_account_sort("acc-1", 2)
        .expect("update sort 1");
    storage
        .insert_account(&build_account("acc-2", Some("cgpt-2"), Some("ws-2")))
        .expect("insert account 2");
    storage
        .update_account_sort("acc-2", 7)
        .expect("update sort 2");

    assert_eq!(next_account_sort(&storage), 12);
}

#[test]
fn resolve_existing_account_for_login_uses_identity_lookup_without_tags() {
    let storage = Storage::open_in_memory().expect("open in memory");
    storage.init().expect("init");
    let existing = build_account(
        "subject-fallback::cgpt=cgpt-1|ws=ws-1",
        Some("cgpt-1"),
        Some("ws-1"),
    );
    storage
        .insert_account(&existing)
        .expect("insert existing account");

    let found = resolve_existing_account_for_login(
        &storage,
        "subject-fallback",
        Some("cgpt-1"),
        Some("ws-1"),
        Some("subject-fallback"),
    )
    .expect("resolve account");

    assert_eq!(found.as_deref(), Some(existing.id.as_str()));
}

#[test]
fn resolve_existing_account_for_login_preserves_tagged_fallback_behavior() {
    let storage = Storage::open_in_memory().expect("open in memory");
    storage.init().expect("init");
    storage
        .insert_account(&build_account(
            "subject-fallback",
            Some("cgpt-1"),
            Some("ws-1"),
        ))
        .expect("insert untagged account");

    let found = resolve_existing_account_for_login(
        &storage,
        "subject-fallback",
        Some("cgpt-1"),
        Some("ws-1"),
        Some("subject-fallback::team-a"),
    )
    .expect("resolve tagged account");

    assert_eq!(found.as_deref(), Some("subject-fallback"));
}

#[test]
fn resolve_existing_account_for_login_with_tags_uses_identity_candidates() {
    let storage = Storage::open_in_memory().expect("open in memory");
    storage.init().expect("init");
    storage
        .insert_account(&build_account(
            "subject-fallback",
            Some("cgpt-target"),
            Some("ws-target"),
        ))
        .expect("insert fallback account");
    storage
        .insert_account(&build_account(
            "acc-unrelated-newer",
            Some("cgpt-other"),
            Some("ws-other"),
        ))
        .expect("insert unrelated account");
    storage
        .touch_account_updated_at("acc-unrelated-newer")
        .expect("touch unrelated");

    let found = resolve_existing_account_for_login(
        &storage,
        "subject-fallback",
        Some("cgpt-target"),
        Some("ws-target"),
        Some("subject-fallback::team-a"),
    )
    .expect("resolve account");

    assert_eq!(found.as_deref(), Some("subject-fallback"));
}

#[test]
fn resolve_existing_account_for_login_keeps_same_team_accounts_separate_by_subject() {
    let storage = Storage::open_in_memory().expect("open in memory");
    storage.init().expect("init");
    let first = build_account("user-a::cgpt=shared|ws=team", Some("shared"), Some("team"));
    let second = build_account("user-b::cgpt=shared|ws=team", Some("shared"), Some("team"));
    storage.insert_account(&first).expect("insert first");
    storage.insert_account(&second).expect("insert second");
    storage
        .update_account_subject_identity(&first.id, "user-a")
        .expect("set first subject");
    storage
        .update_account_subject_identity(&second.id, "user-b")
        .expect("set second subject");

    assert_eq!(
        resolve_existing_account_for_login(
            &storage,
            "user-a",
            Some("shared"),
            Some("team"),
            Some("user-a"),
        )
        .expect("resolve first")
        .as_deref(),
        Some(first.id.as_str())
    );
    assert_eq!(
        resolve_existing_account_for_login(
            &storage,
            "user-b",
            Some("shared"),
            Some("team"),
            Some("user-b"),
        )
        .expect("resolve second")
        .as_deref(),
        Some(second.id.as_str())
    );
}

/// 函数 `jwt_with_claims`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - payload: 参数 payload
///
/// # 返回
/// 返回函数执行结果
fn jwt_with_claims(payload: &str) -> String {
    format!("eyJhbGciOiJIUzI1NiJ9.{payload}.sig")
}

/// 函数 `ensure_workspace_allowed_accepts_matching_auth_chatgpt_account_id`
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
fn ensure_workspace_allowed_accepts_matching_auth_chatgpt_account_id() {
    let token = jwt_with_claims(
        "eyJzdWIiOiJ1c2VyLTEiLCJodHRwczovL2FwaS5vcGVuYWkuY29tL2F1dGgiOnsiY2hhdGdwdF9hY2NvdW50X2lkIjoib3JnX2FiYyJ9fQ",
    );
    let claims = parse_id_token_claims(&token).expect("claims");

    let result = ensure_workspace_allowed(Some("org_abc"), &claims, &token, &token);

    assert!(result.is_ok(), "workspace should match: {:?}", result);
}

/// 函数 `ensure_workspace_allowed_rejects_mismatched_workspace`
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
fn ensure_workspace_allowed_rejects_mismatched_workspace() {
    let token = jwt_with_claims("eyJzdWIiOiJ1c2VyLTEiLCJ3b3Jrc3BhY2VfaWQiOiJvcmdfYWJjIn0");
    let claims = parse_id_token_claims(&token).expect("claims");

    let result = ensure_workspace_allowed(Some("org_other"), &claims, &token, &token);

    assert_eq!(
        result.expect_err("should reject mismatch"),
        "Login is restricted to workspace id org_other."
    );
}

/// 函数 `ensure_workspace_allowed_accepts_composite_scope_values_after_normalization`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-17
///
/// # 参数
/// 无
///
/// # 返回
/// 无
#[test]
fn ensure_workspace_allowed_accepts_composite_scope_values_after_normalization() {
    let token = "eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiJ1c2VyLTEiLCJ3b3Jrc3BhY2VfaWQiOiJnb29nbGUtb2F1dGgyfDEwNTY3MTMwNzY2NTg0MTQxOTc0ODo6Y2dwdD1lZDA4ZDU2YS1jMDM4LTQzMjItYjMyNS01M2Y1MDRjMGM4OGN8d3M9b3JnLUFQNnlwY01pODRUaGZ1ZWxpNkVVM0I0bSIsImh0dHBzOi8vYXBpLm9wZW5haS5jb20vYXV0aCI6eyJjaGF0Z3B0X2FjY291bnRfaWQiOiJnb29nbGUtb2F1dGgyfDEwNTY3MTMwNzY2NTg0MTQxOTc0ODo6Y2dwdD1lZDA4ZDU2YS1jMDM4LTQzMjItYjMyNS01M2Y1MDRjMGM4OGN8d3M9b3JnLUFQNnlwY01pODRUaGZ1ZWxpNkVVM0I0bSJ9fQ.sig".to_string();
    let claims = parse_id_token_claims(&token).expect("claims");

    let result = ensure_workspace_allowed(
        Some("org-AP6ypcMi84Thfueli6EU3B4m"),
        &claims,
        &token,
        &token,
    );

    assert!(result.is_ok(), "workspace should match after normalization");
}

/// 函数 `parse_token_endpoint_error_prefers_error_description`
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
fn parse_token_endpoint_error_prefers_error_description() {
    let detail = parse_token_endpoint_error(
        r#"{"error":"invalid_grant","error_description":"refresh token expired"}"#,
    );

    assert_eq!(detail.to_string(), "refresh token expired");
}

/// 函数 `parse_token_endpoint_error_reads_nested_error_message_and_code`
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
fn parse_token_endpoint_error_reads_nested_error_message_and_code() {
    let detail = parse_token_endpoint_error(
        r#"{"error":{"code":"proxy_auth_required","message":"proxy authentication required"}}"#,
    );

    assert_eq!(detail.to_string(), "proxy authentication required");
}

/// 函数 `parse_token_endpoint_error_preserves_plain_text_for_display`
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
fn parse_token_endpoint_error_preserves_plain_text_for_display() {
    let detail = parse_token_endpoint_error("service unavailable");

    assert_eq!(detail.to_string(), "service unavailable");
}

/// 函数 `parse_token_endpoint_error_summarizes_challenge_html`
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
fn parse_token_endpoint_error_summarizes_challenge_html() {
    let detail =
        parse_token_endpoint_error("<html><title>Just a moment...</title><body>cf</body></html>");

    assert_eq!(
        detail.to_string(),
        "Cloudflare 安全验证页（title=Just a moment...）"
    );
}

/// 函数 `parse_token_endpoint_error_summarizes_blocked_cloudflare_html`
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
fn parse_token_endpoint_error_summarizes_blocked_cloudflare_html() {
    let detail = parse_token_endpoint_error(
        "<html><body>Cloudflare error: Sorry, you have been blocked</body></html>",
    );

    assert_eq!(
        detail.to_string(),
        "Access blocked by Cloudflare. This usually happens when connecting from a restricted region"
    );
}

/// 函数 `parse_token_endpoint_error_summarizes_generic_html`
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
fn parse_token_endpoint_error_summarizes_generic_html() {
    let detail = parse_token_endpoint_error("<html><title>502 Bad Gateway</title></html>");

    assert_eq!(
        detail.to_string(),
        "上游返回 HTML 错误页（title=502 Bad Gateway）"
    );
}

/// 函数 `format_token_endpoint_status_error_appends_debug_headers`
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
fn format_token_endpoint_status_error_appends_debug_headers() {
    let mut headers = HeaderMap::new();
    headers.insert(
        "x-oai-request-id",
        HeaderValue::from_static("req_token_123"),
    );
    headers.insert("cf-ray", HeaderValue::from_static("ray_token_123"));
    headers.insert(
        "x-openai-authorization-error",
        HeaderValue::from_static("expired_session"),
    );
    headers.insert(
        "x-error-json",
        HeaderValue::from_static("eyJlcnJvciI6eyJjb2RlIjoidG9rZW5fZXhwaXJlZCJ9fQ=="),
    );

    let message = format_token_endpoint_status_error(
        reqwest::StatusCode::FORBIDDEN,
        &headers,
        "<html><title>Just a moment...</title></html>",
    );

    assert!(message.contains("token endpoint returned status 403 Forbidden"));
    assert!(message.contains("Cloudflare 安全验证页（title=Just a moment...）"));
    assert!(message.contains("request_id=req_token_123"));
    assert!(message.contains("cf_ray=ray_token_123"));
    assert!(message.contains("auth_error=expired_session"));
    assert!(message.contains("identity_error_code=token_expired"));
    assert!(message.contains("kind=cloudflare_challenge"));
}

/// 函数 `format_token_endpoint_status_error_marks_cloudflare_blocked_kind`
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
fn format_token_endpoint_status_error_marks_cloudflare_blocked_kind() {
    let mut headers = HeaderMap::new();
    headers.insert(
        "x-request-id",
        HeaderValue::from_static("req_token_blocked"),
    );
    headers.insert("cf-ray", HeaderValue::from_static("ray_token_blocked"));

    let message = format_token_endpoint_status_error(
        reqwest::StatusCode::FORBIDDEN,
        &headers,
        "<html><body>Cloudflare error: Sorry, you have been blocked</body></html>",
    );

    assert!(message.contains("token endpoint returned status 403 Forbidden"));
    assert!(message.contains(
        "Access blocked by Cloudflare. This usually happens when connecting from a restricted region"
    ));
    assert!(message.contains("request_id=req_token_blocked"));
    assert!(message.contains("cf_ray=ray_token_blocked"));
    assert!(message.contains("kind=cloudflare_blocked"));
}

/// 函数 `format_api_key_exchange_status_error_appends_debug_headers`
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
fn format_api_key_exchange_status_error_appends_debug_headers() {
    let mut headers = HeaderMap::new();
    headers.insert("x-request-id", HeaderValue::from_static("req_api_key_123"));
    headers.insert("cf-ray", HeaderValue::from_static("ray_api_key_123"));
    headers.insert(
        "x-error-json",
        HeaderValue::from_static("eyJlcnJvciI6eyJjb2RlIjoicHJveHlfYXV0aF9yZXF1aXJlZCJ9fQ=="),
    );

    let message = format_api_key_exchange_status_error(
        reqwest::StatusCode::BAD_GATEWAY,
        &headers,
        "<html><title>502 Bad Gateway</title></html>",
    );

    assert!(message.contains("api key exchange failed with status 502 Bad Gateway"));
    assert!(message.contains("上游返回 HTML 错误页（title=502 Bad Gateway）"));
    assert!(message.contains("request_id=req_api_key_123"));
    assert!(message.contains("cf_ray=ray_api_key_123"));
    assert!(message.contains("identity_error_code=proxy_auth_required"));
    assert!(message.contains("kind=html"));
}

/// 函数 `format_token_endpoint_status_error_accepts_raw_error_json_header`
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
fn format_token_endpoint_status_error_accepts_raw_error_json_header() {
    let mut headers = HeaderMap::new();
    headers.insert(
        "x-request-id",
        HeaderValue::from_static("req_token_raw_123"),
    );
    headers.insert(
        "x-error-json",
        HeaderValue::from_static("{\"identity_error_code\":\"org_membership_required\"}"),
    );

    let message = format_token_endpoint_status_error(
        reqwest::StatusCode::FORBIDDEN,
        &headers,
        "<html><title>Just a moment...</title></html>",
    );

    assert!(message.contains("request_id=req_token_raw_123"));
    assert!(message.contains("identity_error_code=org_membership_required"));
    assert!(message.contains("kind=cloudflare_challenge"));
}

/// 函数 `format_token_endpoint_status_error_uses_header_only_blocked_signal`
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
fn format_token_endpoint_status_error_uses_header_only_blocked_signal() {
    let mut headers = HeaderMap::new();
    headers.insert(
        "x-openai-authorization-error",
        HeaderValue::from_static("unsupported_country_region_territory"),
    );
    headers.insert(
        "cf-ray",
        HeaderValue::from_static("ray_token_header_blocked"),
    );

    let message = format_token_endpoint_status_error(reqwest::StatusCode::FORBIDDEN, &headers, "");

    assert!(message.contains("token endpoint returned status 403 Forbidden"));
    assert!(message.contains(
        "Access blocked by Cloudflare. This usually happens when connecting from a restricted region"
    ));
    assert!(message.contains("auth_error=unsupported_country_region_territory"));
    assert!(message.contains("kind=cloudflare_blocked"));
}

/// 函数 `format_api_key_exchange_status_error_uses_identity_header_when_body_empty`
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
fn format_api_key_exchange_status_error_uses_identity_header_when_body_empty() {
    let mut headers = HeaderMap::new();
    headers.insert(
        "x-error-json",
        HeaderValue::from_static("{\"identity_error_code\":\"org_membership_required\"}"),
    );

    let message =
        format_api_key_exchange_status_error(reqwest::StatusCode::FORBIDDEN, &headers, "");

    assert!(message.contains("api key exchange failed with status 403 Forbidden"));
    assert!(message.contains("identity error: org_membership_required"));
    assert!(message.contains("identity_error_code=org_membership_required"));
    assert!(message.contains("kind=identity_error"));
}

/// 函数 `format_token_endpoint_status_error_uses_cloudflare_edge_kind_when_only_cf_ray_exists`
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
fn format_token_endpoint_status_error_uses_cloudflare_edge_kind_when_only_cf_ray_exists() {
    let mut headers = HeaderMap::new();
    headers.insert("cf-ray", HeaderValue::from_static("ray_token_only_cf"));

    let message =
        format_token_endpoint_status_error(reqwest::StatusCode::BAD_GATEWAY, &headers, "");

    assert!(message.contains("token endpoint returned status 502 Bad Gateway"));
    assert!(message.contains("cf_ray=ray_token_only_cf"));
    assert!(message.contains("kind=cloudflare_edge"));
}

/// 函数 `issuer_uses_loopback_host_accepts_local_test_issuers`
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
fn issuer_uses_loopback_host_accepts_local_test_issuers() {
    assert!(issuer_uses_loopback_host("http://127.0.0.1:1455"));
    assert!(issuer_uses_loopback_host("http://localhost:1455"));
}

/// 函数 `issuer_uses_loopback_host_rejects_remote_issuers`
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
fn issuer_uses_loopback_host_rejects_remote_issuers() {
    assert!(!issuer_uses_loopback_host("https://auth.openai.com"));
}

#[test]
fn loopback_auth_http_client_reuses_cached_no_proxy_client() {
    let before = openai_auth_loopback_http_client_build_count();

    let _first = auth_http_client_for_issuer("http://127.0.0.1:1455");
    let after_first = openai_auth_loopback_http_client_build_count();
    let _second = auth_http_client_for_issuer("http://localhost:1455");
    let after_second = openai_auth_loopback_http_client_build_count();

    assert!(
        after_first == before || after_first == before + 1,
        "first loopback auth client access should initialize at most once; before={before}, after_first={after_first}"
    );
    assert_eq!(
        after_second, after_first,
        "second loopback auth client access should reuse the cached client"
    );
}

/// 函数 `exchange_code_for_tokens_matches_official_login_server_headers`
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
fn exchange_code_for_tokens_matches_official_login_server_headers() {
    let client = Client::builder().no_proxy().build().expect("build client");
    let request = build_exchange_code_request(
        &client,
        "http://127.0.0.1:1455",
        "client-test",
        "http://localhost:1455/auth/callback",
        "verifier-test",
        "code-test",
    )
    .expect("build exchange request");

    let find = |name: &str| {
        request
            .headers()
            .get(name)
            .and_then(|value| value.to_str().ok())
    };
    let body = request
        .body()
        .and_then(|body| body.as_bytes())
        .map(|body| String::from_utf8_lossy(body).into_owned())
        .expect("request body");

    assert_eq!(request.url().path(), "/oauth/token");
    assert_eq!(
        find("Content-Type"),
        Some("application/x-www-form-urlencoded")
    );
    assert_eq!(find("Originator"), None);
    assert_eq!(find("x-openai-internal-codex-residency"), None);
    assert_eq!(find("User-Agent"), None);
    assert!(body.contains("grant_type=authorization_code"));
    assert!(body.contains("code=code-test"));
    assert!(body.contains("code_verifier=verifier-test"));
}

/// 函数 `obtain_api_key_matches_official_login_server_headers`
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
fn obtain_api_key_matches_official_login_server_headers() {
    let client = Client::builder().no_proxy().build().expect("build client");
    let request = build_api_key_exchange_request(
        &client,
        "http://127.0.0.1:1455",
        "client-test",
        "id-token-test",
    )
    .expect("build api key exchange request");

    let find = |name: &str| {
        request
            .headers()
            .get(name)
            .and_then(|value| value.to_str().ok())
    };
    let body = request
        .body()
        .and_then(|body| body.as_bytes())
        .map(|body| String::from_utf8_lossy(body).into_owned())
        .expect("request body");

    assert_eq!(request.url().path(), "/oauth/token");
    assert_eq!(
        find("Content-Type"),
        Some("application/x-www-form-urlencoded")
    );
    assert_eq!(find("Originator"), None);
    assert_eq!(find("x-openai-internal-codex-residency"), None);
    assert_eq!(find("User-Agent"), None);
    assert!(body.contains("grant_type=urn%3Aietf%3Aparams%3Aoauth%3Agrant-type%3Atoken-exchange"));
    assert!(body.contains("requested_token=openai-api-key"));
}
