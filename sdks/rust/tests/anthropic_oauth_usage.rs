#![cfg(feature = "anthropic")]

use mockito::Matcher;
use motosan_ai::providers::anthropic::AnthropicProvider;
use motosan_ai::providers::ProviderImpl;
use motosan_ai::{ChatRequest, Message, StreamEventType};
use serde_json::json;
use tokio_stream::StreamExt;

/// OAuth chat() collects usage tokens from message_start and message_delta
/// stream events instead of hardcoding them to 0.
#[tokio::test]
async fn oauth_chat_captures_usage_from_stream_events() {
    let mut server = mockito::Server::new_async().await;

    // Simulate a realistic Anthropic SSE stream with usage in
    // message_start (input_tokens) and message_delta (output_tokens).
    let sse_body = [
        "event: message_start",
        &format!(
            "data: {}",
            json!({
                "type": "message_start",
                "message": {
                    "id": "msg_test",
                    "type": "message",
                    "role": "assistant",
                    "content": [],
                    "model": "claude-sonnet-4-5-20250514",
                    "usage": {"input_tokens": 100, "output_tokens": 0}
                }
            })
        ),
        "",
        "event: content_block_delta",
        &format!(
            "data: {}",
            json!({"type": "content_block_delta", "index": 0, "delta": {"type": "text_delta", "text": "Hello!"}})
        ),
        "",
        "event: message_delta",
        &format!(
            "data: {}",
            json!({
                "type": "message_delta",
                "delta": {"stop_reason": "end_turn"},
                "usage": {"output_tokens": 42}
            })
        ),
        "",
        "event: message_stop",
        &format!("data: {}", json!({"type": "message_stop"})),
        "",
    ]
    .join("\n");

    let mock = server
        .mock("POST", "/v1/messages")
        .match_header("authorization", "Bearer sk-ant-oat01-usage-test")
        .match_header(
            "anthropic-beta",
            Matcher::Regex("oauth-2025-04-20".to_string()),
        )
        .with_status(200)
        .with_header("content-type", "text/event-stream")
        .with_body(sse_body)
        .create_async()
        .await;

    let provider = AnthropicProvider::new("sk-ant-oat01-usage-test", None, Some(server.url()));
    let request = ChatRequest::builder()
        .message(Message::user("hello"))
        .build();

    let response = provider.chat(request).await.expect("chat response");

    assert_eq!(response.content, "Hello!");
    assert_eq!(
        response.usage.input_tokens, 100,
        "input_tokens should come from message_start"
    );
    assert_eq!(
        response.usage.output_tokens, 42,
        "output_tokens should come from message_delta"
    );

    mock.assert_async().await;
}

/// Stream path emits Usage events that carry token counts.
#[tokio::test]
async fn stream_emits_usage_events_from_message_start_and_delta() {
    let mut server = mockito::Server::new_async().await;

    let sse_body = [
        "event: message_start",
        &format!(
            "data: {}",
            json!({
                "type": "message_start",
                "message": {
                    "id": "msg_test",
                    "type": "message",
                    "role": "assistant",
                    "content": [],
                    "model": "claude-sonnet-4-5-20250514",
                    "usage": {"input_tokens": 55, "output_tokens": 0}
                }
            })
        ),
        "",
        "event: content_block_delta",
        &format!(
            "data: {}",
            json!({"type": "content_block_delta", "index": 0, "delta": {"type": "text_delta", "text": "Hi"}})
        ),
        "",
        "event: message_delta",
        &format!(
            "data: {}",
            json!({
                "type": "message_delta",
                "delta": {"stop_reason": "end_turn"},
                "usage": {"output_tokens": 10}
            })
        ),
        "",
        "event: message_stop",
        &format!("data: {}", json!({"type": "message_stop"})),
        "",
    ]
    .join("\n");

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
    let mut usage_events = Vec::new();

    while let Some(event_item) = stream.next().await {
        let event = event_item.expect("stream item should not fail");
        if event.event_type == StreamEventType::Usage {
            usage_events.push(event);
        }
    }

    assert_eq!(usage_events.len(), 2, "should emit 2 usage events");

    // message_start usage
    let start_usage = usage_events[0].usage.as_ref().expect("usage field");
    assert_eq!(start_usage.input_tokens, 55);
    assert_eq!(start_usage.output_tokens, 0);

    // message_delta usage
    let delta_usage = usage_events[1].usage.as_ref().expect("usage field");
    assert_eq!(delta_usage.input_tokens, 0);
    assert_eq!(delta_usage.output_tokens, 10);

    mock.assert_async().await;
}
