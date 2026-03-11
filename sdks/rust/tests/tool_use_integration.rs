#![cfg(any(feature = "anthropic", feature = "openai", feature = "minimax"))]

use motosan_ai::{ChatRequest, Message};
use serde_json::json;

#[cfg(feature = "anthropic")]
#[tokio::test]
async fn anthropic_extracts_tool_use_from_response() {
    use motosan_ai::providers::anthropic::AnthropicProvider;
    use motosan_ai::providers::ProviderImpl;

    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("POST", "/v1/messages")
        .match_header("x-api-key", "test-key")
        .with_status(200)
        .with_body(
            json!({
                "model": "claude-sonnet-4-6",
                "stop_reason": "tool_use",
                "usage": {"input_tokens": 11, "output_tokens": 7},
                "content": [
                    {"type": "text", "text": "calling tool"},
                    {"type": "tool_use", "id": "toolu_1", "name": "get_weather", "input": {"city": "Taipei"}}
                ]
            })
            .to_string(),
        )
        .create_async()
        .await;

    let provider = AnthropicProvider::new("test-key", None, Some(server.url()));
    let request = ChatRequest::builder()
        .message(Message::user("weather?"))
        .build();

    let response = provider.chat(request).await.expect("chat response");
    assert_eq!(response.tool_calls.len(), 1);
    assert_eq!(response.tool_calls[0].id, "toolu_1");
    assert_eq!(response.tool_calls[0].name, "get_weather");
    assert_eq!(response.tool_calls[0].input["city"], "Taipei");
    mock.assert_async().await;
}

#[cfg(feature = "openai")]
#[tokio::test]
async fn openai_extracts_tool_calls_from_response() {
    use motosan_ai::providers::openai::OpenAIProvider;
    use motosan_ai::providers::ProviderImpl;

    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("POST", "/v1/chat/completions")
        .match_header("authorization", "Bearer test-key")
        .with_status(200)
        .with_body(
            json!({
                "model": "gpt-5.3-codex",
                "choices": [{
                    "message": {
                        "content": "",
                        "tool_calls": [{
                            "id": "call_1",
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

    let provider = OpenAIProvider::new("test-key", None, Some(server.url()));
    let request = ChatRequest::builder()
        .message(Message::user("weather?"))
        .build();

    let response = provider.chat(request).await.expect("chat response");
    assert_eq!(response.tool_calls.len(), 1);
    assert_eq!(response.tool_calls[0].id, "call_1");
    assert_eq!(response.tool_calls[0].name, "get_weather");
    assert_eq!(response.tool_calls[0].input["city"], "Taipei");
    mock.assert_async().await;
}

#[cfg(feature = "minimax")]
#[tokio::test]
async fn minimax_extracts_tool_calls_from_response() {
    use motosan_ai::providers::minimax::MinimaxProvider;
    use motosan_ai::providers::ProviderImpl;

    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("POST", "/v1/text/chatcompletion_v2")
        .match_header("authorization", "Bearer test-key")
        .with_status(200)
        .with_body(
            json!({
                "model": "MiniMax-M2.5-highspeed",
                "choices": [{
                    "message": {
                        "content": "",
                        "tool_calls": [{
                            "id": "call_2",
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

    let provider = MinimaxProvider::new("test-key", None, Some(server.url()));
    let request = ChatRequest::builder()
        .message(Message::user("weather?"))
        .build();

    let response = provider.chat(request).await.expect("chat response");
    assert_eq!(response.tool_calls.len(), 1);
    assert_eq!(response.tool_calls[0].id, "call_2");
    assert_eq!(response.tool_calls[0].name, "get_weather");
    assert_eq!(response.tool_calls[0].input["city"], "Taipei");
    mock.assert_async().await;
}
