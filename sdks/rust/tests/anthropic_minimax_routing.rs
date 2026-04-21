#![cfg(feature = "minimax")]

use mockito::Matcher;
use motosan_ai::{Client, Message, MotosanError, Provider, StopReason};
use serde_json::json;

#[tokio::test]
async fn minimax_routes_via_anthropic_messages_endpoint_with_default_model() {
    let mut server = mockito::Server::new_async().await;
    let base = format!("{}/anthropic", server.url());

    let mock = server
        .mock("POST", "/anthropic/v1/messages")
        .match_header("x-api-key", "test-key")
        .match_header("anthropic-version", "2023-06-01")
        .match_body(Matcher::Regex(
            r#"\"model\"\s*:\s*\"MiniMax-M2\.7\""#.to_string(),
        ))
        .with_status(200)
        .with_body(
            json!({
                "id": "msg_1",
                "type": "message",
                "role": "assistant",
                "model": "MiniMax-M2.7",
                "stop_reason": "end_turn",
                "usage": {"input_tokens": 3, "output_tokens": 2},
                "content": [{"type": "text", "text": "hello from minimax"}]
            })
            .to_string(),
        )
        .create_async()
        .await;

    let client = Client::builder()
        .provider(Provider::Minimax)
        .api_key("test-key")
        .minimax_base_url(base)
        .build()
        .expect("client build");

    let response = client
        .chat(vec![Message::user("hello")])
        .await
        .expect("chat response");

    assert_eq!(response.content, "hello from minimax");
    assert!(matches!(response.stop_reason, StopReason::EndTurn));
    mock.assert_async().await;
}

#[tokio::test]
async fn minimax_declares_text_only_capabilities() {
    let client = Client::builder()
        .provider(Provider::Minimax)
        .api_key("test-key")
        .build()
        .expect("client build");

    let req = motosan_ai::ChatRequest::builder()
        .message(Message::user_with_image("look", "abc123", "image/png"))
        .build();

    let result = client.chat_with(req).await;
    assert!(matches!(result, Err(MotosanError::UnsupportedFeature(_))));
}
