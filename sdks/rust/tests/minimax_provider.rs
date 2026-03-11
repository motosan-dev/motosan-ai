#![cfg(feature = "minimax")]

use mockito::Matcher;
use motosan_ai::providers::minimax::MinimaxProvider;
use motosan_ai::providers::ProviderImpl;
use motosan_ai::{ChatRequest, Message, MotosanError, StopReason, DEFAULT_MINIMAX_MODEL};
use serde_json::json;
use tokio_stream::StreamExt;

#[tokio::test]
async fn minimax_chat_maps_response() {
    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("POST", "/chat/completions")
        .match_header("authorization", "Bearer test-key")
        .with_status(200)
        .with_body(
            json!({
                "model": "MiniMax-Text-01",
                "choices": [{"message": {"content": "hello from minimax"}, "finish_reason": "stop"}],
                "usage": {"prompt_tokens": 9, "completion_tokens": 4}
            })
            .to_string(),
        )
        .create_async()
        .await;

    let provider = MinimaxProvider::new("test-key", None, Some(server.url()));
    let request = ChatRequest::builder()
        .message(Message::user("hello"))
        .system("rules")
        .temperature(0.3)
        .max_tokens(40)
        .build();

    let response = provider.chat(request).await.expect("chat response");
    assert_eq!(response.content, "hello from minimax");
    assert_eq!(response.model, "MiniMax-Text-01");
    assert!(matches!(response.stop_reason, StopReason::Stop));

    mock.assert_async().await;
}

#[tokio::test]
async fn minimax_request_uses_default_model_and_allows_override() {
    let mut server = mockito::Server::new_async().await;

    let default_mock = server
        .mock("POST", "/chat/completions")
        .match_header("authorization", "Bearer test-key")
        .match_body(Matcher::Regex(format!(
            r#"\"model\"\s*:\s*\"{}\""#,
            DEFAULT_MINIMAX_MODEL
        )))
        .with_status(200)
        .with_body(
            json!({
                "model": DEFAULT_MINIMAX_MODEL,
                "choices": [{"message": {"content": "ok"}, "finish_reason": "stop"}],
                "usage": {"prompt_tokens": 1, "completion_tokens": 1}
            })
            .to_string(),
        )
        .create_async()
        .await;

    let provider = MinimaxProvider::new("test-key", None, Some(server.url()));
    let request = ChatRequest::builder()
        .message(Message::user("hello"))
        .build();
    let _ = provider.chat(request).await.expect("chat response");
    default_mock.assert_async().await;

    server.reset();
    let override_model = "MiniMax-Text-01";
    let override_mock = server
        .mock("POST", "/chat/completions")
        .match_header("authorization", "Bearer test-key")
        .match_body(Matcher::Regex(format!(
            r#"\"model\"\s*:\s*\"{}\""#,
            override_model
        )))
        .with_status(200)
        .with_body(
            json!({
                "model": override_model,
                "choices": [{"message": {"content": "ok"}, "finish_reason": "stop"}],
                "usage": {"prompt_tokens": 1, "completion_tokens": 1}
            })
            .to_string(),
        )
        .create_async()
        .await;

    let request = ChatRequest::builder()
        .message(Message::user("hello"))
        .model(override_model)
        .build();
    let _ = provider.chat(request).await.expect("chat response");
    override_mock.assert_async().await;
}

