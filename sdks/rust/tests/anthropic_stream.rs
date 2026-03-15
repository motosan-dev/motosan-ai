#![cfg(feature = "anthropic")]

use motosan_ai::providers::anthropic::AnthropicProvider;
use motosan_ai::providers::ProviderImpl;
use motosan_ai::{ChatRequest, Message, StreamEventType, Tool};
use serde_json::json;
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

#[tokio::test]
async fn anthropic_stream_emits_tool_use_events() {
    let mut server = mockito::Server::new_async().await;
    let sse_body = concat!(
        "event: content_block_start\n",
        "data: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}\n\n",
        "event: content_block_delta\n",
        "data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"Let me check\"}}\n\n",
        "event: content_block_stop\n",
        "data: {\"type\":\"content_block_stop\",\"index\":0}\n\n",
        "event: content_block_start\n",
        "data: {\"type\":\"content_block_start\",\"index\":1,\"content_block\":{\"type\":\"tool_use\",\"id\":\"toolu_1\",\"name\":\"get_weather\"}}\n\n",
        "event: content_block_delta\n",
        "data: {\"type\":\"content_block_delta\",\"index\":1,\"delta\":{\"type\":\"input_json_delta\",\"partial_json\":\"{\\\"city\\\":\"}}\n\n",
        "event: content_block_delta\n",
        "data: {\"type\":\"content_block_delta\",\"index\":1,\"delta\":{\"type\":\"input_json_delta\",\"partial_json\":\"\\\"Taipei\\\"}\"}}\n\n",
        "event: content_block_stop\n",
        "data: {\"type\":\"content_block_stop\",\"index\":1}\n\n",
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
        .message(Message::user("weather in Taipei?"))
        .tools(vec![Tool {
            name: "get_weather".to_string(),
            description: Some("Get weather".to_string()),
            input_schema: Some(
                json!({"type": "object", "properties": {"city": {"type": "string"}}}),
            ),
        }])
        .build();

    let mut stream = provider.stream(request).await.expect("stream response");
    let mut received = Vec::new();
    while let Some(event) = stream.next().await {
        received.push(event);
    }

    // Text event
    let text_events: Vec<_> = received
        .iter()
        .filter(|e| e.event_type == StreamEventType::Text && !e.done)
        .collect();
    assert_eq!(text_events.len(), 1);
    assert_eq!(text_events[0].content, "Let me check");

    // Tool call start
    let starts: Vec<_> = received
        .iter()
        .filter(|e| e.event_type == StreamEventType::ToolCallStart)
        .collect();
    assert_eq!(starts.len(), 1);
    assert_eq!(starts[0].tool_call_id.as_deref(), Some("toolu_1"));
    assert_eq!(starts[0].tool_call_name.as_deref(), Some("get_weather"));

    // Tool call args
    let args_events: Vec<_> = received
        .iter()
        .filter(|e| e.event_type == StreamEventType::ToolCallArgs)
        .collect();
    assert_eq!(args_events.len(), 2);
    let full_args: String = args_events
        .iter()
        .filter_map(|e| e.tool_call_args_delta.as_deref())
        .collect();
    assert_eq!(full_args, "{\"city\":\"Taipei\"}");

    // Tool call end
    let ends: Vec<_> = received
        .iter()
        .filter(|e| e.event_type == StreamEventType::ToolCallEnd)
        .collect();
    assert!(!ends.is_empty());

    // Done
    assert!(received.last().unwrap().done);

    mock.assert_async().await;
}
