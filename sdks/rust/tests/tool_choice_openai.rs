#![cfg(feature = "openai")]

use mockito::Matcher;
use motosan_ai::providers::openai::OpenAIProvider;
use motosan_ai::providers::ProviderImpl;
use motosan_ai::{ChatRequest, Message, Tool, ToolChoice};
use serde_json::json;

fn dummy_tool() -> Tool {
    Tool {
        name: "get_weather".to_string(),
        description: Some("Get weather info".to_string()),
        input_schema: Some(json!({"type": "object", "properties": {"city": {"type": "string"}}})),
        cache: false,
    }
}

fn success_body() -> String {
    json!({
        "id": "chatcmpl-1",
        "model": "gpt-4o",
        "choices": [{
            "index": 0,
            "message": {"role": "assistant", "content": "ok"},
            "finish_reason": "stop"
        }],
        "usage": {"prompt_tokens": 5, "completion_tokens": 10}
    })
    .to_string()
}

#[tokio::test]
async fn openai_tool_choice_auto() {
    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("POST", "/v1/chat/completions")
        .match_body(Matcher::Regex(r#""tool_choice"\s*:\s*"auto""#.to_string()))
        .with_status(200)
        .with_body(success_body())
        .create_async()
        .await;

    let provider = OpenAIProvider::new("test-key", None)
        .with_chat_url(format!("{}/v1/chat/completions", server.url()));
    let request = ChatRequest::builder()
        .message(Message::user("hello"))
        .tools(vec![dummy_tool()])
        .tool_choice(ToolChoice::Auto)
        .build();

    provider.chat(request).await.expect("chat ok");
    mock.assert_async().await;
}

#[tokio::test]
async fn openai_tool_choice_required() {
    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("POST", "/v1/chat/completions")
        .match_body(Matcher::Regex(
            r#""tool_choice"\s*:\s*"required""#.to_string(),
        ))
        .with_status(200)
        .with_body(success_body())
        .create_async()
        .await;

    let provider = OpenAIProvider::new("test-key", None)
        .with_chat_url(format!("{}/v1/chat/completions", server.url()));
    let request = ChatRequest::builder()
        .message(Message::user("hello"))
        .tools(vec![dummy_tool()])
        .tool_choice(ToolChoice::Required)
        .build();

    provider.chat(request).await.expect("chat ok");
    mock.assert_async().await;
}

#[tokio::test]
async fn openai_tool_choice_none() {
    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("POST", "/v1/chat/completions")
        .match_body(Matcher::Regex(r#""tool_choice"\s*:\s*"none""#.to_string()))
        .with_status(200)
        .with_body(success_body())
        .create_async()
        .await;

    let provider = OpenAIProvider::new("test-key", None)
        .with_chat_url(format!("{}/v1/chat/completions", server.url()));
    let request = ChatRequest::builder()
        .message(Message::user("hello"))
        .tools(vec![dummy_tool()])
        .tool_choice(ToolChoice::None)
        .build();

    provider.chat(request).await.expect("chat ok");
    mock.assert_async().await;
}

#[tokio::test]
async fn openai_tool_choice_forced_tool() {
    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("POST", "/v1/chat/completions")
        .match_body(Matcher::Regex(
            r#""tool_choice"\s*:\s*\{[^}]*"function"\s*:\s*\{[^}]*"name"\s*:\s*"get_weather""#
                .to_string(),
        ))
        .with_status(200)
        .with_body(success_body())
        .create_async()
        .await;

    let provider = OpenAIProvider::new("test-key", None)
        .with_chat_url(format!("{}/v1/chat/completions", server.url()));
    let request = ChatRequest::builder()
        .message(Message::user("hello"))
        .tools(vec![dummy_tool()])
        .tool_choice(ToolChoice::Tool {
            name: "get_weather".to_string(),
        })
        .build();

    provider.chat(request).await.expect("chat ok");
    mock.assert_async().await;
}
