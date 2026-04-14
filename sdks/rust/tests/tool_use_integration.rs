#![cfg(any(feature = "anthropic", feature = "openai", feature = "minimax"))]

use mockito::Matcher;
use motosan_ai::{ChatRequest, Message, StopReason, Tool};
use serde_json::json;

#[cfg(feature = "anthropic")]
#[tokio::test]
async fn anthropic_serializes_tools_and_extracts_tool_use() {
    use motosan_ai::providers::anthropic::AnthropicProvider;
    use motosan_ai::providers::ProviderImpl;

    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("POST", "/v1/messages")
        .match_header("x-api-key", "test-key")
        .match_body(Matcher::Regex(r#"\"tools\"\s*:\s*\["#.to_string()))
        .match_body(Matcher::Regex(
            r#"\"name\"\s*:\s*\"get_weather\""#.to_string(),
        ))
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
        .tools(vec![Tool {
            name: "get_weather".to_string(),
            description: Some("Get weather by city".to_string()),
            input_schema: Some(json!({"type":"object","properties":{"city":{"type":"string"}}})),
            cache: false,
        }])
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
async fn openai_serializes_tools_and_extracts_tool_calls() {
    use motosan_ai::providers::openai::OpenAIProvider;
    use motosan_ai::providers::ProviderImpl;

    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("POST", "/v1/chat/completions")
        .match_header("authorization", "Bearer test-key")
        .match_body(Matcher::Regex(r#"\"tools\"\s*:\s*\["#.to_string()))
        .match_body(Matcher::Regex(
            r#"\"name\"\s*:\s*\"get_weather\""#.to_string(),
        ))
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

    let provider = OpenAIProvider::new("test-key", None)
        .with_chat_url(format!("{}/v1/chat/completions", server.url()));
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
    assert_eq!(response.tool_calls[0].id, "call_1");
    assert_eq!(response.tool_calls[0].name, "get_weather");
    assert_eq!(response.tool_calls[0].input["city"], "Taipei");
    mock.assert_async().await;
}

#[cfg(feature = "minimax")]
#[tokio::test]
async fn minimax_serializes_tools_and_extracts_tool_calls() {
    use motosan_ai::providers::minimax::MinimaxProvider;
    use motosan_ai::providers::ProviderImpl;

    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("POST", "/chat/completions")
        .match_header("authorization", "Bearer test-key")
        .match_body(Matcher::Regex(r#"\"tools\"\s*:\s*\["#.to_string()))
        .match_body(Matcher::Regex(
            r#"\"name\"\s*:\s*\"get_weather\""#.to_string(),
        ))
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
                "usage": {"prompt_tokens": 10, "completion_tokens": 4},
                "base_resp": {"status_code": 0, "status_msg": ""}
            })
            .to_string(),
        )
        .create_async()
        .await;

    let provider = MinimaxProvider::new("test-key", None, Some(server.url()));
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
    assert_eq!(response.tool_calls[0].id, "call_2");
    assert_eq!(response.tool_calls[0].name, "get_weather");
    assert_eq!(response.tool_calls[0].input["city"], "Taipei");
    mock.assert_async().await;
}

#[cfg(feature = "anthropic")]
#[tokio::test]
async fn anthropic_multi_turn_tool_use_roundtrip() {
    use motosan_ai::providers::anthropic::AnthropicProvider;
    use motosan_ai::providers::ProviderImpl;

    let mut server = mockito::Server::new_async().await;
    let provider = AnthropicProvider::new("test-key", None, Some(server.url()));

    let turn1_mock = server
        .mock("POST", "/v1/messages")
        .match_header("x-api-key", "test-key")
        .with_status(200)
        .with_body(
            json!({
                "model": "claude-sonnet-4-6",
                "stop_reason": "tool_use",
                "usage": {"input_tokens": 11, "output_tokens": 7},
                "content": [
                    {"type": "text", "text": "Let me check"},
                    {"type": "tool_use", "id": "toolu_1", "name": "get_weather", "input": {"city": "Taipei"}}
                ]
            })
            .to_string(),
        )
        .create_async()
        .await;

    let resp1 = provider
        .chat(
            ChatRequest::builder()
                .message(Message::user("what's the weather?"))
                .tools(vec![Tool {
                    name: "get_weather".to_string(),
                    description: Some("Get weather by city".to_string()),
                    input_schema: Some(
                        json!({"type":"object","properties":{"city":{"type":"string"}}}),
                    ),
                    cache: false,
                }])
                .build(),
        )
        .await
        .expect("turn1 response");
    assert!(matches!(resp1.stop_reason, StopReason::ToolUse));
    turn1_mock.assert_async().await;

    server.reset();
    let turn2_mock = server
        .mock("POST", "/v1/messages")
        .match_header("x-api-key", "test-key")
        .match_body(Matcher::Regex(r#"\"type\"\s*:\s*\"tool_use\""#.to_string()))
        .match_body(Matcher::Regex(
            r#"\"tool_use_id\"\s*:\s*\"toolu_1\""#.to_string(),
        ))
        .match_body(Matcher::Regex(
            r#"\"type\"\s*:\s*\"tool_result\""#.to_string(),
        ))
        .with_status(200)
        .with_body(
            json!({
                "model": "claude-sonnet-4-6",
                "stop_reason": "end_turn",
                "usage": {"input_tokens": 15, "output_tokens": 9},
                "content": [{"type": "text", "text": "It is 25°C and cloudy."}]
            })
            .to_string(),
        )
        .create_async()
        .await;

    let turn2_messages = vec![
        Message::user("what's the weather?"),
        Message::assistant_with_tool_calls("Let me check", resp1.tool_calls.clone()),
        Message::tool_result(resp1.tool_calls[0].id.clone(), "25°C, cloudy"),
    ];
    let resp2 = provider
        .chat(ChatRequest::builder().messages(turn2_messages).build())
        .await
        .expect("turn2 response");
    assert!(matches!(resp2.stop_reason, StopReason::EndTurn));
    assert!(resp2.content.contains("25"));
    turn2_mock.assert_async().await;
}

#[cfg(feature = "openai")]
#[tokio::test]
async fn openai_multi_turn_tool_use_roundtrip() {
    use motosan_ai::providers::openai::OpenAIProvider;
    use motosan_ai::providers::ProviderImpl;

    let mut server = mockito::Server::new_async().await;
    let provider = OpenAIProvider::new("test-key", None)
        .with_chat_url(format!("{}/v1/chat/completions", server.url()));

    let turn1_mock = server
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

    let resp1 = provider
        .chat(
            ChatRequest::builder()
                .message(Message::user("what's the weather?"))
                .tools(vec![Tool {
                    name: "get_weather".to_string(),
                    description: Some("Get weather by city".to_string()),
                    input_schema: Some(
                        json!({"type":"object","properties":{"city":{"type":"string"}}}),
                    ),
                    cache: false,
                }])
                .build(),
        )
        .await
        .expect("turn1 response");
    assert!(matches!(resp1.stop_reason, StopReason::ToolUse));
    turn1_mock.assert_async().await;

    server.reset();
    let turn2_mock = server
        .mock("POST", "/v1/chat/completions")
        .match_header("authorization", "Bearer test-key")
        .match_body(Matcher::Regex(r#"\"tool_calls\"\s*:\s*\["#.to_string()))
        .match_body(Matcher::Regex(
            r#"\"arguments\"\s*:\s*\"\{\\\"city\\\":\\\"Taipei\\\"\}\""#.to_string(),
        ))
        .match_body(Matcher::Regex(
            r#"\"tool_call_id\"\s*:\s*\"call_1\""#.to_string(),
        ))
        .with_status(200)
        .with_body(
            json!({
                "model": "gpt-5.3-codex",
                "choices": [{
                    "message": {"content": "It is 25°C and cloudy."},
                    "finish_reason": "stop"
                }],
                "usage": {"prompt_tokens": 15, "completion_tokens": 9}
            })
            .to_string(),
        )
        .create_async()
        .await;

    let turn2_messages = vec![
        Message::user("what's the weather?"),
        Message::assistant_with_tool_calls("Let me check", resp1.tool_calls.clone()),
        Message::tool_result(resp1.tool_calls[0].id.clone(), "25°C, cloudy"),
    ];
    let resp2 = provider
        .chat(ChatRequest::builder().messages(turn2_messages).build())
        .await
        .expect("turn2 response");
    assert!(matches!(resp2.stop_reason, StopReason::Stop));
    assert!(resp2.content.contains("25"));
    turn2_mock.assert_async().await;
}

#[cfg(feature = "minimax")]
#[tokio::test]
async fn minimax_multi_turn_tool_use_roundtrip() {
    use motosan_ai::providers::minimax::MinimaxProvider;
    use motosan_ai::providers::ProviderImpl;

    let mut server = mockito::Server::new_async().await;
    let provider = MinimaxProvider::new("test-key", None, Some(server.url()));

    let turn1_mock = server
        .mock("POST", "/chat/completions")
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
                "usage": {"prompt_tokens": 10, "completion_tokens": 4},
                "base_resp": {"status_code": 0, "status_msg": ""}
            })
            .to_string(),
        )
        .create_async()
        .await;

    let resp1 = provider
        .chat(
            ChatRequest::builder()
                .message(Message::user("what's the weather?"))
                .tools(vec![Tool {
                    name: "get_weather".to_string(),
                    description: Some("Get weather by city".to_string()),
                    input_schema: Some(
                        json!({"type":"object","properties":{"city":{"type":"string"}}}),
                    ),
                    cache: false,
                }])
                .build(),
        )
        .await
        .expect("turn1 response");
    assert!(matches!(resp1.stop_reason, StopReason::ToolUse));
    turn1_mock.assert_async().await;

    server.reset();
    let turn2_mock = server
        .mock("POST", "/chat/completions")
        .match_header("authorization", "Bearer test-key")
        .match_body(Matcher::Regex(r#"\"tool_calls\"\s*:\s*\["#.to_string()))
        .match_body(Matcher::Regex(
            r#"\"arguments\"\s*:\s*\"\{\\\"city\\\":\\\"Taipei\\\"\}\""#.to_string(),
        ))
        .match_body(Matcher::Regex(
            r#"\"tool_call_id\"\s*:\s*\"call_2\""#.to_string(),
        ))
        .with_status(200)
        .with_body(
            json!({
                "model": "MiniMax-M2.5-highspeed",
                "choices": [{
                    "message": {"content": "It is 25°C and cloudy."},
                    "finish_reason": "stop"
                }],
                "usage": {"prompt_tokens": 15, "completion_tokens": 9},
                "base_resp": {"status_code": 0, "status_msg": ""}
            })
            .to_string(),
        )
        .create_async()
        .await;

    let turn2_messages = vec![
        Message::user("what's the weather?"),
        Message::assistant_with_tool_calls("Let me check", resp1.tool_calls.clone()),
        Message::tool_result(resp1.tool_calls[0].id.clone(), "25°C, cloudy"),
    ];
    let resp2 = provider
        .chat(ChatRequest::builder().messages(turn2_messages).build())
        .await
        .expect("turn2 response");
    assert!(matches!(resp2.stop_reason, StopReason::Stop));
    assert!(resp2.content.contains("25"));
    turn2_mock.assert_async().await;
}
