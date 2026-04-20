//! Live Gemini integration tests — hits the real Google Generative AI API.
//!
//! Requires `GOOGLE_API_KEY` env var. Skips automatically when not set.
//!
//! Run manually:
//!     GOOGLE_API_KEY=... cargo test --features gemini --test gemini_live -- --nocapture

#![cfg(feature = "gemini")]

use motosan_ai::{
    ChatRequest, Client, Message, Provider, StopReason, StreamEventType, Tool, ToolCall, ToolChoice,
};
use serde_json::json;
use std::time::Duration;
use tokio_stream::StreamExt;

fn api_key() -> Option<String> {
    match std::env::var("GOOGLE_API_KEY") {
        Ok(k) if !k.trim().is_empty() => Some(k),
        _ => None,
    }
}

fn client() -> Option<Client> {
    let key = api_key()?;
    Some(
        Client::builder()
            .provider(Provider::Gemini)
            .api_key(key)
            .model("gemini-2.0-flash")
            .build()
            .expect("client build"),
    )
}

async fn cooldown() {
    tokio::time::sleep(Duration::from_secs(2)).await;
}

// ---------------------------------------------------------------------------
// 1. Basic chat
// ---------------------------------------------------------------------------

#[tokio::test]
async fn live_chat_basic() {
    let Some(client) = client() else {
        eprintln!("GOOGLE_API_KEY not set — skipping");
        return;
    };

    let resp = client
        .chat(vec![Message::user("Reply with exactly one word: PONG")])
        .await
        .expect("chat failed");

    assert!(
        resp.content.contains("PONG"),
        "Expected PONG in response, got: {:?}",
        resp.content
    );
    assert!(!resp.model.is_empty(), "model should be set");
    assert!(resp.usage.input_tokens > 0);
    assert!(resp.usage.output_tokens > 0);
    assert_eq!(resp.stop_reason, StopReason::EndTurn);
    cooldown().await;
}

// ---------------------------------------------------------------------------
// 2. System prompt
// ---------------------------------------------------------------------------

#[tokio::test]
async fn live_system_prompt() {
    let Some(client) = client() else {
        eprintln!("GOOGLE_API_KEY not set — skipping");
        return;
    };

    let req = ChatRequest::builder()
        .messages(vec![Message::user("What are you?")])
        .system("You are a helpful robot. Always start your response with ROBOT:")
        .build();
    let resp = client.chat_with(req).await.expect("chat failed");

    assert!(
        resp.content.contains("ROBOT:"),
        "Expected ROBOT: prefix, got: {:?}",
        resp.content
    );
    cooldown().await;
}

// ---------------------------------------------------------------------------
// 3. Streaming — basic text
// ---------------------------------------------------------------------------

#[tokio::test]
async fn live_stream_basic() {
    let Some(client) = client() else {
        eprintln!("GOOGLE_API_KEY not set — skipping");
        return;
    };

    let mut stream = client
        .stream(vec![Message::user("Reply with exactly: STREAM_OK")])
        .await
        .expect("stream failed");

    let mut text_chunks = 0u32;
    let mut saw_usage = false;
    let mut full_text = String::new();

    while let Some(ev) = stream.next().await {
        match ev.event_type {
            StreamEventType::Text => {
                text_chunks += 1;
                full_text.push_str(&ev.content);
            }
            StreamEventType::Usage => {
                saw_usage = true;
                assert!(ev.usage.as_ref().map(|u| u.input_tokens).unwrap_or(0) > 0);
            }
            _ => {}
        }
    }

    assert!(text_chunks > 0, "no text chunks received");
    assert!(saw_usage, "no usage event received");
    assert!(
        full_text.contains("STREAM_OK"),
        "Expected STREAM_OK in: {full_text:?}"
    );
    cooldown().await;
}

// ---------------------------------------------------------------------------
// 4. Streaming — collect
// ---------------------------------------------------------------------------

#[tokio::test]
async fn live_stream_collect() {
    let Some(client) = client() else {
        eprintln!("GOOGLE_API_KEY not set — skipping");
        return;
    };

    let resp = client
        .stream_collect(vec![Message::user("Say exactly: COLLECT_OK")])
        .await
        .expect("stream_collect failed");

    assert!(
        resp.content.contains("COLLECT_OK"),
        "Expected COLLECT_OK, got: {:?}",
        resp.content
    );
    assert_eq!(resp.stop_reason, StopReason::EndTurn);
    assert!(resp.usage.input_tokens > 0);
    assert!(resp.usage.output_tokens > 0);
    cooldown().await;
}

