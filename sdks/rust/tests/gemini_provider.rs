//! Gemini HTTP provider integration tests — uses mockito, no real API key needed.
//!
//! Run:
//!     cargo test --features gemini --test gemini_provider

#![cfg(feature = "gemini")]

use mockito::Matcher;
use motosan_ai::providers::gemini::GeminiProvider;
use motosan_ai::providers::ProviderImpl;
use motosan_ai::{
    ChatRequest, Message, MotosanError, RetryPolicy, StopReason, StreamEventType, Tool, ToolCall,
};
use serde_json::json;
use tokio_stream::StreamExt;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn fast_retry() -> RetryPolicy {
    RetryPolicy::new()
        .max_retries(2)
        .base_delay_ms(1)
        .max_delay_ms(5)
        .jitter(false)
        .respect_retry_after(false)
}

fn no_retry() -> RetryPolicy {
    RetryPolicy::new()
        .max_retries(0)
        .base_delay_ms(0)
        .max_delay_ms(0)
}

fn ok_body(text: &str) -> String {
    json!({
        "candidates": [{
            "content": {"parts": [{"text": text}], "role": "model"},
            "finishReason": "STOP"
        }],
        "usageMetadata": {"promptTokenCount": 5, "candidatesTokenCount": 2},
        "modelVersion": "gemini-2.0-flash"
    })
    .to_string()
}

fn tool_body(fn_name: &str, args: serde_json::Value) -> String {
    json!({
        "candidates": [{
            "content": {
                "parts": [{"functionCall": {"name": fn_name, "args": args}}],
                "role": "model"
            },
            "finishReason": "STOP"
        }],
        "usageMetadata": {"promptTokenCount": 8, "candidatesTokenCount": 3},
        "modelVersion": "gemini-2.0-flash"
    })
    .to_string()
}

// ---------------------------------------------------------------------------
// chat() — success paths
// ---------------------------------------------------------------------------

#[tokio::test]
async fn chat_returns_text_content() {
    let mut server = mockito::Server::new_async().await;
    server
        .mock("POST", Matcher::Regex("generateContent".into()))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(ok_body("Hello from Gemini"))
        .create_async()
        .await;

    let provider = GeminiProvider::new("key", None, Some(server.url()));
    let resp = provider
        .chat(
            ChatRequest::builder()
                .messages(vec![Message::user("hi")])
                .build(),
        )
        .await
        .unwrap();

    assert_eq!(resp.content, "Hello from Gemini");
    assert_eq!(resp.stop_reason, StopReason::EndTurn);
    assert_eq!(resp.usage.input_tokens, 5);
    assert_eq!(resp.usage.output_tokens, 2);
}

#[tokio::test]
async fn chat_model_override_in_url() {
    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("POST", Matcher::Regex("gemini-1.5-pro".into()))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(ok_body("ok"))
        .create_async()
        .await;

    let provider = GeminiProvider::new("key", None, Some(server.url()));
    let req = ChatRequest::builder()
        .messages(vec![Message::user("hi")])
        .model("gemini-1.5-pro")
        .build();
    provider.chat(req).await.unwrap();
    mock.assert_async().await;
}

#[tokio::test]
async fn chat_sends_system_instruction() {
    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("POST", Matcher::Regex("generateContent".into()))
        .match_body(Matcher::PartialJsonString(
            r#"{"systemInstruction":{"parts":[{"text":"Be terse."}]}}"#.into(),
        ))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(ok_body("ok"))
        .create_async()
        .await;

    let provider = GeminiProvider::new("key", None, Some(server.url()));
    let req = ChatRequest::builder()
        .messages(vec![Message::user("hi")])
        .system("Be terse.")
        .build();
    provider.chat(req).await.unwrap();
    mock.assert_async().await;
}

