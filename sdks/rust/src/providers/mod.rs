use crate::error::MotosanError;
use crate::stream::BoxStream;
use crate::types::{ChatRequest, ChatResponse};
use async_trait::async_trait;

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

#[cfg(feature = "anthropic")]
pub mod anthropic;

#[cfg(feature = "openai")]
pub mod openai;

#[cfg(feature = "minimax")]
pub mod minimax;

