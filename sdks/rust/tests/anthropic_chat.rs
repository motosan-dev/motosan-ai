#![cfg(feature = "anthropic")]

use mockito::Matcher;
use motosan_ai::providers::anthropic::AnthropicProvider;
use motosan_ai::providers::ProviderImpl;
use motosan_ai::{ChatRequest, Message, MotosanError, StopReason, DEFAULT_ANTHROPIC_MODEL};
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
    let mock = server
        .mock("POST", "/v1/messages")
        .match_header("authorization", "Bearer sk-ant-oat01-test-token")
        .match_header("anthropic-beta", "oauth-2025-04-20")
        .match_header("anthropic-version", "2023-06-01")
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

    let provider = AnthropicProvider::new("sk-ant-oat01-test-token", None, Some(server.url()));
    let request = ChatRequest::builder()
        .message(Message::user("hello"))
        .build();
    let _ = provider.chat(request).await.expect("chat response");
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
