#![cfg(feature = "openai")]

use motosan_ai::providers::openai::OpenAIProvider;
use motosan_ai::providers::ProviderImpl;
use motosan_ai::{ChatRequest, ContentBlock, ImageSource, Message};
use serde_json::json;

#[tokio::test]
async fn openai_vision_request_serializes_content_blocks() {
    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("POST", "/v1/chat/completions")
        .match_body(mockito::Matcher::Regex(
            r#""type"\s*:\s*"image_url""#.to_string(),
        ))
        .with_body(
            json!({
                "choices": [{"message": {"role": "assistant", "content": "I see a cat"}, "finish_reason": "stop"}],
                "model": "gpt-4o",
                "usage": {"prompt_tokens": 100, "completion_tokens": 10}
            })
            .to_string(),
        )
        .create_async()
        .await;

    let provider = OpenAIProvider::new("test-key", None, Some(server.url()));

    let msg = Message::user_with_image("What is in this image?", "iVBOR...", "image/png");
    let request = ChatRequest::builder().messages(vec![msg]).build();

    let response = provider.chat(request).await.unwrap();
    assert_eq!(response.content, "I see a cat");
    mock.assert_async().await;
}

#[tokio::test]
async fn openai_vision_base64_uses_data_url_format() {
    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("POST", "/v1/chat/completions")
        .match_body(mockito::Matcher::Regex(
            r#"data:image/png;base64,"#.to_string(),
        ))
        .with_body(
            json!({
                "choices": [{"message": {"role": "assistant", "content": "A PNG image"}, "finish_reason": "stop"}],
                "model": "gpt-4o",
                "usage": {"prompt_tokens": 50, "completion_tokens": 5}
            })
            .to_string(),
        )
        .create_async()
        .await;

    let provider = OpenAIProvider::new("test-key", None, Some(server.url()));

    let msg = Message::user_with_image("Describe this", "abc123==", "image/png");
    let request = ChatRequest::builder().messages(vec![msg]).build();

    let response = provider.chat(request).await.unwrap();
    assert_eq!(response.content, "A PNG image");
    mock.assert_async().await;
}

#[tokio::test]
async fn openai_plain_text_still_works() {
    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("POST", "/v1/chat/completions")
        .match_body(mockito::Matcher::Regex(
            r#""content"\s*:\s*"hello""#.to_string(),
        ))
        .with_body(
            json!({
                "choices": [{"message": {"role": "assistant", "content": "hi"}, "finish_reason": "stop"}],
                "model": "gpt-4o",
                "usage": {"prompt_tokens": 5, "completion_tokens": 2}
            })
            .to_string(),
        )
        .create_async()
        .await;

    let provider = OpenAIProvider::new("test-key", None, Some(server.url()));

    let msg = Message::user("hello");
    let request = ChatRequest::builder().messages(vec![msg]).build();

    let response = provider.chat(request).await.unwrap();
    assert_eq!(response.content, "hi");
    mock.assert_async().await;
}

#[tokio::test]
async fn openai_url_image_serializes_correctly() {
    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("POST", "/v1/chat/completions")
        .match_body(mockito::Matcher::Regex(
            r#""url"\s*:\s*"https://"#.to_string(),
        ))
        .with_body(
            json!({
                "choices": [{"message": {"role": "assistant", "content": "A landscape"}, "finish_reason": "stop"}],
                "model": "gpt-4o",
                "usage": {"prompt_tokens": 50, "completion_tokens": 5}
            })
            .to_string(),
        )
        .create_async()
        .await;

    let provider = OpenAIProvider::new("test-key", None, Some(server.url()));

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
    assert_eq!(response.content, "A landscape");
    mock.assert_async().await;
}
