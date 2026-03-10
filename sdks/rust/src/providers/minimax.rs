use crate::error::MotosanError;
use crate::providers::ProviderImpl;
use crate::stream::BoxStream;
use crate::types::{ChatRequest, ChatResponse, StopReason, Usage};
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
            model: model.unwrap_or_else(|| "MiniMax-Text-01".to_string()),
            base_url: base_url.unwrap_or_else(|| "https://api.minimax.chat".to_string()),
        }
    }
}

#[async_trait]
impl ProviderImpl for MinimaxProvider {
    async fn chat(&self, req: ChatRequest) -> Result<ChatResponse, MotosanError> {
        let model = req.model.clone().unwrap_or_else(|| self.model.clone());
        let mut messages = Vec::new();

        if let Some(system) = &req.system {
            messages.push(json!({"role": "system", "content": system}));
        }

        for message in &req.messages {
            let role = match message.role {
                crate::types::Role::User => "user",
                crate::types::Role::Assistant => "assistant",
                crate::types::Role::System => "system",
            };
            messages.push(json!({"role": role, "content": message.content}));
        }

        let mut body = json!({
            "model": model,
            "messages": messages,
        });
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
            .post(format!("{}/v1/text/chatcompletion_v2", self.base_url))
            .header("authorization", format!("Bearer {}", self.api_key))
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
                .unwrap_or("minimax request failed")
                .to_string();
            return Err(match status.as_u16() {
                401 => MotosanError::Auth(message),
                429 => MotosanError::RateLimit(message),
                400 => MotosanError::InvalidRequest(message),
                _ => MotosanError::ProviderError(message),
            });
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
            .unwrap_or("MiniMax-Text-01")
            .to_string();

        let usage = Usage {
            input_tokens: payload
                .get("usage")
                .and_then(|usage| usage.get("prompt_tokens"))
                .and_then(Value::as_u64)
                .unwrap_or(0) as u32,
            output_tokens: payload
                .get("usage")
                .and_then(|usage| usage.get("completion_tokens"))
                .and_then(Value::as_u64)
                .unwrap_or(0) as u32,
        };

        Ok(ChatResponse {
            content,
            model,
            usage,
            stop_reason,
        })
    }

    async fn stream(&self, req: ChatRequest) -> Result<BoxStream, MotosanError> {
        let model = req.model.clone().unwrap_or_else(|| self.model.clone());
        let mut messages = Vec::new();

        if let Some(system) = &req.system {
            messages.push(json!({"role": "system", "content": system}));
        }

        for message in &req.messages {
            let role = match message.role {
                crate::types::Role::User => "user",
                crate::types::Role::Assistant => "assistant",
                crate::types::Role::System => "system",
            };
            messages.push(json!({"role": role, "content": message.content}));
        }

        let mut body = json!({
            "model": model,
            "messages": messages,
            "stream": true,
        });
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
            .post(format!("{}/v1/text/chatcompletion_v2", self.base_url))
            .header("authorization", format!("Bearer {}", self.api_key))
            .json(&body)
            .send()
            .await
            .map_err(|error| MotosanError::Network(error.to_string()))?;

        let status = response.status();
        if !status.is_success() {
            let message = response
                .text()
                .await
                .unwrap_or_else(|_| "minimax stream request failed".to_string());
            return Err(match status.as_u16() {
                401 => MotosanError::Auth(message),
                429 => MotosanError::RateLimit(message),
                400 => MotosanError::InvalidRequest(message),
                _ => MotosanError::ProviderError(message),
            });
        }

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
