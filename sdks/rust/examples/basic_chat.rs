//! Basic chat example using Anthropic.
//!
//! Run with:
//!   ANTHROPIC_API_KEY=sk-ant-... cargo run --example basic_chat --features anthropic

use motosan_ai::{Client, Message, Provider};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let api_key = std::env::var("ANTHROPIC_API_KEY")
        .expect("ANTHROPIC_API_KEY environment variable not set");

    let client = Client::builder()
        .provider(Provider::Anthropic)
        .api_key(api_key)
        .build()?;

    let response = client.chat(vec![
        Message::user("What is the capital of France? Answer in one sentence."),
    ]).await?;

    println!("Response: {}", response.content);
    println!("Model: {}", response.model);
    println!("Tokens: {} in, {} out", response.usage.input_tokens, response.usage.output_tokens);

    Ok(())
}
