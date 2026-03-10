#![cfg(feature = "minimax")]

use mockito::Matcher;
use motosan_ai::providers::minimax::MinimaxProvider;
use motosan_ai::providers::ProviderImpl;
use motosan_ai::{ChatRequest, Message, StopReason, DEFAULT_MINIMAX_MODEL};
use serde_json::json;
use tokio_stream::StreamExt;

#[tokio::test]
async fn minimax_chat_maps_response() {
    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("POST", "/v1/text/chatcompletion_v2")
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
        .mock("POST", "/v1/text/chatcompletion_v2")
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
        .mock("POST", "/v1/text/chatcompletion_v2")
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
async fn minimax_stream_emits_content_and_done() {
    let mut server = mockito::Server::new_async().await;
    let sse_body = concat!(
        "data: {\"choices\":[{\"delta\":{\"content\":\"hel\"}}]}\n\n",
        "data: {\"choices\":[{\"delta\":{\"content\":\"lo\"}}]}\n\n",
        "data: [DONE]\n\n"
    );

    let mock = server
        .mock("POST", "/v1/text/chatcompletion_v2")
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
        .mock("POST", "/v1/text/chatcompletion_v2")
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
