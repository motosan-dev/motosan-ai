use crate::error::MotosanError;
use crate::retry::RetryPolicy;
use crate::types::{ChatRequest, Role, StopReason, StreamEvent, Usage};
use futures_core::Stream;
use reqwest::Client;
use serde_json::{json, Value};
use std::collections::{HashSet, VecDeque};
use std::pin::Pin;
use std::task::Poll;

/// Default endpoint for the ChatGPT-backend Responses API.
const CHATGPT_CODEX_URL: &str = "https://chatgpt.com/backend-api/codex/responses";
/// `originator` header value codex's CLI sends; settled GREEN by the spike.
const ORIGINATOR: &str = "codex_cli_rs";

// NOTE: `#[allow(dead_code)]` is for Task 1 ONLY (no `ProviderImpl` impl yet, so
// the fields/methods are not all read). It is removed in Task 4 when `stream`/`chat`
// land and consume them.
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct ChatGptCodexProvider {
    http: Client,
    access_token: String,
    account_id: String,
    model: String,
    base_url: String,
    retry_policy: RetryPolicy,
}

#[allow(dead_code)]
impl ChatGptCodexProvider {
    pub fn new(
        access_token: impl Into<String>,
        account_id: impl Into<String>,
        model: impl Into<String>,
        base_url: Option<String>,
    ) -> Self {
        Self {
            http: Client::new(),
            access_token: access_token.into(),
            account_id: account_id.into(),
            model: model.into(),
            base_url: base_url.unwrap_or_else(|| CHATGPT_CODEX_URL.to_string()),
            retry_policy: RetryPolicy::default(),
        }
    }

    pub fn with_retry_policy(mut self, policy: RetryPolicy) -> Self {
        self.retry_policy = policy;
        self
    }

    fn apply_auth(&self, request: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        request
            .header("authorization", format!("Bearer {}", self.access_token))
            .header("chatgpt-account-id", &self.account_id)
            .header("originator", ORIGINATOR)
            .header("openai-beta", "responses=experimental")
            .header("accept", "text/event-stream")
    }

    fn url(&self) -> String {
        self.base_url.clone()
    }

