use async_trait::async_trait;
use crate::{error::MotosanError, stream::BoxStream, types::{ChatRequest, ChatResponse}};

/// Core trait that all provider implementations must satisfy.
#[async_trait]
pub trait ProviderImpl: Send + Sync {
    async fn chat(&self, req: ChatRequest) -> Result<ChatResponse, MotosanError>;
    async fn stream(&self, req: ChatRequest) -> Result<BoxStream, MotosanError>;
}

#[cfg(feature = "anthropic")]
pub mod anthropic;

#[cfg(feature = "openai")]
pub mod openai;

#[cfg(feature = "minimax")]
pub mod minimax;
