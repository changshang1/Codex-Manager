use super::{
    append_output_text_raw, json, mark_first_response_ms, should_emit_keepalive_after_first_frame,
    stream_idle_timed_out, stream_wait_timeout, Arc, Cursor, Map, Mutex, PassthroughSseCollector,
    Read, SseKeepAliveFrame, UpstreamSseFramePump, UpstreamSseFramePumpItem, Value,
};
use std::collections::BTreeSet;
use std::time::Instant;

/// Chat Completions SSE → Responses SSE 流式桥接 reader。
///
/// 消费上游 Chat 流的 `chat.completion.chunk` 帧，按 Responses SSE 事件顺序
/// 发射 `response.created` / `response.in_progress` / `response.output_item.added` /
/// `response.content_part.added` / `response.output_text.delta` / `response.output_text.done` /
/// `response.function_call_arguments.delta` / `response.custom_tool_call_input.delta` 以及
/// `response.completed`（带密集 output 快照与 usage）。
pub(crate) struct ResponsesFromChatCompletionsSseReader {
    upstream: UpstreamSseFramePump,
    out_cursor: Cursor<Vec<u8>>,
    state: ChatToResponsesStreamState,
    usage_collector: Arc<Mutex<PassthroughSseCollector>>,
    request_started_at: Instant,
    last_upstream_activity: Instant,
    saw_upstream_frame: bool,
}

/// 请求级 Chat→Responses 流式状态机。
///
/// 持有响应 ID、序列号、output-index 分配、当前文本/推理 item、按 Chat index 与
/// call ID 键控的工具调用缓冲、custom tool 元数据、usage 与终止状态。
#[derive(Default)]
struct ChatToResponsesStreamState {
    response_id: Option<String>,
    model: Option<String>,
    created_at: i64,
    sequence: i64,
    started: bool,
    text_item_started: bool,
    text_part_started: bool,
    text_finished: bool,
    text_output_index: Option<usize>,
    reasoning_item_started: bool,
    reasoning_part_started: bool,
    reasoning_finished: bool,
    reasoning_output_index: Option<usize>,
    completed: bool,
    saw_done: bool,
    upstream_failed: bool,
    output_text: String,
    reasoning_text: String,
    next_output_index: usize,
    tools: Vec<PendingChatTool>,
    final_tools: Vec<(usize, Value)>,
    input_tokens: i64,
    cached_input_tokens: i64,
    cache_write_tokens: i64,
    output_tokens: i64,
    reasoning_output_tokens: i64,
    total_tokens: Option<i64>,
    stop_reason: String,
    custom_tool_names: BTreeSet<String>,
}

#[derive(Default)]
struct PendingChatTool {
    chat_index: usize,
    call_id: Option<String>,
    name: Option<String>,
    arguments: String,
    last_sent_arguments_len: usize,
    last_sent_custom_input: String,
    output_index: Option<usize>,
    emitted_added: bool,
    emitted_done: bool,
}

impl ResponsesFromChatCompletionsSseReader {
    pub(crate) fn from_reader<R>(
        upstream: R,
        usage_collector: Arc<Mutex<PassthroughSseCollector>>,
        custom_tool_names: BTreeSet<String>,
        request_started_at: Instant,
    ) -> Self
    where
        R: Read + Send + 'static,
    {
        let mut state = ChatToResponsesStreamState {
            stop_reason: "stop".to_string(),
            ..Default::default()
        };
        state.custom_tool_names = custom_tool_names;
        Self {
            upstream: UpstreamSseFramePump::from_reader(upstream),
            out_cursor: Cursor::new(Vec::new()),
            state,
            usage_collector,
            request_started_at,
            last_upstream_activity: Instant::now(),
            saw_upstream_frame: false,
        }
    }

    pub(crate) fn new(
        upstream: reqwest::blocking::Response,
        usage_collector: Arc<Mutex<PassthroughSseCollector>>,
        custom_tool_names: BTreeSet<String>,
        request_started_at: Instant,
    ) -> Self {
        Self::from_reader(
            upstream,
            usage_collector,
            custom_tool_names,
            request_started_at,
        )
    }

