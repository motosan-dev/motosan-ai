const DEFAULT_MAX_TOKENS: u32 = 4096;

use crate::error::MotosanError;
use crate::models::DEFAULT_ANTHROPIC_MODEL;
use crate::providers::{
    extract_error_message, is_retryable_network_error, is_retryable_status, map_http_error,
    parse_retry_after, sleep_before_retry, ChatResponseBuilder, ProviderImpl,
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

pub struct AnthropicProvider {
    http: Client,
    api_key: String,
    model: String,
    base_url: String,
    retry_policy: RetryPolicy,
}

impl AnthropicProvider {
    pub fn new(
        api_key: impl Into<String>,
        model: Option<String>,
        base_url: Option<String>,
    ) -> Self {
        Self {
            http: Client::new(),
            api_key: api_key.into(),
            model: model.unwrap_or_else(|| DEFAULT_ANTHROPIC_MODEL.to_string()),
            base_url: base_url.unwrap_or_else(|| "https://api.anthropic.com".to_string()),
            retry_policy: RetryPolicy::default(),
        }
    }

    pub fn with_retry_policy(mut self, retry_policy: RetryPolicy) -> Self {
        self.retry_policy = retry_policy;
        self
    }

    fn endpoint(&self) -> String {
        format!("{}/v1/messages", self.base_url)
    }

    fn is_setup_token(token: &str) -> bool {
        token.starts_with("sk-ant-oat01-")
    }

    fn apply_auth(&self, request: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        if Self::is_setup_token(&self.api_key) {
            request
                .header("authorization", format!("Bearer {}", self.api_key))
                .header("anthropic-beta", "oauth-2025-04-20")
        } else {
            request.header("x-api-key", &self.api_key)
        }
    }

    fn with_auth_hint(status_code: u16, message: String, is_setup_token: bool) -> String {
        if status_code != 401 {
            return message;
        }

        if is_setup_token {
            format!(
                "{message}. Hint: setup tokens (sk-ant-oat01-*) require Authorization: Bearer and anthropic-beta: oauth-2025-04-20"
            )
        } else {
            format!(
                "{message}. Hint: Anthropic API keys use x-api-key; setup tokens (sk-ant-oat01-*) use Authorization: Bearer plus anthropic-beta: oauth-2025-04-20"
            )
        }
    }
}

struct AnthropicRequestBuilder {
    req: ChatRequest,
    default_model: String,
    stream: bool,
}

impl AnthropicRequestBuilder {
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
        let explicit_system = self.req.system.clone();
        let mut extracted_systems = Vec::new();
        let mut messages = Vec::new();

        for message in &self.req.messages {
            match message.role {
                Role::System => extracted_systems.push(message.content.clone()),
                Role::User => messages.push(json!({"role": "user", "content": message.content})),
                Role::Assistant => {
                    if message.tool_calls.is_empty() {
                        messages.push(json!({"role": "assistant", "content": message.content}));
                    } else {
                        let mut blocks = Vec::new();
                        if !message.content.is_empty() {
                            blocks.push(json!({"type": "text", "text": message.content}));
                        }
                        for tool_call in &message.tool_calls {
                            blocks.push(json!({
                                "type": "tool_use",
                                "id": tool_call.id,
                                "name": tool_call.name,
                                "input": tool_call.input,
                            }));
                        }
                        messages.push(json!({"role": "assistant", "content": blocks}));
                    }
                }
                Role::Tool => {
                    if let Some(tool_use_id) = &message.tool_call_id {
                        messages.push(json!({
                            "role": "user",
                            "content": [{
                                "type": "tool_result",
                                "tool_use_id": tool_use_id,
                                "content": message.content,
                            }],
                        }));
                    }
                }
            }
        }

        let system = explicit_system.or_else(|| {
            if extracted_systems.is_empty() {
                None
            } else {
                Some(extracted_systems.join("\n"))
            }
        });

        let mut body = json!({
            "model": model,
            "messages": messages,
        });

        if self.stream {
            body["stream"] = json!(true);
        }
        if let Some(system_prompt) = system {
            body["system"] = json!(system_prompt);
        }
        if let Some(temperature) = self.req.temperature {
            body["temperature"] = json!(temperature);
        }
        let max_tokens = self.req.max_tokens.unwrap_or(DEFAULT_MAX_TOKENS);
        body["max_tokens"] = json!(max_tokens);
        if let Some(tools) = self.req.tools {
            let mapped_tools: Vec<Value> = tools
                .into_iter()
                .map(|tool| {
                    json!({
                        "name": tool.name,
                        "description": tool.description.unwrap_or_default(),
                        "input_schema": tool.input_schema.unwrap_or_else(|| json!({"type":"object","properties":{}})),
                    })
                })
                .collect();
            if !mapped_tools.is_empty() {
                body["tools"] = json!(mapped_tools);
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
impl ProviderImpl for AnthropicProvider {
    async fn chat(&self, req: ChatRequest) -> Result<ChatResponse, MotosanError> {
        let body = AnthropicRequestBuilder::new(req, self.model.clone()).build();
        let mut attempt = 0;
        let payload: Value;
        loop {
            let request = self
                .http
                .post(self.endpoint())
                .header("anthropic-version", "2023-06-01")
                .json(&body);
            let response = match self.apply_auth(request).send().await {
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
                payload = current_payload;
                break;
            }

            if attempt < self.retry_policy.max_retries && is_retryable_status(status.as_u16()) {
                attempt += 1;
                sleep_before_retry(&self.retry_policy, attempt, retry_after).await;
                continue;
            }

            let message = extract_error_message(&current_payload, "anthropic request failed");
            let message = Self::with_auth_hint(
                status.as_u16(),
                message,
                Self::is_setup_token(&self.api_key),
            );
            return Err(map_http_error(status.as_u16(), message));
        }

        let content = payload
            .get("content")
            .and_then(Value::as_array)
            .map(|items| {
                items
                    .iter()
                    .filter_map(|item| item.get("text").and_then(Value::as_str))
                    .collect::<Vec<_>>()
                    .join("")
            })
            .unwrap_or_default();

        let tool_calls = payload
            .get("content")
            .and_then(Value::as_array)
            .map(|items| {
                items
                    .iter()
                    .filter(|item| item.get("type").and_then(Value::as_str) == Some("tool_use"))
                    .map(|item| ToolCall {
                        id: item
                            .get("id")
                            .and_then(Value::as_str)
                            .unwrap_or_default()
                            .to_string(),
                        name: item
                            .get("name")
                            .and_then(Value::as_str)
                            .unwrap_or_default()
                            .to_string(),
                        input: item.get("input").cloned().unwrap_or_else(|| json!({})),
                    })
                    .filter(|call| !call.id.is_empty() && !call.name.is_empty())
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();

        let model = payload
            .get("model")
            .and_then(Value::as_str)
            .unwrap_or(DEFAULT_ANTHROPIC_MODEL)
            .to_string();

        let input_tokens = payload
            .get("usage")
            .and_then(|usage| usage.get("input_tokens"))
            .and_then(Value::as_u64)
            .unwrap_or(0) as u32;
        let output_tokens = payload
            .get("usage")
            .and_then(|usage| usage.get("output_tokens"))
            .and_then(Value::as_u64)
            .unwrap_or(0) as u32;

        let stop_reason = match payload.get("stop_reason").and_then(Value::as_str) {
            Some("end_turn") => StopReason::EndTurn,
            Some("max_tokens") => StopReason::MaxTokens,
            Some("tool_use") => StopReason::ToolUse,
            Some("stop") => StopReason::Stop,
            _ => StopReason::Other,
        };

        Ok(ChatResponseBuilder::new(DEFAULT_ANTHROPIC_MODEL)
            .content(content)
            .tool_calls(tool_calls)
            .model(model)
            .usage(input_tokens, output_tokens)
            .stop_reason(stop_reason)
            .build())
    }

    async fn stream(&self, req: ChatRequest) -> Result<BoxStream, MotosanError> {
        let body = AnthropicRequestBuilder::new(req, self.model.clone())
            .stream(true)
            .build();
        let mut attempt = 0;
        let response = loop {
            let request = self
                .http
                .post(self.endpoint())
                .header("anthropic-version", "2023-06-01")
                .json(&body);
            let response = match self.apply_auth(request).send().await {
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

            let message = response
                .text()
                .await
                .unwrap_or_else(|_| "anthropic stream request failed".to_string());
            let message = Self::with_auth_hint(
                status.as_u16(),
                message,
                Self::is_setup_token(&self.api_key),
            );
            return Err(map_http_error(status.as_u16(), message));
        };

        let raw_stream = response.bytes_stream().eventsource();
        let adapter = AnthropicStreamAdapter {
            inner: Box::pin(raw_stream),
            pending: std::collections::VecDeque::new(),
        };

        Ok(Box::pin(adapter))
    }
}

/// Stream adapter that parses Anthropic SSE events including tool_use blocks.
struct AnthropicStreamAdapter {
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
}

impl Stream for AnthropicStreamAdapter {
    type Item = StreamEvent;

    fn poll_next(
        mut self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> Poll<Option<Self::Item>> {
        // Drain any pending events first
        if let Some(event) = self.pending.pop_front() {
            return Poll::Ready(Some(event));
        }

        loop {
            match self.inner.as_mut().poll_next(cx) {
                Poll::Ready(Some(Ok(event))) => {
                    let payload: Value = match serde_json::from_str(&event.data) {
                        Ok(v) => v,
                        Err(_) => continue,
                    };
                    let event_type = match payload.get("type").and_then(Value::as_str) {
                        Some(t) => t,
                        None => continue,
                    };

                    match event_type {
                        "content_block_start" => {
                            let block = payload.get("content_block");
                            if let Some(block) = block {
                                if block.get("type").and_then(Value::as_str) == Some("tool_use") {
                                    let id = block
                                        .get("id")
                                        .and_then(Value::as_str)
                                        .unwrap_or_default();
                                    let name = block
                                        .get("name")
                                        .and_then(Value::as_str)
                                        .unwrap_or_default();
                                    return Poll::Ready(Some(StreamEvent::tool_call_start(
                                        id, name,
                                    )));
                                }
                            }
                            continue;
                        }
                        "content_block_delta" => {
                            let delta = match payload.get("delta") {
                                Some(d) => d,
                                None => continue,
                            };
                            let delta_type = delta.get("type").and_then(Value::as_str);

                            match delta_type {
                                Some("input_json_delta") => {
                                    let partial = delta
                                        .get("partial_json")
                                        .and_then(Value::as_str)
                                        .unwrap_or_default();
                                    if !partial.is_empty() {
                                        return Poll::Ready(Some(StreamEvent::tool_call_args(
                                            partial,
                                        )));
                                    }
                                    continue;
                                }
                                _ => {
                                    // text_delta or untyped delta with "text" field
                                    let text = delta
                                        .get("text")
                                        .and_then(Value::as_str)
                                        .unwrap_or_default();
                                    if !text.is_empty() {
                                        return Poll::Ready(Some(StreamEvent::text(text)));
                                    }
                                    continue;
                                }
                            }
                        }
                        "content_block_stop" => {
                            return Poll::Ready(Some(StreamEvent::tool_call_end()));
                        }
                        "message_stop" => {
                            return Poll::Ready(Some(StreamEvent::done()));
                        }
                        _ => continue,
                    }
                }
                Poll::Ready(Some(Err(_))) => continue,
                Poll::Ready(None) => return Poll::Ready(None),
                Poll::Pending => return Poll::Pending,
            }
        }
    }
}
