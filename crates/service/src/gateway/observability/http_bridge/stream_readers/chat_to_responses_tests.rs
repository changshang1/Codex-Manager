use serde_json::{json, Value};
use std::collections::BTreeSet;
use std::io::Read;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use super::{PassthroughSseCollector, ResponsesFromChatCompletionsSseReader};

fn parse_events(body: &[u8]) -> Vec<(String, Value)> {
    let text = String::from_utf8_lossy(body);
    let mut events = Vec::new();
    let mut current_event = String::new();
    let mut current_data = String::new();
    for line in text.lines() {
        if line.trim().is_empty() {
            if !current_data.is_empty() {
                events.push((
                    current_event.clone(),
                    serde_json::from_str(current_data.trim()).unwrap_or(Value::Null),
                ));
            }
            current_event.clear();
            current_data.clear();
            continue;
        }
        if let Some(rest) = line.strip_prefix("event: ") {
            current_event = rest.to_string();
        } else if let Some(rest) = line.strip_prefix("data: ") {
            current_data.push_str(rest);
        }
    }
    if !current_data.is_empty() {
        events.push((
            current_event.clone(),
            serde_json::from_str(current_data.trim()).unwrap_or(Value::Null),
        ));
    }
    events
}

fn collect_reader(reader: &mut dyn Read) -> Vec<u8> {
    let mut out = Vec::new();
    let mut buf = [0u8; 4096];
    loop {
        match reader.read(&mut buf) {
            Ok(0) | Err(_) => break,
            Ok(n) => out.extend_from_slice(&buf[..n]),
        }
    }
    out
}

fn chat_chunk(value: Value) -> Vec<u8> {
    format!("data: {}\n\n", serde_json::to_string(&value).unwrap()).into_bytes()
}

/// 断言事件序列中 type 的顺序与内容。
fn assert_event_types(events: &[(String, Value)], expected: &[&str]) {
    let actual: Vec<&str> = events
        .iter()
        .filter_map(|(_, value)| value.get("type").and_then(Value::as_str))
        .collect();
    assert_eq!(actual, expected, "事件类型顺序不匹配");
}

#[test]
fn stream_text_event_order_and_final_snapshot() {
    let stream: Vec<u8> = [
        chat_chunk(json!({
            "id": "chatcmpl-1",
            "object": "chat.completion.chunk",
            "created": 100,
            "model": "deepseek-v4-flash",
            "choices": [{ "index": 0, "delta": { "content": "你好" }, "finish_reason": null }],
        })),
        chat_chunk(json!({
            "id": "chatcmpl-1",
            "object": "chat.completion.chunk",
            "choices": [{ "index": 0, "delta": { "content": "世界" }, "finish_reason": null }],
        })),
        chat_chunk(json!({
            "id": "chatcmpl-1",
            "object": "chat.completion.chunk",
            "choices": [{ "index": 0, "delta": {}, "finish_reason": "stop" }],
            "usage": { "prompt_tokens": 10, "completion_tokens": 5, "total_tokens": 15 },
        })),
        b"data: [DONE]\n\n".to_vec(),
    ]
    .concat();
    let cursor = std::io::Cursor::new(stream);
    let collector = Arc::new(Mutex::new(PassthroughSseCollector::default()));
    let mut reader = ResponsesFromChatCompletionsSseReader::from_reader(
        cursor,
        Arc::clone(&collector),
        BTreeSet::new(),
        Instant::now(),
    );
    let body = collect_reader(&mut reader);
    let events = parse_events(&body);
    assert_event_types(
        &events,
        &[
            "response.created",
            "response.in_progress",
            "response.output_item.added",
            "response.content_part.added",
            "response.output_text.delta",
            "response.output_text.delta",
            "response.output_text.done",
            "response.content_part.done",
            "response.output_item.done",
            "response.completed",
        ],
    );

    let completed = events
        .iter()
        .find(|(_, value)| value.get("type").and_then(Value::as_str) == Some("response.completed"))
        .map(|(_, value)| value.clone())
        .unwrap();
    let response = completed.get("response").unwrap();
    assert_eq!(
        response.get("id").and_then(Value::as_str),
        Some("resp_chatcmpl-1")
    );
    assert_eq!(
        response.get("status").and_then(Value::as_str),
        Some("completed")
    );
    let output = response.get("output").and_then(Value::as_array).unwrap();
    assert_eq!(output.len(), 1);
    let message = &output[0];
    assert_eq!(message.get("type").and_then(Value::as_str), Some("message"));
    assert_eq!(
        message.get("content").and_then(Value::as_array).unwrap()[0]
            .get("text")
            .and_then(Value::as_str),
        Some("你好世界")
    );
    let usage = response.get("usage").unwrap();
    assert_eq!(usage.get("input_tokens").and_then(Value::as_i64), Some(10));
    assert_eq!(usage.get("output_tokens").and_then(Value::as_i64), Some(5));
    assert_eq!(usage.get("total_tokens").and_then(Value::as_i64), Some(15));

    let collector_guard = collector.lock().unwrap();
    assert!(collector_guard.saw_terminal);
    assert!(collector_guard.terminal_error.is_none());
}

