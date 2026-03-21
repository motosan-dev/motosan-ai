#![cfg(feature = "anthropic")]

use motosan_ai::providers::anthropic::AnthropicProvider;
use motosan_ai::providers::ProviderImpl;
use motosan_ai::{ChatRequest, ContentBlock, ImageSource, Message};
use serde_json::json;

#[tokio::test]
async fn anthropic_vision_request_serializes_content_blocks() {
    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("POST", "/v1/messages")
        .match_body(mockito::Matcher::Regex(
            r#""type"\s*:\s*"image""#.to_string(),
        ))
        .with_body(
            json!({
                "content": [{"type": "text", "text": "I see a cat"}],
                "model": "claude-sonnet-4-20250514",
                "role": "assistant",
                "stop_reason": "end_turn",
                "usage": {"input_tokens": 100, "output_tokens": 10}
            })
            .to_string(),
        )
        .create_async()
        .await;

    let provider = AnthropicProvider::new("test-key", None, Some(server.url()));

    let msg = Message::user_with_image("What is in this image?", "iVBOR...", "image/png");
    let request = ChatRequest::builder().messages(vec![msg]).build();

    let response = provider.chat(request).await.unwrap();
    assert_eq!(response.content, "I see a cat");
    mock.assert_async().await;
}

#[tokio::test]
async fn anthropic_plain_text_still_works() {
    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("POST", "/v1/messages")
        .match_body(mockito::Matcher::Regex(
            r#""content"\s*:\s*"hello""#.to_string(),
        ))
        .with_body(
            json!({
                "content": [{"type": "text", "text": "hi"}],
                "model": "claude-sonnet-4-20250514",
                "role": "assistant",
                "stop_reason": "end_turn",
                "usage": {"input_tokens": 5, "output_tokens": 2}
            })
            .to_string(),
        )
        .create_async()
        .await;

    let provider = AnthropicProvider::new("test-key", None, Some(server.url()));

    let msg = Message::user("hello");
    let request = ChatRequest::builder().messages(vec![msg]).build();

    let response = provider.chat(request).await.unwrap();
    assert_eq!(response.content, "hi");
    mock.assert_async().await;
}

#[tokio::test]
async fn anthropic_url_image_serializes_correctly() {
    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("POST", "/v1/messages")
        .match_body(mockito::Matcher::Regex(r#""type"\s*:\s*"url""#.to_string()))
        .with_body(
            json!({
                "content": [{"type": "text", "text": "A landscape photo"}],
                "model": "claude-sonnet-4-20250514",
                "role": "assistant",
                "stop_reason": "end_turn",
                "usage": {"input_tokens": 50, "output_tokens": 5}
            })
            .to_string(),
        )
        .create_async()
        .await;

    let provider = AnthropicProvider::new("test-key", None, Some(server.url()));

    let msg = Message::user_with_blocks(vec![
        ContentBlock::Text {
            text: "Describe this image".to_string(),
        },
        ContentBlock::Image {
            source: ImageSource::Url {
                url: "https://example.com/photo.jpg".to_string(),
            },
        },
    ]);
    let request = ChatRequest::builder().messages(vec![msg]).build();

    let response = provider.chat(request).await.unwrap();
    assert_eq!(response.content, "A landscape photo");
    mock.assert_async().await;
}
