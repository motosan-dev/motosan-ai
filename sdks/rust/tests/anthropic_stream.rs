#![cfg(feature = "anthropic")]

use mockito::Matcher;
use motosan_ai::providers::anthropic::AnthropicProvider;
use motosan_ai::providers::ProviderImpl;
use motosan_ai::{ChatRequest, Message, StopReason, StreamEventType, Tool};
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
        .match_header(
            "anthropic-beta",
            Matcher::Regex("oauth-2025-04-20".to_string()),
        )
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
            schema: motosan_agent_primitives::ToolSchema {
                name: "get_weather".to_string(),
                description: "Get weather".to_string(),
                input_schema: json!({"type": "object", "properties": {"city": {"type": "string"}}}),
            },
            cache: false,
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

    // Tool call args — must carry same id
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
    assert_eq!(args_events[0].tool_call_id.as_deref(), Some("toolu_1"));
    assert_eq!(args_events[1].tool_call_id.as_deref(), Some("toolu_1"));

    // Tool call end — must carry same id
    let ends: Vec<_> = received
        .iter()
        .filter(|e| e.event_type == StreamEventType::ToolCallEnd)
        .collect();
    assert_eq!(ends.len(), 1);
    assert_eq!(ends[0].tool_call_id.as_deref(), Some("toolu_1"));

    // Done
    assert!(received.last().unwrap().done);

    mock.assert_async().await;
}

// ---------------------------------------------------------------------------
// stop_reason variant coverage (table-driven via shared helper)
// ---------------------------------------------------------------------------

async fn anthropic_stream_with_message_delta_stop_reason(
    delta_reason: &str,
) -> motosan_ai::StreamEvent {
    let mut server = mockito::Server::new_async().await;
    let sse_body = format!(
        concat!(
            "event: content_block_delta\n",
            "data: {{\"type\":\"content_block_delta\",\"delta\":{{\"text\":\"hi\"}}}}\n\n",
            "event: message_delta\n",
            "data: {{\"type\":\"message_delta\",\"delta\":{{\"stop_reason\":\"{}\"}},\"usage\":{{\"input_tokens\":3,\"output_tokens\":1}}}}\n\n",
            "event: message_stop\n",
            "data: {{\"type\":\"message_stop\"}}\n\n"
        ),
        delta_reason
    );

    let mock = server
        .mock("POST", "/v1/messages")
        .match_header("x-api-key", Matcher::Any)
        .with_status(200)
        .with_header("content-type", "text/event-stream")
        .with_body(&sse_body)
        .create_async()
        .await;

    let provider = AnthropicProvider::new("test-key", None, Some(server.url()));
    let request = ChatRequest::builder()
        .message(Message::user("hello"))
        .build();

    let mut stream = provider.stream(request).await.expect("stream response");
    let mut events = Vec::new();
    while let Some(event) = stream.next().await {
        events.push(event);
    }
    mock.assert_async().await;

    events.into_iter().find(|e| e.done).expect("done event")
}

#[tokio::test]
async fn anthropic_stream_propagates_end_turn_stop_reason() {
    let done = anthropic_stream_with_message_delta_stop_reason("end_turn").await;
    assert_eq!(done.stop_reason, Some(StopReason::EndTurn));
}

#[tokio::test]
async fn anthropic_stream_propagates_tool_use_stop_reason() {
    let done = anthropic_stream_with_message_delta_stop_reason("tool_use").await;
    assert_eq!(done.stop_reason, Some(StopReason::ToolUse));
}

#[tokio::test]
async fn anthropic_stream_propagates_stop_sequence_stop_reason() {
    let done = anthropic_stream_with_message_delta_stop_reason("stop_sequence").await;
    assert_eq!(done.stop_reason, Some(StopReason::StopSequence));
}

#[tokio::test]
async fn anthropic_stream_propagates_unknown_stop_reason_as_other() {
    let done = anthropic_stream_with_message_delta_stop_reason("something_new").await;
    assert_eq!(done.stop_reason, Some(StopReason::Other));
}

