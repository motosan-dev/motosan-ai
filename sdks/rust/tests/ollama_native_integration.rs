#![cfg(feature = "ollama_native")]

use motosan_ai::providers::ollama::OllamaProvider;
use motosan_ai::providers::ProviderImpl;
use motosan_ai::{ChatRequest, Message, DEFAULT_OLLAMA_MODEL};
use tokio_stream::StreamExt;

fn ollama_host() -> Option<String> {
    std::env::var("OLLAMA_HOST").ok()
}

#[tokio::test]
async fn ollama_native_integration_chat() {
    let host = match ollama_host() {
        Some(h) => h,
        None => {
            eprintln!("OLLAMA_HOST not set, skipping integration test");
            return;
        }
    };

    let provider = OllamaProvider::new(DEFAULT_OLLAMA_MODEL.to_string(), host);
    let request = ChatRequest::builder()
        .message(Message::user("Say hello in one word"))
        .build();

    let response = provider.chat(request).await.expect("chat response");
    assert!(!response.content.is_empty());
    assert!(!response.model.is_empty());
}

#[tokio::test]
async fn ollama_native_integration_stream() {
    let host = match ollama_host() {
        Some(h) => h,
        None => {
            eprintln!("OLLAMA_HOST not set, skipping integration test");
            return;
        }
    };

    let provider = OllamaProvider::new(DEFAULT_OLLAMA_MODEL.to_string(), host);
    let request = ChatRequest::builder()
        .message(Message::user("Say hello in one word"))
        .build();

    let mut stream = provider.stream(request).await.expect("stream");
    let mut content = String::new();
    let mut got_done = false;

    while let Some(event_item) = stream.next().await {
        let event = event_item.expect("stream item should not fail");
        if event.done {
            got_done = true;
        } else {
            content.push_str(&event.content);
        }
    }

    assert!(!content.is_empty());
    assert!(got_done);
}

#[tokio::test]
async fn ollama_native_integration_think_mode() {
    let host = match ollama_host() {
        Some(h) => h,
        None => {
            eprintln!("OLLAMA_HOST not set, skipping integration test");
            return;
        }
    };

    // Use a reasoning model if available; otherwise this test just verifies the flag is sent
    let model =
        std::env::var("OLLAMA_THINK_MODEL").unwrap_or_else(|_| DEFAULT_OLLAMA_MODEL.to_string());

    let provider = OllamaProvider::new(model, host).with_think(Some("on".into()));

    let request = ChatRequest::builder()
        .message(Message::user("What is 2+2?"))
        .build();

    let response = provider.chat(request).await.expect("chat response");
    assert!(!response.content.is_empty());
}
