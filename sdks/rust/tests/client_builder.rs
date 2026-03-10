use motosan_ai::{Client, Message, MotosanError, Provider};

#[test]
fn builder_requires_provider_and_api_key() {
    let missing_provider = Client::builder().api_key("k").build();
    assert!(matches!(missing_provider, Err(MotosanError::Config(_))));

    let missing_api_key = Client::builder().provider(Provider::OpenAI).build();
    assert!(matches!(missing_api_key, Err(MotosanError::Config(_))));
}

#[tokio::test]
async fn chat_and_stream_exist_and_dispatch() {
    let client = Client::builder()
        .provider(Provider::OpenAI)
        .api_key("k")
        .build()
        .expect("build client");

    let messages = vec![Message::user("hello")];
    let chat_result = client.chat(messages.clone()).await;
    let stream_result = client.stream(messages).await;

    #[cfg(not(feature = "openai"))]
    assert!(matches!(chat_result, Err(MotosanError::Config(_))));
    #[cfg(not(feature = "openai"))]
    assert!(matches!(stream_result, Err(MotosanError::Config(_))));

    #[cfg(feature = "openai")]
    assert!(chat_result.is_err());
    #[cfg(feature = "openai")]
    assert!(stream_result.is_err());
}