    /// Serialize a [`ChatRequest`] into a ChatGPT-backend **Responses API** body.
    ///
    /// Shape settled by the route-B spike (scope §1) + PI's `buildRequestBody`:
    /// `store:false` is hard-required, the system prompt goes in `instructions`
    /// (NOT into `input`), non-system messages convert to Responses **input
    /// items**, and reasoning encrypted content is requested via `include`.
    ///
    /// ## Content-block coverage
    ///
    /// The motosan `Message`/`ContentBlock` model carries `Text`/`Image`/
    /// `Document` blocks plus flat `content` text and `tool_calls`. This
    /// serializer maps the cases the ChatGPT-backend transport needs:
    /// - user/assistant flat text -> `message` items with `input_text`/`output_text`;
    /// - assistant `tool_calls` -> `function_call` items;
    /// - `Role::Tool` results -> `function_call_output` items.
    ///
    /// Multimodal `ContentBlock::Image`/`Document` blocks are **not** emitted
    /// here — v1 of this provider is text-only (`ProviderCapabilities::text_only`),
    /// and a request carrying them would be rejected upstream by validation.
    /// See `// TODO(phase2)` below for the image passthrough hook.
    pub fn build_responses_body(&self, req: &ChatRequest) -> Value {
        let model = req.model.clone().unwrap_or_else(|| self.model.clone());

        // Instructions = the system prompt. `system_blocks` (joined) takes
        // priority over the `system` string, then any `Role::System` message.
        // Mirrors openai.rs's `chat_via_responses` precedence.
        let mut instructions_parts: Vec<String> = Vec::new();
        if let Some(blocks) = &req.system_blocks {
            for b in blocks {
                let trimmed = b.text.trim();
                if !trimmed.is_empty() {
                    instructions_parts.push(trimmed.to_string());
                }
            }
        } else if let Some(system) = &req.system {
            let trimmed = system.trim();
            if !trimmed.is_empty() {
                instructions_parts.push(trimmed.to_string());
            }
        }

        let mut input: Vec<Value> = Vec::new();
        for message in &req.messages {
            match message.role {
                // System messages go into `instructions`, never `input`.
                Role::System => {
                    let trimmed = message.content.trim();
                    if !trimmed.is_empty() {
                        instructions_parts.push(trimmed.to_string());
                    }
                }
                Role::User => {
                    // TODO(phase2): emit `input_image` blocks for
                    // `message.content_blocks` (Image/Document) — text-only v1.
                    input.push(json!({
                        "type": "message",
                        "role": "user",
                        "content": [{"type": "input_text", "text": message.content}],
                    }));
                }
                Role::Assistant => {
                    // Assistant text (if any) becomes an `output_text` message item.
                    if !message.content.is_empty() {
                        input.push(json!({
                            "type": "message",
                            "role": "assistant",
                            "content": [{"type": "output_text", "text": message.content}],
                        }));
                    }
                    // Each tool call becomes its own `function_call` item.
                    for tool_call in &message.tool_calls {
                        input.push(json!({
                            "type": "function_call",
                            "call_id": tool_call.id,
                            "name": tool_call.name,
                            "arguments": serde_json::to_string(&tool_call.input)
                                .unwrap_or_else(|_| "{}".to_string()),
                        }));
                    }
                }
                Role::Tool => {
                    if let Some(call_id) = &message.tool_call_id {
                        input.push(json!({
                            "type": "function_call_output",
                            "call_id": call_id,
                            "output": message.content,
                        }));
                    }
                }
            }
        }

        let instructions = if instructions_parts.is_empty() {
            "You are a helpful assistant.".to_string()
        } else {
            instructions_parts.join("\n\n")
        };

        let mut body = json!({
            "model": model,
            "store": false,
            "stream": true,
            "instructions": instructions,
            "input": input,
            "include": ["reasoning.encrypted_content"],
            "tool_choice": "auto",
            "parallel_tool_calls": true,
        });

        // Conditional: tools (Responses flat tool shape, `strict: null`).
        if let Some(tools) = &req.tools {
            let mapped: Vec<Value> = tools
                .iter()
                .map(|tool| {
                    json!({
                        "type": "function",
                        "name": tool.schema.name,
                        "description": tool.schema.description,
                        "parameters": tool.schema.input_schema,
                        "strict": Value::Null,
                    })
                })
                .collect();
            if !mapped.is_empty() {
                body["tools"] = json!(mapped);
            }
        }

        // Conditional: reasoning `{effort, summary:"auto"}` when an effort is
        // supplied via provider_options (no first-class field on ChatRequest).
        if let Some(effort) = req
            .provider_options
            .as_ref()
            .and_then(|opts| opts.get("reasoning_effort"))
            .and_then(Value::as_str)
        {
            body["reasoning"] = json!({"effort": effort, "summary": "auto"});
        }

        // Conditional: temperature.
        if let Some(temperature) = req.temperature {
            body["temperature"] = json!(temperature);
        }

        body
    }
}

/// Translate the ChatGPT-backend typed `response.*` SSE stream into motosan
/// [`StreamEvent`]s.
///
/// Structurally a copy of `gemini_code_assist`'s `CodeAssistStreamAdapter`
/// shell (the `Stream`/`poll_next` driver, the `pending` queue, and the
/// `seen_tool_ids` set). The Gemini per-event parse block is replaced by
/// [`ChatGptCodexStreamAdapter::handle_event`], a pure mapping from one
/// Responses event JSON to zero-or-more queued [`StreamEvent`]s — kept
/// separate so the mapping is unit-testable from fixture frames without
/// mocking a byte stream.
#[allow(dead_code)]
struct ChatGptCodexStreamAdapter {
    inner: Pin<
        Box<
            dyn Stream<
                    Item = Result<
                        eventsource_stream::Event,
                        eventsource_stream::EventStreamError<reqwest::Error>,
                    >,
                > + Send,
        >,
    >,
    pending: VecDeque<StreamEvent>,
    /// `call_id`s seen via `response.output_item.added` (function_call) so the
    /// matching `response.output_item.done` can close the same id.
    seen_tool_ids: HashSet<String>,
    /// Set once any `function_call` item is observed, so `response.completed`
    /// resolves to `ToolUse` (mirrors the gated `cli_terminal_stop_reason`
    /// helper, which `chatgpt-codex` is not in scope to reach from here).
    saw_tool_call: bool,
    /// A fatal stream error to surface on the next `poll_next` (top-level
    /// `error` / `response.failed`).
    error: Option<String>,
}

