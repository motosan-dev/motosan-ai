use crate::error::MotosanError;
use crate::models::DEFAULT_OPENAI_MODEL;
use crate::providers::{
    extract_error_message, is_retryable_network_error, is_retryable_status, map_http_error,
    parse_retry_after, reject_document_blocks, sleep_before_retry, ChatResponseBuilder,
    ProviderImpl,
};
use crate::retry::RetryPolicy;
use crate::stream::BoxStream;
use crate::types::{
    ChatRequest, ChatResponse, ContentBlock, ImageSource, Role, StopReason, StreamEvent, ToolCall,
    ToolChoice,
};

use async_trait::async_trait;
use eventsource_stream::Eventsource;
use futures_core::Stream;
use reqwest::Client;
use serde_json::{json, Value};
use std::pin::Pin;
use std::task::Poll;

#[derive(Debug, Clone)]
pub enum OpenAIAuthStyle {
    Bearer,
    XApiKey,
    Custom(String),
}

/// Default chat completions endpoint for OpenAI.
pub const DEFAULT_OPENAI_CHAT_URL: &str = "https://api.openai.com/v1/chat/completions";

/// Default Responses API endpoint for OpenAI.
pub const DEFAULT_OPENAI_RESPONSES_URL: &str = "https://api.openai.com/v1/responses";

pub struct OpenAIProvider {
    http: Client,
    api_key: String,
    model: String,
    /// Full URL POSTed for chat completions. Defaults to [`DEFAULT_OPENAI_CHAT_URL`].
    chat_url: String,
    /// Full URL POSTed for the Responses API fallback. Defaults to
    /// [`DEFAULT_OPENAI_RESPONSES_URL`]. Only used when
    /// [`with_responses_fallback`](Self::with_responses_fallback) is enabled.
    responses_url: String,
    auth_style: OpenAIAuthStyle,
    responses_fallback: bool,
    retry_policy: RetryPolicy,
}

impl OpenAIProvider {
    /// Create a provider pointing at the OpenAI defaults.
    ///
    /// Override the URLs via [`with_chat_url`](Self::with_chat_url) and/or
    /// [`with_responses_url`](Self::with_responses_url) to target
    /// OpenAI-compatible endpoints (Groq, DeepSeek, Ollama, self-hosted
    /// proxies, etc.).
    pub fn new(api_key: impl Into<String>, model: Option<String>) -> Self {
        Self {
            http: Client::new(),
            api_key: api_key.into(),
            model: model.unwrap_or_else(|| DEFAULT_OPENAI_MODEL.to_string()),
            chat_url: DEFAULT_OPENAI_CHAT_URL.to_string(),
            responses_url: DEFAULT_OPENAI_RESPONSES_URL.to_string(),
            auth_style: OpenAIAuthStyle::Bearer,
            responses_fallback: false,
            retry_policy: RetryPolicy::default(),
        }
    }

    pub fn with_auth_style(mut self, auth_style: OpenAIAuthStyle) -> Self {
        self.auth_style = auth_style;
        self
    }

    pub fn with_responses_fallback(mut self, enabled: bool) -> Self {
        self.responses_fallback = enabled;
        self
    }

    pub fn with_retry_policy(mut self, retry_policy: RetryPolicy) -> Self {
        self.retry_policy = retry_policy;
        self
    }

    /// Override the chat completions URL.
    ///
    /// Pass the **full URL** that the provider should POST to, e.g.
    /// `https://api.groq.com/openai/v1/chat/completions`. Trailing slashes
    /// are trimmed defensively, but no other normalization is applied —
    /// what you pass is what gets hit.
    pub fn with_chat_url(mut self, url: impl Into<String>) -> Self {
        self.chat_url = url.into().trim_end_matches('/').to_string();
        self
    }

    /// Override the Responses API URL.
    ///
    /// Only relevant when [`with_responses_fallback`](Self::with_responses_fallback)
    /// is enabled. Most OpenAI-compatible providers do not expose this
    /// endpoint, so setting this is rarely needed outside of real OpenAI.
    pub fn with_responses_url(mut self, url: impl Into<String>) -> Self {
        self.responses_url = url.into().trim_end_matches('/').to_string();
        self
    }

