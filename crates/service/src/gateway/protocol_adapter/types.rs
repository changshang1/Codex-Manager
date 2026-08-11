use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ResponseAdapter {
    Passthrough,
    AnthropicMessagesFromResponses,
    ResponsesFromAnthropicMessages,
    ChatCompletionsFromResponses,
    /// 将上游 Chat Completions 响应转换为 Responses 格式（Responses→Chat 桥接）
    ResponsesFromChatCompletions,
    #[allow(dead_code)]
    CompactFromChatCompletions,
    ImagesB64JsonFromResponses,
    ImagesUrlFromResponses,
    GeminiJson,
    GeminiSse,
    GeminiCliJson,
    GeminiCliSse,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)]
pub(crate) enum GeminiStreamOutputMode {
    Sse,
    Raw,
}

pub(crate) type ToolNameRestoreMap = BTreeMap<String, String>;

#[derive(Debug)]
pub(crate) struct AdaptedGatewayRequest {
    pub(crate) path: String,
    pub(crate) body: Vec<u8>,
    pub(crate) response_adapter: ResponseAdapter,
    pub(crate) gemini_stream_output_mode: Option<GeminiStreamOutputMode>,
    pub(crate) tool_name_restore_map: ToolNameRestoreMap,
    /// 请求中声明的 custom tool 名（Responses 侧 custom tool 需要在下游识别）。
    /// 聚合 API 的 Responses→Chat 桥接直接使用转换返回的 custom tool 名集合；
    /// 主网关路径暂不产生该适配器，字段保留以对齐 AdaptedGatewayRequest 的完整契约。
    #[allow(dead_code)]
    pub(crate) custom_tool_names: BTreeSet<String>,
}
