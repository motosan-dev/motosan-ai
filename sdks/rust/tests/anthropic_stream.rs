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

#[tokio::test]
async fn anthropic_stream_setup_token_uses_bearer_and_oauth_beta_header() {
    let mut server = mockito::Server::new_async().await;
    let sse_body = concat!(
        "event: content_block_delta\n",
        "data: {\"type\":\"content_block_delta\",\"delta\":{\"text\":\"ok\"}}\n\n",
        "event: message_stop\n",
        "data: {\"type\":\"message_stop\"}\n\n"
    );

    let mock = server
        .mock("POST", "/v1/messages")
        .match_header("authorization", "Bearer sk-ant-oat01-stream-token")
        .match_header("anthropic-beta", "oauth-2025-04-20")
        .match_header("anthropic-version", "2023-06-01")
        .with_status(200)
        .with_header("content-type", "text/event-stream")
        .with_body(sse_body)
        .create_async()
        .await;

    let provider = AnthropicProvider::new("sk-ant-oat01-stream-token", None, Some(server.url()));
    let request = ChatRequest::builder()
        .message(Message::user("hello"))
        .build();

    let mut stream = provider.stream(request).await.expect("stream response");
    let first = stream.next().await.expect("first event");
    let done = stream.next().await.expect("done event");

    assert_eq!(first.content, "ok");
    assert!(done.done);
    mock.assert_async().await;
}

#[tokio::test]
async fn anthropic_stream_with_system_and_max_tokens() {
    let mut server = mockito::Server::new_async().await;
    let sse_body = concat!(
        "event: content_block_delta\n",
        "data: {\"type\":\"content_block_delta\",\"delta\":{\"text\":\"reply\"}}\n\n",
        "event: message_stop\n",
        "data: {\"type\":\"message_stop\"}\n\n"
    );

    let mock = server
        .mock("POST", "/v1/messages")
        .match_header("x-api-key", "test-key")
        .match_body(mockito::Matcher::AllOf(vec![
            mockito::Matcher::Regex(r#""system"\s*:\s*"test""#.to_string()),
            mockito::Matcher::Regex(r#""max_tokens"\s*:\s*100"#.to_string()),
        ]))
        .with_status(200)
        .with_header("content-type", "text/event-stream")
        .with_body(sse_body)
        .create_async()
        .await;

    let provider = AnthropicProvider::new("test-key", None, Some(server.url()));
    let request = ChatRequest::builder()
        .system("test")
        .max_tokens(100)
        .message(Message::user("hello"))
        .build();

    let mut stream = provider.stream(request).await.expect("stream response");
    let mut received = Vec::new();

    while let Some(event) = stream.next().await {
        received.push(event);
    }

    assert_eq!(received.len(), 2);
    assert_eq!(received[0].content, "reply");
    assert!(!received[0].done);
    assert!(received[1].done);

    mock.assert_async().await;
}

#[tokio::test]
async fn stream_with_passes_system_and_max_tokens_to_provider() {
    let mut server = mockito::Server::new_async().await;
    let sse_body = concat!(
        "event: content_block_delta\n",
        "data: {\"type\":\"content_block_delta\",\"delta\":{\"text\":\"ok\"}}\n\n",
        "event: message_stop\n",
        "data: {\"type\":\"message_stop\"}\n\n"
    );

    let mock = server
        .mock("POST", "/v1/messages")
        .match_header("x-api-key", "test-key")
        .match_body(mockito::Matcher::AllOf(vec![
            mockito::Matcher::Regex(r#""system"\s*:\s*"be helpful""#.to_string()),
            mockito::Matcher::Regex(r#""max_tokens"\s*:\s*100"#.to_string()),
            mockito::Matcher::Regex(r#""stream"\s*:\s*true"#.to_string()),
        ]))
        .with_status(200)
        .with_header("content-type", "text/event-stream")
        .with_body(sse_body)
        .create_async()
        .await;

    let provider = AnthropicProvider::new("test-key", None, Some(server.url()));
    let request = ChatRequest::builder()
        .message(Message::user("hello"))
        .system("be helpful")
        .max_tokens(100)
        .build();

    let mut stream = provider.stream(request).await.expect("stream response");
    let mut received = Vec::new();
    while let Some(event) = stream.next().await {
        received.push(event);
    }

    assert_eq!(received.len(), 2);
    assert_eq!(received[0].content, "ok");
    assert!(received[1].done);
    mock.assert_async().await;
}

#[tokio::test]
async fn client_stream_with_dispatches_to_provider() {
    let mut server = mockito::Server::new_async().await;
    let sse_body = concat!(
        "event: content_block_delta\n",
        "data: {\"type\":\"content_block_delta\",\"delta\":{\"text\":\"hi\"}}\n\n",
        "event: message_stop\n",
        "data: {\"type\":\"message_stop\"}\n\n"
    );

    let _mock = server
        .mock("POST", "/v1/messages")
        .with_status(200)
        .with_header("content-type", "text/event-stream")
        .with_body(sse_body)
        .create_async()
        .await;

    let provider = AnthropicProvider::new("test-key", None, Some(server.url()));
    let request = ChatRequest::builder()
        .message(Message::user("hi"))
        .system("test system prompt")
        .max_tokens(200)
        .temperature(0.5)
        .build();

    let mut stream = provider
        .stream(request)
        .await
        .expect("stream_with response");
    let first = stream.next().await.expect("first event");
    assert_eq!(first.content, "hi");
}
