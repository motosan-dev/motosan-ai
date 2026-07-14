//! Live OpenAI integration tests — hits real API.
//!
//! Requires `OPENAI_API_KEY` env var. Skips automatically if not set.
//!
//! Run manually:
//!     OPENAI_API_KEY=... cargo test --features openai --test openai_live -- --nocapture

#![cfg(feature = "openai")]

use motosan_ai::{ChatRequest, Client, Message, Provider, StopReason, Tool};
use serde_json::json;
use std::time::Duration;
use tokio_stream::StreamExt;

fn api_key() -> Option<String> {
    match std::env::var("OPENAI_API_KEY") {
        Ok(k) if !k.trim().is_empty() => Some(k),
        _ => None,
    }
}

fn client() -> Option<Client> {
    let key = api_key()?;
    Some(
        Client::builder()
            .provider(Provider::OpenAI)
            .api_key(key)
            .model("gpt-4o-mini")
            .build()
            .expect("client build"),
    )
}

async fn cooldown() {
    tokio::time::sleep(Duration::from_secs(2)).await;
}

#[tokio::test]
async fn live_openai_stream_propagates_max_tokens_stop_reason() {
    let Some(client) = client() else {
        eprintln!("OPENAI_API_KEY not set, skipping");
        return;
    };

    // max_tokens=8 forces OpenAI to truncate → finish_reason = "length"
    let request = ChatRequest::builder()
        .message(Message::user(
            "Write a long detailed essay about the history of the Roman Empire.",
        ))
        .max_tokens(8)
        .build();

    let mut stream = client.stream_with(request).await.expect("stream failed");

    let mut events: Vec<motosan_ai::StreamEvent> = Vec::new();
    while let Some(event_item) = stream.next().await {
        let event = event_item.expect("stream item should not fail");
        events.push(event);
    }

    // Use the FIRST done event — providers may legitimately emit multiple
    // (e.g. finish_reason chunk + `[DONE]` sentinel), and only the first one
    // is guaranteed to carry the authoritative stop_reason.
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
async fn live_openai_collect_stream_records_max_tokens_on_chat_response() {
    let Some(client) = client() else {
        eprintln!("OPENAI_API_KEY not set, skipping");
        return;
    };

    let request = ChatRequest::builder()
        .message(Message::user(
            "Write a long detailed essay about the history of the Roman Empire.",
        ))
        .max_tokens(8)
        .build();

    let stream = client.stream_with(request).await.expect("stream failed");
    let response = motosan_ai::collect_stream(stream).await.unwrap();

    assert_eq!(
        response.stop_reason,
        StopReason::MaxTokens,
        "collect_stream should backfill MaxTokens from terminal done event, got: {:?}",
        response.stop_reason
    );

    cooldown().await;
}

#[tokio::test]
async fn live_openai_parallel_tool_calls_collect_intact() {
    let Some(client) = client() else {
        eprintln!("OPENAI_API_KEY not set, skipping");
        return;
    };

    let tools = vec![
        Tool {
            schema: motosan_agent_primitives::ToolSchema {
                name: "get_weather".to_string(),
                description: "Get current weather for a city.".to_string(),
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "city": {
                            "type": "string",
                            "description": "City name"
                        }
                    },
                    "required": ["city"]
                }),
            },
            cache: false,
        },
        Tool {
            schema: motosan_agent_primitives::ToolSchema {
                name: "get_time".to_string(),
                description: "Get current local time for a timezone.".to_string(),
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "tz": {
                            "type": "string",
                            "description": "IANA timezone name"
                        }
                    },
                    "required": ["tz"]
                }),
            },
            cache: false,
        },
    ];

    let request = ChatRequest::builder()
        .message(Message::user(
            "Call both tools in this turn: get_weather with city Taipei, and get_time with tz Asia/Tokyo.",
        ))
        .tools(tools)
        .tool_choice(motosan_ai::ToolChoice::Required)
        .provider_options(json!({"parallel_tool_calls": true}))
        .build();

    let stream = client.stream_with(request).await.expect("stream failed");
    let response = motosan_ai::collect_stream(stream).await.unwrap();

    assert!(
        !response.tool_calls.is_empty(),
        "expected at least one streamed tool call"
    );
    assert!(
        response
            .tool_calls
            .iter()
            .all(|tool_call| !tool_call.id.is_empty() && !tool_call.name.is_empty()),
        "tool call ids and names should be non-empty: {:?}",
        response.tool_calls
    );
    assert!(
        response
            .tool_calls
            .iter()
            .all(|tool_call| tool_call.input.is_object()),
        "tool call inputs should be JSON objects: {:?}",
        response.tool_calls
    );
    assert_eq!(response.stop_reason, StopReason::ToolUse);

    cooldown().await;
}
