use crate::error::MotosanError;
use crate::providers::Provider;
use crate::retry::RetryPolicy;
use crate::stream::BoxStream;
use crate::think_stripper::ThinkStripper;
use crate::types::{ChatRequest, ChatResponse, Message, StreamEvent, StreamEventType};
use std::time::Duration;

#[derive(Debug, Clone)]
pub struct Client {
    provider: Provider,
    api_key: String,
    model: Option<String>,
    openai_auth_header: Option<String>,
    openai_responses_fallback: bool,
    minimax_expose_reasoning: bool,
    ollama_base_url: String,
    ollama_native: bool,
    ollama_think: Option<String>,
    ollama_keep_alive: Option<String>,
    ollama_num_ctx: Option<u32>,
    retry_policy: RetryPolicy,
    stream_read_timeout: Option<Duration>,
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

    pub fn stream_read_timeout(&self) -> Option<Duration> {
        self.stream_read_timeout
    }

    pub fn minimax_expose_reasoning(&self) -> bool {
        self.minimax_expose_reasoning
    }

    pub fn openai_auth_header(&self) -> Option<&str> {
        self.openai_auth_header.as_deref()
    }

    pub fn openai_responses_fallback(&self) -> bool {
        self.openai_responses_fallback
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

        let raw = self.dispatch_stream(request_builder.build()).await?;
        Ok(Self::wrap_with_think_stripper(raw))
    }

    pub async fn stream_with(&self, request: ChatRequest) -> Result<BoxStream, MotosanError> {
        let raw = self.dispatch_stream(request).await?;
        Ok(Self::wrap_with_think_stripper(raw))
    }

