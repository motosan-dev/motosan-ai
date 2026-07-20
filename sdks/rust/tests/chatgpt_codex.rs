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
use motosan_ai::auth::{async_trait, TokenSource};
use motosan_ai::providers::chatgpt_codex::ChatGptCodexProvider;
use motosan_ai::providers::ProviderImpl;
use motosan_ai::{
    collect_model_stream, ChatRequest, FreeformTool, FreeformToolFormat, FunctionCallOutputPayload,
    Message, ModelChatRequest, ModelContextItem, ModelToolCall, ModelToolOutput, ModelToolSpec,
    MotosanError, RetryPolicy, StopReason, StreamEventType,
};
use std::sync::Arc;
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

fn freeform_exec_tool() -> ModelToolSpec {
    ModelToolSpec::Freeform(FreeformTool {
        name: "exec".to_string(),
        description: "Run JavaScript".to_string(),
        format: FreeformToolFormat {
            r#type: "grammar".to_string(),
            syntax: "lark".to_string(),
            definition: "start: source".to_string(),
        },
    })
}

fn native_custom_request() -> ModelChatRequest {
    ModelChatRequest::builder()
        .context_item(ModelContextItem::Message(Message::user("run js")))
        .tool_spec(freeform_exec_tool())
        .build()
}

#[tokio::test]
async fn custom_tool_stream_decodes_custom_delta_and_done() {
    let mut server = mockito::Server::new_async().await;
    let sse = concat!(
        "data: {\"type\":\"response.custom_tool_call_input.delta\",\"call_id\":\"call_js\",\"delta\":\"console.\"}\n\n",
        "data: {\"type\":\"response.custom_tool_call_input.delta\",\"call_id\":\"call_js\",\"delta\":\"log(1);\\n\"}\n\n",
        "data: {\"type\":\"response.output_item.done\",\"item\":{\"type\":\"custom_tool_call\",\"call_id\":\"call_js\",\"name\":\"exec\",\"input\":\"console.log(1);\\n\"}}\n\n",
        "data: {\"type\":\"response.completed\",\"response\":{\"status\":\"completed\",\"usage\":{\"input_tokens\":2,\"output_tokens\":3}}}\n\n"
    );
    let mock = server
        .mock("POST", Matcher::Any)
        .with_status(200)
        .with_header("content-type", "text/event-stream")
        .with_body(sse)
        .create_async()
        .await;

    let provider =
        ChatGptCodexProvider::new("oauth-token", "acct-123", "gpt-5.5", Some(server.url()));
    let stream = provider
        .model_stream(native_custom_request())
        .await
        .expect("native stream");
    let response = collect_model_stream(stream)
        .await
        .expect("collect native stream");

    assert_eq!(
        response.tool_calls,
        vec![ModelToolCall::Freeform {
            id: "call_js".to_string(),
            name: "exec".to_string(),
            input: "console.log(1);\n".to_string(),
        }]
    );
    assert_eq!(response.stop_reason, StopReason::ToolUse);
    assert_eq!(response.usage.output_tokens, 3);
    mock.assert_async().await;
}

#[tokio::test]
async fn custom_tool_chat_collects_native_stream() {
    let mut server = mockito::Server::new_async().await;
    let sse = concat!(
        "data: {\"type\":\"response.output_item.done\",\"item\":{\"type\":\"custom_tool_call\",\"call_id\":\"call_js\",\"name\":\"exec\",\"input\":\"text('captured');\"}}\n\n",
        "data: {\"type\":\"response.completed\",\"response\":{\"status\":\"completed\",\"usage\":{\"input_tokens\":4,\"output_tokens\":5}}}\n\n"
    );
    server
        .mock("POST", Matcher::Any)
        .with_status(200)
        .with_header("content-type", "text/event-stream")
        .with_body(sse)
        .create_async()
        .await;

    let provider =
        ChatGptCodexProvider::new("oauth-token", "acct-123", "gpt-5.5", Some(server.url()));
    let response = provider
        .model_chat(native_custom_request())
        .await
        .expect("native chat");

    assert_eq!(
        response.tool_calls,
        vec![ModelToolCall::Freeform {
            id: "call_js".to_string(),
            name: "exec".to_string(),
            input: "text('captured');".to_string(),
        }]
    );
    assert_eq!(response.model, "gpt-5.5");
}

