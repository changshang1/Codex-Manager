use serde_json::{json, Value};
use std::collections::BTreeSet;

use super::{adapt_responses_request_to_chat_completions, ResponsesToChatCompletionsBridge};

fn chat_body(bridge: &ResponsesToChatCompletionsBridge) -> Value {
    serde_json::from_slice::<Value>(bridge.body.as_slice()).unwrap()
}

fn assert_chat_message<'a>(messages: &'a [Value], index: usize, role: &str) -> &'a Value {
    let message = messages.get(index).unwrap();
    assert_eq!(message.get("role").and_then(Value::as_str), Some(role));
    message
}

#[test]
fn text_request_conversion_maps_instructions_model_and_reasoning() {
    let body = json!({
        "model": "deepseek-v4-flash",
        "instructions": "请用中文回答。",
        "input": "你好，介绍一下你自己",
        "reasoning": { "effort": "high" },
        "max_output_tokens": 2048,
        "stream": true,
    });
    let bridge = adapt_responses_request_to_chat_completions(
        serde_json::to_vec(&body).unwrap().as_slice(),
        None,
        "/v1/chat/completions",
    )
    .expect("conversion should succeed");
    let chat = chat_body(&bridge);

    assert_eq!(
        chat.get("model").and_then(Value::as_str),
        Some("deepseek-v4-flash")
    );
    assert_eq!(chat.get("stream").and_then(Value::as_bool), Some(true));
    assert_eq!(
        chat.get("reasoning_effort").and_then(Value::as_str),
        Some("high")
    );
    assert_eq!(
        chat.get("max_completion_tokens").and_then(Value::as_i64),
        Some(2048)
    );
    assert_eq!(
        chat.get("messages").and_then(Value::as_array).map(Vec::len),
        Some(2)
    );

    let messages = chat.get("messages").and_then(Value::as_array).unwrap();
    let system = assert_chat_message(messages, 0, "system");
    assert_eq!(
        system.get("content").and_then(Value::as_str),
        Some("请用中文回答。")
    );
    let user = assert_chat_message(messages, 1, "user");
    assert_eq!(
        user.get("content").and_then(Value::as_str),
        Some("你好，介绍一下你自己")
    );
}

#[test]
fn model_override_takes_precedence() {
    let body = json!({
        "model": "default-model",
        "input": "hi",
    });
    let bridge = adapt_responses_request_to_chat_completions(
        serde_json::to_vec(&body).unwrap().as_slice(),
        Some("overridden-model"),
        "/v1/chat/completions",
    )
    .expect("conversion should succeed");
    let chat = chat_body(&bridge);
    assert_eq!(
        chat.get("model").and_then(Value::as_str),
        Some("overridden-model")
    );
}

#[test]
fn string_input_becomes_user_message() {
    let body = json!({ "input": "plain text input" });
    let bridge = adapt_responses_request_to_chat_completions(
        serde_json::to_vec(&body).unwrap().as_slice(),
        None,
        "/v1/chat/completions",
    )
    .expect("conversion should succeed");
    let chat = chat_body(&bridge);
    let messages = chat.get("messages").and_then(Value::as_array).unwrap();
    assert_eq!(messages.len(), 1);
    assert_eq!(
        messages[0].get("role").and_then(Value::as_str),
        Some("user")
    );
    assert_eq!(
        messages[0].get("content").and_then(Value::as_str),
        Some("plain text input")
    );
}