    /// Stream a chat request and collect the full response into a [`ChatResponse`].
    ///
    /// This is a convenience wrapper around [`stream`](Self::stream) +
    /// [`collect_stream`](crate::stream::collect_stream) that removes the
    /// boilerplate of manually consuming stream events.
    #[cfg(any(
        feature = "anthropic",
        feature = "openai",
        feature = "minimax",
        feature = "ollama_native"
    ))]
    pub async fn stream_collect(
        &self,
        messages: Vec<Message>,
    ) -> Result<ChatResponse, MotosanError> {
        let stream = self.stream(messages).await?;
        let mut response = crate::stream::collect_stream(stream).await;
        if let Some(model) = &self.model {
            response.model = model.clone();
        }
        Ok(response)
    }

    /// Stream a fully-configured [`ChatRequest`] and collect the response.
    ///
    /// Like [`stream_collect`](Self::stream_collect) but accepts an already-
    /// built request for full control over system prompt, tools, temperature,
    /// etc.
    #[cfg(any(
        feature = "anthropic",
        feature = "openai",
        feature = "minimax",
        feature = "ollama_native"
    ))]
    pub async fn stream_collect_with(
        &self,
        request: ChatRequest,
    ) -> Result<ChatResponse, MotosanError> {
        let model_hint = request
            .model
            .clone()
            .or_else(|| self.model.clone())
            .unwrap_or_default();
        let stream = self.stream_with(request).await?;
        let mut response = crate::stream::collect_stream(stream).await;
        if response.model.is_empty() {
            response.model = model_hint;
        }
        Ok(response)
    }

    fn wrap_with_think_stripper(raw: BoxStream) -> BoxStream {
        Box::pin(ThinkStripperStream {
            inner: raw,
            stripper: ThinkStripper::new(),
        })
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
            Provider::Ollama => {
                #[cfg(feature = "ollama_native")]
                {
                    if self.ollama_native {
                        use crate::providers::ProviderImpl;
                        return self.build_ollama_native_provider().chat(request).await;
                    }
                }
                #[cfg(feature = "ollama")]
                {
                    use crate::providers::ProviderImpl;
                    return self.build_ollama_provider().chat(request).await;
                }
                #[cfg(not(feature = "ollama"))]
                {
                    let _ = request;
                    return Err(Self::feature_not_enabled("ollama"));
                }
            }
        }
    }

    async fn dispatch_stream(&self, request: ChatRequest) -> Result<BoxStream, MotosanError> {
        let raw = self.dispatch_stream_inner(request).await?;
        #[cfg(any(
            feature = "anthropic",
            feature = "openai",
            feature = "minimax",
            feature = "ollama_native"
        ))]
        if let Some(timeout) = self.stream_read_timeout {
            return Ok(Box::pin(ReadTimeoutStream::new(raw, timeout)));
        }
        Ok(raw)
    }

    async fn dispatch_stream_inner(&self, request: ChatRequest) -> Result<BoxStream, MotosanError> {
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
            Provider::Ollama => {
                #[cfg(feature = "ollama_native")]
                {
                    if self.ollama_native {
                        use crate::providers::ProviderImpl;
                        return self.build_ollama_native_provider().stream(request).await;
                    }
                }
                #[cfg(feature = "ollama")]
                {
                    use crate::providers::ProviderImpl;
                    return self.build_ollama_provider().stream(request).await;
                }
                #[cfg(not(feature = "ollama"))]
                {
                    let _ = request;
                    return Err(Self::feature_not_enabled("ollama"));
                }
            }
        }
    }

    #[cfg(any(
        not(feature = "anthropic"),
        not(feature = "openai"),
        not(feature = "minimax"),
        not(feature = "ollama"),
        not(feature = "ollama_native")
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
        use crate::providers::openai::{OpenAIAuthStyle, OpenAIProvider};

        let auth_style = match self.openai_auth_header.as_deref() {
            None => OpenAIAuthStyle::Bearer,
            Some(header) if header.eq_ignore_ascii_case("x-api-key") => OpenAIAuthStyle::XApiKey,
            Some(header) => OpenAIAuthStyle::Custom(header.to_string()),
        };

        OpenAIProvider::new(self.api_key.clone(), self.model.clone(), None)
            .with_auth_style(auth_style)
            .with_responses_fallback(self.openai_responses_fallback)
            .with_retry_policy(self.retry_policy.clone())
    }

    #[cfg(feature = "minimax")]
    fn build_minimax_provider(&self) -> crate::providers::minimax::MinimaxProvider {
        crate::providers::minimax::MinimaxProvider::new(
            self.api_key.clone(),
            self.model.clone(),
            None,
        )
        .with_expose_reasoning(self.minimax_expose_reasoning)
        .with_retry_policy(self.retry_policy.clone())
    }

    #[cfg(feature = "ollama")]
    fn build_ollama_provider(&self) -> crate::providers::openai::OpenAIProvider {
        use crate::providers::openai::{OpenAIAuthStyle, OpenAIProvider};

        let model = self
            .model
            .clone()
            .unwrap_or_else(|| crate::models::DEFAULT_OLLAMA_MODEL.to_string());

        OpenAIProvider::new(
            "".to_string(),
            Some(model),
            Some(self.ollama_base_url.clone()),
        )
        .with_auth_style(OpenAIAuthStyle::Bearer)
        .with_retry_policy(self.retry_policy.clone())
    }

    #[cfg(feature = "ollama_native")]
    fn build_ollama_native_provider(&self) -> crate::providers::ollama::OllamaProvider {
        let model = self
            .model
            .clone()
            .unwrap_or_else(|| crate::models::DEFAULT_OLLAMA_MODEL.to_string());
        crate::providers::ollama::OllamaProvider::new(model, self.ollama_base_url.clone())
            .with_think(self.ollama_think.clone())
            .with_keep_alive(self.ollama_keep_alive.clone())
            .with_num_ctx(self.ollama_num_ctx)
            .with_retry_policy(self.retry_policy.clone())
    }
}

#[derive(Debug, Default, Clone)]
pub struct ClientBuilder {
    provider: Option<Provider>,
    api_key: Option<String>,
    model: Option<String>,
    openai_auth_header: Option<String>,
    openai_responses_fallback: Option<bool>,
    minimax_expose_reasoning: Option<bool>,
    ollama_base_url: Option<String>,
    ollama_native: Option<bool>,
    ollama_think: Option<String>,
    ollama_keep_alive: Option<String>,
    ollama_num_ctx: Option<u32>,
    retry_policy: Option<RetryPolicy>,
    stream_read_timeout_secs: Option<u64>,
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

    pub fn openai_auth_bearer(mut self) -> Self {
        self.openai_auth_header = None;
        self
    }

    pub fn openai_auth_x_api_key(mut self) -> Self {
        self.openai_auth_header = Some("x-api-key".to_string());
        self
    }

    pub fn openai_auth_custom_header(mut self, header_name: impl Into<String>) -> Self {
        self.openai_auth_header = Some(header_name.into());
        self
    }

    pub fn openai_responses_fallback(mut self, enabled: bool) -> Self {
        self.openai_responses_fallback = Some(enabled);
        self
    }

    pub fn minimax_expose_reasoning(mut self, minimax_expose_reasoning: bool) -> Self {
        self.minimax_expose_reasoning = Some(minimax_expose_reasoning);
        self
    }

