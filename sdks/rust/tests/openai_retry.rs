#![cfg(feature = "openai")]

use motosan_ai::providers::openai::OpenAIProvider;
use motosan_ai::providers::ProviderImpl;
use motosan_ai::{ChatRequest, Message, MotosanError, RetryPolicy};
use serde_json::json;
use std::time::{Duration, Instant};
use tokio_stream::StreamExt;

#[tokio::test]
async fn openai_chat_retries_429_then_succeeds() {
    let mut server = mockito::Server::new_async().await;
    server
        .mock("POST", "/v1/chat/completions")
        .with_status(429)
        .with_body(json!({"error": {"message": "rate limited"}}).to_string())
        .expect(1)
        .create_async()
        .await;
    server
        .mock("POST", "/v1/chat/completions")
        .with_status(200)
        .with_body(
            json!({
                "model": "gpt-5.3-codex",
                "choices": [{"message": {"content": "ok"}, "finish_reason": "stop"}],
                "usage": {"prompt_tokens": 1, "completion_tokens": 1}
            })
            .to_string(),
        )
        .expect(1)
        .create_async()
        .await;

    let policy = RetryPolicy::new()
        .max_retries(1)
        .base_delay_ms(0)
        .max_delay_ms(0)
        .jitter(false);
    let provider = OpenAIProvider::new("test-key", None)
        .with_chat_url(format!("{}/v1/chat/completions", server.url()))
        .with_retry_policy(policy);

    let response = provider
        .chat(
            ChatRequest::builder()
                .message(Message::user("hello"))
                .build(),
        )
        .await
        .expect("should retry and succeed");

    assert_eq!(response.content, "ok");
}

#[tokio::test]
async fn openai_chat_retries_500_twice_then_succeeds() {
    let mut server = mockito::Server::new_async().await;
    server
        .mock("POST", "/v1/chat/completions")
        .with_status(500)
        .with_body(json!({"error": {"message": "server error"}}).to_string())
        .expect(2)
        .create_async()
        .await;
    server
        .mock("POST", "/v1/chat/completions")
        .with_status(200)
        .with_body(
            json!({
                "model": "gpt-5.3-codex",
                "choices": [{"message": {"content": "recovered"}, "finish_reason": "stop"}],
                "usage": {"prompt_tokens": 1, "completion_tokens": 1}
            })
            .to_string(),
        )
        .expect(1)
        .create_async()
        .await;

    let policy = RetryPolicy::new()
        .max_retries(2)
        .base_delay_ms(0)
        .max_delay_ms(0)
        .jitter(false);
    let provider = OpenAIProvider::new("test-key", None)
        .with_chat_url(format!("{}/v1/chat/completions", server.url()))
        .with_retry_policy(policy);

    let response = provider
        .chat(
            ChatRequest::builder()
                .message(Message::user("hello"))
                .build(),
        )
        .await
        .expect("should retry and succeed");

    assert_eq!(response.content, "recovered");
}

#[tokio::test]
async fn openai_chat_does_not_retry_on_400() {
    let mut server = mockito::Server::new_async().await;
    let bad_request = server
        .mock("POST", "/v1/chat/completions")
        .with_status(400)
        .with_body(json!({"error": {"message": "bad request"}}).to_string())
        .expect(1)
        .create_async()
        .await;

    let policy = RetryPolicy::new()
        .max_retries(3)
        .base_delay_ms(0)
        .max_delay_ms(0)
        .jitter(false);
    let provider = OpenAIProvider::new("test-key", None)
        .with_chat_url(format!("{}/v1/chat/completions", server.url()))
        .with_retry_policy(policy);

    let result = provider
        .chat(
            ChatRequest::builder()
                .message(Message::user("hello"))
                .build(),
        )
        .await;

    assert!(matches!(result, Err(MotosanError::InvalidRequest(_))));
    bad_request.assert_async().await;
}

