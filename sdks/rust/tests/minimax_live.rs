//! Live MiniMax integration tests — hits real API.
//!
//! Requires `MINIMAX_API_KEY` env var. Skips automatically if not set.
//!
//! Run manually:
//!     MINIMAX_API_KEY=... cargo test --features minimax --test minimax_live -- --nocapture

#![cfg(feature = "minimax")]

use motosan_ai::{ChatRequest, Client, Message, Provider, StopReason};
use std::time::Duration;
use tokio_stream::StreamExt;

fn api_key() -> Option<String> {
    match std::env::var("MINIMAX_API_KEY") {
        Ok(k) if !k.trim().is_empty() => Some(k),
        _ => None,
    }
}

fn client() -> Option<Client> {
    let key = api_key()?;
    Some(
        Client::builder()
            .provider(Provider::Minimax)
            .api_key(key)
            .build()
            .expect("client build"),
    )
}

async fn cooldown() {
    tokio::time::sleep(Duration::from_secs(2)).await;
}

#[tokio::test]
async fn live_minimax_stream_propagates_max_tokens_stop_reason() {
    let Some(client) = client() else {
        eprintln!("MINIMAX_API_KEY not set, skipping");
        return;
    };

    // max_tokens=8 forces MiniMax to truncate → finish_reason = "length"
    let request = ChatRequest::builder()
        .message(Message::user(
            "Write a long detailed essay about the history of the Roman Empire.",
        ))
        .max_tokens(8)
        .build();

    let mut stream = client.stream_with(request).await.expect("stream failed");

    let mut events: Vec<motosan_ai::StreamEvent> = Vec::new();
    while let Some(event) = stream.next().await {
        events.push(event);
    }

    let done = events
        .iter()
        .find(|e| e.done)
        .expect("expected at least one done event from the stream");
    assert_eq!(
        done.stop_reason,
        Some(StopReason::MaxTokens),
        "expected MaxTokens propagated from finish_reason=length, got: {:?}",
        done.stop_reason
    );

    cooldown().await;
}

#[tokio::test]
async fn live_minimax_collect_stream_records_max_tokens_on_chat_response() {
    let Some(client) = client() else {
        eprintln!("MINIMAX_API_KEY not set, skipping");
        return;
    };

    let request = ChatRequest::builder()
        .message(Message::user(
            "Write a long detailed essay about the history of the Roman Empire.",
        ))
        .max_tokens(8)
        .build();

    let stream = client.stream_with(request).await.expect("stream failed");
    let response = motosan_ai::collect_stream(stream).await;

    assert_eq!(
        response.stop_reason,
        StopReason::MaxTokens,
        "collect_stream should backfill MaxTokens from terminal done event, got: {:?}",
        response.stop_reason
    );

    cooldown().await;
}
