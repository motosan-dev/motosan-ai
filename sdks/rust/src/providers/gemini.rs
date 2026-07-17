use std::sync::atomic::{AtomicU64, Ordering};

use crate::error::MotosanError;
use crate::models::DEFAULT_GEMINI_MODEL;
use crate::providers::{
    extract_error_message, extract_request_id, map_http_error, parse_retry_after, send_with_retry,
    ChatResponseBuilder, ProviderImpl,
};
use crate::retry::RetryPolicy;
use crate::stream::BoxStream;
use crate::types::{
    ChatRequest, ChatResponse, ContentBlock, ImageSource, Role, StopReason, StreamEvent, ToolCall,
    ToolChoice, Usage,
};
use async_trait::async_trait;
use eventsource_stream::Eventsource;
use futures_core::Stream;
use reqwest::Client;
use serde_json::{json, Value};
use std::collections::VecDeque;
use std::pin::Pin;
use std::task::Poll;

const DEFAULT_MAX_TOKENS: u32 = 8192;
const BASE_URL: &str = "https://generativelanguage.googleapis.com/v1beta";

static TOOL_CALL_COUNTER: AtomicU64 = AtomicU64::new(0);

fn gen_tool_call_id() -> String {
    let n = TOOL_CALL_COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("call_{n}")
}

#[derive(Debug, Clone)]
pub struct GeminiProvider {
    http: Client,
    api_key: String,
    model: String,
    base_url: String,
    retry_policy: RetryPolicy,
}

impl GeminiProvider {
    pub fn new(
        api_key: impl Into<String>,
        model: Option<String>,
        base_url: Option<String>,
    ) -> Self {
        Self {
            http: Client::new(),
            api_key: api_key.into(),
            model: model.unwrap_or_else(|| DEFAULT_GEMINI_MODEL.to_string()),
            base_url: base_url.unwrap_or_else(|| BASE_URL.to_string()),
            retry_policy: RetryPolicy::default(),
        }
    }

    pub fn with_retry_policy(mut self, policy: RetryPolicy) -> Self {
        self.retry_policy = policy;
        self
    }

    /// Replace the internal `reqwest::Client` with a caller-supplied one.
    /// `ClientBuilder::build()` uses this to hand every HTTP provider one
    /// shared, connect-timeout-configured client so all providers share a
    /// single connection pool instead of each constructing their own.
    pub fn with_http_client(mut self, http: Client) -> Self {
        self.http = http;
        self
    }

    fn generate_url(&self, req: &ChatRequest) -> String {
        let model = req.model.as_deref().unwrap_or(&self.model);
        format!("{}/models/{}:generateContent", self.base_url, model)
    }

    fn stream_url(&self, req: &ChatRequest) -> String {
        let model = req.model.as_deref().unwrap_or(&self.model);
        format!(
            "{}/models/{}:streamGenerateContent?alt=sse",
            self.base_url, model
        )
    }

    fn apply_auth(&self, req: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        req.header("x-goog-api-key", &self.api_key)
    }

