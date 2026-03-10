#![cfg(feature = "anthropic")]

use motosan_ai::providers::anthropic::AnthropicProvider;
use motosan_ai::providers::ProviderImpl;
use motosan_ai::{ChatRequest, Message};
use tokio_stream::StreamExt;

#[tokio::test]
async fn anthropic_stream_emits_content_and_done_event() {
    let mut server = mockito::Server::new_async().await;
    let sse_body = concat!(
        "event: content_block_delta\n",
        "data: {\"type\":\"content_block_delta\",\"delta\":{\"text\":\"hel\"}}\n\n",
        "event: content_block_delta\n",
        "data: {\"type\":\"content_block_delta\",\"delta\":{\"text\":\"lo\"}}\n\n",
        "event: message_stop\n",
        "data: {\"type\":\"message_stop\"}\n\n"
    );

    let mock = server
        .mock("POST", "/v1/messages")
        .match_header("x-api-key", "test-key")
        .with_status(200)
        .with_header("content-type", "text/event-stream")
        .with_body(sse_body)
        .create_async()
        .await;

    let provider = AnthropicProvider::new("test-key", None, Some(server.url()));
    let request = ChatRequest::builder()
        .message(Message::user("hello"))
        .build();

    let mut stream = provider.stream(request).await.expect("stream response");
    let mut received = Vec::new();

    while let Some(event) = stream.next().await {
        received.push(event);
    }

    assert_eq!(received.len(), 3);
    assert_eq!(received[0].content, "hel");
    assert!(!received[0].done);
    assert_eq!(received[1].content, "lo");
    assert!(!received[1].done);
    assert!(received[2].done);

    mock.assert_async().await;
}

#[tokio::test]
async fn anthropic_stream_ignores_unknown_and_malformed_events() {
    let mut server = mockito::Server::new_async().await;
    let sse_body = concat!(
        "event: ping\n",
        "data: {\"type\":\"ping\"}\n\n",
        "event: content_block_delta\n",
        "data: not-json\n\n",
        "event: some_unknown_event\n",
        "data: {\"type\":\"unknown\"}\n\n",
        "event: content_block_delta\n",
        "data: {\"type\":\"content_block_delta\",\"delta\":{\"text\":\"ok\"}}\n\n",
        "event: message_stop\n",
        "data: {\"type\":\"message_stop\"}\n\n"
    );

    let mock = server
        .mock("POST", "/v1/messages")
        .match_header("x-api-key", "test-key")
        .with_status(200)
        .with_header("content-type", "text/event-stream")
        .with_body(sse_body)
        .create_async()
        .await;

    let provider = AnthropicProvider::new("test-key", None, Some(server.url()));
    let request = ChatRequest::builder()
        .message(Message::user("hello"))
        .build();

    let mut stream = provider.stream(request).await.expect("stream response");
    let mut received = Vec::new();

    while let Some(event) = stream.next().await {
        received.push(event);
    }

    assert_eq!(received.len(), 2);
    assert_eq!(received[0].content, "ok");
    assert!(!received[0].done);
    assert!(received[1].done);

    mock.assert_async().await;
}
