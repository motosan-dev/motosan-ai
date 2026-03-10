#![cfg(feature = "anthropic")]

use motosan_ai::providers::anthropic::AnthropicProvider;
use motosan_ai::providers::ProviderImpl;
use motosan_ai::{ChatRequest, Message, StopReason};
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