    pub(crate) fn build_request(req: &ChatRequest) -> Value {
        let mut contents: Vec<Value> = Vec::new();
        let mut extracted_system: Option<String> = None;

        for message in &req.messages {
            match message.role {
                Role::System => {
                    extracted_system = Some(message.content.clone());
                }
                Role::User => {
                    let mut parts: Vec<Value> = Vec::new();
                    if !message.content.is_empty() {
                        parts.push(json!({"text": message.content}));
                    }
                    for block in &message.content_blocks {
                        match block {
                            ContentBlock::Text { text } => {
                                parts.push(json!({"text": text}));
                            }
                            ContentBlock::Image { source } => match source {
                                ImageSource::Base64 { media_type, data } => {
                                    parts.push(json!({
                                        "inlineData": {
                                            "mimeType": media_type,
                                            "data": data
                                        }
                                    }));
                                }
                                ImageSource::Url { url } => {
                                    parts.push(json!({"fileData": {"fileUri": url}}));
                                }
                            },
                            ContentBlock::Document { .. } => unreachable!("document block reached Gemini serializer; caller skipped validate_request"),
                        }
                    }
                    if parts.is_empty() {
                        parts.push(json!({"text": ""}));
                    }
                    contents.push(json!({"role": "user", "parts": parts}));
                }
                Role::Assistant => {
                    let mut parts: Vec<Value> = Vec::new();
                    if !message.content.is_empty() {
                        parts.push(json!({"text": message.content}));
                    }
                    for tc in &message.tool_calls {
                        parts.push(json!({
                            "functionCall": {
                                "name": tc.name,
                                "args": tc.input
                            }
                        }));
                    }
                    if parts.is_empty() {
                        parts.push(json!({"text": ""}));
                    }
                    contents.push(json!({"role": "model", "parts": parts}));
                }
                Role::Tool => {
                    // tool_call_id holds the function name in this SDK's convention
                    let name = message.tool_call_id.as_deref().unwrap_or("");
                    let response: Value = serde_json::from_str(&message.content)
                        .unwrap_or_else(|_| json!({"result": message.content}));
                    contents.push(json!({
                        "role": "user",
                        "parts": [{"functionResponse": {"name": name, "response": response}}]
                    }));
                }
            }
        }

        let system_text = if let Some(ref blocks) = req.system_blocks {
            let joined: String = blocks
                .iter()
                .map(|b| b.text.as_str())
                .collect::<Vec<_>>()
                .join("\n");
            if joined.is_empty() {
                req.system
                    .as_deref()
                    .or(extracted_system.as_deref())
                    .unwrap_or("")
                    .to_string()
            } else {
                joined
            }
        } else {
            req.system
                .as_deref()
                .or(extracted_system.as_deref())
                .unwrap_or("")
                .to_string()
        };

        let max_tokens = req.max_tokens.unwrap_or(DEFAULT_MAX_TOKENS);
        let mut gen_config = json!({"maxOutputTokens": max_tokens});
        if let Some(temp) = req.temperature {
            gen_config["temperature"] = json!(temp);
        }
        if let Some(ref stops) = req.stop_sequences {
            if !stops.is_empty() {
                gen_config["stopSequences"] = json!(stops);
            }
        }

        let mut body = json!({
            "contents": contents,
            "generationConfig": gen_config,
        });

        if !system_text.is_empty() {
            body["systemInstruction"] = json!({"parts": [{"text": system_text}]});
        }

        if let Some(ref tools) = req.tools {
            if !tools.is_empty() {
                let declarations: Vec<Value> = tools
                    .iter()
                    .map(|t| {
                        let mut decl = json!({
                            "name": t.name,
                            "description": t.description,
                        });
                        decl["parameters"] = t.input_schema.clone();
                        decl
                    })
                    .collect();
                body["tools"] = json!([{"functionDeclarations": declarations}]);

                let mode = match &req.tool_choice {
                    None | Some(ToolChoice::Auto) => "AUTO",
                    Some(ToolChoice::Required) => "ANY",
                    Some(ToolChoice::None) => {
                        body.as_object_mut().map(|m| m.remove("tools"));
                        "NONE"
                    }
                    Some(ToolChoice::Tool { .. }) => "ANY",
                };
                if mode != "NONE" {
                    let mut fc_config = json!({"mode": mode});
                    if let Some(ToolChoice::Tool { name }) = &req.tool_choice {
                        fc_config["allowedFunctionNames"] = json!([name]);
                    }
                    body["toolConfig"] = json!({"functionCallingConfig": fc_config});
                }
            }
        }

        if let Some(po) = &req.provider_options {
            if let Some(map) = po.as_object() {
                for (k, v) in map {
                    body[k] = v.clone();
                }
            }
        }

        body
    }

    pub(crate) fn parse_response(payload: &Value, default_model: &str) -> ChatResponse {
        let candidate = payload
            .get("candidates")
            .and_then(|c| c.get(0))
            .cloned()
            .unwrap_or(json!({}));

        let parts = candidate
            .get("content")
            .and_then(|c| c.get("parts"))
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();

        let mut text = String::new();
        let mut tool_calls: Vec<ToolCall> = Vec::new();

        for part in &parts {
            if let Some(t) = part.get("text").and_then(Value::as_str) {
                text.push_str(t);
            }
            if let Some(fc) = part.get("functionCall") {
                let name = fc
                    .get("name")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string();
                let input = fc.get("args").cloned().unwrap_or(json!({}));
                tool_calls.push(ToolCall {
                    id: gen_tool_call_id(),
                    name,
                    input,
                });
            }
        }

        let finish_reason = candidate
            .get("finishReason")
            .and_then(Value::as_str)
            .unwrap_or("STOP");

        let stop_reason = match finish_reason {
            "STOP" if !tool_calls.is_empty() => StopReason::ToolUse,
            "STOP" => StopReason::EndTurn,
            "MAX_TOKENS" => StopReason::MaxTokens,
            _ if !tool_calls.is_empty() => StopReason::ToolUse,
            _ => StopReason::Other,
        };

        let usage_meta = payload.get("usageMetadata").cloned().unwrap_or(json!({}));
        let input_tokens = usage_meta
            .get("promptTokenCount")
            .and_then(Value::as_u64)
            .unwrap_or(0) as u32;
        let output_tokens = usage_meta
            .get("candidatesTokenCount")
            .and_then(Value::as_u64)
            .unwrap_or(0) as u32;

        let model = payload
            .get("modelVersion")
            .and_then(Value::as_str)
            .unwrap_or(default_model)
            .to_string();

        ChatResponseBuilder::new(model)
            .content(text)
            .tool_calls(tool_calls)
            .stop_reason(stop_reason)
            .usage(input_tokens, output_tokens)
            .build()
    }
}

