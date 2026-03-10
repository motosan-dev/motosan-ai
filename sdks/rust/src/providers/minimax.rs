//! MiniMax provider.
//!
//! Implements the `ProviderImpl` trait for MiniMax's Chat Completions API.
//! Enabled via `features = ["minimax"]`.

use async_trait::async_trait;
use crate::{error::MotosanError, stream::BoxStream, types::{ChatRequest, ChatResponse}};
use super::ProviderImpl;

pub struct MinimaxProvider {
    api_key: String,
    client: reqwest::Client,
}

impl MinimaxProvider {
    pub fn new(api_key: impl Into<String>) -> Self {
        Self { api_key: api_key.into(), client: reqwest::Client::new() }
    }
}

#[async_trait]
impl ProviderImpl for MinimaxProvider {
    async fn chat(&self, _req: ChatRequest) -> Result<ChatResponse, MotosanError> {
        // TODO: implement — tracked in issue #7
        Err(MotosanError::Config("MiniMax provider not yet implemented".to_string()))
    }

    async fn stream(&self, _req: ChatRequest) -> Result<BoxStream, MotosanError> {
        // TODO: implement — tracked in issue #8
        Err(MotosanError::Stream("MiniMax streaming not yet implemented".to_string()))
    }
}
