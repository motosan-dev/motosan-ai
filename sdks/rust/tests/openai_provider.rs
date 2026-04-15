#![cfg(feature = "openai")]

use mockito::Matcher;
use motosan_ai::providers::openai::{OpenAIAuthStyle, OpenAIProvider};
use motosan_ai::providers::ProviderImpl;
use motosan_ai::{
    ChatRequest, Message, MotosanError, StopReason, StreamEventType, Tool, DEFAULT_OPENAI_MODEL,
};
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

    let provider = OpenAIProvider::new("test-key", None)
        .with_chat_url(format!("{}/v1/chat/completions", server.url()));
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
async fn openai_request_uses_default_model_and_allows_override() {
    let mut server = mockito::Server::new_async().await;

    let default_mock = server
        .mock("POST", "/v1/chat/completions")
        .match_header("authorization", "Bearer test-key")
        .match_body(Matcher::Regex(format!(
            r#"\"model\"\s*:\s*\"{}\""#,
            DEFAULT_OPENAI_MODEL
        )))
        .with_status(200)
        .with_body(
            json!({
                "model": DEFAULT_OPENAI_MODEL,
                "choices": [{"message": {"content": "ok"}, "finish_reason": "stop"}],
                "usage": {"prompt_tokens": 1, "completion_tokens": 1}
            })
            .to_string(),
        )
        .create_async()
        .await;

    let provider = OpenAIProvider::new("test-key", None)
        .with_chat_url(format!("{}/v1/chat/completions", server.url()));
    let request = ChatRequest::builder()
        .message(Message::user("hello"))
        .build();
    let _ = provider.chat(request).await.expect("chat response");
    default_mock.assert_async().await;

    server.reset();
    let override_model = "gpt-4o";
    let override_mock = server
        .mock("POST", "/v1/chat/completions")
        .match_header("authorization", "Bearer test-key")
        .match_body(Matcher::Regex(format!(
            r#"\"model\"\s*:\s*\"{}\""#,
            override_model
        )))
        .with_status(200)
        .with_body(
            json!({
                "model": override_model,
                "choices": [{"message": {"content": "ok"}, "finish_reason": "stop"}],
                "usage": {"prompt_tokens": 1, "completion_tokens": 1}
            })
            .to_string(),
        )
        .create_async()
        .await;

    let request = ChatRequest::builder()
        .message(Message::user("hello"))
        .model(override_model)
        .build();
    let _ = provider.chat(request).await.expect("chat response");
    override_mock.assert_async().await;
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

    let provider = OpenAIProvider::new("test-key", None)
        .with_chat_url(format!("{}/v1/chat/completions", server.url()));
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

    let provider = OpenAIProvider::new("test-key", None)
        .with_chat_url(format!("{}/v1/chat/completions", server.url()));
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

#[tokio::test]
async fn openai_with_chat_url_trims_trailing_slash() {
    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("POST", "/v1/chat/completions")
        .match_header("authorization", "Bearer test-key")
        .with_status(200)
        .with_body(
            json!({
                "model": "gpt-4o",
                "choices": [{"message": {"content": "ok"}, "finish_reason": "stop"}],
                "usage": {"prompt_tokens": 1, "completion_tokens": 1}
            })
            .to_string(),
        )
        .create_async()
        .await;

    // Trailing slash on the URL is trimmed defensively — should still hit /v1/chat/completions.
    let provider = OpenAIProvider::new("test-key", None)
        .with_chat_url(format!("{}/v1/chat/completions/", server.url()));
    let request = ChatRequest::builder()
        .message(Message::user("hello"))
        .build();

    let response = provider.chat(request).await.expect("chat response");
    assert_eq!(response.content, "ok");
    mock.assert_async().await;
}

#[tokio::test]
async fn openai_chat_falls_back_to_reasoning_content_when_content_empty() {
    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("POST", "/v1/chat/completions")
        .match_header("authorization", "Bearer test-key")
        .with_status(200)
        .with_body(
            json!({
                "model": "gpt-5.3-codex",
                "choices": [{"message": {"content": "", "reasoning_content": "fallback"}, "finish_reason": "stop"}],
                "usage": {"prompt_tokens": 1, "completion_tokens": 1}
            })
            .to_string(),
        )
        .create_async()
        .await;

    let provider = OpenAIProvider::new("test-key", None)
        .with_chat_url(format!("{}/v1/chat/completions", server.url()));
    let request = ChatRequest::builder()
        .message(Message::user("hello"))
        .build();

    let response = provider.chat(request).await.expect("chat response");
    assert_eq!(response.content, "fallback");
    mock.assert_async().await;
}

#[tokio::test]
async fn openai_stream_falls_back_to_reasoning_content_and_skips_empty_chunks() {
    let mut server = mockito::Server::new_async().await;
    let sse_body = concat!(
        "data: {\"choices\":[{\"delta\":{\"content\":\"\"}}]}\n\n",
        "data: {\"choices\":[{\"delta\":{\"reasoning_content\":\"thinking fallback\"}}]}\n\n",
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

    let provider = OpenAIProvider::new("test-key", None)
        .with_chat_url(format!("{}/v1/chat/completions", server.url()));
    let request = ChatRequest::builder()
        .message(Message::user("hello"))
        .build();

    let mut stream = provider.stream(request).await.expect("stream response");
    let mut events = Vec::new();
    while let Some(event) = stream.next().await {
        events.push(event);
    }

    assert_eq!(events.len(), 2);
    assert_eq!(events[0].content, "thinking fallback");
    assert!(events[1].done);
    mock.assert_async().await;
}

#[tokio::test]
async fn openai_stream_maps_structured_error_payload() {
    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("POST", "/v1/chat/completions")
        .match_header("authorization", "Bearer test-key")
        .with_status(401)
        .with_body(json!({"error": {"message": "bad key"}}).to_string())
        .create_async()
        .await;

    let provider = OpenAIProvider::new("test-key", None)
        .with_chat_url(format!("{}/v1/chat/completions", server.url()));
    let request = ChatRequest::builder()
        .message(Message::user("hello"))
        .build();

    let result = provider.stream(request).await;
    assert!(matches!(result, Err(MotosanError::Auth(_))));
    mock.assert_async().await;
}

#[tokio::test]
async fn openai_auth_style_x_api_key_is_supported() {
    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("POST", "/v1/chat/completions")
        .match_header("x-api-key", "test-key")
        .with_status(200)
        .with_body(
            json!({
                "model": "gpt-4o",
                "choices": [{"message": {"content": "ok"}, "finish_reason": "stop"}],
                "usage": {"prompt_tokens": 1, "completion_tokens": 1}
            })
            .to_string(),
        )
        .create_async()
        .await;

    let provider = OpenAIProvider::new("test-key", None)
        .with_chat_url(format!("{}/v1/chat/completions", server.url()))
        .with_auth_style(OpenAIAuthStyle::XApiKey);
    let request = ChatRequest::builder()
        .message(Message::user("hello"))
        .build();

    let response = provider.chat(request).await.expect("chat response");
    assert_eq!(response.content, "ok");
    mock.assert_async().await;
}

#[tokio::test]
async fn openai_chat_can_fallback_to_responses_api_on_404() {
    let mut server = mockito::Server::new_async().await;
    server
        .mock("POST", "/v1/chat/completions")
        .match_header("authorization", "Bearer test-key")
        .with_status(404)
        .with_body(json!({"error": {"message": "not found"}}).to_string())
        .create_async()
        .await;

    let responses_mock = server
        .mock("POST", "/v1/responses")
        .match_header("authorization", "Bearer test-key")
        .with_status(200)
        .with_body(
            json!({
                "model": "gpt-5.3-codex",
                "output_text": "fallback response",
                "usage": {"input_tokens": 5, "output_tokens": 3}
            })
            .to_string(),
        )
        .create_async()
        .await;

    let provider = OpenAIProvider::new("test-key", None)
        .with_chat_url(format!("{}/v1/chat/completions", server.url()))
        .with_responses_url(format!("{}/v1/responses", server.url()))
        .with_responses_fallback(true);
    let request = ChatRequest::builder()
        .message(Message::user("hello"))
        .build();

    let response = provider.chat(request).await.expect("chat response");
    assert_eq!(response.content, "fallback response");
    assert_eq!(response.usage.input_tokens, 5);
    assert_eq!(response.usage.output_tokens, 3);
    responses_mock.assert_async().await;
}

#[tokio::test]
async fn openai_stream_emits_tool_call_events() {
    let mut server = mockito::Server::new_async().await;
    let sse_body = concat!(
        "data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"call_1\",\"function\":{\"name\":\"get_weather\",\"arguments\":\"\"}}]},\"finish_reason\":null}]}\n\n",
        "data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"function\":{\"arguments\":\"{\\\"city\\\":\"}}]},\"finish_reason\":null}]}\n\n",
        "data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"function\":{\"arguments\":\"\\\"Taipei\\\"}\"}}]},\"finish_reason\":null}]}\n\n",
        "data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"tool_calls\"}]}\n\n"
    );

    let mock = server
        .mock("POST", "/v1/chat/completions")
        .match_header("authorization", "Bearer test-key")
        .with_status(200)
        .with_header("content-type", "text/event-stream")
        .with_body(sse_body)
        .create_async()
        .await;

    let provider = OpenAIProvider::new("test-key", None)
        .with_chat_url(format!("{}/v1/chat/completions", server.url()));
    let request = ChatRequest::builder()
        .message(Message::user("weather in Taipei?"))
        .tools(vec![Tool {
            name: "get_weather".to_string(),
            description: Some("Get weather".to_string()),
            input_schema: Some(
                json!({"type": "object", "properties": {"city": {"type": "string"}}}),
            ),
            cache: false,
        }])
        .build();

    let mut stream = provider.stream(request).await.expect("stream response");
    let mut received = Vec::new();
    while let Some(event) = stream.next().await {
        received.push(event);
    }

    // Tool call start
    let starts: Vec<_> = received
        .iter()
        .filter(|e| e.event_type == StreamEventType::ToolCallStart)
        .collect();
    assert_eq!(starts.len(), 1);
    assert_eq!(starts[0].tool_call_id.as_deref(), Some("call_1"));
    assert_eq!(starts[0].tool_call_name.as_deref(), Some("get_weather"));

    // Tool call args
    let args_events: Vec<_> = received
        .iter()
        .filter(|e| e.event_type == StreamEventType::ToolCallArgs)
        .collect();
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
    assert_eq!(ends.len(), 1);

    // Done
    assert!(received.last().unwrap().done);

    mock.assert_async().await;
}

#[tokio::test]
async fn openai_stream_propagates_finish_reason_max_tokens() {
    let mut server = mockito::Server::new_async().await;
    let sse_body = concat!(
        "data: {\"choices\":[{\"delta\":{\"content\":\"truncated\"},\"finish_reason\":null}]}\n\n",
        "data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"length\"}]}\n\n",
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

    let provider = OpenAIProvider::new("test-key", None)
        .with_chat_url(format!("{}/v1/chat/completions", server.url()));
    let request = ChatRequest::builder()
        .message(Message::user("write a long story"))
        .build();

    let mut stream = provider.stream(request).await.expect("stream response");
    let mut events = Vec::new();
    while let Some(event) = stream.next().await {
        events.push(event);
    }

    // Find the first done event and assert it carries MaxTokens.
    let first_done = events
        .iter()
        .find(|e| e.done)
        .expect("expected a terminal done event");
    assert_eq!(first_done.stop_reason, Some(StopReason::MaxTokens));

    mock.assert_async().await;
}

#[tokio::test]
async fn openai_stream_propagates_finish_reason_tool_calls() {
    let mut server = mockito::Server::new_async().await;
    let sse_body = concat!(
        "data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"call_1\",\"function\":{\"name\":\"lookup\",\"arguments\":\"\"}}]},\"finish_reason\":null}]}\n\n",
        "data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"function\":{\"arguments\":\"{}\"}}]},\"finish_reason\":null}]}\n\n",
        "data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"tool_calls\"}]}\n\n",
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

    let provider = OpenAIProvider::new("test-key", None)
        .with_chat_url(format!("{}/v1/chat/completions", server.url()));
    let request = ChatRequest::builder()
        .message(Message::user("call a tool"))
        .build();

    let mut stream = provider.stream(request).await.expect("stream response");
    let mut events = Vec::new();
    while let Some(event) = stream.next().await {
        events.push(event);
    }

    let first_done = events
        .iter()
        .find(|e| e.done)
        .expect("expected a terminal done event");
    assert_eq!(first_done.stop_reason, Some(StopReason::ToolUse));

    mock.assert_async().await;
}

#[tokio::test]
async fn openai_stream_propagates_finish_reason_stop() {
    let mut server = mockito::Server::new_async().await;
    let sse_body = concat!(
        "data: {\"choices\":[{\"delta\":{\"content\":\"done.\"},\"finish_reason\":null}]}\n\n",
        "data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}]}\n\n",
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

    let provider = OpenAIProvider::new("test-key", None)
        .with_chat_url(format!("{}/v1/chat/completions", server.url()));
    let request = ChatRequest::builder().message(Message::user("hi")).build();

    let mut stream = provider.stream(request).await.expect("stream response");
    let mut events = Vec::new();
    while let Some(event) = stream.next().await {
        events.push(event);
    }

    let done = events.iter().find(|e| e.done).expect("done event");
    assert_eq!(done.stop_reason, Some(StopReason::Stop));
    // Exactly one done event, even with [DONE] sentinel present.
    assert_eq!(events.iter().filter(|e| e.done).count(), 1);

    mock.assert_async().await;
}

#[tokio::test]
async fn openai_stream_eof_flush_when_done_sentinel_missing() {
    // Some non-conformant OpenAI-compatible proxies skip the `[DONE]` line.
    // Adapter must still emit a terminal done event with stop_reason from
    // the upstream end-of-stream fallback.
    let mut server = mockito::Server::new_async().await;
    let sse_body = concat!(
        "data: {\"choices\":[{\"delta\":{\"content\":\"hello\"},\"finish_reason\":null}]}\n\n",
        "data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"length\"}]}\n\n" // NOTE: no [DONE] sentinel
    );

    let mock = server
        .mock("POST", "/v1/chat/completions")
        .match_header("authorization", "Bearer test-key")
        .with_status(200)
        .with_header("content-type", "text/event-stream")
        .with_body(sse_body)
        .create_async()
        .await;

    let provider = OpenAIProvider::new("test-key", None)
        .with_chat_url(format!("{}/v1/chat/completions", server.url()));
    let request = ChatRequest::builder()
        .message(Message::user("hello"))
        .build();

    let mut stream = provider.stream(request).await.expect("stream response");
    let mut events = Vec::new();
    while let Some(event) = stream.next().await {
        events.push(event);
    }

    let done = events
        .iter()
        .find(|e| e.done)
        .expect("EOF flush should still emit a done event when [DONE] sentinel is missing");
    assert_eq!(done.stop_reason, Some(StopReason::MaxTokens));
    assert_eq!(events.iter().filter(|e| e.done).count(), 1);

    mock.assert_async().await;
}

#[tokio::test]
async fn openai_stream_emits_done_on_eof_without_finish_reason_or_done_sentinel() {
    // Worst-case non-conformant proxy: stream closes after a text chunk
    // with no `finish_reason` and no `[DONE]`. Adapter must still emit
    // exactly one terminal `done` event so callers don't hang.
    let mut server = mockito::Server::new_async().await;
    // no finish_reason, no [DONE] — exercises the EOF-flush invariant.
    let sse_body = "data: {\"choices\":[{\"delta\":{\"content\":\"hello\"}}]}\n\n";

    let mock = server
        .mock("POST", "/v1/chat/completions")
        .match_header("authorization", "Bearer test-key")
        .with_status(200)
        .with_header("content-type", "text/event-stream")
        .with_body(sse_body)
        .create_async()
        .await;

    let provider = OpenAIProvider::new("test-key", None)
        .with_chat_url(format!("{}/v1/chat/completions", server.url()));
    let request = ChatRequest::builder()
        .message(Message::user("hello"))
        .build();

    let mut stream = provider.stream(request).await.expect("stream response");
    let mut events = Vec::new();
    while let Some(event) = stream.next().await {
        events.push(event);
    }

    let done_count = events.iter().filter(|e| e.done).count();
    assert_eq!(
        done_count, 1,
        "expected exactly one terminal done event, got {done_count}"
    );
    let done = events.iter().find(|e| e.done).unwrap();
    assert_eq!(done.stop_reason, None, "no stop_reason was reported");
    assert_eq!(events[0].content, "hello");

    mock.assert_async().await;
}

#[tokio::test]
async fn openai_stream_done_count_is_exactly_one_when_done_sentinel_present() {
    // Regression test for the historical double-done bug: even with a
    // finish_reason chunk AND the [DONE] sentinel, the stream should emit
    // exactly one terminal done event (not two).
    let mut server = mockito::Server::new_async().await;
    let sse_body = concat!(
        "data: {\"choices\":[{\"delta\":{\"content\":\"ok\"}}]}\n\n",
        "data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}]}\n\n",
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

    let provider = OpenAIProvider::new("test-key", None)
        .with_chat_url(format!("{}/v1/chat/completions", server.url()));
    let request = ChatRequest::builder().message(Message::user("hi")).build();

    let mut stream = provider.stream(request).await.expect("stream response");
    let mut events = Vec::new();
    while let Some(event) = stream.next().await {
        events.push(event);
    }

    let done_count = events.iter().filter(|e| e.done).count();
    assert_eq!(
        done_count, 1,
        "expected exactly one done event, got {done_count}"
    );
    let done = events.iter().find(|e| e.done).unwrap();
    assert_eq!(done.stop_reason, Some(StopReason::Stop));

    mock.assert_async().await;
}
