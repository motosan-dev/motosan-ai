pub mod client;
pub mod error;
pub mod models;
pub mod providers;
pub mod retry;
pub mod stream;
pub mod think_stripper;
pub mod types;

#[cfg(feature = "agent-tool")]
pub mod tool_compat;

#[cfg(feature = "claude-code")]
pub use providers::claude_code;
#[cfg(feature = "claude-code")]
pub use providers::claude_code::ClaudeCodeProvider;

#[cfg(feature = "codex-cli")]
pub use providers::codex_cli;
#[cfg(feature = "codex-cli")]
pub use providers::codex_cli::CodexCliProvider;

pub use client::{Client, ClientBuilder};
pub use error::MotosanError;
pub use models::{
    ANTHROPIC_MODELS, DEFAULT_ANTHROPIC_MODEL, DEFAULT_MINIMAX_MODEL, DEFAULT_OLLAMA_MODEL,
    DEFAULT_OPENAI_MODEL, MINIMAX_MODELS, OPENAI_MODELS,
};
pub use providers::Provider;
pub use retry::RetryPolicy;
#[cfg(any(
    feature = "anthropic",
    feature = "openai",
    feature = "minimax",
    feature = "ollama_native"
))]
pub use stream::collect_stream;
pub use stream::{BoxStream, StreamEvent};
pub use types::{
    ChatRequest, ChatRequestBuilder, ChatResponse, ContentBlock, DocumentSource, ImageSource,
    McpServerConfig, McpServerType, McpToolConfig, Message, Role, StopReason, StreamEventType,
    SystemBlock, ThinkingConfig, Tool, ToolCall, ToolChoice, Usage,
};
