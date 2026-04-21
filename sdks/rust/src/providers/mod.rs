use crate::error::MotosanError;
#[cfg(any(
    feature = "anthropic",
    feature = "openai",
    feature = "minimax",
    feature = "ollama_native",
    feature = "gemini",
    feature = "gemini-code-assist",
))]
use crate::retry::RetryPolicy;
use crate::stream::BoxStream;
use crate::types::{ChatRequest, ChatResponse, ContentBlock, ProviderCapabilities};
#[cfg(any(
    feature = "anthropic",
    feature = "openai",
    feature = "minimax",
    feature = "ollama_native",
    feature = "gemini",
    feature = "gemini-code-assist",
))]
use crate::types::{StopReason, ToolCall, Usage};
use async_trait::async_trait;
#[cfg(any(
    feature = "anthropic",
    feature = "openai",
    feature = "minimax",
    feature = "ollama_native",
    feature = "gemini",
    feature = "gemini-code-assist",
))]
use reqwest::header::HeaderMap;
#[cfg(any(
    feature = "anthropic",
    feature = "openai",
    feature = "minimax",
    feature = "ollama_native",
    feature = "gemini",
    feature = "gemini-code-assist",
))]
use serde_json::Value;
#[cfg(any(
    feature = "anthropic",
    feature = "openai",
    feature = "minimax",
    feature = "ollama_native",
    feature = "gemini",
    feature = "gemini-code-assist",
))]
use std::time::Duration;

#[derive(Debug, Clone, Copy)]
pub enum Provider {
    Anthropic,
    OpenAI,
    Minimax,
    Ollama,
    /// Shells out to the `claude` CLI. Requires the `claude-code` feature.
    ClaudeCode,
    /// Shells out to OpenAI's `codex exec --json`. Requires the `codex-cli` feature.
    CodexCli,
    /// Shells out to Google's `gemini -p` CLI. Requires the `gemini-cli` feature.
    GeminiCli,
    /// HTTP client for the Google Generative AI REST API. Requires the `gemini` feature.
    Gemini,
    /// Shells out to Google Cloud Code Assist API (cloudcode-pa.googleapis.com).
    /// Requires OAuth token with cloud-platform scope. Requires the `gemini-code-assist` feature.
    GeminiCodeAssist,
}

#[async_trait]
pub trait ProviderImpl: Send + Sync {
    fn capabilities(&self) -> ProviderCapabilities {
        ProviderCapabilities::text_only()
    }

    fn validate_request(&self, req: &ChatRequest) -> Result<(), MotosanError> {
        let caps = self.capabilities();
        for msg in &req.messages {
            for block in &msg.content_blocks {
                match block {
                    ContentBlock::Image { .. } if !caps.supports_image => {
                        return Err(MotosanError::UnsupportedFeature(
                            "provider does not support image input".into(),
                        ));
                    }
                    ContentBlock::Document { .. } if !caps.supports_document => {
                        return Err(MotosanError::UnsupportedFeature(
                            "provider does not support document input".into(),
                        ));
                    }
                    _ => {}
                }
            }
        }
        Ok(())
    }

    async fn chat(&self, _req: ChatRequest) -> Result<ChatResponse, MotosanError>;
    async fn stream(&self, _req: ChatRequest) -> Result<BoxStream, MotosanError>;
}

#[cfg(any(
    feature = "anthropic",
    feature = "openai",
    feature = "minimax",
    feature = "ollama_native",
    feature = "gemini",
    feature = "gemini-code-assist",
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
    feature = "ollama_native",
    feature = "gemini",
    feature = "gemini-code-assist",
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
                cache_creation_input_tokens: None,
                cache_read_input_tokens: None,
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
            cache_creation_input_tokens: None,
            cache_read_input_tokens: None,
        };
        self
    }

    pub(crate) fn cache_usage(
        mut self,
        cache_creation_input_tokens: Option<u32>,
        cache_read_input_tokens: Option<u32>,
    ) -> Self {
        self.usage.cache_creation_input_tokens = cache_creation_input_tokens;
        self.usage.cache_read_input_tokens = cache_read_input_tokens;
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
    feature = "ollama_native",
    feature = "gemini",
    feature = "gemini-code-assist",
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
    feature = "ollama_native",
    feature = "gemini",
    feature = "gemini-code-assist",
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
    feature = "ollama_native",
    feature = "gemini",
    feature = "gemini-code-assist",
))]
pub(crate) fn is_retryable_status(status_code: u16) -> bool {
    status_code == 429 || status_code >= 500
}

