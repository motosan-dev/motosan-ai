#![cfg(feature = "gemini")]

use motosan_ai::providers::gemini::GeminiProvider;
use motosan_ai::providers::ProviderImpl;
use motosan_ai::{ChatRequest, ContentBlock, ImageSource, Message};
use serde_json::json;

fn mock_response(text: &str) -> String {
    json!({
        "candidates": [{
            "content": {"parts": [{"text": text}], "role": "model"},
            "finishReason": "STOP"
        }],
        "usageMetadata": {"promptTokenCount": 50, "candidatesTokenCount": 5},
        "modelVersion": "gemini-2.5-flash"
    })
    .to_string()
}

#[tokio::test]
async fn gemini_base64_image_serializes_as_inline_data() {
    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock(
            "POST",
            mockito::Matcher::Regex(r"generateContent".to_string()),
        )
        .match_body(mockito::Matcher::Regex(r#""inlineData""#.to_string()))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(mock_response("A red image"))
        .create_async()
        .await;

    let provider = GeminiProvider::new("test-key", None, Some(server.url()));
    let msg = Message::user_with_image("What color is this?", "abc123==", "image/png");
    let resp = provider
        .chat(ChatRequest::builder().messages(vec![msg]).build())
        .await
        .unwrap();

    assert_eq!(resp.content, "A red image");
    mock.assert_async().await;
}

#[tokio::test]
async fn gemini_base64_image_uses_correct_mime_type() {
    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock(
            "POST",
            mockito::Matcher::Regex(r"generateContent".to_string()),
        )
        .match_body(mockito::Matcher::Regex(
            r#""mimeType"\s*:\s*"image/jpeg""#.to_string(),
        ))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(mock_response("A JPEG image"))
        .create_async()
        .await;

    let provider = GeminiProvider::new("test-key", None, Some(server.url()));
    let msg = Message::user_with_image("Describe this", "abc123==", "image/jpeg");
    let resp = provider
        .chat(ChatRequest::builder().messages(vec![msg]).build())
        .await
        .unwrap();

    assert_eq!(resp.content, "A JPEG image");
    mock.assert_async().await;
}

#[tokio::test]
async fn gemini_url_image_serializes_as_file_data() {
    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock(
            "POST",
            mockito::Matcher::Regex(r"generateContent".to_string()),
        )
        .match_body(mockito::Matcher::Regex(r#""fileData""#.to_string()))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(mock_response("A landscape"))
        .create_async()
        .await;

    let provider = GeminiProvider::new("test-key", None, Some(server.url()));
    let msg = Message::user_with_blocks(vec![
        ContentBlock::Text {
            text: "Describe this".to_string(),
        },
        ContentBlock::Image {
            source: ImageSource::Url {
                url: "https://example.com/photo.jpg".to_string(),
            },
        },
    ]);
    let resp = provider
        .chat(ChatRequest::builder().messages(vec![msg]).build())
        .await
        .unwrap();

    assert_eq!(resp.content, "A landscape");
    mock.assert_async().await;
}

#[tokio::test]
async fn gemini_url_image_includes_file_uri() {
    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock(
            "POST",
            mockito::Matcher::Regex(r"generateContent".to_string()),
        )
        .match_body(mockito::Matcher::Regex(
            r#""fileUri"\s*:\s*"https://example\.com"#.to_string(),
        ))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(mock_response("ok"))
        .create_async()
        .await;

    let provider = GeminiProvider::new("test-key", None, Some(server.url()));
    let msg = Message::user_with_blocks(vec![
        ContentBlock::Image {
            source: ImageSource::Url {
                url: "https://example.com/img.png".to_string(),
            },
        },
        ContentBlock::Text {
            text: "What is this?".to_string(),
        },
    ]);
    provider
        .chat(ChatRequest::builder().messages(vec![msg]).build())
        .await
        .unwrap();
    mock.assert_async().await;
}

#[tokio::test]
async fn gemini_plain_text_still_works() {
    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock(
            "POST",
            mockito::Matcher::Regex(r"generateContent".to_string()),
        )
        .match_body(mockito::Matcher::Regex(
            r#""text"\s*:\s*"hello""#.to_string(),
        ))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(mock_response("hi"))
        .create_async()
        .await;

    let provider = GeminiProvider::new("test-key", None, Some(server.url()));
    let msg = Message::user("hello");
    let resp = provider
        .chat(ChatRequest::builder().messages(vec![msg]).build())
        .await
        .unwrap();

    assert_eq!(resp.content, "hi");
    mock.assert_async().await;
}

#[tokio::test]
async fn gemini_mixed_text_and_image_sends_both_parts() {
    let mut server = mockito::Server::new_async().await;
    // Both "inlineData" and "text" must appear in the request body
    let mock = server
        .mock(
            "POST",
            mockito::Matcher::Regex(r"generateContent".to_string()),
        )
        .match_body(mockito::Matcher::AllOf(vec![
            mockito::Matcher::Regex(r#""inlineData""#.to_string()),
            mockito::Matcher::Regex(r#""text""#.to_string()),
        ]))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(mock_response("Mixed content received"))
        .create_async()
        .await;

    let provider = GeminiProvider::new("test-key", None, Some(server.url()));
    let msg = Message::user_with_image("Describe this image", "abc123==", "image/png");
    let resp = provider
        .chat(ChatRequest::builder().messages(vec![msg]).build())
        .await
        .unwrap();

    assert_eq!(resp.content, "Mixed content received");
    mock.assert_async().await;
}
