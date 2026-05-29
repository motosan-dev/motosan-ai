//! Live MiniMax integration tests — hits real API.
//!
//! Requires `MINIMAX_API_KEY` env var. Skips automatically if not set.
//!
//! Run manually:
//!     MINIMAX_API_KEY=... cargo test --features minimax --test minimax_live -- --nocapture

#![cfg(feature = "minimax")]

use motosan_ai::{ChatRequest, Client, Message, Provider, StopReason, Tool, ToolChoice};
use serde_json::json;
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
async fn live_minimax_chat_basic_returns_text() {
    let Some(client) = client() else {
        eprintln!("MINIMAX_API_KEY not set, skipping");
        return;
    };

    let response = client
        .chat(vec![Message::user("Reply with exactly: pong")])
        .await
        .expect("chat failed");

    assert!(
        !response.content.trim().is_empty(),
        "expected non-empty content from basic minimax chat"
    );

    cooldown().await;
}

#[tokio::test]
async fn live_minimax_tool_use_roundtrip() {
    let Some(client) = client() else {
        eprintln!("MINIMAX_API_KEY not set, skipping");
        return;
    };

    let tools = vec![Tool {
        schema: motosan_agent_primitives::ToolSchema {
            name: "add".to_string(),
            description: "Add two integers".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "a": {"type": "integer"},
                    "b": {"type": "integer"}
                },
                "required": ["a", "b"]
            }),
        },
        cache: false,
    }];

    let turn1 = client
        .chat_with(
            ChatRequest::builder()
                .message(Message::user("Use the add tool to compute 2 + 3."))
                .tools(tools.clone())
                .tool_choice(ToolChoice::Required)
                .build(),
        )
        .await
        .expect("turn1 failed");

    assert!(
        !turn1.tool_calls.is_empty(),
        "expected tool call in turn1, got stop_reason={:?}, content={:?}",
        turn1.stop_reason,
        turn1.content
    );

    let tc = &turn1.tool_calls[0];
    let a = tc.input.get("a").and_then(|v| v.as_i64()).unwrap_or(0);
    let b = tc.input.get("b").and_then(|v| v.as_i64()).unwrap_or(0);
    let result = (a + b).to_string();

    let turn2_messages = vec![
        Message::user("Use the add tool to compute 2 + 3."),
        Message::assistant_with_tool_calls(turn1.content.clone(), turn1.tool_calls.clone()),
        Message::tool_result(tc.id.clone(), result),
    ];

    let turn2 = client
        .chat_with(
            ChatRequest::builder()
                .messages(turn2_messages)
                .tools(tools)
                .build(),
        )
        .await
        .expect("turn2 failed");

    assert!(
        turn2.content.contains('5') || turn2.content.to_lowercase().contains("five"),
        "expected final answer mentioning 5, got: {:?}",
        turn2.content
    );

    cooldown().await;
}

#[tokio::test]
async fn live_minimax_thinking_blocks_exposed() {
    let Some(client) = client() else {
        eprintln!("MINIMAX_API_KEY not set, skipping");
        return;
    };

    let response = client
        .chat_with(
            ChatRequest::builder()
                .message(Message::user(
                    "Solve 23*47 step by step, then give only the final number on the last line.",
                ))
                .thinking(256)
                .build(),
        )
        .await
        .expect("thinking chat failed");

    assert!(
        response.thinking.as_deref().is_some_and(|s| !s.trim().is_empty()),
        "expected non-empty thinking content when thinking is enabled; got thinking={:?}, content={:?}",
        response.thinking,
        response.content
    );

    cooldown().await;
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
