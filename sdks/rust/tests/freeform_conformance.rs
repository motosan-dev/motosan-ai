#![cfg(all(feature = "openai", feature = "chatgpt-codex"))]

//! Freeform parity conformance gates (specs/types.md § Native Model API).
//!
//! Cross-SDK mirrors:
//! - sdks/python/tests/test_freeform_conformance.py
//! - sdks/typescript/tests/freeform-conformance.test.ts
//!
//! Rust already implements every rule asserted here; the file exists because a
//! cross-SDK gate that skips one SDK is not a gate (milestone D9). It adds no
//! source changes and no version bump.
//!
//! # Proving this suite still bites
//!
//! A conformance suite passes by construction the day it is written, so
//! passing says nothing. Re-prove it after any refactor of the native surface
//! by making each mutation below in turn, running
//! `cargo test --all-features --test freeform_conformance`, and confirming the
//! named test fails — then reverting. Each was verified against this file as
//! merged.
//!
//! 1. `src/providers/responses.rs` — truncate the payload
//!    `"{} ended without a terminal event"` to `"{} ended"`.
//!    Fails: `openai_eof_without_terminal_yields_the_exact_incomplete_payload`
//!    and `chatgpt_codex_eof_without_terminal_yields_the_exact_incomplete_payload`.
//! 2. `src/providers/responses.rs` `build_model_request_body` — move the
//!    `provider_options` merge loop above the `temperature` assignment, so it
//!    no longer merges last.
//!    Fails: `max_tokens_maps_to_max_output_tokens_and_provider_options_merge_last`.
//! 3. `src/providers/chatgpt_codex.rs` `build_model_responses_body` — delete
//!    the block that removes a stray top-level `reasoning_effort` key.
//!    Fails: `codex_reasoning_effort_never_reaches_the_wire_and_per_request_wins`.
//!
//! Deleting a whole module and watching compilation fail is NOT such a check:
//! a file with zero assertions fails identically.

use mockito::Matcher;
use motosan_ai::providers::chatgpt_codex::ChatGptCodexProvider;
use motosan_ai::providers::openai::OpenAIProvider;
use motosan_ai::providers::responses::build_model_request_body;
use motosan_ai::providers::ProviderImpl;
use motosan_ai::{
    collect_model_stream, FreeformTool, FreeformToolFormat, FunctionCallOutputPayload, Message,
    ModelChatRequest, ModelContextItem, ModelStreamDelta, ModelToolCall, ModelToolOutput,
    ModelToolSpec, MotosanError, StopReason, Tool, ToolChoice, ToolSchema, Usage,
};
use serde_json::json;
use tokio_stream::{iter, StreamExt};

/// The Rust fixture for "looks like JSON but is JavaScript".
const JS_THAT_LOOKS_LIKE_JSON: &str = "{\"this\":\"looks like json\"}\nconsole.log('but is JS');";

