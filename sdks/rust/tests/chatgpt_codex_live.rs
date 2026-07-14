//! Live ChatGptCodexProvider smoke test.
//!
//! This hits the real ChatGPT-backend API and requires `~/.codex/auth.json`.
//! Run manually with:
//! `cargo test --features chatgpt-codex --test chatgpt_codex_live -- --ignored --nocapture`

#![cfg(feature = "chatgpt-codex")]

use motosan_ai::providers::chatgpt_codex::ChatGptCodexProvider;
use motosan_ai::providers::ProviderImpl;
use motosan_ai::{ChatRequest, Message, StreamEventType, Tool, ToolSchema};
use serde_json::{json, Value};
use std::{env, fs};
use tokio_stream::StreamExt;

fn load_codex_auth() -> Option<(String, String)> {
    let home = env::var("HOME").ok()?;
    let path = std::path::Path::new(&home).join(".codex/auth.json");
    let raw = fs::read_to_string(path).ok()?;
    let auth: Value = serde_json::from_str(&raw).ok()?;
    let tokens = auth.get("tokens")?;
    let access_token = tokens.get("access_token")?.as_str()?.to_string();
    let account_id = tokens.get("account_id")?.as_str()?.to_string();
    Some((access_token, account_id))
}

#[tokio::test]
#[ignore = "live: makes a real chatgpt.com call; needs ~/.codex/auth.json"]
async fn live_codex_streamed_tool_call_ids_are_consistent() {
    let Some((access_token, account_id)) = load_codex_auth() else {
        eprintln!("skipping live test: missing ~/.codex/auth.json tokens");
        return;
    };

    let provider = ChatGptCodexProvider::new(access_token, account_id, "gpt-5.5", None);
    let add_schema = ToolSchema::new(
        "add",
        "Add two integers.",
        json!({
            "type": "object",
            "properties": {
                "a": {"type": "integer"},
                "b": {"type": "integer"}
            },
            "required": ["a", "b"]
        }),
    );
    let tool = Tool::from(add_schema);
    let request = ChatRequest::builder()
        .messages(vec![Message::user(
            "You MUST call the `add` tool with a=2 and b=3. Do not answer in text.",
        )])
        .tools(vec![tool])
        .build();

    let mut stream = provider.stream(request).await.expect("stream starts");
    let mut events = Vec::new();
    while let Some(item) = stream.next().await {
        events.push(item.expect("stream event"));
    }

    let start = events
        .iter()
        .find(|event| event.event_type == StreamEventType::ToolCallStart)
        .expect("tool call start");
    let call_id = start.tool_call_id.as_deref().expect("tool call start id");
    assert!(
        call_id.starts_with("call_"),
        "expected call_* id, got {call_id}"
    );

    let mut args = String::new();
    for event in events
        .iter()
        .filter(|event| event.event_type == StreamEventType::ToolCallArgs)
    {
        assert_eq!(event.tool_call_id.as_deref(), Some(call_id));
        args.push_str(event.tool_call_args_delta.as_deref().expect("args delta"));
    }

    let parsed_args: Value = serde_json::from_str(&args).expect("tool args json");
    let parsed_obj = parsed_args.as_object().expect("tool args object");
    assert_eq!(parsed_obj.get("a"), Some(&json!(2)));
    assert_eq!(parsed_obj.get("b"), Some(&json!(3)));

    let end = events
        .iter()
        .find(|event| event.event_type == StreamEventType::ToolCallEnd)
        .expect("tool call end");
    assert_eq!(end.tool_call_id.as_deref(), Some(call_id));
}