#[test]
fn function_tool_declaration_and_history() {
    let body = json!({
        "tools": [{
            "type": "function",
            "name": "get_weather",
            "description": "查询天气",
            "parameters": { "type": "object", "properties": { "city": { "type": "string" } } },
        }],
        "input": [
            { "type": "function_call", "call_id": "call_1", "name": "get_weather", "arguments": { "city": "北京" } },
            { "type": "function_call_output", "call_id": "call_1", "output": "晴天 25°C" },
        ],
    });
    let bridge = adapt_responses_request_to_chat_completions(
        serde_json::to_vec(&body).unwrap().as_slice(),
        None,
        "/v1/chat/completions",
    )
    .expect("conversion should succeed");
    let chat = chat_body(&bridge);

    let tools = chat.get("tools").and_then(Value::as_array).unwrap();
    assert_eq!(tools.len(), 1);
    assert_eq!(
        tools[0].get("type").and_then(Value::as_str),
        Some("function")
    );
    let function = tools[0].get("function").unwrap();
    assert_eq!(
        function.get("name").and_then(Value::as_str),
        Some("get_weather")
    );
    assert_eq!(
        function.get("description").and_then(Value::as_str),
        Some("查询天气")
    );

    let messages = chat.get("messages").and_then(Value::as_array).unwrap();
    assert_eq!(messages.len(), 2);
    let assistant = assert_chat_message(messages, 0, "assistant");
    let tool_calls = assistant
        .get("tool_calls")
        .and_then(Value::as_array)
        .unwrap();
    assert_eq!(
        tool_calls[0].get("id").and_then(Value::as_str),
        Some("call_1")
    );
    let call_function = tool_calls[0].get("function").unwrap();
    assert_eq!(
        call_function.get("name").and_then(Value::as_str),
        Some("get_weather")
    );
    let arguments: Value = serde_json::from_str(
        call_function
            .get("arguments")
            .and_then(Value::as_str)
            .unwrap(),
    )
    .unwrap();
    assert_eq!(arguments.get("city").and_then(Value::as_str), Some("北京"));

    let tool = assert_chat_message(messages, 1, "tool");
    assert_eq!(
        tool.get("tool_call_id").and_then(Value::as_str),
        Some("call_1")
    );
    assert_eq!(
        tool.get("content").and_then(Value::as_str),
        Some("晴天 25°C")
    );
}

#[test]
fn custom_apply_patch_declaration_and_wrapping() {
    let body = json!({
        "tools": [{
            "type": "custom",
            "name": "apply_patch",
            "description": "应用代码补丁",
        }],
        "input": [
            {
                "type": "custom_tool_call",
                "call_id": "call_patch",
                "name": "apply_patch",
                "input": "*** Begin Patch\n*** Update File: src/main.rs",
            },
        ],
    });
    let bridge = adapt_responses_request_to_chat_completions(
        serde_json::to_vec(&body).unwrap().as_slice(),
        None,
        "/v1/chat/completions",
    )
    .expect("conversion should succeed");

    assert!(bridge.custom_tool_names.contains("apply_patch"));

    let chat = chat_body(&bridge);
    let tools = chat.get("tools").and_then(Value::as_array).unwrap();
    let function = tools[0].get("function").unwrap();
    let parameters = function.get("parameters").unwrap();
    assert_eq!(
        parameters.get("type").and_then(Value::as_str),
        Some("object")
    );
    assert_eq!(
        parameters
            .get("properties")
            .and_then(|p| p.get("input"))
            .and_then(|i| i.get("type"))
            .and_then(Value::as_str),
        Some("string")
    );
    assert_eq!(
        parameters
            .get("required")
            .and_then(Value::as_array)
            .map(|items| items.len()),
        Some(1)
    );

    let messages = chat.get("messages").and_then(Value::as_array).unwrap();
    let assistant = assert_chat_message(messages, 0, "assistant");
    let tool_calls = assistant
        .get("tool_calls")
        .and_then(Value::as_array)
        .unwrap();
    let arguments: Value = serde_json::from_str(
        tool_calls[0]
            .get("function")
            .and_then(|f| f.get("arguments"))
            .and_then(Value::as_str)
            .unwrap(),
    )
    .unwrap();
    let input = arguments.get("input").and_then(Value::as_str).unwrap();
    assert!(input.contains("*** Update File: src/main.rs"));
}

#[test]
fn custom_tool_call_output_becomes_tool_message() {
    let body = json!({
        "input": [
            {
                "type": "custom_tool_call",
                "call_id": "call_patch",
                "name": "apply_patch",
                "input": "patch",
            },
            {
                "type": "custom_tool_call_output",
                "call_id": "call_patch",
                "output": "Patch applied.",
            },
        ],
    });
    let bridge = adapt_responses_request_to_chat_completions(
        serde_json::to_vec(&body).unwrap().as_slice(),
        None,
        "/v1/chat/completions",
    )
    .expect("conversion should succeed");
    assert!(bridge.custom_tool_names.contains("apply_patch"));
    let chat = chat_body(&bridge);
    let messages = chat.get("messages").and_then(Value::as_array).unwrap();
    assert_eq!(messages.len(), 2);
    let tool = assert_chat_message(messages, 1, "tool");
    assert_eq!(
        tool.get("content").and_then(Value::as_str),
        Some("Patch applied.")
    );
}