#[tokio::test]
async fn native_stream_maps_response_incomplete_to_max_tokens_done() {
    let mut server = mockito::Server::new_async().await;
    let sse = concat!(
        "data: {\"type\":\"response.output_text.delta\",\"delta\":\"partial\"}\n\n",
        "data: {\"type\":\"response.incomplete\",\"response\":{\"status\":\"incomplete\",\"usage\":{\"input_tokens\":6,\"output_tokens\":7},\"incomplete_details\":{\"reason\":\"max_output_tokens\"}}}\n\n"
    );
    let mock = server
        .mock("POST", Matcher::Any)
        .with_status(200)
        .with_header("content-type", "text/event-stream")
        .with_body(sse)
        .create_async()
        .await;

    let provider =
        ChatGptCodexProvider::new("oauth-token", "acct-123", "gpt-5.5", Some(server.url()));
    let response = provider
        .model_chat(
            ModelChatRequest::builder()
                .context_item(ModelContextItem::Message(Message::user("short")))
                .build(),
        )
        .await
        .expect("native chat");

    assert_eq!(response.content, "partial");
    assert_eq!(response.stop_reason, StopReason::MaxTokens);
    assert_eq!(response.usage.output_tokens, 7);
    mock.assert_async().await;
}

#[tokio::test]
async fn custom_tool_request_sends_custom_tool_and_history_byte_exact() {
    let mut server = mockito::Server::new_async().await;
    let raw = "{\"this\":\"looks like json\"}\nconsole.log('but is JS');";
    let mock = server
        .mock("POST", Matcher::Any)
        .match_body(Matcher::AllOf(vec![
            Matcher::Regex(r#""type"\s*:\s*"custom""#.to_string()),
            Matcher::Regex(r#""type"\s*:\s*"custom_tool_call""#.to_string()),
            Matcher::Regex(r#""type"\s*:\s*"custom_tool_call_output""#.to_string()),
            Matcher::Regex(r#""input"\s*:\s*"\{\\"this\\":\\"looks like json\\"\}\\nconsole\.log\('but is JS'\);"#.to_string()),
        ]))
        .with_status(200)
        .with_header("content-type", "text/event-stream")
        .with_body(
            "data: {\"type\":\"response.completed\",\"response\":{\"status\":\"completed\",\"usage\":{\"input_tokens\":1,\"output_tokens\":1}}}\n\n",
        )
        .create_async()
        .await;

    let provider =
        ChatGptCodexProvider::new("oauth-token", "acct-123", "gpt-5.5", Some(server.url()));
    let request = ModelChatRequest::builder()
        .context_item(ModelContextItem::Message(Message::user("run js")))
        .context_item(ModelContextItem::ToolCall(ModelToolCall::Freeform {
            id: "call_js".to_string(),
            name: "exec".to_string(),
            input: raw.to_string(),
        }))
        .context_item(ModelContextItem::ToolOutput(ModelToolOutput::Custom {
            call_id: "call_js".to_string(),
            name: Some("exec".to_string()),
            output: FunctionCallOutputPayload::Text("done".to_string()),
        }))
        .tool_spec(freeform_exec_tool())
        .build();

    let response = provider.model_chat(request).await.expect("native chat");

    assert_eq!(response.stop_reason, StopReason::EndTurn);
    mock.assert_async().await;
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

// ---------------------------------------------------------------------------
// F5: per-attempt TokenSource resolution
// ---------------------------------------------------------------------------

#[derive(Default)]
struct SequenceTokenSource {
    calls: std::sync::atomic::AtomicUsize,
}

impl std::fmt::Debug for SequenceTokenSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SequenceTokenSource")
            .finish_non_exhaustive()
    }
}

#[async_trait]
impl TokenSource for SequenceTokenSource {
    async fn access_token(&self) -> Result<String, MotosanError> {
        let n = self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Ok(format!("tok-{}", n + 1))
    }
}

#[tokio::test]
async fn token_source_is_consulted_once_per_attempt() {
    let mut server = mockito::Server::new_async().await;
    // Attempt 1 must carry the token minted for it (tok-1) and gets a
    // retryable 500; the mocks are disambiguated by the auth header, so a
    // stale-token second attempt would match NEITHER mock and fail loudly.
    let first = server
        .mock("POST", Matcher::Any)
        .match_header("authorization", "Bearer tok-1")
        .with_status(500)
        .with_body(r#"{"error":{"message":"overloaded"}}"#)
        .expect(1)
        .create_async()
        .await;
    // Attempt 2 must re-resolve and carry tok-2.
    let second = server
        .mock("POST", Matcher::Any)
        .match_header("authorization", "Bearer tok-2")
        .with_status(200)
        .with_header("content-type", "text/event-stream")
        .with_body(FIXTURE)
        .expect(1)
        .create_async()
        .await;

    let source = Arc::new(SequenceTokenSource::default());
    let provider =
        ChatGptCodexProvider::new("ignored-static", "acct-123", "gpt-5.5", Some(server.url()))
            .with_retry_policy(
                RetryPolicy::new()
                    .max_retries(1)
                    .base_delay_ms(0)
                    .max_delay_ms(0)
                    .jitter(false),
            )
            .with_token_source(source.clone());

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
    assert_eq!(
        source.calls.load(std::sync::atomic::Ordering::SeqCst),
        2,
        "token source must be consulted exactly once per attempt"
    );
    first.assert_async().await;
    second.assert_async().await;
    eprintln!(
        "500-then-200: token source calls=2; attempt 1 used Bearer tok-1; \
         attempt 2 used refreshed Bearer tok-2"
    );
}

// ---------------------------------------------------------------------------
// F5: ClientBuilder::chatgpt_codex_token_source
// ---------------------------------------------------------------------------

#[derive(Debug)]
struct SentinelSource;

#[async_trait]
impl TokenSource for SentinelSource {
    async fn access_token(&self) -> Result<String, MotosanError> {
        Err(MotosanError::Auth {
            message: "sentinel token source consulted".to_string(),
            status_code: None,
            retry_after: None,
            request_id: None,
        })
    }
}

#[tokio::test]
async fn builder_token_source_wins_over_static_access_token() {
    // The builder-made provider always targets the real chatgpt.com URL
    // (client.rs passes base_url: None), so observe the seam via a sentinel
    // source that errors BEFORE any network I/O: if the static token had
    // won, chat() would have attempted a real HTTP call instead.
    let client = motosan_ai::Client::builder()
        .provider(motosan_ai::Provider::OpenAiChatGpt)
        .chatgpt_codex("static-token-should-lose", "acct-123", "gpt-5.5")
        .chatgpt_codex_token_source(Arc::new(SentinelSource))
        .build()
        .expect("build succeeds");

    let err = client
        .chat(vec![Message::user("hi")])
        .await
        .expect_err("sentinel source fails the attempt before any I/O");
    assert!(
        matches!(err, MotosanError::Auth { ref message, .. }
            if message == "sentinel token source consulted"),
        "got {err:?}"
    );
}

#[tokio::test]
async fn builder_token_source_alone_is_sufficient() {
    // Pins the access-token waiver: no chatgpt_codex(access_token, ...) call
    // at all. (Verified: build() never required it — api_key is waived for
    // Provider::OpenAiChatGpt at client.rs:1006-1013 and the static token
    // defaults to "" — so this is a pin, not a behavior change.)
    let client = motosan_ai::Client::builder()
        .provider(motosan_ai::Provider::OpenAiChatGpt)
        .model("gpt-5.5")
        .chatgpt_codex_token_source(Arc::new(SentinelSource))
        .build()
        .expect("token_source alone must build");

    let err = client
        .chat(vec![Message::user("hi")])
        .await
        .expect_err("sentinel source fails the attempt before any I/O");
    assert!(matches!(err, MotosanError::Auth { .. }), "got {err:?}");
}
