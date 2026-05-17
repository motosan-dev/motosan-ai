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
    openai_chat_url: Option<String>,
    openai_responses_url: Option<String>,
    anthropic_base_url: Option<String>,
    minimax_base_url: Option<String>,
    ollama_base_url: String,
    ollama_native: bool,
    ollama_think: Option<String>,
    ollama_keep_alive: Option<String>,
    ollama_num_ctx: Option<u32>,
    retry_policy: RetryPolicy,
    stream_read_timeout: Option<Duration>,
    /// Pre-built Claude Code provider instance used when `provider ==
    /// Provider::ClaudeCode`. Configured via [`ClientBuilder::claude_code`].
    /// If `None`, a default [`ClaudeCodeProvider::new`] is used at dispatch
    /// time.
    #[cfg(feature = "claude-code")]
    claude_code: Option<crate::providers::claude_code::ClaudeCodeProvider>,
    /// Pre-built Codex CLI provider instance used when `provider ==
    /// Provider::CodexCli`. Configured via [`ClientBuilder::codex_cli`].
    #[cfg(feature = "codex-cli")]
    codex_cli: Option<crate::providers::codex_cli::CodexCliProvider>,
    /// Pre-built Gemini CLI provider instance used when `provider ==
    /// Provider::GeminiCli`. Configured via [`ClientBuilder::gemini_cli`].
    #[cfg(feature = "gemini-cli")]
    gemini_cli: Option<crate::providers::gemini_cli::GeminiCliProvider>,
    /// Pre-built Gemini Code Assist provider instance used when `provider ==
    /// Provider::GeminiCodeAssist`. Configured via
    /// [`ClientBuilder::gemini_code_assist`].
    #[cfg(feature = "gemini-code-assist")]
    gemini_code_assist: Option<crate::providers::gemini_code_assist::GeminiCodeAssistProvider>,
    /// GCP project ID used to construct a [`GeminiCodeAssistProvider`] on demand
    /// when no pre-built provider is available. Defaults to empty string when
    /// not set (which will produce an API error on first use).
    #[cfg(feature = "gemini-code-assist")]
    gemini_code_assist_project_id: Option<String>,
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

    pub fn openai_auth_header(&self) -> Option<&str> {
        self.openai_auth_header.as_deref()
    }

    pub fn openai_responses_fallback(&self) -> bool {
        self.openai_responses_fallback
    }

    /// The custom Anthropic base URL, if one was set via
    /// [`ClientBuilder::anthropic_base_url`]. Returns `None` when the client
    /// uses the default `https://api.anthropic.com`.
    pub fn anthropic_base_url(&self) -> Option<&str> {
        self.anthropic_base_url.as_deref()
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
        feature = "ollama_native",
        feature = "gemini",
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
        feature = "ollama_native",
        feature = "gemini",
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
                    let p = self.build_anthropic_provider();
                    p.validate_request(&request)?;
                    return p.chat(request).await;
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
                    let p = self.build_openai_provider();
                    p.validate_request(&request)?;
                    return p.chat(request).await;
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
                    let p = self.build_minimax_provider();
                    p.validate_request(&request)?;
                    return p.chat(request).await;
                }
                #[cfg(not(feature = "minimax"))]
                {
                    let _ = request;
                    return Err(Self::feature_not_enabled("minimax"));
                }
            }
            Provider::Ollama => {
                #[cfg(feature = "ollama")]
                {
                    use crate::providers::ProviderImpl;
                    // Auto-route to OllamaProvider (native /api/chat) when
                    // ollama_native is explicitly enabled OR any of the
                    // Ollama-specific tuning fields is set, since the
                    // OpenAI-compat /v1/chat/completions endpoint silently
                    // drops keep_alive / options.num_ctx / think
                    // server-side. Otherwise stay on the OpenAI-compat
                    // path for backwards compatibility.
                    //
                    // Capability trade-off: OllamaProvider is text-only
                    // (no image capability) while the OpenAI-compat path
                    // declares with_image(). Auto-switching strips image
                    // capability — the wrapped validate_request error
                    // below tells the caller WHY their image input
                    // stopped working.
                    let needs_native = self.ollama_native
                        || self.ollama_keep_alive.is_some()
                        || self.ollama_num_ctx.is_some()
                        || self.ollama_think.is_some();
                    if needs_native {
                        let p = self.build_ollama_native_provider();
                        p.validate_request(&request).map_err(|e| match e {
                            MotosanError::UnsupportedFeature(msg) => MotosanError::UnsupportedFeature(format!(
                                "{msg} — Provider::Ollama was auto-routed to the native /api/chat endpoint \
                                 because one of ollama_keep_alive / ollama_num_ctx / ollama_think is set, \
                                 and the native endpoint is text-only. Either remove the tuning field(s) to \
                                 stay on the OpenAI-compat path (which supports images), or remove the image \
                                 input."
                            )),
                            other => other,
                        })?;
                        p.chat(request).await
                    } else {
                        let p = self.build_ollama_provider();
                        p.validate_request(&request)?;
                        p.chat(request).await
                    }
                }
                #[cfg(not(feature = "ollama"))]
                {
                    let _ = request;
                    Err(Self::feature_not_enabled("ollama"))
                }
            }
            Provider::ClaudeCode => {
                #[cfg(feature = "claude-code")]
                {
                    use crate::providers::ProviderImpl;
                    let p = self.build_claude_code_provider();
                    p.validate_request(&request)?;
                    return p.chat(request).await;
                }
                #[cfg(not(feature = "claude-code"))]
                {
                    let _ = request;
                    return Err(Self::feature_not_enabled("claude-code"));
                }
            }
            Provider::CodexCli => {
                #[cfg(feature = "codex-cli")]
                {
                    use crate::providers::ProviderImpl;
                    let p = self.build_codex_cli_provider();
                    p.validate_request(&request)?;
                    return p.chat(request).await;
                }
                #[cfg(not(feature = "codex-cli"))]
                {
                    let _ = request;
                    return Err(Self::feature_not_enabled("codex-cli"));
                }
            }
            Provider::GeminiCli => {
                #[cfg(feature = "gemini-cli")]
                {
                    use crate::providers::ProviderImpl;
                    let p = self.build_gemini_cli_provider();
                    p.validate_request(&request)?;
                    return p.chat(request).await;
                }
                #[cfg(not(feature = "gemini-cli"))]
                {
                    let _ = request;
                    return Err(Self::feature_not_enabled("gemini-cli"));
                }
            }
            Provider::Gemini => {
                #[cfg(feature = "gemini")]
                {
                    use crate::providers::ProviderImpl;
                    let p = self.build_gemini_provider();
                    p.validate_request(&request)?;
                    return p.chat(request).await;
                }
                #[cfg(not(feature = "gemini"))]
                {
                    let _ = request;
                    return Err(Self::feature_not_enabled("gemini"));
                }
            }
            Provider::GeminiCodeAssist => {
                #[cfg(feature = "gemini-code-assist")]
                {
                    use crate::providers::ProviderImpl;
                    let p = self.build_gemini_code_assist_provider();
                    p.validate_request(&request)?;
                    return p.chat(request).await;
                }
                #[cfg(not(feature = "gemini-code-assist"))]
                {
                    let _ = request;
                    return Err(Self::feature_not_enabled("gemini-code-assist"));
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
            feature = "ollama_native",
            feature = "gemini",
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
                    let p = self.build_anthropic_provider();
                    p.validate_request(&request)?;
                    return p.stream(request).await;
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
                    let p = self.build_openai_provider();
                    p.validate_request(&request)?;
                    return p.stream(request).await;
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
                    let p = self.build_minimax_provider();
                    p.validate_request(&request)?;
                    return p.stream(request).await;
                }
                #[cfg(not(feature = "minimax"))]
                {
                    let _ = request;
                    return Err(Self::feature_not_enabled("minimax"));
                }
            }
            Provider::Ollama => {
                #[cfg(feature = "ollama")]
                {
                    if self.ollama_native {
                        use crate::providers::ProviderImpl;
                        let p = self.build_ollama_native_provider();
                        p.validate_request(&request)?;
                        return p.stream(request).await;
                    }
                }
                #[cfg(feature = "ollama")]
                {
                    use crate::providers::ProviderImpl;
                    let p = self.build_ollama_provider();
                    p.validate_request(&request)?;
                    return p.stream(request).await;
                }
                #[cfg(not(feature = "ollama"))]
                {
                    let _ = request;
                    return Err(Self::feature_not_enabled("ollama"));
                }
            }
            Provider::ClaudeCode => {
                #[cfg(feature = "claude-code")]
                {
                    use crate::providers::ProviderImpl;
                    let p = self.build_claude_code_provider();
                    p.validate_request(&request)?;
                    return p.stream(request).await;
                }
                #[cfg(not(feature = "claude-code"))]
                {
                    let _ = request;
                    return Err(Self::feature_not_enabled("claude-code"));
                }
            }
            Provider::CodexCli => {
                #[cfg(feature = "codex-cli")]
                {
                    use crate::providers::ProviderImpl;
                    let p = self.build_codex_cli_provider();
                    p.validate_request(&request)?;
                    return p.stream(request).await;
                }
                #[cfg(not(feature = "codex-cli"))]
                {
                    let _ = request;
                    return Err(Self::feature_not_enabled("codex-cli"));
                }
            }
            Provider::GeminiCli => {
                #[cfg(feature = "gemini-cli")]
                {
                    use crate::providers::ProviderImpl;
                    let p = self.build_gemini_cli_provider();
                    p.validate_request(&request)?;
                    return p.stream(request).await;
                }
                #[cfg(not(feature = "gemini-cli"))]
                {
                    let _ = request;
                    return Err(Self::feature_not_enabled("gemini-cli"));
                }
            }
            Provider::Gemini => {
                #[cfg(feature = "gemini")]
                {
                    use crate::providers::ProviderImpl;
                    let p = self.build_gemini_provider();
                    p.validate_request(&request)?;
                    return p.stream(request).await;
                }
                #[cfg(not(feature = "gemini"))]
                {
                    let _ = request;
                    return Err(Self::feature_not_enabled("gemini"));
                }
            }
            Provider::GeminiCodeAssist => {
                #[cfg(feature = "gemini-code-assist")]
                {
                    use crate::providers::ProviderImpl;
                    let p = self.build_gemini_code_assist_provider();
                    p.validate_request(&request)?;
                    return p.stream(request).await;
                }
                #[cfg(not(feature = "gemini-code-assist"))]
                {
                    let _ = request;
                    return Err(Self::feature_not_enabled("gemini-code-assist"));
                }
            }
        }
    }

    #[cfg(any(
        not(feature = "anthropic"),
        not(feature = "openai"),
        not(feature = "minimax"),
        not(feature = "ollama"),
        not(feature = "ollama_native"),
        not(feature = "claude-code"),
        not(feature = "codex-cli"),
        not(feature = "gemini-cli"),
        not(feature = "gemini"),
        not(feature = "gemini-code-assist"),
    ))]
    fn feature_not_enabled(provider: &str) -> MotosanError {
        MotosanError::Config(format!("{provider} feature is not enabled"))
    }

    #[cfg(feature = "anthropic")]
    fn build_anthropic_provider(&self) -> crate::providers::anthropic::AnthropicProvider {
        crate::providers::anthropic::AnthropicProvider::new(
            self.api_key.clone(),
            self.model.clone(),
            self.anthropic_base_url.clone(),
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

        let mut provider = OpenAIProvider::new(self.api_key.clone(), self.model.clone())
            .with_auth_style(auth_style)
            .with_responses_fallback(self.openai_responses_fallback)
            .with_retry_policy(self.retry_policy.clone());

        if let Some(ref url) = self.openai_chat_url {
            provider = provider.with_chat_url(url.clone());
        }
        if let Some(ref url) = self.openai_responses_url {
            provider = provider.with_responses_url(url.clone());
        }

        provider
    }

    #[cfg(feature = "minimax")]
    fn build_minimax_provider(&self) -> crate::providers::anthropic::AnthropicProvider {
        use crate::types::ProviderCapabilities;

        let model = self
            .model
            .clone()
            .unwrap_or_else(|| "MiniMax-M2.7".to_string());
        let base_url = self
            .minimax_base_url
            .clone()
            .unwrap_or_else(|| "https://api.minimax.io/anthropic".to_string());

        crate::providers::anthropic::AnthropicProvider::new(
            self.api_key.clone(),
            Some(model),
            Some(base_url),
        )
        .with_capabilities(ProviderCapabilities::text_only())
        .with_retry_policy(self.retry_policy.clone())
    }

    #[cfg(feature = "ollama")]
    // Uses OpenAIProvider under the hood, which declares with_image() capabilities.
    // Multimodal accuracy here is model-dependent; use the ollama_native path for text-only safety.
    fn build_ollama_provider(&self) -> crate::providers::openai::OpenAIProvider {
        use crate::providers::openai::{OpenAIAuthStyle, OpenAIProvider};

        let model = self
            .model
            .clone()
            .unwrap_or_else(|| crate::models::DEFAULT_OLLAMA_MODEL.to_string());

        let chat_url = format!(
            "{}/v1/chat/completions",
            self.ollama_base_url.trim_end_matches('/')
        );

        OpenAIProvider::new("".to_string(), Some(model))
            .with_chat_url(chat_url)
            .with_auth_style(OpenAIAuthStyle::Bearer)
            .with_retry_policy(self.retry_policy.clone())
    }

    #[cfg(feature = "ollama")]
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

    #[cfg(feature = "claude-code")]
    fn build_claude_code_provider(&self) -> crate::providers::claude_code::ClaudeCodeProvider {
        // Prefer the pre-built instance the caller supplied via
        // `ClientBuilder::claude_code()`; fall back to a default instance
        // if none was provided. Client-level `.model()` is forwarded to
        // the default provider.
        match self.claude_code.clone() {
            Some(provider) => provider,
            None => {
                let mut provider = crate::providers::claude_code::ClaudeCodeProvider::new();
                if let Some(ref m) = self.model {
                    provider = provider.model(m.clone());
                }
                provider
            }
        }
    }

    #[cfg(feature = "gemini")]
    fn build_gemini_provider(&self) -> crate::providers::gemini::GeminiProvider {
        crate::providers::gemini::GeminiProvider::new(
            self.api_key.clone(),
            self.model.clone(),
            None,
        )
        .with_retry_policy(self.retry_policy.clone())
    }

    #[cfg(feature = "gemini-code-assist")]
    fn build_gemini_code_assist_provider(
        &self,
    ) -> crate::providers::gemini_code_assist::GeminiCodeAssistProvider {
        match self.gemini_code_assist.clone() {
            Some(provider) => provider,
            None => crate::providers::gemini_code_assist::GeminiCodeAssistProvider::new(
                self.api_key.clone(),
                self.gemini_code_assist_project_id
                    .clone()
                    .unwrap_or_default(),
                self.model.clone(),
                None,
            )
            .with_retry_policy(self.retry_policy.clone()),
        }
    }

    #[cfg(feature = "gemini-cli")]
    fn build_gemini_cli_provider(&self) -> crate::providers::gemini_cli::GeminiCliProvider {
        match self.gemini_cli.clone() {
            Some(provider) => provider,
            None => {
                let mut provider = crate::providers::gemini_cli::GeminiCliProvider::new();
                if let Some(ref m) = self.model {
                    provider = provider.model(m.clone());
                }
                provider
            }
        }
    }

    #[cfg(feature = "codex-cli")]
    fn build_codex_cli_provider(&self) -> crate::providers::codex_cli::CodexCliProvider {
        match self.codex_cli.clone() {
            Some(provider) => provider,
            None => {
                let mut provider = crate::providers::codex_cli::CodexCliProvider::new();
                if let Some(ref m) = self.model {
                    provider = provider.model(m.clone());
                }
                provider
            }
        }
    }
}