    fn next_chunk(&mut self) -> std::io::Result<Vec<u8>> {
        if self.state.completed {
            return Ok(Vec::new());
        }
        loop {
            match self
                .upstream
                .recv_timeout(stream_wait_timeout(self.last_upstream_activity))
            {
                Ok(UpstreamSseFramePumpItem::Frame(frame)) => {
                    self.last_upstream_activity = Instant::now();
                    self.saw_upstream_frame = true;
                    mark_first_response_ms(&self.usage_collector, self.request_started_at);
                    let mapped = self.process_sse_frame(&frame);
                    if !mapped.is_empty() {
                        return Ok(mapped);
                    }
                }
                Ok(UpstreamSseFramePumpItem::Error(err)) => {
                    self.state.upstream_failed = true;
                    return Ok(self.fail_stream(format!("上游流式读取失败: {err}")));
                }
                Ok(UpstreamSseFramePumpItem::Eof)
                | Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                    let finished = self.finish_stream();
                    if !finished.is_empty() {
                        mark_first_response_ms(&self.usage_collector, self.request_started_at);
                    }
                    return Ok(finished);
                }
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                    if stream_idle_timed_out(self.last_upstream_activity) {
                        self.state.upstream_failed = true;
                        return Ok(self.fail_stream("上游流式响应超时".to_string()));
                    }
                    if should_emit_keepalive_after_first_frame(self.saw_upstream_frame) {
                        return Ok(SseKeepAliveFrame::Comment.bytes().to_vec());
                    }
                }
            }
        }
    }

    fn process_sse_frame(&mut self, lines: &[String]) -> Vec<u8> {
        let mut data_lines = Vec::new();
        for line in lines {
            let trimmed = line.trim_end_matches(['\r', '\n']);
            if let Some(rest) = trimmed.strip_prefix("data:") {
                data_lines.push(rest.trim_start().to_string());
            }
        }
        if data_lines.is_empty() {
            return Vec::new();
        }
        let data = data_lines.join("\n");
        if data.trim() == "[DONE]" {
            self.state.saw_done = true;
            return self.finish_stream();
        }
        let value = match serde_json::from_str::<Value>(&data) {
            Ok(value) => value,
            Err(err) => {
                self.state.upstream_failed = true;
                return self.fail_stream(format!("上游返回了无效的 Chat SSE JSON: {err}"));
            }
        };
        if let Some(error) = value.get("error") {
            self.state.upstream_failed = true;
            let message = error
                .get("message")
                .and_then(Value::as_str)
                .map(str::to_string)
                .unwrap_or_else(|| error.to_string());
            return self.fail_stream(format!("上游 Chat SSE 错误: {message}"));
        }
        self.consume_chat_chunk(&value)
    }

    fn consume_chat_chunk(&mut self, value: &Value) -> Vec<u8> {
        let mut out = String::new();
        if self.state.response_id.is_none() {
            if let Some(id) = value.get("id").and_then(Value::as_str) {
                let normalized = id.trim().to_string();
                if !normalized.is_empty() {
                    self.state.response_id = Some(if normalized.starts_with("resp_") {
                        normalized
                    } else {
                        format!("resp_{normalized}")
                    });
                }
            }
        }
        if self.state.model.is_none() {
            if let Some(model) = value.get("model").and_then(Value::as_str) {
                let model = model.trim().to_string();
                if !model.is_empty() {
                    self.state.model = Some(model);
                }
            }
        }
        if self.state.created_at == 0 {
            self.state.created_at = value
                .get("created")
                .or_else(|| value.get("created_at"))
                .and_then(Value::as_i64)
                .unwrap_or(0);
        }
        if let Some(usage) = value.get("usage").and_then(Value::as_object) {
            self.capture_usage(usage);
        }
        let choices = value
            .get("choices")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        for choice in &choices {
            let finish_reason = choice
                .get("finish_reason")
                .and_then(Value::as_str)
                .unwrap_or_default();
            if !finish_reason.is_empty() {
                self.state.stop_reason = finish_reason.to_string();
            }
            let Some(delta) = choice.get("delta").and_then(Value::as_object).cloned() else {
                continue;
            };
            self.consume_delta(&delta, &mut out);
        }
        out.into_bytes()
    }

    fn consume_delta(&mut self, delta: &Map<String, Value>, out: &mut String) {
        if let Some(text) = delta.get("content").and_then(Value::as_str) {
            if !text.is_empty() {
                append_output_text_raw(&mut self.state.output_text, text);
                self.ensure_text_part_started(out);
                append_sse_event(
                    out,
                    "response.output_text.delta",
                    &json!({
                        "type": "response.output_text.delta",
                        "delta": text,
                        "item_id": self.text_item_id(),
                        "output_index": self.text_output_index(),
                        "content_index": 0,
                    }),
                );
            }
        }
        if let Some(reasoning) = delta
            .get("reasoning_content")
            .or_else(|| delta.get("reasoning"))
            .and_then(Value::as_str)
        {
            if !reasoning.is_empty() {
                append_output_text_raw(&mut self.state.reasoning_text, reasoning);
                self.ensure_reasoning_part_started(out);
                append_sse_event(
                    out,
                    "response.reasoning_summary_text.delta",
                    &json!({
                        "type": "response.reasoning_summary_text.delta",
                        "delta": reasoning,
                        "item_id": self.reasoning_item_id(),
                        "output_index": self.reasoning_output_index(),
                        "summary_index": 0,
                    }),
                );
            }
        }
        if let Some(tool_calls) = delta.get("tool_calls").and_then(Value::as_array) {
            for tool_call in tool_calls {
                self.consume_tool_call_delta(tool_call, out);
            }
        }
    }

    fn consume_tool_call_delta(&mut self, tool_call: &Value, out: &mut String) {
        let Some(tool_call) = tool_call.as_object() else {
            return;
        };
        let chat_index = tool_call
            .get("index")
            .and_then(Value::as_i64)
            .unwrap_or(0)
            .max(0) as usize;
        let index = self.ensure_tool_slot(chat_index);
        if let Some(id) = tool_call
            .get("id")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|id| !id.is_empty())
        {
            self.state.tools[index].call_id = Some(id.to_string());
        }
        if let Some(function) = tool_call.get("function").and_then(Value::as_object) {
            if let Some(name) = function
                .get("name")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|name| !name.is_empty())
            {
                self.state.tools[index].name = Some(name.to_string());
            }
            if let Some(arguments) = function.get("arguments").and_then(Value::as_str) {
                merge_streamed_arguments(&mut self.state.tools[index].arguments, arguments);
            }
        }
        // 身份（call_id + name）齐备后才可发射，避免虚构 identity。
        let tool = &self.state.tools[index];
        let can_emit = tool.call_id.is_some() && tool.name.is_some();
        if !can_emit {
            return;
        }
        self.emit_tool_delta_if_needed(index, out);
    }

    fn ensure_tool_slot(&mut self, chat_index: usize) -> usize {
        if let Some(position) = self
            .state
            .tools
            .iter()
            .position(|tool| tool.chat_index == chat_index)
        {
            return position;
        }
        self.state.tools.push(PendingChatTool {
            chat_index,
            ..Default::default()
        });
        self.state.tools.len() - 1
    }

    fn emit_tool_delta_if_needed(&mut self, index: usize, out: &mut String) {
        if self.state.tools[index].emitted_done {
            return;
        }
        self.ensure_response_started(out);
        if self.state.tools[index].output_index.is_none() {
            let output_index = self.allocate_output_index();
            self.state.tools[index].output_index = Some(output_index);
        }
        if !self.state.tools[index].emitted_added {
            self.state.tools[index].emitted_added = true;
            let tool = &self.state.tools[index];
            let call_id = tool.call_id.as_deref().unwrap_or_default();
            let name = tool.name.as_deref().unwrap_or_default();
            let is_custom = self.state.custom_tool_names.contains(name);
            let item = if is_custom {
                json!({
                    "id": call_id,
                    "type": "custom_tool_call",
                    "status": "in_progress",
                    "call_id": call_id,
                    "name": name,
                    "input": "",
                })
            } else {
                json!({
                    "id": call_id,
                    "type": "function_call",
                    "status": "in_progress",
                    "call_id": call_id,
                    "name": name,
                    "arguments": "",
                })
            };
            append_sse_event(
                out,
                "response.output_item.added",
                &json!({
                    "type": "response.output_item.added",
                    "output_index": self.tool_output_index(index),
                    "item": item,
                }),
            );
        }
        let tool = &self.state.tools[index];
        let is_custom = self
            .state
            .custom_tool_names
            .contains(tool.name.as_deref().unwrap_or_default());
        if is_custom {
            let Ok(input) = parse_custom_tool_input(tool.arguments.as_str()) else {
                return;
            };
            let sent = tool.last_sent_custom_input.as_str();
            if input == sent {
                return;
            }
            let Some(delta) = input.strip_prefix(sent) else {
                return;
            };
            if delta.is_empty() {
                return;
            }
            let call_id = tool.call_id.as_deref().unwrap_or_default();
            append_sse_event(
                out,
                "response.custom_tool_call_input.delta",
                &json!({
                    "type": "response.custom_tool_call_input.delta",
                    "delta": delta,
                    "item_id": call_id,
                    "output_index": self.tool_output_index(index),
                }),
            );
            self.state.tools[index].last_sent_custom_input = input;
        } else {
            let arguments = tool.arguments.as_str();
            let new_len = arguments.len();
            if new_len <= tool.last_sent_arguments_len {
                return;
            }
            let delta = &arguments[tool.last_sent_arguments_len..new_len];
            let call_id = tool.call_id.as_deref().unwrap_or_default();
            append_sse_event(
                out,
                "response.function_call_arguments.delta",
                &json!({
                    "type": "response.function_call_arguments.delta",
                    "delta": delta,
                    "item_id": call_id,
                    "output_index": self.tool_output_index(index),
                }),
            );
            self.state.tools[index].last_sent_arguments_len = new_len;
        }
    }

    fn finish_tool_items(&mut self, out: &mut String) -> Result<(), String> {
        let tool_count = self.state.tools.len();
        for index in 0..tool_count {
            // Chat 工具调用没有稳定身份就无法承接下一轮 tool result，必须按协议错误终止。
            let identity_ready =
                self.state.tools[index].call_id.is_some() && self.state.tools[index].name.is_some();
            if !identity_ready {
                return Err(format!(
                    "Chat tool call index {} missing id or function name",
                    self.state.tools[index].chat_index
                ));
            }
            self.emit_tool_delta_if_needed(index, out);
            if self.state.tools[index].emitted_done {
                continue;
            }
            self.state.tools[index].emitted_done = true;
            let tool = &self.state.tools[index];
            let call_id = tool.call_id.as_deref().unwrap_or_default().to_string();
            let name = tool.name.as_deref().unwrap_or_default().to_string();
            let is_custom = self.state.custom_tool_names.contains(name.as_str());
            let arguments = if tool.arguments.trim().is_empty() {
                "{}".to_string()
            } else {
                tool.arguments.clone()
            };
            let item = if is_custom {
                let input = parse_custom_tool_input(arguments.as_str())
                    .map_err(|err| format!("custom tool '{name}' 返回了无效参数包装: {err}"))?;
                json!({
                    "id": call_id,
                    "type": "custom_tool_call",
                    "status": "completed",
                    "call_id": call_id,
                    "name": name,
                    "input": input,
                })
            } else {
                json!({
                    "id": call_id,
                    "type": "function_call",
                    "status": "completed",
                    "call_id": call_id,
                    "name": name,
                    "arguments": arguments,
                })
            };
            let output_index = self.tool_output_index(index);
            append_sse_event(
                out,
                "response.output_item.done",
                &json!({
                    "type": "response.output_item.done",
                    "output_index": output_index,
                    "item": item.clone(),
                }),
            );
            self.state.final_tools.push((output_index, item));
        }
        Ok(())
    }

    fn ensure_response_started(&mut self, out: &mut String) {
        if self.state.started {
            return;
        }
        self.state.started = true;
        self.state.sequence = 1;
        let response = self.response_payload("in_progress");
        append_sse_event(
            out,
            "response.created",
            &json!({
                "type": "response.created",
                "sequence_number": self.state.sequence,
                "response": response,
            }),
        );
        append_sse_event(
            out,
            "response.in_progress",
            &json!({
                "type": "response.in_progress",
                "sequence_number": self.state.sequence,
                "response": self.response_payload("in_progress"),
            }),
        );
    }

    fn ensure_text_part_started(&mut self, out: &mut String) {
        self.ensure_response_started(out);
        if self.state.text_output_index.is_none() {
            self.state.text_output_index = Some(self.allocate_output_index());
        }
        if !self.state.text_item_started {
            self.state.text_item_started = true;
            append_sse_event(
                out,
                "response.output_item.added",
                &json!({
                    "type": "response.output_item.added",
                    "output_index": self.text_output_index(),
                    "item": {
                        "id": self.text_item_id(),
                        "type": "message",
                        "status": "in_progress",
                        "role": "assistant",
                        "content": [],
                    }
                }),
            );
        }
        if !self.state.text_part_started {
            self.state.text_part_started = true;
            append_sse_event(
                out,
                "response.content_part.added",
                &json!({
                    "type": "response.content_part.added",
                    "item_id": self.text_item_id(),
                    "output_index": self.text_output_index(),
                    "content_index": 0,
                    "part": { "type": "output_text", "text": "" },
                }),
            );
        }
    }

    fn finish_text_item(&mut self, out: &mut String) {
        if !self.state.text_part_started || self.state.text_finished {
            return;
        }
        self.state.text_finished = true;
        let text = self.state.output_text.clone();
        append_sse_event(
            out,
            "response.output_text.done",
            &json!({
                "type": "response.output_text.done",
                "text": text,
                "item_id": self.text_item_id(),
                "output_index": self.text_output_index(),
                "content_index": 0,
            }),
        );
        append_sse_event(
            out,
            "response.content_part.done",
            &json!({
                "type": "response.content_part.done",
                "item_id": self.text_item_id(),
                "output_index": self.text_output_index(),
                "content_index": 0,
                "part": { "type": "output_text", "text": text },
            }),
        );
        append_sse_event(
            out,
            "response.output_item.done",
            &json!({
                "type": "response.output_item.done",
                "output_index": self.text_output_index(),
                "item": {
                    "id": self.text_item_id(),
                    "type": "message",
                    "status": "completed",
                    "role": "assistant",
                    "content": [{ "type": "output_text", "text": text }],
                }
            }),
        );
    }

    fn ensure_reasoning_part_started(&mut self, out: &mut String) {
        self.ensure_response_started(out);
        if self.state.reasoning_output_index.is_none() {
            self.state.reasoning_output_index = Some(self.allocate_output_index());
        }
        if !self.state.reasoning_item_started {
            self.state.reasoning_item_started = true;
            append_sse_event(
                out,
                "response.output_item.added",
                &json!({
                    "type": "response.output_item.added",
                    "output_index": self.reasoning_output_index(),
                    "item": {
                        "id": self.reasoning_item_id(),
                        "type": "reasoning",
                        "status": "in_progress",
                        "summary": [],
                    }
                }),
            );
        }
        if !self.state.reasoning_part_started {
            self.state.reasoning_part_started = true;
            append_sse_event(
                out,
                "response.reasoning_summary_part.added",
                &json!({
                    "type": "response.reasoning_summary_part.added",
                    "item_id": self.reasoning_item_id(),
                    "output_index": self.reasoning_output_index(),
                    "summary_index": 0,
                    "part": { "type": "summary_text", "text": "" },
                }),
            );
        }
    }

    fn finish_reasoning_item(&mut self, out: &mut String) {
        if !self.state.reasoning_part_started || self.state.reasoning_finished {
            return;
        }
        self.state.reasoning_finished = true;
        let text = self.state.reasoning_text.clone();
        append_sse_event(
            out,
            "response.reasoning_summary_text.done",
            &json!({
                "type": "response.reasoning_summary_text.done",
                "text": text,
                "item_id": self.reasoning_item_id(),
                "output_index": self.reasoning_output_index(),
                "summary_index": 0,
            }),
        );
        append_sse_event(
            out,
            "response.reasoning_summary_part.done",
            &json!({
                "type": "response.reasoning_summary_part.done",
                "item_id": self.reasoning_item_id(),
                "output_index": self.reasoning_output_index(),
                "summary_index": 0,
                "part": { "type": "summary_text", "text": text },
            }),
        );
        append_sse_event(
            out,
            "response.output_item.done",
            &json!({
                "type": "response.output_item.done",
                "output_index": self.reasoning_output_index(),
                "item": {
                    "id": self.reasoning_item_id(),
                    "type": "reasoning",
                    "status": "completed",
                    "summary": [{ "type": "summary_text", "text": text }],
                }
            }),
        );
    }

    /// 终止流。只有成功终止（收到 [DONE]）才发射 `response.completed`；
    /// 上游错误/截断（未收到 [DONE] 即 EOF）不得合成成功快照。
    fn finish_stream(&mut self) -> Vec<u8> {
        if self.state.completed {
            return Vec::new();
        }
        if self.state.upstream_failed || !self.state.saw_done {
            return self.fail_stream("上游流式响应中断，未返回 [DONE]".to_string());
        }
        let mut out = String::new();
        self.ensure_response_started(&mut out);
        self.finish_reasoning_item(&mut out);
        self.finish_text_item(&mut out);
        if let Err(message) = self.finish_tool_items(&mut out) {
            return self.fail_stream_with_prefix(out, message);
        }
        self.state.completed = true;
        self.publish_usage();
        append_sse_event(
            &mut out,
            "response.completed",
            &json!({
                "type": "response.completed",
                "response": self.response_payload("completed"),
            }),
        );
        out.into_bytes()
    }

    fn fail_stream(&mut self, message: String) -> Vec<u8> {
        self.fail_stream_with_prefix(String::new(), message)
    }

    fn fail_stream_with_prefix(&mut self, mut out: String, message: String) -> Vec<u8> {
        if self.state.completed {
            return Vec::new();
        }
        self.ensure_response_started(&mut out);
        self.state.completed = true;
        if let Ok(mut collector) = self.usage_collector.lock() {
            collector.saw_terminal = true;
            collector.terminal_error = Some(message.clone());
        }
        append_sse_event(
            &mut out,
            "response.failed",
            &json!({
                "type": "response.failed",
                "response": {
                    "id": self.response_id(),
                    "object": "response",
                    "created_at": self.state.created_at,
                    "status": "failed",
                    "model": self.model(),
                    "output": [],
                    "error": {
                        "code": "upstream_protocol_error",
                        "message": message,
                    },
                    "usage": self.usage_payload(),
                }
            }),
        );
        out.into_bytes()
    }

    fn publish_usage(&self) {
        if let Ok(mut collector) = self.usage_collector.lock() {
            collector.usage.input_tokens = Some(self.state.input_tokens);
            collector.usage.cached_input_tokens = Some(self.state.cached_input_tokens);
            collector.usage.cache_write_tokens = Some(self.state.cache_write_tokens);
            collector.usage.output_tokens = Some(self.state.output_tokens);
            collector.usage.total_tokens = self.state.total_tokens;
            collector.usage.reasoning_output_tokens = Some(self.state.reasoning_output_tokens);
            if !self.state.output_text.trim().is_empty() {
                collector.usage.output_text = Some(self.state.output_text.clone());
            }
            collector.saw_terminal = true;
        }
    }

    fn capture_usage(&mut self, usage: &Map<String, Value>) {
        if let Some(value) = usage_i64(usage, &["prompt_tokens", "input_tokens"]) {
            self.state.input_tokens = value;
        }
        if let Some(value) = usage_i64(
            usage,
            &[
                "prompt_tokens_details.cached_tokens",
                "input_tokens_details.cached_tokens",
                "prompt_cache_hit_tokens",
            ],
        ) {
            self.state.cached_input_tokens = value;
        }
        if let Some(value) = usage_i64(
            usage,
            &[
                "prompt_tokens_details.cache_write_tokens",
                "input_tokens_details.cache_write_tokens",
                "prompt_cache_miss_tokens",
            ],
        ) {
            self.state.cache_write_tokens = value;
        }
        if let Some(value) = usage_i64(usage, &["completion_tokens", "output_tokens"]) {
            self.state.output_tokens = value;
        }
        if let Some(value) = usage_i64(
            usage,
            &[
                "completion_tokens_details.reasoning_tokens",
                "output_tokens_details.reasoning_tokens",
            ],
        ) {
            self.state.reasoning_output_tokens = value;
        }
        self.state.total_tokens = usage_i64(usage, &["total_tokens"])
            .or_else(|| Some(self.state.input_tokens + self.state.output_tokens));
    }

    fn response_payload(&self, status: &str) -> Value {
        json!({
            "id": self.response_id(),
            "object": "response",
            "created_at": self.state.created_at,
            "status": status,
            "model": self.model(),
            "output": if status == "completed" { self.completed_output() } else { Value::Array(Vec::new()) },
            "usage": self.usage_payload(),
        })
    }

    fn completed_output(&self) -> Value {
        let mut indexed_output = Vec::new();
        if !self.state.reasoning_text.is_empty() {
            indexed_output.push((
                self.reasoning_output_index(),
                json!({
                    "id": self.reasoning_item_id(),
                    "type": "reasoning",
                    "status": "completed",
                    "summary": [{ "type": "summary_text", "text": self.state.reasoning_text }],
                }),
            ));
        }
        if !self.state.output_text.is_empty() {
            indexed_output.push((
                self.text_output_index(),
                json!({
                    "id": self.text_item_id(),
                    "type": "message",
                    "status": "completed",
                    "role": "assistant",
                    "content": [{ "type": "output_text", "text": self.state.output_text }],
                }),
            ));
        }
        indexed_output.extend(self.state.final_tools.iter().cloned());
        indexed_output.sort_by_key(|(index, _)| *index);
        Value::Array(indexed_output.into_iter().map(|(_, item)| item).collect())
    }

    fn usage_payload(&self) -> Value {
        json!({
            "input_tokens": self.state.input_tokens,
            "output_tokens": self.state.output_tokens,
            "total_tokens": self
                .state
                .total_tokens
                .unwrap_or(self.state.input_tokens + self.state.output_tokens),
            "input_tokens_details": {
                "cached_tokens": self.state.cached_input_tokens,
                "cache_write_tokens": self.state.cache_write_tokens,
            },
            "output_tokens_details": { "reasoning_tokens": self.state.reasoning_output_tokens },
        })
    }

    fn response_id(&self) -> String {
        self.state
            .response_id
            .clone()
            .unwrap_or_else(|| "resp_codexmanager".to_string())
    }

    fn text_item_id(&self) -> String {
        format!("msg_{}", self.response_id())
    }

    fn reasoning_item_id(&self) -> String {
        format!("rs_{}", self.response_id())
    }

    fn allocate_output_index(&mut self) -> usize {
        let index = self.state.next_output_index;
        self.state.next_output_index += 1;
        index
    }

    fn text_output_index(&self) -> usize {
        self.state.text_output_index.unwrap_or(0)
    }

    fn reasoning_output_index(&self) -> usize {
        self.state.reasoning_output_index.unwrap_or(0)
    }

    fn tool_output_index(&self, tool_slot: usize) -> usize {
        self.state.tools[tool_slot].output_index.unwrap_or(0)
    }

    fn model(&self) -> String {
        self.state.model.clone().unwrap_or_default()
    }
}