#[tokio::test]
async fn minimax_request_merges_system_prompt_into_first_user_message() {
    let mut server = mockito::Server::new_async().await;

    let no_system_role_mock = server
        .mock("POST", "/chat/completions")
        .match_header("authorization", "Bearer test-key")
        .match_body(Matcher::Regex(r#"\"role\"\s*:\s*\"system\""#.to_string()))
        .expect(0)
        .with_status(200)
        .with_body("{}")
        .create_async()
        .await;

    let merged_system_mock = server
        .mock("POST", "/chat/completions")
        .match_header("authorization", "Bearer test-key")
        .match_body(Matcher::Regex(
            r#"\"content\"\s*:\s*\"global rules\\n\\nmessage rules\\n\\nhello\""#.to_string(),
        ))
        .with_status(200)
        .with_body(
            json!({
                "model": "MiniMax-Text-01",
                "choices": [{"message": {"content": "ok"}, "finish_reason": "stop"}],
                "usage": {"prompt_tokens": 1, "completion_tokens": 1},
                "base_resp": {"status_code": 0, "status_msg": ""}
            })
            .to_string(),
        )
        .create_async()
        .await;

    let provider = MinimaxProvider::new("test-key", None, Some(server.url()));
    let request = ChatRequest::builder()
        .system("global rules")
        .message(Message::system("message rules"))
        .message(Message::user("hello"))
        .build();

    let response = provider.chat(request).await.expect("chat response");
    assert_eq!(response.content, "ok");

    merged_system_mock.assert_async().await;
    no_system_role_mock.assert_async().await;
}

#[tokio::test]
async fn minimax_request_inserts_user_message_when_only_system_prompts_exist() {
    let mut server = mockito::Server::new_async().await;

    let mock = server
        .mock("POST", "/chat/completions")
        .match_header("authorization", "Bearer test-key")
        .match_body(Matcher::Regex(r#"\"role\"\s*:\s*\"user\""#.to_string()))
        .match_body(Matcher::Regex(
            r#"\"content\"\s*:\s*\"only system\""#.to_string(),
        ))
        .with_status(200)
        .with_body(
            json!({
                "model": "MiniMax-Text-01",
                "choices": [{"message": {"content": "ok"}, "finish_reason": "stop"}],
                "usage": {"prompt_tokens": 1, "completion_tokens": 1},
                "base_resp": {"status_code": 0, "status_msg": ""}
            })
            .to_string(),
        )
        .create_async()
        .await;

    let provider = MinimaxProvider::new("test-key", None, Some(server.url()));
    let request = ChatRequest::builder().system("only system").build();

    let response = provider.chat(request).await.expect("chat response");
    assert_eq!(response.content, "ok");
    mock.assert_async().await;
}

#[tokio::test]
async fn minimax_stream_emits_content_and_done() {
    let mut server = mockito::Server::new_async().await;
    let sse_body = concat!(
        "data: {\"choices\":[{\"delta\":{\"content\":\"hel\"}}]}\n\n",
        "data: {\"choices\":[{\"delta\":{\"content\":\"lo\"}}]}\n\n",
        "data: [DONE]\n\n"
    );

    let mock = server
        .mock("POST", "/chat/completions")
        .match_header("authorization", "Bearer test-key")
        .with_status(200)
        .with_header("content-type", "text/event-stream")
        .with_body(sse_body)
        .create_async()
        .await;

    let provider = MinimaxProvider::new("test-key", None, Some(server.url()));
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
async fn minimax_stream_ignores_malformed_chunks() {
    let mut server = mockito::Server::new_async().await;
    let sse_body = concat!(
        "data: not-json\n\n",
        "data: {\"choices\":[{\"delta\":{\"content\":\"ok\"}}]}\n\n",
        "data: [DONE]\n\n"
    );

    let mock = server
        .mock("POST", "/chat/completions")
        .match_header("authorization", "Bearer test-key")
        .with_status(200)
        .with_header("content-type", "text/event-stream")
        .with_body(sse_body)
        .create_async()
        .await;

    let provider = MinimaxProvider::new("test-key", None, Some(server.url()));
    let request = ChatRequest::builder()
        .message(Message::user("hello"))
        .build();

    let mut stream = provider.stream(request).await.expect("stream response");
    let mut events = Vec::new();
    while let Some(event) = stream.next().await {
        events.push(event);
    }

    assert_eq!(events.len(), 2);
    assert_eq!(events[0].content, "ok");
    assert!(!events[0].done);
    assert!(events[1].done);

    mock.assert_async().await;
}

#[tokio::test]
async fn minimax_maps_payload_level_invalid_api_key_to_auth_error() {
    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("POST", "/chat/completions")
        .match_header("authorization", "Bearer bad-key")
        .with_status(200)
        .with_body(
            json!({
                "base_resp": {
                    "status_code": 2049,
                    "status_msg": "invalid api key"
                }
            })
            .to_string(),
        )
        .create_async()
        .await;

    let provider = MinimaxProvider::new("bad-key", None, Some(server.url()));
    let request = ChatRequest::builder()
        .message(Message::user("hello"))
        .build();

    let result = provider.chat(request).await;
    assert!(matches!(result, Err(MotosanError::Auth(_))));

    mock.assert_async().await;
}

#[tokio::test]
async fn minimax_maps_payload_level_insufficient_balance_to_rate_limit_error() {
    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("POST", "/chat/completions")
        .match_header("authorization", "Bearer test-key")
        .with_status(200)
        .with_body(
            json!({
                "base_resp": {
                    "status_code": 1008,
                    "status_msg": "insufficient balance (1008)"
                }
            })
            .to_string(),
        )
        .create_async()
        .await;

    let provider = MinimaxProvider::new("test-key", None, Some(server.url()));
    let request = ChatRequest::builder()
        .message(Message::user("hello"))
        .build();

    let result = provider.chat(request).await;
    assert!(matches!(result, Err(MotosanError::RateLimit(_))));

    mock.assert_async().await;
}

#[tokio::test]
async fn minimax_maps_payload_level_4xxx_to_invalid_request_error() {
    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("POST", "/chat/completions")
        .match_header("authorization", "Bearer test-key")
        .with_status(200)
        .with_body(
            json!({
                "base_resp": {
                    "status_code": 4001,
                    "status_msg": "bad request"
                }
            })
            .to_string(),
        )
        .create_async()
        .await;

    let provider = MinimaxProvider::new("test-key", None, Some(server.url()));
    let request = ChatRequest::builder()
        .message(Message::user("hello"))
        .build();

    let result = provider.chat(request).await;
    assert!(matches!(result, Err(MotosanError::InvalidRequest(_))));

    mock.assert_async().await;
}

#[tokio::test]
async fn minimax_chat_strips_think_blocks_by_default() {
    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("POST", "/chat/completions")
        .match_header("authorization", "Bearer test-key")
        .with_status(200)
        .with_body(
            json!({
                "model": "MiniMax-M2.5-highspeed",
                "choices": [{"message": {"content": "<think>internal chain</think>\n\npong"}, "finish_reason": "stop"}],
                "usage": {"prompt_tokens": 9, "completion_tokens": 4},
                "base_resp": {"status_code": 0, "status_msg": ""}
            })
            .to_string(),
        )
        .create_async()
        .await;

    let provider = MinimaxProvider::new("test-key", None, Some(server.url()));
    let request = ChatRequest::builder()
        .message(Message::user("reply pong"))
        .build();

    let response = provider.chat(request).await.expect("chat response");
    assert_eq!(response.content, "pong");
    mock.assert_async().await;
}

#[tokio::test]
async fn minimax_chat_can_expose_reasoning_from_provider_flag() {
    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("POST", "/chat/completions")
        .match_header("authorization", "Bearer test-key")
        .with_status(200)
        .with_body(
            json!({
                "model": "MiniMax-M2.5-highspeed",
                "choices": [{"message": {"content": "<think>internal chain</think>\n\npong"}, "finish_reason": "stop"}],
                "usage": {"prompt_tokens": 9, "completion_tokens": 4},
                "base_resp": {"status_code": 0, "status_msg": ""}
            })
            .to_string(),
        )
        .create_async()
        .await;

    let provider =
        MinimaxProvider::new("test-key", None, Some(server.url())).with_expose_reasoning(true);
    let request = ChatRequest::builder()
        .message(Message::user("reply pong"))
        .build();

    let response = provider.chat(request).await.expect("chat response");
    assert!(response.content.contains("<think>internal chain</think>"));
    assert!(response.content.ends_with("pong"));
    mock.assert_async().await;
}

#[tokio::test]
async fn minimax_chat_can_expose_reasoning_from_request_options() {
    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("POST", "/chat/completions")
        .match_header("authorization", "Bearer test-key")
        .with_status(200)
        .with_body(
            json!({
                "model": "MiniMax-M2.5-highspeed",
                "choices": [{"message": {"content": "<think>internal chain</think>\n\npong"}, "finish_reason": "stop"}],
                "usage": {"prompt_tokens": 9, "completion_tokens": 4},
                "base_resp": {"status_code": 0, "status_msg": ""}
            })
            .to_string(),
        )
        .create_async()
        .await;

    let provider = MinimaxProvider::new("test-key", None, Some(server.url()));
    let request = ChatRequest::builder()
        .message(Message::user("reply pong"))
        .provider_options(json!({"minimax_expose_reasoning": true}))
        .build();

    let response = provider.chat(request).await.expect("chat response");
    assert!(response.content.contains("<think>internal chain</think>"));
    assert!(response.content.ends_with("pong"));
    mock.assert_async().await;
}

#[tokio::test]
async fn minimax_chat_falls_back_to_reasoning_content_when_content_empty() {
    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("POST", "/chat/completions")
        .match_header("authorization", "Bearer test-key")
        .with_status(200)
        .with_body(
            json!({
                "model": "MiniMax-M2.5-highspeed",
                "choices": [{"message": {"content": "", "reasoning_content": "fallback answer"}, "finish_reason": "stop"}],
                "usage": {"prompt_tokens": 9, "completion_tokens": 4},
                "base_resp": {"status_code": 0, "status_msg": ""}
            })
            .to_string(),
        )
        .create_async()
        .await;

    let provider = MinimaxProvider::new("test-key", None, Some(server.url()));
    let request = ChatRequest::builder()
        .message(Message::user("reply"))
        .build();

    let response = provider.chat(request).await.expect("chat response");
    assert_eq!(response.content, "fallback answer");
    mock.assert_async().await;
}

#[tokio::test]
async fn minimax_chat_uses_reasoning_content_when_content_only_think_blocks() {
    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("POST", "/chat/completions")
        .match_header("authorization", "Bearer test-key")
        .with_status(200)
        .with_body(
            json!({
                "model": "MiniMax-M2.5-highspeed",
                "choices": [{"message": {"content": "<think>secret</think>", "reasoning_content": "public fallback"}, "finish_reason": "stop"}],
                "usage": {"prompt_tokens": 9, "completion_tokens": 4},
                "base_resp": {"status_code": 0, "status_msg": ""}
            })
            .to_string(),
        )
        .create_async()
        .await;

    let provider = MinimaxProvider::new("test-key", None, Some(server.url()));
    let request = ChatRequest::builder()
        .message(Message::user("reply"))
        .build();

    let response = provider.chat(request).await.expect("chat response");
    assert_eq!(response.content, "public fallback");
    mock.assert_async().await;
}

#[tokio::test]
async fn minimax_stream_falls_back_to_reasoning_content_when_delta_content_empty() {
    let mut server = mockito::Server::new_async().await;
    let sse_body = concat!(
        "data: {\"choices\":[{\"delta\":{\"content\":\"\",\"reasoning_content\":\"fallback\"}}]}\n\n",
        "data: [DONE]\n\n"
    );

    let mock = server
        .mock("POST", "/chat/completions")
        .match_header("authorization", "Bearer test-key")
        .with_status(200)
        .with_header("content-type", "text/event-stream")
        .with_body(sse_body)
        .create_async()
        .await;

    let provider = MinimaxProvider::new("test-key", None, Some(server.url()));
    let request = ChatRequest::builder()
        .message(Message::user("hello"))
        .build();

    let mut stream = provider.stream(request).await.expect("stream response");
    let mut events = Vec::new();
    while let Some(event) = stream.next().await {
        events.push(event);
    }

    assert_eq!(events.len(), 2);
    assert_eq!(events[0].content, "fallback");
    assert!(!events[0].done);
    assert!(events[1].done);
    mock.assert_async().await;
}
