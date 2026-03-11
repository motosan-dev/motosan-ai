use crate::error::MotosanError;
use crate::models::DEFAULT_MINIMAX_MODEL;
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

pub struct MinimaxProvider {
    http: Client,
    api_key: String,
    model: String,
    base_url: String,
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
            retry_policy: RetryPolicy::default(),
        }
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

        let mapped_status = if status_code == 2049 { 401 } else { 500 };
        Some((mapped_status, message))
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
impl ProviderImpl for MinimaxProvider {
    async fn chat(&self, req: ChatRequest) -> Result<ChatResponse, MotosanError> {
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

        let content = payload
            .get("choices")
            .and_then(Value::as_array)
            .and_then(|choices| choices.first())
            .and_then(|choice| choice.get("message"))
            .and_then(|message| message.get("content"))
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();

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
            .model(model)
            .usage(input_tokens, output_tokens)
            .stop_reason(stop_reason)
            .build())
    }

    async fn stream(&self, req: ChatRequest) -> Result<BoxStream, MotosanError> {
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

            let message = response
                .text()
                .await
                .unwrap_or_else(|_| "minimax stream request failed".to_string());
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
                    let text = payload
                        .get("choices")
                        .and_then(Value::as_array)
                        .and_then(|choices| choices.first())
                        .and_then(|choice| choice.get("delta"))
                        .and_then(|delta| delta.get("content"))
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_string();
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
