use crate::error::MotosanError;
use crate::models::DEFAULT_MINIMAX_MODEL;
use crate::providers::{
    extract_error_message, is_retryable_network_error, is_retryable_status, map_http_error,
    parse_retry_after, reject_document_blocks, sleep_before_retry, ChatResponseBuilder,
    ProviderImpl,
};
use crate::retry::RetryPolicy;
use crate::stream::BoxStream;
use crate::types::{ChatRequest, ChatResponse, Role, StopReason, StreamEvent, ToolCall};

use async_trait::async_trait;
use eventsource_stream::Eventsource;
use futures_core::Stream;
use reqwest::Client;
use serde_json::{json, Value};
use std::pin::Pin;
use std::task::Poll;

pub struct MinimaxProvider {
    http: Client,
    api_key: String,
    model: String,
    base_url: String,
    expose_reasoning: bool,
    retry_policy: RetryPolicy,
}

impl MinimaxProvider {
    pub fn new(
        api_key: impl Into<String>,
        model: Option<String>,
        base_url: Option<String>,
    ) -> Self {
        Self {
            http: Client::new(),
            api_key: api_key.into(),
            model: model.unwrap_or_else(|| DEFAULT_MINIMAX_MODEL.to_string()),
            base_url: base_url.unwrap_or_else(|| "https://api.minimax.io/v1".to_string()),
            expose_reasoning: false,
            retry_policy: RetryPolicy::default(),
        }
    }

    pub fn with_expose_reasoning(mut self, expose_reasoning: bool) -> Self {
        self.expose_reasoning = expose_reasoning;
        self
    }

    pub fn with_retry_policy(mut self, retry_policy: RetryPolicy) -> Self {
        self.retry_policy = retry_policy;
        self
    }

    fn endpoint(&self) -> String {
        format!("{}/chat/completions", self.base_url.trim_end_matches('/'))
    }

    fn minimax_payload_error(payload: &Value) -> Option<(u16, String)> {
        let base_resp = payload.get("base_resp")?;
        let status_code = base_resp.get("status_code").and_then(Value::as_i64)?;
        if status_code == 0 {
            return None;
        }

        let message = base_resp
            .get("status_msg")
            .and_then(Value::as_str)
            .unwrap_or("minimax request failed")
            .to_string();

        let message_lower = message.to_ascii_lowercase();
        let mapped_status = match status_code {
            2049 => 401,
            1008 => 429,
            4000..=4999 => 400,
            5000..=5999 => 500,
            _ if message_lower.contains("invalid api key")
                || message_lower.contains("unauthorized")
                || message_lower.contains("authentication") =>
            {
                401
            }
            _ if message_lower.contains("insufficient balance")
                || message_lower.contains("quota")
                || message_lower.contains("rate limit")
                || message_lower.contains("too many") =>
            {
                429
            }
            _ => 500,
        };
        Some((mapped_status, message))
    }

    fn resolve_expose_reasoning(&self, req: &ChatRequest) -> bool {
        req.provider_options
            .as_ref()
            .and_then(Value::as_object)
            .and_then(|opts| opts.get("minimax_expose_reasoning"))
            .and_then(Value::as_bool)
            .unwrap_or(self.expose_reasoning)
    }

    fn strip_think_blocks(content: &str) -> String {
        let mut remaining = content;
        let mut sanitized = String::new();

        while let Some(open_idx) = remaining.find("<think>") {
            sanitized.push_str(&remaining[..open_idx]);
            let after_open = &remaining[open_idx + "<think>".len()..];
            if let Some(close_idx) = after_open.find("</think>") {
                remaining = &after_open[close_idx + "</think>".len()..];
            } else {
                remaining = "";
                break;
            }
        }

        sanitized.push_str(remaining);
        sanitized.trim().to_string()
    }

    fn first_non_empty_text(value: Option<&Value>) -> Option<&str> {
        value
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|text| !text.is_empty())
    }

    fn extract_chat_content(payload: &Value, expose_reasoning: bool) -> String {
        let message = payload
            .get("choices")
            .and_then(Value::as_array)
            .and_then(|choices| choices.first())
            .and_then(|choice| choice.get("message"));

        let content = Self::first_non_empty_text(message.and_then(|msg| msg.get("content")));
        let reasoning =
            Self::first_non_empty_text(message.and_then(|msg| msg.get("reasoning_content")));

        if expose_reasoning {
            return content.or(reasoning).unwrap_or_default().to_string();
        }

        if let Some(content_text) = content {
            let sanitized = Self::strip_think_blocks(content_text);
            if !sanitized.is_empty() {
                return sanitized;
            }
        }

        reasoning.map(Self::strip_think_blocks).unwrap_or_default()
    }

    fn extract_stream_delta_text(payload: &Value, expose_reasoning: bool) -> String {
        let delta = payload
            .get("choices")
            .and_then(Value::as_array)
            .and_then(|choices| choices.first())
            .and_then(|choice| choice.get("delta"));

        let content = Self::first_non_empty_text(delta.and_then(|d| d.get("content")));
        let reasoning = Self::first_non_empty_text(delta.and_then(|d| d.get("reasoning_content")));

        if expose_reasoning {
            return content.or(reasoning).unwrap_or_default().to_string();
        }

        if let Some(content_text) = content {
            let sanitized = Self::strip_think_blocks(content_text);
            if !sanitized.is_empty() {
                return sanitized;
            }
        }

        reasoning.map(Self::strip_think_blocks).unwrap_or_default()
    }
}

