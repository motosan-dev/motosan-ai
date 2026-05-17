#![cfg(feature = "ollama")]

use mockito::Matcher;
use motosan_ai::{Client, Message, Provider};

#[tokio::test]
async fn ollama_with_keep_alive_routes_to_api_chat_endpoint() {
    // With ollama_keep_alive set, the client must POST to /api/chat
    // (native) rather than /v1/chat/completions (OpenAI-compat), because
    // the OpenAI-compat endpoint silently drops keep_alive server-side.
    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("POST", "/api/chat")
        .match_body(Matcher::Regex(
            r#"\"keep_alive\"\s*:\s*\"10m\""#.to_string(),
        ))
        .with_status(200)
        .with_body(
            serde_json::json!({
                "model": "llama3",
                "message": {"role": "assistant", "content": "ok"},
                "done": true,
                "done_reason": "stop",
                "prompt_eval_count": 1,
                "eval_count": 1
            })
            .to_string(),
        )
        .create_async()
        .await;

    let client = Client::builder()
        .provider(Provider::Ollama)
        .api_key("ollama")
        .ollama_base_url(server.url())
        .ollama_keep_alive("10m")
        .build()
        .expect("build client");

    let _ = client
        .chat(vec![Message::user("hi")])
        .await
        .expect("chat against mock should succeed");
    mock.assert_async().await;
}

