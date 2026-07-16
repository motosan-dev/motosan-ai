#[cfg(any(feature = "anthropic", feature = "openai", feature = "minimax"))]
use motosan_ai::providers::ProviderImpl;
#[cfg(any(feature = "anthropic", feature = "openai", feature = "minimax"))]
use motosan_ai::{ChatRequest, Message, MotosanError, RetryPolicy};
#[cfg(any(feature = "anthropic", feature = "openai", feature = "minimax"))]
use serde_json::json;

#[cfg(any(feature = "anthropic", feature = "openai", feature = "minimax"))]
fn sample_request() -> ChatRequest {
    ChatRequest::builder()
        .message(Message::user("hello"))
        .build()
}

#[cfg(feature = "anthropic")]
#[tokio::test]
async fn anthropic_maps_401_429_500() {
    let mut server = mockito::Server::new_async().await;
    let provider = motosan_ai::providers::anthropic::AnthropicProvider::new(
        "test-key",
        None,
        Some(server.url()),
    )
    .with_retry_policy(RetryPolicy::new().max_retries(0).jitter(false));

    let unauthorized = server
        .mock("POST", "/v1/messages")
        .with_status(401)
        .with_body(json!({"error": {"message": "bad key"}}).to_string())
        .create_async()
        .await;
    let err = provider
        .chat(sample_request())
        .await
        .expect_err("401 should fail");
    assert!(matches!(err, MotosanError::Auth { .. }));
    unauthorized.assert_async().await;

    server.reset();
    let rate_limited = server
        .mock("POST", "/v1/messages")
        .with_status(429)
        .with_header("retry-after", "7")
        .with_header("request-id", "req_abc")
        .with_body(json!({"error": {"message": "too many"}}).to_string())
        .create_async()
        .await;
    let err = provider
        .chat(sample_request())
        .await
        .expect_err("429 should fail");
    assert!(matches!(err, MotosanError::RateLimit { .. }));
    assert_eq!(err.status_code(), Some(429));
    assert_eq!(err.retry_after(), Some(std::time::Duration::from_secs(7)));
    assert_eq!(err.request_id(), Some("req_abc"));
    assert_eq!(err.to_string(), "rate limit error: too many");
    rate_limited.assert_async().await;

    server.reset();
    let provider_error = server
        .mock("POST", "/v1/messages")
        .with_status(500)
        .with_body(json!({"error": {"message": "boom"}}).to_string())
        .create_async()
        .await;
    let err = provider
        .chat(sample_request())
        .await
        .expect_err("500 should fail");
    assert!(matches!(err, MotosanError::ProviderError { .. }));
    provider_error.assert_async().await;
}

#[cfg(feature = "openai")]
#[tokio::test]
async fn openai_maps_401_429_500() {
    let mut server = mockito::Server::new_async().await;
    let provider = motosan_ai::providers::openai::OpenAIProvider::new("test-key", None)
        .with_chat_url(format!("{}/v1/chat/completions", server.url()))
        .with_retry_policy(RetryPolicy::new().max_retries(0).jitter(false));

    let unauthorized = server
        .mock("POST", "/v1/chat/completions")
        .with_status(401)
        .with_body(json!({"error": {"message": "bad key"}}).to_string())
        .create_async()
        .await;
    let err = provider
        .chat(sample_request())
        .await
        .expect_err("401 should fail");
    assert!(matches!(err, MotosanError::Auth { .. }));
    unauthorized.assert_async().await;

    server.reset();
    let rate_limited = server
        .mock("POST", "/v1/chat/completions")
        .with_status(429)
        .with_body(json!({"error": {"message": "too many"}}).to_string())
        .create_async()
        .await;
    let err = provider
        .chat(sample_request())
        .await
        .expect_err("429 should fail");
    assert!(matches!(err, MotosanError::RateLimit { .. }));
    rate_limited.assert_async().await;

    server.reset();
    let provider_error = server
        .mock("POST", "/v1/chat/completions")
        .with_status(500)
        .with_body(json!({"error": {"message": "boom"}}).to_string())
        .create_async()
        .await;
    let err = provider
        .chat(sample_request())
        .await
        .expect_err("500 should fail");
    assert!(matches!(err, MotosanError::ProviderError { .. }));
    provider_error.assert_async().await;
}

#[cfg(feature = "minimax")]
#[tokio::test]
async fn minimax_maps_401_429_500() {
    let mut server = mockito::Server::new_async().await;
    let provider = motosan_ai::providers::anthropic::AnthropicProvider::new(
        "test-key",
        Some("MiniMax-M2.7".to_string()),
        Some(format!("{}/anthropic", server.url())),
    )
    .with_capabilities(motosan_ai::ProviderCapabilities::text_only())
    .with_retry_policy(RetryPolicy::new().max_retries(0).jitter(false));

    let unauthorized = server
        .mock("POST", "/anthropic/v1/messages")
        .with_status(401)
        .with_body(json!({"error": {"message": "bad key"}}).to_string())
        .create_async()
        .await;
    let err = provider
        .chat(sample_request())
        .await
        .expect_err("401 should fail");
    assert!(matches!(err, MotosanError::Auth { .. }));
    unauthorized.assert_async().await;

    server.reset();
    let rate_limited = server
        .mock("POST", "/anthropic/v1/messages")
        .with_status(429)
        .with_body(json!({"error": {"message": "too many"}}).to_string())
        .create_async()
        .await;
    let err = provider
        .chat(sample_request())
        .await
        .expect_err("429 should fail");
    assert!(matches!(err, MotosanError::RateLimit { .. }));
    rate_limited.assert_async().await;

    server.reset();
    let provider_error = server
        .mock("POST", "/anthropic/v1/messages")
        .with_status(500)
        .with_body(json!({"error": {"message": "boom"}}).to_string())
        .create_async()
        .await;
    let err = provider
        .chat(sample_request())
        .await
        .expect_err("500 should fail");
    assert!(matches!(err, MotosanError::ProviderError { .. }));
    provider_error.assert_async().await;
}
