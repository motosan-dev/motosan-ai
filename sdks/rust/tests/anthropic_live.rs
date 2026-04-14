//! Live Anthropic integration tests — hits real API with OAuth token.
//!
//! Requires `ANTHROPIC_API_KEY` env var (supports `sk-ant-oat01-*` OAuth tokens).
//! Skips automatically if not set.
//!
//! Run manually:
//!     ANTHROPIC_API_KEY=... cargo test --features anthropic --test anthropic_live -- --nocapture

#![cfg(feature = "anthropic")]

use motosan_ai::{ChatRequest, Client, Message, Provider, StopReason, StreamEventType, Tool};
use serde_json::json;
use std::time::Duration;
use tokio_stream::StreamExt;

fn api_key() -> Option<String> {
    match std::env::var("ANTHROPIC_API_KEY") {
        Ok(k) if !k.trim().is_empty() => Some(k),
        _ => None,
    }
}

fn client() -> Option<Client> {
    let key = api_key()?;
    Some(
        Client::builder()
            .provider(Provider::Anthropic)
            .api_key(key)
            .model("claude-sonnet-4-6")
            .build()
            .expect("client build"),
    )
}

async fn cooldown() {
    tokio::time::sleep(Duration::from_secs(3)).await;
}

// ---------------------------------------------------------------------------
// 1. chat — basic
// ---------------------------------------------------------------------------

#[tokio::test]
async fn live_chat_basic() {
    let Some(client) = client() else {
        eprintln!("ANTHROPIC_API_KEY not set, skipping");
        return;
    };

    let resp = client
        .chat(vec![Message::user("Reply with exactly one word: PONG")])
        .await
        .expect("chat failed");

    assert!(
        resp.content.contains("PONG"),
        "Expected PONG, got: {:?}",
        resp.content
    );
    assert!(!resp.model.is_empty());
    cooldown().await;
}

// ---------------------------------------------------------------------------
// 2. stream — basic
// ---------------------------------------------------------------------------

#[tokio::test]
async fn live_stream_basic() {
    let Some(client) = client() else {
        eprintln!("ANTHROPIC_API_KEY not set, skipping");
        return;
    };

    let mut stream = client
        .stream(vec![Message::user("Reply with exactly: STREAM_OK")])
        .await
        .expect("stream failed");

    let mut chunks = Vec::new();
    while let Some(event) = stream.next().await {
        if !event.content.is_empty() {
            chunks.push(event.content.clone());
        }
        if event.done {
            break;
        }
    }
    // Drain remaining after done (ThinkStripper may have buffered tail)
    while let Some(event) = stream.next().await {
        if !event.content.is_empty() {
            chunks.push(event.content.clone());
        }
    }
    let full: String = chunks.join("");
    assert!(
        full.contains("STREAM_OK"),
        "Expected STREAM_OK, got: {:?}",
        full
    );
    cooldown().await;
}

// ---------------------------------------------------------------------------
// 3. system prompt
// ---------------------------------------------------------------------------

#[tokio::test]
async fn live_system_prompt() {
    let Some(client) = client() else {
        eprintln!("ANTHROPIC_API_KEY not set, skipping");
        return;
    };

    let request = ChatRequest::builder()
        .message(Message::user("What is your name? Reply in one sentence."))
        .system("Your name is TestBot. Always introduce yourself as TestBot.")
        .build();

    let resp = client.chat_with(request).await.expect("chat failed");
    assert!(
        resp.content.contains("TestBot"),
        "Expected TestBot, got: {:?}",
        resp.content
    );
    cooldown().await;
}

// ---------------------------------------------------------------------------
// 4. temperature
// ---------------------------------------------------------------------------

#[tokio::test]
async fn live_temperature() {
    let Some(client) = client() else {
        eprintln!("ANTHROPIC_API_KEY not set, skipping");
        return;
    };

    let request = ChatRequest::builder()
        .message(Message::user("Reply with exactly one word: TEMP_OK"))
        .temperature(0.0)
        .build();

    let resp = client.chat_with(request).await.expect("chat failed");
    assert!(
        resp.content.contains("TEMP_OK"),
        "Expected TEMP_OK, got: {:?}",
        resp.content
    );
    cooldown().await;
}

// ---------------------------------------------------------------------------
// 5. tool use — single turn
// ---------------------------------------------------------------------------

#[tokio::test]
async fn live_tool_use_single_turn() {
    let Some(client) = client() else {
        eprintln!("ANTHROPIC_API_KEY not set, skipping");
        return;
    };

    let request = ChatRequest::builder()
        .message(Message::user("What's the weather in Tokyo? Use the tool."))
        .tools(vec![Tool {
            name: "get_weather".to_string(),
            description: Some("Get current weather for a city".to_string()),
            input_schema: Some(json!({
                "type": "object",
                "properties": {"city": {"type": "string", "description": "City name"}},
                "required": ["city"]
            })),
            cache: false,
        }])
        .build();

    let resp = client.chat_with(request).await.expect("chat failed");
    assert!(
        !resp.tool_calls.is_empty(),
        "Expected tool_calls, got none. content={:?}",
        resp.content
    );
    let tc = &resp.tool_calls[0];
    assert_eq!(tc.name, "get_weather");
    assert!(tc.input.get("city").is_some(), "Expected city in input");
    assert!(!tc.id.is_empty());
    assert!(matches!(resp.stop_reason, StopReason::ToolUse));
    cooldown().await;
}

