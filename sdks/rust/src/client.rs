use crate::error::MotosanError;
use crate::providers::Provider;
use crate::retry::RetryPolicy;
use crate::stream::BoxStream;
use crate::types::{ChatRequest, ChatResponse, Message};

#[derive(Debug, Clone)]
pub struct Client {
    provider: Provider,
    api_key: String,
    model: Option<String>,
    retry_policy: RetryPolicy,
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

    pub fn retry_policy(&self) -> &RetryPolicy {
        &self.retry_policy
    }

    pub async fn chat(&self, messages: Vec<Message>) -> Result<ChatResponse, MotosanError> {
        let mut request_builder = ChatRequest::builder().messages(messages);
        if let Some(model) = &self.model {
            request_builder = request_builder.model(model.clone());
        }

        self.chat_with(request_builder.build()).await
    }

    pub async fn chat_with(&self, request: ChatRequest) -> Result<ChatResponse, MotosanError> {
        self.dispatch_chat(request).await
    }

    pub async fn stream(&self, messages: Vec<Message>) -> Result<BoxStream, MotosanError> {
        let mut request_builder = ChatRequest::builder().messages(messages);
        if let Some(model) = &self.model {
            request_builder = request_builder.model(model.clone());
        }

        self.dispatch_stream(request_builder.build()).await
    }

    async fn dispatch_chat(&self, request: ChatRequest) -> Result<ChatResponse, MotosanError> {
        match self.provider {
            Provider::Anthropic => {
                #[cfg(feature = "anthropic")]
                {
                    use crate::providers::ProviderImpl;
                    return self.build_anthropic_provider().chat(request).await;
                }
                #[cfg(not(feature = "anthropic"))]
                {
                    let _ = request;
                    return Err(Self::feature_not_enabled("anthropic"));
                }
            }
            Provider::OpenAI => {
                #[cfg(feature = "openai")]
                {
                    use crate::providers::ProviderImpl;
                    return self.build_openai_provider().chat(request).await;
                }
                #[cfg(not(feature = "openai"))]
                {
                    let _ = request;
                    return Err(Self::feature_not_enabled("openai"));
                }
            }
            Provider::Minimax => {
                #[cfg(feature = "minimax")]
                {
                    use crate::providers::ProviderImpl;
                    return self.build_minimax_provider().chat(request).await;
                }
                #[cfg(not(feature = "minimax"))]
                {
                    let _ = request;
                    return Err(Self::feature_not_enabled("minimax"));
                }
            }
        }
    }

    async fn dispatch_stream(&self, request: ChatRequest) -> Result<BoxStream, MotosanError> {
        match self.provider {
            Provider::Anthropic => {
                #[cfg(feature = "anthropic")]
                {
                    use crate::providers::ProviderImpl;
                    return self.build_anthropic_provider().stream(request).await;
                }
                #[cfg(not(feature = "anthropic"))]
                {
                    let _ = request;
                    return Err(Self::feature_not_enabled("anthropic"));
                }
            }
            Provider::OpenAI => {
                #[cfg(feature = "openai")]
                {
                    use crate::providers::ProviderImpl;
                    return self.build_openai_provider().stream(request).await;
                }
                #[cfg(not(feature = "openai"))]
                {
                    let _ = request;
                    return Err(Self::feature_not_enabled("openai"));
                }
            }
            Provider::Minimax => {
                #[cfg(feature = "minimax")]
                {
                    use crate::providers::ProviderImpl;
                    return self.build_minimax_provider().stream(request).await;
                }
                #[cfg(not(feature = "minimax"))]
                {
                    let _ = request;
                    return Err(Self::feature_not_enabled("minimax"));
                }
            }
        }
    }

    #[cfg(any(
        not(feature = "anthropic"),
        not(feature = "openai"),
        not(feature = "minimax")
    ))]
    fn feature_not_enabled(provider: &str) -> MotosanError {
        MotosanError::Config(format!("{provider} feature is not enabled"))
    }

    #[cfg(feature = "anthropic")]
    fn build_anthropic_provider(&self) -> crate::providers::anthropic::AnthropicProvider {
        crate::providers::anthropic::AnthropicProvider::new(
            self.api_key.clone(),
            self.model.clone(),
            None,
        )
        .with_retry_policy(self.retry_policy.clone())
    }

    #[cfg(feature = "openai")]
    fn build_openai_provider(&self) -> crate::providers::openai::OpenAIProvider {
        crate::providers::openai::OpenAIProvider::new(
            self.api_key.clone(),
            self.model.clone(),
            None,
        )
        .with_retry_policy(self.retry_policy.clone())
    }

    #[cfg(feature = "minimax")]
    fn build_minimax_provider(&self) -> crate::providers::minimax::MinimaxProvider {
        crate::providers::minimax::MinimaxProvider::new(
            self.api_key.clone(),
            self.model.clone(),
            None,
        )
        .with_retry_policy(self.retry_policy.clone())
    }
}

#[derive(Debug, Default, Clone)]
pub struct ClientBuilder {
    provider: Option<Provider>,
    api_key: Option<String>,
    model: Option<String>,
    retry_policy: Option<RetryPolicy>,
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

    pub fn retry_policy(mut self, retry_policy: RetryPolicy) -> Self {
        self.retry_policy = Some(retry_policy);
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
            retry_policy: self.retry_policy.unwrap_or_default(),
        })
    }
}
