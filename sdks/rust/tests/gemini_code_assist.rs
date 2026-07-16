//! GeminiCodeAssist provider integration tests — mockito, no real API needed.
//!
//! Run: cargo test --features gemini-code-assist --test gemini_code_assist

#![cfg(feature = "gemini-code-assist")]

use mockito::Matcher;
use motosan_ai::providers::gemini_code_assist::GeminiCodeAssistProvider;
use motosan_ai::providers::ProviderImpl;
use motosan_ai::{ChatRequest, Message, MotosanError, RetryPolicy, StopReason, StreamEventType};
use serde_json::json;
use tokio_stream::StreamExt;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn no_retry() -> RetryPolicy {
    RetryPolicy::new()
        .max_retries(0)
        .base_delay_ms(0)
        .max_delay_ms(0)
}

fn fast_retry() -> RetryPolicy {
    RetryPolicy::new()
        .max_retries(1)
        .base_delay_ms(1)
        .max_delay_ms(5)
        .jitter(false)
}

/// SSE line for a text chunk. If `finish` is Some, adds finishReason + usageMetadata.
fn sse_text(text: &str, finish: Option<&str>) -> String {
    let mut candidate = json!({"content": {"parts": [{"text": text}], "role": "model"}});
    if let Some(r) = finish {
        candidate["finishReason"] = json!(r);
    }
    let mut payload = json!({"response": {"candidates": [candidate]}});
    if finish.is_some() {
        payload["response"]["usageMetadata"] =
            json!({"promptTokenCount": 5, "candidatesTokenCount": 3});
    }
    format!("data: {}\n\n", payload)
}

/// SSE line for a function-call chunk.
fn sse_tool(fn_name: &str, args: serde_json::Value, id: Option<&str>) -> String {
    let mut fc = json!({"name": fn_name, "args": args});
    if let Some(id) = id {
        fc["id"] = json!(id);
    }
    let payload = json!({
        "response": {
            "candidates": [{"content": {"parts": [{"functionCall": fc}], "role": "model"}, "finishReason": "STOP"}],
            "usageMetadata": {"promptTokenCount": 6, "candidatesTokenCount": 4}
        }
    });
    format!("data: {}\n\n", payload)
}

// ---------------------------------------------------------------------------
// chat() tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn chat_returns_text() {
    let mut server = mockito::Server::new_async().await;
    let body = format!("{}{}", sse_text("Hello!", None), sse_text("", Some("STOP")));
    server
        .mock("POST", Matcher::Regex("streamGenerateContent".into()))
        .with_status(200)
        .with_header("content-type", "text/event-stream")
        .with_body(body)
        .create_async()
        .await;

    let provider =
        GeminiCodeAssistProvider::new("ya29.fake", "my-project", None, Some(server.url()));
    let resp = provider
        .chat(
            ChatRequest::builder()
                .messages(vec![Message::user("hi")])
                .build(),
        )
        .await
        .unwrap();

    assert_eq!(resp.content, "Hello!");
    assert_eq!(resp.stop_reason, StopReason::EndTurn);
}

#[tokio::test]
async fn chat_sends_bearer_header() {
    let mut server = mockito::Server::new_async().await;
    let body = sse_text("ok", Some("STOP"));
    let mock = server
        .mock("POST", Matcher::Regex("streamGenerateContent".into()))
        .match_header("authorization", "Bearer ya29.fake")
        .with_status(200)
        .with_header("content-type", "text/event-stream")
        .with_body(body)
        .create_async()
        .await;

    let provider =
        GeminiCodeAssistProvider::new("ya29.fake", "my-project", None, Some(server.url()));
    provider
        .chat(
            ChatRequest::builder()
                .messages(vec![Message::user("hi")])
                .build(),
        )
        .await
        .unwrap();

    mock.assert_async().await;
}

#[tokio::test]
async fn chat_sends_project_in_body() {
    let mut server = mockito::Server::new_async().await;
    let body = sse_text("ok", Some("STOP"));
    let mock = server
        .mock("POST", Matcher::Regex("streamGenerateContent".into()))
        .match_body(Matcher::PartialJsonString(
            r#"{"project":"my-project"}"#.into(),
        ))
        .with_status(200)
        .with_header("content-type", "text/event-stream")
        .with_body(body)
        .create_async()
        .await;

    let provider =
        GeminiCodeAssistProvider::new("ya29.fake", "my-project", None, Some(server.url()));
    provider
        .chat(
            ChatRequest::builder()
                .messages(vec![Message::user("hi")])
                .build(),
        )
        .await
        .unwrap();

    mock.assert_async().await;
}

