use super::{
    align_existing_prompt_cache_key_with_native_anchor, has_native_thread_anchor,
    resolve_fallback_thread_anchor, resolve_local_conversation_id_with_sticky_fallback,
};
use axum::http::{HeaderMap, HeaderValue};
use codexmanager_core::storage::ConversationBinding;
use serde_json::json;

fn sample_headers(
    conversation_id: Option<&str>,
    turn_state: Option<&str>,
    x_api_key: Option<&str>,
) -> crate::gateway::IncomingHeaderSnapshot {
    let mut headers = HeaderMap::new();
    if let Some(conversation_id) = conversation_id {
        headers.insert(
            "conversation_id",
            HeaderValue::from_str(conversation_id).expect("conversation header"),
        );
    }
    if let Some(turn_state) = turn_state {
        headers.insert(
            "x-codex-turn-state",
            HeaderValue::from_str(turn_state).expect("turn-state header"),
        );
    }
    if let Some(x_api_key) = x_api_key {
        headers.insert(
            "x-api-key",
            HeaderValue::from_str(x_api_key).expect("api key header"),
        );
    }
    crate::gateway::IncomingHeaderSnapshot::from_http_headers(&headers)
}

fn sample_binding() -> ConversationBinding {
    ConversationBinding {
        platform_key_hash: "hash".to_string(),
        conversation_id: "sticky-conversation".to_string(),
        account_id: "acc_1".to_string(),
        thread_epoch: 2,
        thread_anchor: "thread-anchor-2".to_string(),
        status: "active".to_string(),
        last_model: None,
        last_switch_reason: None,
        created_at: 1,
        updated_at: 1,
        last_used_at: 1,
    }
}

#[test]
fn native_thread_anchor_detects_turn_state_without_conversation_id() {
    let headers = sample_headers(None, Some("turn-state-1"), Some("pk_test"));

    assert!(has_native_thread_anchor(&headers));
}

#[test]
fn sticky_fallback_is_disabled_when_turn_state_exists() {
    let headers = sample_headers(None, Some("turn-state-1"), Some("pk_test"));

    let actual = resolve_local_conversation_id_with_sticky_fallback(&headers, true);

    assert_eq!(actual, None);
}

#[test]
fn fallback_thread_anchor_is_suppressed_when_native_anchor_exists() {
    let headers = sample_headers(Some("conversation-1"), None, Some("pk_test"));

    let actual =
        resolve_fallback_thread_anchor(&headers, Some("conversation-1"), Some(&sample_binding()));

    assert_eq!(actual, None);
}

#[test]
fn native_conversation_replaces_conflicting_prompt_cache_key() {
    let headers = sample_headers(Some("conversation-1"), None, Some("pk_test"));
    let body = serde_json::to_vec(&json!({
        "model": "gpt-5.4",
        "prompt_cache_key": "client-thread"
    }))
    .expect("serialize body");

    let actual = align_existing_prompt_cache_key_with_native_anchor(body, &headers);
    let payload: serde_json::Value = serde_json::from_slice(&actual).expect("parse body");

    assert_eq!(payload["prompt_cache_key"], "conversation-1");
}

#[test]
fn complete_session_turn_anchor_removes_conflicting_prompt_cache_key() {
    let mut headers = HeaderMap::new();
    headers.insert("session_id", HeaderValue::from_static("session-1"));
    headers.insert(
        "x-codex-turn-state",
        HeaderValue::from_static("turn-state-1"),
    );
    let headers = crate::gateway::IncomingHeaderSnapshot::from_http_headers(&headers);
    let body = serde_json::to_vec(&json!({
        "model": "gpt-5.4",
        "prompt_cache_key": "client-thread"
    }))
    .expect("serialize body");

    let actual = align_existing_prompt_cache_key_with_native_anchor(body, &headers);
    let payload: serde_json::Value = serde_json::from_slice(&actual).expect("parse body");

    assert!(payload.get("prompt_cache_key").is_none());
}