#[test]
fn fragmented_function_arguments_and_parallel_calls() {
    let stream: Vec<u8> = [
        chat_chunk(json!({
            "id": "chatcmpl-p",
            "choices": [{ "index": 0, "delta": { "tool_calls": [{ "index": 0, "id": "call_a", "function": { "name": "get_weather", "arguments": "{\"city\":" } }] }, "finish_reason": null }],
        })),
        chat_chunk(json!({
            "id": "chatcmpl-p",
            "choices": [{ "index": 0, "delta": { "tool_calls": [{ "index": 0, "function": { "arguments": "\"北京\"}" } }] }, "finish_reason": null }],
        })),
        chat_chunk(json!({
            "id": "chatcmpl-p",
            "choices": [{ "index": 0, "delta": { "tool_calls": [{ "index": 1, "id": "call_b", "function": { "name": "get_time", "arguments": "{\"tz\":\"UTC\"}" } }] }, "finish_reason": null }],
        })),
        chat_chunk(json!({
            "id": "chatcmpl-p",
            "choices": [{ "index": 0, "delta": {}, "finish_reason": "tool_calls" }],
        })),
        b"data: [DONE]\n\n".to_vec(),
    ]
    .concat();
    let cursor = std::io::Cursor::new(stream);
    let collector = Arc::new(Mutex::new(PassthroughSseCollector::default()));
    let mut reader = ResponsesFromChatCompletionsSseReader::from_reader(
        cursor,
        Arc::clone(&collector),
        BTreeSet::new(),
        Instant::now(),
    );
    let body = collect_reader(&mut reader);
    let events = parse_events(&body);

    // 两个工具：每个有 output_item.added、function_call_arguments.delta、output_item.done。
    let added: Vec<&Value> = events
        .iter()
        .filter(|(_, value)| {
            value.get("type").and_then(Value::as_str) == Some("response.output_item.added")
        })
        .map(|(_, value)| value)
        .collect();
    assert_eq!(added.len(), 2, "应为两个工具的 output_item.added");
    // 输出索引独立且稳定（文本为空时从 0 开始）。
    let indexes: Vec<i64> = added
        .iter()
        .filter_map(|event| event.get("output_index").and_then(Value::as_i64))
        .collect();
    assert_eq!(indexes, vec![0, 1]);

    let deltas: Vec<&Value> = events
        .iter()
        .filter(|(_, value)| {
            value.get("type").and_then(Value::as_str)
                == Some("response.function_call_arguments.delta")
        })
        .map(|(_, value)| value)
        .collect();
    assert!(!deltas.is_empty());

    let completed = events
        .iter()
        .find(|(_, value)| value.get("type").and_then(Value::as_str) == Some("response.completed"))
        .map(|(_, value)| value.clone())
        .unwrap();
    let output = completed
        .get("response")
        .and_then(|r| r.get("output"))
        .and_then(Value::as_array)
        .unwrap();
    assert_eq!(output.len(), 2);
    let call_a = output
        .iter()
        .find(|item| item.get("name").and_then(Value::as_str) == Some("get_weather"))
        .unwrap();
    assert_eq!(
        call_a.get("call_id").and_then(Value::as_str),
        Some("call_a")
    );
    let arguments: Value =
        serde_json::from_str(call_a.get("arguments").and_then(Value::as_str).unwrap()).unwrap();
    assert_eq!(arguments.get("city").and_then(Value::as_str), Some("北京"));
    let call_b = output
        .iter()
        .find(|item| item.get("name").and_then(Value::as_str) == Some("get_time"))
        .unwrap();
    assert_eq!(
        call_b.get("call_id").and_then(Value::as_str),
        Some("call_b")
    );
}

