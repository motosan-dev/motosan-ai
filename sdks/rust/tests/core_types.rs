use motosan_ai::{
    collect_model_stream, ChatRequest, FreeformTool, FreeformToolFormat, FunctionCallOutputPayload,
    Message, ModelChatRequest, ModelChatResponse, ModelContextItem, ModelStreamDelta,
    ModelToolCall, ModelToolOutput, ModelToolSpec, Role, StopReason, Tool, ToolCall, ToolChoice,
    Usage,
};
use tokio_stream::iter;

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
    assert!(user.tool_calls.is_empty());
    assert!(assistant.tool_calls.is_empty());
    assert!(system.tool_calls.is_empty());
    assert!(tool.tool_calls.is_empty());
}

#[test]
fn assistant_with_tool_calls_constructor_sets_values() {
    let tool_calls = vec![ToolCall {
        id: "call_1".to_string(),
        name: "get_weather".to_string(),
        input: serde_json::json!({"city": "Taipei"}),
    }];

    let assistant = Message::assistant_with_tool_calls("Let me check", tool_calls.clone());
    let tool_result = Message::tool_result("call_1", "25°C");

    assert!(matches!(assistant.role, Role::Assistant));
    assert_eq!(assistant.content, "Let me check");
    assert_eq!(assistant.tool_calls, tool_calls);
    assert!(assistant.tool_call_id.is_none());

    assert!(matches!(tool_result.role, Role::Tool));
    assert_eq!(tool_result.tool_call_id.as_deref(), Some("call_1"));
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

fn grammar_fixture() -> FreeformTool {
    FreeformTool {
        name: "exec".to_string(),
        description: "Run JavaScript".to_string(),
        format: FreeformToolFormat {
            r#type: "grammar".to_string(),
            syntax: "lark".to_string(),
            definition: "start: source".to_string(),
        },
    }
}

#[test]
fn freeform_format_is_mandatory_and_exact() {
    let value = serde_json::to_value(grammar_fixture()).expect("serialize freeform tool");

    assert_eq!(
        value,
        serde_json::json!({
            "type": "custom",
            "name": "exec",
            "description": "Run JavaScript",
            "format": {
                "type": "grammar",
                "syntax": "lark",
                "definition": "start: source"
            }
        })
    );

    let missing_format = serde_json::json!({
        "type": "custom",
        "name": "exec",
        "description": "Run JavaScript"
    });
    let err =
        serde_json::from_value::<FreeformTool>(missing_format).expect_err("format is mandatory");
    assert!(
        err.to_string().contains("format"),
        "unexpected error: {err}"
    );
}

#[test]
fn freeform_call_preserves_raw_input() {
    let raw = "const x = {a: 1};\nconsole.log(`raw ${x.a}`);\n";
    let call = ModelToolCall::Freeform {
        id: "call_js".to_string(),
        name: "exec".to_string(),
        input: raw.to_string(),
    };

    let encoded = serde_json::to_string(&call).expect("serialize freeform call");
    let decoded: ModelToolCall = serde_json::from_str(&encoded).expect("deserialize call");

    assert_eq!(decoded, call);
    match decoded {
        ModelToolCall::Freeform { input, .. } => assert_eq!(input.as_bytes(), raw.as_bytes()),
        other => panic!("expected freeform call, got {other:?}"),
    }
}

#[test]
fn custom_output_round_trips_with_call_identity() {
    let output = ModelToolOutput::Custom {
        call_id: "call_js".to_string(),
        name: Some("exec".to_string()),
        output: FunctionCallOutputPayload::Text("stdout: 42".to_string()),
    };

    let encoded = serde_json::to_value(&output).expect("serialize custom output");
    assert_eq!(encoded["type"], "custom_tool_call_output");
    assert_eq!(encoded["call_id"], "call_js");
    assert_eq!(encoded["name"], "exec");
    assert_eq!(encoded["output"], "stdout: 42");

    let decoded: ModelToolOutput =
        serde_json::from_value(encoded).expect("deserialize custom output");
    assert_eq!(decoded, output);
}

#[test]
fn native_context_preserves_mixed_item_order() {
    let request = ModelChatRequest::builder()
        .model("gpt-5.5-codex")
        .context_item(ModelContextItem::Message(Message::user("run it")))
        .context_item(ModelContextItem::ToolCall(ModelToolCall::Freeform {
            id: "call_js".to_string(),
            name: "exec".to_string(),
            input: "console.log(1);".to_string(),
        }))
        .context_item(ModelContextItem::ToolOutput(ModelToolOutput::Custom {
            call_id: "call_js".to_string(),
            name: Some("exec".to_string()),
            output: FunctionCallOutputPayload::Text("1\n".to_string()),
        }))
        .tool_spec(ModelToolSpec::Freeform(grammar_fixture()))
        .build();

    assert_eq!(request.model.as_deref(), Some("gpt-5.5-codex"));
    assert_eq!(request.context.len(), 3);
    assert!(matches!(
        request.context[0],
        ModelContextItem::Message(Message {
            role: Role::User,
            ..
        })
    ));
    assert!(matches!(
        request.context[1],
        ModelContextItem::ToolCall(ModelToolCall::Freeform { .. })
    ));
    assert!(matches!(
        request.context[2],
        ModelContextItem::ToolOutput(ModelToolOutput::Custom { .. })
    ));
}

#[test]
fn native_response_carries_freeform_calls() {
    let response = ModelChatResponse {
        content: String::new(),
        thinking: None,
        tool_calls: vec![ModelToolCall::Freeform {
            id: "call_js".to_string(),
            name: "exec".to_string(),
            input: "console.log(1);".to_string(),
        }],
        model: "gpt-5.5-codex".to_string(),
        usage: Usage {
            input_tokens: 10,
            output_tokens: 5,
            cache_creation_input_tokens: None,
            cache_read_input_tokens: None,
        },
        stop_reason: StopReason::ToolUse,
        session_id: None,
    };

    assert_eq!(response.tool_calls.len(), 1);
    assert!(matches!(
        response.tool_calls[0],
        ModelToolCall::Freeform { .. }
    ));
}

#[test]
fn native_blocking_response_preserves_thinking_separately() {
    let response = ModelChatResponse {
        content: "answer".to_string(),
        thinking: Some("private reasoning".to_string()),
        tool_calls: Vec::new(),
        model: "gpt-5.5-codex".to_string(),
        usage: Usage {
            input_tokens: 1,
            output_tokens: 2,
            cache_creation_input_tokens: None,
            cache_read_input_tokens: None,
        },
        stop_reason: StopReason::EndTurn,
        session_id: None,
    };

    assert_eq!(response.content, "answer");
    assert_eq!(response.thinking.as_deref(), Some("private reasoning"));
}

#[tokio::test]
async fn native_stream_carries_freeform_delta_and_done() {
    let events = vec![
        ModelStreamDelta::FreeformInput {
            call_id: "call_js".to_string(),
            delta: "console.".to_string(),
        },
        ModelStreamDelta::FreeformInput {
            call_id: "call_js".to_string(),
            delta: "log(1);".to_string(),
        },
        ModelStreamDelta::ToolCallDone {
            call: ModelToolCall::Freeform {
                id: "call_js".to_string(),
                name: "exec".to_string(),
                input: "console.log(1);".to_string(),
            },
        },
        ModelStreamDelta::Done {
            stop_reason: StopReason::ToolUse,
        },
    ];
    let stream = Box::pin(iter(events.into_iter().map(Ok)));

    let response = collect_model_stream(stream)
        .await
        .expect("collect model stream");

    assert_eq!(response.stop_reason, StopReason::ToolUse);
    assert_eq!(
        response.tool_calls,
        vec![ModelToolCall::Freeform {
            id: "call_js".to_string(),
            name: "exec".to_string(),
            input: "console.log(1);".to_string(),
        }]
    );
}

#[tokio::test]
async fn native_stream_carries_thinking_delta_and_done() {
    let events = vec![
        ModelStreamDelta::ThinkingDelta {
            delta: "think ".to_string(),
        },
        ModelStreamDelta::ThinkingDelta {
            delta: "hard".to_string(),
        },
        ModelStreamDelta::ThinkingDone {
            thinking: "think hard".to_string(),
        },
        ModelStreamDelta::Text {
            delta: "answer".to_string(),
        },
        ModelStreamDelta::Done {
            stop_reason: StopReason::EndTurn,
        },
    ];
    let stream = Box::pin(iter(events.into_iter().map(Ok)));

    let response = collect_model_stream(stream)
        .await
        .expect("collect model stream");

    assert_eq!(response.content, "answer");
    assert_eq!(response.thinking.as_deref(), Some("think hard"));
}

#[tokio::test]
async fn thinking_done_does_not_duplicate_accumulated_deltas() {
    let events = vec![
        ModelStreamDelta::ThinkingDelta {
            delta: "same".to_string(),
        },
        ModelStreamDelta::ThinkingDone {
            thinking: "same".to_string(),
        },
        ModelStreamDelta::Done {
            stop_reason: StopReason::EndTurn,
        },
    ];
    let stream = Box::pin(iter(events.into_iter().map(Ok)));

    let response = collect_model_stream(stream)
        .await
        .expect("collect model stream");

    assert_eq!(response.thinking.as_deref(), Some("same"));
}

#[test]
fn responses_codec_encodes_function_and_custom_tools() {
    let function = Tool::from(motosan_ai::ToolSchema::new(
        "sum",
        "Add numbers",
        serde_json::json!({"type": "object", "properties": {"a": {"type": "number"}}}),
    ));

    let tools = motosan_ai::providers::responses::encode_tools(&[
        ModelToolSpec::Function(function),
        ModelToolSpec::Freeform(grammar_fixture()),
    ]);

    assert_eq!(tools[0]["type"], "function");
    assert_eq!(tools[0]["name"], "sum");
    assert_eq!(tools[0]["parameters"]["type"], "object");
    assert_eq!(
        tools[1],
        serde_json::json!({
            "type": "custom",
            "name": "exec",
            "description": "Run JavaScript",
            "format": {
                "type": "grammar",
                "syntax": "lark",
                "definition": "start: source"
            }
        })
    );
}

#[test]
fn responses_codec_encodes_function_and_custom_outputs() {
    let input = motosan_ai::providers::responses::encode_input(&[
        ModelContextItem::ToolOutput(ModelToolOutput::Function {
            call_id: "call_fn".to_string(),
            output: FunctionCallOutputPayload::Text("{\"ok\":true}".to_string()),
        }),
        ModelContextItem::ToolOutput(ModelToolOutput::Custom {
            call_id: "call_js".to_string(),
            name: Some("exec".to_string()),
            output: FunctionCallOutputPayload::Text("stdout".to_string()),
        }),
    ]);

    assert_eq!(input[0]["type"], "function_call_output");
    assert_eq!(input[0]["call_id"], "call_fn");
    assert_eq!(input[0]["output"], "{\"ok\":true}");
    assert_eq!(input[1]["type"], "custom_tool_call_output");
    assert_eq!(input[1]["call_id"], "call_js");
    assert!(input[1].get("name").is_none());
    assert_eq!(input[1]["output"], "stdout");
}

#[test]
fn responses_codec_encodes_user_content_blocks_as_input_image() {
    let input = motosan_ai::providers::responses::encode_input(&[ModelContextItem::Message(
        Message::user_with_image("inspect", "abc123", "image/png"),
    )]);

    let content = input[0]["content"]
        .as_array()
        .expect("message content array");
    assert_eq!(content[0]["type"], "input_text");
    assert_eq!(content[0]["text"], "inspect");
    assert_eq!(content[1]["type"], "input_image");
    assert_eq!(content[1]["image_url"], "data:image/png;base64,abc123");
}

#[test]
fn responses_codec_encodes_native_tool_choice_and_stop_sequences() {
    let body = motosan_ai::providers::responses::build_model_request_body(
        &ModelChatRequest::builder()
            .context_item(ModelContextItem::Message(Message::user("hi")))
            .tool_choice(ToolChoice::Required)
            .stop("END")
            .build(),
        "gpt-test",
        false,
        None,
    );

    assert_eq!(body["tool_choice"], "required");
    assert_eq!(body["stop"], serde_json::json!(["END"]));

    let forced = motosan_ai::providers::responses::build_model_request_body(
        &ModelChatRequest::builder()
            .context_item(ModelContextItem::Message(Message::user("hi")))
            .tool_choice(ToolChoice::Tool {
                name: "run_js".to_string(),
            })
            .build(),
        "gpt-test",
        false,
        None,
    );

    assert_eq!(
        forced["tool_choice"],
        serde_json::json!({"type": "function", "name": "run_js"})
    );
}

#[test]
fn responses_codec_preserves_mixed_ordered_history() {
    let raw = "const value = {\"not\":\"function args\"};\nvalue.not;\n";
    let input = motosan_ai::providers::responses::encode_input(&[
        ModelContextItem::Message(Message::user("run js")),
        ModelContextItem::ToolCall(ModelToolCall::Function {
            id: "call_fn".to_string(),
            name: "sum".to_string(),
            arguments: "{\"a\":1}".to_string(),
        }),
        ModelContextItem::ToolOutput(ModelToolOutput::Function {
            call_id: "call_fn".to_string(),
            output: FunctionCallOutputPayload::Text("1".to_string()),
        }),
        ModelContextItem::ToolCall(ModelToolCall::Freeform {
            id: "call_js".to_string(),
            name: "exec".to_string(),
            input: raw.to_string(),
        }),
        ModelContextItem::ToolOutput(ModelToolOutput::Custom {
            call_id: "call_js".to_string(),
            name: Some("exec".to_string()),
            output: FunctionCallOutputPayload::Text("function args".to_string()),
        }),
    ]);

    let types: Vec<_> = input
        .iter()
        .map(|item| item["type"].as_str().unwrap())
        .collect();
    assert_eq!(
        types,
        vec![
            "message",
            "function_call",
            "function_call_output",
            "custom_tool_call",
            "custom_tool_call_output"
        ]
    );
    assert_eq!(
        input[3]["input"].as_str().unwrap().as_bytes(),
        raw.as_bytes()
    );
    assert!(input[3].get("arguments").is_none());
}

#[test]
fn responses_codec_decodes_function_and_custom_calls() {
    let function = serde_json::json!({
        "type": "function_call",
        "call_id": "call_fn",
        "name": "sum",
        "arguments": "{\"a\":1}"
    });
    let custom = serde_json::json!({
        "type": "custom_tool_call",
        "call_id": "call_js",
        "name": "exec",
        "input": "const a = {raw: true};\n"
    });

    assert_eq!(
        motosan_ai::providers::responses::decode_tool_call(&function),
        Some(ModelToolCall::Function {
            id: "call_fn".to_string(),
            name: "sum".to_string(),
            arguments: "{\"a\":1}".to_string(),
        })
    );
    assert_eq!(
        motosan_ai::providers::responses::decode_tool_call(&custom),
        Some(ModelToolCall::Freeform {
            id: "call_js".to_string(),
            name: "exec".to_string(),
            input: "const a = {raw: true};\n".to_string(),
        })
    );
}

#[test]
fn responses_codec_preserves_raw_custom_input_without_json_parsing() {
    let raw = "{\"this\":\"looks like json\"}\nconsole.log('but is JavaScript');";
    let item = serde_json::json!({
        "type": "custom_tool_call",
        "call_id": "call_js",
        "name": "exec",
        "input": raw
    });

    let decoded =
        motosan_ai::providers::responses::decode_tool_call(&item).expect("decode custom call");

    match decoded {
        ModelToolCall::Freeform { input, .. } => assert_eq!(input.as_bytes(), raw.as_bytes()),
        other => panic!("expected freeform call, got {other:?}"),
    }
}
