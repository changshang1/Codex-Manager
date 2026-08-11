use serde_json::{json, Map, Value};
use std::collections::BTreeSet;

use super::ResponseAdapter;

/// Responses→Chat Completions 桥接的请求转换结果。
///
/// 持有转换后的 Chat Completions JSON 请求体、请求声明过的 custom tool 名集合、
/// 下游响应适配器（固定为 `ResponseAdapter::ResponsesFromChatCompletions`）以及
/// 有效上游路径（调用方根据候选 action 决定，本模块不做路径猜测）。
#[derive(Debug)]
pub(crate) struct ResponsesToChatCompletionsBridge {
    /// 转换后的 Chat Completions JSON 请求体
    pub(crate) body: Vec<u8>,
    /// 请求声明（custom tool 声明或 custom_tool_call 历史）的 custom tool 名
    pub(crate) custom_tool_names: BTreeSet<String>,
    /// 下游响应适配器（SPEC 契约字段；生产代码经函数返回类型推断，测试断言其值）
    #[allow(dead_code)]
    pub(crate) response_adapter: ResponseAdapter,
    /// 有效上游路径（默认 /v1/chat/completions，可被调用方覆盖）
    pub(crate) chat_path: String,
}

/// 将 Responses API 请求转换为 Chat Completions 请求。
///
/// 拒绝无法通过 Chat Completions 表达的状态性/托管字段（如 `previous_response_id`、
/// `conversation`、hosted-only 工具、无 Chat 等价的 input item），调用方应当将错误
/// 转成结构化 400 响应。
///
/// # 参数
/// - body: 原始 Responses 请求体（JSON）
/// - model_override: 候选渠道的模型覆盖值，非空时优先使用
/// - default_path: 调用方给定的有效上游路径（如 /v1/chat/completions）
pub(crate) fn adapt_responses_request_to_chat_completions(
    body: &[u8],
    model_override: Option<&str>,
    default_path: &str,
) -> Result<ResponsesToChatCompletionsBridge, String> {
    let payload = serde_json::from_slice::<Value>(body)
        .map_err(|err| format!("invalid responses request json: {err}"))?;
    let obj = payload
        .as_object()
        .ok_or_else(|| "responses request body must be an object".to_string())?;

    let mut custom_tool_names = BTreeSet::new();
    let mut rewritten = Map::new();

    // 模型映射：model_override 优先，其次请求中的 model；都没有则不写 model 字段。
    let model = model_override.and_then(normalize_text).or_else(|| {
        obj.get("model")
            .and_then(Value::as_str)
            .and_then(normalize_text)
    });
    if let Some(model) = model {
        rewritten.insert("model".to_string(), Value::String(model));
    }

    // instructions → 开头的 system 消息
    let mut messages = Vec::new();
    if let Some(instructions) = obj
        .get("instructions")
        .and_then(Value::as_str)
        .and_then(normalize_text)
    {
        messages.push(chat_system_message(&instructions));
    }

    // input → Chat messages（string 或 item 数组）
    responses_input_to_chat_messages(obj.get("input"), &mut messages, &mut custom_tool_names)?;
    if messages.is_empty() {
        messages.push(json!({ "role": "user", "content": "" }));
    }
    rewritten.insert("messages".to_string(), Value::Array(messages));

    // tools → Chat function tools
    if let Some(tools) = obj.get("tools") {
        rewritten.insert(
            "tools".to_string(),
            responses_tools_to_chat(tools, &mut custom_tool_names)?,
        );
        if let Some(tool_choice) = obj.get("tool_choice") {
            rewritten.insert(
                "tool_choice".to_string(),
                responses_tool_choice_to_chat(tool_choice)?,
            );
        }
    }

    // parallel_tool_calls 同名字段透传
    if let Some(value) = obj.get("parallel_tool_calls") {
        rewritten.insert("parallel_tool_calls".to_string(), value.clone());
    }

    // reasoning.effort → reasoning_effort（effort 为 none 时不写）
    if let Some(effort) = responses_reasoning_effort(obj.get("reasoning")) {
        rewritten.insert("reasoning_effort".to_string(), Value::String(effort));
    }

    // max_output_tokens → max_completion_tokens
    if let Some(max_output_tokens) = obj.get("max_output_tokens").and_then(Value::as_i64) {
        rewritten.insert(
            "max_completion_tokens".to_string(),
            Value::from(max_output_tokens),
        );
    }

    // stream 透传；两个协议的缺省值均为 false。流式 Chat 必须显式请求 usage，
    // 否则多数兼容上游不会在最终 chunk 返回 token 统计。
    let is_stream = obj.get("stream").and_then(Value::as_bool).unwrap_or(false);
    rewritten.insert("stream".to_string(), Value::Bool(is_stream));
    if is_stream {
        rewritten.insert(
            "stream_options".to_string(),
            json!({ "include_usage": true }),
        );
    }

    for field in ["temperature", "top_p"] {
        if let Some(value) = obj.get(field) {
            rewritten.insert(field.to_string(), value.clone());
        }
    }

    // 拒绝状态性/无法映射的字段
    reject_stateful_fields(obj)?;

    Ok(ResponsesToChatCompletionsBridge {
        body: serde_json::to_vec(&Value::Object(rewritten))
            .map_err(|err| format!("serialize chat completions request failed: {err}"))?,
        custom_tool_names,
        response_adapter: ResponseAdapter::ResponsesFromChatCompletions,
        chat_path: default_path.to_string(),
    })
}

