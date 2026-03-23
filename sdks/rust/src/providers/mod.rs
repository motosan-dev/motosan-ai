use crate::error::MotosanError;
#[cfg(any(
    feature = "anthropic",
    feature = "openai",
    feature = "minimax",
    feature = "ollama_native"
))]
use crate::retry::RetryPolicy;
use crate::stream::BoxStream;
#[cfg(any(feature = "openai", feature = "minimax", feature = "ollama_native"))]
use crate::types::ContentBlock;
use crate::types::{ChatRequest, ChatResponse};
#[cfg(any(
    feature = "anthropic",
    feature = "openai",
    feature = "minimax",
    feature = "ollama_native"
))]
use crate::types::{StopReason, ToolCall, Usage};
use async_trait::async_trait;
#[cfg(any(
    feature = "anthropic",
    feature = "openai",
    feature = "minimax",
    feature = "ollama_native"
))]
use reqwest::header::HeaderMap;
#[cfg(any(
    feature = "anthropic",
    feature = "openai",
    feature = "minimax",
    feature = "ollama_native"
))]
use serde_json::Value;
#[cfg(any(
    feature = "anthropic",
    feature = "openai",
    feature = "minimax",
    feature = "ollama_native"
))]
use std::time::Duration;

#[derive(Debug, Clone, Copy)]
pub enum Provider {
    Anthropic,
    OpenAI,
    Minimax,
    Ollama,
}

#[async_trait]
pub trait ProviderImpl: Send + Sync {
    async fn chat(&self, _req: ChatRequest) -> Result<ChatResponse, MotosanError>;
    async fn stream(&self, _req: ChatRequest) -> Result<BoxStream, MotosanError>;
}

#[cfg(any(
    feature = "anthropic",
    feature = "openai",
    feature = "minimax",
    feature = "ollama_native"
))]
pub(crate) struct ChatResponseBuilder {
    content: String,
    thinking: Option<String>,
    tool_calls: Vec<ToolCall>,
    model: String,
    usage: Usage,
    stop_reason: StopReason,
}

#[cfg(any(
    feature = "anthropic",
    feature = "openai",
    feature = "minimax",
    feature = "ollama_native"
))]
impl ChatResponseBuilder {
    pub(crate) fn new(default_model: impl Into<String>) -> Self {
        Self {
            content: String::new(),
            thinking: None,
            tool_calls: Vec::new(),
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

    pub(crate) fn thinking(mut self, thinking: Option<String>) -> Self {
        self.thinking = thinking;
        self
    }

    pub(crate) fn model(mut self, model: impl Into<String>) -> Self {
        self.model = model.into();
        self
    }

    pub(crate) fn tool_calls(mut self, tool_calls: Vec<ToolCall>) -> Self {
        self.tool_calls = tool_calls;
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
            thinking: self.thinking,
            tool_calls: self.tool_calls,
            model: self.model,
            usage: self.usage,
            stop_reason: self.stop_reason,
        }
    }
}

#[cfg(any(
    feature = "anthropic",
    feature = "openai",
    feature = "minimax",
    feature = "ollama_native"
))]
pub(crate) fn extract_error_message(payload: &Value, fallback: &str) -> String {
    payload
        .get("error")
        .and_then(|error| error.get("message"))
        .and_then(Value::as_str)
        .unwrap_or(fallback)
        .to_string()
}

#[cfg(any(
    feature = "anthropic",
    feature = "openai",
    feature = "minimax",
    feature = "ollama_native"
))]
pub(crate) fn map_http_error(status_code: u16, message: String) -> MotosanError {
    match status_code {
        401 => MotosanError::Auth(message),
        429 => MotosanError::RateLimit(message),
        400 => MotosanError::InvalidRequest(message),
        _ => MotosanError::ProviderError(message),
    }
}

#[cfg(any(
    feature = "anthropic",
    feature = "openai",
    feature = "minimax",
    feature = "ollama_native"
))]
pub(crate) fn is_retryable_status(status_code: u16) -> bool {
    status_code == 429 || status_code >= 500
}

#[cfg(any(
    feature = "anthropic",
    feature = "openai",
    feature = "minimax",
    feature = "ollama_native"
))]
pub(crate) fn is_retryable_network_error(error: &reqwest::Error) -> bool {
    error.is_timeout() || error.is_connect() || error.is_request() || error.is_body()
}

#[cfg(any(
    feature = "anthropic",
    feature = "openai",
    feature = "minimax",
    feature = "ollama_native"
))]
pub(crate) fn parse_retry_after(headers: &HeaderMap) -> Option<Duration> {
    let raw = headers.get("retry-after")?.to_str().ok()?.trim();
    let seconds = raw.parse::<u64>().ok()?;
    Some(Duration::from_secs(seconds))
}

#[cfg(any(
    feature = "anthropic",
    feature = "openai",
    feature = "minimax",
    feature = "ollama_native"
))]
pub(crate) async fn sleep_before_retry(
    policy: &RetryPolicy,
    attempt: u32,
    retry_after: Option<Duration>,
) {
    let delay = if policy.respect_retry_after {
        retry_after.unwrap_or_else(|| policy.delay_for_attempt(attempt))
    } else {
        policy.delay_for_attempt(attempt)
    };

    tokio::time::sleep(delay).await;
}

/// Return an `UnsupportedFeature` error if any message contains a `Document` block.
#[cfg(any(feature = "openai", feature = "minimax", feature = "ollama_native"))]
pub(crate) fn reject_document_blocks(
    req: &ChatRequest,
    provider_name: &str,
) -> Result<(), MotosanError> {
    for message in &req.messages {
        for block in &message.content_blocks {
            if matches!(block, ContentBlock::Document { .. }) {
                return Err(MotosanError::UnsupportedFeature(format!(
                    "Document content blocks (PDF) are not supported by the {} provider",
                    provider_name
                )));
            }
        }
    }
    Ok(())
}

#[cfg(feature = "anthropic")]
pub mod anthropic;

#[cfg(feature = "openai")]
pub mod openai;

#[cfg(feature = "minimax")]
pub mod minimax;

#[cfg(feature = "ollama_native")]
pub mod ollama;
