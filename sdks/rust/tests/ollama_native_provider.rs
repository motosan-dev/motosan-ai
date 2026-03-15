#![cfg(feature = "ollama_native")]

use mockito::Matcher;
use motosan_ai::providers::ollama::OllamaProvider;
use motosan_ai::providers::ProviderImpl;
use motosan_ai::{ChatRequest, Message, StopReason, Tool, DEFAULT_OLLAMA_MODEL};
use serde_json::json;
use tokio_stream::StreamExt;

fn build_provider(base_url: String) -> OllamaProvider {
    OllamaProvider::new(DEFAULT_OLLAMA_MODEL.to_string(), base_url)
}

fn build_provider_with_think(base_url: String) -> OllamaProvider {
    OllamaProvider::new(DEFAULT_OLLAMA_MODEL.to_string(), base_url).with_think(Some("on".into()))
}

#[tokio::test]
async fn ollama_native_chat_maps_response() {
    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("POST", "/api/chat")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            json!({
                "model": "llama3.2",
                "message": {"role": "assistant", "content": "hello from ollama native"},
                "done": true,
                "prompt_eval_count": 12,
                "eval_count": 8,
                "total_duration": 123456
            })
            .to_string(),
        )
        .create_async()
        .await;

    let provider = build_provider(server.url());
    let request = ChatRequest::builder()
        .message(Message::user("hello"))
        .build();

    let response = provider.chat(request).await.expect("chat response");
    assert_eq!(response.content, "hello from ollama native");
    assert_eq!(response.model, "llama3.2");
    assert_eq!(response.usage.input_tokens, 12);
    assert_eq!(response.usage.output_tokens, 8);
    assert!(matches!(response.stop_reason, StopReason::Stop));

    mock.assert_async().await;
}

#[tokio::test]
async fn ollama_native_stream_emits_deltas_and_done() {
    let mut server = mockito::Server::new_async().await;
    let ndjson_body = concat!(
        "{\"message\":{\"role\":\"assistant\",\"content\":\"The\"},\"done\":false}\n",
        "{\"message\":{\"role\":\"assistant\",\"content\":\" sky\"},\"done\":false}\n",
        "{\"message\":{\"role\":\"assistant\",\"content\":\" is blue.\"},\"done\":false}\n",
        "{\"done\":true,\"total_duration\":174560334,\"eval_count\":18}\n"
    );

    let mock = server
        .mock("POST", "/api/chat")
        .with_status(200)
        .with_header("content-type", "application/x-ndjson")
        .with_body(ndjson_body)
        .create_async()
        .await;

    let provider = build_provider(server.url());
    let request = ChatRequest::builder()
        .message(Message::user("hello"))
        .build();

    let mut stream = provider.stream(request).await.expect("stream response");
    let mut events = Vec::new();

    while let Some(event) = stream.next().await {
        events.push(event);
    }

    assert_eq!(events.len(), 4);
    assert_eq!(events[0].content, "The");
    assert!(!events[0].done);
    assert_eq!(events[1].content, " sky");
    assert!(!events[1].done);
    assert_eq!(events[2].content, " is blue.");
    assert!(!events[2].done);
    assert!(events[3].done);

    mock.assert_async().await;
}

#[tokio::test]
async fn ollama_native_tool_calls_generates_id() {
    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("POST", "/api/chat")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            json!({
                "model": "llama3.2",
                "message": {
                    "role": "assistant",
                    "content": "",
                    "tool_calls": [{
                        "function": {
                            "name": "get_weather",
                            "arguments": {"location": "Paris"}
                        }
                    }]
                },
                "done": true
            })
            .to_string(),
        )
        .create_async()
        .await;

    let provider = build_provider(server.url());
    let request = ChatRequest::builder()
        .message(Message::user("weather?"))
        .tools(vec![Tool {
            name: "get_weather".to_string(),
            description: Some("Get weather".to_string()),
            input_schema: Some(
                json!({"type":"object","properties":{"location":{"type":"string"}}}),
            ),
        }])
        .build();

    let response = provider.chat(request).await.expect("chat response");
    assert_eq!(response.tool_calls.len(), 1);
    // ID should be generated since native Ollama doesn't provide one
    assert_eq!(response.tool_calls[0].id, "call_0");
    assert_eq!(response.tool_calls[0].name, "get_weather");
    assert_eq!(response.tool_calls[0].input["location"], "Paris");
    assert!(matches!(response.stop_reason, StopReason::ToolUse));

    mock.assert_async().await;
}