impl Read for ResponsesFromChatCompletionsSseReader {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        loop {
            let n = self.out_cursor.read(buf)?;
            if n > 0 {
                return Ok(n);
            }
            let chunk = self.next_chunk()?;
            if chunk.is_empty() {
                return Ok(0);
            }
            self.out_cursor = Cursor::new(chunk);
        }
    }
}

fn append_sse_event(buffer: &mut String, event: &str, payload: &Value) {
    buffer.push_str("event: ");
    buffer.push_str(event);
    buffer.push('\n');
    buffer.push_str("data: ");
    buffer.push_str(payload.to_string().as_str());
    buffer.push_str("\n\n");
}

fn merge_streamed_arguments(buffer: &mut String, incoming: &str) {
    if incoming.is_empty() || buffer.starts_with(incoming) {
        return;
    }
    if incoming.starts_with(buffer.as_str()) {
        buffer.clear();
        buffer.push_str(incoming);
    } else {
        buffer.push_str(incoming);
    }
}

fn parse_custom_tool_input(arguments: &str) -> Result<String, String> {
    let value = serde_json::from_str::<Value>(arguments)
        .map_err(|err| format!("arguments 不是合法 JSON: {err}"))?;
    let object = value
        .as_object()
        .ok_or_else(|| "arguments 必须是包含 input 的对象".to_string())?;
    if object.len() != 1 {
        return Err("arguments 只能包含 input 字段".to_string());
    }
    object
        .get("input")
        .and_then(Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| "arguments.input 必须是字符串".to_string())
}

fn usage_i64(usage: &Map<String, Value>, paths: &[&str]) -> Option<i64> {
    for path in paths {
        let mut current: Option<&Value> = None;
        let mut found = true;
        for (index, segment) in path.split('.').enumerate() {
            current = if index == 0 {
                usage.get(segment)
            } else {
                current
                    .and_then(Value::as_object)
                    .and_then(|object| object.get(segment))
            };
            if current.is_none() {
                found = false;
                break;
            }
        }
        if !found {
            continue;
        }
        if let Some(value) = current.and_then(Value::as_i64) {
            return Some(value);
        }
    }
    None
}

#[cfg(test)]
#[path = "chat_to_responses_tests.rs"]
mod tests;
