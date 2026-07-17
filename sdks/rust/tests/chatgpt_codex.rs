//! ChatGptCodexProvider end-to-end test — mockito, no real API needed.
//!
//! Drives a full `ChatGptCodexProvider::stream(...)` against a mock HTTP
//! server that replays the REAL captured ChatGPT-backend SSE stream
//! (`tests/fixtures/chatgpt_codex_sse.txt`), and asserts the streamed text,
//! terminal stop reason, and usage match the fixture.
//!
//! Run: cargo test --features chatgpt-codex --test chatgpt_codex

#![cfg(feature = "chatgpt-codex")]

use mockito::Matcher;
use motosan_ai::providers::chatgpt_codex::ChatGptCodexProvider;
use motosan_ai::providers::ProviderImpl;
use motosan_ai::{ChatRequest, Message, MotosanError, RetryPolicy, StopReason, StreamEventType};
use tokio_stream::StreamExt;

/// The REAL captured SSE stream from the route-B spike (the `event:`/`data:`
/// frames only — the spike's `→ POST ...` / `HTTP 200` annotation lines were
/// stripped so the body is a valid `text/event-stream` payload).
const FIXTURE: &str = include_str!("fixtures/chatgpt_codex_sse.txt");

/// The final assistant text the fixture streams (concatenation of the
/// `response.output_text.delta` frames; also echoed in `output_text.done`).
const EXPECTED_TEXT: &str = "Hi there, friend";

fn no_retry() -> RetryPolicy {
    RetryPolicy::new()
        .max_retries(0)
        .base_delay_ms(0)
        .max_delay_ms(0)
}

