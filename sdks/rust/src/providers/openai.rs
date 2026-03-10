//! OpenAI provider.
//!
//! Implements the `ProviderImpl` trait for OpenAI's Chat Completions API.
//! Enabled via `features = ["openai"]`.

use async_trait::async_trait;
use crate::{error::MotosanError, stream::BoxStream, types::{ChatRequest, ChatResponse}};
use super::ProviderImpl;

pub struct OpenAIProvider {
    api_key: String,
    client: reqwest::Client,
}

impl OpenAIProvider {
    pub fn new(api_key: impl Into<String>) -> Self {
        Self { api_key: api_key.into(), client: reqwest::Client::new() }
    }
}

#[async_trait]
impl ProviderImpl for OpenAIProvider {
    async fn chat(&self, _req: ChatRequest) -> Result<ChatResponse, MotosanError> {
        // TODO: implement — tracked in issue #5
        Err(MotosanError::Config("OpenAI provider not yet implemented".to_string()))
    }

    async fn stream(&self, _req: ChatRequest) -> Result<BoxStream, MotosanError> {
        // TODO: implement — tracked in issue #6
        Err(MotosanError::Stream("OpenAI streaming not yet implemented".to_string()))
    }
}
