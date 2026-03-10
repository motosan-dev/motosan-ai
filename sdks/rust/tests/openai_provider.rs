#![cfg(feature = "openai")]

use motosan_ai::providers::openai::OpenAIProvider;
use motosan_ai::providers::ProviderImpl;
use motosan_ai::{ChatRequest, Message, StopReason};
use serde_json::json;
use tokio_stream::StreamExt;

#[tokio::test]
async fn openai_chat_maps_response() {
    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("POST", "/v1/chat/completions")
        .match_header("authorization", "Bearer test-key")
        .with_status(200)
        .with_body(
            json!({
                "model": "gpt-4o",
                "choices": [{"message": {"content": "hello from openai"}, "finish_reason": "stop"}],
                "usage": {"prompt_tokens": 12, "completion_tokens": 8}
            })
            .to_string(),
        )
        .create_async()
        .await;

    let provider = OpenAIProvider::new("test-key", None, Some(server.url()));
    let request = ChatRequest::builder()
        .message(Message::system("rules"))
        .message(Message::user("hello"))
        .temperature(0.1)
        .max_tokens(50)
        .build();

    let response = provider.chat(request).await.expect("chat response");
    assert_eq!(response.content, "hello from openai");
    assert_eq!(response.model, "gpt-4o");
    assert_eq!(response.usage.input_tokens, 12);
    assert!(matches!(response.stop_reason, StopReason::Stop));

    mock.assert_async().await;
}

#[tokio::test]
async fn openai_stream_emits_deltas_and_done() {
    let mut server = mockito::Server::new_async().await;
    let sse_body = concat!(
        "data: {\"choices\":[{\"delta\":{\"content\":\"hel\"}}]}\n\n",
        "data: {\"choices\":[{\"delta\":{\"content\":\"lo\"}}]}\n\n",
        "data: [DONE]\n\n"
    );

    let mock = server
        .mock("POST", "/v1/chat/completions")
        .match_header("authorization", "Bearer test-key")
        .with_status(200)
        .with_header("content-type", "text/event-stream")
        .with_body(sse_body)
        .create_async()
        .await;

    let provider = OpenAIProvider::new("test-key", None, Some(server.url()));
    let request = ChatRequest::builder()
        .message(Message::user("hello"))
        .build();

    let mut stream = provider.stream(request).await.expect("stream response");
    let mut events = Vec::new();

    while let Some(event) = stream.next().await {
        events.push(event);
    }

    assert_eq!(events.len(), 3);
    assert_eq!(events[0].content, "hel");
    assert_eq!(events[1].content, "lo");
    assert!(events[2].done);

    mock.assert_async().await;
}

#[tokio::test]
async fn openai_stream_ignores_malformed_chunks() {
    let mut server = mockito::Server::new_async().await;
    let sse_body = concat!(
        "data: not-json\n\n",
        "data: {\"choices\":[{\"delta\":{\"content\":\"ok\"}}]}\n\n",
        "data: [DONE]\n\n"
    );

    let mock = server
        .mock("POST", "/v1/chat/completions")
        .match_header("authorization", "Bearer test-key")
        .with_status(200)
        .with_header("content-type", "text/event-stream")
        .with_body(sse_body)
        .create_async()
        .await;

    let provider = OpenAIProvider::new("test-key", None, Some(server.url()));
    let request = ChatRequest::builder()
        .message(Message::user("hello"))
        .build();

    let mut stream = provider.stream(request).await.expect("stream response");
    let mut events = Vec::new();

    while let Some(event) = stream.next().await {
        events.push(event);
    }

    assert_eq!(events.len(), 2);
    assert_eq!(events[0].content, "ok");
    assert!(!events[0].done);
    assert!(events[1].done);

    mock.assert_async().await;
}
