#![cfg(feature = "ollama")]

use mockito::Matcher;
use motosan_ai::providers::openai::{OpenAIAuthStyle, OpenAIProvider};
use motosan_ai::providers::ProviderImpl;
use motosan_ai::{ChatRequest, Message, StopReason, Tool, DEFAULT_OLLAMA_MODEL};
use serde_json::json;
use tokio_stream::StreamExt;

fn build_ollama_provider(base_url: String) -> OpenAIProvider {
    OpenAIProvider::new("", Some(DEFAULT_OLLAMA_MODEL.to_string()))
        .with_chat_url(format!(
            "{}/v1/chat/completions",
            base_url.trim_end_matches('/')
        ))
        .with_auth_style(OpenAIAuthStyle::Bearer)
}

#[tokio::test]
async fn ollama_chat_maps_response() {
    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("POST", "/v1/chat/completions")
        .match_header("authorization", Matcher::Any)
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            json!({
                "model": "llama3.2",
                "choices": [{"message": {"content": "hello from ollama"}, "finish_reason": "stop"}],
                "usage": {"prompt_tokens": 10, "completion_tokens": 5}
            })
            .to_string(),
        )
        .create_async()
        .await;

    let provider = build_ollama_provider(server.url());
    let request = ChatRequest::builder()
        .message(Message::user("hello"))
        .build();

    let response = provider.chat(request).await.expect("chat response");
    assert_eq!(response.content, "hello from ollama");
    assert_eq!(response.model, "llama3.2");
    assert_eq!(response.usage.input_tokens, 10);
    assert_eq!(response.usage.output_tokens, 5);
    assert!(matches!(response.stop_reason, StopReason::Stop));

    mock.assert_async().await;
}

#[tokio::test]
async fn ollama_stream_emits_deltas_and_done() {
    let mut server = mockito::Server::new_async().await;
    let sse_body = concat!(
        "data: {\"choices\":[{\"delta\":{\"content\":\"hel\"}}]}\n\n",
        "data: {\"choices\":[{\"delta\":{\"content\":\"lo\"}}]}\n\n",
        "data: [DONE]\n\n"
    );

    let mock = server
        .mock("POST", "/v1/chat/completions")
        .match_header("authorization", Matcher::Any)
        .with_status(200)
        .with_header("content-type", "text/event-stream")
        .with_body(sse_body)
        .create_async()
        .await;

    let provider = build_ollama_provider(server.url());
    let request = ChatRequest::builder()
        .message(Message::user("hello"))
        .build();

    let mut stream = provider.stream(request).await.expect("stream response");
    let mut events = Vec::new();

    while let Some(event) = stream.next().await {
        events.push(event);
    }

    assert_eq!(events.len(), 3);
    assert_eq!(events[0].content, "hel");
    assert_eq!(events[1].content, "lo");
    assert!(events[2].done);

    mock.assert_async().await;
}

#[tokio::test]
async fn ollama_chat_with_tool_calls() {
    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("POST", "/v1/chat/completions")
        .match_header("authorization", Matcher::Any)
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            json!({
                "model": "llama3.2",
                "choices": [{
                    "message": {
                        "content": "",
                        "tool_calls": [{
                            "id": "call_ollama_1",
                            "type": "function",
                            "function": {"name": "get_weather", "arguments": "{\"city\":\"Taipei\"}"}
                        }]
                    },
                    "finish_reason": "tool_calls"
                }],
                "usage": {"prompt_tokens": 10, "completion_tokens": 4}
            })
            .to_string(),
        )
        .create_async()
        .await;

    let provider = build_ollama_provider(server.url());
    let request = ChatRequest::builder()
        .message(Message::user("weather?"))
        .tools(vec![Tool {
            name: "get_weather".to_string(),
            description: Some("Get weather by city".to_string()),
            input_schema: Some(json!({"type":"object","properties":{"city":{"type":"string"}}})),
            cache: false,
        }])
        .build();

    let response = provider.chat(request).await.expect("chat response");
    assert_eq!(response.tool_calls.len(), 1);
    assert_eq!(response.tool_calls[0].id, "call_ollama_1");
    assert_eq!(response.tool_calls[0].name, "get_weather");
    assert_eq!(response.tool_calls[0].input["city"], "Taipei");
    assert!(matches!(response.stop_reason, StopReason::ToolUse));

    mock.assert_async().await;
}

#[tokio::test]
async fn ollama_uses_default_model() {
    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("POST", "/v1/chat/completions")
        .match_header("authorization", Matcher::Any)
        .match_body(Matcher::Regex(format!(
            r#"\"model\"\s*:\s*\"{}\""#,
            DEFAULT_OLLAMA_MODEL
        )))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            json!({
                "model": DEFAULT_OLLAMA_MODEL,
                "choices": [{"message": {"content": "ok"}, "finish_reason": "stop"}],
                "usage": {"prompt_tokens": 1, "completion_tokens": 1}
            })
            .to_string(),
        )
        .create_async()
        .await;

    let provider = build_ollama_provider(server.url());
    let request = ChatRequest::builder()
        .message(Message::user("hello"))
        .build();

    let response = provider.chat(request).await.expect("chat response");
    assert_eq!(response.model, DEFAULT_OLLAMA_MODEL);

    mock.assert_async().await;
}

#[tokio::test]
async fn ollama_client_builder_sets_base_url() {
    use motosan_ai::{Client, Provider};

    let client = Client::builder()
        .provider(Provider::Ollama)
        .api_key("")
        .ollama_base_url("http://my-ollama:11434")
        .build()
        .expect("client build");

    assert!(matches!(client.provider(), Provider::Ollama));
}