#[test]
fn tool_choice_mapping() {
    for (source, expected) in [
        ("auto", "auto"),
        ("required", "required"),
        ("none", "none"),
        ("any", "required"),
    ] {
        let body = json!({
            "tools": [{ "type": "function", "name": "f", "parameters": {} }],
            "tool_choice": source,
            "input": "x",
        });
        let bridge = adapt_responses_request_to_chat_completions(
            serde_json::to_vec(&body).unwrap().as_slice(),
            None,
            "/v1/chat/completions",
        )
        .expect("conversion should succeed");
        let chat = chat_body(&bridge);
        assert_eq!(
            chat.get("tool_choice").and_then(Value::as_str),
            Some(expected),
            "tool_choice {source} should map to {expected}"
        );
    }

    let body = json!({
        "tools": [{ "type": "function", "name": "f", "parameters": {} }],
        "tool_choice": { "type": "function", "name": "f" },
        "input": "x",
    });
    let bridge = adapt_responses_request_to_chat_completions(
        serde_json::to_vec(&body).unwrap().as_slice(),
        None,
        "/v1/chat/completions",
    )
    .expect("conversion should succeed");
    let chat = chat_body(&bridge);
    let choice = chat.get("tool_choice").unwrap();
    assert_eq!(choice.get("type").and_then(Value::as_str), Some("function"));
    assert_eq!(
        choice
            .get("function")
            .and_then(|f| f.get("name"))
            .and_then(Value::as_str),
        Some("f")
    );
}

#[test]
fn parallel_tool_calls_passthrough() {
    let body = json!({
        "tools": [{ "type": "function", "name": "f", "parameters": {} }],
        "parallel_tool_calls": false,
        "input": "x",
    });
    let bridge = adapt_responses_request_to_chat_completions(
        serde_json::to_vec(&body).unwrap().as_slice(),
        None,
        "/v1/chat/completions",
    )
    .expect("conversion should succeed");
    let chat = chat_body(&bridge);
    assert_eq!(
        chat.get("parallel_tool_calls").and_then(Value::as_bool),
        Some(false)
    );
}

#[test]
fn default_stream_is_false_and_chat_path_returned() {
    let body = json!({ "input": "hi" });
    let bridge = adapt_responses_request_to_chat_completions(
        serde_json::to_vec(&body).unwrap().as_slice(),
        None,
        "/v1/chat/completions",
    )
    .expect("conversion should succeed");
    assert_eq!(
        chat_body(&bridge).get("stream").and_then(Value::as_bool),
        Some(false)
    );
    assert_eq!(bridge.chat_path, "/v1/chat/completions");
    assert_eq!(
        bridge.response_adapter,
        super::super::ResponseAdapter::ResponsesFromChatCompletions
    );
}

#[test]
fn streaming_requests_enable_usage_chunks() {
    let body = json!({ "input": "hi", "stream": true });
    let bridge = adapt_responses_request_to_chat_completions(
        serde_json::to_vec(&body).unwrap().as_slice(),
        None,
        "/v1/chat/completions",
    )
    .expect("conversion should succeed");
    let chat = chat_body(&bridge);
    assert_eq!(
        chat.get("stream_options")
            .and_then(|value| value.get("include_usage"))
            .and_then(Value::as_bool),
        Some(true)
    );
}

#[test]
fn consecutive_parallel_history_calls_share_one_assistant_message() {
    let body = json!({
        "input": [
            { "type": "function_call", "call_id": "call_a", "name": "a", "arguments": { "x": 1 } },
            { "type": "function_call", "call_id": "call_b", "name": "b", "arguments": { "y": 2 } },
            { "type": "function_call_output", "call_id": "call_a", "output": "A" },
            { "type": "function_call_output", "call_id": "call_b", "output": "B" }
        ]
    });
    let bridge = adapt_responses_request_to_chat_completions(
        serde_json::to_vec(&body).unwrap().as_slice(),
        None,
        "/v1/chat/completions",
    )
    .expect("conversion should succeed");
    let chat = chat_body(&bridge);
    let messages = chat.get("messages").and_then(Value::as_array).unwrap();
    assert_eq!(messages.len(), 3);
    assert_eq!(
        messages[0].get("role").and_then(Value::as_str),
        Some("assistant")
    );
    assert_eq!(
        messages[0]
            .get("tool_calls")
            .and_then(Value::as_array)
            .map(Vec::len),
        Some(2)
    );
    assert_eq!(
        messages[1].get("role").and_then(Value::as_str),
        Some("tool")
    );
    assert_eq!(
        messages[2].get("role").and_then(Value::as_str),
        Some("tool")
    );
}