#[tokio::test]
async fn thinking_block_start_then_immediate_stop_emits_nothing_yet() {
    // Pre-Task-3/4 state check: opening and immediately closing a thinking
    // block with no deltas must not crash and must not leak any spurious
    // events. After Task 4 the closing block will emit ThinkingDone(""),
    // and this test will be updated then.
    let mut server = mockito::Server::new_async().await;
    let sse_body = concat!(
        "event: content_block_start\n",
        "data: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"thinking\",\"thinking\":\"\"}}\n\n",
        "event: content_block_stop\n",
        "data: {\"type\":\"content_block_stop\",\"index\":0}\n\n",
        "event: content_block_start\n",
        "data: {\"type\":\"content_block_start\",\"index\":1,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}\n\n",
        "event: content_block_delta\n",
        "data: {\"type\":\"content_block_delta\",\"index\":1,\"delta\":{\"text\":\"ok\"}}\n\n",
        "event: message_stop\n",
        "data: {\"type\":\"message_stop\"}\n\n",
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
    let request = ChatRequest::builder().message(Message::user("hi")).build();
    let mut stream = provider.stream(request).await.expect("stream");

    let mut text_seen = String::new();
    let mut done_seen = false;
    while let Some(ev) = stream.next().await {
        if ev.done {
            done_seen = true;
            break;
        }
        if ev.event_type == StreamEventType::Text {
            text_seen.push_str(&ev.content);
        }
    }

    assert!(done_seen, "must terminate");
    assert_eq!(
        text_seen, "ok",
        "text after the thinking block must survive"
    );
    mock.assert_async().await;
}

#[tokio::test]
async fn thinking_delta_events_emitted_in_order() {
    let mut server = mockito::Server::new_async().await;
    let sse_body = concat!(
        "event: content_block_start\n",
        "data: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"thinking\",\"thinking\":\"\"}}\n\n",
        "event: content_block_delta\n",
        "data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"thinking_delta\",\"thinking\":\"Let me \"}}\n\n",
        "event: content_block_delta\n",
        "data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"thinking_delta\",\"thinking\":\"think...\"}}\n\n",
        "event: content_block_stop\n",
        "data: {\"type\":\"content_block_stop\",\"index\":0}\n\n",
        "event: message_stop\n",
        "data: {\"type\":\"message_stop\"}\n\n",
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
    let request = ChatRequest::builder().message(Message::user("hi")).build();
    let mut stream = provider.stream(request).await.expect("stream");

    let mut thinking_chunks: Vec<String> = Vec::new();
    let mut done_seen = false;
    while let Some(ev) = stream.next().await {
        if ev.done {
            done_seen = true;
            break;
        }
        if ev.event_type == StreamEventType::ThinkingDelta {
            thinking_chunks.push(ev.content);
        }
    }

    assert!(done_seen, "stream must terminate");
    assert_eq!(
        thinking_chunks,
        vec!["Let me ".to_string(), "think...".to_string()],
        "ThinkingDelta events must arrive in order with exact content"
    );
    mock.assert_async().await;
}

#[tokio::test]
async fn thinking_done_emitted_with_full_text_after_deltas() {
    let mut server = mockito::Server::new_async().await;
    let sse_body = concat!(
        "event: content_block_start\n",
        "data: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"thinking\",\"thinking\":\"\"}}\n\n",
        "event: content_block_delta\n",
        "data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"thinking_delta\",\"thinking\":\"A \"}}\n\n",
        "event: content_block_delta\n",
        "data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"thinking_delta\",\"thinking\":\"B \"}}\n\n",
        "event: content_block_delta\n",
        "data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"thinking_delta\",\"thinking\":\"C\"}}\n\n",
        "event: content_block_delta\n",
        "data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"signature_delta\",\"signature\":\"sig...\"}}\n\n",
        "event: content_block_stop\n",
        "data: {\"type\":\"content_block_stop\",\"index\":0}\n\n",
        "event: content_block_start\n",
        "data: {\"type\":\"content_block_start\",\"index\":1,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}\n\n",
        "event: content_block_delta\n",
        "data: {\"type\":\"content_block_delta\",\"index\":1,\"delta\":{\"text\":\"answer\"}}\n\n",
        "event: content_block_stop\n",
        "data: {\"type\":\"content_block_stop\",\"index\":1}\n\n",
        "event: message_stop\n",
        "data: {\"type\":\"message_stop\"}\n\n",
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
    let request = ChatRequest::builder().message(Message::user("hi")).build();
    let mut stream = provider.stream(request).await.expect("stream");

    let mut labels: Vec<String> = Vec::new();
    let mut done_seen = false;
    while let Some(ev) = stream.next().await {
        if ev.done {
            done_seen = true;
            break;
        }
        match ev.event_type {
            StreamEventType::ThinkingDelta => labels.push(format!("td:{}", ev.content)),
            StreamEventType::ThinkingDone => labels.push(format!("tD:{}", ev.content)),
            StreamEventType::Text => labels.push(format!("t:{}", ev.content)),
            _ => {}
        }
    }

    assert!(done_seen, "stream must terminate");
    assert_eq!(
        labels,
        vec![
            "td:A ".to_string(),
            "td:B ".to_string(),
            "td:C".to_string(),
            "tD:A B C".to_string(),
            "t:answer".to_string(),
        ],
        "Per-turn order: ThinkingDelta* -> ThinkingDone(full) -> Text*"
    );
    mock.assert_async().await;
}

#[tokio::test]
async fn thinking_done_not_emitted_for_non_thinking_block_stop() {
    // Regression guard: a content_block_stop that closes a tool_use or
    // text block must NOT emit a stray ThinkingDone. Only triggers when
    // current_thinking_buf was Some.
    let mut server = mockito::Server::new_async().await;
    let sse_body = concat!(
        "event: content_block_start\n",
        "data: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}\n\n",
        "event: content_block_delta\n",
        "data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"text\":\"hi\"}}\n\n",
        "event: content_block_stop\n",
        "data: {\"type\":\"content_block_stop\",\"index\":0}\n\n",
        "event: message_stop\n",
        "data: {\"type\":\"message_stop\"}\n\n",
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
    let request = ChatRequest::builder().message(Message::user("hi")).build();
    let mut stream = provider.stream(request).await.expect("stream");

    let mut saw_thinking_done = false;
    let mut done_seen = false;
    while let Some(ev) = stream.next().await {
        if ev.done {
            done_seen = true;
            break;
        }
        if ev.event_type == StreamEventType::ThinkingDone {
            saw_thinking_done = true;
        }
    }

    assert!(done_seen);
    assert!(
        !saw_thinking_done,
        "no thinking block was opened; no ThinkingDone expected"
    );
    mock.assert_async().await;
}

#[tokio::test]
async fn redacted_thinking_block_is_silently_consumed() {
    let mut server = mockito::Server::new_async().await;
    let sse_body = concat!(
        "event: content_block_start\n",
        "data: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"redacted_thinking\",\"data\":\"encrypted_blob_xyz\"}}\n\n",
        "event: content_block_stop\n",
        "data: {\"type\":\"content_block_stop\",\"index\":0}\n\n",
        "event: content_block_start\n",
        "data: {\"type\":\"content_block_start\",\"index\":1,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}\n\n",
        "event: content_block_delta\n",
        "data: {\"type\":\"content_block_delta\",\"index\":1,\"delta\":{\"text\":\"answer\"}}\n\n",
        "event: content_block_stop\n",
        "data: {\"type\":\"content_block_stop\",\"index\":1}\n\n",
        "event: message_stop\n",
        "data: {\"type\":\"message_stop\"}\n\n",
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
    let request = ChatRequest::builder().message(Message::user("hi")).build();
    let mut stream = provider.stream(request).await.expect("stream");

    let mut events: Vec<StreamEventType> = Vec::new();
    let mut content = String::new();
    let mut done_seen = false;
    while let Some(ev) = stream.next().await {
        if ev.done {
            done_seen = true;
            break;
        }
        events.push(ev.event_type.clone());
        if ev.event_type == StreamEventType::Text {
            content.push_str(&ev.content);
        }
    }

    assert!(done_seen);
    assert_eq!(content, "answer");
    assert!(
        !events.contains(&StreamEventType::ThinkingDelta),
        "redacted_thinking must not produce ThinkingDelta events"
    );
    assert!(
        !events.contains(&StreamEventType::ThinkingDone),
        "redacted_thinking must not produce a ThinkingDone event"
    );
    mock.assert_async().await;
}

#[tokio::test]
async fn orphan_thinking_delta_without_start_does_not_crash() {
    // Malformed/unexpected stream: a thinking_delta arrives without a
    // preceding content_block_start. Defensive Task-3 code emits the
    // event without crashing or polluting the accumulator.
    let mut server = mockito::Server::new_async().await;
    let sse_body = concat!(
        "event: content_block_delta\n",
        "data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"thinking_delta\",\"thinking\":\"orphan\"}}\n\n",
        "event: message_stop\n",
        "data: {\"type\":\"message_stop\"}\n\n",
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
    let request = ChatRequest::builder().message(Message::user("hi")).build();
    let mut stream = provider.stream(request).await.expect("stream");

    let mut saw_thinking_delta = false;
    let mut saw_thinking_done = false;
    let mut done_seen = false;
    while let Some(ev) = stream.next().await {
        if ev.done {
            done_seen = true;
            break;
        }
        match ev.event_type {
            StreamEventType::ThinkingDelta => {
                saw_thinking_delta = true;
                assert_eq!(ev.content, "orphan");
            }
            StreamEventType::ThinkingDone => saw_thinking_done = true,
            _ => {}
        }
    }

    assert!(done_seen);
    assert!(
        saw_thinking_delta,
        "orphan ThinkingDelta should still be emitted"
    );
    assert!(
        !saw_thinking_done,
        "no thinking block was opened or closed; no ThinkingDone expected"
    );
    mock.assert_async().await;
}
