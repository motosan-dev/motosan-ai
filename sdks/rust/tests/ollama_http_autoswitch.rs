#![cfg(feature = "ollama")]

use mockito::Matcher;
use motosan_ai::{Client, Message, Provider};

#[tokio::test]
async fn ollama_with_keep_alive_routes_to_api_chat_endpoint() {
    // With ollama_keep_alive set, the client must POST to /api/chat
    // (native) rather than /v1/chat/completions (OpenAI-compat), because
    // the OpenAI-compat endpoint silently drops keep_alive server-side.
    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("POST", "/api/chat")
        .match_body(Matcher::Regex(r#"\"keep_alive\"\s*:\s*\"10m\""#.to_string()))
        .with_status(200)
        .with_body(
            serde_json::json!({
                "model": "llama3",
                "message": {"role": "assistant", "content": "ok"},
                "done": true,
                "done_reason": "stop",
                "prompt_eval_count": 1,
                "eval_count": 1
            })
            .to_string(),
        )
        .create_async()
        .await;

    let client = Client::builder()
        .provider(Provider::Ollama)
        .api_key("ollama")
        .ollama_base_url(server.url())
        .ollama_keep_alive("10m")
        .build()
        .expect("build client");

    let _ = client
        .chat(vec![Message::user("hi")])
        .await
        .expect("chat against mock should succeed");
    mock.assert_async().await;
}

#[tokio::test]
async fn ollama_with_num_ctx_streams_from_api_chat_endpoint() {
    use tokio_stream::StreamExt;

    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("POST", "/api/chat")
        .match_body(Matcher::Regex(
            r#"\"options\"\s*:\s*\{[^}]*\"num_ctx\"\s*:\s*4096"#.to_string(),
        ))
        .match_body(Matcher::Regex(r#"\"stream\"\s*:\s*true"#.to_string()))
        .with_status(200)
        // Minimal NDJSON: one chunk + a done marker.
        .with_body(
            "{\"model\":\"llama3\",\"message\":{\"role\":\"assistant\",\"content\":\"hi\"},\"done\":false}\n\
             {\"model\":\"llama3\",\"message\":{\"role\":\"assistant\",\"content\":\"\"},\"done\":true,\"done_reason\":\"stop\",\"prompt_eval_count\":1,\"eval_count\":1}\n",
        )
        .create_async()
        .await;

    let client = Client::builder()
        .provider(Provider::Ollama)
        .api_key("ollama")
        .ollama_base_url(server.url())
        .ollama_num_ctx(4096)
        .build()
        .expect("build client");

    let mut stream = client
        .stream(vec![Message::user("hi")])
        .await
        .expect("stream against mock should open");
    let mut seen_text = false;
    while let Some(event) = stream.next().await {
        if !event.content.is_empty() {
            seen_text = true;
        }
    }
    assert!(seen_text, "stream should yield at least one text chunk");
    mock.assert_async().await;
}
