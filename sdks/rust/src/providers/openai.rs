use crate::error::MotosanError;
use crate::models::DEFAULT_OPENAI_MODEL;
use crate::providers::{
    extract_error_message, is_retryable_network_error, is_retryable_status, map_http_error,
    parse_retry_after, sleep_before_retry, ChatResponseBuilder, ProviderImpl,
};
use crate::retry::RetryPolicy;
use crate::stream::BoxStream;
use crate::types::{ChatRequest, ChatResponse, Role, StopReason};
use async_trait::async_trait;
use eventsource_stream::Eventsource;
use reqwest::Client;
use serde_json::{json, Value};
use tokio_stream::StreamExt;

#[derive(Debug, Clone)]
pub enum OpenAIAuthStyle {
    Bearer,
    XApiKey,
    Custom(String),
}

pub struct OpenAIProvider {
    http: Client,
    api_key: String,
    model: String,
    base_url: String,
    auth_style: OpenAIAuthStyle,
    responses_fallback: bool,
    retry_policy: RetryPolicy,
}

impl OpenAIProvider {
    pub fn new(
        api_key: impl Into<String>,
        model: Option<String>,
        base_url: Option<String>,
    ) -> Self {
        Self {
            http: Client::new(),
            api_key: api_key.into(),
            model: model.unwrap_or_else(|| DEFAULT_OPENAI_MODEL.to_string()),
            base_url: base_url.unwrap_or_else(|| "https://api.openai.com".to_string()),
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

    fn endpoint(&self) -> String {
        format!(
            "{}/v1/chat/completions",
            self.base_url.trim_end_matches('/')
        )
    }

    fn responses_endpoint(&self) -> String {
        let normalized = self.base_url.trim_end_matches('/');
        if let Some(prefix) = normalized.strip_suffix("/chat/completions") {
            return format!("{prefix}/responses");
        }
        if normalized.ends_with("/v1") {
            return format!("{normalized}/responses");
        }
        format!("{normalized}/v1/responses")
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

    fn extract_stream_delta_text(payload: &Value) -> String {
        let delta = payload
            .get("choices")
            .and_then(Value::as_array)
            .and_then(|choices| choices.first())
            .and_then(|choice| choice.get("delta"));

        let content = Self::first_non_empty_text(delta.and_then(|d| d.get("content")));
        let reasoning = Self::first_non_empty_text(delta.and_then(|d| d.get("reasoning_content")));

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
        if let Some(system) = &req.system {
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
                    input.push(json!({"role": "assistant", "content": message.content}))
                }
                Role::User => input.push(json!({"role": "user", "content": message.content})),
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
            .apply_auth(self.http.post(self.responses_endpoint()).json(&body))
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

        if let Some(system) = &self.req.system {
            messages.push(json!({"role": "system", "content": system}));
        }

        for message in &self.req.messages {
            let role = match message.role {
                Role::User => "user",
                Role::Assistant => "assistant",
                Role::System => "system",
            };
            messages.push(json!({"role": role, "content": message.content}));
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
        let fallback_request = req.clone();
        let body = OpenAIRequestBuilder::new(req, self.model.clone()).build();
        let mut attempt = 0;
        let payload: Value;
        loop {
            let response = match self
                .apply_auth(self.http.post(self.endpoint()).json(&body))
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
            .model(model)
            .usage(input_tokens, output_tokens)
            .stop_reason(stop_reason)
            .build())
    }

    async fn stream(&self, req: ChatRequest) -> Result<BoxStream, MotosanError> {
        let body = OpenAIRequestBuilder::new(req, self.model.clone())
            .stream(true)
            .build();
        let mut attempt = 0;
        let response = loop {
            let response = match self
                .apply_auth(self.http.post(self.endpoint()).json(&body))
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

        let parsed_stream = response
            .bytes_stream()
            .eventsource()
            .filter_map(|event| match event {
                Ok(event) => {
                    if event.data.trim() == "[DONE]" {
                        return Some(crate::types::StreamEvent {
                            content: String::new(),
                            done: true,
                        });
                    }

                    let payload: Value = serde_json::from_str(&event.data).ok()?;
                    let text = Self::extract_stream_delta_text(&payload);
                    if text.is_empty() {
                        return None;
                    }
                    Some(crate::types::StreamEvent {
                        content: text,
                        done: false,
                    })
                }
                Err(_) => None,
            });

        Ok(Box::pin(parsed_stream))
    }
}
