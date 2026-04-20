//! Live test: GeminiProvider with image input.
//! Run:
//!   GOOGLE_API_KEY=AIza... cargo test --features gemini --test gemini_vision_live -- --nocapture

#![cfg(feature = "gemini")]

use motosan_ai::{ChatRequest, Client, ContentBlock, ImageSource, Message, Provider};

fn client() -> Option<Client> {
    let key = std::env::var("GOOGLE_API_KEY")
        .ok()
        .filter(|s| !s.is_empty())?;
    Some(
        Client::builder()
            .provider(Provider::Gemini)
            .api_key(key)
            .model("gemini-2.5-flash")
            .build()
            .expect("client build"),
    )
}

// 10x10 solid red PNG (base64)
const RED_PIXEL_PNG: &str =
    "iVBORw0KGgoAAAANSUhEUgAAAAoAAAAKCAIAAAACUFjqAAAAEklEQVR4nGP4z8CAB+GTG8HSALfKY52fTcuYAAAAAElFTkSuQmCC";

#[tokio::test]
async fn vision_describe_red_pixel() {
    let Some(client) = client() else {
        eprintln!("GOOGLE_API_KEY not set — skipping");
        return;
    };

    let msg = Message::user_with_blocks(vec![
        ContentBlock::Image {
            source: ImageSource::Base64 {
                media_type: "image/png".into(),
                data: RED_PIXEL_PNG.into(),
            },
        },
        ContentBlock::Text {
            text: "What color is this image? Reply in one word.".into(),
        },
    ]);

    let req = ChatRequest::builder().messages(vec![msg]).build();
    let resp = client.chat_with(req).await.expect("chat failed");

    println!("Response: {:?}", resp.content);
    println!("Model: {}", resp.model);
    println!(
        "Tokens: in={} out={}",
        resp.usage.input_tokens, resp.usage.output_tokens
    );

    assert!(!resp.content.is_empty(), "expected non-empty response");
    let lower = resp.content.to_lowercase();
    assert!(
        lower.contains("red") || lower.contains("crimson"),
        "expected color mention, got: {:?}",
        resp.content
    );
}
