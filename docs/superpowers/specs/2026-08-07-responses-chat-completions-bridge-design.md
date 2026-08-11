# Responses to Chat Completions Bridge Design

## Goal

Allow a Codex client that speaks the OpenAI Responses API to use an aggregate API
whose upstream only implements OpenAI Chat Completions. The first target is:

```text
Codex -> CodexManager /v1/responses
      -> OpenCode Go /v1/chat/completions
      -> deepseek-v4-flash
```

The bridge must support normal text, streaming, function tools, Codex custom
tools such as `apply_patch`, multi-turn tool history, parallel tool calls,
reasoning metadata when available, usage, and upstream errors.

## Design Sources

The implementation may reuse architecture and behavior from MIT-licensed code,
with required copyright notices retained when substantial code is copied:

- LiteLLM `litellm/responses/litellm_completion_transformation/`: request,
  response, stream, session, and custom-tool separation.
- OmniRoute `open-sse/translator/{request,response}/openai-responses*`: Codex
  custom-tool wrapping and Responses SSE lifecycle behavior.

QuantumNous/new-api is AGPL-3.0. Its registry and golden-test organization may
inform the design, but its implementation must not be copied into this project.

## Non-Goals

- Do not change native Responses, Anthropic, Gemini, image, or existing Chat to
  Responses routes unless required to share a small protocol primitive.
- Do not infer protocol conversion from a URL or `action` string.
- Do not emulate hosted Responses tools such as web search, computer use, or
  file search through Chat Completions.
- Do not silently discard stateful Responses fields.
- Do not make a paid OpenCode Go request in automated tests.

## Persisted Configuration

Add `upstream_wire` to aggregate APIs with these normalized values:

- `passthrough`: existing behavior and migration default.
- `chat_completions`: bridge incoming `/v1/responses` to a Chat Completions
  upstream and bridge the response back to Responses.

`action` remains only a path override. When `upstream_wire=chat_completions` and
`action` is absent, the effective upstream path is `/v1/chat/completions`.
Existing rows must remain `passthrough` after migration.

Expose this as a Chinese-first select in the aggregate API modal. The option is
shown for Codex, Responses-compatible, and compatible aggregate APIs. Existing
create/update/list RPC, Web command, normalization, and TypeScript types must
carry the value end to end.

## Request Conversion

Create a focused protocol-adapter module. It accepts the final candidate body
after model override and compatibility rewriting, and returns:

- the Chat Completions JSON body;
- a request-scoped bridge context containing declared custom-tool names;
- `ResponseAdapter::ResponsesFromChatCompletions`;
- the effective Chat upstream path.

Field mapping:

| Responses | Chat Completions |
|---|---|
| `model` | `model` |
| `instructions` | leading `system` message |
| string `input` | `user` message |
| message input items | same-role Chat messages |
| `function_call` | assistant `tool_calls[]` |
| `function_call_output` | `tool` message with `tool_call_id` |
| `custom_tool_call` | assistant function call with `{"input": raw}` |
| `custom_tool_call_output` | `tool` message with `tool_call_id` |
| function tool | Chat function tool |
| custom tool | Chat function tool with required string `input` |
| `tool_choice` | equivalent Chat choice where representable |
| `parallel_tool_calls` | same field |
| `reasoning.effort` | `reasoning_effort` |
| `max_output_tokens` | `max_completion_tokens` |
| `stream` | `stream` |

The custom function schema is:

```json
{
  "type": "function",
  "function": {
    "name": "apply_patch",
    "description": "...",
    "parameters": {
      "type": "object",
      "properties": { "input": { "type": "string" } },
      "required": ["input"],
      "additionalProperties": false
    }
  }
}
```

Reject before contacting the upstream when a request depends on unsupported
stateful or hosted behavior, including non-empty `previous_response_id`,
`conversation`, hosted-only tools, or an input item without a valid Chat
equivalent. The error must be a structured 400 response, not a partial rewrite.

## Non-Streaming Response Conversion

Convert a Chat completion into one Responses object:

- `id` uses a stable `resp_` value for the request.
- text becomes a completed `message` with `output_text` content.
- function calls become completed `function_call` output items.
- calls whose names were declared as custom tools become `custom_tool_call`
  items, with the JSON wrapper's `input` string unwrapped.
- Chat usage maps to Responses input/output/total token fields and preserves
  cached and reasoning token details when supplied.
- a malformed custom-tool wrapper is returned as a protocol error instead of
  being passed to Codex as an empty patch.

## Streaming Response State Machine

Implement a request-local `ChatToResponsesStreamState`. It owns response ID,
sequence number, output-index allocation, current text/reasoning items, tool
calls keyed by Chat index and call ID, custom-tool metadata, usage, and terminal
state.

Emit, in order as applicable:

1. `response.created`
2. `response.in_progress`
3. `response.output_item.added`
4. `response.content_part.added` for text
5. `response.output_text.delta` for text
6. `response.function_call_arguments.delta` for function tools, or
   `response.custom_tool_call_input.delta` for custom tools
7. corresponding `*.done` events
8. `response.content_part.done` and `response.output_item.done`
9. `response.completed` with a dense final `output` snapshot and usage

Requirements:

- A tool name or call ID may arrive later than its arguments; buffer until the
  item can be emitted without fabricating identity.
- Parallel tool calls must retain independent buffers and output indexes.
- Repeated cumulative argument snapshots must not duplicate content.
- A tool call present only in the final Chat chunk must still be emitted before
  completion.
- `[DONE]` finalizes only once.
- Upstream JSON/SSE errors and truncated streams must not synthesize a successful
  `response.completed`; return/emit a failed gateway response using existing
  error handling.
- The adapter must not retry after response bytes have been delivered.

## Integration Boundaries

- `aggregate_api.rs` selects the bridge from persisted `upstream_wire`, not from
  `action`.
- Request conversion belongs under `gateway/protocol_adapter/`.
- Non-streaming conversion belongs in `http_bridge/body_conversion.rs` or a
  focused child module.
- Streaming conversion belongs in a dedicated stream reader under
  `http_bridge/stream_readers/`.
- Delivery selects the new adapter without changing existing adapters.
- Request logs record original/adapted paths and the adapter name, but never the
  prompt, tool input, or API key.

## Tests

Add focused Rust tests for:

1. migration/default and create/update/list round-trip of `upstream_wire`;
2. path defaulting and custom `action` behavior;
3. text request conversion, instructions, model override, reasoning, and usage;
4. function tool declaration, call history, and tool output;
5. custom `apply_patch` declaration and history wrapping;
6. non-streaming text/function/custom response conversion;
7. streaming text event order and final snapshot;
8. fragmented function arguments and parallel calls;
9. custom tool events and exact raw patch round-trip;
10. missing IDs, malformed arguments, upstream error, and truncated stream;
11. unsupported `previous_response_id` and hosted tools rejected before send;
12. regression tests proving passthrough and existing adapters are unchanged.

An external mock server may later verify the exact OpenCode Go endpoint and
headers, but no API key or paid request belongs in the repository tests.

## Acceptance Criteria

- A mock `/v1/chat/completions` upstream can complete a streamed Codex text turn.
- A normal function tool can complete a full request/call/result/follow-up loop.
- a custom `apply_patch` payload round-trips byte-for-byte through the bridge.
- two parallel tool calls preserve names, IDs, arguments, and output indexes.
- existing aggregate API rows and native Responses paths retain old behavior.
- relevant service/core tests pass, followed by the narrowest practical
  workspace regression suite.