#[tokio::test]
async fn ollama_with_num_ctx_streams_from_api_chat_endpoint() {
    use tokio_stream::StreamExt;

    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("POST", "/api/chat")
        .match_body(Matcher::Regex(
            r#"\"options\"\s*:\s*\{[^}]*\"num_ctx\"\s*:\s*4096"#.to_string(),
        ))
        .match_body(Matcher::Regex(r#"\"stream\"\s*:\s*true"#.to_string()))
        .with_status(200)
        // Minimal NDJSON: one chunk + a done marker.
        .with_body(
            "{\"model\":\"llama3\",\"message\":{\"role\":\"assistant\",\"content\":\"hi\"},\"done\":false}\n\
             {\"model\":\"llama3\",\"message\":{\"role\":\"assistant\",\"content\":\"\"},\"done\":true,\"done_reason\":\"stop\",\"prompt_eval_count\":1,\"eval_count\":1}\n",
        )
        .create_async()
        .await;

    let client = Client::builder()
        .provider(Provider::Ollama)
        .api_key("ollama")
        .ollama_base_url(server.url())
        .ollama_num_ctx(4096)
        .build()
        .expect("build client");

    let mut stream = client
        .stream(vec![Message::user("hi")])
        .await
        .expect("stream against mock should open");
    let mut seen_text = false;
    while let Some(event) = stream.next().await {
        if !event.content.is_empty() {
            seen_text = true;
        }
    }
    assert!(seen_text, "stream should yield at least one text chunk");
    mock.assert_async().await;
}

#[tokio::test]
async fn ollama_without_tuning_fields_stays_on_openai_compat_endpoint() {
    // Regression guard: callers who don't set any of the 3 tuning fields
    // and don't enable ollama_native(true) should continue to hit the
    // OpenAI-compat /v1/chat/completions endpoint as in 0.14.x. This
    // preserves backwards compatibility for the common case.
    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("POST", "/v1/chat/completions")
        .with_status(200)
        .with_body(
            serde_json::json!({
                "model": "llama3",
                "choices": [{
                    "message": {"role": "assistant", "content": "ok"},
                    "finish_reason": "stop"
                }],
                "usage": {"prompt_tokens": 1, "completion_tokens": 1}
            })
            .to_string(),
        )
        .create_async()
        .await;

    let client = Client::builder()
        .provider(Provider::Ollama)
        .api_key("ollama")
        .ollama_base_url(server.url())
        .build()
        .expect("build client");

    let _ = client
        .chat(vec![Message::user("hi")])
        .await
        .expect("chat should succeed on the openai-compat fallback");
    mock.assert_async().await;
}

#[tokio::test]
async fn ollama_with_tuning_field_plus_image_returns_wrapped_error() {
    // When the auto-switch fires AND the request has image content, the
    // text-only OllamaProvider's validate_request rejects it. The
    // dispatch arm wraps that rejection with the auto-switch context so
    // the caller knows WHY images stopped working.
    use motosan_ai::MotosanError;

    // No mockito server needed — validate_request fires before any HTTP
    // call, so the request never leaves the client.
    let client = Client::builder()
        .provider(Provider::Ollama)
        .api_key("ollama")
        .ollama_base_url("http://example.invalid")
        .ollama_keep_alive("5m") // triggers auto-switch
        .build()
        .expect("build client");

    let request = motosan_ai::ChatRequest::builder()
        .message(Message::user_with_image(
            "describe this",
            "abc123",
            "image/png",
        ))
        .build();

    let err = client
        .chat_with(request)
        .await
        .expect_err("validate_request should reject image on text-only OllamaProvider");

    match err {
        MotosanError::UnsupportedFeature(msg) => {
            assert!(
                msg.contains("auto-routed") && msg.contains("text-only"),
                "wrapped error should explain the auto-switch context, got: {msg}"
            );
            assert!(
                msg.contains("ollama_keep_alive")
                    || msg.contains("ollama_num_ctx")
                    || msg.contains("ollama_think"),
                "wrapped error should mention the field that triggered the switch, got: {msg}"
            );
        }
        other => panic!("expected UnsupportedFeature, got: {other:?}"),
    }
}

#[tokio::test]
#[ignore] // Requires `ollama serve` running on localhost:11434 with at least one model pulled.
async fn live_ollama_auto_switch_against_real_server() {
    // Real end-to-end verification of the §3 fix. The mockito tests above
    // only prove motosan-ai's request shape is correct; this one proves
    // a real Ollama server actually accepts the request and returns text
    // — i.e. the auto-switched /api/chat path works against the live
    // binary, not just our mocks.
    //
    // Scope honesty: this test proves "Ollama accepts the request and
    // responds non-empty" — it does NOT prove Ollama HONORS keep_alive
    // and num_ctx in observable ways (verifying those would need server
    // logs / process inspection). For that level of verification, run
    // this with `OLLAMA_VERBOSE_LOGS=1 ollama serve` in another terminal
    // and watch the server log lines for the request body.
    //
    // To run:
    //   cargo test --features ollama --test ollama_http_autoswitch \
    //     live_ollama_auto_switch_against_real_server -- --ignored --nocapture
    //
    // Configuration:
    //   OLLAMA_MODEL    — REQUIRED. Name of any chat model you have
    //                     pulled (`ollama list` to check, `ollama pull
    //                     <model>` to add). No default because pulled
    //                     models vary by machine — defaulting to a
    //                     specific tag would silently fail for most
    //                     readers.
    //   OLLAMA_BASE_URL — optional, defaults to http://localhost:11434
    //
    // Forensic note: manual run on 2026-05-17 against `llama3.1:8b`
    // confirmed `num_ctx=512` was actually honored by the server (log
    // line: `llama_context: n_ctx_seq (512) < n_ctx_train (131072)`).
    // Re-verify the same way if the routing logic changes.

    let model = std::env::var("OLLAMA_MODEL").expect(
        "set OLLAMA_MODEL to any chat model you have pulled — \
         run `ollama list` to see what's available",
    );
    let base_url =
        std::env::var("OLLAMA_BASE_URL").unwrap_or_else(|_| "http://localhost:11434".to_string());

    let client = Client::builder()
        .provider(Provider::Ollama)
        .api_key("ollama") // ignored by Ollama but required by ClientBuilder
        .ollama_base_url(&base_url)
        .model(&model)
        .ollama_keep_alive("30s") // short pin so the test doesn't tie up the GPU
        .ollama_num_ctx(512)
        .build()
        .expect("build client");

    let response = client
        .chat(vec![Message::user("Reply with exactly the word: pong")])
        .await
        .unwrap_or_else(|e| {
            panic!("Ollama chat failed against {base_url} with model {model}: {e}.\nIs `ollama serve` running?")
        });

    assert!(
        !response.content.trim().is_empty(),
        "Ollama auto-switched /api/chat returned empty content; \
         expected a non-empty reply. Got: {:?}",
        response.content
    );
}

#[tokio::test]
#[ignore] // Requires `ollama serve` running on localhost:11434 + OLLAMA_MODEL env var.
async fn live_ollama_think_string_parser_round_trip() {
    // Verifies the 0.15.1 fix: ollama_think("yes") and ollama_think("true")
    // both still produce a wire body the real Ollama server accepts.
    //
    // Scope honesty: most common pre-pulled models (llama3.1:8b, qwen2.5,
    // mistral) don't actually support `think` — they accept the field
    // silently and return a normal response. This test verifies "Ollama
    // doesn't reject the new serialization shape", not "thinking actually
    // happens". For the latter, set OLLAMA_MODEL to a think-capable model
    // like deepseek-r1 or qwen3.
    //
    // To run:
    //   OLLAMA_MODEL=llama3.1:8b cargo test --features ollama \
    //     --test ollama_http_autoswitch live_ollama_think -- --ignored --nocapture

    let model = std::env::var("OLLAMA_MODEL").expect(
        "set OLLAMA_MODEL to any chat model you have pulled — \
         run `ollama list` to see what's available",
    );
    let base_url =
        std::env::var("OLLAMA_BASE_URL").unwrap_or_else(|_| "http://localhost:11434".to_string());

    let client = Client::builder()
        .provider(Provider::Ollama)
        .api_key("ollama")
        .ollama_base_url(&base_url)
        .model(&model)
        .ollama_think("yes") // parser maps to bool true on the wire
        .ollama_keep_alive("30s")
        .build()
        .expect("build client");

    let response = client
        .chat(vec![Message::user("Reply with exactly the word: pong")])
        .await
        .unwrap_or_else(|e| {
            panic!("Ollama chat failed against {base_url} with model {model} and think=yes: {e}.\nIs `ollama serve` running?")
        });

    assert!(
        !response.content.trim().is_empty(),
        "Ollama with think=yes returned empty content; \
         expected a non-empty reply. Got: {:?}",
        response.content
    );
}
