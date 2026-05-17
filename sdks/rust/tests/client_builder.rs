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

#[cfg(feature = "anthropic")]
#[test]
fn builder_anthropic_base_url_defaults_to_none_and_round_trips_override() {
    // Default: no override set → getter returns None (provider falls back to
    // https://api.anthropic.com).
    let default_client = Client::builder()
        .provider(Provider::Anthropic)
        .api_key("k")
        .build()
        .expect("build client");
    assert_eq!(default_client.anthropic_base_url(), None);

    // Override: set a custom URL → getter returns it verbatim.
    let custom_client = Client::builder()
        .provider(Provider::Anthropic)
        .api_key("k")
        .anthropic_base_url("https://proxy.example.com/anthropic")
        .build()
        .expect("build client");
    assert_eq!(
        custom_client.anthropic_base_url(),
        Some("https://proxy.example.com/anthropic")
    );
}

#[cfg(feature = "anthropic")]
#[tokio::test]
async fn builder_anthropic_base_url_is_forwarded_to_http_request() {
    // Regression guard: the getter test above can pass even if
    // build_anthropic_provider stops threading self.anthropic_base_url into
    // AnthropicProvider::new. This test asserts the override actually reaches
    // the wire by pointing the client at a mockito server and verifying the
    // POST lands on that host.
    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("POST", "/v1/messages")
        .match_header("x-api-key", "test-key")
        .with_status(200)
        .with_body(
            serde_json::json!({
                "model": "claude-sonnet-4-6",
                "stop_reason": "end_turn",
                "usage": {"input_tokens": 1, "output_tokens": 1},
                "content": [{"type": "text", "text": "ok"}]
            })
            .to_string(),
        )
        .create_async()
        .await;

    let client = Client::builder()
        .provider(Provider::Anthropic)
        .api_key("test-key")
        .anthropic_base_url(server.url())
        .build()
        .expect("build client");

    let _ = client
        .chat(vec![Message::user("hi")])
        .await
        .expect("chat against mock server");
    mock.assert_async().await;
}

#[cfg(feature = "minimax")]
#[tokio::test]
async fn builder_accepts_minimax_base_url_override() {
    let client = Client::builder()
        .provider(Provider::Minimax)
        .api_key("k")
        .minimax_base_url("https://api.minimaxi.com/anthropic")
        .build()
        .expect("build client");

    // Validation should fire before any network call for text-only minimax.
    let req = motosan_ai::ChatRequest::builder()
        .message(Message::user_with_image("look", "abc123", "image/png"))
        .build();
    let result = client.chat_with(req).await;
    assert!(matches!(result, Err(MotosanError::UnsupportedFeature(_))));
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

#[test]
fn build_rejects_ollama_fields_with_non_ollama_provider() {
    // ollama_keep_alive set but provider is OpenAI — should error at build()
    // rather than silently dropping the value at runtime.
    let result = Client::builder()
        .provider(Provider::OpenAI)
        .api_key("k")
        .ollama_keep_alive("10m")
        .build();
    let err = result.expect_err("should reject ollama_* on non-Ollama provider");
    let msg = format!("{err}");
    assert!(
        msg.contains("ollama_keep_alive") && msg.contains("Provider::Ollama"),
        "error message should name the field + correct provider, got: {msg}"
    );
}

#[test]
fn build_accepts_ollama_fields_with_ollama_provider() {
    // Sanity guard: the validation should NOT fire for the correct provider.
    let result = Client::builder()
        .provider(Provider::Ollama)
        .api_key("ollama")
        .ollama_keep_alive("10m")
        .ollama_num_ctx(8192)
        .ollama_think("yes")
        .build();
    assert!(result.is_ok(), "valid combo should build");
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

#[cfg(feature = "claude-code")]
#[tokio::test]
#[ignore] // Requires `claude` CLI installed + auth.
async fn integration_client_dispatches_to_claude_code_stream() {
    // Regression guard for the 0.14.3 fix: claude --print --output-format
    // stream-json now requires --verbose (claude ≥ 2.1.x). Without the
    // --verbose arg in claude_code/mod.rs the CLI errors at startup, stdout
    // is empty, and this stream yields ZERO events — which is exactly what
    // capo's manual smoke surfaced. This test fails fast if the regression
    // ever returns: the stream must produce at least one non-empty Text
    // event for a trivial "pong" prompt.
    use motosan_ai::{ChatRequest, ClaudeCodeProvider, StreamEventType};
    use tokio_stream::StreamExt;

    let client = Client::builder()
        .provider(Provider::ClaudeCode)
        .claude_code(ClaudeCodeProvider::new().model("sonnet"))
        .build()
        .expect("build client");

    let request = ChatRequest::builder()
        .message(Message::user("Reply with only the word 'pong'."))
        .build();

    let mut stream = client
        .stream_with(request)
        .await
        .expect("claude stream should succeed via Client dispatch");

    let mut saw_non_empty_text = false;
    while let Some(event) = stream.next().await {
        if matches!(event.event_type, StreamEventType::Text) && !event.content.trim().is_empty() {
            saw_non_empty_text = true;
        }
    }

    assert!(
        saw_non_empty_text,
        "claude_code .stream() emitted no non-empty Text events — \
         did `--verbose` get dropped from claude_code/mod.rs?"
    );
}

#[cfg(feature = "claude-code")]
#[tokio::test]
#[ignore] // Requires `claude` CLI installed + auth.
async fn integration_claude_code_short_reply_not_truncated() {
    // Regression guard for the 0.15.3 ThinkStripper flush-before-done fix.
    // Pre-fix: claude-code emits the entire assistant turn as one Text
    // event followed by Done; `ThinkStripperStream` only flushed on
    // `Poll::Ready(None)`, but `collect_stream` (used by `chat_with`)
    // breaks on `event.done`, so the 6-char tail buffer was never
    // observed. A 4-char reply like "pong" disappeared entirely. The
    // sibling `integration_client_dispatches_to_claude_code_stream` test
    // would still have passed under the bug (a tail-truncated Text event
    // is still non-empty) — this test pins the round-trip content.
    use motosan_ai::{ChatRequest, ClaudeCodeProvider};

    let client = Client::builder()
        .provider(Provider::ClaudeCode)
        .claude_code(ClaudeCodeProvider::new().model("sonnet"))
        .build()
        .expect("build client");

    let request = ChatRequest::builder()
        .message(Message::user(
            "Reply with only the four letters 'pong' (no punctuation, no whitespace, nothing else).",
        ))
        .build();

    let response = client
        .chat_with(request)
        .await
        .expect("claude chat should succeed via Client dispatch");

    let trimmed = response.content.trim();
    assert!(
        trimmed.contains("pong"),
        "claude reply should contain 'pong' end-to-end via collect_stream — got: {trimmed:?}. \
         If this fails with content \"\" or a tail-stripped string, the \
         ThinkStripperStream flush-before-done fix in src/client.rs has regressed."
    );
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
