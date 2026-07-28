#![cfg(feature = "openai")]

use mockito::Matcher;
use motosan_ai::providers::openai::{OpenAIAuthStyle, OpenAIProvider};
use motosan_ai::providers::ProviderImpl;
use motosan_ai::{
    collect_model_stream, ChatRequest, FreeformTool, FreeformToolFormat, FunctionCallOutputPayload,
    Message, ModelChatRequest, ModelContextItem, ModelToolCall, ModelToolOutput, ModelToolSpec,
    MotosanError, RetryPolicy, StopReason, StreamEventType, Tool, DEFAULT_OPENAI_MODEL,
};
use serde_json::json;
use tokio_stream::StreamExt;

fn freeform_exec_tool() -> ModelToolSpec {
    ModelToolSpec::Freeform(FreeformTool {
        name: "exec".to_string(),
        description: "Run JavaScript".to_string(),
        format: FreeformToolFormat {
            r#type: "grammar".to_string(),
            syntax: "lark".to_string(),
            definition: "start: source".to_string(),
        },
    })
}

fn native_custom_request() -> ModelChatRequest {
    ModelChatRequest::builder()
        .model("gpt-5.5-codex")
        .context_item(ModelContextItem::Message(Message::user("run js")))
        .tool_spec(freeform_exec_tool())
        .build()
}