fn chat_system_message(text: &str) -> Value {
    json!({ "role": "system", "content": text })
}

fn normalize_text(value: &str) -> Option<String> {
    let value = value.trim();
    (!value.is_empty()).then(|| value.to_string())
}

/// 将 Responses input（string 或 item 数组）转换为 Chat messages。
fn responses_input_to_chat_messages(
    input: Option<&Value>,
    messages: &mut Vec<Value>,
    custom_tool_names: &mut BTreeSet<String>,
) -> Result<(), String> {
    match input {
        Some(Value::String(text)) => {
            messages.push(json!({ "role": "user", "content": text }));
        }
        Some(Value::Array(items)) => {
            for item in items {
                responses_input_item_to_chat(item, messages, custom_tool_names)?;
            }
        }
        Some(item @ Value::Object(_)) => {
            responses_input_item_to_chat(item, messages, custom_tool_names)?;
        }
        Some(Value::Null) | None => {}
        Some(other) => {
            return Err(format!(
                "responses input must be a string, object, or array, got {other}"
            ));
        }
    }
    Ok(())
}

fn responses_input_item_to_chat(
    item: &Value,
    messages: &mut Vec<Value>,
    custom_tool_names: &mut BTreeSet<String>,
) -> Result<(), String> {
    let obj = item
        .as_object()
        .ok_or_else(|| "responses input array items must be objects".to_string())?;
    match obj.get("type").and_then(Value::as_str).unwrap_or("message") {
        "message" => {
            let role = obj
                .get("role")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .unwrap_or("user");
            let chat_role = match role {
                "developer" | "system" => "system",
                "assistant" => "assistant",
                "user" => "user",
                other => {
                    return Err(format!(
                        "responses message role '{other}' has no chat completions equivalent"
                    ));
                }
            };
            let content = responses_message_content_to_chat(obj.get("content"))?;
            messages.push(json!({
                "role": chat_role,
                "content": content,
            }));
        }
        "function_call" => {
            let call_id = obj
                .get("call_id")
                .or_else(|| obj.get("id"))
                .and_then(Value::as_str)
                .and_then(normalize_text)
                .ok_or_else(|| "function_call input item missing call_id".to_string())?;
            let name = obj
                .get("name")
                .and_then(Value::as_str)
                .and_then(normalize_text)
                .ok_or_else(|| "function_call input item missing name".to_string())?;
            let arguments = serialize_function_arguments(obj.get("arguments"));
            push_assistant_tool_call(messages, call_id, name, arguments);
        }
        "custom_tool_call" => {
            let call_id = obj
                .get("call_id")
                .or_else(|| obj.get("id"))
                .and_then(Value::as_str)
                .and_then(normalize_text)
                .ok_or_else(|| "custom_tool_call input item missing call_id".to_string())?;
            let name = obj
                .get("name")
                .and_then(Value::as_str)
                .and_then(normalize_text)
                .ok_or_else(|| "custom_tool_call input item missing name".to_string())?;
            custom_tool_names.insert(name.clone());
            // custom tool 的 input 必须是原始字符串，再包装为 Chat function 参数。
            let raw_input = obj
                .get("input")
                .and_then(Value::as_str)
                .ok_or_else(|| "custom_tool_call input must be a string".to_string())?;
            let arguments = serde_json::to_string(&json!({ "input": raw_input }))
                .unwrap_or_else(|_| "{}".to_string());
            push_assistant_tool_call(messages, call_id, name, arguments);
        }
        "function_call_output" | "custom_tool_call_output" => {
            let call_id = obj
                .get("call_id")
                .or_else(|| obj.get("id"))
                .and_then(Value::as_str)
                .and_then(normalize_text)
                .ok_or_else(|| "tool call output input item missing call_id".to_string())?;
            let output = match obj.get("output") {
                Some(Value::String(text)) => text.clone(),
                Some(other) => other.to_string(),
                None => String::new(),
            };
            messages.push(json!({
                "role": "tool",
                "tool_call_id": call_id,
                "content": output,
            }));
        }
        other => {
            return Err(format!(
                "responses input item type '{other}' has no chat completions equivalent"
            ));
        }
    }
    Ok(())
}