#[tokio::test]
async fn ollama_native_think_mode_sends_think_true() {
    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("POST", "/api/chat")
        .match_body(Matcher::Regex(r#""think"\s*:\s*true"#.to_string()))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            json!({
                "model": "qwen3",
                "message": {
                    "role": "assistant",
                    "content": "The answer is 42.",
                    "thinking": "Let me reason about this..."
                },
                "done": true,
                "prompt_eval_count": 5,
                "eval_count": 10
            })
            .to_string(),
        )
        .create_async()
        .await;

    let provider = build_provider_with_think(server.url());
    let request = ChatRequest::builder()
        .message(Message::user("What is the meaning of life?"))
        .build();

    let response = provider.chat(request).await.expect("chat response");
    // When both thinking and content are present, they are combined
    assert!(response.content.contains("Let me reason about this..."));
    assert!(response.content.contains("The answer is 42."));

    mock.assert_async().await;
}

#[tokio::test]
async fn ollama_native_think_mode_thinking_only() {
    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("POST", "/api/chat")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            json!({
                "model": "qwen3",
                "message": {
                    "role": "assistant",
                    "content": "",
                    "thinking": "Deep thoughts here"
                },
                "done": true,
                "prompt_eval_count": 5,
                "eval_count": 10
            })
            .to_string(),
        )
        .create_async()
        .await;

    let provider = build_provider_with_think(server.url());
    let request = ChatRequest::builder()
        .message(Message::user("think"))
        .build();

    let response = provider.chat(request).await.expect("chat response");
    assert_eq!(response.content, "Deep thoughts here");

    mock.assert_async().await;
}

#[tokio::test]
async fn ollama_native_default_model() {
    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("POST", "/api/chat")
        .match_body(Matcher::Regex(format!(
            r#""model"\s*:\s*"{}""#,
            DEFAULT_OLLAMA_MODEL
        )))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            json!({
                "model": DEFAULT_OLLAMA_MODEL,
                "message": {"role": "assistant", "content": "ok"},
                "done": true,
                "prompt_eval_count": 1,
                "eval_count": 1
            })
            .to_string(),
        )
        .create_async()
        .await;

    let provider = build_provider(server.url());
    let request = ChatRequest::builder().message(Message::user("hi")).build();

    let response = provider.chat(request).await.expect("chat response");
    assert_eq!(response.model, DEFAULT_OLLAMA_MODEL);

    mock.assert_async().await;
}

#[tokio::test]
async fn ollama_native_keep_alive_in_request() {
    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("POST", "/api/chat")
        .match_body(Matcher::Regex(r#""keep_alive"\s*:\s*"10m""#.to_string()))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            json!({
                "model": "llama3.2",
                "message": {"role": "assistant", "content": "ok"},
                "done": true
            })
            .to_string(),
        )
        .create_async()
        .await;

    let provider = OllamaProvider::new(DEFAULT_OLLAMA_MODEL.to_string(), server.url())
        .with_keep_alive(Some("10m".to_string()));

    let request = ChatRequest::builder().message(Message::user("hi")).build();

    let _ = provider.chat(request).await.expect("chat response");
    mock.assert_async().await;
}

#[tokio::test]
async fn ollama_native_stream_with_thinking() {
    let mut server = mockito::Server::new_async().await;
    let ndjson_body = concat!(
        "{\"message\":{\"role\":\"assistant\",\"content\":\"\",\"thinking\":\"Hmm\"},\"done\":false}\n",
        "{\"message\":{\"role\":\"assistant\",\"content\":\"Answer\"},\"done\":false}\n",
        "{\"done\":true}\n"
    );

    let mock = server
        .mock("POST", "/api/chat")
        .with_status(200)
        .with_header("content-type", "application/x-ndjson")
        .with_body(ndjson_body)
        .create_async()
        .await;

    let provider = build_provider_with_think(server.url());
    let request = ChatRequest::builder()
        .message(Message::user("think and answer"))
        .build();

    let mut stream = provider.stream(request).await.expect("stream response");
    let mut events = Vec::new();

    while let Some(event) = stream.next().await {
        events.push(event);
    }

    assert_eq!(events.len(), 3);
    assert_eq!(events[0].content, "Hmm");
    assert_eq!(events[1].content, "Answer");
    assert!(events[2].done);

    mock.assert_async().await;
}

#[tokio::test]
async fn ollama_native_client_builder() {
    use motosan_ai::{Client, Provider};

    let client = Client::builder()
        .provider(Provider::Ollama)
        .api_key("")
        .ollama_base_url("http://my-ollama:11434")
        .ollama_native(true)
        .ollama_think("on")
        .ollama_keep_alive("5m")
        .ollama_num_ctx(8192)
        .build()
        .expect("client build");

    assert!(matches!(client.provider(), Provider::Ollama));
}