#[tokio::test]
async fn native_custom_openai_responses_request_encodes_custom_grammar() {
    let mut server = mockito::Server::new_async().await;
    let raw = "const x = {a: 1};\nconsole.log(x.a);\n";
    let mock = server
        .mock("POST", "/v1/responses")
        .match_header("authorization", "Bearer test-key")
        .match_body(Matcher::AllOf(vec![
            Matcher::Regex(r#""tools"\s*:"#.to_string()),
            Matcher::Regex(r#""type"\s*:\s*"custom""#.to_string()),
            Matcher::Regex(r#""format"\s*:"#.to_string()),
            Matcher::Regex(r#""syntax"\s*:\s*"lark""#.to_string()),
            Matcher::Regex(r#""definition"\s*:\s*"start: source""#.to_string()),
        ]))
        .with_status(200)
        .with_body(
            json!({
                "model": "gpt-5.5-codex",
                "status": "completed",
                "output": [{
                    "type": "custom_tool_call",
                    "call_id": "call_js",
                    "name": "exec",
                    "input": raw
                }],
                "usage": {"input_tokens": 9, "output_tokens": 7}
            })
            .to_string(),
        )
        .create_async()
        .await;

    let provider = OpenAIProvider::new("test-key", None)
        .with_responses_api(true)
        .with_responses_url(format!("{}/v1/responses", server.url()));

    let response = provider
        .model_chat(native_custom_request())
        .await
        .expect("native response");

    assert_eq!(
        response.tool_calls,
        vec![ModelToolCall::Freeform {
            id: "call_js".to_string(),
            name: "exec".to_string(),
            input: raw.to_string(),
        }]
    );
    assert_eq!(response.stop_reason, StopReason::ToolUse);
    assert_eq!(response.usage.input_tokens, 9);
    mock.assert_async().await;
}

#[tokio::test]
async fn native_openai_responses_request_encodes_image_blocks() {
    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("POST", "/v1/responses")
        .match_body(Matcher::AllOf(vec![
            Matcher::Regex(r#""type"\s*:\s*"input_text""#.to_string()),
            Matcher::Regex(r#""text"\s*:\s*"inspect""#.to_string()),
            Matcher::Regex(r#""type"\s*:\s*"input_image""#.to_string()),
            Matcher::Regex(r#""image_url"\s*:\s*"data:image/png;base64,abc123""#.to_string()),
        ]))
        .with_status(200)
        .with_body(
            json!({
                "model": "gpt-5.5-codex",
                "status": "completed",
                "output": [{
                    "type": "message",
                    "role": "assistant",
                    "content": [{"type": "output_text", "text": "ok"}]
                }],
                "usage": {"input_tokens": 1, "output_tokens": 1}
            })
            .to_string(),
        )
        .create_async()
        .await;

    let provider = OpenAIProvider::new("test-key", None)
        .with_responses_api(true)
        .with_responses_url(format!("{}/v1/responses", server.url()));
    let request = ModelChatRequest::builder()
        .context_item(ModelContextItem::Message(Message::user_with_image(
            "inspect",
            "abc123",
            "image/png",
        )))
        .build();

    let response = provider.model_chat(request).await.expect("native response");

    assert_eq!(response.content, "ok");
    mock.assert_async().await;
}

#[tokio::test]
async fn native_custom_openai_chat_completions_rejects_before_http() {
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

    let err = provider
        .model_chat(native_custom_request())
        .await
        .expect_err("chat completions must reject native freeform tools");

    assert!(matches!(err, MotosanError::UnsupportedFeature(msg) if msg.contains("freeform")));
    mock.assert_async().await;
}

#[tokio::test]
async fn native_custom_openai_chat_completions_stream_rejects_before_http() {
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

    let err = match provider.model_stream(native_custom_request()).await {
        Ok(_) => panic!("chat completions must reject native freeform streams"),
        Err(err) => err,
    };

    assert!(matches!(err, MotosanError::UnsupportedFeature(msg) if msg.contains("freeform")));
    mock.assert_async().await;
}

#[tokio::test]
async fn native_custom_openai_replays_symmetric_history_byte_exact() {
    let mut server = mockito::Server::new_async().await;
    let raw = "{\"this\":\"looks like json\"}\nconsole.log('but is JS');";
    let mock = server
        .mock("POST", "/v1/responses")
        .match_body(Matcher::AllOf(vec![
            Matcher::Regex(r#""type"\s*:\s*"custom_tool_call""#.to_string()),
            Matcher::Regex(r#""type"\s*:\s*"custom_tool_call_output""#.to_string()),
            Matcher::Regex(r#""input"\s*:\s*"\{\\"this\\":\\"looks like json\\"\}\\nconsole\.log\('but is JS'\);"#.to_string()),
            Matcher::Regex(r#""call_id"\s*:\s*"call_js""#.to_string()),
            Matcher::Regex(r#""name"\s*:\s*"exec""#.to_string()),
        ]))
        .with_status(200)
        .with_body(
            json!({
                "model": "gpt-5.5-codex",
                "status": "completed",
                "output": [{
                    "type": "message",
                    "role": "assistant",
                    "content": [{"type": "output_text", "text": "ok"}]
                }],
                "usage": {"input_tokens": 1, "output_tokens": 1}
            })
            .to_string(),
        )
        .create_async()
        .await;

    let provider = OpenAIProvider::new("test-key", None)
        .with_responses_api(true)
        .with_responses_url(format!("{}/v1/responses", server.url()));
    let request = ModelChatRequest::builder()
        .context_item(ModelContextItem::Message(Message::user("run js")))
        .context_item(ModelContextItem::ToolCall(ModelToolCall::Freeform {
            id: "call_js".to_string(),
            name: "exec".to_string(),
            input: raw.to_string(),
        }))
        .context_item(ModelContextItem::ToolOutput(ModelToolOutput::Custom {
            call_id: "call_js".to_string(),
            name: Some("exec".to_string()),
            output: FunctionCallOutputPayload::Text("done".to_string()),
        }))
        .tool_spec(freeform_exec_tool())
        .build();

    let response = provider.model_chat(request).await.expect("native response");

    assert_eq!(response.content, "ok");
    mock.assert_async().await;
}

#[tokio::test]
async fn native_custom_openai_stream_decodes_custom_delta_and_done() {
    let mut server = mockito::Server::new_async().await;
    let raw = "console.log(1);\n";
    let sse = concat!(
        "data: {\"type\":\"response.custom_tool_call_input.delta\",\"call_id\":\"call_js\",\"delta\":\"console.\"}\n\n",
        "data: {\"type\":\"response.custom_tool_call_input.delta\",\"call_id\":\"call_js\",\"delta\":\"log(1);\\n\"}\n\n",
        "data: {\"type\":\"response.output_item.done\",\"item\":{\"type\":\"custom_tool_call\",\"call_id\":\"call_js\",\"name\":\"exec\",\"input\":\"console.log(1);\\n\"}}\n\n",
        "data: {\"type\":\"response.completed\",\"response\":{\"status\":\"completed\",\"usage\":{\"input_tokens\":2,\"output_tokens\":3}}}\n\n"
    );
    let mock = server
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
        .model_stream(native_custom_request())
        .await
        .expect("native stream");
    let response = collect_model_stream(stream)
        .await
        .expect("collect native stream");

    assert_eq!(
        response.tool_calls,
        vec![ModelToolCall::Freeform {
            id: "call_js".to_string(),
            name: "exec".to_string(),
            input: raw.to_string(),
        }]
    );
    assert_eq!(response.stop_reason, StopReason::ToolUse);
    assert_eq!(response.usage.output_tokens, 3);
    mock.assert_async().await;
}

#[tokio::test]
async fn native_openai_stream_eof_without_terminal_is_incomplete() {
    let mut server = mockito::Server::new_async().await;
    // Text deltas but NO response.completed / response.incomplete → truncated.
    let sse = concat!(
        "data: {\"type\":\"response.output_text.delta\",\"delta\":\"hel\"}\n\n",
        "data: {\"type\":\"response.output_text.delta\",\"delta\":\"lo\"}\n\n"
    );
    let mock = server
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
        .model_stream(native_custom_request())
        .await
        .expect("native stream");
    let err = collect_model_stream(stream)
        .await
        .expect_err("EOF without terminal must yield IncompleteStream");
    match err {
        MotosanError::IncompleteStream(msg) => {
            assert_eq!(msg, "openai ended without a terminal event")
        }
        other => panic!("expected IncompleteStream, got {other:?}"),
    }
    mock.assert_async().await;
}

#[tokio::test]
async fn openai_chat_maps_response() {
    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("POST", "/v1/chat/completions")
        .match_header("authorization", "Bearer test-key")
        .with_status(200)
        .with_body(
            json!({
                "model": "gpt-4o",
                "choices": [{"message": {"content": "hello from openai"}, "finish_reason": "stop"}],
                "usage": {"prompt_tokens": 12, "completion_tokens": 8}
            })
            .to_string(),
        )
        .create_async()
        .await;

    let provider = OpenAIProvider::new("test-key", None)
        .with_chat_url(format!("{}/v1/chat/completions", server.url()));
    let request = ChatRequest::builder()
        .message(Message::system("rules"))
        .message(Message::user("hello"))
        .temperature(0.1)
        .max_tokens(50)
        .build();

    let response = provider.chat(request).await.expect("chat response");
    assert_eq!(response.content, "hello from openai");
    assert_eq!(response.model, "gpt-4o");
    assert_eq!(response.usage.input_tokens, 12);
    assert!(matches!(response.stop_reason, StopReason::Stop));

    mock.assert_async().await;
}

#[tokio::test]
async fn openai_request_uses_default_model_and_allows_override() {
    let mut server = mockito::Server::new_async().await;

    let default_mock = server
        .mock("POST", "/v1/chat/completions")
        .match_header("authorization", "Bearer test-key")
        .match_body(Matcher::Regex(format!(
            r#"\"model\"\s*:\s*\"{}\""#,
            DEFAULT_OPENAI_MODEL
        )))
        .with_status(200)
        .with_body(
            json!({
                "model": DEFAULT_OPENAI_MODEL,
                "choices": [{"message": {"content": "ok"}, "finish_reason": "stop"}],
                "usage": {"prompt_tokens": 1, "completion_tokens": 1}
            })
            .to_string(),
        )
        .create_async()
        .await;

    let provider = OpenAIProvider::new("test-key", None)
        .with_chat_url(format!("{}/v1/chat/completions", server.url()));
    let request = ChatRequest::builder()
        .message(Message::user("hello"))
        .build();
    let _ = provider.chat(request).await.expect("chat response");
    default_mock.assert_async().await;

    server.reset();
    let override_model = "gpt-4o";
    let override_mock = server
        .mock("POST", "/v1/chat/completions")
        .match_header("authorization", "Bearer test-key")
        .match_body(Matcher::Regex(format!(
            r#"\"model\"\s*:\s*\"{}\""#,
            override_model
        )))
        .with_status(200)
        .with_body(
            json!({
                "model": override_model,
                "choices": [{"message": {"content": "ok"}, "finish_reason": "stop"}],
                "usage": {"prompt_tokens": 1, "completion_tokens": 1}
            })
            .to_string(),
        )
        .create_async()
        .await;

    let request = ChatRequest::builder()
        .message(Message::user("hello"))
        .model(override_model)
        .build();
    let _ = provider.chat(request).await.expect("chat response");
    override_mock.assert_async().await;
}

#[tokio::test]
async fn openai_stream_emits_deltas_and_done() {
    let mut server = mockito::Server::new_async().await;
    let sse_body = concat!(
        "data: {\"choices\":[{\"delta\":{\"content\":\"hel\"}}]}\n\n",
        "data: {\"choices\":[{\"delta\":{\"content\":\"lo\"}}]}\n\n",
        "data: [DONE]\n\n"
    );

    let mock = server
        .mock("POST", "/v1/chat/completions")
        .match_header("authorization", "Bearer test-key")
        .with_status(200)
        .with_header("content-type", "text/event-stream")
        .with_body(sse_body)
        .create_async()
        .await;

    let provider = OpenAIProvider::new("test-key", None)
        .with_chat_url(format!("{}/v1/chat/completions", server.url()));
    let request = ChatRequest::builder()
        .message(Message::user("hello"))
        .build();

    let mut stream = provider.stream(request).await.expect("stream response");
    let mut events = Vec::new();

    while let Some(event_item) = stream.next().await {
        let event = event_item.expect("stream item should not fail");
        events.push(event);
    }

    assert_eq!(events.len(), 3);
    assert_eq!(events[0].content, "hel");
    assert_eq!(events[1].content, "lo");
    assert!(events[2].done);

    mock.assert_async().await;
}

#[tokio::test]
async fn openai_stream_ignores_malformed_chunks() {
    let mut server = mockito::Server::new_async().await;
    let sse_body = concat!(
        "data: not-json\n\n",
        "data: {\"choices\":[{\"delta\":{\"content\":\"ok\"}}]}\n\n",
        "data: [DONE]\n\n"
    );

    let mock = server
        .mock("POST", "/v1/chat/completions")
        .match_header("authorization", "Bearer test-key")
        .with_status(200)
        .with_header("content-type", "text/event-stream")
        .with_body(sse_body)
        .create_async()
        .await;

    let provider = OpenAIProvider::new("test-key", None)
        .with_chat_url(format!("{}/v1/chat/completions", server.url()));
    let request = ChatRequest::builder()
        .message(Message::user("hello"))
        .build();

    let mut stream = provider.stream(request).await.expect("stream response");
    let mut events = Vec::new();

    while let Some(event_item) = stream.next().await {
        let event = event_item.expect("stream item should not fail");
        events.push(event);
    }

    assert_eq!(events.len(), 2);
    assert_eq!(events[0].content, "ok");
    assert!(!events[0].done);
    assert!(events[1].done);

    mock.assert_async().await;
}

#[tokio::test]
async fn openai_with_chat_url_trims_trailing_slash() {
    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("POST", "/v1/chat/completions")
        .match_header("authorization", "Bearer test-key")
        .with_status(200)
        .with_body(
            json!({
                "model": "gpt-4o",
                "choices": [{"message": {"content": "ok"}, "finish_reason": "stop"}],
                "usage": {"prompt_tokens": 1, "completion_tokens": 1}
            })
            .to_string(),
        )
        .create_async()
        .await;

    // Trailing slash on the URL is trimmed defensively — should still hit /v1/chat/completions.
    let provider = OpenAIProvider::new("test-key", None)
        .with_chat_url(format!("{}/v1/chat/completions/", server.url()));
    let request = ChatRequest::builder()
        .message(Message::user("hello"))
        .build();

    let response = provider.chat(request).await.expect("chat response");
    assert_eq!(response.content, "ok");
    mock.assert_async().await;
}

#[tokio::test]
async fn openai_chat_falls_back_to_reasoning_content_when_content_empty() {
    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("POST", "/v1/chat/completions")
        .match_header("authorization", "Bearer test-key")
        .with_status(200)
        .with_body(
            json!({
                "model": "gpt-5.3-codex",
                "choices": [{"message": {"content": "", "reasoning_content": "fallback"}, "finish_reason": "stop"}],
                "usage": {"prompt_tokens": 1, "completion_tokens": 1}
            })
            .to_string(),
        )
        .create_async()
        .await;

    let provider = OpenAIProvider::new("test-key", None)
        .with_chat_url(format!("{}/v1/chat/completions", server.url()));
    let request = ChatRequest::builder()
        .message(Message::user("hello"))
        .build();

    let response = provider.chat(request).await.expect("chat response");
    assert_eq!(response.content, "fallback");
    mock.assert_async().await;
}

#[tokio::test]
async fn openai_stream_falls_back_to_reasoning_content_and_skips_empty_chunks() {
    let mut server = mockito::Server::new_async().await;
    let sse_body = concat!(
        "data: {\"choices\":[{\"delta\":{\"content\":\"\"}}]}\n\n",
        "data: {\"choices\":[{\"delta\":{\"reasoning_content\":\"thinking fallback\"}}]}\n\n",
        "data: [DONE]\n\n"
    );

    let mock = server
        .mock("POST", "/v1/chat/completions")
        .match_header("authorization", "Bearer test-key")
        .with_status(200)
        .with_header("content-type", "text/event-stream")
        .with_body(sse_body)
        .create_async()
        .await;

    let provider = OpenAIProvider::new("test-key", None)
        .with_chat_url(format!("{}/v1/chat/completions", server.url()));
    let request = ChatRequest::builder()
        .message(Message::user("hello"))
        .build();

    let mut stream = provider.stream(request).await.expect("stream response");
    let mut events = Vec::new();
    while let Some(event_item) = stream.next().await {
        let event = event_item.expect("stream item should not fail");
        events.push(event);
    }

    assert_eq!(events.len(), 2);
    assert_eq!(events[0].content, "thinking fallback");
    assert!(events[1].done);
    mock.assert_async().await;
}

#[tokio::test]
async fn openai_stream_maps_structured_error_payload() {
    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("POST", "/v1/chat/completions")
        .match_header("authorization", "Bearer test-key")
        .with_status(401)
        .with_body(json!({"error": {"message": "bad key"}}).to_string())
        .create_async()
        .await;

    let provider = OpenAIProvider::new("test-key", None)
        .with_chat_url(format!("{}/v1/chat/completions", server.url()));
    let request = ChatRequest::builder()
        .message(Message::user("hello"))
        .build();

    let result = provider.stream(request).await;
    assert!(matches!(result, Err(MotosanError::Auth { .. })));
    mock.assert_async().await;
}

#[tokio::test]
async fn openai_auth_style_x_api_key_is_supported() {
    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("POST", "/v1/chat/completions")
        .match_header("x-api-key", "test-key")
        .with_status(200)
        .with_body(
            json!({
                "model": "gpt-4o",
                "choices": [{"message": {"content": "ok"}, "finish_reason": "stop"}],
                "usage": {"prompt_tokens": 1, "completion_tokens": 1}
            })
            .to_string(),
        )
        .create_async()
        .await;

    let provider = OpenAIProvider::new("test-key", None)
        .with_chat_url(format!("{}/v1/chat/completions", server.url()))
        .with_auth_style(OpenAIAuthStyle::XApiKey);
    let request = ChatRequest::builder()
        .message(Message::user("hello"))
        .build();

    let response = provider.chat(request).await.expect("chat response");
    assert_eq!(response.content, "ok");
    mock.assert_async().await;
}

#[tokio::test]
async fn openai_chat_can_fallback_to_responses_api_on_404() {
    let mut server = mockito::Server::new_async().await;
    server
        .mock("POST", "/v1/chat/completions")
        .match_header("authorization", "Bearer test-key")
        .with_status(404)
        .with_body(json!({"error": {"message": "not found"}}).to_string())
        .create_async()
        .await;

    let responses_mock = server
        .mock("POST", "/v1/responses")
        .match_header("authorization", "Bearer test-key")
        .with_status(200)
        .with_body(
            json!({
                "model": "gpt-5.3-codex",
                "output_text": "fallback response",
                "usage": {"input_tokens": 5, "output_tokens": 3}
            })
            .to_string(),
        )
        .create_async()
        .await;

    let provider = OpenAIProvider::new("test-key", None)
        .with_chat_url(format!("{}/v1/chat/completions", server.url()))
        .with_responses_url(format!("{}/v1/responses", server.url()))
        .with_responses_fallback(true);
    let request = ChatRequest::builder()
        .message(Message::user("hello"))
        .build();

    let response = provider.chat(request).await.expect("chat response");
    assert_eq!(response.content, "fallback response");
    assert_eq!(response.usage.input_tokens, 5);
    assert_eq!(response.usage.output_tokens, 3);
    responses_mock.assert_async().await;
}

#[tokio::test]
async fn openai_responses_fallback_non_json_error_preserves_http_metadata() {
    let mut server = mockito::Server::new_async().await;
    server
        .mock("POST", "/v1/chat/completions")
        .match_header("authorization", "Bearer test-key")
        .with_status(404)
        .with_body(json!({"error": {"message": "not found"}}).to_string())
        .create_async()
        .await;

    let responses_mock = server
        .mock("POST", "/v1/responses")
        .match_header("authorization", "Bearer test-key")
        .with_status(503)
        .with_header("retry-after", "7")
        .with_header("request-id", "resp_req")
        .with_body("upstream unavailable")
        .create_async()
        .await;

    let provider = OpenAIProvider::new("test-key", None)
        .with_chat_url(format!("{}/v1/chat/completions", server.url()))
        .with_responses_url(format!("{}/v1/responses", server.url()))
        .with_retry_policy(RetryPolicy::new().max_retries(0))
        .with_responses_fallback(true);
    let request = ChatRequest::builder()
        .message(Message::user("hello"))
        .build();

    let err = provider
        .chat(request)
        .await
        .expect_err("non-JSON Responses API 503 should fail");
    assert!(matches!(err, MotosanError::ProviderError { .. }));
    assert_eq!(err.status_code(), Some(503));
    assert_eq!(err.retry_after(), Some(std::time::Duration::from_secs(7)));
    assert_eq!(err.request_id(), Some("resp_req"));
    assert_eq!(
        err.to_string(),
        "provider error: openai responses request failed"
    );
    responses_mock.assert_async().await;
}

#[tokio::test]
async fn openai_stream_emits_tool_call_events() {
    let mut server = mockito::Server::new_async().await;
    let sse_body = concat!(
        "data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"call_1\",\"function\":{\"name\":\"get_weather\",\"arguments\":\"\"}}]},\"finish_reason\":null}]}\n\n",
        "data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"function\":{\"arguments\":\"{\\\"city\\\":\"}}]},\"finish_reason\":null}]}\n\n",
        "data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"function\":{\"arguments\":\"\\\"Taipei\\\"}\"}}]},\"finish_reason\":null}]}\n\n",
        "data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"tool_calls\"}]}\n\n"
    );

    let mock = server
        .mock("POST", "/v1/chat/completions")
        .match_header("authorization", "Bearer test-key")
        .with_status(200)
        .with_header("content-type", "text/event-stream")
        .with_body(sse_body)
        .create_async()
        .await;

    let provider = OpenAIProvider::new("test-key", None)
        .with_chat_url(format!("{}/v1/chat/completions", server.url()));
    let request = ChatRequest::builder()
        .message(Message::user("weather in Taipei?"))
        .tools(vec![Tool {
            schema: motosan_agent_primitives::ToolSchema {
                name: "get_weather".to_string(),
                description: "Get weather".to_string(),
                input_schema: json!({"type": "object", "properties": {"city": {"type": "string"}}}),
            },
            cache: false,
        }])
        .build();

    let mut stream = provider.stream(request).await.expect("stream response");
    let mut received = Vec::new();
    while let Some(event_item) = stream.next().await {
        let event = event_item.expect("stream item should not fail");
        received.push(event);
    }

    // Tool call start
    let starts: Vec<_> = received
        .iter()
        .filter(|e| e.event_type == StreamEventType::ToolCallStart)
        .collect();
    assert_eq!(starts.len(), 1);
    assert_eq!(starts[0].tool_call_id.as_deref(), Some("call_1"));
    assert_eq!(starts[0].tool_call_name.as_deref(), Some("get_weather"));

    // Tool call args
    let args_events: Vec<_> = received
        .iter()
        .filter(|e| e.event_type == StreamEventType::ToolCallArgs)
        .collect();
    let full_args: String = args_events
        .iter()
        .filter_map(|e| e.tool_call_args_delta.as_deref())
        .collect();
    assert_eq!(full_args, "{\"city\":\"Taipei\"}");

    // Tool call end
    let ends: Vec<_> = received
        .iter()
        .filter(|e| e.event_type == StreamEventType::ToolCallEnd)
        .collect();
    assert_eq!(ends.len(), 1);

    // Done
    assert!(received.last().unwrap().done);

    mock.assert_async().await;
}

#[tokio::test]
async fn openai_stream_parallel_tool_calls_interleaved_stay_sequential_per_call() {
    let mut server = mockito::Server::new_async().await;
    let sse_body = concat!(
        "data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"call_A\",\"function\":{\"name\":\"get_weather\",\"arguments\":\"\"}}]},\"finish_reason\":null}]}\n\n",
        "data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":1,\"id\":\"call_B\",\"function\":{\"name\":\"get_time\",\"arguments\":\"\"}}]},\"finish_reason\":null}]}\n\n",
        "data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"function\":{\"arguments\":\"{\\\"city\\\":\"}}]},\"finish_reason\":null}]}\n\n",
        "data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":1,\"function\":{\"arguments\":\"{\\\"tz\\\":\\\"Asia/Tokyo\\\"}\"}}]},\"finish_reason\":null}]}\n\n",
        "data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"function\":{\"arguments\":\"\\\"Taipei\\\"}\"}}]},\"finish_reason\":null}]}\n\n",
        "data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"tool_calls\"}]}\n\n",
        "data: [DONE]\n\n"
    );

    let mock = server
        .mock("POST", "/v1/chat/completions")
        .match_header("authorization", "Bearer test-key")
        .with_status(200)
        .with_header("content-type", "text/event-stream")
        .with_body(sse_body)
        .create_async()
        .await;

    let provider = OpenAIProvider::new("test-key", None)
        .with_chat_url(format!("{}/v1/chat/completions", server.url()));
    let request = ChatRequest::builder()
        .message(Message::user("weather in Taipei and time in Tokyo?"))
        .tools(vec![
            Tool {
                schema: motosan_agent_primitives::ToolSchema {
                    name: "get_weather".to_string(),
                    description: "Get weather".to_string(),
                    input_schema: json!({
                        "type": "object",
                        "properties": {"city": {"type": "string"}}
                    }),
                },
                cache: false,
            },
            Tool {
                schema: motosan_agent_primitives::ToolSchema {
                    name: "get_time".to_string(),
                    description: "Get local time".to_string(),
                    input_schema: json!({
                        "type": "object",
                        "properties": {"tz": {"type": "string"}}
                    }),
                },
                cache: false,
            },
        ])
        .build();

    let mut stream = provider.stream(request).await.expect("stream response");
    let mut received = Vec::new();
    while let Some(event_item) = stream.next().await {
        let event = event_item.expect("stream item should not fail");
        received.push(event);
    }

    let tool_sequence: Vec<_> = received
        .iter()
        .filter(|event| {
            matches!(
                event.event_type,
                StreamEventType::ToolCallStart
                    | StreamEventType::ToolCallArgs
                    | StreamEventType::ToolCallEnd
            )
        })
        .map(|event| {
            (
                event.event_type.clone(),
                event.tool_call_id.as_deref(),
                event.tool_call_name.as_deref(),
                event.tool_call_args_delta.as_deref(),
            )
        })
        .collect();

    assert_eq!(
        tool_sequence,
        vec![
            (
                StreamEventType::ToolCallStart,
                Some("call_A"),
                Some("get_weather"),
                None
            ),
            (
                StreamEventType::ToolCallArgs,
                Some("call_A"),
                None,
                Some("{\"city\":")
            ),
            (
                StreamEventType::ToolCallArgs,
                Some("call_A"),
                None,
                Some("\"Taipei\"}")
            ),
            (StreamEventType::ToolCallEnd, Some("call_A"), None, None),
            (
                StreamEventType::ToolCallStart,
                Some("call_B"),
                Some("get_time"),
                None
            ),
            (
                StreamEventType::ToolCallArgs,
                Some("call_B"),
                None,
                Some("{\"tz\":\"Asia/Tokyo\"}")
            ),
            (StreamEventType::ToolCallEnd, Some("call_B"), None, None),
        ]
    );

    let collected_stream = Box::pin(tokio_stream::iter(
        received.into_iter().map(Ok::<_, MotosanError>),
    ));
    let response = motosan_ai::collect_stream(collected_stream)
        .await
        .expect("collect stream");

    assert_eq!(response.tool_calls.len(), 2);
    assert_eq!(response.tool_calls[0].id, "call_A");
    assert_eq!(response.tool_calls[0].name, "get_weather");
    assert_eq!(response.tool_calls[0].input, json!({"city": "Taipei"}));
    assert_eq!(response.tool_calls[1].id, "call_B");
    assert_eq!(response.tool_calls[1].name, "get_time");
    assert_eq!(response.tool_calls[1].input, json!({"tz": "Asia/Tokyo"}));
    assert_eq!(response.stop_reason, StopReason::ToolUse);

    mock.assert_async().await;
}

#[tokio::test]
async fn openai_stream_propagates_finish_reason_max_tokens() {
    let mut server = mockito::Server::new_async().await;
    let sse_body = concat!(
        "data: {\"choices\":[{\"delta\":{\"content\":\"truncated\"},\"finish_reason\":null}]}\n\n",
        "data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"length\"}]}\n\n",
        "data: [DONE]\n\n"
    );

    let mock = server
        .mock("POST", "/v1/chat/completions")
        .match_header("authorization", "Bearer test-key")
        .with_status(200)
        .with_header("content-type", "text/event-stream")
        .with_body(sse_body)
        .create_async()
        .await;

    let provider = OpenAIProvider::new("test-key", None)
        .with_chat_url(format!("{}/v1/chat/completions", server.url()));
    let request = ChatRequest::builder()
        .message(Message::user("write a long story"))
        .build();

    let mut stream = provider.stream(request).await.expect("stream response");
    let mut events = Vec::new();
    while let Some(event_item) = stream.next().await {
        let event = event_item.expect("stream item should not fail");
        events.push(event);
    }

    // Find the first done event and assert it carries MaxTokens.
    let first_done = events
        .iter()
        .find(|e| e.done)
        .expect("expected a terminal done event");
    assert_eq!(first_done.stop_reason, Some(StopReason::MaxTokens));

    mock.assert_async().await;
}

#[tokio::test]
async fn openai_stream_propagates_finish_reason_tool_calls() {
    let mut server = mockito::Server::new_async().await;
    let sse_body = concat!(
        "data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"call_1\",\"function\":{\"name\":\"lookup\",\"arguments\":\"\"}}]},\"finish_reason\":null}]}\n\n",
        "data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"function\":{\"arguments\":\"{}\"}}]},\"finish_reason\":null}]}\n\n",
        "data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"tool_calls\"}]}\n\n",
        "data: [DONE]\n\n"
    );

    let mock = server
        .mock("POST", "/v1/chat/completions")
        .match_header("authorization", "Bearer test-key")
        .with_status(200)
        .with_header("content-type", "text/event-stream")
        .with_body(sse_body)
        .create_async()
        .await;

    let provider = OpenAIProvider::new("test-key", None)
        .with_chat_url(format!("{}/v1/chat/completions", server.url()));
    let request = ChatRequest::builder()
        .message(Message::user("call a tool"))
        .build();

    let mut stream = provider.stream(request).await.expect("stream response");
    let mut events = Vec::new();
    while let Some(event_item) = stream.next().await {
        let event = event_item.expect("stream item should not fail");
        events.push(event);
    }

    let first_done = events
        .iter()
        .find(|e| e.done)
        .expect("expected a terminal done event");
    assert_eq!(first_done.stop_reason, Some(StopReason::ToolUse));

    mock.assert_async().await;
}

#[tokio::test]
async fn openai_stream_propagates_finish_reason_stop() {
    let mut server = mockito::Server::new_async().await;
    let sse_body = concat!(
        "data: {\"choices\":[{\"delta\":{\"content\":\"done.\"},\"finish_reason\":null}]}\n\n",
        "data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}]}\n\n",
        "data: [DONE]\n\n"
    );

    let mock = server
        .mock("POST", "/v1/chat/completions")
        .match_header("authorization", "Bearer test-key")
        .with_status(200)
        .with_header("content-type", "text/event-stream")
        .with_body(sse_body)
        .create_async()
        .await;

    let provider = OpenAIProvider::new("test-key", None)
        .with_chat_url(format!("{}/v1/chat/completions", server.url()));
    let request = ChatRequest::builder().message(Message::user("hi")).build();

    let mut stream = provider.stream(request).await.expect("stream response");
    let mut events = Vec::new();
    while let Some(event_item) = stream.next().await {
        let event = event_item.expect("stream item should not fail");
        events.push(event);
    }

    let done = events.iter().find(|e| e.done).expect("done event");
    assert_eq!(done.stop_reason, Some(StopReason::Stop));
    // Exactly one done event, even with [DONE] sentinel present.
    assert_eq!(events.iter().filter(|e| e.done).count(), 1);

    mock.assert_async().await;
}

#[tokio::test]
async fn openai_stream_finish_reason_then_eof_completes_with_stop_reason() {
    // Amended M3 rule (2026-07-17): `finish_reason` is the SEMANTIC
    // terminal event; `[DONE]` is only the transport epilogue. EOF after a
    // finish_reason chunk is a COMPLETE stream — the adapter emits done
    // carrying the stashed stop_reason, NOT Err(IncompleteStream).
    // (Adjusted survival of the pre-0.24
    // `openai_stream_eof_flush_when_done_sentinel_missing` pin.)
    let mut server = mockito::Server::new_async().await;
    let sse_body = concat!(
        "data: {\"choices\":[{\"delta\":{\"content\":\"hello\"},\"finish_reason\":null}]}\n\n",
        "data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"length\"}]}\n\n" // no [DONE]
    );

    server
        .mock("POST", "/v1/chat/completions")
        .with_status(200)
        .with_header("content-type", "text/event-stream")
        .with_body(sse_body)
        .create_async()
        .await;

    let provider = OpenAIProvider::new("test-key", None)
        .with_chat_url(format!("{}/v1/chat/completions", server.url()));
    let request = ChatRequest::builder()
        .message(Message::user("hello"))
        .build();

    let mut stream = provider.stream(request).await.expect("stream response");
    let mut events = Vec::new();
    while let Some(item) = stream.next().await {
        events.push(item.expect("finish_reason-terminated stream must not error"));
    }

    let done = events
        .iter()
        .find(|e| e.done)
        .expect("EOF after finish_reason emits the terminal done");
    assert_eq!(done.stop_reason, Some(StopReason::MaxTokens));
    assert_eq!(events.iter().filter(|e| e.done).count(), 1);
    assert_eq!(events[0].content, "hello");
}

#[tokio::test]
async fn openai_stream_eof_without_terminal_yields_incomplete_stream() {
    // Flip of pre-0.24 `openai_stream_emits_done_on_eof_without_finish_reason_or_done_sentinel`.
    let mut server = mockito::Server::new_async().await;
    let sse_body = "data: {\"choices\":[{\"delta\":{\"content\":\"hello\"}}]}\n\n";

    server
        .mock("POST", "/v1/chat/completions")
        .with_status(200)
        .with_header("content-type", "text/event-stream")
        .with_body(sse_body)
        .create_async()
        .await;

    let provider = OpenAIProvider::new("test-key", None)
        .with_chat_url(format!("{}/v1/chat/completions", server.url()));
    let request = ChatRequest::builder()
        .message(Message::user("hello"))
        .build();

    let mut stream = provider.stream(request).await.expect("stream response");
    let mut text = String::new();
    let mut last_err = None;
    while let Some(item) = stream.next().await {
        match item {
            Ok(ev) => text.push_str(&ev.content),
            Err(e) => {
                last_err = Some(e);
                break;
            }
        }
    }
    assert_eq!(text, "hello", "deltas before truncation still arrive");
    match last_err.expect("EOF without terminal must yield an error") {
        MotosanError::IncompleteStream(msg) => {
            assert_eq!(msg, "openai ended without a terminal event")
        }
        other => panic!("expected IncompleteStream, got {other:?}"),
    }
}

#[tokio::test]
async fn openai_stream_done_count_is_exactly_one_when_done_sentinel_present() {
    // Regression test for the historical double-done bug: even with a
    // finish_reason chunk AND the [DONE] sentinel, the stream should emit
    // exactly one terminal done event (not two).
    let mut server = mockito::Server::new_async().await;
    let sse_body = concat!(
        "data: {\"choices\":[{\"delta\":{\"content\":\"ok\"}}]}\n\n",
        "data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}]}\n\n",
        "data: [DONE]\n\n"
    );

    let mock = server
        .mock("POST", "/v1/chat/completions")
        .match_header("authorization", "Bearer test-key")
        .with_status(200)
        .with_header("content-type", "text/event-stream")
        .with_body(sse_body)
        .create_async()
        .await;

    let provider = OpenAIProvider::new("test-key", None)
        .with_chat_url(format!("{}/v1/chat/completions", server.url()));
    let request = ChatRequest::builder().message(Message::user("hi")).build();

    let mut stream = provider.stream(request).await.expect("stream response");
    let mut events = Vec::new();
    while let Some(event_item) = stream.next().await {
        let event = event_item.expect("stream item should not fail");
        events.push(event);
    }

    let done_count = events.iter().filter(|e| e.done).count();
    assert_eq!(
        done_count, 1,
        "expected exactly one done event, got {done_count}"
    );
    let done = events.iter().find(|e| e.done).unwrap();
    assert_eq!(done.stop_reason, Some(StopReason::Stop));

    mock.assert_async().await;
}
