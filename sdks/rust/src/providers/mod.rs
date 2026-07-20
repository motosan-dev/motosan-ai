use crate::error::MotosanError;
use crate::stream::{BoxModelStream, BoxStream};
use crate::types::{
    ChatRequest, ChatResponse, ContentBlock, ModelChatRequest, ModelChatResponse, ModelContextItem,
    ModelToolCall, ModelToolOutput, ModelToolSpec, ProviderCapabilities,
};
use async_trait::async_trait;

pub mod responses;

// Shared transport helpers moved to src/transport/ (M4 Task 1). Re-exported
// pub(crate) at their old paths so provider files need no import churn.
#[cfg(feature = "_http")]
#[allow(unused_imports)]
pub(crate) use crate::transport::http::{
    apply_total_timeout, collect_stream_with_total_timeout, extract_error_message,
    extract_request_id, map_http_error, parse_retry_after, send_with_retry, ChatResponseBuilder,
};

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
    /// HTTP client for the ChatGPT backend Responses API
    /// (chatgpt.com/backend-api/codex/responses). OAuth access_token +
    /// chatgpt-account-id. Requires the `chatgpt-codex` feature.
    OpenAiChatGpt,
}

#[cfg(feature = "_http")]
impl Provider {
    /// True when the provider speaks HTTP through the shared reqwest client.
    pub(crate) fn uses_http_transport(&self) -> bool {
        !matches!(
            self,
            Provider::ClaudeCode | Provider::CodexCli | Provider::GeminiCli
        )
    }
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

    fn validate_model_request(&self, req: &ModelChatRequest) -> Result<(), MotosanError> {
        let caps = self.capabilities();
        let has_freeform_spec = req
            .tool_specs
            .iter()
            .any(|spec| matches!(spec, ModelToolSpec::Freeform(_)));
        let has_freeform_history = req.context.iter().any(|item| {
            matches!(
                item,
                ModelContextItem::ToolCall(ModelToolCall::Freeform { .. })
                    | ModelContextItem::ToolOutput(ModelToolOutput::Custom { .. })
            )
        });
        if (has_freeform_spec || has_freeform_history) && !caps.supports_freeform_tools {
            return Err(MotosanError::UnsupportedFeature(
                "provider does not support native freeform tools".into(),
            ));
        }
        if req.thinking.is_some() {
            return Err(MotosanError::UnsupportedFeature(
                "native model request thinking is not supported; use provider_options for provider-specific reasoning controls".into(),
            ));
        }
        if req
            .mcp_servers
            .as_ref()
            .is_some_and(|servers| !servers.is_empty())
            || req
                .mcp_tool_configs
                .as_ref()
                .is_some_and(|configs| !configs.is_empty())
        {
            return Err(MotosanError::UnsupportedFeature(
                "native model requests do not support MCP servers".into(),
            ));
        }

        for item in &req.context {
            if let ModelContextItem::Message(msg) = item {
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
        }

        Ok(())
    }

    async fn chat(&self, _req: ChatRequest) -> Result<ChatResponse, MotosanError>;
    async fn stream(&self, _req: ChatRequest) -> Result<BoxStream, MotosanError>;

    async fn model_chat(&self, req: ModelChatRequest) -> Result<ModelChatResponse, MotosanError> {
        self.validate_model_request(&req)?;
        Err(MotosanError::UnsupportedFeature(
            "provider does not support native model requests".into(),
        ))
    }

    async fn model_stream(&self, req: ModelChatRequest) -> Result<BoxModelStream, MotosanError> {
        self.validate_model_request(&req)?;
        Err(MotosanError::UnsupportedFeature(
            "provider does not support native model streams".into(),
        ))
    }
}

#[cfg(feature = "anthropic")]
pub mod anthropic;

#[cfg(feature = "openai")]
pub mod openai;

#[cfg(feature = "ollama")]
pub mod ollama;

#[cfg(feature = "_cli")]
pub mod redacted_envs;

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

#[cfg(feature = "chatgpt-codex")]
pub mod chatgpt_codex;

#[cfg(test)]
mod validate_tests {
    use super::*;
    use crate::types::{McpServerConfig, McpServerType, Message, ProviderCapabilities};

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

    #[test]
    fn native_model_request_rejects_thinking_before_provider_io() {
        let p = FullProvider;
        let req = ModelChatRequest::builder()
            .context_item(ModelContextItem::Message(Message::user("hello")))
            .thinking(1024)
            .build();

        let result = p.validate_model_request(&req);

        assert!(
            matches!(result, Err(MotosanError::UnsupportedFeature(msg)) if msg.contains("thinking"))
        );
    }

    #[test]
    fn native_model_request_rejects_mcp_servers_before_provider_io() {
        let p = FullProvider;
        let req = ModelChatRequest::builder()
            .context_item(ModelContextItem::Message(Message::user("hello")))
            .mcp_server(McpServerConfig {
                kind: McpServerType::Url,
                url: "https://mcp.example.test".to_string(),
                name: "tools".to_string(),
                authorization_token: None,
            })
            .build();

        let result = p.validate_model_request(&req);

        assert!(
            matches!(result, Err(MotosanError::UnsupportedFeature(msg)) if msg.contains("MCP"))
        );
    }
}
