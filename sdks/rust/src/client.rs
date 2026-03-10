use crate::error::MotosanError;
use crate::providers::Provider;
use crate::stream::BoxStream;
use crate::types::{ChatRequest, ChatResponse, Message};

#[derive(Debug, Clone)]
pub struct Client {
    provider: Provider,
    api_key: String,
    model: Option<String>,
}

impl Client {
    pub fn builder() -> ClientBuilder {
        ClientBuilder::default()
    }

    pub fn provider(&self) -> Provider {
        self.provider
    }

    pub fn api_key(&self) -> &str {
        &self.api_key
    }

    pub fn model(&self) -> Option<&str> {
        self.model.as_deref()
    }

    pub async fn chat(&self, messages: Vec<Message>) -> Result<ChatResponse, MotosanError> {
        self.chat_with(ChatRequest {
            messages,
            model: self.model.clone(),
            system: None,
            temperature: None,
            max_tokens: None,
            tools: None,
            provider_options: None,
        })
        .await
    }

    pub async fn chat_with(&self, request: ChatRequest) -> Result<ChatResponse, MotosanError> {
        self.dispatch_chat(request).await
    }

    pub async fn stream(&self, messages: Vec<Message>) -> Result<BoxStream, MotosanError> {
        self.dispatch_stream(ChatRequest {
            messages,
            model: self.model.clone(),
            system: None,
            temperature: None,
            max_tokens: None,
            tools: None,
            provider_options: None,
        })
        .await
    }

    async fn dispatch_chat(&self, request: ChatRequest) -> Result<ChatResponse, MotosanError> {
        match self.provider {
            Provider::Anthropic => {
                #[cfg(feature = "anthropic")]
                {
                    use crate::providers::anthropic::AnthropicProvider;
                    use crate::providers::ProviderImpl;
                    return AnthropicProvider.chat(request).await;
                }
                #[cfg(not(feature = "anthropic"))]
                {
                    let _ = request;
                    return Err(MotosanError::Config(
                        "anthropic feature is not enabled".to_string(),
                    ));
                }
            }
            Provider::OpenAI => {
                #[cfg(feature = "openai")]
                {
                    use crate::providers::openai::OpenAIProvider;
                    use crate::providers::ProviderImpl;
                    return OpenAIProvider.chat(request).await;
                }
                #[cfg(not(feature = "openai"))]
                {
                    let _ = request;
                    return Err(MotosanError::Config(
                        "openai feature is not enabled".to_string(),
                    ));
                }
            }
            Provider::Minimax => {
                #[cfg(feature = "minimax")]
                {
                    use crate::providers::minimax::MinimaxProvider;
                    use crate::providers::ProviderImpl;
                    return MinimaxProvider.chat(request).await;
                }
                #[cfg(not(feature = "minimax"))]
                {
                    let _ = request;
                    return Err(MotosanError::Config(
                        "minimax feature is not enabled".to_string(),
                    ));
                }
            }
        }
    }

    async fn dispatch_stream(&self, request: ChatRequest) -> Result<BoxStream, MotosanError> {
        match self.provider {
            Provider::Anthropic => {
                #[cfg(feature = "anthropic")]
                {
                    use crate::providers::anthropic::AnthropicProvider;
                    use crate::providers::ProviderImpl;
                    return AnthropicProvider.stream(request).await;
                }
                #[cfg(not(feature = "anthropic"))]
                {
                    let _ = request;
                    return Err(MotosanError::Config(
                        "anthropic feature is not enabled".to_string(),
                    ));
                }
            }
            Provider::OpenAI => {
                #[cfg(feature = "openai")]
                {
                    use crate::providers::openai::OpenAIProvider;
                    use crate::providers::ProviderImpl;
                    return OpenAIProvider.stream(request).await;
                }
                #[cfg(not(feature = "openai"))]
                {
                    let _ = request;
                    return Err(MotosanError::Config(
                        "openai feature is not enabled".to_string(),
                    ));
                }
            }
            Provider::Minimax => {
                #[cfg(feature = "minimax")]
                {
                    use crate::providers::minimax::MinimaxProvider;
                    use crate::providers::ProviderImpl;
                    return MinimaxProvider.stream(request).await;
                }
                #[cfg(not(feature = "minimax"))]
                {
                    let _ = request;
                    return Err(MotosanError::Config(
                        "minimax feature is not enabled".to_string(),
                    ));
                }
            }
        }
    }
}

#[derive(Debug, Default, Clone)]
pub struct ClientBuilder {
    provider: Option<Provider>,
    api_key: Option<String>,
    model: Option<String>,
}

impl ClientBuilder {
    pub fn provider(mut self, provider: Provider) -> Self {
        self.provider = Some(provider);
        self
    }

    pub fn api_key(mut self, api_key: impl Into<String>) -> Self {
        self.api_key = Some(api_key.into());
        self
    }

    pub fn model(mut self, model: impl Into<String>) -> Self {
        self.model = Some(model.into());
        self
    }

    pub fn build(self) -> Result<Client, MotosanError> {
        let provider = self
            .provider
            .ok_or_else(|| MotosanError::Config("provider is required".to_string()))?;
        let api_key = self
            .api_key
            .ok_or_else(|| MotosanError::Config("api_key is required".to_string()))?;

        Ok(Client {
            provider,
            api_key,
            model: self.model,
        })
    }
}
