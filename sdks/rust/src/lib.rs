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

/// Deprecated alias for [`ClaudeCodeProvider`]. Renamed in v0.10.0 for
/// consistency with the HTTP provider naming (`AnthropicProvider`,
/// `OpenAIProvider`, ...). The alias will be removed in a future release.
#[cfg(feature = "claude-code")]
#[deprecated(
    since = "0.10.0",
    note = "Renamed to `ClaudeCodeProvider` for consistency with other providers"
)]
pub type ClaudeCodeClient = ClaudeCodeProvider;

#[cfg(feature = "codex-cli")]
pub use providers::codex_cli;
#[cfg(feature = "codex-cli")]
pub use providers::codex_cli::CodexCliProvider;

/// Deprecated alias for [`CodexCliProvider`]. Renamed in v0.10.0 for
/// consistency with the HTTP provider naming. The alias will be removed
/// in a future release.
#[cfg(feature = "codex-cli")]
#[deprecated(
    since = "0.10.0",
    note = "Renamed to `CodexCliProvider` for consistency with other providers"
)]
pub type CodexCliClient = CodexCliProvider;

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
