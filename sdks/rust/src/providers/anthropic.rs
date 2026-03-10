use crate::error::MotosanError;
use crate::providers::ProviderImpl;
use crate::stream::BoxStream;
use crate::types::{ChatRequest, ChatResponse};
use async_trait::async_trait;

pub struct AnthropicProvider;

#[async_trait]
impl ProviderImpl for AnthropicProvider {
    async fn chat(&self, _req: ChatRequest) -> Result<ChatResponse, MotosanError> {
        Err(MotosanError::ProviderError(
            "anthropic provider not implemented".to_string(),
        ))
    }

    async fn stream(&self, _req: ChatRequest) -> Result<BoxStream, MotosanError> {
        Err(MotosanError::ProviderError(
            "anthropic streaming not implemented".to_string(),
        ))
    }
}

