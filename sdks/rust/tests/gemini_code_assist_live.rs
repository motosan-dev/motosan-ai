//! GeminiCodeAssist 真实 API 测试 — 需要环境变数 GEMINI_OAUTH_TOKEN + GEMINI_PROJECT_ID
//!
//! Run:
//!   GEMINI_OAUTH_TOKEN=ya29.xxx GEMINI_PROJECT_ID=my-project \
//!     cargo test --features gemini-code-assist --test gemini_code_assist_live -- --nocapture

#![cfg(feature = "gemini-code-assist")]

use motosan_ai::providers::gemini_code_assist::GeminiCodeAssistProvider;
use motosan_ai::providers::ProviderImpl;
use motosan_ai::{ChatRequest, Message, StopReason, StreamEventType, Tool, ToolChoice};
use serde_json::json;
use std::time::Duration;
use tokio_stream::StreamExt;

fn creds() -> Option<(String, String)> {
    let token = std::env::var("GEMINI_OAUTH_TOKEN")
        .ok()
        .filter(|s| !s.is_empty())?;
    let project = std::env::var("GEMINI_PROJECT_ID")
        .ok()
        .filter(|s| !s.is_empty())?;
    Some((token, project))
}

fn provider() -> Option<GeminiCodeAssistProvider> {
    let (token, project) = creds()?;
    Some(GeminiCodeAssistProvider::new(
        token,
        project,
        Some("gemini-2.5-flash".into()),
        None,
    ))
}

async fn cooldown() {
    tokio::time::sleep(Duration::from_secs(2)).await;
}

// ---------------------------------------------------------------------------
// 1. chat — 基本文字
// ---------------------------------------------------------------------------

#[tokio::test]
async fn live_chat_basic() {
    let Some(p) = provider() else {
        eprintln!("GEMINI_OAUTH_TOKEN / GEMINI_PROJECT_ID 未设定，跳过");
        return;
    };
    let resp = p
        .chat(
            ChatRequest::builder()
                .messages(vec![Message::user("Reply with exactly one word: PONG")])
                .build(),
        )
        .await
        .expect("chat 失败");

    println!("content: {:?}", resp.content);
    println!("model: {}", resp.model);
    println!("usage: {:?}", resp.usage);
    assert!(
        resp.content.contains("PONG"),
        "预期 PONG，得到: {:?}",
        resp.content
    );
    assert!(!resp.model.is_empty(), "model 应该有值");
    assert!(resp.usage.input_tokens > 0);
    assert!(resp.usage.output_tokens > 0);
    assert_eq!(resp.stop_reason, StopReason::EndTurn);
    cooldown().await;
}

// ---------------------------------------------------------------------------
// 2. chat — system prompt
// ---------------------------------------------------------------------------

#[tokio::test]
async fn live_system_prompt() {
    let Some(p) = provider() else { return };
    let req = ChatRequest::builder()
        .messages(vec![Message::user("What are you?")])
        .system("You are a helpful robot. Always start your response with ROBOT:")
        .build();
    let resp = p.chat(req).await.expect("chat 失败");
    println!("content: {:?}", resp.content);
    assert!(
        resp.content.contains("ROBOT:"),
        "预期 ROBOT: 前缀，得到: {:?}",
        resp.content
    );
    cooldown().await;
}

// ---------------------------------------------------------------------------
// 3. stream — 累积文字
// ---------------------------------------------------------------------------

#[tokio::test]
async fn live_stream_collect() {
    let Some(p) = provider() else { return };
    let stream = p
        .stream(
            ChatRequest::builder()
                .messages(vec![Message::user("Say exactly: STREAM_OK")])
                .build(),
        )
        .await
        .expect("stream 失败");

    let resp = motosan_ai::stream::collect_stream(stream).await;
    println!("content: {:?}", resp.content);
    println!("stop_reason: {:?}", resp.stop_reason);
    println!("usage: {:?}", resp.usage);
    assert!(
        resp.content.contains("STREAM_OK"),
        "预期 STREAM_OK，得到: {:?}",
        resp.content
    );
    assert_eq!(resp.stop_reason, StopReason::EndTurn);
    cooldown().await;
}

// ---------------------------------------------------------------------------
// 4. stream — 事件类型检验
// ---------------------------------------------------------------------------

#[tokio::test]
async fn live_stream_event_types() {
    let Some(p) = provider() else { return };
    let mut stream = p
        .stream(
            ChatRequest::builder()
                .messages(vec![Message::user("Reply with exactly: EVENTS_OK")])
                .build(),
        )
        .await
        .expect("stream 失败");

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
                let u = ev.usage.as_ref().unwrap();
                println!(
                    "usage event: input={} output={}",
                    u.input_tokens, u.output_tokens
                );
                assert!(u.input_tokens > 0);
            }
            _ => {}
        }
    }

    assert!(text_chunks > 0, "没有收到任何 text chunk");
    assert!(saw_usage, "没有收到 usage event");
    assert!(
        full_text.contains("EVENTS_OK"),
        "预期 EVENTS_OK，得到: {:?}",
        full_text
    );
    cooldown().await;
}

// ---------------------------------------------------------------------------
// 5. tool use — 单轮
// ---------------------------------------------------------------------------

#[tokio::test]
async fn live_tool_use_single_turn() {
    let Some(p) = provider() else { return };
    let tool = Tool {
        name: "get_weather".into(),
        description: Some("Get weather for a city.".into()),
        input_schema: Some(json!({
            "type": "object",
            "properties": {"city": {"type": "string"}},
            "required": ["city"]
        })),
        cache: false,
    };
    let req = ChatRequest::builder()
        .messages(vec![Message::user(
            "What is the weather in Tokyo? Use the get_weather tool.",
        )])
        .tools(vec![tool])
        .tool_choice(ToolChoice::Required)
        .build();

    let resp = p.chat(req).await.expect("chat 失败");
    println!("tool_calls: {:?}", resp.tool_calls);
    assert_eq!(
        resp.stop_reason,
        StopReason::ToolUse,
        "预期 ToolUse stop reason"
    );
    assert!(!resp.tool_calls.is_empty(), "预期有 tool calls");
    assert_eq!(resp.tool_calls[0].name, "get_weather");
    assert!(
        resp.tool_calls[0].input.get("city").is_some(),
        "预期有 city 参数"
    );
    cooldown().await;
}

// ---------------------------------------------------------------------------
// 6. max_tokens
// ---------------------------------------------------------------------------

#[tokio::test]
async fn live_max_tokens_stop_reason() {
    let Some(p) = provider() else { return };
    let req = ChatRequest::builder()
        .messages(vec![Message::user(
            "Count from 1 to 1000, one number per line.",
        )])
        .max_tokens(10)
        .build();
    let resp = p.chat(req).await.expect("chat 失败");
    println!("stop_reason: {:?}", resp.stop_reason);
    assert_eq!(resp.stop_reason, StopReason::MaxTokens);
    cooldown().await;
}