#[derive(Debug, Default, Clone)]
pub struct ClientBuilder {
    provider: Option<Provider>,
    api_key: Option<String>,
    model: Option<String>,
    openai_auth_header: Option<String>,
    openai_responses_fallback: Option<bool>,
    openai_chat_url: Option<String>,
    openai_responses_url: Option<String>,
    anthropic_base_url: Option<String>,
    minimax_base_url: Option<String>,
    ollama_base_url: Option<String>,
    ollama_native: Option<bool>,
    ollama_think: Option<String>,
    ollama_keep_alive: Option<String>,
    ollama_num_ctx: Option<u32>,
    retry_policy: Option<RetryPolicy>,
    stream_read_timeout_secs: Option<u64>,
    #[cfg(feature = "claude-code")]
    claude_code: Option<crate::providers::claude_code::ClaudeCodeProvider>,
    #[cfg(feature = "codex-cli")]
    codex_cli: Option<crate::providers::codex_cli::CodexCliProvider>,
    #[cfg(feature = "gemini-cli")]
    gemini_cli: Option<crate::providers::gemini_cli::GeminiCliProvider>,
    #[cfg(feature = "gemini-code-assist")]
    gemini_code_assist: Option<crate::providers::gemini_code_assist::GeminiCodeAssistProvider>,
    #[cfg(feature = "gemini-code-assist")]
    gemini_code_assist_project_id: Option<String>,
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

