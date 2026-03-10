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
    assert!(matches!(err, MotosanError::Auth(_)));
    unauthorized.assert_async().await;

    server.reset();
    let rate_limited = server
        .mock("POST", "/v1/messages")
        .with_status(429)
        .with_body(json!({"error": {"message": "too many"}}).to_string())
        .create_async()
        .await;
    let err = provider
        .chat(sample_request())
        .await
        .expect_err("429 should fail");
    assert!(matches!(err, MotosanError::RateLimit(_)));
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
    assert!(matches!(err, MotosanError::ProviderError(_)));
    provider_error.assert_async().await;
}

#[cfg(feature = "openai")]
#[tokio::test]
async fn openai_maps_401_429_500() {
    let mut server = mockito::Server::new_async().await;
    let provider =
        motosan_ai::providers::openai::OpenAIProvider::new("test-key", None, Some(server.url()))
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
    assert!(matches!(err, MotosanError::Auth(_)));
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
    assert!(matches!(err, MotosanError::RateLimit(_)));
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
    assert!(matches!(err, MotosanError::ProviderError(_)));
    provider_error.assert_async().await;
}

#[cfg(feature = "minimax")]
#[tokio::test]
async fn minimax_maps_401_429_500() {
    let mut server = mockito::Server::new_async().await;
    let provider =
        motosan_ai::providers::minimax::MinimaxProvider::new("test-key", None, Some(server.url()))
            .with_retry_policy(RetryPolicy::new().max_retries(0).jitter(false));

    let unauthorized = server
        .mock("POST", "/v1/text/chatcompletion_v2")
        .with_status(401)
        .with_body(json!({"error": {"message": "bad key"}}).to_string())
        .create_async()
        .await;
    let err = provider
        .chat(sample_request())
        .await
        .expect_err("401 should fail");
    assert!(matches!(err, MotosanError::Auth(_)));
    unauthorized.assert_async().await;

    server.reset();
    let rate_limited = server
        .mock("POST", "/v1/text/chatcompletion_v2")
        .with_status(429)
        .with_body(json!({"error": {"message": "too many"}}).to_string())
        .create_async()
        .await;
    let err = provider
        .chat(sample_request())
        .await
        .expect_err("429 should fail");
    assert!(matches!(err, MotosanError::RateLimit(_)));
    rate_limited.assert_async().await;

    server.reset();
    let provider_error = server
        .mock("POST", "/v1/text/chatcompletion_v2")
        .with_status(500)
        .with_body(json!({"error": {"message": "boom"}}).to_string())
        .create_async()
        .await;
    let err = provider
        .chat(sample_request())
        .await
        .expect_err("500 should fail");
    assert!(matches!(err, MotosanError::ProviderError(_)));
    provider_error.assert_async().await;
}
