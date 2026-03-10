use motosan_ai::{Message, Role, StopReason};

#[test]
fn message_constructors_set_role_and_content() {
    let user = Message::user("hello");
    let assistant = Message::assistant("world");
    let system = Message::system("policy");

    assert!(matches!(user.role, Role::User));
    assert!(matches!(assistant.role, Role::Assistant));
    assert!(matches!(system.role, Role::System));
    assert_eq!(user.content, "hello");
}

#[test]
fn stop_reason_serializes_to_snake_case() {
    let serialized = serde_json::to_string(&StopReason::EndTurn).expect("serialize stop reason");
    assert_eq!(serialized, "\"end_turn\"");
}

