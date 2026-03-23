#![cfg(feature = "anthropic")]

use mockito::Matcher;
use motosan_ai::providers::anthropic::AnthropicProvider;
use motosan_ai::providers::ProviderImpl;
use motosan_ai::{ChatRequest, Message, ThinkingConfig};
use serde_json::json;

#[tokio::test]
async fn thinking_config_serializes_as_enabled() {
    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("POST", "/v1/messages")
        .match_header("x-api-key", "test-key")
        .match_body(Matcher::Regex(
            r#""thinking"\s*:\s*\{\s*"budget_tokens"\s*:\s*8000\s*,\s*"type"\s*:\s*"enabled"\s*\}"#
                .to_string(),
        ))
        .with_status(200)
        .with_body(
            json!({
                "model": "claude-sonnet-4-5",
                "stop_reason": "end_turn",
                "usage": {"input_tokens": 10, "output_tokens": 20},
                "content": [
                    {"type": "thinking", "thinking": "Let me reason about this..."},
                    {"type": "text", "text": "The answer is 42."}
                ]
            })
            .to_string(),
        )
        .create_async()
        .await;

    let provider = AnthropicProvider::new("test-key", None, Some(server.url()));
    let request = ChatRequest::builder()
        .message(Message::user("What is the meaning of life?"))
        .thinking(8000)
        .build();

    let response = provider.chat(request).await.expect("chat response");

    // Text content should exclude thinking blocks
    assert_eq!(response.content, "The answer is 42.");
    // Thinking field should contain the thinking content
    assert_eq!(
        response.thinking.as_deref(),
        Some("Let me reason about this...")
    );

    mock.assert_async().await;
}

#[tokio::test]
async fn thinking_auto_sets_temperature_to_one() {
    let mut server = mockito::Server::new_async().await;
    // Verify temperature is 1.0 even though the user did not set it
    let mock = server
        .mock("POST", "/v1/messages")
        .match_header("x-api-key", "test-key")
        .match_body(Matcher::Regex(r#""temperature"\s*:\s*1\.0"#.to_string()))
        .with_status(200)
        .with_body(
            json!({
                "model": "claude-sonnet-4-5",
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
        .thinking(4000)
        .temperature(0.5) // should be overridden to 1.0
        .build();

    let _ = provider.chat(request).await.expect("chat response");
    mock.assert_async().await;
}

#[tokio::test]
async fn response_without_thinking_has_none() {
    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("POST", "/v1/messages")
        .match_header("x-api-key", "test-key")
        .with_status(200)
        .with_body(
            json!({
                "model": "claude-sonnet-4-5",
                "stop_reason": "end_turn",
                "usage": {"input_tokens": 5, "output_tokens": 10},
                "content": [{"type": "text", "text": "hello"}]
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
    assert_eq!(response.content, "hello");
    assert!(response.thinking.is_none());

    mock.assert_async().await;
}

#[test]
fn thinking_config_builder_sets_budget_tokens() {
    let request = ChatRequest::builder()
        .message(Message::user("test"))
        .thinking(16000)
        .build();

    assert!(request.thinking.is_some());
    assert_eq!(request.thinking.unwrap().budget_tokens, 16000);
}

#[test]
fn thinking_config_serde_roundtrip() {
    let config = ThinkingConfig {
        budget_tokens: 8000,
    };
    let json = serde_json::to_string(&config).unwrap();
    let deserialized: ThinkingConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(deserialized.budget_tokens, 8000);
}

#[test]
fn chat_response_thinking_field_serde() {
    let json = json!({
        "content": "answer",
        "thinking": "my reasoning",
        "tool_calls": [],
        "model": "claude-sonnet-4-5",
        "usage": {"input_tokens": 5, "output_tokens": 10},
        "stop_reason": "end_turn"
    });
    let resp: motosan_ai::ChatResponse = serde_json::from_value(json).unwrap();
    assert_eq!(resp.thinking.as_deref(), Some("my reasoning"));
    assert_eq!(resp.content, "answer");
}

#[test]
fn chat_response_without_thinking_deserializes() {
    let json = json!({
        "content": "answer",
        "tool_calls": [],
        "model": "claude-sonnet-4-5",
        "usage": {"input_tokens": 5, "output_tokens": 10},
        "stop_reason": "end_turn"
    });
    let resp: motosan_ai::ChatResponse = serde_json::from_value(json).unwrap();
    assert!(resp.thinking.is_none());
}
