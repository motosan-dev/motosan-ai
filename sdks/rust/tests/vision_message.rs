use motosan_ai::{ContentBlock, ImageSource, Message, Role};

#[test]
fn user_with_image_creates_correct_blocks() {
    let msg = Message::user_with_image("describe this", "base64data", "image/png");
    assert_eq!(msg.role, Role::User);
    assert_eq!(msg.content, "describe this");
    assert_eq!(msg.content_blocks.len(), 2);
    assert!(
        matches!(&msg.content_blocks[0], ContentBlock::Text { text } if text == "describe this")
    );
    assert!(matches!(&msg.content_blocks[1], ContentBlock::Image { .. }));
}

#[test]
fn user_with_blocks_extracts_text() {
    let msg = Message::user_with_blocks(vec![
        ContentBlock::Text {
            text: "hello".to_string(),
        },
        ContentBlock::Image {
            source: ImageSource::Url {
                url: "https://example.com/img.png".to_string(),
            },
        },
    ]);
    assert_eq!(msg.content, "hello");
    assert_eq!(msg.content_blocks.len(), 2);
}

#[test]
fn existing_user_constructor_has_empty_blocks() {
    let msg = Message::user("hello");
    assert!(msg.content_blocks.is_empty());
    assert_eq!(msg.content, "hello");
}

#[test]
fn message_with_blocks_serialization_skips_empty() {
    let msg = Message::user("hello");
    let json = serde_json::to_value(&msg).unwrap();
    // content_blocks should not be in JSON when empty
    assert!(json.get("content_blocks").is_none());
}

#[test]
fn message_with_blocks_serialization_includes_when_present() {
    let msg = Message::user_with_image("describe", "data", "image/png");
    let json = serde_json::to_value(&msg).unwrap();
    assert!(json.get("content_blocks").is_some());
    assert_eq!(json["content_blocks"].as_array().unwrap().len(), 2);
}

#[test]
fn message_roundtrip_with_blocks() {
    let msg = Message::user_with_image("test", "abc", "image/jpeg");
    let json = serde_json::to_string(&msg).unwrap();
    let parsed: Message = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.content_blocks.len(), 2);
    assert_eq!(parsed.content, "test");
}

#[test]
fn message_roundtrip_without_blocks() {
    let msg = Message::user("plain text");
    let json = serde_json::to_string(&msg).unwrap();
    let parsed: Message = serde_json::from_str(&json).unwrap();
    assert!(parsed.content_blocks.is_empty());
    assert_eq!(parsed.content, "plain text");
}
