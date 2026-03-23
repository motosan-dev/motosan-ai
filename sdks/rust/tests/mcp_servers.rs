#![cfg(feature = "anthropic")]

use mockito::Matcher;
use motosan_ai::providers::anthropic::AnthropicProvider;
use motosan_ai::providers::ProviderImpl;
use motosan_ai::{ChatRequest, McpServerConfig, McpServerType, Message, DEFAULT_ANTHROPIC_MODEL};
use serde_json::json;

#[tokio::test]
async fn anthropic_request_includes_mcp_servers() {
    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("POST", "/v1/messages")
        .match_header("x-api-key", "test-key")
        .match_body(Matcher::Regex(
            r#""mcp_servers".*"type"\s*:\s*"url""#.to_string(),
        ))
        .match_body(Matcher::Regex(
            r#""url"\s*:\s*"https://mcp\.example\.com/sse""#.to_string(),
        ))
        .match_body(Matcher::Regex(r#""name"\s*:\s*"linear""#.to_string()))
        .with_status(200)
        .with_body(
            json!({
                "model": DEFAULT_ANTHROPIC_MODEL,
                "stop_reason": "end_turn",
                "usage": {"input_tokens": 1, "output_tokens": 1},
                "content": [{"type": "text", "text": "ok"}]
            })
            .to_string(),
        )
        .create_async()
        .await;

    let provider = AnthropicProvider::new("test-key", None, Some(server.url()));
    let request = ChatRequest::builder()
        .message(Message::user("hello"))
        .mcp_server(McpServerConfig {
            kind: McpServerType::Url,
            url: "https://mcp.example.com/sse".to_string(),
            name: "linear".to_string(),
            authorization_token: None,
        })
        .build();

    let response = provider.chat(request).await.expect("chat response");
    assert_eq!(response.content, "ok");
    mock.assert_async().await;
}

#[tokio::test]
async fn mcp_server_config_serializes_correctly() {
    let config = McpServerConfig {
        kind: McpServerType::Url,
        url: "https://mcp.example.com/sse".to_string(),
        name: "linear".to_string(),
        authorization_token: None,
    };

    let serialized = serde_json::to_value(&config).unwrap();
    assert_eq!(serialized["type"], "url");
    assert_eq!(serialized["url"], "https://mcp.example.com/sse");
    assert_eq!(serialized["name"], "linear");
    assert!(serialized.get("authorization_token").is_none());
}

#[tokio::test]
async fn mcp_server_config_with_auth_token_serializes_correctly() {
    let config = McpServerConfig {
        kind: McpServerType::Url,
        url: "https://mcp.example.com/sse".to_string(),
        name: "linear".to_string(),
        authorization_token: Some("secret-token".to_string()),
    };

    let serialized = serde_json::to_value(&config).unwrap();
    assert_eq!(serialized["type"], "url");
    assert_eq!(serialized["url"], "https://mcp.example.com/sse");
    assert_eq!(serialized["name"], "linear");
    assert_eq!(serialized["authorization_token"], "secret-token");
}

#[tokio::test]
async fn chat_request_without_mcp_servers_omits_field() {
    let request = ChatRequest::builder()
        .message(Message::user("hello"))
        .build();

    let serialized = serde_json::to_value(&request).unwrap();
    assert!(serialized.get("mcp_servers").is_none());
}

#[tokio::test]
async fn chat_request_builder_mcp_server_accumulates() {
    let request = ChatRequest::builder()
        .message(Message::user("hello"))
        .mcp_server(McpServerConfig {
            kind: McpServerType::Url,
            url: "https://mcp1.example.com/sse".to_string(),
            name: "server1".to_string(),
            authorization_token: None,
        })
        .mcp_server(McpServerConfig {
            kind: McpServerType::Url,
            url: "https://mcp2.example.com/sse".to_string(),
            name: "server2".to_string(),
            authorization_token: None,
        })
        .build();

    let servers = request.mcp_servers.as_ref().unwrap();
    assert_eq!(servers.len(), 2);
    assert_eq!(servers[0].name, "server1");
    assert_eq!(servers[1].name, "server2");
}

#[tokio::test]
async fn chat_request_builder_mcp_servers_replaces() {
    let request = ChatRequest::builder()
        .message(Message::user("hello"))
        .mcp_server(McpServerConfig {
            kind: McpServerType::Url,
            url: "https://should-be-replaced.com/sse".to_string(),
            name: "old".to_string(),
            authorization_token: None,
        })
        .mcp_servers(vec![McpServerConfig {
            kind: McpServerType::Url,
            url: "https://mcp.example.com/sse".to_string(),
            name: "new".to_string(),
            authorization_token: None,
        }])
        .build();

    let servers = request.mcp_servers.as_ref().unwrap();
    assert_eq!(servers.len(), 1);
    assert_eq!(servers[0].name, "new");
}
