use motosan_ai::{ContentBlock, ImageSource};

#[test]
fn content_block_text_serialization() {
    let block = ContentBlock::Text {
        text: "hello".to_string(),
    };
    let json = serde_json::to_value(&block).unwrap();
    assert_eq!(json["type"], "text");
    assert_eq!(json["text"], "hello");
}

#[test]
fn content_block_image_base64_serialization() {
    let block = ContentBlock::Image {
        source: ImageSource::Base64 {
            media_type: "image/png".to_string(),
            data: "iVBOR...".to_string(),
        },
    };
    let json = serde_json::to_value(&block).unwrap();
    assert_eq!(json["type"], "image");
    assert_eq!(json["source"]["type"], "base64");
    assert_eq!(json["source"]["media_type"], "image/png");
}

#[test]
fn content_block_image_url_serialization() {
    let block = ContentBlock::Image {
        source: ImageSource::Url {
            url: "https://example.com/image.png".to_string(),
        },
    };
    let json = serde_json::to_value(&block).unwrap();
    assert_eq!(json["type"], "image");
    assert_eq!(json["source"]["type"], "url");
    assert_eq!(json["source"]["url"], "https://example.com/image.png");
}

#[test]
fn content_block_roundtrip() {
    let block = ContentBlock::Text {
        text: "test".to_string(),
    };
    let json = serde_json::to_string(&block).unwrap();
    let parsed: ContentBlock = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, block);
}

#[test]
fn image_source_roundtrip() {
    let source = ImageSource::Base64 {
        media_type: "image/jpeg".to_string(),
        data: "abc123".to_string(),
    };
    let json = serde_json::to_string(&source).unwrap();
    let parsed: ImageSource = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, source);
}