#[tokio::test]
async fn openai_chat_honors_retry_after_header() {
    let mut server = mockito::Server::new_async().await;
    server
        .mock("POST", "/v1/chat/completions")
        .with_status(429)
        .with_header("retry-after", "1")
        .with_body(json!({"error": {"message": "rate limited"}}).to_string())
        .expect(1)
        .create_async()
        .await;
    server
        .mock("POST", "/v1/chat/completions")
        .with_status(200)
        .with_body(
            json!({
                "model": "gpt-5.3-codex",
                "choices": [{"message": {"content": "ok"}, "finish_reason": "stop"}],
                "usage": {"prompt_tokens": 1, "completion_tokens": 1}
            })
            .to_string(),
        )
        .expect(1)
        .create_async()
        .await;

    let policy = RetryPolicy::new()
        .max_retries(1)
        .base_delay_ms(0)
        .max_delay_ms(0)
        .jitter(false)
        .respect_retry_after(true);
    let provider = OpenAIProvider::new("test-key", None)
        .with_chat_url(format!("{}/v1/chat/completions", server.url()))
        .with_retry_policy(policy);

    let started = Instant::now();
    let _ = provider
        .chat(
            ChatRequest::builder()
                .message(Message::user("hello"))
                .build(),
        )
        .await
        .expect("should retry and succeed");

    assert!(started.elapsed() >= Duration::from_millis(900));
}

#[tokio::test]
async fn openai_stream_retries_initial_call() {
    let mut server = mockito::Server::new_async().await;
    server
        .mock("POST", "/v1/chat/completions")
        .with_status(500)
        .with_body(json!({"error": {"message": "server error"}}).to_string())
        .expect(1)
        .create_async()
        .await;
    server
        .mock("POST", "/v1/chat/completions")
        .with_status(200)
        .with_header("content-type", "text/event-stream")
        .with_body("data: {\"choices\":[{\"delta\":{\"content\":\"ok\"}}]}\n\ndata: [DONE]\n\n")
        .expect(1)
        .create_async()
        .await;

    let policy = RetryPolicy::new()
        .max_retries(1)
        .base_delay_ms(0)
        .max_delay_ms(0)
        .jitter(false);
    let provider = OpenAIProvider::new("test-key", None)
        .with_chat_url(format!("{}/v1/chat/completions", server.url()))
        .with_retry_policy(policy);
    let request = ChatRequest::builder()
        .message(Message::user("hello"))
        .build();

    let mut stream = provider.stream(request).await.expect("stream response");
    let first = stream
        .next()
        .await
        .expect("first event")
        .expect("stream item should not fail");
    let second = stream
        .next()
        .await
        .expect("done event")
        .expect("stream item should not fail");

    assert_eq!(first.content, "ok");
    assert!(second.done);
}

#[tokio::test]
async fn openai_chat_retries_502_with_non_json_body() {
    let mut server = mockito::Server::new_async().await;
    let bad_gateway = server
        .mock("POST", "/v1/chat/completions")
        .with_status(502)
        .with_body("bad gateway")
        .expect(1)
        .create_async()
        .await;
    let success = server
        .mock("POST", "/v1/chat/completions")
        .with_status(200)
        .with_body(
            json!({
                "model": "gpt-5.3-codex",
                "choices": [{"message": {"content": "recovered"}, "finish_reason": "stop"}],
                "usage": {"prompt_tokens": 1, "completion_tokens": 1}
            })
            .to_string(),
        )
        .expect(1)
        .create_async()
        .await;

    let policy = RetryPolicy::new()
        .max_retries(1)
        .base_delay_ms(0)
        .max_delay_ms(0)
        .jitter(false);
    let provider = OpenAIProvider::new("test-key", None)
        .with_chat_url(format!("{}/v1/chat/completions", server.url()))
        .with_retry_policy(policy);

    let response = provider
        .chat(
            ChatRequest::builder()
                .message(Message::user("hello"))
                .build(),
        )
        .await
        .expect("should retry past non-JSON 502 body and succeed");

    assert_eq!(response.content, "recovered");
    bad_gateway.assert_async().await;
    success.assert_async().await;
}
