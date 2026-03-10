use crate::error::MotosanError;
use crate::providers::ProviderImpl;
use crate::stream::BoxStream;
use crate::types::{ChatRequest, ChatResponse, StopReason, Usage};
use async_trait::async_trait;
use reqwest::Client;
use serde_json::{json, Value};

pub struct AnthropicProvider {
    http: Client,
    api_key: String,
    model: String,
    base_url: String,
}

impl AnthropicProvider {
    pub fn new(api_key: impl Into<String>, model: Option<String>, base_url: Option<String>) -> Self {
        Self {
            http: Client::new(),
            api_key: api_key.into(),
            model: model.unwrap_or_else(|| "claude-sonnet-4-5".to_string()),
            base_url: base_url.unwrap_or_else(|| "https://api.anthropic.com".to_string()),
        }
    }
}

#[async_trait]
impl ProviderImpl for AnthropicProvider {
    async fn chat(&self, req: ChatRequest) -> Result<ChatResponse, MotosanError> {
        let model = req.model.clone().unwrap_or_else(|| self.model.clone());
        let explicit_system = req.system.clone();
        let mut extracted_systems = Vec::new();
        let mut messages = Vec::new();

        for message in &req.messages {
            match message.role {
                crate::types::Role::System => extracted_systems.push(message.content.clone()),
                crate::types::Role::User => {
                    messages.push(json!({"role": "user", "content": message.content}))
                }
                crate::types::Role::Assistant => {
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

        if let Some(system_prompt) = system {
            body["system"] = json!(system_prompt);
        }
        if let Some(temperature) = req.temperature {
            body["temperature"] = json!(temperature);
        }
        if let Some(max_tokens) = req.max_tokens {
            body["max_tokens"] = json!(max_tokens);
        }
        if let Some(provider_options) = req.provider_options {
            if let Some(map) = provider_options.as_object() {
                for (key, value) in map {
                    body[key] = value.clone();
                }
            }
        }

        let response = self
            .http
            .post(format!("{}/v1/messages", self.base_url))
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
            let message = payload
                .get("error")
                .and_then(|error| error.get("message"))
                .and_then(Value::as_str)
                .unwrap_or("anthropic request failed")
                .to_string();
            return Err(match status.as_u16() {
                401 => MotosanError::Auth(message),
                429 => MotosanError::RateLimit(message),
                400 => MotosanError::InvalidRequest(message),
                _ => MotosanError::ProviderError(message),
            });
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

        let usage = Usage {
            input_tokens: payload
                .get("usage")
                .and_then(|usage| usage.get("input_tokens"))
                .and_then(Value::as_u64)
                .unwrap_or(0) as u32,
            output_tokens: payload
                .get("usage")
                .and_then(|usage| usage.get("output_tokens"))
                .and_then(Value::as_u64)
                .unwrap_or(0) as u32,
        };

        let stop_reason = match payload.get("stop_reason").and_then(Value::as_str) {
            Some("end_turn") => StopReason::EndTurn,
            Some("max_tokens") => StopReason::MaxTokens,
            Some("tool_use") => StopReason::ToolUse,
            Some("stop") => StopReason::Stop,
            _ => StopReason::Other,
        };

        Ok(ChatResponse {
            content,
            model,
            usage,
            stop_reason,
        })
    }

    async fn stream(&self, _req: ChatRequest) -> Result<BoxStream, MotosanError> {
        Err(MotosanError::ProviderError(
            "anthropic streaming not implemented".to_string(),
        ))
    }
}
