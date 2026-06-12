use crate::error::MotosanError;
use crate::providers::Provider;
use crate::retry::RetryPolicy;
use crate::stream::BoxStream;
use crate::think_stripper::ThinkStripper;
use crate::types::{ChatRequest, ChatResponse, Message, StreamEvent, StreamEventType};
use std::time::Duration;

#[derive(Debug, Clone)]
#[allow(dead_code)]
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
    /// OAuth access token used to construct a [`ChatGptCodexProvider`] on
    /// demand when `provider == Provider::OpenAiChatGpt`. Configured via
    /// [`ClientBuilder::chatgpt_codex`].
    #[cfg(feature = "chatgpt-codex")]
    chatgpt_codex_access_token: Option<String>,
    /// `chatgpt-account-id` header value paired with the access token.
    #[cfg(feature = "chatgpt-codex")]
    chatgpt_codex_account_id: Option<String>,
    /// Model id sent in the Responses body for the ChatGPT-backend provider.
    #[cfg(feature = "chatgpt-codex")]
    chatgpt_codex_model: Option<String>,
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
        let mut response = crate::stream::collect_stream(stream).await?;
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
        let mut response = crate::stream::collect_stream(stream).await?;
        if response.model.is_empty() {
            response.model = model_hint;
        }
        Ok(response)
    }

    fn wrap_with_think_stripper(raw: BoxStream) -> BoxStream {
        Box::pin(ThinkStripperStream {
            inner: raw,
            stripper: ThinkStripper::new(),
            pending: None,
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
                    p.chat(request).await
                }
                #[cfg(not(feature = "anthropic"))]
                {
                    let _ = request;
                    Err(Self::feature_not_enabled("anthropic"))
                }
            }
            Provider::OpenAI => {
                #[cfg(feature = "openai")]
                {
                    use crate::providers::ProviderImpl;
                    let p = self.build_openai_provider();
                    p.validate_request(&request)?;
                    p.chat(request).await
                }
                #[cfg(not(feature = "openai"))]
                {
                    let _ = request;
                    Err(Self::feature_not_enabled("openai"))
                }
            }
            Provider::Minimax => {
                #[cfg(feature = "minimax")]
                {
                    use crate::providers::ProviderImpl;
                    let p = self.build_minimax_provider();
                    p.validate_request(&request)?;
                    p.chat(request).await
                }
                #[cfg(not(feature = "minimax"))]
                {
                    let _ = request;
                    Err(Self::feature_not_enabled("minimax"))
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
                    p.chat(request).await
                }
                #[cfg(not(feature = "claude-code"))]
                {
                    let _ = request;
                    Err(Self::feature_not_enabled("claude-code"))
                }
            }
            Provider::CodexCli => {
                #[cfg(feature = "codex-cli")]
                {
                    use crate::providers::ProviderImpl;
                    let p = self.build_codex_cli_provider();
                    p.validate_request(&request)?;
                    p.chat(request).await
                }
                #[cfg(not(feature = "codex-cli"))]
                {
                    let _ = request;
                    Err(Self::feature_not_enabled("codex-cli"))
                }
            }
            Provider::GeminiCli => {
                #[cfg(feature = "gemini-cli")]
                {
                    use crate::providers::ProviderImpl;
                    let p = self.build_gemini_cli_provider();
                    p.validate_request(&request)?;
                    p.chat(request).await
                }
                #[cfg(not(feature = "gemini-cli"))]
                {
                    let _ = request;
                    Err(Self::feature_not_enabled("gemini-cli"))
                }
            }
            Provider::Gemini => {
                #[cfg(feature = "gemini")]
                {
                    use crate::providers::ProviderImpl;
                    let p = self.build_gemini_provider();
                    p.validate_request(&request)?;
                    p.chat(request).await
                }
                #[cfg(not(feature = "gemini"))]
                {
                    let _ = request;
                    Err(Self::feature_not_enabled("gemini"))
                }
            }
            Provider::GeminiCodeAssist => {
                #[cfg(feature = "gemini-code-assist")]
                {
                    use crate::providers::ProviderImpl;
                    let p = self.build_gemini_code_assist_provider();
                    p.validate_request(&request)?;
                    p.chat(request).await
                }
                #[cfg(not(feature = "gemini-code-assist"))]
                {
                    let _ = request;
                    Err(Self::feature_not_enabled("gemini-code-assist"))
                }
            }
            Provider::OpenAiChatGpt => {
                #[cfg(feature = "chatgpt-codex")]
                {
                    use crate::providers::ProviderImpl;
                    let p = self.build_chatgpt_codex_provider();
                    p.validate_request(&request)?;
                    p.chat(request).await
                }
                #[cfg(not(feature = "chatgpt-codex"))]
                {
                    let _ = request;
                    Err(Self::feature_not_enabled("chatgpt-codex"))
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
                    p.stream(request).await
                }
                #[cfg(not(feature = "anthropic"))]
                {
                    let _ = request;
                    Err(Self::feature_not_enabled("anthropic"))
                }
            }
            Provider::OpenAI => {
                #[cfg(feature = "openai")]
                {
                    use crate::providers::ProviderImpl;
                    let p = self.build_openai_provider();
                    p.validate_request(&request)?;
                    p.stream(request).await
                }
                #[cfg(not(feature = "openai"))]
                {
                    let _ = request;
                    Err(Self::feature_not_enabled("openai"))
                }
            }
            Provider::Minimax => {
                #[cfg(feature = "minimax")]
                {
                    use crate::providers::ProviderImpl;
                    let p = self.build_minimax_provider();
                    p.validate_request(&request)?;
                    p.stream(request).await
                }
                #[cfg(not(feature = "minimax"))]
                {
                    let _ = request;
                    Err(Self::feature_not_enabled("minimax"))
                }
            }
            Provider::Ollama => {
                #[cfg(feature = "ollama")]
                {
                    use crate::providers::ProviderImpl;
                    // Same auto-switch + capability trade-off as
                    // dispatch_chat — keep the routing condition
                    // identical so chat and stream are never split.
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
                        p.stream(request).await
                    } else {
                        let p = self.build_ollama_provider();
                        p.validate_request(&request)?;
                        p.stream(request).await
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
                    p.stream(request).await
                }
                #[cfg(not(feature = "claude-code"))]
                {
                    let _ = request;
                    Err(Self::feature_not_enabled("claude-code"))
                }
            }
            Provider::CodexCli => {
                #[cfg(feature = "codex-cli")]
                {
                    use crate::providers::ProviderImpl;
                    let p = self.build_codex_cli_provider();
                    p.validate_request(&request)?;
                    p.stream(request).await
                }
                #[cfg(not(feature = "codex-cli"))]
                {
                    let _ = request;
                    Err(Self::feature_not_enabled("codex-cli"))
                }
            }
            Provider::GeminiCli => {
                #[cfg(feature = "gemini-cli")]
                {
                    use crate::providers::ProviderImpl;
                    let p = self.build_gemini_cli_provider();
                    p.validate_request(&request)?;
                    p.stream(request).await
                }
                #[cfg(not(feature = "gemini-cli"))]
                {
                    let _ = request;
                    Err(Self::feature_not_enabled("gemini-cli"))
                }
            }
            Provider::Gemini => {
                #[cfg(feature = "gemini")]
                {
                    use crate::providers::ProviderImpl;
                    let p = self.build_gemini_provider();
                    p.validate_request(&request)?;
                    p.stream(request).await
                }
                #[cfg(not(feature = "gemini"))]
                {
                    let _ = request;
                    Err(Self::feature_not_enabled("gemini"))
                }
            }
            Provider::GeminiCodeAssist => {
                #[cfg(feature = "gemini-code-assist")]
                {
                    use crate::providers::ProviderImpl;
                    let p = self.build_gemini_code_assist_provider();
                    p.validate_request(&request)?;
                    p.stream(request).await
                }
                #[cfg(not(feature = "gemini-code-assist"))]
                {
                    let _ = request;
                    Err(Self::feature_not_enabled("gemini-code-assist"))
                }
            }
            Provider::OpenAiChatGpt => {
                #[cfg(feature = "chatgpt-codex")]
                {
                    use crate::providers::ProviderImpl;
                    let p = self.build_chatgpt_codex_provider();
                    p.validate_request(&request)?;
                    p.stream(request).await
                }
                #[cfg(not(feature = "chatgpt-codex"))]
                {
                    let _ = request;
                    Err(Self::feature_not_enabled("chatgpt-codex"))
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
        not(feature = "chatgpt-codex"),
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

    #[cfg(feature = "chatgpt-codex")]
    fn build_chatgpt_codex_provider(
        &self,
    ) -> crate::providers::chatgpt_codex::ChatGptCodexProvider {
        // Constructed on demand from the stored OAuth credentials. The
        // client-level `.model()` is used as a fallback when no
        // chatgpt-codex-specific model was supplied via `chatgpt_codex(...)`.
        let model = self
            .chatgpt_codex_model
            .clone()
            .or_else(|| self.model.clone())
            .unwrap_or_default();
        crate::providers::chatgpt_codex::ChatGptCodexProvider::new(
            self.chatgpt_codex_access_token.clone().unwrap_or_default(),
            self.chatgpt_codex_account_id.clone().unwrap_or_default(),
            model,
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
    #[cfg(feature = "chatgpt-codex")]
    chatgpt_codex_access_token: Option<String>,
    #[cfg(feature = "chatgpt-codex")]
    chatgpt_codex_account_id: Option<String>,
    #[cfg(feature = "chatgpt-codex")]
    chatgpt_codex_model: Option<String>,
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

    /// Force the Ollama provider to use the native `/api/chat` endpoint
    /// instead of the OpenAI-compatible `/v1/chat/completions` path.
    ///
    /// As of 0.15.0, setting this explicitly is **only required** when you
    /// want the native endpoint without setting any tuning fields. Setting
    /// `ollama_think` / `ollama_keep_alive` / `ollama_num_ctx` auto-selects
    /// the native endpoint regardless of this flag, since the OpenAI-compat
    /// endpoint silently drops those fields.
    ///
    /// **Capability trade-off:** the native endpoint is text-only — image
    /// inputs will be rejected. The OpenAI-compatible default supports
    /// images.
    pub fn ollama_native(mut self, native: bool) -> Self {
        self.ollama_native = Some(native);
        self
    }

    /// Set Ollama's `think` parameter controlling whether the model emits
    /// a thinking block. Forwarded only via the native `/api/chat`
    /// endpoint — Ollama's OpenAI-compatible `/v1/chat/completions`
    /// endpoint silently drops this field (verified against
    /// [ollama/openai.go](https://github.com/ollama/ollama/blob/main/openai/openai.go)).
    ///
    /// **Side effect (since 0.15.0):** setting this auto-routes the client
    /// to OllamaProvider (native `/api/chat`) regardless of whether
    /// `ollama_native(true)` was called.
    ///
    /// **Capability trade-off:** OllamaProvider is text-only. If you set
    /// this AND send image content in the request, `validate_request`
    /// will reject the request with a wrapped error explaining the
    /// auto-switch context.
    ///
    /// `ClientBuilder::build()` will return `Err(MotosanError::Config)` if you
    /// set this on a non-`Provider::Ollama` client.
    ///
    /// **Known limitation:** OllamaProvider currently emits this as a
    /// boolean `think: true` regardless of the input string value (see
    /// `providers/ollama.rs:138-140`). The string is accepted for forward
    /// compatibility but not differentiated server-side.
    pub fn ollama_think(mut self, think: impl Into<String>) -> Self {
        self.ollama_think = Some(think.into());
        self
    }

    /// Set Ollama's `keep_alive` duration (e.g. `"5m"`, `"-1"` for forever).
    /// Controls how long Ollama keeps the model loaded after a request.
    /// Forwarded only via the native `/api/chat` endpoint — the
    /// OpenAI-compatible endpoint silently drops this field (verified
    /// against ollama/openai.go).
    ///
    /// **Side effect (since 0.15.0):** setting this auto-routes the client
    /// to OllamaProvider regardless of `ollama_native(true)`.
    ///
    /// **Capability trade-off:** OllamaProvider is text-only. If you set
    /// this AND send image content in the request, `validate_request`
    /// will reject the request with a wrapped error explaining the
    /// auto-switch context.
    ///
    /// `ClientBuilder::build()` will return `Err(MotosanError::Config)` if you
    /// set this on a non-`Provider::Ollama` client.
    pub fn ollama_keep_alive(mut self, duration: impl Into<String>) -> Self {
        self.ollama_keep_alive = Some(duration.into());
        self
    }

    /// Set Ollama's `options.num_ctx` (context window size in tokens).
    /// Forwarded only via the native `/api/chat` endpoint — the
    /// OpenAI-compatible endpoint silently drops nested `options` fields
    /// (verified against ollama/openai.go).
    ///
    /// **Side effect (since 0.15.0):** setting this auto-routes the client
    /// to OllamaProvider regardless of `ollama_native(true)`.
    ///
    /// **Capability trade-off:** OllamaProvider is text-only. If you set
    /// this AND send image content in the request, `validate_request`
    /// will reject the request with a wrapped error explaining the
    /// auto-switch context.
    ///
    /// `ClientBuilder::build()` will return `Err(MotosanError::Config)` if you
    /// set this on a non-`Provider::Ollama` client.
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

    /// Configure the ChatGPT-backend Responses provider
    /// (`Provider::OpenAiChatGpt`). Stores the OAuth `access_token`, the
    /// `chatgpt-account-id` header value, and the model id; the
    /// [`ChatGptCodexProvider`] is constructed from these at dispatch time.
    ///
    /// [`ChatGptCodexProvider`]: crate::providers::chatgpt_codex::ChatGptCodexProvider
    #[cfg(feature = "chatgpt-codex")]
    pub fn chatgpt_codex(
        mut self,
        access_token: impl Into<String>,
        account_id: impl Into<String>,
        model: impl Into<String>,
    ) -> Self {
        self.chatgpt_codex_access_token = Some(access_token.into());
        self.chatgpt_codex_account_id = Some(account_id.into());
        self.chatgpt_codex_model = Some(model.into());
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
                | Provider::OpenAiChatGpt
        );
        let api_key = match self.api_key {
            Some(k) => k,
            None if api_key_required => {
                return Err(MotosanError::Config("api_key is required".to_string()));
            }
            None => String::new(),
        };

        // Guard: ollama_* setters only make sense when the selected
        // provider routes through Ollama (either path — OpenAI-compat or
        // auto-switched native). Catching this at build time prevents the
        // silent no-op that motivated the 0.15.0 fix.
        if !matches!(provider, crate::providers::Provider::Ollama) {
            let mut misused: Vec<&str> = Vec::new();
            if self.ollama_keep_alive.is_some() {
                misused.push("ollama_keep_alive");
            }
            if self.ollama_num_ctx.is_some() {
                misused.push("ollama_num_ctx");
            }
            if self.ollama_think.is_some() {
                misused.push("ollama_think");
            }
            if !misused.is_empty() {
                return Err(MotosanError::Config(format!(
                    "{} can only be used with Provider::Ollama",
                    misused.join(", ")
                )));
            }
        }

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
            #[cfg(feature = "chatgpt-codex")]
            chatgpt_codex_access_token: self.chatgpt_codex_access_token,
            #[cfg(feature = "chatgpt-codex")]
            chatgpt_codex_account_id: self.chatgpt_codex_account_id,
            #[cfg(feature = "chatgpt-codex")]
            chatgpt_codex_model: self.chatgpt_codex_model,
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
    done: bool,
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
            done: false,
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
    type Item = Result<StreamEvent, MotosanError>;

    fn poll_next(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Self::Item>> {
        use std::future::Future;
        use std::task::Poll;

        if self.done {
            return Poll::Ready(None);
        }

        match self.inner.as_mut().poll_next(cx) {
            Poll::Ready(Some(item)) => {
                let timeout = self.timeout;
                self.deadline
                    .as_mut()
                    .reset(tokio::time::Instant::now() + timeout);
                Poll::Ready(Some(item))
            }
            Poll::Ready(None) => Poll::Ready(None),
            Poll::Pending => match self.deadline.as_mut().poll(cx) {
                Poll::Ready(()) => {
                    self.done = true;
                    Poll::Ready(Some(Err(MotosanError::StreamReadTimeout(
                        self.timeout.as_secs(),
                    ))))
                }
                Poll::Pending => Poll::Pending,
            },
        }
    }
}

/// Stream wrapper that filters `<think>...</think>` tags from text events.
struct ThinkStripperStream {
    inner: BoxStream,
    stripper: ThinkStripper,
    // Holds a terminal `done` event when the stripper had buffered text at
    // the moment the inner stream emitted it. The buffered tail is yielded
    // first and the done event is returned on the next poll. Without this,
    // consumers that break on `event.done` (e.g. `collect_stream`) would
    // never observe the flushed tail — losing up to 6 trailing chars on
    // providers like claude-code that emit one Text event per turn.
    pending: Option<Result<StreamEvent, MotosanError>>,
}

impl futures_core::Stream for ThinkStripperStream {
    type Item = Result<StreamEvent, MotosanError>;

    fn poll_next(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Self::Item>> {
        use std::task::Poll;
        loop {
            if let Some(item) = self.pending.take() {
                return Poll::Ready(Some(item));
            }
            match self.inner.as_mut().poll_next(cx) {
                Poll::Ready(Some(Err(e))) => return Poll::Ready(Some(Err(e))),
                Poll::Ready(Some(Ok(event))) => {
                    if event.event_type == StreamEventType::Text && !event.content.is_empty() {
                        let clean = self.stripper.feed(&event.content);
                        if clean.is_empty() {
                            continue; // buffering, skip this event
                        }
                        return Poll::Ready(Some(Ok(StreamEvent::text(clean))));
                    }
                    if event.done {
                        let remaining = self.stripper.flush();
                        if !remaining.is_empty() {
                            self.pending = Some(Ok(event));
                            return Poll::Ready(Some(Ok(StreamEvent::text(remaining))));
                        }
                    }
                    return Poll::Ready(Some(Ok(event)));
                }
                Poll::Ready(None) => {
                    let remaining = self.stripper.flush();
                    if !remaining.is_empty() {
                        return Poll::Ready(Some(Ok(StreamEvent::text(remaining))));
                    }
                    return Poll::Ready(None);
                }
                Poll::Pending => return Poll::Pending,
            }
        }
    }
}

#[cfg(test)]
mod think_stripper_stream_tests {
    use super::*;
    use crate::types::{StreamEvent, Usage};
    use tokio_stream::StreamExt;

    fn mock(events: Vec<StreamEvent>) -> BoxStream {
        Box::pin(tokio_stream::iter(events.into_iter().map(Ok)))
    }

    async fn collect_until_done(mut stream: BoxStream) -> String {
        let mut content = String::new();
        let mut saw_done = false;
        while let Some(item) = stream.next().await {
            let ev = item.expect("mock stream should not fail");
            if ev.done {
                saw_done = true;
                break;
            }
            content.push_str(&ev.content);
        }
        assert!(saw_done, "stream ended without a done event");
        content
    }

    // Reproduces the claude-code regression: provider emits the entire turn
    // as one Text event followed by Done. The 6-char tail buffer must be
    // flushed before the done event reaches the consumer (`collect_stream`
    // breaks on `event.done`), otherwise short replies vanish.
    #[tokio::test]
    async fn short_response_survives_when_followed_by_done() {
        let raw = mock(vec![StreamEvent::text("pong"), StreamEvent::done()]);
        let wrapped = Client::wrap_with_think_stripper(raw);
        assert_eq!(collect_until_done(wrapped).await, "pong");
    }

    #[tokio::test]
    async fn long_response_does_not_lose_six_char_tail() {
        let raw = mock(vec![
            StreamEvent::text("Hello, world!"),
            StreamEvent::done(),
        ]);
        let wrapped = Client::wrap_with_think_stripper(raw);
        assert_eq!(collect_until_done(wrapped).await, "Hello, world!");
    }

    #[tokio::test]
    async fn think_block_still_stripped_with_terminal_done() {
        let raw = mock(vec![
            StreamEvent::text("<think>secret</think>visible"),
            StreamEvent::done(),
        ]);
        let wrapped = Client::wrap_with_think_stripper(raw);
        assert_eq!(collect_until_done(wrapped).await, "visible");
    }

    #[tokio::test]
    async fn flush_does_not_swallow_done_flag() {
        let raw = mock(vec![StreamEvent::text("ok"), StreamEvent::done()]);
        let mut wrapped = Client::wrap_with_think_stripper(raw);
        let mut events = Vec::new();
        while let Some(item) = wrapped.next().await {
            events.push(item.expect("mock stream should not fail"));
        }
        // Must see both the buffered text and the terminal done event.
        assert!(events.iter().any(|e| e.content == "ok" && !e.done));
        assert!(events.iter().any(|e| e.done));
    }

    // Mirrors the actual claude-code wire order (Text → Usage → Done, per
    // providers/claude_code/stream_json.rs). The Usage event arrives while
    // the stripper still holds the 6-char tail; it must pass through without
    // disturbing the buffer, and the tail must still be flushed on the
    // subsequent Done.
    #[tokio::test]
    async fn usage_between_text_and_done_does_not_lose_tail() {
        let raw = mock(vec![
            StreamEvent::text("Hello, world!"),
            StreamEvent::usage(Usage {
                input_tokens: 12,
                output_tokens: 8,
                cache_creation_input_tokens: None,
                cache_read_input_tokens: None,
            }),
            StreamEvent::done(),
        ]);
        let mut wrapped = Client::wrap_with_think_stripper(raw);
        let mut content = String::new();
        let mut saw_usage = false;
        let mut saw_done = false;
        while let Some(item) = wrapped.next().await {
            let ev = item.expect("mock stream should not fail");
            if ev.done {
                saw_done = true;
                break;
            }
            if ev.event_type == StreamEventType::Usage {
                saw_usage = true;
                let u = ev.usage.expect("usage event carries Usage payload");
                assert_eq!(u.input_tokens, 12);
                assert_eq!(u.output_tokens, 8);
                continue;
            }
            content.push_str(&ev.content);
        }
        assert_eq!(content, "Hello, world!");
        assert!(saw_usage, "Usage event must reach the consumer");
        assert!(saw_done, "Done event must reach the consumer");
    }

    // The deferred done event must keep its stop_reason field. Guards against
    // a future refactor that synthesizes a fresh `StreamEvent::done()` instead
    // of queuing the original event into `pending`.
    #[tokio::test]
    async fn deferred_done_preserves_stop_reason() {
        use crate::types::StopReason;
        let raw = mock(vec![
            StreamEvent::text("pong"),
            StreamEvent::done_with_stop_reason(StopReason::EndTurn),
        ]);
        let mut wrapped = Client::wrap_with_think_stripper(raw);
        let mut events = Vec::new();
        while let Some(item) = wrapped.next().await {
            events.push(item.expect("mock stream should not fail"));
        }
        let done = events
            .iter()
            .find(|e| e.done)
            .expect("terminal done event must be observed");
        assert_eq!(done.stop_reason.as_ref(), Some(&StopReason::EndTurn));
    }

    #[tokio::test]
    async fn think_stripper_passes_stream_errors_through() {
        let raw: BoxStream = Box::pin(tokio_stream::iter(vec![Err(MotosanError::Stream(
            "boom".to_string(),
        ))]));
        let mut wrapped = Client::wrap_with_think_stripper(raw);
        let item = wrapped.next().await.expect("error item should be yielded");
        assert!(matches!(item, Err(MotosanError::Stream(msg)) if msg == "boom"));
        assert!(wrapped.next().await.is_none());
    }

    #[cfg(any(
        feature = "anthropic",
        feature = "openai",
        feature = "minimax",
        feature = "ollama_native",
        feature = "gemini",
    ))]
    #[tokio::test]
    async fn read_timeout_yields_error_once_then_ends() {
        let raw: BoxStream = Box::pin(tokio_stream::pending());
        let mut wrapped = ReadTimeoutStream::new(raw, Duration::from_millis(1));
        let item = wrapped.next().await.expect("timeout error item");
        assert!(matches!(item, Err(MotosanError::StreamReadTimeout(0))));
        assert!(wrapped.next().await.is_none());
    }
}

#[cfg(test)]
mod dispatch_validation_tests {
    #[cfg(any(feature = "minimax", feature = "ollama_native"))]
    use super::*;
    #[cfg(any(feature = "minimax", feature = "ollama_native"))]
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
