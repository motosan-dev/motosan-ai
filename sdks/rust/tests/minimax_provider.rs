#![cfg(feature = "minimax")]

use motosan_ai::providers::minimax::MinimaxProvider;
use motosan_ai::providers::ProviderImpl;
use motosan_ai::{ChatRequest, Message, StopReason};
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
    let request = ChatRequest {
        messages: vec![Message::user("hello")],
        model: None,
        system: Some("rules".to_string()),
        temperature: Some(0.3),
        max_tokens: Some(40),
        tools: None,
        provider_options: None,
    };

    let response = provider.chat(request).await.expect("chat response");
    assert_eq!(response.content, "hello from minimax");
    assert_eq!(response.model, "MiniMax-Text-01");
    assert!(matches!(response.stop_reason, StopReason::Stop));

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
        .mock("POST", "/v1/text/chatcompletion_v2")
        .match_header("authorization", "Bearer test-key")
        .with_status(200)
        .with_header("content-type", "text/event-stream")
        .with_body(sse_body)
        .create_async()
        .await;

    let provider = MinimaxProvider::new("test-key", None, Some(server.url()));
    let request = ChatRequest {
        messages: vec![Message::user("hello")],
        model: None,
        system: None,
        temperature: None,
        max_tokens: None,
        tools: None,
        provider_options: None,
    };

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
