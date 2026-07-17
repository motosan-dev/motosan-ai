pub mod auth;
pub mod client;
pub mod error;
pub mod models;
pub mod providers;
pub mod retry;
pub mod stream;
pub mod think_stripper;
pub(crate) mod transport;
pub mod types;

#[cfg(feature = "claude-code")]
pub use providers::claude_code;
#[cfg(feature = "claude-code")]
pub use providers::claude_code::ClaudeCodeProvider;

#[cfg(feature = "codex-cli")]
pub use providers::codex_cli;
#[cfg(feature = "codex-cli")]
pub use providers::codex_cli::CodexCliProvider;

#[cfg(feature = "gemini-cli")]
pub use providers::gemini_cli;
#[cfg(feature = "gemini-cli")]
pub use providers::gemini_cli::GeminiCliProvider;

pub use auth::{StaticTokenSource, TokenSource};
pub use client::{Client, ClientBuilder};
pub use error::MotosanError;
pub use models::{
    ANTHROPIC_MODELS, DEFAULT_ANTHROPIC_MODEL, DEFAULT_OLLAMA_MODEL, DEFAULT_OPENAI_MODEL,
    MINIMAX_MODELS, OPENAI_MODELS,
};
pub use motosan_agent_primitives::ToolSchema;
pub use providers::Provider;
pub use retry::{RetryCause, RetryEvent, RetryPolicy};
pub use stream::collect_stream;
pub use stream::{BoxStream, StreamEvent};
pub use types::{
    ChatRequest, ChatRequestBuilder, ChatResponse, ContentBlock, DocumentSource, ImageSource,
    McpServerConfig, McpServerType, McpToolConfig, Message, ProviderCapabilities, Role, StopReason,
    StreamEventType, SystemBlock, ThinkingConfig, Tool, ToolCall, ToolChoice, Usage,
};