    fn apply_auth(&self, req: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        match &self.auth_style {
            OpenAIAuthStyle::Bearer => {
                req.header("authorization", format!("Bearer {}", self.api_key))
            }
            OpenAIAuthStyle::XApiKey => req.header("x-api-key", self.api_key.clone()),
            OpenAIAuthStyle::Custom(header_name) => req.header(header_name, self.api_key.clone()),
        }
    }

    fn first_non_empty_text(value: Option<&Value>) -> Option<&str> {
        value
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|text| !text.is_empty())
    }

    fn extract_chat_content(payload: &Value) -> String {
        let message = payload
            .get("choices")
            .and_then(Value::as_array)
            .and_then(|choices| choices.first())
            .and_then(|choice| choice.get("message"));

        let content = Self::first_non_empty_text(message.and_then(|msg| msg.get("content")));
        let reasoning =
            Self::first_non_empty_text(message.and_then(|msg| msg.get("reasoning_content")));

        content.or(reasoning).unwrap_or_default().to_string()
    }

    fn extract_responses_text(payload: &Value) -> String {
        if let Some(text) = Self::first_non_empty_text(payload.get("output_text")) {
            return text.to_string();
        }

        payload
            .get("output")
            .and_then(Value::as_array)
            .and_then(|outputs| outputs.first())
            .and_then(|output| output.get("content"))
            .and_then(Value::as_array)
            .and_then(|content_items| {
                content_items.iter().find_map(|item| {
                    Self::first_non_empty_text(item.get("text")).or_else(|| {
                        Self::first_non_empty_text(
                            item.get("content")
                                .and_then(Value::as_array)
                                .and_then(|inner| inner.first())
                                .and_then(|inner_item| inner_item.get("text")),
                        )
                    })
                })
            })
            .unwrap_or_default()
            .to_string()
    }

    async fn chat_via_responses(&self, req: &ChatRequest) -> Result<ChatResponse, MotosanError> {
        let model = req.model.clone().unwrap_or_else(|| self.model.clone());
        let mut instructions_parts = Vec::new();
        // system_blocks takes priority over system string
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

        let mut input = Vec::new();
        for message in &req.messages {
            match message.role {
                Role::System => {
                    let trimmed = message.content.trim();
                    if !trimmed.is_empty() {
                        instructions_parts.push(trimmed.to_string());
                    }
                }
                Role::Assistant => {
                    if message.tool_calls.is_empty() {
                        input.push(json!({"role": "assistant", "content": message.content}));
                    } else {
                        let tool_calls = message
                            .tool_calls
                            .iter()
                            .map(|tool_call| {
                                json!({
                                    "id": tool_call.id,
                                    "type": "function",
                                    "function": {
                                        "name": tool_call.name,
                                        "arguments": serde_json::to_string(&tool_call.input).unwrap_or_default(),
                                    }
                                })
                            })
                            .collect::<Vec<_>>();
                        input.push(json!({
                            "role": "assistant",
                            "content": message.content,
                            "tool_calls": tool_calls,
                        }));
                    }
                }
                Role::User => input.push(json!({"role": "user", "content": message.content})),
                Role::Tool => {
                    if let Some(tool_call_id) = &message.tool_call_id {
                        input.push(json!({
                            "role": "tool",
                            "tool_call_id": tool_call_id,
                            "content": message.content,
                        }));
                    }
                }
            }
        }

        let mut body = json!({
            "model": model,
            "input": input,
        });

        if !instructions_parts.is_empty() {
            body["instructions"] = json!(instructions_parts.join("\n\n"));
        }

        if let Some(provider_options) = &req.provider_options {
            if let Some(map) = provider_options.as_object() {
                for (key, value) in map {
                    body[key] = value.clone();
                }
            }
        }

        let response = self
            .apply_auth(self.http.post(&self.responses_url).json(&body))
            .send()
            .await
            .map_err(|error| MotosanError::Network(error.to_string()))?;

        let status = response.status();
        let payload: Value = response
            .json()
            .await
            .map_err(|error| MotosanError::ProviderError(error.to_string()))?;

        if !status.is_success() {
            let message = extract_error_message(&payload, "openai responses request failed");
            return Err(map_http_error(status.as_u16(), message));
        }

        let content = Self::extract_responses_text(&payload);
        let model = payload
            .get("model")
            .and_then(Value::as_str)
            .unwrap_or(DEFAULT_OPENAI_MODEL)
            .to_string();
        let input_tokens = payload
            .get("usage")
            .and_then(|usage| {
                usage
                    .get("input_tokens")
                    .or_else(|| usage.get("prompt_tokens"))
            })
            .and_then(Value::as_u64)
            .unwrap_or(0) as u32;
        let output_tokens = payload
            .get("usage")
            .and_then(|usage| {
                usage
                    .get("output_tokens")
                    .or_else(|| usage.get("completion_tokens"))
            })
            .and_then(Value::as_u64)
            .unwrap_or(0) as u32;

        Ok(ChatResponseBuilder::new(DEFAULT_OPENAI_MODEL)
            .content(content)
            .model(model)
            .usage(input_tokens, output_tokens)
            .stop_reason(StopReason::Stop)
            .build())
    }
}