// NOTE: `#[allow(dead_code)]` is for Task 3 ONLY — `handle_event` (and the
// adapter itself) is not constructed by library code until Task 4 wires the
// `stream` impl. Removed in Task 4 once `stream` consumes it. Tests exercise it
// directly, so the mapping is fully covered now.
#[allow(dead_code)]
impl ChatGptCodexStreamAdapter {
    /// Map a single decoded Responses SSE `data` JSON value to zero or more
    /// [`StreamEvent`]s, queued onto `pending`. Pure (no I/O); the only side
    /// effects are mutations of `pending` / `seen_tool_ids` / `saw_tool_call`
    /// / `error`. See the module-level mapping table.
    fn handle_event(&mut self, data: &Value) {
        let event_type = data.get("type").and_then(Value::as_str).unwrap_or("");

        match event_type {
            // Final-answer text deltas.
            "response.output_text.delta" => {
                if let Some(delta) = data.get("delta").and_then(Value::as_str) {
                    if !delta.is_empty() {
                        self.pending.push_back(StreamEvent::text(delta));
                    }
                }
            }

            // Reasoning deltas (both the raw and the summary stream) -> thinking.
            "response.reasoning_text.delta" | "response.reasoning_summary_text.delta" => {
                if let Some(delta) = data.get("delta").and_then(Value::as_str) {
                    if !delta.is_empty() {
                        self.pending.push_back(StreamEvent::thinking_delta(delta));
                    }
                }
            }

            // A new output item began. Only `function_call` items open a tool call;
            // `reasoning` / `message` items are handled by their own events.
            "response.output_item.added" => {
                if let Some(item) = data.get("item") {
                    if item.get("type").and_then(Value::as_str) == Some("function_call") {
                        let call_id = item
                            .get("call_id")
                            .and_then(Value::as_str)
                            .unwrap_or("")
                            .to_string();
                        let name = item
                            .get("name")
                            .and_then(Value::as_str)
                            .unwrap_or("")
                            .to_string();
                        if !call_id.is_empty() {
                            self.saw_tool_call = true;
                            self.seen_tool_ids.insert(call_id.clone());
                            self.pending
                                .push_back(StreamEvent::tool_call_start(&call_id, &name));
                        }
                    }
                }
            }

            // Streamed tool-call argument fragments. The wire key is `item_id`.
            "response.function_call_arguments.delta" => {
                let id = data
                    .get("item_id")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string();
                if let Some(delta) = data.get("delta").and_then(Value::as_str) {
                    if !id.is_empty() {
                        self.pending
                            .push_back(StreamEvent::tool_call_args_with_id(&id, delta));
                    }
                }
            }

            // An output item finished. Close `function_call` items; ignore
            // `reasoning` (encrypted_content — no thinking replay in v1) and
            // `message` (text was already streamed).
            "response.output_item.done" => {
                // Only `function_call` items are closed here. `reasoning` items
                // carry `encrypted_content` and `message` items already had
                // their text streamed, so both are ignored.
                // TODO(phase2): round-trip a `reasoning` item's
                // `encrypted_content` as a thinking signature for replay.
                if let Some(item) = data.get("item") {
                    if item.get("type").and_then(Value::as_str) == Some("function_call") {
                        let call_id = item
                            .get("call_id")
                            .and_then(Value::as_str)
                            .unwrap_or("")
                            .to_string();
                        if !call_id.is_empty() {
                            self.pending
                                .push_back(StreamEvent::tool_call_end_with_id(call_id));
                        }
                    }
                }
            }

            // Terminal event: emit usage (if present) then the stop reason.
            "response.completed" => {
                let response = data.get("response");
                if let Some(usage) = response.and_then(|r| r.get("usage")) {
                    let input_tokens = usage
                        .get("input_tokens")
                        .and_then(Value::as_u64)
                        .unwrap_or(0) as u32;
                    let output_tokens = usage
                        .get("output_tokens")
                        .and_then(Value::as_u64)
                        .unwrap_or(0) as u32;
                    self.pending.push_back(StreamEvent::usage(Usage {
                        input_tokens,
                        output_tokens,
                        cache_creation_input_tokens: None,
                        cache_read_input_tokens: None,
                    }));
                }
                let status = response
                    .and_then(|r| r.get("status"))
                    .and_then(Value::as_str)
                    .unwrap_or("completed");
                let stop_reason = match status {
                    _ if self.saw_tool_call => StopReason::ToolUse,
                    "incomplete" => StopReason::MaxTokens,
                    _ => StopReason::EndTurn,
                };
                self.pending
                    .push_back(StreamEvent::done_with_stop_reason(stop_reason));
            }

            // Fatal stream errors -> surfaced as Err on the next poll.
            "error" | "response.failed" => {
                let msg = data
                    .get("message")
                    .and_then(Value::as_str)
                    .or_else(|| {
                        data.get("response")
                            .and_then(|r| r.get("error"))
                            .and_then(|e| e.get("message"))
                            .and_then(Value::as_str)
                    })
                    .or_else(|| {
                        data.get("error")
                            .and_then(|e| e.get("message"))
                            .and_then(Value::as_str)
                    })
                    .unwrap_or("ChatGPT-backend stream error")
                    .to_string();
                self.error = Some(msg);
            }

            // Everything else (response.created/in_progress, content_part.*,
            // output_text.done, reasoning item add, etc.) is ignored.
            _ => {}
        }
    }
}

