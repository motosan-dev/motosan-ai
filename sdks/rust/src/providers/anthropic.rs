use crate::error::MotosanError;
use crate::providers::{extract_error_message, map_http_error, ChatResponseBuilder, ProviderImpl};
use crate::stream::BoxStream;
use crate::types::{ChatRequest, ChatResponse, Role, StopReason};
use async_trait::async_trait;
use eventsource_stream::Eventsource;
use reqwest::Client;
use serde_json::{json, Value};
use tokio_stream::StreamExt;

pub struct AnthropicProvider {
    http: Client,
    api_key: String,
    model: String,
    base_url: String,
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
            model: model.unwrap_or_else(|| "claude-sonnet-4-5".to_string()),
            base_url: base_url.unwrap_or_else(|| "https://api.anthropic.com".to_string()),
        }
    }

    fn endpoint(&self) -> String {
        format!("{}/v1/messages", self.base_url)
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
                    messages.push(json!({"role": "assistant", "content": message.content}))
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
impl ProviderImpl for AnthropicProvider {
    async fn chat(&self, req: ChatRequest) -> Result<ChatResponse, MotosanError> {
        let body = AnthropicRequestBuilder::new(req, self.model.clone()).build();

        let response = self
            .http
            .post(self.endpoint())
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", "2023-06-01")
            .json(&body)
            .send()
            .await
            .map_err(|error| MotosanError::Network(error.to_string()))?;

        let status = response.status();
        let payload: Value = response
            .json()
            .await
            .map_err(|error| MotosanError::ProviderError(error.to_string()))?;

        if !status.is_success() {
            let message = extract_error_message(&payload, "anthropic request failed");
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

        let model = payload
            .get("model")
            .and_then(Value::as_str)
            .unwrap_or("claude-sonnet-4-5")
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

        Ok(ChatResponseBuilder::new("claude-sonnet-4-5")
            .content(content)
            .model(model)
            .usage(input_tokens, output_tokens)
            .stop_reason(stop_reason)
            .build())
    }

    async fn stream(&self, req: ChatRequest) -> Result<BoxStream, MotosanError> {
        let body = AnthropicRequestBuilder::new(req, self.model.clone())
            .stream(true)
            .build();

        let response = self
            .http
            .post(self.endpoint())
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", "2023-06-01")
            .json(&body)
            .send()
            .await
            .map_err(|error| MotosanError::Network(error.to_string()))?;

        let status = response.status();
        if !status.is_success() {
            let message = response
                .text()
                .await
                .unwrap_or_else(|_| "anthropic stream request failed".to_string());
            return Err(map_http_error(status.as_u16(), message));
        }

        let parsed_stream = response
            .bytes_stream()
            .eventsource()
            .filter_map(|event| match event {
                Ok(event) => {
                    let payload: Value = serde_json::from_str(&event.data).ok()?;
                    let event_type = payload.get("type")?.as_str()?;

                    match event_type {
                        "content_block_delta" => {
                            let text = payload
                                .get("delta")
                                .and_then(|delta| delta.get("text"))
                                .and_then(Value::as_str)
                                .unwrap_or("")
                                .to_string();
                            Some(crate::types::StreamEvent {
                                content: text,
                                done: false,
                            })
                        }
                        "message_stop" => Some(crate::types::StreamEvent {
                            content: String::new(),
                            done: true,
                        }),
                        _ => None,
                    }
                }
                Err(_) => None,
            });

        Ok(Box::pin(parsed_stream))
    }
}