#[cfg(any(
    feature = "anthropic",
    feature = "openai",
    feature = "minimax",
    feature = "ollama_native",
    feature = "gemini",
    feature = "gemini-code-assist",
))]
pub(crate) fn is_retryable_network_error(error: &reqwest::Error) -> bool {
    error.is_timeout() || error.is_connect() || error.is_request() || error.is_body()
}

#[cfg(any(
    feature = "anthropic",
    feature = "openai",
    feature = "minimax",
    feature = "ollama_native",
    feature = "gemini",
    feature = "gemini-code-assist",
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
    feature = "ollama_native",
    feature = "gemini",
    feature = "gemini-code-assist",
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

#[cfg(feature = "anthropic")]
pub mod anthropic;

#[cfg(feature = "openai")]
pub mod openai;

#[cfg(feature = "ollama_native")]
pub mod ollama;

#[cfg(feature = "claude-code")]
pub mod claude_code;

#[cfg(feature = "codex-cli")]
pub mod codex_cli;

#[cfg(feature = "gemini-cli")]
pub mod gemini_cli;

#[cfg(feature = "gemini")]
pub mod gemini;

#[cfg(feature = "gemini-code-assist")]
pub mod gemini_code_assist;

#[cfg(test)]
mod validate_tests {
    use super::*;
    use crate::types::{Message, ProviderCapabilities};

    struct TextOnlyProvider;

    #[async_trait]
    impl ProviderImpl for TextOnlyProvider {
        async fn chat(&self, _req: ChatRequest) -> Result<ChatResponse, MotosanError> {
            unimplemented!()
        }
        async fn stream(&self, _req: ChatRequest) -> Result<BoxStream, MotosanError> {
            unimplemented!()
        }
    }

    struct FullProvider;

    #[async_trait]
    impl ProviderImpl for FullProvider {
        fn capabilities(&self) -> ProviderCapabilities {
            ProviderCapabilities::full()
        }
        async fn chat(&self, _req: ChatRequest) -> Result<ChatResponse, MotosanError> {
            unimplemented!()
        }
        async fn stream(&self, _req: ChatRequest) -> Result<BoxStream, MotosanError> {
            unimplemented!()
        }
    }

    fn req_with_image() -> ChatRequest {
        let msg = Message::user_with_image("look", "abc123", "image/png");
        ChatRequest::builder().messages(vec![msg]).build()
    }

    fn req_with_document() -> ChatRequest {
        let msg = Message::user_with_pdf_base64("read this", "abc123");
        ChatRequest::builder().messages(vec![msg]).build()
    }

    fn req_text_only() -> ChatRequest {
        ChatRequest::builder()
            .messages(vec![Message::user("hello")])
            .build()
    }

    #[test]
    fn text_only_provider_rejects_image() {
        let p = TextOnlyProvider;
        let result = p.validate_request(&req_with_image());
        assert!(matches!(result, Err(MotosanError::UnsupportedFeature(_))));
    }

    #[test]
    fn text_only_provider_rejects_document() {
        let p = TextOnlyProvider;
        let result = p.validate_request(&req_with_document());
        assert!(matches!(result, Err(MotosanError::UnsupportedFeature(_))));
    }

    #[test]
    fn full_provider_accepts_image() {
        let p = FullProvider;
        assert!(p.validate_request(&req_with_image()).is_ok());
    }

    #[test]
    fn full_provider_accepts_document() {
        let p = FullProvider;
        assert!(p.validate_request(&req_with_document()).is_ok());
    }

    #[test]
    fn any_provider_accepts_plain_text() {
        let p = TextOnlyProvider;
        assert!(p.validate_request(&req_text_only()).is_ok());
    }
}