    pub fn ollama_base_url(mut self, base_url: impl Into<String>) -> Self {
        self.ollama_base_url = Some(base_url.into());
        self
    }

    pub fn ollama_native(mut self, native: bool) -> Self {
        self.ollama_native = Some(native);
        self
    }

    pub fn ollama_think(mut self, think: impl Into<String>) -> Self {
        self.ollama_think = Some(think.into());
        self
    }

    pub fn ollama_keep_alive(mut self, duration: impl Into<String>) -> Self {
        self.ollama_keep_alive = Some(duration.into());
        self
    }

    pub fn ollama_num_ctx(mut self, tokens: u32) -> Self {
        self.ollama_num_ctx = Some(tokens);
        self
    }

    pub fn stream_read_timeout_secs(mut self, secs: u64) -> Self {
        self.stream_read_timeout_secs = Some(secs);
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
            openai_auth_header: self.openai_auth_header,
            openai_responses_fallback: self.openai_responses_fallback.unwrap_or(false),
            minimax_expose_reasoning: self.minimax_expose_reasoning.unwrap_or(false),
            ollama_base_url: self
                .ollama_base_url
                .unwrap_or_else(|| "http://localhost:11434".to_string()),
            ollama_native: self.ollama_native.unwrap_or(false),
            ollama_think: self.ollama_think,
            ollama_keep_alive: self.ollama_keep_alive,
            ollama_num_ctx: self.ollama_num_ctx,
            retry_policy: self.retry_policy.unwrap_or_default(),
            stream_read_timeout: self.stream_read_timeout_secs.map(Duration::from_secs),
        })
    }
}

/// Stream wrapper that terminates the stream if no event arrives within the
/// configured read timeout. This prevents the client from hanging indefinitely
/// when a provider stops sending SSE events mid-stream.
#[cfg(any(
    feature = "anthropic",
    feature = "openai",
    feature = "minimax",
    feature = "ollama_native"
))]
struct ReadTimeoutStream {
    inner: BoxStream,
    timeout: Duration,
    deadline: std::pin::Pin<Box<tokio::time::Sleep>>,
}

#[cfg(any(
    feature = "anthropic",
    feature = "openai",
    feature = "minimax",
    feature = "ollama_native"
))]
impl ReadTimeoutStream {
    fn new(inner: BoxStream, timeout: Duration) -> Self {
        Self {
            inner,
            timeout,
            deadline: Box::pin(tokio::time::sleep(timeout)),
        }
    }
}

#[cfg(any(
    feature = "anthropic",
    feature = "openai",
    feature = "minimax",
    feature = "ollama_native"
))]
impl futures_core::Stream for ReadTimeoutStream {
    type Item = StreamEvent;

    fn poll_next(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Self::Item>> {
        use std::future::Future;
        use std::task::Poll;

        match self.inner.as_mut().poll_next(cx) {
            Poll::Ready(Some(event)) => {
                let timeout = self.timeout;
                self.deadline
                    .as_mut()
                    .reset(tokio::time::Instant::now() + timeout);
                Poll::Ready(Some(event))
            }
            Poll::Ready(None) => Poll::Ready(None),
            Poll::Pending => match self.deadline.as_mut().poll(cx) {
                Poll::Ready(()) => Poll::Ready(None),
                Poll::Pending => Poll::Pending,
            },
        }
    }
}

/// Stream wrapper that filters `<think>...</think>` tags from text events.
struct ThinkStripperStream {
    inner: BoxStream,
    stripper: ThinkStripper,
}

impl futures_core::Stream for ThinkStripperStream {
    type Item = StreamEvent;

    fn poll_next(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Self::Item>> {
        use std::task::Poll;
        loop {
            match self.inner.as_mut().poll_next(cx) {
                Poll::Ready(Some(event)) => {
                    if event.event_type == StreamEventType::Text && !event.content.is_empty() {
                        let clean = self.stripper.feed(&event.content);
                        if clean.is_empty() {
                            continue; // buffering, skip this event
                        }
                        return Poll::Ready(Some(StreamEvent::text(clean)));
                    }
                    return Poll::Ready(Some(event));
                }
                Poll::Ready(None) => {
                    let remaining = self.stripper.flush();
                    if !remaining.is_empty() {
                        return Poll::Ready(Some(StreamEvent::text(remaining)));
                    }
                    return Poll::Ready(None);
                }
                Poll::Pending => return Poll::Pending,
            }
        }
    }
}