#[test]
fn tool_identity_arriving_later_is_buffered() {
    // 首个 chunk 只有 arguments，没有 name/id；第二个 chunk 才提供 name/id。
    let stream: Vec<u8> = [
        chat_chunk(json!({
            "id": "chatcmpl-late",
            "choices": [{ "index": 0, "delta": { "tool_calls": [{ "index": 0, "function": { "arguments": "{\"x\":1}" } }] }, "finish_reason": null }],
        })),
        chat_chunk(json!({
            "id": "chatcmpl-late",
            "choices": [{ "index": 0, "delta": { "tool_calls": [{ "index": 0, "id": "call_late", "function": { "name": "tool_late" } }] }, "finish_reason": null }],
        })),
        b"data: [DONE]\n\n".to_vec(),
    ]
    .concat();
    let cursor = std::io::Cursor::new(stream);
    let collector = Arc::new(Mutex::new(PassthroughSseCollector::default()));
    let mut reader = ResponsesFromChatCompletionsSseReader::from_reader(
        cursor,
        Arc::clone(&collector),
        BTreeSet::new(),
        Instant::now(),
    );
    let body = collect_reader(&mut reader);
    let events = parse_events(&body);
    // 身份齐备前不发射任何 tool item；完成后恰好一个 tool item。
    let added: Vec<&Value> = events
        .iter()
        .filter(|(_, value)| {
            value.get("type").and_then(Value::as_str) == Some("response.output_item.added")
        })
        .map(|(_, value)| value)
        .collect();
    assert_eq!(added.len(), 1);
    let completed = events
        .iter()
        .find(|(_, value)| value.get("type").and_then(Value::as_str) == Some("response.completed"))
        .unwrap();
    let output = completed
        .1
        .get("response")
        .and_then(|r| r.get("output"))
        .and_then(Value::as_array)
        .unwrap();
    assert_eq!(output.len(), 1);
    assert_eq!(
        output[0].get("name").and_then(Value::as_str),
        Some("tool_late")
    );
    assert_eq!(
        output[0].get("call_id").and_then(Value::as_str),
        Some("call_late")
    );
}

#[test]
fn custom_tool_events_and_exact_patch_roundtrip() {
    let patch = "*** Begin Patch\n*** Update File: src/a.rs\n@@\n- old\n+ new\n*** End Patch";
    let full_arguments = format!("{{\"input\":{}}}", serde_json::to_string(patch).unwrap());
    let stream: Vec<u8> = [
        chat_chunk(json!({
            "id": "chatcmpl-custom",
            "choices": [{ "index": 0, "delta": { "tool_calls": [{ "index": 0, "id": "call_patch", "function": { "name": "apply_patch", "arguments": &full_arguments[..full_arguments.len() / 2] } }] }, "finish_reason": null }],
        })),
        chat_chunk(json!({
            "id": "chatcmpl-custom",
            "choices": [{ "index": 0, "delta": { "tool_calls": [{ "index": 0, "function": { "arguments": &full_arguments[full_arguments.len() / 2..] } }] }, "finish_reason": null }],
        })),
        chat_chunk(json!({
            "id": "chatcmpl-custom",
            "choices": [{ "index": 0, "delta": {}, "finish_reason": "tool_calls" }],
        })),
        b"data: [DONE]\n\n".to_vec(),
    ]
    .concat();
    let mut custom_tool_names = BTreeSet::new();
    custom_tool_names.insert("apply_patch".to_string());
    let cursor = std::io::Cursor::new(stream);
    let collector = Arc::new(Mutex::new(PassthroughSseCollector::default()));
    let mut reader = ResponsesFromChatCompletionsSseReader::from_reader(
        cursor,
        Arc::clone(&collector),
        custom_tool_names,
        Instant::now(),
    );
    let body = collect_reader(&mut reader);
    let events = parse_events(&body);

    let custom_deltas = events
        .iter()
        .filter(|(_, value)| {
            value.get("type").and_then(Value::as_str)
                == Some("response.custom_tool_call_input.delta")
        })
        .filter_map(|(_, value)| value.get("delta").and_then(Value::as_str))
        .collect::<String>();
    assert_eq!(custom_deltas, patch, "custom tool delta 必须还原原始 input");

    let completed = events
        .iter()
        .find(|(_, value)| value.get("type").and_then(Value::as_str) == Some("response.completed"))
        .map(|(_, value)| value.clone())
        .unwrap();
    let output = completed
        .get("response")
        .and_then(|r| r.get("output"))
        .and_then(Value::as_array)
        .unwrap();
    let item = &output[0];
    assert_eq!(
        item.get("type").and_then(Value::as_str),
        Some("custom_tool_call")
    );
    assert_eq!(
        item.get("name").and_then(Value::as_str),
        Some("apply_patch")
    );
    // 新 reader：custom tool 的 done item 将 input 从 {"input": raw} 解包到 `input` 字段。
    let input = item.get("input").and_then(Value::as_str).unwrap();
    assert_eq!(input, patch);
}