// ---------------------------------------------------------------------------
// 5. Tool use — single turn
// ---------------------------------------------------------------------------

#[tokio::test]
async fn live_tool_use_single_turn() {
    let Some(client) = client() else {
        eprintln!("GOOGLE_API_KEY not set — skipping");
        return;
    };

    let tool = Tool {
        name: "get_current_temperature".into(),
        description: Some("Returns the current temperature for a given city.".into()),
        input_schema: Some(json!({
            "type": "object",
            "properties": {
                "city": {"type": "string", "description": "City name"}
            },
            "required": ["city"]
        })),
        cache: false,
    };

    let req = ChatRequest::builder()
        .messages(vec![Message::user(
            "What is the temperature in Tokyo? Use the get_current_temperature tool.",
        )])
        .tools(vec![tool])
        .tool_choice(ToolChoice::Required)
        .build();

    let resp = client.chat_with(req).await.expect("chat failed");

    assert_eq!(resp.stop_reason, StopReason::ToolUse);
    assert!(!resp.tool_calls.is_empty(), "expected tool calls");
    assert_eq!(resp.tool_calls[0].name, "get_current_temperature");
    assert!(
        resp.tool_calls[0].input.get("city").is_some(),
        "expected 'city' in args, got: {:?}",
        resp.tool_calls[0].input
    );
    cooldown().await;
}

// ---------------------------------------------------------------------------
// 6. Tool use — multi turn
// ---------------------------------------------------------------------------

#[tokio::test]
async fn live_tool_use_multi_turn() {
    let Some(client) = client() else {
        eprintln!("GOOGLE_API_KEY not set — skipping");
        return;
    };

    let tool = Tool {
        name: "add_numbers".into(),
        description: Some("Adds two numbers together.".into()),
        input_schema: Some(json!({
            "type": "object",
            "properties": {
                "a": {"type": "number"},
                "b": {"type": "number"}
            },
            "required": ["a", "b"]
        })),
        cache: false,
    };

    // Turn 1: model should call add_numbers
    let req1 = ChatRequest::builder()
        .messages(vec![Message::user(
            "What is 17 + 25? Use the add_numbers tool.",
        )])
        .tools(vec![tool.clone()])
        .build();

    let resp1 = client.chat_with(req1).await.expect("turn 1 failed");
    assert_eq!(resp1.stop_reason, StopReason::ToolUse);
    assert!(!resp1.tool_calls.is_empty());
    let tc = &resp1.tool_calls[0];
    assert_eq!(tc.name, "add_numbers");

    cooldown().await;

    // Turn 2: provide tool result and ask for final answer
    let msgs = vec![
        Message::user("What is 17 + 25? Use the add_numbers tool."),
        Message::assistant_with_tool_calls("", resp1.tool_calls.clone()),
        Message::tool_result(&tc.id, r#"{"result": 42}"#),
    ];
    let req2 = ChatRequest::builder()
        .messages(msgs)
        .tools(vec![tool])
        .build();

    let resp2 = client.chat_with(req2).await.expect("turn 2 failed");
    assert_eq!(resp2.stop_reason, StopReason::EndTurn);
    assert!(
        resp2.content.contains("42"),
        "Expected '42' in final response, got: {:?}",
        resp2.content
    );
    cooldown().await;
}

// ---------------------------------------------------------------------------
// 7. Max tokens
// ---------------------------------------------------------------------------

#[tokio::test]
async fn live_max_tokens_stop_reason() {
    let Some(client) = client() else {
        eprintln!("GOOGLE_API_KEY not set — skipping");
        return;
    };

    let req = ChatRequest::builder()
        .messages(vec![Message::user(
            "Write a very long story about a dragon. Keep going.",
        )])
        .max_tokens(10)
        .build();

    let resp = client.chat_with(req).await.expect("chat failed");
    assert_eq!(resp.stop_reason, StopReason::MaxTokens);
    cooldown().await;
}