fn push_assistant_tool_call(
    messages: &mut Vec<Value>,
    call_id: String,
    name: String,
    arguments: String,
) {
    let tool_call = json!({
        "id": call_id,
        "type": "function",
        "function": { "name": name, "arguments": arguments },
    });
    if let Some(last_assistant) = messages.last_mut().and_then(Value::as_object_mut) {
        if last_assistant.get("role").and_then(Value::as_str) == Some("assistant") {
            let tool_calls = last_assistant
                .entry("tool_calls".to_string())
                .or_insert_with(|| Value::Array(Vec::new()));
            if let Some(tool_calls) = tool_calls.as_array_mut() {
                tool_calls.push(tool_call);
                return;
            }
        }
    }
    messages.push(json!({
        "role": "assistant",
        "content": Value::Null,
        "tool_calls": [tool_call],
    }));
}

/// 将 Responses 消息内容转为 Chat content 字符串。
///
/// content 为字符串时原样；为数组时依次取出 `input_text`/`output_text`/`text`
/// 片段并用空行连接；为空时保留空串。**image 等无 Chat 文本等价的内容部分拒绝**，
/// 避免静默丢弃用户输入。
fn responses_message_content_to_chat(content: Option<&Value>) -> Result<String, String> {
    match content {
        Some(Value::String(text)) => Ok(text.clone()),
        Some(Value::Array(parts)) => {
            let mut segments = Vec::new();
            for part in parts {
                let kind = part.get("type").and_then(Value::as_str).unwrap_or_default();
                match kind {
                    "input_text" | "output_text" | "text" => {
                        if let Some(text) = part
                            .get("text")
                            .and_then(Value::as_str)
                            .map(str::trim)
                            .filter(|value| !value.is_empty())
                        {
                            segments.push(text.to_string());
                        }
                    }
                    "input_image" | "image" => {
                        return Err(
                            "responses message content type 'input_image' has no chat completions equivalent"
                                .to_string(),
                        );
                    }
                    other => {
                        return Err(format!(
                            "responses message content type '{other}' has no chat completions equivalent"
                        ));
                    }
                }
            }
            Ok(segments.join("\n\n"))
        }
        Some(Value::Object(obj)) => {
            match obj.get("type").and_then(Value::as_str).unwrap_or("text") {
                "input_text" | "output_text" | "text" => {}
                kind => {
                    return Err(format!(
                        "responses message content type '{kind}' has no chat completions equivalent"
                    ));
                }
            }
            Ok(obj
                .get("text")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string())
        }
        Some(_) | None => Ok(String::new()),
    }
}

/// 序列化 function_call 的 arguments。
///
/// 已经是字符串 JSON 的原样透传；对象则序列化为 JSON 字符串；缺失则使用 `{}`。
fn serialize_function_arguments(arguments: Option<&Value>) -> String {
    match arguments {
        Some(Value::String(text)) => text.clone(),
        Some(Value::Object(_) | Value::Array(_)) => {
            serde_json::to_string(arguments.unwrap()).unwrap_or_else(|_| "{}".to_string())
        }
        Some(_) => arguments.unwrap().to_string(),
        None => "{}".to_string(),
    }
}