#[tokio::test]
async fn chat_model_in_body_not_url() {
    let mut server = mockito::Server::new_async().await;
    let body = sse_text("ok", Some("STOP"));
    // The URL path should NOT contain the model name
    let mock = server
        .mock("POST", Matcher::Regex("streamGenerateContent".into()))
        .match_body(Matcher::PartialJsonString(
            r#"{"model":"gemini-2.0-flash"}"#.into(),
        ))
        .with_status(200)
        .with_header("content-type", "text/event-stream")
        .with_body(body)
        .create_async()
        .await;

    let provider = GeminiCodeAssistProvider::new(
        "ya29.fake",
        "my-project",
        Some("gemini-2.0-flash".into()),
        Some(server.url()),
    );
    provider
        .chat(
            ChatRequest::builder()
                .messages(vec![Message::user("hi")])
                .build(),
        )
        .await
        .unwrap();

    mock.assert_async().await;

    // Also verify URL does not contain the model
    let url = format!("{}/v1internal:streamGenerateContent?alt=sse", server.url());
    assert!(!url.contains("gemini-2.0-flash"));
}

#[tokio::test]
async fn chat_401_returns_auth_error() {
    let mut server = mockito::Server::new_async().await;
    server
        .mock("POST", Matcher::Regex("streamGenerateContent".into()))
        .with_status(401)
        .with_body(r#"{"error":{"message":"unauthorized"}}"#)
        .create_async()
        .await;

    let provider =
        GeminiCodeAssistProvider::new("bad-token", "my-project", None, Some(server.url()))
            .with_retry_policy(no_retry());
    let err = provider
        .chat(
            ChatRequest::builder()
                .messages(vec![Message::user("hi")])
                .build(),
        )
        .await
        .unwrap_err();

    assert!(
        matches!(err, MotosanError::Auth { .. }),
        "expected Auth, got {err:?}"
    );
}

#[tokio::test]
async fn chat_500_retries_then_succeeds() {
    let mut server = mockito::Server::new_async().await;
    server
        .mock("POST", Matcher::Regex("streamGenerateContent".into()))
        .with_status(500)
        .with_body(r#"{"error":{"message":"internal error"}}"#)
        .expect(1)
        .create_async()
        .await;
    let body = sse_text("recovered", Some("STOP"));
    server
        .mock("POST", Matcher::Regex("streamGenerateContent".into()))
        .with_status(200)
        .with_header("content-type", "text/event-stream")
        .with_body(body)
        .expect(1)
        .create_async()
        .await;

    let provider =
        GeminiCodeAssistProvider::new("ya29.fake", "my-project", None, Some(server.url()))
            .with_retry_policy(fast_retry());
    let resp = provider
        .chat(
            ChatRequest::builder()
                .messages(vec![Message::user("hi")])
                .build(),
        )
        .await
        .unwrap();

    assert_eq!(resp.content, "recovered");
}

#[tokio::test]
async fn chat_with_tool_call_response() {
    let mut server = mockito::Server::new_async().await;
    let body = sse_tool("get_weather", json!({"city": "Tokyo"}), None);
    server
        .mock("POST", Matcher::Regex("streamGenerateContent".into()))
        .with_status(200)
        .with_header("content-type", "text/event-stream")
        .with_body(body)
        .create_async()
        .await;

    let provider =
        GeminiCodeAssistProvider::new("ya29.fake", "my-project", None, Some(server.url()));
    let resp = provider
        .chat(
            ChatRequest::builder()
                .messages(vec![Message::user("weather?")])
                .build(),
        )
        .await
        .unwrap();

    assert_eq!(resp.tool_calls.len(), 1);
    assert_eq!(resp.tool_calls[0].name, "get_weather");
    assert_eq!(resp.tool_calls[0].input["city"], "Tokyo");
    assert!(!resp.tool_calls[0].id.is_empty());
    assert_eq!(resp.stop_reason, StopReason::ToolUse);
}

// ---------------------------------------------------------------------------
// stream() tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn stream_multi_chunk_text() {
    let mut server = mockito::Server::new_async().await;
    let body = format!(
        "{}{}{}",
        sse_text("Hello", None),
        sse_text(", world", None),
        sse_text("!", Some("STOP")),
    );
    server
        .mock("POST", Matcher::Regex("streamGenerateContent".into()))
        .with_status(200)
        .with_header("content-type", "text/event-stream")
        .with_body(body)
        .create_async()
        .await;

    let provider =
        GeminiCodeAssistProvider::new("ya29.fake", "my-project", None, Some(server.url()));
    let stream = provider
        .stream(
            ChatRequest::builder()
                .messages(vec![Message::user("hi")])
                .build(),
        )
        .await
        .unwrap();
    let resp = motosan_ai::stream::collect_stream(stream).await.unwrap();

    assert_eq!(resp.content, "Hello, world!");
    assert_eq!(resp.stop_reason, StopReason::EndTurn);
    assert_eq!(resp.usage.input_tokens, 5);
    assert_eq!(resp.usage.output_tokens, 3);
}

#[tokio::test]
async fn stream_tool_call_with_api_id() {
    let mut server = mockito::Server::new_async().await;
    let body = sse_tool("search", json!({"query": "rust"}), Some("api-id"));
    server
        .mock("POST", Matcher::Regex("streamGenerateContent".into()))
        .with_status(200)
        .with_header("content-type", "text/event-stream")
        .with_body(body)
        .create_async()
        .await;

    let provider =
        GeminiCodeAssistProvider::new("ya29.fake", "my-project", None, Some(server.url()));
    let mut stream = provider
        .stream(
            ChatRequest::builder()
                .messages(vec![Message::user("find")])
                .build(),
        )
        .await
        .unwrap();

    let mut events = vec![];
    while let Some(ev_item) = stream.next().await {
        let ev = ev_item.expect("stream item should not fail");
        events.push(ev);
    }

    let start = events
        .iter()
        .find(|e| e.event_type == StreamEventType::ToolCallStart)
        .expect("missing ToolCallStart");

    assert_eq!(
        start.tool_call_id.as_deref(),
        Some("api-id"),
        "expected id 'api-id', got {:?}",
        start.tool_call_id
    );
}

#[tokio::test]
async fn stream_tool_call_without_id_generates_one() {
    let mut server = mockito::Server::new_async().await;
    // No id in the SSE payload
    let body = sse_tool("my_fn", json!({}), None);
    server
        .mock("POST", Matcher::Regex("streamGenerateContent".into()))
        .with_status(200)
        .with_header("content-type", "text/event-stream")
        .with_body(body)
        .create_async()
        .await;

    let provider =
        GeminiCodeAssistProvider::new("ya29.fake", "my-project", None, Some(server.url()));
    let mut stream = provider
        .stream(
            ChatRequest::builder()
                .messages(vec![Message::user("go")])
                .build(),
        )
        .await
        .unwrap();

    let mut events = vec![];
    while let Some(ev_item) = stream.next().await {
        let ev = ev_item.expect("stream item should not fail");
        events.push(ev);
    }

    let start = events
        .iter()
        .find(|e| e.event_type == StreamEventType::ToolCallStart)
        .expect("missing ToolCallStart");

    let id = start.tool_call_id.as_deref().unwrap_or("");
    assert!(!id.is_empty(), "generated id should not be empty");
    assert!(
        id.starts_with("my_fn"),
        "generated id should start with fn name, got: {id}"
    );
}

#[tokio::test]
async fn stream_max_tokens() {
    let mut server = mockito::Server::new_async().await;
    let body = sse_text("cut off here", Some("MAX_TOKENS"));
    server
        .mock("POST", Matcher::Regex("streamGenerateContent".into()))
        .with_status(200)
        .with_header("content-type", "text/event-stream")
        .with_body(body)
        .create_async()
        .await;

    let provider =
        GeminiCodeAssistProvider::new("ya29.fake", "my-project", None, Some(server.url()));
    let stream = provider
        .stream(
            ChatRequest::builder()
                .messages(vec![Message::user("hi")])
                .build(),
        )
        .await
        .unwrap();
    let resp = motosan_ai::stream::collect_stream(stream).await.unwrap();

    assert_eq!(resp.stop_reason, StopReason::MaxTokens);
}

#[tokio::test]
async fn stream_401_returns_auth_error() {
    let mut server = mockito::Server::new_async().await;
    server
        .mock("POST", Matcher::Regex("streamGenerateContent".into()))
        .with_status(401)
        .with_body(r#"{"error":{"message":"unauthorized"}}"#)
        .create_async()
        .await;

    let provider =
        GeminiCodeAssistProvider::new("bad-token", "my-project", None, Some(server.url()))
            .with_retry_policy(no_retry());
    let result = provider
        .stream(
            ChatRequest::builder()
                .messages(vec![Message::user("hi")])
                .build(),
        )
        .await;
    let err = match result {
        Err(e) => e,
        Ok(_) => panic!("expected error"),
    };
    assert!(matches!(err, MotosanError::Auth { .. }));
}