#[tokio::test]
async fn codex_stream_eof_without_response_completed_yields_incomplete_stream() {
    let mut server = mockito::Server::new_async().await;
    // Truncated: response.created + one text delta, but the terminal
    // `response.completed` frame never arrives.
    let truncated = concat!(
        "data: {\"type\":\"response.created\",\"response\":{\"id\":\"resp_1\",\"status\":\"in_progress\"}}\n\n",
        "data: {\"type\":\"response.output_text.delta\",\"content_index\":0,\"delta\":\"Hi\",\"item_id\":\"msg_1\",\"output_index\":1}\n\n"
    );
    server
        .mock("POST", Matcher::Any)
        .with_status(200)
        .with_header("content-type", "text/event-stream")
        .with_body(truncated)
        .create_async()
        .await;

    let provider =
        ChatGptCodexProvider::new("oauth-token", "acct-123", "gpt-5.5", Some(server.url()));
    let mut stream = provider
        .stream(
            ChatRequest::builder()
                .messages(vec![Message::user("hi")])
                .build(),
        )
        .await
        .unwrap();

    let mut saw_done = false;
    let mut last_err = None;
    while let Some(item) = stream.next().await {
        match item {
            Ok(ev) => saw_done |= ev.done,
            Err(e) => {
                last_err = Some(e);
                break;
            }
        }
    }
    assert!(!saw_done, "must not fabricate a done event on truncation");
    match last_err.expect("EOF without response.completed must yield an error") {
        MotosanError::IncompleteStream(msg) => {
            assert_eq!(msg, "chatgpt-codex ended without a terminal event")
        }
        other => panic!("expected IncompleteStream, got {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// stream() e2e
// ---------------------------------------------------------------------------

#[tokio::test]
async fn stream_replays_real_fixture() {
    let mut server = mockito::Server::new_async().await;
    server
        .mock("POST", Matcher::Any)
        .with_status(200)
        .with_header("content-type", "text/event-stream")
        .with_body(FIXTURE)
        .create_async()
        .await;

    let provider =
        ChatGptCodexProvider::new("oauth-token", "acct-123", "gpt-5.5", Some(server.url()));
    let stream = provider
        .stream(
            ChatRequest::builder()
                .messages(vec![Message::user("hi")])
                .build(),
        )
        .await
        .unwrap();

    let mut events = Vec::new();
    let mut s = stream;
    while let Some(item) = s.next().await {
        events.push(item.expect("stream item should not fail"));
    }

    // Text deltas concatenate to the fixture's final assistant message.
    let text: String = events
        .iter()
        .filter(|e| e.event_type == StreamEventType::Text)
        .map(|e| e.content.as_str())
        .collect();
    assert_eq!(text, EXPECTED_TEXT);

    // Terminal Done with EndTurn (no tool call in the fixture).
    let done = events.iter().find(|e| e.done).expect("a terminal Done");
    assert_eq!(done.stop_reason, Some(StopReason::EndTurn));

    // Usage from the real `response.completed` frame:
    // input_tokens=22, output_tokens=30, cached_tokens=0.
    let usage = events
        .iter()
        .find(|e| e.event_type == StreamEventType::Usage)
        .and_then(|e| e.usage.clone())
        .expect("a usage event");
    assert_eq!(usage.input_tokens, 22);
    assert_eq!(usage.output_tokens, 30);
    // The fixture's `cached_tokens` is 0 -> None (mirrors gemini's mapping).
    assert_eq!(usage.cache_read_input_tokens, None);
}

#[tokio::test]
async fn chat_collects_real_fixture() {
    let mut server = mockito::Server::new_async().await;
    server
        .mock("POST", Matcher::Any)
        .with_status(200)
        .with_header("content-type", "text/event-stream")
        .with_body(FIXTURE)
        .create_async()
        .await;

    let provider =
        ChatGptCodexProvider::new("oauth-token", "acct-123", "gpt-5.5", Some(server.url()));
    let resp = provider
        .chat(
            ChatRequest::builder()
                .messages(vec![Message::user("hi")])
                .build(),
        )
        .await
        .unwrap();

    assert_eq!(resp.content, EXPECTED_TEXT);
    assert_eq!(resp.stop_reason, StopReason::EndTurn);
    assert_eq!(resp.usage.input_tokens, 22);
    assert_eq!(resp.usage.output_tokens, 30);
    // model falls back to the provider's configured model when the stream omits it.
    assert_eq!(resp.model, "gpt-5.5");
}

#[tokio::test]
async fn stream_sends_codex_auth_headers() {
    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("POST", Matcher::Any)
        .match_header("authorization", "Bearer oauth-token")
        .match_header("chatgpt-account-id", "acct-123")
        .match_header("originator", "codex_cli_rs")
        .with_status(200)
        .with_header("content-type", "text/event-stream")
        .with_body(FIXTURE)
        .create_async()
        .await;

    let provider =
        ChatGptCodexProvider::new("oauth-token", "acct-123", "gpt-5.5", Some(server.url()));
    let stream = provider
        .stream(
            ChatRequest::builder()
                .messages(vec![Message::user("hi")])
                .build(),
        )
        .await
        .unwrap();
    // Drain so the request is actually issued.
    let mut s = stream;
    while let Some(item) = s.next().await {
        item.expect("stream item should not fail");
    }

    mock.assert_async().await;
}

#[tokio::test]
async fn stream_401_returns_auth_error() {
    let mut server = mockito::Server::new_async().await;
    server
        .mock("POST", Matcher::Any)
        .with_status(401)
        .with_body(r#"{"error":{"message":"unauthorized"}}"#)
        .create_async()
        .await;

    let provider =
        ChatGptCodexProvider::new("bad-token", "acct-123", "gpt-5.5", Some(server.url()))
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
    assert!(matches!(err, MotosanError::Auth { .. }), "got {err:?}");
}

#[tokio::test]
async fn stream_fires_on_retry_via_shared_engine() {
    use motosan_ai::retry::RetryCause;
    use std::sync::{Arc, Mutex};

    let mut server = mockito::Server::new_async().await;
    server
        .mock("POST", Matcher::Any)
        .with_status(503)
        .with_body(r#"{"error":{"message":"overloaded"}}"#)
        .expect(1)
        .create_async()
        .await;
    server
        .mock("POST", Matcher::Any)
        .with_status(200)
        .with_header("content-type", "text/event-stream")
        .with_body(FIXTURE)
        .expect(1)
        .create_async()
        .await;

    let seen: Arc<Mutex<Vec<(u32, u16)>>> = Arc::new(Mutex::new(Vec::new()));
    let sink = Arc::clone(&seen);
    let mut policy = RetryPolicy::new()
        .max_retries(1)
        .base_delay_ms(0)
        .max_delay_ms(0)
        .jitter(false);
    policy.on_retry = Some(Arc::new(move |evt| {
        let status = match evt.cause {
            RetryCause::Status(code) => code,
            RetryCause::Network(_) => 0,
        };
        sink.lock().unwrap().push((evt.attempt, status));
    }));

    let provider =
        ChatGptCodexProvider::new("oauth-token", "acct-123", "gpt-5.5", Some(server.url()))
            .with_retry_policy(policy);
    let mut stream = provider
        .stream(
            ChatRequest::builder()
                .messages(vec![Message::user("hi")])
                .build(),
        )
        .await
        .unwrap();
    let mut text = String::new();
    while let Some(item) = stream.next().await {
        let ev = item.expect("stream item should not fail");
        if ev.event_type == StreamEventType::Text {
            text.push_str(&ev.content);
        }
    }
    assert_eq!(text, EXPECTED_TEXT);
    assert_eq!(*seen.lock().unwrap(), vec![(1, 503)]);
}
