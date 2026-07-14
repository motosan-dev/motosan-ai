#![cfg(feature = "anthropic")]

use mockito::Matcher;
use motosan_ai::providers::anthropic::AnthropicProvider;
use motosan_ai::providers::ProviderImpl;
use motosan_ai::{
    ChatRequest, Message, MotosanError, RetryPolicy, StopReason, DEFAULT_ANTHROPIC_MODEL,
};
use serde_json::json;

#[tokio::test]
async fn anthropic_chat_maps_response() {
    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("POST", "/v1/messages")
        .match_header("x-api-key", "test-key")
        .match_header("anthropic-version", "2023-06-01")
        .with_status(200)
        .with_body(
            json!({
                "id": "msg_1",
                "model": "claude-sonnet-4-5",
                "stop_reason": "end_turn",
                "usage": {"input_tokens": 10, "output_tokens": 20},
                "content": [{"type": "text", "text": "hello from anthropic"}]
            })
            .to_string(),
        )
        .create_async()
        .await;

    let provider = AnthropicProvider::new("test-key", None, Some(server.url()));
    let request = ChatRequest::builder()
        .message(Message::system("rules"))
        .message(Message::user("hello"))
        .temperature(0.2)
        .max_tokens(100)
        .build();

    let response = provider.chat(request).await.expect("chat response");
    assert_eq!(response.content, "hello from anthropic");
    assert_eq!(response.model, "claude-sonnet-4-5");
    assert_eq!(response.usage.input_tokens, 10);
    assert!(matches!(response.stop_reason, StopReason::EndTurn));

    mock.assert_async().await;
}

#[tokio::test]
async fn anthropic_request_uses_default_model_and_allows_override() {
    let mut server = mockito::Server::new_async().await;

    let default_mock = server
        .mock("POST", "/v1/messages")
        .match_header("x-api-key", "test-key")
        .match_body(Matcher::Regex(format!(
            r#"\"model\"\s*:\s*\"{}\""#,
            DEFAULT_ANTHROPIC_MODEL
        )))
        .with_status(200)
        .with_body(
            json!({
                "model": DEFAULT_ANTHROPIC_MODEL,
                "stop_reason": "end_turn",
                "usage": {"input_tokens": 1, "output_tokens": 1},
                "content": [{"type": "text", "text": "ok"}]
            })
            .to_string(),
        )
        .create_async()
        .await;

    let provider = AnthropicProvider::new("test-key", None, Some(server.url()));
    let request = ChatRequest::builder()
        .message(Message::user("hello"))
        .build();
    let _ = provider.chat(request).await.expect("chat response");
    default_mock.assert_async().await;

    server.reset();
    let override_model = "claude-opus-4-6";
    let override_mock = server
        .mock("POST", "/v1/messages")
        .match_header("x-api-key", "test-key")
        .match_body(Matcher::Regex(format!(
            r#"\"model\"\s*:\s*\"{}\""#,
            override_model
        )))
        .with_status(200)
        .with_body(
            json!({
                "model": override_model,
                "stop_reason": "end_turn",
                "usage": {"input_tokens": 1, "output_tokens": 1},
                "content": [{"type": "text", "text": "ok"}]
            })
            .to_string(),
        )
        .create_async()
        .await;

    let request = ChatRequest::builder()
        .message(Message::user("hello"))
        .model(override_model)
        .build();
    let _ = provider.chat(request).await.expect("chat response");
    override_mock.assert_async().await;
}

#[tokio::test]
async fn anthropic_setup_token_uses_bearer_and_oauth_beta_header() {
    let mut server = mockito::Server::new_async().await;
    // OAuth chat() delegates to stream(), so mock must return SSE format
    let sse_body = [
        "event: content_block_delta",
        &format!("data: {}", json!({"type": "content_block_delta", "index": 0, "delta": {"type": "text_delta", "text": "ok"}})),
        "",
        "event: message_stop",
        &format!("data: {}", json!({"type": "message_stop"})),
        "",
    ].join("\n");
    let mock = server
        .mock("POST", "/v1/messages")
        .match_header("authorization", "Bearer sk-ant-oat01-test-token")
        .match_header(
            "anthropic-beta",
            Matcher::Regex("oauth-2025-04-20".to_string()),
        )
        .match_header("anthropic-version", "2023-06-01")
        .with_status(200)
        .with_header("content-type", "text/event-stream")
        .with_body(sse_body)
        .create_async()
        .await;

    let provider = AnthropicProvider::new("sk-ant-oat01-test-token", None, Some(server.url()));
    let request = ChatRequest::builder()
        .message(Message::user("hello"))
        .build();
    let resp = provider.chat(request).await.expect("chat response");
    assert_eq!(resp.content, "ok");
    mock.assert_async().await;
}