fn exec_tool() -> FreeformTool {
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

fn freeform_request() -> ModelChatRequest {
    ModelChatRequest::builder()
        .context_item(ModelContextItem::Message(Message::user("run js")))
        .tool_spec(ModelToolSpec::Freeform(exec_tool()))
        .build()
}

// ---------------------------------------------------------------------------
// Tool definitions
// ---------------------------------------------------------------------------

#[test]
fn freeform_tool_serializes_with_a_mandatory_exact_format_object() {
    let req = ModelChatRequest::builder()
        .tool_spec(ModelToolSpec::Freeform(exec_tool()))
        .build();
    let body = build_model_request_body(&req, "m", false, None);

    assert_eq!(
        body["tools"],
        json!([{
            "type": "custom",
            "name": "exec",
            "description": "Run JavaScript",
            "format": {"type": "grammar", "syntax": "lark", "definition": "start: source"}
        }])
    );
}

#[test]
fn function_tool_serializes_input_schema_under_parameters() {
    let req = ModelChatRequest::builder()
        .tool_spec(ModelToolSpec::Function(Tool::from(ToolSchema::new(
            "get_weather",
            "Fetch the weather",
            json!({"type": "object"}),
        ))))
        .build();
    let body = build_model_request_body(&req, "m", false, None);

    assert_eq!(
        body["tools"],
        json!([{
            "type": "function",
            "name": "get_weather",
            "description": "Fetch the weather",
            "parameters": {"type": "object"}
        }])
    );
}

// ---------------------------------------------------------------------------
// Ordered history replay
// ---------------------------------------------------------------------------

#[test]
fn ordered_history_replays_byte_exact_and_in_order() {
    let req = ModelChatRequest::builder()
        .context_item(ModelContextItem::Message(Message::user("run js")))
        .context_item(ModelContextItem::ToolCall(ModelToolCall::Freeform {
            id: "call_js".to_string(),
            name: "exec".to_string(),
            input: JS_THAT_LOOKS_LIKE_JSON.to_string(),
        }))
        .context_item(ModelContextItem::ToolOutput(ModelToolOutput::Custom {
            call_id: "call_js".to_string(),
            name: Some("exec".to_string()),
            output: FunctionCallOutputPayload::Text("done".to_string()),
        }))
        .tool_spec(ModelToolSpec::Freeform(exec_tool()))
        .build();
    let body = build_model_request_body(&req, "gpt-5.5-codex", false, None);
    let input = body["input"].as_array().expect("input is an array");

    let types: Vec<&str> = input
        .iter()
        .map(|item| item["type"].as_str().unwrap_or_default())
        .collect();
    assert_eq!(
        types,
        vec!["message", "custom_tool_call", "custom_tool_call_output"]
    );

    // Byte-for-byte: never parsed as JSON, never lowered into `arguments`.
    assert_eq!(input[1]["input"], json!(JS_THAT_LOOKS_LIKE_JSON));
    assert!(input[1].get("arguments").is_none());
    // Identity travels under `call_id`, not `id`.
    assert_eq!(input[1]["call_id"], json!("call_js"));
    assert!(input[1].get("id").is_none());
    assert_eq!(input[2]["call_id"], json!("call_js"));
}

#[test]
fn system_messages_are_hoisted_into_instructions_and_removed_from_input() {
    let req = ModelChatRequest::builder()
        .context_item(ModelContextItem::Message(Message::system("be terse")))
        .context_item(ModelContextItem::Message(Message::user("hi")))
        .build();
    let body = build_model_request_body(&req, "m", false, None);

    assert_eq!(body["instructions"], json!("be terse"));
    let input = body["input"].as_array().expect("input is an array");
    assert_eq!(input.len(), 1);
    assert_eq!(input[0]["role"], json!("user"));
}

#[test]
fn max_tokens_maps_to_max_output_tokens_and_provider_options_merge_last() {
    let req = ModelChatRequest::builder()
        .max_tokens(512)
        .temperature(0.1)
        .tool_choice(ToolChoice::Required)
        .provider_options(json!({"temperature": 0.9}))
        .build();
    let body = build_model_request_body(&req, "m", false, None);

    assert_eq!(body["max_output_tokens"], json!(512));
    assert!(body.get("max_tokens").is_none());
    assert_eq!(body["tool_choice"], json!("required"));
    assert_eq!(body["temperature"], json!(0.9));
}

// ---------------------------------------------------------------------------
// Pre-network rejection
// ---------------------------------------------------------------------------

#[tokio::test]
async fn openai_without_the_responses_opt_in_rejects_freeform_before_http() {
    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("POST", Matcher::Any)
        .expect(0)
        .with_status(500)
        .create_async()
        .await;

    let provider = OpenAIProvider::new("test-key", None)
        .with_chat_url(format!("{}/v1/chat/completions", server.url()))
        .with_responses_url(format!("{}/v1/responses", server.url()));

    let chat_err = provider
        .model_chat(freeform_request())
        .await
        .expect_err("native freeform must be rejected");
    assert!(matches!(chat_err, MotosanError::UnsupportedFeature(msg) if msg.contains("freeform")));

    let stream_err = match provider.model_stream(freeform_request()).await {
        Ok(_) => panic!("native freeform streams must be rejected"),
        Err(err) => err,
    };
    assert!(
        matches!(stream_err, MotosanError::UnsupportedFeature(msg) if msg.contains("freeform"))
    );

    mock.assert_async().await;
}

// ---------------------------------------------------------------------------
// Stream termination
// ---------------------------------------------------------------------------

#[tokio::test]
async fn exactly_one_done_per_successfully_completed_stream() {
    let mut server = mockito::Server::new_async().await;
    let sse = concat!(
        "data: {\"type\":\"response.output_text.delta\",\"delta\":\"hi\"}\n\n",
        "data: {\"type\":\"response.completed\",\"response\":{\"status\":\"completed\"}}\n\n",
        "data: {\"type\":\"response.output_text.delta\",\"delta\":\"trailing\"}\n\n"
    );
    server
        .mock("POST", "/v1/responses")
        .with_status(200)
        .with_header("content-type", "text/event-stream")
        .with_body(sse)
        .create_async()
        .await;

    let provider = OpenAIProvider::new("test-key", None)
        .with_responses_api(true)
        .with_responses_url(format!("{}/v1/responses", server.url()));

    let mut stream = provider
        .model_stream(freeform_request())
        .await
        .expect("native stream");
    let mut dones = 0;
    while let Some(item) = stream.next().await {
        if matches!(
            item.expect("no stream error"),
            ModelStreamDelta::Done { .. }
        ) {
            dones += 1;
        }
    }
    assert_eq!(dones, 1, "exactly one terminal Done per completed stream");
}

#[tokio::test]
async fn openai_eof_without_terminal_yields_the_exact_incomplete_payload() {
    let mut server = mockito::Server::new_async().await;
    let sse = concat!(
        "data: {\"type\":\"response.output_text.delta\",\"delta\":\"hel\"}\n\n",
        "data: {\"type\":\"response.output_text.delta\",\"delta\":\"lo\"}\n\n"
    );
    server
        .mock("POST", "/v1/responses")
        .with_status(200)
        .with_header("content-type", "text/event-stream")
        .with_body(sse)
        .create_async()
        .await;

    let provider = OpenAIProvider::new("test-key", None)
        .with_responses_api(true)
        .with_responses_url(format!("{}/v1/responses", server.url()));

    let mut stream = provider
        .model_stream(freeform_request())
        .await
        .expect("native stream");
    let mut saw_done = false;
    let mut last_err = None;
    while let Some(item) = stream.next().await {
        match item {
            Ok(delta) => saw_done |= matches!(delta, ModelStreamDelta::Done { .. }),
            Err(err) => {
                last_err = Some(err);
                break;
            }
        }
    }

    assert!(!saw_done, "no Done may be fabricated on truncation");
    match last_err.expect("EOF without a terminal must yield an error") {
        MotosanError::IncompleteStream(msg) => {
            assert_eq!(msg, "openai ended without a terminal event")
        }
        other => panic!("expected IncompleteStream, got {other:?}"),
    }
}

#[tokio::test]
async fn chatgpt_codex_eof_without_terminal_yields_the_exact_incomplete_payload() {
    let mut server = mockito::Server::new_async().await;
    let sse = "data: {\"type\":\"response.custom_tool_call_input.delta\",\"call_id\":\"call_js\",\"delta\":\"console.\"}\n\n";
    server
        .mock("POST", Matcher::Any)
        .with_status(200)
        .with_header("content-type", "text/event-stream")
        .with_body(sse)
        .create_async()
        .await;

    let provider =
        ChatGptCodexProvider::new("oauth-token", "acct-123", "gpt-5.5", Some(server.url()));
    let stream = provider
        .model_stream(freeform_request())
        .await
        .expect("native stream");

    match collect_model_stream(stream)
        .await
        .expect_err("EOF without a terminal must yield IncompleteStream")
    {
        MotosanError::IncompleteStream(msg) => {
            assert_eq!(msg, "chatgpt-codex ended without a terminal event")
        }
        other => panic!("expected IncompleteStream, got {other:?}"),
    }
}

#[tokio::test]
async fn response_incomplete_is_a_terminal_that_maps_to_max_tokens() {
    let mut server = mockito::Server::new_async().await;
    let sse = concat!(
        "data: {\"type\":\"response.output_text.delta\",\"delta\":\"partial\"}\n\n",
        "data: {\"type\":\"response.incomplete\",\"response\":{\"status\":\"incomplete\",\"usage\":{\"input_tokens\":6,\"output_tokens\":7}}}\n\n"
    );
    server
        .mock("POST", Matcher::Any)
        .with_status(200)
        .with_header("content-type", "text/event-stream")
        .with_body(sse)
        .create_async()
        .await;

    let provider =
        ChatGptCodexProvider::new("oauth-token", "acct-123", "gpt-5.5", Some(server.url()));
    let response = provider
        .model_chat(
            ModelChatRequest::builder()
                .context_item(ModelContextItem::Message(Message::user("short")))
                .build(),
        )
        .await
        .expect("native chat");

    assert_eq!(response.content, "partial");
    assert_eq!(response.stop_reason, StopReason::MaxTokens);
    assert_eq!(response.usage.output_tokens, 7);
}

// ---------------------------------------------------------------------------
// Collector rules
// ---------------------------------------------------------------------------

#[tokio::test]
async fn tool_call_done_is_authoritative_over_accumulated_freeform_deltas() {
    let stream = Box::pin(iter(
        vec![
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
                    input: "AUTHORITATIVE".to_string(),
                },
            },
            ModelStreamDelta::Done {
                stop_reason: StopReason::ToolUse,
            },
        ]
        .into_iter()
        .map(Ok),
    ));

    let response = collect_model_stream(stream).await.expect("collect");
    assert_eq!(
        response.tool_calls,
        vec![ModelToolCall::Freeform {
            id: "call_js".to_string(),
            name: "exec".to_string(),
            input: "AUTHORITATIVE".to_string(),
        }]
    );
    assert_eq!(response.stop_reason, StopReason::ToolUse);
}

