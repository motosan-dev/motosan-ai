use std::sync::Arc;
use crate::{
    error::MotosanError,
    providers::ProviderImpl,
    stream::BoxStream,
    types::{ChatRequest, ChatResponse, Message},
    Provider,
};

/// The main client for interacting with AI providers.
///
/// # Example
///
/// ```rust,no_run
/// use motosan_ai::{Client, Message, Provider};
///
/// # #[tokio::main]
/// # async fn main() -> Result<(), Box<dyn std::error::Error>> {
/// let client = Client::builder()
///     .provider(Provider::Anthropic)
///     .api_key(std::env::var("ANTHROPIC_API_KEY")?)
///     .build()?;
///
/// let response = client.chat(vec![Message::user("Hello!")]).await?;
/// println!("{}", response.content);
/// # Ok(())
/// # }
/// ```
pub struct Client {
    provider: Arc<dyn ProviderImpl>,
}

impl Client {
    pub fn builder() -> ClientBuilder {
        ClientBuilder::default()
    }

    /// Send a chat request and wait for the full response.
    pub async fn chat(&self, messages: Vec<Message>) -> Result<ChatResponse, MotosanError> {
        self.chat_with(ChatRequest { messages, ..Default::default() }).await
    }

    /// Send a chat request with full control over parameters.
    pub async fn chat_with(&self, req: ChatRequest) -> Result<ChatResponse, MotosanError> {
        self.provider.chat(req).await
    }

    /// Stream a response token by token.
    pub async fn stream(&self, messages: Vec<Message>) -> Result<BoxStream, MotosanError> {
        self.stream_with(ChatRequest { messages, ..Default::default() }).await
    }

    /// Stream with full control over parameters.
    pub async fn stream_with(&self, req: ChatRequest) -> Result<BoxStream, MotosanError> {
        self.provider.stream(req).await
    }
}

/// Builder for [`Client`].
#[derive(Default)]
pub struct ClientBuilder {
    provider: Option<Provider>,
    api_key: Option<String>,
}

impl ClientBuilder {
    pub fn provider(mut self, provider: Provider) -> Self {
        self.provider = Some(provider);
        self
    }

    pub fn api_key(mut self, key: impl Into<String>) -> Self {
        self.api_key = Some(key.into());
        self
    }

    pub fn build(self) -> Result<Client, MotosanError> {
        let api_key = self.api_key
            .ok_or_else(|| MotosanError::Config("api_key is required".into()))?;

        let provider_impl: Arc<dyn ProviderImpl> = match self.provider
            .ok_or_else(|| MotosanError::Config("provider is required".into()))?
        {
            #[cfg(feature = "anthropic")]
            Provider::Anthropic => Arc::new(crate::providers::anthropic::AnthropicProvider::new(api_key)),

            #[cfg(feature = "openai")]
            Provider::OpenAI => Arc::new(crate::providers::openai::OpenAIProvider::new(api_key)),

            #[cfg(feature = "minimax")]
            Provider::MiniMax => Arc::new(crate::providers::minimax::MinimaxProvider::new(api_key)),
        };

        Ok(Client { provider: provider_impl })
    }
}
