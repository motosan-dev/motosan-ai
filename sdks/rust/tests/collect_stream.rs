#![cfg(feature = "anthropic")]

use mockito::Matcher;
use motosan_ai::providers::anthropic::AnthropicProvider;
use motosan_ai::providers::ProviderImpl;
use motosan_ai::{collect_stream, ChatRequest, Message, StopReason, StreamEvent, Tool};
use serde_json::json;

// ---------------------------------------------------------------------------
// collect_stream() unit tests (synthetic BoxStream, no HTTP)
// ---------------------------------------------------------------------------

fn boxed_stream(events: Vec<StreamEvent>) -> motosan_ai::BoxStream {
    Box::pin(tokio_stream::iter(events))
}

#[tokio::test]
async fn collect_stream_text_only() {
    let events = vec![
        StreamEvent::text("Hello"),
        StreamEvent::text(", world!"),
        StreamEvent::done(),
    ];
    let response = collect_stream(boxed_stream(events)).await;

    assert_eq!(response.content, "Hello, world!");
    assert!(response.tool_calls.is_empty());
    assert_eq!(response.stop_reason, StopReason::EndTurn);
    assert!(response.thinking.is_none());
}

#[tokio::test]
async fn collect_stream_with_tool_calls() {
    let events = vec![
        StreamEvent::text("Let me check"),
        StreamEvent::tool_call_start("toolu_1", "get_weather"),
        StreamEvent::tool_call_args_with_id("toolu_1", r#"{"city":"#),
        StreamEvent::tool_call_args_with_id("toolu_1", r#""Taipei"}"#),
        StreamEvent::tool_call_end_with_id("toolu_1"),
        StreamEvent::done(),
    ];
    let response = collect_stream(boxed_stream(events)).await;

    assert_eq!(response.content, "Let me check");
    assert_eq!(response.tool_calls.len(), 1);
    assert_eq!(response.tool_calls[0].id, "toolu_1");
    assert_eq!(response.tool_calls[0].name, "get_weather");
    assert_eq!(response.tool_calls[0].input, json!({"city": "Taipei"}));
    assert_eq!(response.stop_reason, StopReason::ToolUse);
}

#[tokio::test]
async fn collect_stream_multiple_tool_calls() {
    let events = vec![
        StreamEvent::tool_call_start("tc_1", "tool_a"),
        StreamEvent::tool_call_args(r#"{"x":1}"#),
        StreamEvent::tool_call_end(),
        StreamEvent::tool_call_start("tc_2", "tool_b"),
        StreamEvent::tool_call_args(r#"{"y":2}"#),
        StreamEvent::tool_call_end(),
        StreamEvent::done(),
    ];
    let response = collect_stream(boxed_stream(events)).await;

    assert_eq!(response.tool_calls.len(), 2);
    assert_eq!(response.tool_calls[0].name, "tool_a");
    assert_eq!(response.tool_calls[1].name, "tool_b");
    assert_eq!(response.stop_reason, StopReason::ToolUse);
}

#[tokio::test]
async fn collect_stream_with_usage_events() {
    let events = vec![
        StreamEvent::usage(motosan_ai::Usage {
            input_tokens: 100,
            output_tokens: 0,
            cache_creation_input_tokens: None,
            cache_read_input_tokens: None,
        }),
        StreamEvent::text("hi"),
        StreamEvent::usage(motosan_ai::Usage {
            input_tokens: 0,
            output_tokens: 42,
            cache_creation_input_tokens: None,
            cache_read_input_tokens: None,
        }),
        StreamEvent::done(),
    ];
    let response = collect_stream(boxed_stream(events)).await;

    assert_eq!(response.usage.input_tokens, 100);
    assert_eq!(response.usage.output_tokens, 42);
    assert_eq!(response.content, "hi");
}

#[tokio::test]
async fn collect_stream_empty_stream() {
    let events = vec![StreamEvent::done()];
    let response = collect_stream(boxed_stream(events)).await;

    assert_eq!(response.content, "");
    assert!(response.tool_calls.is_empty());
    assert_eq!(response.stop_reason, StopReason::EndTurn);
}

#[tokio::test]
async fn collect_stream_model_is_empty_by_default() {
    let events = vec![StreamEvent::text("x"), StreamEvent::done()];
    let response = collect_stream(boxed_stream(events)).await;
    assert_eq!(response.model, "");
}

// ---------------------------------------------------------------------------
// OAuth path now uses collect_stream internally (via AnthropicProvider)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn oauth_chat_uses_collect_stream_internally() {
    let mut server = mockito::Server::new_async().await;
    let sse_body = concat!(
        "event: message_start\n",
        "data: {\"type\":\"message_start\",\"message\":{\"usage\":{\"input_tokens\":10,\"output_tokens\":0}}}\n\n",
        "event: content_block_delta\n",
        "data: {\"type\":\"content_block_delta\",\"delta\":{\"text\":\"hello\"}}\n\n",
        "event: message_delta\n",
        "data: {\"type\":\"message_delta\",\"usage\":{\"input_tokens\":0,\"output_tokens\":5}}\n\n",
        "event: message_stop\n",
        "data: {\"type\":\"message_stop\"}\n\n"
    );

    let _mock = server
        .mock("POST", "/v1/messages")
        .match_header("authorization", "Bearer sk-ant-oat01-test-token")
        .match_header(
            "anthropic-beta",
            Matcher::Regex("oauth-2025-04-20".to_string()),
        )
        .with_status(200)
        .with_header("content-type", "text/event-stream")
        .with_body(sse_body)
        .create_async()
        .await;

    let provider = AnthropicProvider::new("sk-ant-oat01-test-token", None, Some(server.url()));
    let request = ChatRequest::builder().message(Message::user("hi")).build();

    let response = provider.chat(request).await.expect("chat response");

    assert_eq!(response.content, "hello");
    assert_eq!(response.usage.input_tokens, 10);
    assert_eq!(response.usage.output_tokens, 5);
    assert_eq!(response.stop_reason, StopReason::EndTurn);
    assert!(response.thinking.is_none());
}

// ---------------------------------------------------------------------------
// stream → collect_stream via AnthropicProvider (simulates stream_collect_with)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn stream_then_collect_returns_chat_response() {
    let mut server = mockito::Server::new_async().await;
    let sse_body = concat!(
        "event: message_start\n",
        "data: {\"type\":\"message_start\",\"message\":{\"usage\":{\"input_tokens\":15,\"output_tokens\":0}}}\n\n",
        "event: content_block_delta\n",
        "data: {\"type\":\"content_block_delta\",\"delta\":{\"text\":\"collected\"}}\n\n",
        "event: message_delta\n",
        "data: {\"type\":\"message_delta\",\"usage\":{\"input_tokens\":0,\"output_tokens\":7}}\n\n",
        "event: message_stop\n",
        "data: {\"type\":\"message_stop\"}\n\n"
    );

    let _mock = server
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

    let stream = provider.stream(request).await.expect("stream response");
    let response = collect_stream(stream).await;

    assert_eq!(response.content, "collected");
    assert_eq!(response.usage.input_tokens, 15);
    assert_eq!(response.usage.output_tokens, 7);
    assert_eq!(response.stop_reason, StopReason::EndTurn);
}

// ---------------------------------------------------------------------------
// Tool calls through stream + collect_stream
// ---------------------------------------------------------------------------

#[tokio::test]
async fn stream_collect_with_tool_calls() {
    let mut server = mockito::Server::new_async().await;
    let sse_body = concat!(
        "event: content_block_delta\n",
        "data: {\"type\":\"content_block_delta\",\"delta\":{\"text\":\"checking\"}}\n\n",
        "event: content_block_start\n",
        "data: {\"type\":\"content_block_start\",\"index\":1,\"content_block\":{\"type\":\"tool_use\",\"id\":\"t1\",\"name\":\"search\"}}\n\n",
        "event: content_block_delta\n",
        "data: {\"type\":\"content_block_delta\",\"index\":1,\"delta\":{\"type\":\"input_json_delta\",\"partial_json\":\"{\\\"q\\\":\\\"rust\\\"}\"}}\n\n",
        "event: content_block_stop\n",
        "data: {\"type\":\"content_block_stop\",\"index\":1}\n\n",
        "event: message_stop\n",
        "data: {\"type\":\"message_stop\"}\n\n"
    );

    let _mock = server
        .mock("POST", "/v1/messages")
        .match_header("x-api-key", "test-key")
        .with_status(200)
        .with_header("content-type", "text/event-stream")
        .with_body(sse_body)
        .create_async()
        .await;

    let provider = AnthropicProvider::new("test-key", None, Some(server.url()));
    let request = ChatRequest::builder()
        .message(Message::user("search for rust"))
        .tools(vec![Tool {
            schema: motosan_agent_primitives::ToolSchema {
                name: "search".to_string(),
                description: "Search".to_string(),
                input_schema: json!({"type": "object", "properties": {"q": {"type": "string"}}}),
            },
            cache: false,
        }])
        .build();

    let stream = provider.stream(request).await.expect("stream response");
    let response = collect_stream(stream).await;

    assert_eq!(response.content, "checking");
    assert_eq!(response.tool_calls.len(), 1);
    assert_eq!(response.tool_calls[0].name, "search");
    assert_eq!(response.tool_calls[0].input, json!({"q": "rust"}));
    assert_eq!(response.stop_reason, StopReason::ToolUse);
}

// ---------------------------------------------------------------------------
// Compare chat() vs stream+collect for same mock response (consistency)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn chat_vs_stream_collect_consistency() {
    // Set up two mocks — one for non-stream chat, one for stream
    let mut server = mockito::Server::new_async().await;

    let chat_response_body = json!({
        "id": "msg_1",
        "type": "message",
        "role": "assistant",
        "content": [{"type": "text", "text": "Hello there"}],
        "model": "claude-sonnet-4-20250514",
        "stop_reason": "end_turn",
        "usage": {"input_tokens": 12, "output_tokens": 3}
    });

    let sse_body = concat!(
        "event: message_start\n",
        "data: {\"type\":\"message_start\",\"message\":{\"usage\":{\"input_tokens\":12,\"output_tokens\":0}}}\n\n",
        "event: content_block_delta\n",
        "data: {\"type\":\"content_block_delta\",\"delta\":{\"text\":\"Hello there\"}}\n\n",
        "event: message_delta\n",
        "data: {\"type\":\"message_delta\",\"usage\":{\"input_tokens\":0,\"output_tokens\":3}}\n\n",
        "event: message_stop\n",
        "data: {\"type\":\"message_stop\"}\n\n"
    );

    // First call: non-streaming chat
    let _chat_mock = server
        .mock("POST", "/v1/messages")
        .match_header("x-api-key", "test-key")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(chat_response_body.to_string())
        .expect(1)
        .create_async()
        .await;

    let provider = AnthropicProvider::new("test-key", None, Some(server.url()));
    let chat_req = ChatRequest::builder().message(Message::user("hi")).build();
    let chat_response = provider.chat(chat_req).await.expect("chat response");

    // Remove first mock and create stream mock
    drop(_chat_mock);

    let _stream_mock = server
        .mock("POST", "/v1/messages")
        .match_header("x-api-key", "test-key")
        .with_status(200)
        .with_header("content-type", "text/event-stream")
        .with_body(sse_body)
        .expect(1)
        .create_async()
        .await;

    let stream_req = ChatRequest::builder().message(Message::user("hi")).build();
    let stream = provider.stream(stream_req).await.expect("stream response");
    let mut stream_response = collect_stream(stream).await;
    // collect_stream sets model to empty; match it for comparison
    stream_response.model = chat_response.model.clone();

    assert_eq!(chat_response.content, stream_response.content);
    assert_eq!(chat_response.stop_reason, stream_response.stop_reason);
    assert_eq!(
        chat_response.usage.input_tokens,
        stream_response.usage.input_tokens
    );
    assert_eq!(
        chat_response.usage.output_tokens,
        stream_response.usage.output_tokens
    );
    assert_eq!(
        chat_response.tool_calls.len(),
        stream_response.tool_calls.len()
    );
}

// ---------------------------------------------------------------------------
// stop_reason propagation through stream + collect_stream
// ---------------------------------------------------------------------------

#[tokio::test]
async fn collect_stream_propagates_explicit_stop_reason() {
    // Synthetic stream: text + done event carrying MaxTokens.
    let events = vec![
        StreamEvent::text("partial"),
        StreamEvent::done_with_stop_reason(StopReason::MaxTokens),
    ];
    let response = collect_stream(boxed_stream(events)).await;

    assert_eq!(response.content, "partial");
    assert_eq!(response.stop_reason, StopReason::MaxTokens);
}

#[tokio::test]
async fn collect_stream_falls_back_to_heuristic_when_no_explicit_reason() {
    // Plain done (no stop_reason) + tool call → should fall back to ToolUse.
    let events = vec![
        StreamEvent::tool_call_start("tool_1", "lookup"),
        StreamEvent::tool_call_args("{}"),
        StreamEvent::tool_call_end(),
        StreamEvent::done(),
    ];
    let response = collect_stream(boxed_stream(events)).await;

    assert_eq!(response.stop_reason, StopReason::ToolUse);
}

#[tokio::test]
async fn anthropic_stream_emits_max_tokens_stop_reason() {
    let mut server = mockito::Server::new_async().await;
    let sse_body = concat!(
        "event: content_block_delta\n",
        "data: {\"type\":\"content_block_delta\",\"delta\":{\"text\":\"truncated\"}}\n\n",
        "event: message_delta\n",
        "data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"max_tokens\"},\"usage\":{\"input_tokens\":4,\"output_tokens\":8}}\n\n",
        "event: message_stop\n",
        "data: {\"type\":\"message_stop\"}\n\n"
    );

    let mock = server
        .mock("POST", "/v1/messages")
        .match_header("x-api-key", Matcher::Any)
        .with_status(200)
        .with_header("content-type", "text/event-stream")
        .with_body(sse_body)
        .create_async()
        .await;

    let provider = AnthropicProvider::new("test-key", None, Some(server.url()));
    let request = ChatRequest::builder()
        .message(Message::user("write a long story"))
        .build();

    let mut stream = provider.stream(request).await.expect("stream response");
    let mut events: Vec<StreamEvent> = Vec::new();
    while let Some(event) = tokio_stream::StreamExt::next(&mut stream).await {
        events.push(event);
    }

    // Last event is the terminal done, and it must carry MaxTokens.
    let done = events.last().expect("at least one event");
    assert!(done.done);
    assert_eq!(done.stop_reason, Some(StopReason::MaxTokens));

    // Replay the captured events through collect_stream — same MaxTokens
    // should land on ChatResponse.stop_reason via the explicit-reason path.
    let synthesized = collect_stream(boxed_stream(events)).await;
    assert_eq!(synthesized.stop_reason, StopReason::MaxTokens);
    assert_eq!(synthesized.content, "truncated");

    mock.assert_async().await;
}