#[tokio::test]
async fn chat_sends_tool_declarations() {
    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("POST", Matcher::Regex("generateContent".into()))
        .match_body(Matcher::PartialJsonString(
            r#"{"tools":[{"functionDeclarations":[{"name":"search"}]}]}"#.into(),
        ))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(ok_body("ok"))
        .create_async()
        .await;

    let tool = Tool {
        name: "search".into(),
        description: None,
        input_schema: None,
        cache: false,
    };
    let provider = GeminiProvider::new("key", None, Some(server.url()));
    let req = ChatRequest::builder()
        .messages(vec![Message::user("find")])
        .tools(vec![tool])
        .build();
    provider.chat(req).await.unwrap();
    mock.assert_async().await;
}

#[tokio::test]
async fn chat_returns_tool_call() {
    let mut server = mockito::Server::new_async().await;
    server
        .mock("POST", Matcher::Regex("generateContent".into()))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(tool_body("get_weather", json!({"city": "Tokyo"})))
        .create_async()
        .await;

    let provider = GeminiProvider::new("key", None, Some(server.url()));
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

#[tokio::test]
async fn chat_multi_turn_conversation() {
    let mut server = mockito::Server::new_async().await;
    server
        .mock("POST", Matcher::Regex("generateContent".into()))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(ok_body("pong"))
        .create_async()
        .await;

    let provider = GeminiProvider::new("key", None, Some(server.url()));
    let tool_id = "fn_1";
    let msgs = vec![
        Message::user("ping"),
        Message::assistant_with_tool_calls(
            "",
            vec![ToolCall {
                id: tool_id.into(),
                name: "ping_fn".into(),
                input: json!({}),
            }],
        ),
        Message::tool_result(tool_id, r#"{"ok":true}"#),
        Message::user("continue"),
    ];
    let resp = provider
        .chat(ChatRequest::builder().messages(msgs).build())
        .await
        .unwrap();
    assert_eq!(resp.content, "pong");
}

// ---------------------------------------------------------------------------
// chat() — error paths
// ---------------------------------------------------------------------------

#[tokio::test]
async fn chat_401_returns_auth_error() {
    let mut server = mockito::Server::new_async().await;
    server
        .mock("POST", Matcher::Regex("generateContent".into()))
        .with_status(401)
        .with_body(r#"{"error":{"message":"API key invalid"}}"#)
        .create_async()
        .await;

    let provider =
        GeminiProvider::new("bad-key", None, Some(server.url())).with_retry_policy(no_retry());
    let err = provider
        .chat(
            ChatRequest::builder()
                .messages(vec![Message::user("hi")])
                .build(),
        )
        .await
        .unwrap_err();
    assert!(
        matches!(err, MotosanError::Auth(_)),
        "expected Auth, got {err:?}"
    );
}

#[tokio::test]
async fn chat_400_returns_invalid_request() {
    let mut server = mockito::Server::new_async().await;
    server
        .mock("POST", Matcher::Regex("generateContent".into()))
        .with_status(400)
        .with_body(r#"{"error":{"message":"bad request"}}"#)
        .create_async()
        .await;

    let provider =
        GeminiProvider::new("key", None, Some(server.url())).with_retry_policy(no_retry());
    let err = provider
        .chat(
            ChatRequest::builder()
                .messages(vec![Message::user("hi")])
                .build(),
        )
        .await
        .unwrap_err();
    assert!(
        matches!(err, MotosanError::InvalidRequest(_)),
        "expected InvalidRequest, got {err:?}"
    );
}

#[tokio::test]
async fn chat_500_returns_provider_error() {
    let mut server = mockito::Server::new_async().await;
    server
        .mock("POST", Matcher::Regex("generateContent".into()))
        .with_status(500)
        .with_body(r#"{"error":{"message":"internal error"}}"#)
        .create_async()
        .await;

    let provider =
        GeminiProvider::new("key", None, Some(server.url())).with_retry_policy(no_retry());
    let err = provider
        .chat(
            ChatRequest::builder()
                .messages(vec![Message::user("hi")])
                .build(),
        )
        .await
        .unwrap_err();
    assert!(
        matches!(err, MotosanError::ProviderError(_)),
        "expected ProviderError, got {err:?}"
    );
}

#[tokio::test]
async fn chat_retries_429_then_succeeds() {
    let mut server = mockito::Server::new_async().await;
    server
        .mock("POST", Matcher::Regex("generateContent".into()))
        .with_status(429)
        .with_body(r#"{"error":{"message":"rate limited"}}"#)
        .expect(1)
        .create_async()
        .await;
    server
        .mock("POST", Matcher::Regex("generateContent".into()))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(ok_body("recovered"))
        .expect(1)
        .create_async()
        .await;

    let provider =
        GeminiProvider::new("key", None, Some(server.url())).with_retry_policy(fast_retry());
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
async fn chat_retries_500_then_succeeds() {
    let mut server = mockito::Server::new_async().await;
    server
        .mock("POST", Matcher::Regex("generateContent".into()))
        .with_status(500)
        .with_body(r#"{"error":{"message":"oops"}}"#)
        .expect(1)
        .create_async()
        .await;
    server
        .mock("POST", Matcher::Regex("generateContent".into()))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(ok_body("ok"))
        .expect(1)
        .create_async()
        .await;

    let provider =
        GeminiProvider::new("key", None, Some(server.url())).with_retry_policy(fast_retry());
    let resp = provider
        .chat(
            ChatRequest::builder()
                .messages(vec![Message::user("hi")])
                .build(),
        )
        .await
        .unwrap();
    assert_eq!(resp.content, "ok");
}

#[tokio::test]
async fn chat_exhausts_retries_on_persistent_500() {
    let mut server = mockito::Server::new_async().await;
    // max_retries=1 means 1 initial + 1 retry = 2 total requests
    server
        .mock("POST", Matcher::Regex("generateContent".into()))
        .with_status(500)
        .with_body(r#"{"error":{"message":"still failing"}}"#)
        .expect(2)
        .create_async()
        .await;

    let policy = RetryPolicy::new()
        .max_retries(1)
        .base_delay_ms(1)
        .max_delay_ms(5)
        .jitter(false);
    let provider = GeminiProvider::new("key", None, Some(server.url())).with_retry_policy(policy);
    let err = provider
        .chat(
            ChatRequest::builder()
                .messages(vec![Message::user("hi")])
                .build(),
        )
        .await
        .unwrap_err();
    assert!(matches!(err, MotosanError::ProviderError(_)));
}

// ---------------------------------------------------------------------------
// stream() — success paths
// ---------------------------------------------------------------------------

fn sse_text(text: &str, finish: Option<&str>) -> String {
    let mut candidate = json!({"content": {"parts": [{"text": text}], "role": "model"}});
    if let Some(reason) = finish {
        candidate["finishReason"] = json!(reason);
    }
    let mut payload = json!({"candidates": [candidate]});
    if finish.is_some() {
        payload["usageMetadata"] = json!({"promptTokenCount": 4, "candidatesTokenCount": 3});
    }
    format!("data: {}\n\n", payload)
}

fn sse_tool_call(fn_name: &str, args: serde_json::Value) -> String {
    let payload = json!({
        "candidates": [{
            "content": {
                "parts": [{"functionCall": {"name": fn_name, "args": args}}],
                "role": "model"
            },
            "finishReason": "STOP"
        }],
        "usageMetadata": {"promptTokenCount": 6, "candidatesTokenCount": 4}
    });
    format!("data: {}\n\n", payload)
}

#[tokio::test]
async fn stream_collects_multi_chunk_text() {
    let mut server = mockito::Server::new_async().await;
    let body = format!(
        "{}{}{}",
        sse_text("Hello", None),
        sse_text(", ", None),
        sse_text("world!", Some("STOP")),
    );
    server
        .mock("POST", Matcher::Regex("streamGenerateContent".into()))
        .with_status(200)
        .with_header("content-type", "text/event-stream")
        .with_body(body)
        .create_async()
        .await;

    let provider = GeminiProvider::new("key", None, Some(server.url()));
    let stream = provider
        .stream(
            ChatRequest::builder()
                .messages(vec![Message::user("hi")])
                .build(),
        )
        .await
        .unwrap();
    let resp = motosan_ai::stream::collect_stream(stream).await;
    assert_eq!(resp.content, "Hello, world!");
    assert_eq!(resp.stop_reason, StopReason::EndTurn);
    assert_eq!(resp.usage.input_tokens, 4);
    assert_eq!(resp.usage.output_tokens, 3);
}

#[tokio::test]
async fn stream_emits_correct_event_types() {
    let mut server = mockito::Server::new_async().await;
    let body = format!(
        "{}{}",
        sse_text("chunk1", None),
        sse_text("chunk2", Some("STOP")),
    );
    server
        .mock("POST", Matcher::Regex("streamGenerateContent".into()))
        .with_status(200)
        .with_header("content-type", "text/event-stream")
        .with_body(body)
        .create_async()
        .await;

    let provider = GeminiProvider::new("key", None, Some(server.url()));
    let mut stream = provider
        .stream(
            ChatRequest::builder()
                .messages(vec![Message::user("hi")])
                .build(),
        )
        .await
        .unwrap();

    let mut event_types = vec![];
    while let Some(ev) = stream.next().await {
        event_types.push(ev.event_type.clone());
    }

    assert!(event_types.contains(&StreamEventType::Text));
    assert!(event_types.contains(&StreamEventType::Usage));
}

#[tokio::test]
async fn stream_with_tool_call_events() {
    let mut server = mockito::Server::new_async().await;
    let body = sse_tool_call("search", json!({"query": "rust lang"}));
    server
        .mock("POST", Matcher::Regex("streamGenerateContent".into()))
        .with_status(200)
        .with_header("content-type", "text/event-stream")
        .with_body(body)
        .create_async()
        .await;

    let provider = GeminiProvider::new("key", None, Some(server.url()));
    let mut stream = provider
        .stream(
            ChatRequest::builder()
                .messages(vec![Message::user("find rust")])
                .build(),
        )
        .await
        .unwrap();

    let mut events = vec![];
    while let Some(ev) = stream.next().await {
        events.push(ev);
    }

    let start = events
        .iter()
        .find(|e| e.event_type == StreamEventType::ToolCallStart);
    let args = events
        .iter()
        .find(|e| e.event_type == StreamEventType::ToolCallArgs);
    let end = events
        .iter()
        .find(|e| e.event_type == StreamEventType::ToolCallEnd);
    assert!(start.is_some(), "missing ToolCallStart");
    assert!(args.is_some(), "missing ToolCallArgs");
    assert!(end.is_some(), "missing ToolCallEnd");

    // args delta should contain the function arguments JSON
    let args_delta = args.unwrap().tool_call_args_delta.as_deref().unwrap_or("");
    assert!(args_delta.contains("rust lang"), "args delta: {args_delta}");
}

#[tokio::test]
async fn stream_tool_call_collect_produces_tool_use_stop_reason() {
    let mut server = mockito::Server::new_async().await;
    server
        .mock("POST", Matcher::Regex("streamGenerateContent".into()))
        .with_status(200)
        .with_header("content-type", "text/event-stream")
        .with_body(sse_tool_call("foo", json!({})))
        .create_async()
        .await;

    let provider = GeminiProvider::new("key", None, Some(server.url()));
    let stream = provider
        .stream(
            ChatRequest::builder()
                .messages(vec![Message::user("go")])
                .build(),
        )
        .await
        .unwrap();
    let resp = motosan_ai::stream::collect_stream(stream).await;
    assert_eq!(resp.stop_reason, StopReason::ToolUse);
    assert_eq!(resp.tool_calls.len(), 1);
    assert_eq!(resp.tool_calls[0].name, "foo");
}

#[tokio::test]
async fn stream_max_tokens_stop_reason() {
    let mut server = mockito::Server::new_async().await;
    let body = sse_text("cut off", Some("MAX_TOKENS"));
    server
        .mock("POST", Matcher::Regex("streamGenerateContent".into()))
        .with_status(200)
        .with_header("content-type", "text/event-stream")
        .with_body(body)
        .create_async()
        .await;

    let provider = GeminiProvider::new("key", None, Some(server.url()));
    let stream = provider
        .stream(
            ChatRequest::builder()
                .messages(vec![Message::user("hi")])
                .build(),
        )
        .await
        .unwrap();
    let resp = motosan_ai::stream::collect_stream(stream).await;
    assert_eq!(resp.stop_reason, StopReason::MaxTokens);
}

// ---------------------------------------------------------------------------
// stream() — error paths
// ---------------------------------------------------------------------------

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
        GeminiProvider::new("bad", None, Some(server.url())).with_retry_policy(no_retry());
    let result = provider
        .stream(
            ChatRequest::builder()
                .messages(vec![Message::user("hi")])
                .build(),
        )
        .await;
    let err = match result {
        Err(e) => e,
        Ok(_) => panic!("expected Auth error, got Ok"),
    };
    assert!(
        matches!(err, MotosanError::Auth(_)),
        "expected Auth, got {err:?}"
    );
}

#[tokio::test]
async fn stream_retries_500_then_succeeds() {
    let mut server = mockito::Server::new_async().await;
    server
        .mock("POST", Matcher::Regex("streamGenerateContent".into()))
        .with_status(500)
        .with_body(r#"{"error":{"message":"oops"}}"#)
        .expect(1)
        .create_async()
        .await;
    let body = format!("{}{}", sse_text("Hi", None), sse_text("!", Some("STOP")),);
    server
        .mock("POST", Matcher::Regex("streamGenerateContent".into()))
        .with_status(200)
        .with_header("content-type", "text/event-stream")
        .with_body(body)
        .expect(1)
        .create_async()
        .await;

    let provider =
        GeminiProvider::new("key", None, Some(server.url())).with_retry_policy(fast_retry());
    let stream = provider
        .stream(
            ChatRequest::builder()
                .messages(vec![Message::user("hi")])
                .build(),
        )
        .await
        .unwrap();
    let resp = motosan_ai::stream::collect_stream(stream).await;
    assert_eq!(resp.content, "Hi!");
}

// ---------------------------------------------------------------------------
// OAuth authentication
// ---------------------------------------------------------------------------

#[tokio::test]
async fn chat_oauth_token_sends_bearer_header() {
    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("POST", Matcher::Regex("generateContent".into()))
        .match_header("authorization", "Bearer ya29.fake-token")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(ok_body("hi from oauth"))
        .create_async()
        .await;

    let provider = GeminiProvider::new("ya29.fake-token", None, Some(server.url()));
    let resp = provider
        .chat(
            ChatRequest::builder()
                .messages(vec![Message::user("hello")])
                .build(),
        )
        .await
        .unwrap();

    assert_eq!(resp.content, "hi from oauth");
    mock.assert_async().await;
}

#[tokio::test]
async fn chat_oauth_token_omits_key_query_param() {
    let mut server = mockito::Server::new_async().await;
    // URL must NOT contain ?key=
    let mock = server
        .mock(
            "POST",
            Matcher::Regex(r"^/models/gemini-2\.0-flash:generateContent$".into()),
        )
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(ok_body("no key in url"))
        .create_async()
        .await;

    let provider = GeminiProvider::new("ya29.fake-token", None, Some(server.url()));
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
async fn stream_oauth_token_sends_bearer_header() {
    let mut server = mockito::Server::new_async().await;
    let body = format!("{}{}", sse_text("hello", None), sse_text("!", Some("STOP")),);
    let mock = server
        .mock("POST", Matcher::Regex("streamGenerateContent".into()))
        .match_header("authorization", "Bearer ya29.fake-token")
        .with_status(200)
        .with_header("content-type", "text/event-stream")
        .with_body(body)
        .create_async()
        .await;

    let provider = GeminiProvider::new("ya29.fake-token", None, Some(server.url()));
    let stream = provider
        .stream(
            ChatRequest::builder()
                .messages(vec![Message::user("hi")])
                .build(),
        )
        .await
        .unwrap();
    let resp = motosan_ai::stream::collect_stream(stream).await;
    assert_eq!(resp.content, "hello!");
    mock.assert_async().await;
}
