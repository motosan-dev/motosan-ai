#![cfg(feature = "anthropic")]

use mockito::Matcher;
use motosan_ai::providers::anthropic::AnthropicProvider;
use motosan_ai::providers::ProviderImpl;
use motosan_ai::{
    ChatRequest, McpServerConfig, McpServerType, McpToolConfig, Message, DEFAULT_ANTHROPIC_MODEL,
};
use serde_json::json;
use tokio_stream::StreamExt;

#[tokio::test]
async fn anthropic_request_includes_mcp_servers_and_toolset() {
    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("POST", "/v1/messages")
        .match_header("x-api-key", "test-key")
        .match_header("anthropic-beta", "mcp-client-2025-11-20")
        .match_body(Matcher::Regex(
            r#""mcp_servers".*"type"\s*:\s*"url""#.to_string(),
        ))
        .match_body(Matcher::Regex(
            r#""url"\s*:\s*"https://mcp\.example\.com/sse""#.to_string(),
        ))
        .match_body(Matcher::Regex(r#""name"\s*:\s*"linear""#.to_string()))
        .match_body(Matcher::Regex(r#""type"\s*:\s*"mcp_toolset""#.to_string()))
        .match_body(Matcher::Regex(
            r#""mcp_server_name"\s*:\s*"linear""#.to_string(),
        ))
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
    assert!(serialized.get("mcp_tool_configs").is_none());
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

    // Auto-populated mcp_tool_configs
    let configs = request.mcp_tool_configs.as_ref().unwrap();
    assert_eq!(configs.len(), 2);
    assert_eq!(
        configs[0],
        McpToolConfig::All {
            mcp_server_name: "server1".to_string()
        }
    );
    assert_eq!(
        configs[1],
        McpToolConfig::All {
            mcp_server_name: "server2".to_string()
        }
    );
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

    // mcp_tool_configs also replaced
    let configs = request.mcp_tool_configs.as_ref().unwrap();
    assert_eq!(configs.len(), 1);
    assert_eq!(
        configs[0],
        McpToolConfig::All {
            mcp_server_name: "new".to_string()
        }
    );
}

#[tokio::test]
async fn mcp_tool_config_allowed_mode() {
    let request = ChatRequest::builder()
        .message(Message::user("hello"))
        .mcp_server(McpServerConfig {
            kind: McpServerType::Url,
            url: "https://mcp.example.com/sse".to_string(),
            name: "linear".to_string(),
            authorization_token: None,
        })
        .mcp_tool_config(McpToolConfig::Allowed {
            mcp_server_name: "linear".to_string(),
            allowed_tools: vec!["list_issues".to_string(), "create_issue".to_string()],
        })
        .build();

    let configs = request.mcp_tool_configs.as_ref().unwrap();
    assert_eq!(configs.len(), 1);
    assert_eq!(
        configs[0],
        McpToolConfig::Allowed {
            mcp_server_name: "linear".to_string(),
            allowed_tools: vec!["list_issues".to_string(), "create_issue".to_string()],
        }
    );
}

#[tokio::test]
async fn mcp_tool_config_denied_mode() {
    let request = ChatRequest::builder()
        .message(Message::user("hello"))
        .mcp_server(McpServerConfig {
            kind: McpServerType::Url,
            url: "https://mcp.example.com/sse".to_string(),
            name: "linear".to_string(),
            authorization_token: None,
        })
        .mcp_tool_config(McpToolConfig::Denied {
            mcp_server_name: "linear".to_string(),
            denied_tools: vec!["delete_issue".to_string()],
        })
        .build();

    let configs = request.mcp_tool_configs.as_ref().unwrap();
    assert_eq!(configs.len(), 1);
    assert_eq!(
        configs[0],
        McpToolConfig::Denied {
            mcp_server_name: "linear".to_string(),
            denied_tools: vec!["delete_issue".to_string()],
        }
    );
}

#[tokio::test]
async fn no_beta_header_without_mcp() {
    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("POST", "/v1/messages")
        .match_header("x-api-key", "test-key")
        .match_header("anthropic-beta", Matcher::Missing)
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
        .build();

    let response = provider.chat(request).await.expect("chat response");
    assert_eq!(response.content, "ok");
    mock.assert_async().await;
}

#[tokio::test]
async fn mcp_tool_configs_replaces_auto_generated() {
    let request = ChatRequest::builder()
        .message(Message::user("hello"))
        .mcp_server(McpServerConfig {
            kind: McpServerType::Url,
            url: "https://mcp.example.com/sse".to_string(),
            name: "linear".to_string(),
            authorization_token: None,
        })
        .mcp_tool_configs(vec![McpToolConfig::Denied {
            mcp_server_name: "linear".to_string(),
            denied_tools: vec!["dangerous_tool".to_string()],
        }])
        .build();

    let configs = request.mcp_tool_configs.as_ref().unwrap();
    assert_eq!(configs.len(), 1);
    assert_eq!(
        configs[0],
        McpToolConfig::Denied {
            mcp_server_name: "linear".to_string(),
            denied_tools: vec!["dangerous_tool".to_string()],
        }
    );
}

/// Direct `ChatRequest` construction with only `mcp_tool_configs` (no `mcp_servers`)
/// must still trigger the beta header.
#[tokio::test]
async fn beta_header_sent_when_only_mcp_tool_configs_present() {
    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("POST", "/v1/messages")
        .match_header("x-api-key", "test-key")
        .match_header("anthropic-beta", "mcp-client-2025-11-20")
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

    // Construct ChatRequest directly (bypassing builder) with only mcp_tool_configs
    let request = ChatRequest {
        messages: vec![Message::user("hello")],
        model: None,
        system: None,
        system_blocks: None,
        system_cache: false,
        temperature: None,
        max_tokens: None,
        tools: None,
        tool_choice: None,
        provider_options: None,
        mcp_servers: None,
        mcp_tool_configs: Some(vec![McpToolConfig::All {
            mcp_server_name: "linear".to_string(),
        }]),
        thinking: None,
        stop_sequences: None,
    };

    let response = provider.chat(request).await.expect("chat response");
    assert_eq!(response.content, "ok");
    mock.assert_async().await;
}

/// When both OAuth (setup token) and MCP servers are active, a single merged
/// `anthropic-beta` header must be sent containing both `oauth-2025-04-20`
/// and `mcp-client-2025-11-20`.
#[tokio::test]
async fn oauth_plus_mcp_sends_single_merged_beta_header() {
    let mut server = mockito::Server::new_async().await;

    // OAuth chat() delegates to stream(), so mock must return SSE format.
    let sse_body = [
        "event: message_start",
        &format!(
            "data: {}",
            json!({
                "type": "message_start",
                "message": {
                    "id": "msg_test",
                    "type": "message",
                    "role": "assistant",
                    "content": [],
                    "model": DEFAULT_ANTHROPIC_MODEL,
                    "usage": {"input_tokens": 5, "output_tokens": 0}
                }
            })
        ),
        "",
        "event: content_block_delta",
        &format!(
            "data: {}",
            json!({"type": "content_block_delta", "index": 0, "delta": {"type": "text_delta", "text": "merged"}})
        ),
        "",
        "event: message_delta",
        &format!(
            "data: {}",
            json!({"type": "message_delta", "delta": {"stop_reason": "end_turn"}, "usage": {"output_tokens": 1}})
        ),
        "",
        "event: message_stop",
        &format!("data: {}", json!({"type": "message_stop"})),
        "",
    ]
    .join("\n");

    // Expect exactly ONE anthropic-beta header that contains BOTH betas.
    let expected_beta =
        "claude-code-20250219,oauth-2025-04-20,fine-grained-tool-streaming-2025-05-14,interleaved-thinking-2025-05-14,mcp-client-2025-11-20";
    let mock = server
        .mock("POST", "/v1/messages")
        .match_header("authorization", "Bearer sk-ant-oat01-mcp-oauth-test")
        .match_header("anthropic-beta", expected_beta)
        .with_status(200)
        .with_header("content-type", "text/event-stream")
        .with_body(sse_body)
        .create_async()
        .await;

    let provider = AnthropicProvider::new("sk-ant-oat01-mcp-oauth-test", None, Some(server.url()));
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
    assert_eq!(response.content, "merged");
    mock.assert_async().await;
}

/// Stream path with OAuth + MCP also sends a single merged beta header.
#[tokio::test]
async fn oauth_plus_mcp_stream_sends_single_merged_beta_header() {
    let mut server = mockito::Server::new_async().await;

    let sse_body = [
        "event: content_block_delta",
        &format!(
            "data: {}",
            json!({"type": "content_block_delta", "index": 0, "delta": {"type": "text_delta", "text": "streamed"}})
        ),
        "",
        "event: message_stop",
        &format!("data: {}", json!({"type": "message_stop"})),
        "",
    ]
    .join("\n");

    let expected_beta =
        "claude-code-20250219,oauth-2025-04-20,fine-grained-tool-streaming-2025-05-14,interleaved-thinking-2025-05-14,mcp-client-2025-11-20";
    let mock = server
        .mock("POST", "/v1/messages")
        .match_header("authorization", "Bearer sk-ant-oat01-stream-mcp-test")
        .match_header("anthropic-beta", expected_beta)
        .with_status(200)
        .with_header("content-type", "text/event-stream")
        .with_body(sse_body)
        .create_async()
        .await;

    let provider = AnthropicProvider::new("sk-ant-oat01-stream-mcp-test", None, Some(server.url()));
    let request = ChatRequest::builder()
        .message(Message::user("hello"))
        .mcp_server(McpServerConfig {
            kind: McpServerType::Url,
            url: "https://mcp.example.com/sse".to_string(),
            name: "linear".to_string(),
            authorization_token: None,
        })
        .build();

    let mut stream = provider.stream(request).await.expect("stream");
    let mut text = String::new();
    while let Some(event_item) = stream.next().await {
        let event = event_item.expect("stream item should not fail");
        text.push_str(&event.content);
    }
    assert_eq!(text, "streamed");
    mock.assert_async().await;
}