    /// Override the OpenAI chat completions URL. Pass the full URL the
    /// provider should POST to (e.g. a Groq / DeepSeek / proxy endpoint).
    pub fn openai_chat_url(mut self, url: impl Into<String>) -> Self {
        self.openai_chat_url = Some(url.into());
        self
    }

    /// Override the OpenAI Responses API URL. Only relevant when
    /// [`openai_responses_fallback`](Self::openai_responses_fallback) is
    /// enabled.
    pub fn openai_responses_url(mut self, url: impl Into<String>) -> Self {
        self.openai_responses_url = Some(url.into());
        self
    }

    /// Override the Anthropic base URL. Useful for staging, on-prem
    /// proxies, or Anthropic-compatible third-party endpoints. Defaults to
    /// `https://api.anthropic.com` when unset.
    pub fn anthropic_base_url(mut self, anthropic_base_url: impl Into<String>) -> Self {
        self.anthropic_base_url = Some(anthropic_base_url.into());
        self
    }

    pub fn minimax_base_url(mut self, minimax_base_url: impl Into<String>) -> Self {
        self.minimax_base_url = Some(minimax_base_url.into());
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

    /// Attach a pre-built [`ClaudeCodeProvider`] to use when `Provider::ClaudeCode`
    /// is selected. If not called, the client uses
    /// `ClaudeCodeProvider::new()` with the top-level `.model()` forwarded.
    ///
    /// [`ClaudeCodeProvider`]: crate::providers::claude_code::ClaudeCodeProvider
    #[cfg(feature = "claude-code")]
    pub fn claude_code(
        mut self,
        provider: crate::providers::claude_code::ClaudeCodeProvider,
    ) -> Self {
        self.claude_code = Some(provider);
        self
    }

    /// Attach a pre-built [`CodexCliProvider`] to use when `Provider::CodexCli`
    /// is selected. If not called, the client uses `CodexCliProvider::new()`
    /// with the top-level `.model()` forwarded.
    ///
    /// [`CodexCliProvider`]: crate::providers::codex_cli::CodexCliProvider
    #[cfg(feature = "codex-cli")]
    pub fn codex_cli(mut self, provider: crate::providers::codex_cli::CodexCliProvider) -> Self {
        self.codex_cli = Some(provider);
        self
    }

    /// Attach a pre-built [`GeminiCliProvider`] to use when `Provider::GeminiCli`
    /// is selected. If not called, the client uses `GeminiCliProvider::new()`
    /// with the top-level `.model()` forwarded.
    ///
    /// [`GeminiCliProvider`]: crate::providers::gemini_cli::GeminiCliProvider
    #[cfg(feature = "gemini-cli")]
    pub fn gemini_cli(mut self, provider: crate::providers::gemini_cli::GeminiCliProvider) -> Self {
        self.gemini_cli = Some(provider);
        self
    }

    /// Attach a pre-built [`GeminiCodeAssistProvider`] to use when
    /// `Provider::GeminiCodeAssist` is selected. The provider must already have
    /// the OAuth access token and GCP project ID configured.
    ///
    /// [`GeminiCodeAssistProvider`]: crate::providers::gemini_code_assist::GeminiCodeAssistProvider
    #[cfg(feature = "gemini-code-assist")]
    pub fn gemini_code_assist(
        mut self,
        provider: crate::providers::gemini_code_assist::GeminiCodeAssistProvider,
    ) -> Self {
        self.gemini_code_assist = Some(provider);
        self
    }

    /// Set the GCP project ID used when constructing a [`GeminiCodeAssistProvider`]
    /// from scratch (i.e. when [`gemini_code_assist`](Self::gemini_code_assist)
    /// has not been called). Has no effect if a pre-built provider is provided.
    ///
    /// [`GeminiCodeAssistProvider`]: crate::providers::gemini_code_assist::GeminiCodeAssistProvider
    #[cfg(feature = "gemini-code-assist")]
    pub fn gemini_code_assist_project_id(mut self, project_id: impl Into<String>) -> Self {
        self.gemini_code_assist_project_id = Some(project_id.into());
        self
    }

    pub fn build(self) -> Result<Client, MotosanError> {
        let provider = self
            .provider
            .ok_or_else(|| MotosanError::Config("provider is required".to_string()))?;
        // CLI backends (Claude Code / Codex CLI) authenticate via their own
        // channels (local login state / `CODEX_API_KEY`), so `api_key` is
        // optional when the selected provider is a CLI backend. HTTP
        // providers still require it.
        let api_key_required = !matches!(
            provider,
            Provider::ClaudeCode
                | Provider::CodexCli
                | Provider::GeminiCli
                | Provider::GeminiCodeAssist
        );
        let api_key = match self.api_key {
            Some(k) => k,
            None if api_key_required => {
                return Err(MotosanError::Config("api_key is required".to_string()));
            }
            None => String::new(),
        };

        Ok(Client {
            provider,
            api_key,
            model: self.model,
            openai_auth_header: self.openai_auth_header,
            openai_responses_fallback: self.openai_responses_fallback.unwrap_or(false),
            openai_chat_url: self.openai_chat_url,
            openai_responses_url: self.openai_responses_url,
            anthropic_base_url: self.anthropic_base_url,
            minimax_base_url: self.minimax_base_url,
            ollama_base_url: self
                .ollama_base_url
                .unwrap_or_else(|| "http://localhost:11434".to_string()),
            ollama_native: self.ollama_native.unwrap_or(false),
            ollama_think: self.ollama_think,
            ollama_keep_alive: self.ollama_keep_alive,
            ollama_num_ctx: self.ollama_num_ctx,
            retry_policy: self.retry_policy.unwrap_or_default(),
            stream_read_timeout: self.stream_read_timeout_secs.map(Duration::from_secs),
            #[cfg(feature = "claude-code")]
            claude_code: self.claude_code,
            #[cfg(feature = "codex-cli")]
            codex_cli: self.codex_cli,
            #[cfg(feature = "gemini-cli")]
            gemini_cli: self.gemini_cli,
            #[cfg(feature = "gemini-code-assist")]
            gemini_code_assist: self.gemini_code_assist,
            #[cfg(feature = "gemini-code-assist")]
            gemini_code_assist_project_id: self.gemini_code_assist_project_id,
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
    feature = "ollama_native",
    feature = "gemini",
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
    feature = "ollama_native",
    feature = "gemini",
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
    feature = "ollama_native",
    feature = "gemini",
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

#[cfg(test)]
mod dispatch_validation_tests {
    use super::*;
    use crate::types::Message;

    #[cfg(feature = "ollama_native")]
    #[tokio::test]
    async fn dispatch_chat_rejects_image_for_ollama() {
        let client = Client::builder()
            .provider(crate::providers::Provider::Ollama)
            .api_key("")
            .ollama_base_url("http://localhost:11434")
            .ollama_native(true)
            .build()
            .expect("client build");
        let msg = Message::user_with_image("look", "abc123", "image/png");
        let req = ChatRequest::builder().messages(vec![msg]).build();
        let result = client.chat_with(req).await;
        assert!(matches!(result, Err(MotosanError::UnsupportedFeature(_))));
    }

    #[cfg(feature = "ollama_native")]
    #[tokio::test]
    async fn dispatch_stream_rejects_image_for_ollama() {
        let client = Client::builder()
            .provider(crate::providers::Provider::Ollama)
            .api_key("")
            .ollama_base_url("http://localhost:11434")
            .ollama_native(true)
            .build()
            .expect("client build");
        let msg = Message::user_with_image("look", "abc123", "image/png");
        let req = ChatRequest::builder().messages(vec![msg]).build();
        let result = client.stream_with(req).await;
        assert!(matches!(result, Err(MotosanError::UnsupportedFeature(_))));
    }

    // Validates that the framework-level guard fires before any HTTP call for
    // a text-only HTTP provider. No server needed — UnsupportedFeature is returned
    // from validate_request() before reqwest touches the network.
    #[cfg(feature = "minimax")]
    #[tokio::test]
    async fn dispatch_chat_rejects_image_for_minimax() {
        let client = Client::builder()
            .provider(crate::providers::Provider::Minimax)
            .api_key("fake-key")
            .build()
            .expect("client build");
        let msg = Message::user_with_image("look", "abc123", "image/png");
        let req = ChatRequest::builder().messages(vec![msg]).build();
        let result = client.chat_with(req).await;
        assert!(matches!(result, Err(MotosanError::UnsupportedFeature(_))));
    }

    #[cfg(feature = "minimax")]
    #[tokio::test]
    async fn dispatch_chat_rejects_document_for_minimax() {
        let client = Client::builder()
            .provider(crate::providers::Provider::Minimax)
            .api_key("fake-key")
            .build()
            .expect("client build");
        let msg = Message::user_with_pdf_base64("summarize", "abc123");
        let req = ChatRequest::builder().messages(vec![msg]).build();
        let result = client.chat_with(req).await;
        assert!(matches!(result, Err(MotosanError::UnsupportedFeature(_))));
    }
}
