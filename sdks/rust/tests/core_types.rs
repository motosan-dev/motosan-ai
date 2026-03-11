use motosan_ai::{ChatRequest, Message, Role, StopReason};

#[test]
fn message_constructors_set_role_and_content() {
    let user = Message::user("hello");
    let assistant = Message::assistant("world");
    let system = Message::system("policy");
    let tool = Message::tool("{}", "call_1");

    assert!(matches!(user.role, Role::User));
    assert!(matches!(assistant.role, Role::Assistant));
    assert!(matches!(system.role, Role::System));
    assert!(matches!(tool.role, Role::Tool));
    assert_eq!(user.content, "hello");
    assert_eq!(tool.tool_call_id.as_deref(), Some("call_1"));
}

#[test]
fn stop_reason_serializes_to_snake_case() {
    let serialized = serde_json::to_string(&StopReason::EndTurn).expect("serialize stop reason");
    assert_eq!(serialized, "\"end_turn\"");
}

#[test]
fn chat_request_builder_populates_optional_fields() {
    let request = ChatRequest::builder()
        .message(Message::user("hello"))
        .system("rules")
        .model("gpt-4o")
        .temperature(0.2)
        .max_tokens(128)
        .provider_options(serde_json::json!({"reasoning_effort": "low"}))
        .build();

    assert_eq!(request.messages.len(), 1);
    assert_eq!(request.system.as_deref(), Some("rules"));
    assert_eq!(request.model.as_deref(), Some("gpt-4o"));
    assert_eq!(request.temperature, Some(0.2));
    assert_eq!(request.max_tokens, Some(128));
    assert!(request.provider_options.is_some());
}