#[tokio::test]
async fn usage_replaces_rather_than_merges() {
    let stream = Box::pin(iter(
        vec![
            ModelStreamDelta::Usage {
                usage: Usage {
                    input_tokens: 100,
                    output_tokens: 100,
                    cache_creation_input_tokens: None,
                    cache_read_input_tokens: None,
                },
            },
            ModelStreamDelta::Usage {
                usage: Usage {
                    input_tokens: 1,
                    output_tokens: 2,
                    cache_creation_input_tokens: None,
                    cache_read_input_tokens: None,
                },
            },
            ModelStreamDelta::Done {
                stop_reason: StopReason::EndTurn,
            },
        ]
        .into_iter()
        .map(Ok),
    ));

    let response = collect_model_stream(stream).await.expect("collect");
    assert_eq!(response.usage.input_tokens, 1);
    assert_eq!(response.usage.output_tokens, 2);
}

#[tokio::test]
async fn thinking_done_wins_over_accumulated_thinking_deltas() {
    let stream = Box::pin(iter(
        vec![
            ModelStreamDelta::ThinkingDelta {
                delta: "think ".to_string(),
            },
            ModelStreamDelta::ThinkingDelta {
                delta: "hard".to_string(),
            },
            ModelStreamDelta::ThinkingDone {
                thinking: "AUTHORITATIVE".to_string(),
            },
            ModelStreamDelta::Text {
                delta: "answer".to_string(),
            },
            ModelStreamDelta::Done {
                stop_reason: StopReason::EndTurn,
            },
        ]
        .into_iter()
        .map(Ok),
    ));

    let response = collect_model_stream(stream).await.expect("collect");
    assert_eq!(response.thinking.as_deref(), Some("AUTHORITATIVE"));
    assert_eq!(response.content, "answer");
}