#[test]
fn custom_tool_history_requires_string_input() {
    let body = json!({
        "input": [{
            "type": "custom_tool_call",
            "call_id": "call_patch",
            "name": "apply_patch",
            "input": { "unexpected": true }
        }]
    });
    let err = adapt_responses_request_to_chat_completions(
        serde_json::to_vec(&body).unwrap().as_slice(),
        None,
        "/v1/chat/completions",
    )
    .expect_err("non-string custom input must be rejected");
    assert!(err.contains("must be a string"));
}

#[test]
fn previous_response_id_is_rejected() {
    let body = json!({
        "previous_response_id": "resp_123",
        "input": "hi",
    });
    let err = adapt_responses_request_to_chat_completions(
        serde_json::to_vec(&body).unwrap().as_slice(),
        None,
        "/v1/chat/completions",
    )
    .expect_err("previous_response_id must be rejected");
    assert!(err.contains("previous_response_id"));
}

#[test]
fn hosted_tools_are_rejected() {
    for tool_type in ["web_search", "computer_use", "file_search"] {
        let body = json!({
            "tools": [{ "type": tool_type, "name": "x" }],
            "input": "hi",
        });
        let err = adapt_responses_request_to_chat_completions(
            serde_json::to_vec(&body).unwrap().as_slice(),
            None,
            "/v1/chat/completions",
        )
        .expect_err(&format!("{tool_type} must be rejected"));
        assert!(err.contains("hosted-only") || err.contains("cannot be bridged"));
    }
}

#[test]
fn input_item_without_chat_equivalent_is_rejected() {
    let body = json!({
        "input": [{
            "type": "message",
            "role": "user",
            "content": [{ "type": "input_image", "image_url": "data:image/png;base64,xxx" }],
        }],
    });
    let err = adapt_responses_request_to_chat_completions(
        serde_json::to_vec(&body).unwrap().as_slice(),
        None,
        "/v1/chat/completions",
    )
    .expect_err("input_image has no chat equivalent and must be rejected");
    assert!(err.contains("no chat completions equivalent"));
}

#[test]
fn conversation_is_rejected() {
    let body = json!({
        "conversation": "some-state",
        "input": "hi",
    });
    let err = adapt_responses_request_to_chat_completions(
        serde_json::to_vec(&body).unwrap().as_slice(),
        None,
        "/v1/chat/completions",
    )
    .expect_err("conversation must be rejected");
    assert!(err.contains("conversation"));
}

#[test]
fn developer_message_maps_to_system() {
    let body = json!({
        "input": [{
            "type": "message",
            "role": "developer",
            "content": [{ "type": "input_text", "text": "dev rules" }],
        }],
    });
    let bridge = adapt_responses_request_to_chat_completions(
        serde_json::to_vec(&body).unwrap().as_slice(),
        None,
        "/v1/chat/completions",
    )
    .expect("conversion should succeed");
    let chat = chat_body(&bridge);
    let messages = chat.get("messages").and_then(Value::as_array).unwrap();
    assert_eq!(
        messages[0].get("role").and_then(Value::as_str),
        Some("system")
    );
    assert_eq!(
        messages[0].get("content").and_then(Value::as_str),
        Some("dev rules")
    );
}

#[test]
fn custom_tool_names_set_collects_declarations_only() {
    let body = json!({
        "tools": [{ "type": "custom", "name": "apply_patch" }],
        "input": [
            { "type": "function_call", "call_id": "c1", "name": "get_weather", "arguments": {} },
        ],
    });
    let bridge = adapt_responses_request_to_chat_completions(
        serde_json::to_vec(&body).unwrap().as_slice(),
        None,
        "/v1/chat/completions",
    )
    .expect("conversion should succeed");
    let expected: BTreeSet<String> = ["apply_patch".to_string()].into_iter().collect();
    assert_eq!(bridge.custom_tool_names, expected);
}