impl Stream for ChatGptCodexStreamAdapter {
    type Item = Result<StreamEvent, MotosanError>;

    fn poll_next(
        mut self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> Poll<Option<Self::Item>> {
        if let Some(ev) = self.pending.pop_front() {
            return Poll::Ready(Some(Ok(ev)));
        }
        if let Some(msg) = self.error.take() {
            return Poll::Ready(Some(Err(MotosanError::Stream(msg))));
        }

        loop {
            match self.inner.as_mut().poll_next(cx) {
                Poll::Ready(Some(Ok(event))) => {
                    let data = event.data.trim();
                    if data.is_empty() || data == "[DONE]" {
                        continue;
                    }
                    let value: Value = match serde_json::from_str(data) {
                        Ok(v) => v,
                        Err(_) => continue,
                    };
                    self.handle_event(&value);

                    if let Some(first) = self.pending.pop_front() {
                        return Poll::Ready(Some(Ok(first)));
                    }
                    if let Some(msg) = self.error.take() {
                        return Poll::Ready(Some(Err(MotosanError::Stream(msg))));
                    }
                    continue;
                }
                Poll::Ready(Some(Err(e))) => {
                    return Poll::Ready(Some(Err(MotosanError::Stream(e.to_string()))));
                }
                Poll::Ready(None) => return Poll::Ready(None),
                Poll::Pending => return Poll::Pending,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{ChatRequest, Message, ToolCall};
    use serde_json::json;

    fn test_provider() -> ChatGptCodexProvider {
        ChatGptCodexProvider::new("test-token", "acct-123", "gpt-5.5", None)
    }

    fn simple_user_request(text: &str) -> ChatRequest {
        ChatRequest::builder().message(Message::user(text)).build()
    }

    #[test]
    fn body_has_required_codex_fields() {
        let p = test_provider();
        let req = simple_user_request("hi");
        let body = p.build_responses_body(&req);

        assert_eq!(body["store"], json!(false)); // REQUIRED
        assert_eq!(body["stream"], json!(true));
        assert_eq!(body["model"], json!("gpt-5.5"));
        assert!(body["instructions"].is_string()); // system prompt here, not a message
        assert!(body["input"].is_array()); // converted Responses items
        assert_eq!(body["include"], json!(["reasoning.encrypted_content"]));
        assert_eq!(body["tool_choice"], json!("auto"));
        assert_eq!(body["parallel_tool_calls"], json!(true));

        // The single user message becomes one Responses `message` input item
        // with an `input_text` content block — system text is NOT in `input`.
        let input = body["input"].as_array().unwrap();
        assert_eq!(input.len(), 1);
        assert_eq!(input[0]["type"], "message");
        assert_eq!(input[0]["role"], "user");
        assert_eq!(input[0]["content"][0]["type"], "input_text");
        assert_eq!(input[0]["content"][0]["text"], "hi");

        // No tools / reasoning / temperature on a bare request.
        assert!(body.get("tools").is_none());
        assert!(body.get("reasoning").is_none());
        assert!(body.get("temperature").is_none());
    }

    #[test]
    fn system_message_goes_to_instructions_not_input() {
        let p = test_provider();
        let req = ChatRequest::builder()
            .message(Message::system("You are a pirate."))
            .message(Message::user("hi"))
            .build();
        let body = p.build_responses_body(&req);

        assert_eq!(body["instructions"], json!("You are a pirate."));
        let input = body["input"].as_array().unwrap();
        // Only the user message survives into `input`.
        assert_eq!(input.len(), 1);
        assert_eq!(input[0]["role"], "user");
    }

    #[test]
    fn assistant_text_becomes_output_text_item() {
        let p = test_provider();
        let req = ChatRequest::builder()
            .message(Message::user("hi"))
            .message(Message::assistant("hello there"))
            .build();
        let body = p.build_responses_body(&req);

        let input = body["input"].as_array().unwrap();
        assert_eq!(input.len(), 2);
        assert_eq!(input[1]["type"], "message");
        assert_eq!(input[1]["role"], "assistant");
        assert_eq!(input[1]["content"][0]["type"], "output_text");
        assert_eq!(input[1]["content"][0]["text"], "hello there");
    }

    #[test]
    fn tool_call_and_result_serialize_as_function_items() {
        let p = test_provider();
        let tool = motosan_agent_primitives::ToolSchema::new(
            "get_weather",
            "Fetch the weather",
            json!({"type": "object", "properties": {"city": {"type": "string"}}}),
        );
        let req = ChatRequest::builder()
            .tool_schemas(&[tool])
            .message(Message::user("weather in Paris?"))
            .message(Message::assistant_with_tool_calls(
                "",
                vec![ToolCall {
                    id: "call_1".to_string(),
                    name: "get_weather".to_string(),
                    input: json!({"city": "Paris"}),
                }],
            ))
            .message(Message::tool_result("call_1", "sunny, 21C"))
            .build();
        let body = p.build_responses_body(&req);

        // Tools serialize to the Responses flat tool shape (strict: null).
        let tools = body["tools"].as_array().unwrap();
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0]["type"], "function");
        assert_eq!(tools[0]["name"], "get_weather");
        assert_eq!(tools[0]["description"], "Fetch the weather");
        assert!(tools[0]["parameters"].is_object());
        assert_eq!(tools[0]["strict"], json!(null));

        let input = body["input"].as_array().unwrap();
        // user message, function_call, function_call_output.
        assert_eq!(input.len(), 3);

        // Assistant tool call -> function_call item.
        let fc = &input[1];
        assert_eq!(fc["type"], "function_call");
        assert_eq!(fc["call_id"], "call_1");
        assert_eq!(fc["name"], "get_weather");
        // arguments are a JSON-encoded string.
        let args: serde_json::Value =
            serde_json::from_str(fc["arguments"].as_str().unwrap()).unwrap();
        assert_eq!(args, json!({"city": "Paris"}));

        // Tool result -> function_call_output item.
        let out = &input[2];
        assert_eq!(out["type"], "function_call_output");
        assert_eq!(out["call_id"], "call_1");
        assert_eq!(out["output"], "sunny, 21C");
    }

    #[test]
    fn reasoning_and_temperature_are_conditional() {
        let p = test_provider();
        let req = ChatRequest::builder()
            .message(Message::user("hi"))
            .temperature(0.3)
            .provider_options(json!({"reasoning_effort": "high"}))
            .build();
        let body = p.build_responses_body(&req);

        // `temperature` is an f32 on ChatRequest; serde_json widens it to f64
        // so compare against the same widening rather than the f64 literal 0.3.
        assert_eq!(body["temperature"], json!(0.3_f32));
        assert_eq!(body["reasoning"]["effort"], json!("high"));
        assert_eq!(body["reasoning"]["summary"], json!("auto"));
    }

    mod adapter_tests {
        use super::super::ChatGptCodexStreamAdapter;
        use crate::types::{StopReason, StreamEvent, StreamEventType};
        use serde_json::Value;
        use std::collections::{HashSet, VecDeque};

        /// A fresh adapter wired to an empty inner stream — `handle_event` is
        /// driven directly, so the `inner` stream is never polled in these
        /// mapping tests.
        fn fresh_adapter() -> ChatGptCodexStreamAdapter {
            ChatGptCodexStreamAdapter {
                inner: Box::pin(tokio_stream::iter(Vec::new())),
                pending: VecDeque::new(),
                seen_tool_ids: HashSet::new(),
                saw_tool_call: false,
                error: None,
            }
        }

        /// Feed each `data:` JSON line through `handle_event`, draining the
        /// queued [`StreamEvent`]s after every frame.
        fn drive(adapter: &mut ChatGptCodexStreamAdapter, frames: &[&str]) -> Vec<StreamEvent> {
            let mut out = Vec::new();
            for frame in frames {
                let value: Value = serde_json::from_str(frame).expect("valid frame json");
                adapter.handle_event(&value);
                while let Some(ev) = adapter.pending.pop_front() {
                    out.push(ev);
                }
            }
            out
        }

        /// Real text-delta frames captured from the live backend
        /// (`/tmp/codex-sse-fixture.txt`), plus a complete `response.completed`
        /// frame (the captured one is truncated before `usage`, so the
        /// `usage`/`status` fields here follow the OpenAI Responses shape the
        /// plan's mapping table documents: `response.usage.input_tokens` /
        /// `output_tokens`, `response.status`).
        const TEXT_FRAMES: &[&str] = &[
            r#"{"type":"response.created","response":{"id":"resp_1","status":"in_progress"}}"#,
            r#"{"type":"response.output_text.delta","content_index":0,"delta":"Hi","item_id":"msg_1","output_index":1}"#,
            r#"{"type":"response.output_text.delta","content_index":0,"delta":" there","item_id":"msg_1","output_index":1}"#,
            r#"{"type":"response.output_text.delta","content_index":0,"delta":",","item_id":"msg_1","output_index":1}"#,
            r#"{"type":"response.output_text.delta","content_index":0,"delta":" friend","item_id":"msg_1","output_index":1}"#,
            r#"{"type":"response.output_text.done","content_index":0,"item_id":"msg_1","text":"Hi there, friend"}"#,
            r#"{"type":"response.completed","response":{"id":"resp_1","status":"completed","usage":{"input_tokens":12,"output_tokens":5}}}"#,
        ];

        #[test]
        fn adapter_emits_text_and_done() {
            let mut adapter = fresh_adapter();
            let events = drive(&mut adapter, TEXT_FRAMES);

            let text: String = events
                .iter()
                .filter(|e| e.event_type == StreamEventType::Text)
                .map(|e| e.content.as_str())
                .collect();
            assert_eq!(text, "Hi there, friend");

            let done = events.iter().find(|e| e.done).expect("a done event");
            assert_eq!(done.stop_reason, Some(StopReason::EndTurn));
        }

        #[test]
        fn adapter_emits_usage_from_response_completed() {
            let mut adapter = fresh_adapter();
            let events = drive(&mut adapter, TEXT_FRAMES);

            let usage = events
                .iter()
                .find(|e| e.event_type == StreamEventType::Usage)
                .and_then(|e| e.usage.clone())
                .expect("a usage event");
            assert_eq!(usage.input_tokens, 12);
            assert_eq!(usage.output_tokens, 5);
        }

        #[test]
        fn adapter_maps_reasoning_delta_to_thinking() {
            let mut adapter = fresh_adapter();
            let events = drive(
                &mut adapter,
                &[
                    r#"{"type":"response.reasoning_text.delta","delta":"think "}"#,
                    r#"{"type":"response.reasoning_summary_text.delta","delta":"more"}"#,
                ],
            );
            let thinking: String = events
                .iter()
                .filter(|e| e.event_type == StreamEventType::ThinkingDelta)
                .map(|e| e.content.as_str())
                .collect();
            assert_eq!(thinking, "think more");
        }

        #[test]
        fn adapter_handles_function_call_lifecycle() {
            // Synthetic added/args.delta/done triple for a function_call item.
            let mut adapter = fresh_adapter();
            let events = drive(
                &mut adapter,
                &[
                    r#"{"type":"response.output_item.added","item":{"type":"function_call","call_id":"call_42","name":"get_weather"}}"#,
                    r#"{"type":"response.function_call_arguments.delta","item_id":"call_42","delta":"{\"city\":"}"#,
                    r#"{"type":"response.function_call_arguments.delta","item_id":"call_42","delta":"\"Paris\"}"}"#,
                    r#"{"type":"response.output_item.done","item":{"type":"function_call","call_id":"call_42","name":"get_weather"}}"#,
                    r#"{"type":"response.completed","response":{"status":"completed","usage":{"input_tokens":3,"output_tokens":7}}}"#,
                ],
            );

            let start = events
                .iter()
                .find(|e| e.event_type == StreamEventType::ToolCallStart)
                .expect("tool_call_start");
            assert_eq!(start.tool_call_id.as_deref(), Some("call_42"));
            assert_eq!(start.tool_call_name.as_deref(), Some("get_weather"));

            let args: String = events
                .iter()
                .filter(|e| e.event_type == StreamEventType::ToolCallArgs)
                .filter_map(|e| e.tool_call_args_delta.clone())
                .collect();
            assert_eq!(args, r#"{"city":"Paris"}"#);

            let end = events
                .iter()
                .find(|e| e.event_type == StreamEventType::ToolCallEnd)
                .expect("tool_call_end");
            assert_eq!(end.tool_call_id.as_deref(), Some("call_42"));

            // Any tool call seen => terminal stop reason is ToolUse, not EndTurn.
            let done = events.iter().find(|e| e.done).expect("a done event");
            assert_eq!(done.stop_reason, Some(StopReason::ToolUse));
        }

        #[test]
        fn adapter_maps_incomplete_to_max_tokens() {
            let mut adapter = fresh_adapter();
            let events = drive(
                &mut adapter,
                &[r#"{"type":"response.completed","response":{"status":"incomplete","usage":{"input_tokens":1,"output_tokens":1}}}"#],
            );
            let done = events.iter().find(|e| e.done).expect("a done event");
            assert_eq!(done.stop_reason, Some(StopReason::MaxTokens));
        }

        #[tokio::test]
        async fn adapter_surfaces_top_level_error() {
            use tokio_stream::StreamExt as _;

            let frame =
                r#"{"type":"error","message":"rate limited","code":"rate_limit_exceeded"}"#;
            let items = vec![Ok(eventsource_stream::Event {
                event: String::new(),
                data: frame.to_string(),
                id: String::new(),
                retry: None,
            })];
            let mut adapter = ChatGptCodexStreamAdapter {
                inner: Box::pin(tokio_stream::iter(items)),
                pending: VecDeque::new(),
                seen_tool_ids: HashSet::new(),
                saw_tool_call: false,
                error: None,
            };

            let item = adapter.next().await.expect("one item");
            match item {
                Err(crate::error::MotosanError::Stream(msg)) => assert!(msg.contains("rate limited")),
                other => panic!("expected Stream error, got {other:?}"),
            }
        }
    }
}