#[tokio::test]
async fn freeform_input_survives_the_whole_stream_byte_for_byte() {
    let mut server = mockito::Server::new_async().await;
    let item = json!({
        "type": "response.output_item.done",
        "item": {
            "type": "custom_tool_call",
            "call_id": "call_js",
            "name": "exec",
            "input": JS_THAT_LOOKS_LIKE_JSON
        }
    });
    let sse = format!(
        "data: {item}\n\ndata: {{\"type\":\"response.completed\",\"response\":{{\"status\":\"completed\"}}}}\n\n"
    );
    server
        .mock("POST", "/v1/responses")
        .with_status(200)
        .with_header("content-type", "text/event-stream")
        .with_body(sse)
        .create_async()
        .await;

    let provider = OpenAIProvider::new("test-key", None)
        .with_responses_api(true)
        .with_responses_url(format!("{}/v1/responses", server.url()));
    let stream = provider
        .model_stream(freeform_request())
        .await
        .expect("native stream");
    let response = collect_model_stream(stream).await.expect("collect");

    assert_eq!(
        response.tool_calls,
        vec![ModelToolCall::Freeform {
            id: "call_js".to_string(),
            name: "exec".to_string(),
            input: JS_THAT_LOOKS_LIKE_JSON.to_string(),
        }]
    );
}

// ---------------------------------------------------------------------------
// Codex body normalization
// ---------------------------------------------------------------------------

#[test]
fn codex_reasoning_effort_never_reaches_the_wire_and_per_request_wins() {
    let provider = ChatGptCodexProvider::new("test-token", "acct-123", "gpt-5.5", None)
        .with_reasoning_effort(Some("low".to_string()));
    let req = ModelChatRequest::builder()
        .context_item(ModelContextItem::Message(Message::user("hi")))
        .provider_options(json!({"reasoning_effort": "high"}))
        .build();
    let body = provider.build_model_responses_body(&req);

    assert_eq!(
        body["reasoning"],
        json!({"effort": "high", "summary": "auto"})
    );
    assert!(
        body.get("reasoning_effort").is_none(),
        "the raw reasoning_effort key must never reach the wire"
    );
}

#[test]
fn codex_hard_sets_its_body_fields_over_the_caller() {
    let provider = ChatGptCodexProvider::new("test-token", "acct-123", "gpt-5.5", None);
    let req = ModelChatRequest::builder()
        .context_item(ModelContextItem::Message(Message::user("hi")))
        .tool_choice(ToolChoice::Required)
        .build();
    let body = provider.build_model_responses_body(&req);

    assert_eq!(body["store"], json!(false));
    assert_eq!(body["stream"], json!(true));
    assert_eq!(body["include"], json!(["reasoning.encrypted_content"]));
    assert_eq!(body["parallel_tool_calls"], json!(true));
    assert_eq!(body["tool_choice"], json!("auto"));
    assert_eq!(body["instructions"], json!("You are a helpful assistant."));
}