struct OpenAIRequestBuilder {
    req: ChatRequest,
    default_model: String,
    stream: bool,
}

impl OpenAIRequestBuilder {
    fn new(req: ChatRequest, default_model: String) -> Self {
        Self {
            req,
            default_model,
            stream: false,
        }
    }

    fn stream(mut self, stream: bool) -> Self {
        self.stream = stream;
        self
    }

    fn build(self) -> Value {
        let model = self
            .req
            .model
            .clone()
            .unwrap_or_else(|| self.default_model.clone());
        let mut messages = Vec::new();

        // system_blocks takes priority over system string
        if let Some(blocks) = &self.req.system_blocks {
            let joined: String = blocks
                .iter()
                .map(|b| b.text.as_str())
                .collect::<Vec<_>>()
                .join("\n");
            if !joined.is_empty() {
                messages.push(json!({"role": "system", "content": joined}));
            }
        } else if let Some(system) = &self.req.system {
            messages.push(json!({"role": "system", "content": system}));
        }

        for message in &self.req.messages {
            match message.role {
                Role::User => {
                    if !message.content_blocks.is_empty() {
                        // Use structured content blocks (vision/multimodal)
                        let blocks: Vec<Value> = message.content_blocks.iter().map(|block| {
                            match block {
                                ContentBlock::Text { text } => json!({"type": "text", "text": text}),
                                ContentBlock::Image { source } => match source {
                                    ImageSource::Base64 { media_type, data } => json!({
                                        "type": "image_url",
                                        "image_url": {"url": format!("data:{media_type};base64,{data}")}
                                    }),
                                    ImageSource::Url { url } => json!({
                                        "type": "image_url",
                                        "image_url": {"url": url}
                                    }),
                                },
                                // Document blocks are rejected before reaching this point.
                                ContentBlock::Document { .. } => unreachable!("Document blocks should be rejected before serialization"),
                            }
                        }).collect();
                        messages.push(json!({"role": "user", "content": blocks}));
                    } else {
                        messages.push(json!({"role": "user", "content": message.content}));
                    }
                }
                Role::Assistant => {
                    if message.tool_calls.is_empty() {
                        messages.push(json!({"role": "assistant", "content": message.content}));
                    } else {
                        let tool_calls = message
                            .tool_calls
                            .iter()
                            .map(|tool_call| {
                                json!({
                                    "id": tool_call.id,
                                    "type": "function",
                                    "function": {
                                        "name": tool_call.name,
                                        "arguments": serde_json::to_string(&tool_call.input).unwrap_or_default(),
                                    }
                                })
                            })
                            .collect::<Vec<_>>();
                        messages.push(json!({
                            "role": "assistant",
                            "content": message.content,
                            "tool_calls": tool_calls,
                        }));
                    }
                }
                Role::System => {
                    messages.push(json!({"role": "system", "content": message.content}))
                }
                Role::Tool => {
                    if let Some(tool_call_id) = &message.tool_call_id {
                        messages.push(json!({
                            "role": "tool",
                            "tool_call_id": tool_call_id,
                            "content": message.content,
                        }));
                    }
                }
            }
        }

        let mut body = json!({
            "model": model,
            "messages": messages,
        });

        if self.stream {
            body["stream"] = json!(true);
        }
        if let Some(temperature) = self.req.temperature {
            body["temperature"] = json!(temperature);
        }
        if let Some(max_tokens) = self.req.max_tokens {
            body["max_tokens"] = json!(max_tokens);
        }
        if let Some(tools) = self.req.tools {
            let mapped_tools: Vec<Value> = tools
                .into_iter()
                .map(|tool| {
                    json!({
                        "type": "function",
                        "function": {
                            "name": tool.name,
                            "description": tool.description.unwrap_or_default(),
                            "parameters": tool.input_schema.unwrap_or_else(|| json!({"type":"object","properties":{}})),
                        }
                    })
                })
                .collect();
            if !mapped_tools.is_empty() {
                body["tools"] = json!(mapped_tools);
            }
        }
        if let Some(tool_choice) = &self.req.tool_choice {
            match tool_choice {
                ToolChoice::Auto => {
                    body["tool_choice"] = json!("auto");
                }
                ToolChoice::Required => {
                    body["tool_choice"] = json!("required");
                }
                ToolChoice::None => {
                    body["tool_choice"] = json!("none");
                }
                ToolChoice::Tool { name } => {
                    body["tool_choice"] = json!({"type": "function", "function": {"name": name}});
                }
            }
        }
        if let Some(ref stop_sequences) = self.req.stop_sequences {
            if !stop_sequences.is_empty() {
                body["stop"] = json!(stop_sequences);
            }
        }
        if let Some(provider_options) = self.req.provider_options {
            if let Some(map) = provider_options.as_object() {
                for (key, value) in map {
                    body[key] = value.clone();
                }
            }
        }

        body
    }
}

