#![cfg(feature = "anthropic")]

use motosan_ai::providers::anthropic::AnthropicProvider;
use motosan_ai::providers::ProviderImpl;
use motosan_ai::{ChatRequest, Client, Message, MotosanError, Provider};
use std::time::Duration;
use tokio_stream::StreamExt;

// M3 milestone-Done conformance gates (specs/types.md stream termination).
// Cross-SDK mirrors:
// - sdks/python/tests/test_m3_stream_conformance.py
// - sdks/typescript/tests/m3-stream-conformance.test.ts

#[tokio::test]
async fn anthropic_stream_eof_without_terminal_event_yields_incomplete_stream() {
    let mut server = mockito::Server::new_async().await;
    let sse_body = concat!(
        "event: message_start\n",
        "data: {\"type\":\"message_start\",\"message\":{\"usage\":{\"input_tokens\":3,\"output_tokens\":0}}}\n\n",
        "event: content_block_delta\n",
        "data: {\"type\":\"content_block_delta\",\"delta\":{\"text\":\"partial\"}}\n\n"
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

    let mut stream = provider.stream(request).await.expect("stream response");
    let mut contents = Vec::new();
    let mut terminal_err = None;
    while let Some(item) = stream.next().await {
        match item {
            Ok(event) => {
                assert!(!event.done, "no done event may be fabricated on EOF");
                contents.push(event.content.clone());
            }
            Err(err) => {
                terminal_err = Some(err);
                break;
            }
        }
    }

    assert!(
        contents.iter().any(|t| t == "partial"),
        "text before the drop is still yielded, got {contents:?}"
    );
    let err = terminal_err.expect("EOF without message_stop must yield a typed error");
    assert!(
        matches!(err, MotosanError::IncompleteStream(_)),
        "got {err:?}"
    );
    assert_eq!(
        err.to_string(),
        "incomplete stream: anthropic ended without a terminal event"
    );
    mock.assert_async().await;
}

#[tokio::test]
async fn client_stream_read_idle_timeout_yields_stream_read_timeout() {
    let mut server = mockito::Server::new_async().await;
    server
        .mock("POST", "/v1/messages")
        .with_status(200)
        .with_header("content-type", "text/event-stream")
        .with_chunked_body(|w| {
            w.write_all(b"event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"delta\":{\"text\":\"tick-tick-tick\"}}\n\n")?;
            w.flush()?;
            std::thread::sleep(std::time::Duration::from_secs(2));
            let _ = w.write_all(b"event: message_stop\ndata: {\"type\":\"message_stop\"}\n\n");
            Ok(())
        })
        .create_async()
        .await;

    let client = Client::builder()
        .provider(Provider::Anthropic)
        .api_key("test-key")
        .anthropic_base_url(server.url())
        .read_idle_timeout(Duration::from_millis(250))
        .build()
        .expect("client build");

    let mut stream = client
        .stream(vec![Message::user("hi")])
        .await
        .expect("stream open");

    let mut text = String::new();
    let mut saw_timeout = false;
    let mut saw_done = false;
    while let Some(item) = stream.next().await {
        match item {
            Ok(event) => {
                saw_done |= event.done;
                text.push_str(&event.content);
            }
            Err(MotosanError::StreamReadTimeout(_)) => saw_timeout = true,
            Err(other) => panic!("expected StreamReadTimeout on read-idle expiry, got {other:?}"),
        }
    }
    assert!(
        saw_timeout,
        "idle stall must yield MotosanError::StreamReadTimeout"
    );
    assert!(
        !saw_done,
        "no fabricated terminal done after an idle timeout"
    );
    assert!(
        text.starts_with("tick"),
        "pre-stall content must be delivered, got {text:?}"
    );
}