#[test]
fn missing_done_does_not_synthesize_completed() {
    // 流在无 [DONE] 的情况下 EOF（截断）。
    let stream: Vec<u8> = chat_chunk(json!({
        "id": "chatcmpl-trunc",
        "choices": [{ "index": 0, "delta": { "content": "半截" }, "finish_reason": null }],
    }));
    let cursor = std::io::Cursor::new(stream);
    let collector = Arc::new(Mutex::new(PassthroughSseCollector::default()));
    let mut reader = ResponsesFromChatCompletionsSseReader::from_reader(
        cursor,
        Arc::clone(&collector),
        BTreeSet::new(),
        Instant::now(),
    );
    let body = collect_reader(&mut reader);
    let events = parse_events(&body);
    let completed = events
        .iter()
        .any(|(_, value)| value.get("type").and_then(Value::as_str) == Some("response.completed"));
    assert!(!completed, "截断流不得合成 response.completed");
    // 截断必须发射 failed 网关事件而不是成功的 completed。
    let failed = events
        .iter()
        .any(|(_, value)| value.get("type").and_then(Value::as_str) == Some("response.failed"));
    assert!(failed, "截断流应发射 response.failed");
    let collector_guard = collector.lock().unwrap();
    assert!(collector_guard.saw_terminal);
    assert!(collector_guard.terminal_error.is_some());
}

#[test]
fn tool_call_without_stable_identity_fails_instead_of_being_dropped() {
    let stream: Vec<u8> = [
        chat_chunk(json!({
            "id": "chatcmpl-missing-tool-id",
            "choices": [{
                "index": 0,
                "delta": {
                    "tool_calls": [{
                        "index": 0,
                        "function": { "name": "read_file", "arguments": "{}" }
                    }]
                },
                "finish_reason": "tool_calls"
            }]
        })),
        b"data: [DONE]\n\n".to_vec(),
    ]
    .concat();
    let cursor = std::io::Cursor::new(stream);
    let collector = Arc::new(Mutex::new(PassthroughSseCollector::default()));
    let mut reader = ResponsesFromChatCompletionsSseReader::from_reader(
        cursor,
        Arc::clone(&collector),
        BTreeSet::new(),
        Instant::now(),
    );
    let mut output = String::new();
    reader.read_to_string(&mut output).unwrap();

    assert!(output.contains("event: response.failed"));
    assert!(!output.contains("event: response.completed"));
    assert!(collector
        .lock()
        .unwrap()
        .terminal_error
        .as_deref()
        .is_some_and(|message| message.contains("missing id or function name")));
}