#[async_trait]
impl ProviderImpl for OpenAIProvider {
    async fn chat(&self, req: ChatRequest) -> Result<ChatResponse, MotosanError> {
        reject_document_blocks(&req, "OpenAI")?;
        let fallback_request = req.clone();
        let body = OpenAIRequestBuilder::new(req, self.model.clone()).build();
        let mut attempt = 0;
        let payload: Value;
        loop {
            let response = match self
                .apply_auth(self.http.post(&self.chat_url).json(&body))
                .send()
                .await
            {
                Ok(response) => response,
                Err(error) => {
                    if attempt < self.retry_policy.max_retries && is_retryable_network_error(&error)
                    {
                        attempt += 1;
                        sleep_before_retry(&self.retry_policy, attempt, None).await;
                        continue;
                    }
                    return Err(MotosanError::Network(error.to_string()));
                }
            };

            let status = response.status();
            let retry_after = parse_retry_after(response.headers());

            if self.responses_fallback && status.as_u16() == 404 {
                return self.chat_via_responses(&fallback_request).await;
            }

            let current_payload: Value = response
                .json()
                .await
                .map_err(|error| MotosanError::ProviderError(error.to_string()))?;

            if status.is_success() {
                payload = current_payload;
                break;
            }

            if attempt < self.retry_policy.max_retries && is_retryable_status(status.as_u16()) {
                attempt += 1;
                sleep_before_retry(&self.retry_policy, attempt, retry_after).await;
                continue;
            }

            let message = extract_error_message(&current_payload, "openai request failed");
            return Err(map_http_error(status.as_u16(), message));
        }

        let content = Self::extract_chat_content(&payload);

        let tool_calls = payload
            .get("choices")
            .and_then(Value::as_array)
            .and_then(|choices| choices.first())
            .and_then(|choice| choice.get("message"))
            .and_then(|message| message.get("tool_calls"))
            .and_then(Value::as_array)
            .map(|calls| {
                calls
                    .iter()
                    .filter_map(|call| {
                        let id = call.get("id").and_then(Value::as_str)?.to_string();
                        let function = call.get("function")?;
                        let name = function.get("name").and_then(Value::as_str)?.to_string();
                        let arguments = function
                            .get("arguments")
                            .and_then(Value::as_str)
                            .unwrap_or("{}");
                        let input = serde_json::from_str(arguments)
                            .unwrap_or_else(|_| Value::String(arguments.to_string()));
                        Some(ToolCall { id, name, input })
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();

        let stop_reason = match payload
            .get("choices")
            .and_then(Value::as_array)
            .and_then(|choices| choices.first())
            .and_then(|choice| choice.get("finish_reason"))
            .and_then(Value::as_str)
        {
            Some("stop") => StopReason::Stop,
            Some("length") => StopReason::MaxTokens,
            Some("tool_calls") => StopReason::ToolUse,
            _ => StopReason::Other,
        };

        let model = payload
            .get("model")
            .and_then(Value::as_str)
            .unwrap_or(DEFAULT_OPENAI_MODEL)
            .to_string();
        let input_tokens = payload
            .get("usage")
            .and_then(|usage| usage.get("prompt_tokens"))
            .and_then(Value::as_u64)
            .unwrap_or(0) as u32;
        let output_tokens = payload
            .get("usage")
            .and_then(|usage| usage.get("completion_tokens"))
            .and_then(Value::as_u64)
            .unwrap_or(0) as u32;

        Ok(ChatResponseBuilder::new(DEFAULT_OPENAI_MODEL)
            .content(content)
            .tool_calls(tool_calls)
            .model(model)
            .usage(input_tokens, output_tokens)
            .stop_reason(stop_reason)
            .build())
    }

    async fn stream(&self, req: ChatRequest) -> Result<BoxStream, MotosanError> {
        reject_document_blocks(&req, "OpenAI")?;
        let body = OpenAIRequestBuilder::new(req, self.model.clone())
            .stream(true)
            .build();
        let mut attempt = 0;
        let response = loop {
            let response = match self
                .apply_auth(self.http.post(&self.chat_url).json(&body))
                .send()
                .await
            {
                Ok(response) => response,
                Err(error) => {
                    if attempt < self.retry_policy.max_retries && is_retryable_network_error(&error)
                    {
                        attempt += 1;
                        sleep_before_retry(&self.retry_policy, attempt, None).await;
                        continue;
                    }
                    return Err(MotosanError::Network(error.to_string()));
                }
            };

            let status = response.status();
            if status.is_success() {
                break response;
            }

            let retry_after = parse_retry_after(response.headers());
            if attempt < self.retry_policy.max_retries && is_retryable_status(status.as_u16()) {
                attempt += 1;
                sleep_before_retry(&self.retry_policy, attempt, retry_after).await;
                continue;
            }

            let current_payload: Value = response
                .json()
                .await
                .unwrap_or_else(|_| json!({"error": {"message": "openai stream request failed"}}));
            let message = extract_error_message(&current_payload, "openai stream request failed");
            return Err(map_http_error(status.as_u16(), message));
        };

        let raw_stream = response.bytes_stream().eventsource();
        let adapter = OpenAIStreamAdapter {
            inner: Box::pin(raw_stream),
            pending: std::collections::VecDeque::new(),
            seen_tool_ids: Vec::new(),
            pending_stop_reason: None,
            done_emitted: false,
        };

        Ok(Box::pin(adapter))
    }
}

/// Stream adapter that parses OpenAI SSE events including tool_calls in deltas.
struct OpenAIStreamAdapter {
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
    pending: std::collections::VecDeque<StreamEvent>,
    seen_tool_ids: Vec<String>,
    /// Captured from the last chunk's `choices[0].finish_reason`. Stashed
    /// rather than emitted immediately so we can attach it to a single
    /// terminal `done` event when the `[DONE]` sentinel arrives — this
    /// avoids emitting two `done` events per stream.
    pending_stop_reason: Option<StopReason>,
    /// Whether we have already emitted a terminal `done` event. Prevents
    /// the EOF fallback from emitting a second one when the upstream
    /// stream closes cleanly after the `[DONE]` sentinel.
    done_emitted: bool,
}

impl OpenAIStreamAdapter {
    fn parse_event(&mut self, data: &str) -> bool {
        if data.trim() == "[DONE]" {
            // Emit a single terminal done event, attaching any stop_reason
            // captured from the previous chunk's finish_reason field.
            let done = match self.pending_stop_reason.take() {
                Some(reason) => StreamEvent::done_with_stop_reason(reason),
                None => StreamEvent::done(),
            };
            self.pending.push_back(done);
            self.done_emitted = true;
            return true;
        }

        let payload: Value = match serde_json::from_str(data) {
            Ok(v) => v,
            Err(_) => return false,
        };

        let choice = match payload
            .get("choices")
            .and_then(Value::as_array)
            .and_then(|c| c.first())
        {
            Some(c) => c,
            None => return false,
        };

        let delta = choice.get("delta");

        // Text content
        if let Some(delta) = delta {
            let content = delta.get("content").and_then(Value::as_str).unwrap_or("");
            let reasoning = delta
                .get("reasoning_content")
                .and_then(Value::as_str)
                .unwrap_or("");
            let text = if !content.is_empty() {
                content
            } else if !reasoning.is_empty() {
                reasoning
            } else {
                ""
            };
            if !text.is_empty() {
                self.pending.push_back(StreamEvent::text(text));
            }

            // Tool calls in delta
            if let Some(tool_calls) = delta.get("tool_calls").and_then(Value::as_array) {
                for tc in tool_calls {
                    let tc_id = tc.get("id").and_then(Value::as_str);
                    let function = tc.get("function");
                    let tc_name = function.and_then(|f| f.get("name")).and_then(Value::as_str);
                    let tc_args = function
                        .and_then(|f| f.get("arguments"))
                        .and_then(Value::as_str);

                    if let (Some(id), Some(name)) = (tc_id, tc_name) {
                        self.seen_tool_ids.push(id.to_string());
                        self.pending
                            .push_back(StreamEvent::tool_call_start(id, name));
                    }
                    if let Some(args) = tc_args {
                        if !args.is_empty() {
                            let id = tc_id.unwrap_or("");
                            self.pending
                                .push_back(StreamEvent::tool_call_args_with_id(id, args));
                        }
                    }
                }
            }
        }

        // Finish reason — stash for the upcoming `[DONE]` sentinel so we
        // emit exactly one terminal done event with stop_reason attached.
        let finish_reason = choice.get("finish_reason").and_then(Value::as_str);
        if let Some(reason) = finish_reason {
            if reason == "tool_calls" {
                let ids: Vec<String> = self.seen_tool_ids.drain(..).collect();
                for id in &ids {
                    self.pending
                        .push_back(StreamEvent::tool_call_end_with_id(id));
                }
            }
            self.pending_stop_reason = Some(map_finish_reason(reason));
        }

        false
    }
}

/// Map an OpenAI `finish_reason` string to our [`StopReason`] enum. Mirrors the
/// non-streaming logic in `extract_chat_response` so the streaming path
/// reports the same value.
pub(crate) fn map_finish_reason(reason: &str) -> StopReason {
    match reason {
        "stop" => StopReason::Stop,
        "length" => StopReason::MaxTokens,
        "tool_calls" => StopReason::ToolUse,
        _ => StopReason::Other,
    }
}

impl Stream for OpenAIStreamAdapter {
    type Item = StreamEvent;

    fn poll_next(
        mut self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> Poll<Option<Self::Item>> {
        if let Some(event) = self.pending.pop_front() {
            return Poll::Ready(Some(event));
        }

        loop {
            match self.inner.as_mut().poll_next(cx) {
                Poll::Ready(Some(Ok(event))) => {
                    self.parse_event(&event.data);
                    if let Some(evt) = self.pending.pop_front() {
                        return Poll::Ready(Some(evt));
                    }
                    continue;
                }
                Poll::Ready(Some(Err(_))) => continue,
                Poll::Ready(None) => {
                    // End of upstream stream. Guarantee the consumer always
                    // sees exactly one terminal `done` event, even when the
                    // provider closes the connection without sending
                    // `[DONE]` and without any `finish_reason` chunk (some
                    // non-conformant proxies do this).
                    if !self.done_emitted {
                        self.done_emitted = true;
                        let done = match self.pending_stop_reason.take() {
                            Some(reason) => StreamEvent::done_with_stop_reason(reason),
                            None => StreamEvent::done(),
                        };
                        return Poll::Ready(Some(done));
                    }
                    return Poll::Ready(None);
                }
                Poll::Pending => return Poll::Pending,
            }
        }
    }
}
