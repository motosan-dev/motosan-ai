use motosan_ai::{Client, Message, MotosanError, Provider, RetryPolicy};

#[test]
fn builder_requires_provider_and_api_key() {
    let missing_provider = Client::builder().api_key("k").build();
    assert!(matches!(missing_provider, Err(MotosanError::Config(_))));

    let missing_api_key = Client::builder().provider(Provider::OpenAI).build();
    assert!(matches!(missing_api_key, Err(MotosanError::Config(_))));
}

#[test]
fn builder_uses_default_retry_policy_and_allows_override() {
    let client = Client::builder()
        .provider(Provider::OpenAI)
        .api_key("k")
        .build()
        .expect("build client");
    assert_eq!(
        client.retry_policy().max_retries,
        RetryPolicy::default().max_retries
    );

    let custom_policy = RetryPolicy::new()
        .max_retries(5)
        .base_delay_ms(10)
        .max_delay_ms(100)
        .jitter(false)
        .respect_retry_after(false);

    let client = Client::builder()
        .provider(Provider::OpenAI)
        .api_key("k")
        .retry_policy(custom_policy.clone())
        .build()
        .expect("build client");

    assert_eq!(client.retry_policy().max_retries, custom_policy.max_retries);
    assert_eq!(
        client.retry_policy().base_delay_ms,
        custom_policy.base_delay_ms
    );
    assert_eq!(
        client.retry_policy().max_delay_ms,
        custom_policy.max_delay_ms
    );
    assert_eq!(client.retry_policy().jitter, custom_policy.jitter);
    assert_eq!(
        client.retry_policy().respect_retry_after,
        custom_policy.respect_retry_after
    );
}

#[test]
fn builder_defaults_minimax_expose_reasoning_to_false_and_allows_override() {
    let default_client = Client::builder()
        .provider(Provider::Minimax)
        .api_key("k")
        .build()
        .expect("build client");
    assert!(!default_client.minimax_expose_reasoning());

    let custom_client = Client::builder()
        .provider(Provider::Minimax)
        .api_key("k")
        .minimax_expose_reasoning(true)
        .build()
        .expect("build client");
    assert!(custom_client.minimax_expose_reasoning());
}

#[test]
fn builder_defaults_openai_options_and_allows_override() {
    let default_client = Client::builder()
        .provider(Provider::OpenAI)
        .api_key("k")
        .build()
        .expect("build client");
    assert_eq!(default_client.openai_auth_header(), None);
    assert!(!default_client.openai_responses_fallback());

    let custom_client = Client::builder()
        .provider(Provider::OpenAI)
        .api_key("k")
        .openai_auth_x_api_key()
        .openai_responses_fallback(true)
        .build()
        .expect("build client");
    assert_eq!(custom_client.openai_auth_header(), Some("x-api-key"));
    assert!(custom_client.openai_responses_fallback());

    let custom_header_client = Client::builder()
        .provider(Provider::OpenAI)
        .api_key("k")
        .openai_auth_custom_header("X-Auth-Token")
        .build()
        .expect("build client");
    assert_eq!(
        custom_header_client.openai_auth_header(),
        Some("X-Auth-Token")
    );

    let reset_to_bearer_client = Client::builder()
        .provider(Provider::OpenAI)
        .api_key("k")
        .openai_auth_custom_header("X-Auth-Token")
        .openai_auth_bearer()
        .build()
        .expect("build client");
    assert_eq!(reset_to_bearer_client.openai_auth_header(), None);
}

#[tokio::test]
async fn chat_and_stream_exist_and_dispatch() {
    let client = Client::builder()
        .provider(Provider::OpenAI)
        .api_key("k")
        .build()
        .expect("build client");

    let messages = vec![Message::user("hello")];
    let chat_result = client.chat(messages.clone()).await;
    let stream_result = client.stream(messages).await;

    #[cfg(not(feature = "openai"))]
    assert!(matches!(chat_result, Err(MotosanError::Config(_))));
    #[cfg(not(feature = "openai"))]
    assert!(matches!(stream_result, Err(MotosanError::Config(_))));

    #[cfg(feature = "openai")]
    assert!(chat_result.is_err());
    #[cfg(feature = "openai")]
    assert!(stream_result.is_err());
}

#[cfg(feature = "codex-cli")]
#[tokio::test]
async fn client_builder_allows_codex_cli_without_api_key() {
    use motosan_ai::codex_cli::SandboxMode;
    use motosan_ai::CodexCliProvider;

    // No `.api_key()` call. CLI backends authenticate via their own
    // channels (local login state / CODEX_API_KEY env), so build() must
    // succeed without one.
    let client = Client::builder()
        .provider(Provider::CodexCli)
        .codex_cli(
            CodexCliProvider::new()
                .sandbox(SandboxMode::ReadOnly)
                .ephemeral(true),
        )
        .build()
        .expect("build client without api_key");

    // Sanity: the provider reflects what we asked for.
    assert!(matches!(client.provider(), Provider::CodexCli));
}

#[cfg(feature = "claude-code")]
#[tokio::test]
async fn client_builder_allows_claude_code_without_api_key() {
    use motosan_ai::ClaudeCodeProvider;

    let client = Client::builder()
        .provider(Provider::ClaudeCode)
        .claude_code(ClaudeCodeProvider::new().model("sonnet"))
        .build()
        .expect("build client without api_key");

    assert!(matches!(client.provider(), Provider::ClaudeCode));
}

#[test]
fn client_builder_still_requires_api_key_for_http_providers() {
    // Regression guard: relaxing api_key for CLI backends must not also
    // relax it for HTTP providers.
    let result = Client::builder().provider(Provider::Anthropic).build();
    assert!(matches!(result, Err(MotosanError::Config(_))));
}

#[cfg(feature = "codex-cli")]
#[tokio::test]
#[ignore] // Requires `codex` CLI installed + auth.
async fn integration_client_dispatches_to_codex_cli() {
    // End-to-end: Client::builder().provider(Provider::CodexCli).build()
    // should actually spawn `codex exec` when .chat() is called. This is
    // the payoff for v0.11 — downstream consumers can hold a single
    // `Client` and dispatch to any backend by name.
    use motosan_ai::codex_cli::SandboxMode;
    use motosan_ai::{ChatRequest, CodexCliProvider};

    let client = Client::builder()
        .provider(Provider::CodexCli)
        .codex_cli(
            CodexCliProvider::new()
                .sandbox(SandboxMode::ReadOnly)
                .ephemeral(true),
        )
        .build()
        .expect("build client");

    let request = ChatRequest::builder()
        .message(Message::user("Reply with only the word 'pong'."))
        .build();

    let response = client
        .chat_with(request)
        .await
        .expect("codex chat should succeed via Client dispatch");
    assert!(
        response.content.to_lowercase().contains("pong"),
        "expected 'pong', got: {}",
        response.content
    );
}