#[test]
fn usage_merges_cached_and_reasoning_details() {
    let stream: Vec<u8> = [
        chat_chunk(json!({
            "id": "chatcmpl-u",
            "choices": [{ "index": 0, "delta": { "content": "ok" }, "finish_reason": "stop" }],
            "usage": {
                "prompt_tokens": 100,
                "completion_tokens": 20,
                "total_tokens": 120,
                "prompt_tokens_details": { "cached_tokens": 60, "cache_write_tokens": 10 },
                "completion_tokens_details": { "reasoning_tokens": 8 },
            },
        })),
        b"data: [DONE]\n\n".to_vec(),
    ]
    .concat();
    let cursor = std::io::Cursor::new(stream);
    let collector = Arc::new(Mutex::new(PassthroughSseCollector::default()));
    let mut reader = ResponsesFromChatCompletionsSseReader::from_reader(
        cursor,
        Arc::clone(&collector),
        BTreeSet::new(),
        Instant::now(),
    );
    let body = collect_reader(&mut reader);
    let events = parse_events(&body);
    let completed = events
        .iter()
        .find(|(_, value)| value.get("type").and_then(Value::as_str) == Some("response.completed"))
        .unwrap();
    let usage = completed
        .1
        .get("response")
        .and_then(|r| r.get("usage"))
        .unwrap();
    assert_eq!(usage.get("input_tokens").and_then(Value::as_i64), Some(100));
    assert_eq!(usage.get("output_tokens").and_then(Value::as_i64), Some(20));
    assert_eq!(usage.get("total_tokens").and_then(Value::as_i64), Some(120));
    assert_eq!(
        usage
            .get("input_tokens_details")
            .and_then(|d| d.get("cached_tokens"))
            .and_then(Value::as_i64),
        Some(60)
    );
    assert_eq!(
        usage
            .get("output_tokens_details")
            .and_then(|d| d.get("reasoning_tokens"))
            .and_then(Value::as_i64),
        Some(8)
    );
}

#[test]
fn no_text_or_tools_emits_minimal_completed() {
    let stream: Vec<u8> = [
        chat_chunk(json!({
            "id": "chatcmpl-empty",
            "choices": [{ "index": 0, "delta": {}, "finish_reason": "stop" }],
        })),
        b"data: [DONE]\n\n".to_vec(),
    ]
    .concat();
    let cursor = std::io::Cursor::new(stream);
    let collector = Arc::new(Mutex::new(PassthroughSseCollector::default()));
    let mut reader = ResponsesFromChatCompletionsSseReader::from_reader(
        cursor,
        Arc::clone(&collector),
        BTreeSet::new(),
        Instant::now(),
    );
    let body = collect_reader(&mut reader);
    let events = parse_events(&body);
    let completed = events
        .iter()
        .find(|(_, value)| value.get("type").and_then(Value::as_str) == Some("response.completed"))
        .map(|(_, value)| value.clone())
        .unwrap();
    let response = completed.get("response").unwrap();
    let output = response.get("output").and_then(Value::as_array).unwrap();
    assert!(output.is_empty());
    assert_eq!(
        response.get("id").and_then(Value::as_str),
        Some("resp_chatcmpl-empty")
    );
}

#[test]
fn cumulative_function_argument_snapshots_are_not_duplicated() {
    let stream: Vec<u8> = [
        chat_chunk(json!({
            "id": "chatcmpl-cumulative",
            "choices": [{ "index": 0, "delta": { "tool_calls": [{
                "index": 0,
                "id": "call_cumulative",
                "function": { "name": "lookup", "arguments": "{\"city\":" }
            }] }, "finish_reason": null }]
        })),
        chat_chunk(json!({
            "id": "chatcmpl-cumulative",
            "choices": [{ "index": 0, "delta": { "tool_calls": [{
                "index": 0,
                "function": { "arguments": "{\"city\":\"北京\"}" }
            }] }, "finish_reason": "tool_calls" }]
        })),
        b"data: [DONE]\n\n".to_vec(),
    ]
    .concat();
    let collector = Arc::new(Mutex::new(PassthroughSseCollector::default()));
    let mut reader = ResponsesFromChatCompletionsSseReader::from_reader(
        std::io::Cursor::new(stream),
        Arc::clone(&collector),
        BTreeSet::new(),
        Instant::now(),
    );
    let events = parse_events(&collect_reader(&mut reader));
    let completed = events
        .iter()
        .find(|(_, value)| value.get("type").and_then(Value::as_str) == Some("response.completed"))
        .unwrap();
    assert_eq!(
        completed.1["response"]["output"][0]["arguments"],
        "{\"city\":\"北京\"}"
    );
}