struct MinimaxRequestBuilder {
    req: ChatRequest,
    default_model: String,
    stream: bool,
}

impl MinimaxRequestBuilder {
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
        let mut system_parts = Vec::new();
        // system_blocks takes priority over system string
        if let Some(blocks) = &self.req.system_blocks {
            for b in blocks {
                let trimmed = b.text.trim();
                if !trimmed.is_empty() {
                    system_parts.push(trimmed.to_string());
                }
            }
        } else if let Some(system) = &self.req.system {
            let trimmed = system.trim();
            if !trimmed.is_empty() {
                system_parts.push(trimmed.to_string());
            }
        }

        let mut messages: Vec<Value> = Vec::new();
        for message in &self.req.messages {
            match message.role {
                Role::System => {
                    let trimmed = message.content.trim();
                    if !trimmed.is_empty() {
                        system_parts.push(trimmed.to_string());
                    }
                }
                Role::User => {
                    messages.push(json!({"role": "user", "content": message.content}));
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
                Role::Tool => {
                    if let Some(tool_call_id) = &message.tool_call_id {
                        messages.push(json!({
                            "role": "tool",
                            "content": message.content,
                            "tool_call_id": tool_call_id,
                        }));
                    }
                }
            }
        }

        if !system_parts.is_empty() {
            let merged_system = system_parts.join("\n\n");
            if let Some(first_user) = messages
                .iter_mut()
                .find(|message| message.get("role").and_then(Value::as_str) == Some("user"))
            {
                let content = first_user
                    .get("content")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                first_user["content"] = json!(format!("{}\n\n{}", merged_system, content));
            } else {
                messages.insert(0, json!({"role": "user", "content": merged_system}));
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
        if let Some(ref stop_sequences) = self.req.stop_sequences {
            if !stop_sequences.is_empty() {
                body["stop"] = json!(stop_sequences);
            }
        }
        if let Some(provider_options) = self.req.provider_options {
            if let Some(map) = provider_options.as_object() {
                for (key, value) in map {
                    if key == "minimax_expose_reasoning" {
                        continue;
                    }
                    body[key] = value.clone();
                }
            }
        }

        body
    }
}

#[async_trait]
impl ProviderImpl for MinimaxProvider {
    async fn chat(&self, req: ChatRequest) -> Result<ChatResponse, MotosanError> {
        reject_document_blocks(&req, "MiniMax")?;
        let expose_reasoning = self.resolve_expose_reasoning(&req);
        let body = MinimaxRequestBuilder::new(req, self.model.clone()).build();
        let mut attempt = 0;
        let payload: Value;
        loop {
            let response = match self
                .http
                .post(self.endpoint())
                .header("authorization", format!("Bearer {}", self.api_key))
                .json(&body)
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
            let current_payload: Value = response
                .json()
                .await
                .map_err(|error| MotosanError::ProviderError(error.to_string()))?;

            if status.is_success() {
                if let Some((mapped_status, message)) =
                    Self::minimax_payload_error(&current_payload)
                {
                    return Err(map_http_error(mapped_status, message));
                }
                payload = current_payload;
                break;
            }

            if attempt < self.retry_policy.max_retries && is_retryable_status(status.as_u16()) {
                attempt += 1;
                sleep_before_retry(&self.retry_policy, attempt, retry_after).await;
                continue;
            }

            let message = extract_error_message(&current_payload, "minimax request failed");
            return Err(map_http_error(status.as_u16(), message));
        }

        let content = Self::extract_chat_content(&payload, expose_reasoning);

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
            .unwrap_or(DEFAULT_MINIMAX_MODEL)
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

        Ok(ChatResponseBuilder::new(DEFAULT_MINIMAX_MODEL)
            .content(content)
            .tool_calls(tool_calls)
            .model(model)
            .usage(input_tokens, output_tokens)
            .stop_reason(stop_reason)
            .build())
    }

    async fn stream(&self, req: ChatRequest) -> Result<BoxStream, MotosanError> {
        reject_document_blocks(&req, "MiniMax")?;
        let expose_reasoning = self.resolve_expose_reasoning(&req);
        let body = MinimaxRequestBuilder::new(req, self.model.clone())
            .stream(true)
            .build();
        let mut attempt = 0;
        let response = loop {
            let response = match self
                .http
                .post(self.endpoint())
                .header("authorization", format!("Bearer {}", self.api_key))
                .json(&body)
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

            let body_bytes = response.bytes().await.ok();

            if let Some(payload) = body_bytes
                .as_deref()
                .and_then(|bytes| serde_json::from_slice::<Value>(bytes).ok())
            {
                if let Some((mapped_status, message)) = Self::minimax_payload_error(&payload) {
                    return Err(map_http_error(mapped_status, message));
                }

                let message = extract_error_message(&payload, "minimax stream request failed");
                return Err(map_http_error(status.as_u16(), message));
            }

            let message = body_bytes
                .as_deref()
                .map(|bytes| String::from_utf8_lossy(bytes).trim().to_string())
                .filter(|message| !message.is_empty())
                .unwrap_or_else(|| "minimax stream request failed".to_string());
            return Err(map_http_error(status.as_u16(), message));
        };

        let raw_stream = response.bytes_stream().eventsource();
        let adapter = MinimaxStreamAdapter {
            inner: Box::pin(raw_stream),
            pending: std::collections::VecDeque::new(),
            expose_reasoning,
            seen_tool_ids: Vec::new(),
            pending_stop_reason: None,
        };

        Ok(Box::pin(adapter))
    }
}

/// Stream adapter that parses Minimax SSE events including tool_calls.
struct MinimaxStreamAdapter {
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
    expose_reasoning: bool,
    seen_tool_ids: Vec<String>,
    /// Stashed from the last chunk's `finish_reason`, drained on `[DONE]`.
    /// Mirrors `OpenAIStreamAdapter::pending_stop_reason`.
    pending_stop_reason: Option<StopReason>,
}

impl Stream for MinimaxStreamAdapter {
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
                    if event.data.trim() == "[DONE]" {
                        let done = match self.pending_stop_reason.take() {
                            Some(reason) => StreamEvent::done_with_stop_reason(reason),
                            None => StreamEvent::done(),
                        };
                        return Poll::Ready(Some(done));
                    }

                    let payload: Value = match serde_json::from_str(&event.data) {
                        Ok(v) => v,
                        Err(_) => continue,
                    };

                    let choice = payload
                        .get("choices")
                        .and_then(Value::as_array)
                        .and_then(|c| c.first());

                    if let Some(choice) = choice {
                        let delta = choice.get("delta");

                        // Text content
                        if let Some(delta) = delta {
                            let text = MinimaxProvider::extract_stream_delta_text(
                                &payload,
                                self.expose_reasoning,
                            );
                            if !text.is_empty() {
                                self.pending.push_back(StreamEvent::text(text));
                            }

                            // Tool calls
                            if let Some(tool_calls) =
                                delta.get("tool_calls").and_then(Value::as_array)
                            {
                                for tc in tool_calls {
                                    let tc_id = tc.get("id").and_then(Value::as_str);
                                    let function = tc.get("function");
                                    let tc_name = function
                                        .and_then(|f| f.get("name"))
                                        .and_then(Value::as_str);
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
                                            self.pending.push_back(
                                                StreamEvent::tool_call_args_with_id(id, args),
                                            );
                                        }
                                    }
                                }
                            }
                        }

                        // Finish reason
                        let finish_reason = choice.get("finish_reason").and_then(Value::as_str);
                        if let Some(reason) = finish_reason {
                            if reason == "tool_calls" {
                                let ids: Vec<String> = self.seen_tool_ids.drain(..).collect();
                                for id in &ids {
                                    self.pending
                                        .push_back(StreamEvent::tool_call_end_with_id(id));
                                }
                            }
                            // Stash for the upcoming `[DONE]` sentinel so we
                            // emit exactly one terminal done event with
                            // stop_reason attached. Mapping inlined to keep
                            // `--features minimax` independent of openai.
                            self.pending_stop_reason = Some(match reason {
                                "stop" => StopReason::Stop,
                                "length" => StopReason::MaxTokens,
                                "tool_calls" => StopReason::ToolUse,
                                _ => StopReason::Other,
                            });
                        }
                    }

                    if let Some(evt) = self.pending.pop_front() {
                        return Poll::Ready(Some(evt));
                    }
                    continue;
                }
                Poll::Ready(Some(Err(_))) => continue,
                Poll::Ready(None) => {
                    // End of upstream stream — flush a final done event with
                    // the stashed stop_reason if `[DONE]` was never sent.
                    if let Some(reason) = self.pending_stop_reason.take() {
                        return Poll::Ready(Some(StreamEvent::done_with_stop_reason(reason)));
                    }
                    return Poll::Ready(None);
                }
                Poll::Pending => return Poll::Pending,
            }
        }
    }
}
