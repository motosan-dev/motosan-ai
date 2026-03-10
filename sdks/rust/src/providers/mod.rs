use crate::error::MotosanError;
use crate::stream::BoxStream;
use crate::types::{ChatRequest, ChatResponse, StopReason, Usage};
use async_trait::async_trait;
use serde_json::Value;

#[derive(Debug, Clone, Copy)]
pub enum Provider {
    Anthropic,
    OpenAI,
    Minimax,
}

#[async_trait]
pub trait ProviderImpl: Send + Sync {
    async fn chat(&self, _req: ChatRequest) -> Result<ChatResponse, MotosanError>;
    async fn stream(&self, _req: ChatRequest) -> Result<BoxStream, MotosanError>;
}

pub(crate) struct ChatResponseBuilder {
    content: String,
    model: String,
    usage: Usage,
    stop_reason: StopReason,
}

impl ChatResponseBuilder {
    pub(crate) fn new(default_model: impl Into<String>) -> Self {
        Self {
            content: String::new(),
            model: default_model.into(),
            usage: Usage {
                input_tokens: 0,
                output_tokens: 0,
            },
            stop_reason: StopReason::Other,
        }
    }

    pub(crate) fn content(mut self, content: impl Into<String>) -> Self {
        self.content = content.into();
        self
    }

    pub(crate) fn model(mut self, model: impl Into<String>) -> Self {
        self.model = model.into();
        self
    }

    pub(crate) fn usage(mut self, input_tokens: u32, output_tokens: u32) -> Self {
        self.usage = Usage {
            input_tokens,
            output_tokens,
        };
        self
    }

    pub(crate) fn stop_reason(mut self, stop_reason: StopReason) -> Self {
        self.stop_reason = stop_reason;
        self
    }

    pub(crate) fn build(self) -> ChatResponse {
        ChatResponse {
            content: self.content,
            model: self.model,
            usage: self.usage,
            stop_reason: self.stop_reason,
        }
    }
}

pub(crate) fn extract_error_message(payload: &Value, fallback: &str) -> String {
    payload
        .get("error")
        .and_then(|error| error.get("message"))
        .and_then(Value::as_str)
        .unwrap_or(fallback)
        .to_string()
}

pub(crate) fn map_http_error(status_code: u16, message: String) -> MotosanError {
    match status_code {
        401 => MotosanError::Auth(message),
        429 => MotosanError::RateLimit(message),
        400 => MotosanError::InvalidRequest(message),
        _ => MotosanError::ProviderError(message),
    }
}

#[cfg(feature = "anthropic")]
pub mod anthropic;

#[cfg(feature = "openai")]
pub mod openai;

#[cfg(feature = "minimax")]
pub mod minimax;