/// 将 Responses tools 转换为 Chat function tools。
///
/// function tool 原样包装；custom tool 包装为带 `input` 字符串参数的 function schema；
/// hosted-only 工具（web_search、computer_use、file_search 等）拒绝。
fn responses_tools_to_chat(
    tools: &Value,
    custom_tool_names: &mut BTreeSet<String>,
) -> Result<Value, String> {
    let items = tools
        .as_array()
        .ok_or_else(|| "responses tools must be an array".to_string())?;
    let mut out = Vec::new();
    for item in items {
        let tool = item
            .as_object()
            .ok_or_else(|| "responses tools entries must be objects".to_string())?;
        let kind = tool
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or("function");
        match kind {
            "function" => {
                let name = tool
                    .get("name")
                    .and_then(Value::as_str)
                    .and_then(normalize_text)
                    .ok_or_else(|| "responses function tool missing name".to_string())?;
                let mut function = Map::new();
                function.insert("name".to_string(), Value::String(name));
                if let Some(description) = tool
                    .get("description")
                    .and_then(Value::as_str)
                    .and_then(normalize_text)
                {
                    function.insert("description".to_string(), Value::String(description));
                }
                if let Some(parameters) = tool.get("parameters") {
                    function.insert("parameters".to_string(), parameters.clone());
                }
                if let Some(strict) = tool.get("strict") {
                    function.insert("strict".to_string(), strict.clone());
                }
                out.push(json!({ "type": "function", "function": Value::Object(function) }));
            }
            "custom" => {
                let name = tool
                    .get("name")
                    .and_then(Value::as_str)
                    .and_then(normalize_text)
                    .ok_or_else(|| "responses custom tool missing name".to_string())?;
                custom_tool_names.insert(name.clone());
                let mut function = Map::new();
                function.insert("name".to_string(), Value::String(name));
                if let Some(description) = tool
                    .get("description")
                    .and_then(Value::as_str)
                    .and_then(normalize_text)
                {
                    function.insert("description".to_string(), Value::String(description));
                }
                // 严格按 SPEC：custom tool 包装为带 input 字符串参数的 function schema
                function.insert(
                    "parameters".to_string(),
                    json!({
                        "type": "object",
                        "properties": { "input": { "type": "string" } },
                        "required": ["input"],
                        "additionalProperties": false,
                    }),
                );
                out.push(json!({ "type": "function", "function": Value::Object(function) }));
            }
            other => {
                return Err(format!(
                    "hosted-only responses tool type '{other}' cannot be bridged to chat completions"
                ));
            }
        }
    }
    Ok(Value::Array(out))
}

/// 将 Responses tool_choice 映射为 Chat 等价的 tool_choice。
fn responses_tool_choice_to_chat(tool_choice: &Value) -> Result<Value, String> {
    match tool_choice {
        Value::String(value) => {
            let mapped = match value.as_str() {
                "auto" | "required" | "none" => value.to_string(),
                "any" => "required".to_string(),
                other => {
                    return Err(format!(
                        "responses tool_choice string '{other}' cannot be bridged to chat completions"
                    ));
                }
            };
            Ok(Value::String(mapped))
        }
        Value::Object(obj) => {
            let name = obj
                .get("name")
                .and_then(Value::as_str)
                .or_else(|| {
                    obj.get("function")
                        .and_then(Value::as_object)
                        .and_then(|function| function.get("name"))
                        .and_then(Value::as_str)
                })
                .and_then(normalize_text)
                .ok_or_else(|| {
                    "responses tool_choice object requires a function name".to_string()
                })?;
            Ok(json!({ "type": "function", "function": { "name": name } }))
        }
        other => Err(format!("unsupported responses tool_choice value: {other}")),
    }
}

/// 从 reasoning 对象中提取 effort，映射为 reasoning_effort。
///
/// effort 为 none 时不写（不发送），无 effort 时省略。
fn responses_reasoning_effort(reasoning: Option<&Value>) -> Option<String> {
    let obj = reasoning?.as_object()?;
    let effort = obj
        .get("effort")
        .and_then(Value::as_str)
        .and_then(normalize_text)?;
    if effort.eq_ignore_ascii_case("none") {
        return None;
    }
    Some(effort.to_ascii_lowercase())
}

/// 拒绝无法通过 Chat Completions 表达的状态性字段，避免静默丢弃。
fn reject_stateful_fields(obj: &Map<String, Value>) -> Result<(), String> {
    if let Some(value) = obj.get("previous_response_id") {
        if value.as_str().is_some_and(|text| !text.trim().is_empty()) {
            return Err(
                "previous_response_id is not supported when bridging to chat completions"
                    .to_string(),
            );
        }
    }
    if let Some(value) = obj.get("conversation") {
        let has_content = match value {
            Value::Null => false,
            Value::String(text) => !text.trim().is_empty(),
            Value::Object(map) => !map.is_empty(),
            Value::Array(items) => !items.is_empty(),
            _ => true,
        };
        if has_content {
            return Err(
                "conversation is not supported when bridging to chat completions".to_string(),
            );
        }
    }
    Ok(())
}

// 供上游协议路由使用的公开类型别名（保持与现有 ToolNameRestoreMap 一致的可见性语义）。
#[cfg(test)]
#[path = "responses_to_chat_completions_tests.rs"]
mod tests;
