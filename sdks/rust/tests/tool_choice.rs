use motosan_ai::{ChatRequest, Message, Tool, ToolChoice};

#[test]
fn tool_choice_auto_serializes_with_tagged_type() {
    let tc = ToolChoice::Auto;
    let json = serde_json::to_value(&tc).unwrap();
    assert_eq!(json, serde_json::json!({"type": "auto"}));
}

#[test]
fn tool_choice_required_serializes_with_tagged_type() {
    let tc = ToolChoice::Required;
    let json = serde_json::to_value(&tc).unwrap();
    assert_eq!(json, serde_json::json!({"type": "required"}));
}

#[test]
fn tool_choice_none_serializes_with_tagged_type() {
    let tc = ToolChoice::None;
    let json = serde_json::to_value(&tc).unwrap();
    assert_eq!(json, serde_json::json!({"type": "none"}));
}

#[test]
fn tool_choice_tool_serializes_with_name() {
    let tc = ToolChoice::Tool {
        name: "get_weather".to_string(),
    };
    let json = serde_json::to_value(&tc).unwrap();
    assert_eq!(
        json,
        serde_json::json!({"type": "tool", "name": "get_weather"})
    );
}

#[test]
fn tool_choice_roundtrip_serde() {
    let variants = vec![
        ToolChoice::Auto,
        ToolChoice::Required,
        ToolChoice::None,
        ToolChoice::Tool {
            name: "search".to_string(),
        },
    ];
    for variant in variants {
        let json = serde_json::to_string(&variant).unwrap();
        let deserialized: ToolChoice = serde_json::from_str(&json).unwrap();
        assert_eq!(variant, deserialized);
    }
}

#[test]
fn chat_request_builder_sets_tool_choice() {
    let request = ChatRequest::builder()
        .message(Message::user("hello"))
        .tool_choice(ToolChoice::Required)
        .build();

    assert_eq!(request.tool_choice, Some(ToolChoice::Required));
}

#[test]
fn chat_request_tool_choice_defaults_to_none_option() {
    let request = ChatRequest::builder()
        .message(Message::user("hello"))
        .build();

    assert!(request.tool_choice.is_none());
}

#[test]
fn chat_request_serialization_omits_null_tool_choice() {
    let request = ChatRequest::builder()
        .message(Message::user("hello"))
        .build();

    let json = serde_json::to_value(&request).unwrap();
    assert!(json.get("tool_choice").is_none());
}

#[test]
fn chat_request_serialization_includes_tool_choice_when_set() {
    let request = ChatRequest::builder()
        .message(Message::user("hello"))
        .tools(vec![Tool {
            name: "get_weather".to_string(),
            description: Some("Get weather".to_string()),
            input_schema: None,
        }])
        .tool_choice(ToolChoice::Tool {
            name: "get_weather".to_string(),
        })
        .build();

    let json = serde_json::to_value(&request).unwrap();
    let tc = json.get("tool_choice").expect("tool_choice should exist");
    assert_eq!(tc["type"], "tool");
    assert_eq!(tc["name"], "get_weather");
}