#[tokio::test]
async fn anthropic_setup_token_401_includes_actionable_hint() {
    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("POST", "/v1/messages")
        .with_status(401)
        .with_body(json!({"error": {"message": "invalid token"}}).to_string())
        .create_async()
        .await;

    let provider = AnthropicProvider::new("sk-ant-oat01-test-token", None, Some(server.url()));
    let request = ChatRequest::builder()
        .message(Message::user("hello"))
        .build();
    let err = provider.chat(request).await.expect_err("should fail");

    assert!(matches!(err, MotosanError::Auth(_)));
    assert!(format!("{err}").contains("oauth-2025-04-20"));
    mock.assert_async().await;
}

#[tokio::test]
async fn chat_without_max_tokens_uses_default() {
    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("POST", "/v1/messages")
        .match_header("x-api-key", "test-key")
        .match_body(Matcher::Regex(r#""max_tokens"\s*:\s*8192"#.to_string()))
        .with_status(200)
        .with_body(
            json!({
                "model": DEFAULT_ANTHROPIC_MODEL,
                "stop_reason": "end_turn",
                "usage": {"input_tokens": 1, "output_tokens": 1},
                "content": [{"type": "text", "text": "ok"}]
            })
            .to_string(),
        )
        .create_async()
        .await;

    let provider = AnthropicProvider::new("test-key", None, Some(server.url()));
    let request = ChatRequest::builder()
        .message(Message::user("hello"))
        .build();
    let response = provider.chat(request).await.expect("chat response");
    assert_eq!(response.content, "ok");
    mock.assert_async().await;
}

#[tokio::test]
async fn chat_with_explicit_max_tokens_overrides_default() {
    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("POST", "/v1/messages")
        .match_header("x-api-key", "test-key")
        .match_body(Matcher::Regex(r#""max_tokens"\s*:\s*2000"#.to_string()))
        .with_status(200)
        .with_body(
            json!({
                "model": DEFAULT_ANTHROPIC_MODEL,
                "stop_reason": "end_turn",
                "usage": {"input_tokens": 1, "output_tokens": 1},
                "content": [{"type": "text", "text": "ok"}]
            })
            .to_string(),
        )
        .create_async()
        .await;

    let provider = AnthropicProvider::new("test-key", None, Some(server.url()));
    let request = ChatRequest::builder()
        .message(Message::user("hello"))
        .max_tokens(2000)
        .build();
    let response = provider.chat(request).await.expect("chat response");
    assert_eq!(response.content, "ok");
    mock.assert_async().await;
}

#[tokio::test]
async fn anthropic_chat_retries_503_with_non_json_body() {
    let mut server = mockito::Server::new_async().await;
    // Canonical proxy/LB failure: retryable status with an HTML (non-JSON) body.
    let unavailable = server
        .mock("POST", "/v1/messages")
        .with_status(503)
        .with_body("<html>Service Unavailable</html>")
        .expect(1)
        .create_async()
        .await;
    let success = server
        .mock("POST", "/v1/messages")
        .with_status(200)
        .with_body(
            json!({
                "model": DEFAULT_ANTHROPIC_MODEL,
                "stop_reason": "end_turn",
                "usage": {"input_tokens": 1, "output_tokens": 1},
                "content": [{"type": "text", "text": "recovered"}]
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
    let provider =
        AnthropicProvider::new("test-key", None, Some(server.url())).with_retry_policy(policy);
    let request = ChatRequest::builder()
        .message(Message::user("hello"))
        .build();

    let response = provider
        .chat(request)
        .await
        .expect("503 with non-JSON body should be retried, not aborted with a parse error");

    assert_eq!(response.content, "recovered");
    unavailable.assert_async().await;
    success.assert_async().await;
}