#[async_trait]
impl ProviderImpl for GeminiProvider {
    fn capabilities(&self) -> crate::types::ProviderCapabilities {
        crate::types::ProviderCapabilities::with_image()
    }

    async fn chat(&self, req: ChatRequest) -> Result<ChatResponse, MotosanError> {
        let model = req.model.clone().unwrap_or_else(|| self.model.clone());
        let url = self.generate_url(&req);
        let body = Self::build_request(&req);

        let response = send_with_retry(&self.retry_policy, || {
            self.apply_auth(
                self.http
                    .post(&url)
                    .header("content-type", "application/json"),
            )
            .json(&body)
        })
        .await?;

        let status = response.status().as_u16();
        if status != 200 {
            let retry_after = parse_retry_after(response.headers());
            let request_id = extract_request_id(response.headers());
            let payload: Value = response.json().await.unwrap_or(json!({}));
            let msg = extract_error_message(&payload, "Gemini API error");
            return Err(map_http_error(status, msg, retry_after, request_id));
        }

        let payload: Value = response
            .json()
            .await
            .map_err(|e| MotosanError::ProviderError {
                message: format!("failed to parse Gemini response: {e}"),
                status_code: None,
                retry_after: None,
                request_id: None,
            })?;
        Ok(Self::parse_response(&payload, &model))
    }

    async fn stream(&self, req: ChatRequest) -> Result<BoxStream, MotosanError> {
        let url = self.stream_url(&req);
        let body = Self::build_request(&req);

        let response = send_with_retry(&self.retry_policy, || {
            self.apply_auth(
                self.http
                    .post(&url)
                    .header("content-type", "application/json"),
            )
            .json(&body)
        })
        .await?;

        let status = response.status().as_u16();
        if status != 200 {
            let retry_after = parse_retry_after(response.headers());
            let request_id = extract_request_id(response.headers());
            let payload: Value = response.json().await.unwrap_or(json!({}));
            let msg = extract_error_message(&payload, "Gemini stream error");
            return Err(map_http_error(status, msg, retry_after, request_id));
        }

        let sse = response.bytes_stream().eventsource();
        let adapter = GeminiStreamAdapter {
            inner: Box::pin(sse),
            pending: VecDeque::new(),
            saw_terminal: false,
        };
        Ok(Box::pin(adapter))
    }
}

struct GeminiStreamAdapter {
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
    saw_terminal: bool,
}

impl Stream for GeminiStreamAdapter {
    type Item = Result<StreamEvent, MotosanError>;