#[test]
fn reasoning_is_a_separate_output_item_from_answer_text() {
    let stream: Vec<u8> = [
        chat_chunk(json!({
            "id": "chatcmpl-reasoning",
            "choices": [{ "index": 0, "delta": { "reasoning_content": "先分析" }, "finish_reason": null }]
        })),
        chat_chunk(json!({
            "id": "chatcmpl-reasoning",
            "choices": [{ "index": 0, "delta": { "content": "最终答案" }, "finish_reason": "stop" }]
        })),
        b"data: [DONE]\n\n".to_vec(),
    ]
    .concat();
    let collector = Arc::new(Mutex::new(PassthroughSseCollector::default()));
    let mut reader = ResponsesFromChatCompletionsSseReader::from_reader(
        std::io::Cursor::new(stream),
        Arc::clone(&collector),
        BTreeSet::new(),
        Instant::now(),
    );
    let events = parse_events(&collect_reader(&mut reader));
    assert!(events.iter().any(|(_, value)| {
        value.get("type").and_then(Value::as_str) == Some("response.reasoning_summary_text.delta")
    }));
    let completed = events
        .iter()
        .find(|(_, value)| value.get("type").and_then(Value::as_str) == Some("response.completed"))
        .unwrap();
    let output = completed.1["response"]["output"].as_array().unwrap();
    assert_eq!(output[0]["type"], "reasoning");
    assert_eq!(output[0]["summary"][0]["text"], "先分析");
    assert_eq!(output[1]["type"], "message");
    assert_eq!(output[1]["content"][0]["text"], "最终答案");
}

#[test]
fn malformed_custom_wrapper_emits_failed_not_completed() {
    let stream: Vec<u8> = [
        chat_chunk(json!({
            "id": "chatcmpl-bad-custom",
            "choices": [{ "index": 0, "delta": { "tool_calls": [{
                "index": 0,
                "id": "call_patch",
                "function": { "name": "apply_patch", "arguments": "{\"wrong\":true}" }
            }] }, "finish_reason": "tool_calls" }]
        })),
        b"data: [DONE]\n\n".to_vec(),
    ]
    .concat();
    let custom_names: BTreeSet<String> = ["apply_patch".to_string()].into_iter().collect();
    let collector = Arc::new(Mutex::new(PassthroughSseCollector::default()));
    let mut reader = ResponsesFromChatCompletionsSseReader::from_reader(
        std::io::Cursor::new(stream),
        Arc::clone(&collector),
        custom_names,
        Instant::now(),
    );
    let events = parse_events(&collect_reader(&mut reader));
    assert!(events.iter().any(|(_, value)| {
        value.get("type").and_then(Value::as_str) == Some("response.failed")
    }));
    assert!(!events.iter().any(|(_, value)| {
        value.get("type").and_then(Value::as_str) == Some("response.completed")
    }));
    assert!(collector.lock().unwrap().terminal_error.is_some());
}

#[test]
fn upstream_sse_error_emits_failed_not_completed() {
    let stream = chat_chunk(json!({ "error": { "message": "quota exceeded" } }));
    let collector = Arc::new(Mutex::new(PassthroughSseCollector::default()));
    let mut reader = ResponsesFromChatCompletionsSseReader::from_reader(
        std::io::Cursor::new(stream),
        Arc::clone(&collector),
        BTreeSet::new(),
        Instant::now(),
    );
    let events = parse_events(&collect_reader(&mut reader));
    assert!(events.iter().any(|(_, value)| {
        value.get("type").and_then(Value::as_str) == Some("response.failed")
    }));
    assert!(!events.iter().any(|(_, value)| {
        value.get("type").and_then(Value::as_str) == Some("response.completed")
    }));
    assert!(collector
        .lock()
        .unwrap()
        .terminal_error
        .as_deref()
        .is_some_and(|message| message.contains("quota exceeded")));
}