// ---------------------------------------------------------------------------
// 6. tool use — multi-turn
// ---------------------------------------------------------------------------

#[tokio::test]
async fn live_tool_use_multi_turn() {
    let Some(client) = client() else {
        eprintln!("ANTHROPIC_API_KEY not set, skipping");
        return;
    };

    let tools = vec![Tool {
        name: "get_weather".to_string(),
        description: Some("Get current weather for a city".to_string()),
        input_schema: Some(json!({
            "type": "object",
            "properties": {"city": {"type": "string"}},
            "required": ["city"]
        })),
        cache: false,
    }];

    // Turn 1: model calls tool
    let req1 = ChatRequest::builder()
        .message(Message::user(
            "What's the weather in Taipei? Use get_weather tool.",
        ))
        .tools(tools.clone())
        .build();

    let resp1 = client.chat_with(req1).await.expect("turn1 failed");
    assert!(
        !resp1.tool_calls.is_empty(),
        "Expected tool call in turn 1, content={:?}",
        resp1.content
    );
    let tc = &resp1.tool_calls[0];

    cooldown().await;

    // Turn 2: provide tool result
    let req2 = ChatRequest::builder()
        .messages(vec![
            Message::user("What's the weather in Taipei? Use get_weather tool."),
            Message::assistant_with_tool_calls(&resp1.content, resp1.tool_calls.clone()),
            Message::tool_result(&tc.id, r#"{"temperature": 28, "condition": "Sunny"}"#),
        ])
        .tools(tools)
        .build();

    let resp2 = client.chat_with(req2).await.expect("turn2 failed");
    assert!(!resp2.content.is_empty(), "Expected text in turn 2");
    let lower = resp2.content.to_lowercase();
    assert!(
        lower.contains("28") || lower.contains("sunny") || lower.contains("taipei"),
        "Expected weather info, got: {:?}",
        resp2.content
    );
    cooldown().await;
}

// ---------------------------------------------------------------------------
// 7. stream + tool use
// ---------------------------------------------------------------------------

#[tokio::test]
async fn live_stream_tool_use() {
    let Some(client) = client() else {
        eprintln!("ANTHROPIC_API_KEY not set, skipping");
        return;
    };

    let request = ChatRequest::builder()
        .message(Message::user("Use the calculate tool to compute 2+2."))
        .tools(vec![Tool {
            name: "calculate".to_string(),
            description: Some("Calculate a math expression".to_string()),
            input_schema: Some(json!({
                "type": "object",
                "properties": {"expression": {"type": "string", "description": "Math expression"}},
                "required": ["expression"]
            })),
            cache: false,
        }])
        .build();

    let mut stream = client.stream_with(request).await.expect("stream failed");

    let mut events = Vec::new();
    while let Some(event) = stream.next().await {
        events.push(event.clone());
        if event.done {
            break;
        }
    }

    let types: Vec<_> = events.iter().map(|e| e.event_type.clone()).collect();
    assert!(
        types.contains(&StreamEventType::ToolCallStart),
        "Expected ToolCallStart, got: {:?}",
        types
    );
    assert!(
        types.contains(&StreamEventType::ToolCallArgs),
        "Expected ToolCallArgs, got: {:?}",
        types
    );
    assert!(
        types.contains(&StreamEventType::ToolCallEnd),
        "Expected ToolCallEnd, got: {:?}",
        types
    );

    let starts: Vec<_> = events
        .iter()
        .filter(|e| e.event_type == StreamEventType::ToolCallStart)
        .collect();
    assert_eq!(starts[0].tool_call_name.as_deref(), Some("calculate"));
    assert!(starts[0].tool_call_id.is_some());

    let args: String = events
        .iter()
        .filter(|e| e.event_type.clone() == StreamEventType::ToolCallArgs)
        .filter_map(|e| e.tool_call_args_delta.as_deref())
        .collect();
    let parsed: serde_json::Value = serde_json::from_str(&args).expect("failed to parse tool args");
    assert!(
        parsed.get("expression").is_some(),
        "Expected expression in args, got: {:?}",
        parsed
    );
}

// ---------------------------------------------------------------------------
// 6. stop_reason propagation through real SSE stream
// ---------------------------------------------------------------------------

#[tokio::test]
async fn live_stream_propagates_max_tokens_stop_reason() {
    let Some(client) = client() else {
        eprintln!("ANTHROPIC_API_KEY not set, skipping");
        return;
    };

    // max_tokens=8 forces Anthropic to truncate → message_delta.delta.stop_reason = "max_tokens"
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
        "expected MaxTokens propagated from message_delta.stop_reason, got: {:?}",
        done.stop_reason
    );

    cooldown().await;
}

#[tokio::test]
async fn live_collect_stream_records_max_tokens_on_chat_response() {
    let Some(client) = client() else {
        eprintln!("ANTHROPIC_API_KEY not set, skipping");
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
        "collect_stream should backfill MaxTokens from the terminal done event, got: {:?}",
        response.stop_reason
    );

    cooldown().await;
}