    fn poll_next(
        mut self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> Poll<Option<Self::Item>> {
        if let Some(ev) = self.pending.pop_front() {
            return Poll::Ready(Some(Ok(ev)));
        }

        loop {
            match self.inner.as_mut().poll_next(cx) {
                Poll::Ready(Some(Ok(event))) => {
                    if event.data.trim() == "[DONE]" || event.data.trim().is_empty() {
                        continue;
                    }
                    let payload: Value = match serde_json::from_str(&event.data) {
                        Ok(v) => v,
                        Err(_) => continue,
                    };

                    let candidate = match payload.get("candidates").and_then(|c| c.get(0)) {
                        Some(c) => c.clone(),
                        None => continue,
                    };

                    let parts = candidate
                        .get("content")
                        .and_then(|c| c.get("parts"))
                        .and_then(Value::as_array)
                        .cloned()
                        .unwrap_or_default();

                    let finish_reason = candidate.get("finishReason").and_then(Value::as_str);

                    let mut has_tool_calls = false;

                    for part in &parts {
                        if let Some(text) = part.get("text").and_then(Value::as_str) {
                            if !text.is_empty() {
                                self.pending.push_back(StreamEvent::text(text));
                            }
                        }
                        if let Some(fc) = part.get("functionCall") {
                            has_tool_calls = true;
                            let name = fc
                                .get("name")
                                .and_then(Value::as_str)
                                .unwrap_or("")
                                .to_string();
                            let args = fc.get("args").cloned().unwrap_or(json!({}));
                            let args_str = args.to_string();
                            let id = gen_tool_call_id();
                            self.pending
                                .push_back(StreamEvent::tool_call_start(&id, &name));
                            self.pending
                                .push_back(StreamEvent::tool_call_args_with_id(&id, &args_str));
                            self.pending
                                .push_back(StreamEvent::tool_call_end_with_id(id));
                        }
                    }

                    if let Some(usage_meta) = payload.get("usageMetadata") {
                        let input_tokens = usage_meta
                            .get("promptTokenCount")
                            .and_then(Value::as_u64)
                            .unwrap_or(0) as u32;
                        let output_tokens = usage_meta
                            .get("candidatesTokenCount")
                            .and_then(Value::as_u64)
                            .unwrap_or(0) as u32;
                        self.pending.push_back(StreamEvent::usage(Usage {
                            input_tokens,
                            output_tokens,
                            cache_creation_input_tokens: None,
                            cache_read_input_tokens: None,
                        }));
                    }

                    if let Some(reason) = finish_reason {
                        self.saw_terminal = true;
                        let stop_reason = match reason {
                            "STOP" if has_tool_calls => StopReason::ToolUse,
                            "STOP" => StopReason::EndTurn,
                            "MAX_TOKENS" => StopReason::MaxTokens,
                            _ if has_tool_calls => StopReason::ToolUse,
                            _ => StopReason::Other,
                        };
                        self.pending
                            .push_back(StreamEvent::done_with_stop_reason(stop_reason));
                    }

                    if let Some(first) = self.pending.pop_front() {
                        return Poll::Ready(Some(Ok(first)));
                    }
                    continue;
                }
                Poll::Ready(Some(Err(e))) => {
                    self.saw_terminal = true;
                    return Poll::Ready(Some(Err(MotosanError::Stream(e.to_string()))));
                }
                Poll::Ready(None) => {
                    if !self.saw_terminal {
                        self.saw_terminal = true;
                        return Poll::Ready(Some(Err(MotosanError::IncompleteStream(
                            "gemini ended without a terminal event".to_string(),
                        ))));
                    }
                    return Poll::Ready(None);
                }
                Poll::Pending => return Poll::Pending,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{ContentBlock, ImageSource, Message, SystemBlock, Tool, ToolCall};

    fn user_msg(text: &str) -> Message {
        Message::user(text)
    }

    fn assistant_msg(text: &str) -> Message {
        Message::assistant(text)
    }

    fn build(msgs: Vec<Message>) -> Value {
        let req = ChatRequest::builder().messages(msgs).build();
        GeminiProvider::build_request(&req)
    }

    #[test]
    fn capabilities_support_image_only() {
        let p = GeminiProvider::new("key", None, None);
        let caps = p.capabilities();
        assert!(caps.supports_image);
        assert!(!caps.supports_document);
    }

    #[tokio::test]
    async fn adapter_surfaces_inner_stream_error() {
        use eventsource_stream::EventStreamError;
        use tokio_stream::StreamExt;

        let utf8 = String::from_utf8(vec![0xff]).unwrap_err();
        let inner = tokio_stream::iter(vec![Err(EventStreamError::Utf8(utf8))]);
        let mut adapter = GeminiStreamAdapter {
            inner: Box::pin(inner),
            pending: VecDeque::new(),
            saw_terminal: false,
        };

        let item = adapter.next().await.expect("one item");
        assert!(matches!(item, Err(MotosanError::Stream(_))));
    }

    #[test]
    fn simple_user_message() {
        let body = build(vec![user_msg("Hello")]);
        assert_eq!(body["contents"][0]["role"], "user");
        assert_eq!(body["contents"][0]["parts"][0]["text"], "Hello");
    }

    #[test]
    fn assistant_message_becomes_model_role() {
        let body = build(vec![user_msg("Hi"), assistant_msg("Hello back")]);
        assert_eq!(body["contents"][1]["role"], "model");
        assert_eq!(body["contents"][1]["parts"][0]["text"], "Hello back");
    }

    #[test]
    fn system_message_extracted_to_system_instruction() {
        let req = ChatRequest::builder()
            .messages(vec![user_msg("Hi")])
            .system("Be concise.")
            .build();
        let body = GeminiProvider::build_request(&req);
        assert_eq!(body["systemInstruction"]["parts"][0]["text"], "Be concise.");
        assert_eq!(body["contents"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn tool_result_message_becomes_function_response() {
        let tool_msg = Message::tool_result("get_weather", r#"{"result": "sunny"}"#);
        let body = build(vec![user_msg("?"), tool_msg]);
        let part = &body["contents"][1]["parts"][0];
        assert_eq!(part["functionResponse"]["name"], "get_weather");
        assert_eq!(part["functionResponse"]["response"]["result"], "sunny");
    }

    #[test]
    fn tool_result_without_id_uses_empty_name() {
        let tool_msg = Message::tool_result("", "done");
        let body = build(vec![user_msg("?"), tool_msg]);
        let part = &body["contents"][1]["parts"][0];
        assert_eq!(part["functionResponse"]["name"], "");
        assert_eq!(part["functionResponse"]["response"]["result"], "done");
    }

    #[test]
    fn tool_choice_required_maps_to_any() {
        let tool = Tool {
            schema: motosan_agent_primitives::ToolSchema {
                name: "search".to_string(),
                description: "Search".to_string(),
                input_schema: json!({"type": "object", "properties": {}}),
            },
            cache: false,
        };
        let req = ChatRequest::builder()
            .messages(vec![user_msg("find it")])
            .tools(vec![tool])
            .tool_choice(ToolChoice::Required)
            .build();
        let body = GeminiProvider::build_request(&req);
        assert_eq!(body["toolConfig"]["functionCallingConfig"]["mode"], "ANY");
    }

    #[test]
    fn tool_choice_none_removes_tools() {
        let tool = Tool {
            schema: motosan_agent_primitives::ToolSchema {
                name: "search".to_string(),
                description: String::new(),
                input_schema: serde_json::json!({"type":"object"}),
            },
            cache: false,
        };
        let req = ChatRequest::builder()
            .messages(vec![user_msg("hi")])
            .tools(vec![tool])
            .tool_choice(ToolChoice::None)
            .build();
        let body = GeminiProvider::build_request(&req);
        assert!(body.get("tools").is_none());
    }

    #[test]
    fn temperature_included_when_set() {
        let req = ChatRequest::builder()
            .messages(vec![user_msg("hi")])
            .temperature(0.3)
            .build();
        let body = GeminiProvider::build_request(&req);
        let temp = body["generationConfig"]["temperature"]
            .as_f64()
            .expect("temperature should be a number");
        assert!((temp - 0.3).abs() < 1e-6, "expected ~0.3, got {temp}");
    }

    #[test]
    fn parse_text_response() {
        let raw = json!({
            "candidates": [{
                "content": {"parts": [{"text": "Hello world"}], "role": "model"},
                "finishReason": "STOP"
            }],
            "usageMetadata": {"promptTokenCount": 10, "candidatesTokenCount": 5},
            "modelVersion": "gemini-2.5-flash"
        });
        let resp = GeminiProvider::parse_response(&raw, DEFAULT_GEMINI_MODEL);
        assert_eq!(resp.content, "Hello world");
        assert_eq!(resp.tool_calls.len(), 0);
        assert_eq!(resp.stop_reason, StopReason::EndTurn);
        assert_eq!(resp.usage.input_tokens, 10);
        assert_eq!(resp.usage.output_tokens, 5);
        assert_eq!(resp.model, "gemini-2.5-flash");
    }

    #[test]
    fn parse_tool_call_response() {
        let raw = json!({
            "candidates": [{
                "content": {
                    "parts": [{"functionCall": {"name": "search", "args": {"q": "rust"}}}],
                    "role": "model"
                },
                "finishReason": "STOP"
            }],
            "usageMetadata": {"promptTokenCount": 8, "candidatesTokenCount": 3},
            "modelVersion": "gemini-2.5-flash"
        });
        let resp = GeminiProvider::parse_response(&raw, DEFAULT_GEMINI_MODEL);
        assert_eq!(resp.tool_calls.len(), 1);
        assert_eq!(resp.tool_calls[0].name, "search");
        assert_eq!(resp.tool_calls[0].input["q"], "rust");
        assert_eq!(resp.stop_reason, StopReason::ToolUse);
    }

    #[test]
    fn parse_max_tokens_stop_reason() {
        let raw = json!({
            "candidates": [{
                "content": {"parts": [{"text": "truncated"}], "role": "model"},
                "finishReason": "MAX_TOKENS"
            }],
            "usageMetadata": {"promptTokenCount": 5, "candidatesTokenCount": 100}
        });
        let resp = GeminiProvider::parse_response(&raw, DEFAULT_GEMINI_MODEL);
        assert_eq!(resp.stop_reason, StopReason::MaxTokens);
    }

    // ---- build_request: additional coverage ----

    #[test]
    fn image_base64_block_serialized_as_inline_data() {
        // Only image block → content is empty → one part: the inlineData
        let msg = Message::user_with_blocks(vec![ContentBlock::Image {
            source: ImageSource::Base64 {
                media_type: "image/png".into(),
                data: "abc123".into(),
            },
        }]);
        let body = build(vec![user_msg("look at this"), msg]);
        let parts = &body["contents"][1]["parts"];
        assert_eq!(parts[0]["inlineData"]["mimeType"], "image/png");
        assert_eq!(parts[0]["inlineData"]["data"], "abc123");
    }

    #[test]
    fn image_url_block_serialized_as_file_data() {
        let msg = Message::user_with_blocks(vec![ContentBlock::Image {
            source: ImageSource::Url {
                url: "https://example.com/img.jpg".into(),
            },
        }]);
        let body = build(vec![msg]);
        let part = &body["contents"][0]["parts"][0];
        assert_eq!(part["fileData"]["fileUri"], "https://example.com/img.jpg");
    }

    #[test]
    fn multiple_tools_become_function_declarations() {
        let tools = vec![
            Tool {
                schema: motosan_agent_primitives::ToolSchema {
                    name: "search".into(),
                    description: "Search".into(),
                    input_schema: serde_json::json!({"type":"object"}),
                },
                cache: false,
            },
            Tool {
                schema: motosan_agent_primitives::ToolSchema {
                    name: "calc".into(),
                    description: "Calculate".into(),
                    input_schema: serde_json::json!({"type":"object"}),
                },
                cache: false,
            },
        ];
        let req = ChatRequest::builder()
            .messages(vec![user_msg("hi")])
            .tools(tools)
            .build();
        let body = GeminiProvider::build_request(&req);
        let declarations = &body["tools"][0]["functionDeclarations"];
        assert_eq!(declarations.as_array().unwrap().len(), 2);
        assert_eq!(declarations[0]["name"], "search");
        assert_eq!(declarations[1]["name"], "calc");
    }

    #[test]
    fn tool_input_schema_included_in_declaration() {
        let schema =
            json!({"type": "object", "properties": {"q": {"type": "string"}}, "required": ["q"]});
        let tool = Tool {
            schema: motosan_agent_primitives::ToolSchema {
                name: "search".into(),
                description: String::new(),
                input_schema: schema.clone(),
            },
            cache: false,
        };
        let req = ChatRequest::builder()
            .messages(vec![user_msg("find")])
            .tools(vec![tool])
            .build();
        let body = GeminiProvider::build_request(&req);
        assert_eq!(
            body["tools"][0]["functionDeclarations"][0]["parameters"],
            schema
        );
    }

    #[test]
    fn tool_choice_auto_maps_to_auto_mode() {
        let tool = Tool {
            schema: motosan_agent_primitives::ToolSchema {
                name: "t".into(),
                description: String::new(),
                input_schema: serde_json::json!({"type":"object"}),
            },
            cache: false,
        };
        let req = ChatRequest::builder()
            .messages(vec![user_msg("hi")])
            .tools(vec![tool])
            .tool_choice(ToolChoice::Auto)
            .build();
        let body = GeminiProvider::build_request(&req);
        assert_eq!(body["toolConfig"]["functionCallingConfig"]["mode"], "AUTO");
    }

    #[test]
    fn tool_choice_specific_tool_sets_allowed_function_names() {
        let tool = Tool {
            schema: motosan_agent_primitives::ToolSchema {
                name: "special".into(),
                description: String::new(),
                input_schema: serde_json::json!({"type":"object"}),
            },
            cache: false,
        };
        let req = ChatRequest::builder()
            .messages(vec![user_msg("hi")])
            .tools(vec![tool])
            .tool_choice(ToolChoice::Tool {
                name: "special".into(),
            })
            .build();
        let body = GeminiProvider::build_request(&req);
        let fc = &body["toolConfig"]["functionCallingConfig"];
        assert_eq!(fc["mode"], "ANY");
        assert_eq!(fc["allowedFunctionNames"][0], "special");
    }

    #[test]
    fn stop_sequences_in_generation_config() {
        let req = ChatRequest::builder()
            .messages(vec![user_msg("hi")])
            .stop_sequences(vec!["END".into(), "STOP".into()])
            .build();
        let body = GeminiProvider::build_request(&req);
        let stops = &body["generationConfig"]["stopSequences"];
        assert_eq!(stops[0], "END");
        assert_eq!(stops[1], "STOP");
    }

    #[test]
    fn max_tokens_override_in_generation_config() {
        let req = ChatRequest::builder()
            .messages(vec![user_msg("hi")])
            .max_tokens(256)
            .build();
        let body = GeminiProvider::build_request(&req);
        assert_eq!(body["generationConfig"]["maxOutputTokens"], 256);
    }

    #[test]
    fn provider_options_merged_into_body() {
        let req = ChatRequest::builder()
            .messages(vec![user_msg("hi")])
            .provider_options(
                json!({"safetySettings": [{"category": "ALL", "threshold": "BLOCK_NONE"}]}),
            )
            .build();
        let body = GeminiProvider::build_request(&req);
        assert!(body.get("safetySettings").is_some());
    }

    #[test]
    fn system_blocks_joined_into_system_instruction() {
        let req = ChatRequest::builder()
            .messages(vec![user_msg("hi")])
            .system_blocks(vec![
                SystemBlock::new("Block 1."),
                SystemBlock::new("Block 2."),
            ])
            .build();
        let body = GeminiProvider::build_request(&req);
        let text = body["systemInstruction"]["parts"][0]["text"]
            .as_str()
            .unwrap();
        assert_eq!(text, "Block 1.\nBlock 2.");
    }

    #[test]
    fn system_blocks_take_priority_over_system_field() {
        let req = ChatRequest::builder()
            .messages(vec![user_msg("hi")])
            .system("fallback")
            .system_blocks(vec![SystemBlock::new("from blocks")])
            .build();
        let body = GeminiProvider::build_request(&req);
        let text = body["systemInstruction"]["parts"][0]["text"]
            .as_str()
            .unwrap();
        assert_eq!(text, "from blocks");
        assert!(!text.contains("fallback"));
    }

    #[test]
    fn empty_system_blocks_falls_back_to_system_field() {
        let req = ChatRequest::builder()
            .messages(vec![user_msg("hi")])
            .system("use this")
            .system_blocks(vec![])
            .build();
        let body = GeminiProvider::build_request(&req);
        let text = body["systemInstruction"]["parts"][0]["text"]
            .as_str()
            .unwrap();
        assert_eq!(text, "use this");
    }

    #[test]
    fn assistant_with_text_and_tool_calls() {
        let tc = ToolCall {
            id: "c1".into(),
            name: "foo".into(),
            input: json!({"x": 1}),
        };
        let msg = Message::assistant_with_tool_calls("Let me check.", vec![tc]);
        let body = build(vec![user_msg("go"), msg]);
        let parts = &body["contents"][1]["parts"];
        assert_eq!(body["contents"][1]["role"], "model");
        assert_eq!(parts[0]["text"], "Let me check.");
        assert_eq!(parts[1]["functionCall"]["name"], "foo");
        assert_eq!(parts[1]["functionCall"]["args"]["x"], 1);
    }

    #[test]
    fn empty_tools_list_omits_tools_key() {
        let req = ChatRequest::builder()
            .messages(vec![user_msg("hi")])
            .tools(vec![])
            .build();
        let body = GeminiProvider::build_request(&req);
        assert!(body.get("tools").is_none());
        assert!(body.get("toolConfig").is_none());
    }

    // ---- parse_response: additional coverage ----

    #[test]
    fn parse_multiple_tool_calls() {
        let raw = json!({
            "candidates": [{
                "content": {
                    "parts": [
                        {"functionCall": {"name": "search", "args": {"q": "a"}}},
                        {"functionCall": {"name": "calc", "args": {"n": 2}}}
                    ],
                    "role": "model"
                },
                "finishReason": "STOP"
            }],
            "usageMetadata": {"promptTokenCount": 5, "candidatesTokenCount": 5}
        });
        let resp = GeminiProvider::parse_response(&raw, DEFAULT_GEMINI_MODEL);
        assert_eq!(resp.tool_calls.len(), 2);
        assert_eq!(resp.tool_calls[0].name, "search");
        assert_eq!(resp.tool_calls[1].name, "calc");
        assert_eq!(resp.stop_reason, StopReason::ToolUse);
    }

    #[test]
    fn parse_tool_calls_have_unique_ids() {
        let raw = json!({
            "candidates": [{
                "content": {
                    "parts": [
                        {"functionCall": {"name": "a", "args": {}}},
                        {"functionCall": {"name": "b", "args": {}}}
                    ],
                    "role": "model"
                },
                "finishReason": "STOP"
            }],
            "usageMetadata": {"promptTokenCount": 1, "candidatesTokenCount": 1}
        });
        let resp = GeminiProvider::parse_response(&raw, DEFAULT_GEMINI_MODEL);
        assert_ne!(resp.tool_calls[0].id, resp.tool_calls[1].id);
        assert!(!resp.tool_calls[0].id.is_empty());
    }

    #[test]
    fn parse_mixed_text_and_tool_call() {
        let raw = json!({
            "candidates": [{
                "content": {
                    "parts": [
                        {"text": "Let me search for that."},
                        {"functionCall": {"name": "search", "args": {"q": "test"}}}
                    ],
                    "role": "model"
                },
                "finishReason": "STOP"
            }],
            "usageMetadata": {"promptTokenCount": 4, "candidatesTokenCount": 6}
        });
        let resp = GeminiProvider::parse_response(&raw, DEFAULT_GEMINI_MODEL);
        assert_eq!(resp.content, "Let me search for that.");
        assert_eq!(resp.tool_calls.len(), 1);
        assert_eq!(resp.stop_reason, StopReason::ToolUse);
    }

    #[test]
    fn parse_missing_model_version_uses_default() {
        let raw = json!({
            "candidates": [{
                "content": {"parts": [{"text": "hi"}], "role": "model"},
                "finishReason": "STOP"
            }],
            "usageMetadata": {"promptTokenCount": 1, "candidatesTokenCount": 1}
        });
        let resp = GeminiProvider::parse_response(&raw, "my-default-model");
        assert_eq!(resp.model, "my-default-model");
    }

    #[test]
    fn parse_missing_usage_metadata_yields_zero_tokens() {
        let raw = json!({
            "candidates": [{
                "content": {"parts": [{"text": "hi"}], "role": "model"},
                "finishReason": "STOP"
            }]
        });
        let resp = GeminiProvider::parse_response(&raw, DEFAULT_GEMINI_MODEL);
        assert_eq!(resp.usage.input_tokens, 0);
        assert_eq!(resp.usage.output_tokens, 0);
    }

    #[test]
    fn parse_safety_finish_reason_returns_other() {
        let raw = json!({
            "candidates": [{
                "content": {"parts": [{"text": ""}], "role": "model"},
                "finishReason": "SAFETY"
            }],
            "usageMetadata": {"promptTokenCount": 2, "candidatesTokenCount": 0}
        });
        let resp = GeminiProvider::parse_response(&raw, DEFAULT_GEMINI_MODEL);
        assert_eq!(resp.stop_reason, StopReason::Other);
    }

    #[test]
    fn parse_empty_candidates_array_returns_empty_response() {
        let raw = json!({"candidates": []});
        let resp = GeminiProvider::parse_response(&raw, DEFAULT_GEMINI_MODEL);
        assert_eq!(resp.content, "");
        assert_eq!(resp.tool_calls.len(), 0);
    }

    mod chat_tests {
        use super::*;

        #[tokio::test]
        async fn chat_returns_text_response() {
            let mut server = mockito::Server::new_async().await;
            let mock = server
                .mock(
                    "POST",
                    mockito::Matcher::Regex(r"generateContent".to_string()),
                )
                .with_status(200)
                .with_header("content-type", "application/json")
                .with_body(
                    r#"{
                        "candidates":[{"content":{"parts":[{"text":"Hi!"}],"role":"model"},"finishReason":"STOP"}],
                        "usageMetadata":{"promptTokenCount":5,"candidatesTokenCount":2},
                        "modelVersion":"gemini-2.5-flash"
                    }"#,
                )
                .create_async()
                .await;

            let provider = GeminiProvider::new("fake-key", None, Some(server.url()));
            let req = ChatRequest::builder()
                .messages(vec![Message::user("Hello")])
                .build();
            let resp = provider.chat(req).await.unwrap();
            assert_eq!(resp.content, "Hi!");
            assert_eq!(resp.stop_reason, StopReason::EndTurn);
            mock.assert_async().await;
        }

        #[tokio::test]
        async fn chat_retries_on_429() {
            let mut server = mockito::Server::new_async().await;
            let _m1 = server
                .mock(
                    "POST",
                    mockito::Matcher::Regex(r"generateContent".to_string()),
                )
                .with_status(429)
                .with_body(r#"{"error":{"message":"rate limited"}}"#)
                .create_async()
                .await;
            let _m2 = server
                .mock(
                    "POST",
                    mockito::Matcher::Regex(r"generateContent".to_string()),
                )
                .with_status(200)
                .with_body(
                    r#"{"candidates":[{"content":{"parts":[{"text":"ok"}],"role":"model"},"finishReason":"STOP"}],"usageMetadata":{"promptTokenCount":1,"candidatesTokenCount":1}}"#,
                )
                .create_async()
                .await;

            use crate::retry::RetryPolicy;
            let policy = RetryPolicy {
                max_retries: 2,
                base_delay_ms: 1,
                max_delay_ms: 10,
                jitter: false,
                respect_retry_after: false,
                on_retry: None,
            };
            let provider =
                GeminiProvider::new("fake-key", None, Some(server.url())).with_retry_policy(policy);
            let req = ChatRequest::builder()
                .messages(vec![Message::user("hi")])
                .build();
            let resp = provider.chat(req).await.unwrap();
            assert_eq!(resp.content, "ok");
        }

        #[tokio::test]
        async fn stream_emits_text_and_done() {
            let mut server = mockito::Server::new_async().await;
            let sse_body = concat!(
                "data: {\"candidates\":[{\"content\":{\"parts\":[{\"text\":\"Hi\"}],\"role\":\"model\"}}]}\n\n",
                "data: {\"candidates\":[{\"content\":{\"parts\":[{\"text\":\" there\"}],\"role\":\"model\"},\"finishReason\":\"STOP\"}],\"usageMetadata\":{\"promptTokenCount\":3,\"candidatesTokenCount\":2}}\n\n",
            );
            let mock = server
                .mock(
                    "POST",
                    mockito::Matcher::Regex(r"streamGenerateContent".to_string()),
                )
                .with_status(200)
                .with_header("content-type", "text/event-stream")
                .with_body(sse_body)
                .create_async()
                .await;

            let provider = GeminiProvider::new("fake-key", None, Some(server.url()));
            let req = ChatRequest::builder()
                .messages(vec![Message::user("Hello")])
                .build();
            let stream = provider.stream(req).await.unwrap();
            let resp = crate::stream::collect_stream(stream).await.unwrap();
            assert_eq!(resp.content, "Hi there");
            assert_eq!(resp.stop_reason, StopReason::EndTurn);
            mock.assert_async().await;
        }
    }
}
