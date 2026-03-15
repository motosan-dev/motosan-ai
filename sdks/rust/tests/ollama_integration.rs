#![cfg(feature = "ollama")]

use motosan_ai::{Client, Message, Provider};

#[tokio::test]
async fn ollama_integration_chat() {
    let host = match std::env::var("OLLAMA_HOST") {
        Ok(h) => h,
        Err(_) => {
            eprintln!("OLLAMA_HOST not set, skipping integration test");
            return;
        }
    };

    let client = Client::builder()
        .provider(Provider::Ollama)
        .api_key("")
        .ollama_base_url(host)
        .build()
        .expect("client build");

    let response = client
        .chat(vec![Message::user("Say hello in one word.")])
        .await
        .expect("ollama chat");

    assert!(!response.content.is_empty());
}
