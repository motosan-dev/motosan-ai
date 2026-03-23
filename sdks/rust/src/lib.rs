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

pub use client::{Client, ClientBuilder};
pub use error::MotosanError;
pub use models::{
    ANTHROPIC_MODELS, DEFAULT_ANTHROPIC_MODEL, DEFAULT_MINIMAX_MODEL, DEFAULT_OLLAMA_MODEL,
    DEFAULT_OPENAI_MODEL, MINIMAX_MODELS, OPENAI_MODELS,
};
pub use providers::Provider;
pub use retry::RetryPolicy;
pub use stream::{BoxStream, StreamEvent};
pub use types::{
    ChatRequest, ChatRequestBuilder, ChatResponse, ContentBlock, ImageSource, McpServerConfig,
    McpServerType, Message, Role, StopReason, StreamEventType, Tool, ToolCall, ToolChoice, Usage,
};
