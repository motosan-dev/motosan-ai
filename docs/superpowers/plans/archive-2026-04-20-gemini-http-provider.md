# Gemini HTTP Provider Implementation Plan

> ⚠️ **Archive note:** This is a historical implementation plan. API snippets here may not match current released interfaces. Use `README.md`, `sdks/rust/README.md`, `sdks/python/README.md`, and `specs/types.md` as source of truth.

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a native HTTP `GeminiProvider` that calls the Google Generative AI REST API, implementing the same `ProviderImpl` trait as `AnthropicProvider` and `OpenAIProvider`.

**Architecture:** `GeminiProvider` lives in `sdks/rust/src/providers/gemini.rs`, gated behind a new `gemini` Cargo feature. It converts `ChatRequest` → Gemini JSON, POSTs to `generativelanguage.googleapis.com`, and adapts the SSE response stream into `StreamEvent` items. The `Client` dispatch matches the pattern already used for Anthropic/OpenAI.

**Tech Stack:** Rust 1.82, `reqwest` 0.12 (already optional dep), `eventsource-stream` 0.2 (already optional dep), `serde_json`, `mockito` 1.x (dev-dep, already present).

---

## File Map

| Action | Path | Responsibility |
|--------|------|----------------|
| Create | `sdks/rust/src/providers/gemini.rs` | Struct, request builder, response parser, stream adapter, `ProviderImpl` |
| Modify | `sdks/rust/src/models.rs` | `DEFAULT_GEMINI_MODEL` constant + `GEMINI_MODELS` list |
| Modify | `sdks/rust/Cargo.toml` | `gemini` feature + extend `full` |
| Modify | `sdks/rust/src/providers/mod.rs` | `Provider::Gemini` variant + `pub mod gemini` + extend cfg guards |
| Modify | `sdks/rust/src/client.rs` | `dispatch_chat` + `dispatch_stream_inner` arms + `build_gemini_provider` + `stream_collect`/`stream_collect_with` cfg macros |

---

## Gemini API Reference (read before every task)

**Endpoints** (base = `https://generativelanguage.googleapis.com/v1beta`):
- Non-streaming: `POST /models/{model}:generateContent?key={api_key}`
- Streaming:     `POST /models/{model}:streamGenerateContent?alt=sse&key={api_key}`

**Request body:**
```json
{
  "contents": [
    {"role": "user",  "parts": [{"text": "Hello"}]},
    {"role": "model", "parts": [{"text": "Hi"}, {"functionCall": {"name": "foo", "args": {}}}]},
    {"role": "user",  "parts": [{"functionResponse": {"name": "foo", "response": {"result": "ok"}}}]}
  ],
  "systemInstruction": {"parts": [{"text": "You are helpful."}]},
  "tools": [{"functionDeclarations": [{"name": "foo", "description": "...", "parameters": {...}}]}],
  "toolConfig": {"functionCallingConfig": {"mode": "AUTO"}},
  "generationConfig": {"temperature": 0.7, "maxOutputTokens": 8192}
}
```

**Notes:**
- Gemini roles are `"user"` and `"model"` (never `"assistant"` or `"system"`).
- `systemInstruction` is a top-level field, not a message.
- `Role::Tool` messages become `role: "user"` with a `functionResponse` part.
- Gemini **does not return tool call IDs** — generate them locally with an atomic counter.
- `ToolChoice::Required` → mode `"ANY"`, `ToolChoice::None` → mode `"NONE"`, `ToolChoice::Tool{name}` → mode `"ANY"` + `allowedFunctionNames: [name]`.

**Non-streaming response:**
```json
{
  "candidates": [{"content": {"parts": [{"text": "Hi"}], "role": "model"}, "finishReason": "STOP"}],
  "usageMetadata": {"promptTokenCount": 10, "candidatesTokenCount": 5},
  "modelVersion": "gemini-2.0-flash"
}
```

**Streaming SSE:** Each `data:` line is a complete `GenerateContentResponse` JSON. Text parts are incremental deltas. `functionCall` parts arrive as complete objects in one event. `usageMetadata` and `finishReason` appear only in the last event.

**finishReason mapping:**
- `"STOP"` → `StopReason::EndTurn` (unless tool calls present → `ToolUse`)
- `"MAX_TOKENS"` → `StopReason::MaxTokens`
- anything else → `StopReason::Other`

---

## Task 1 — Feature flag, model constants, Provider variant

**Files:**
- Modify: `sdks/rust/Cargo.toml`
- Modify: `sdks/rust/src/models.rs`
- Modify: `sdks/rust/src/providers/mod.rs`

No tests — these are wiring changes.

- [ ] **Step 1: Add `gemini` feature to Cargo.toml**

In `[features]` section, after the `gemini-cli` entry:
```toml
gemini = [
  "dep:reqwest",
  "dep:eventsource-stream",
  "dep:tokio-stream",
  "dep:tokio",
]
```

Also extend the `full` feature line to include `"gemini"`:
```toml
full = ["anthropic", "openai", "minimax", "ollama", "ollama_native", "gemini"]
```

- [ ] **Step 2: Add model constants to `sdks/rust/src/models.rs`**

Append at the end of the file:
```rust
pub const DEFAULT_GEMINI_MODEL: &str = "gemini-2.0-flash";

pub const GEMINI_MODELS: &[&str] = &[
    "gemini-2.0-flash",
    "gemini-2.0-flash-lite",
    "gemini-2.5-pro",
    "gemini-2.5-flash",
    "gemini-1.5-pro",
    "gemini-1.5-flash",
];
```

- [ ] **Step 3: Add `Provider::Gemini` variant to the enum in `sdks/rust/src/providers/mod.rs`**

In the `Provider` enum (around line 44), add after `GeminiCli`:
```rust
    /// HTTP client for the Google Generative AI REST API. Requires the `gemini` feature.
    Gemini,
```

- [ ] **Step 4: Add `pub mod gemini` at the bottom of `sdks/rust/src/providers/mod.rs`**

After the `gemini-cli` mod line:
```rust
#[cfg(feature = "gemini")]
pub mod gemini;
```

- [ ] **Step 5: Extend the cfg guards in `mod.rs` to include `gemini`**

Every `#[cfg(any(feature = "anthropic", feature = "openai", feature = "minimax", feature = "ollama_native"))]` guard that covers shared helpers (`ChatResponseBuilder`, `extract_error_message`, `map_http_error`, `is_retryable_status`, `is_retryable_network_error`, `parse_retry_after`, `sleep_before_retry`) needs `feature = "gemini"` added. Example (line 2-8 area):

```rust
#[cfg(any(
    feature = "anthropic",
    feature = "openai",
    feature = "minimax",
    feature = "ollama_native",
    feature = "gemini",
))]
use crate::retry::RetryPolicy;
```

Apply the same `feature = "gemini"` addition to all six `#[cfg(any(...))]` blocks that guard those shared helpers. Do NOT change the `reject_document_blocks` guard — that stays at `openai/minimax/ollama_native` only.

- [ ] **Step 6: Verify it compiles**

```bash
cd sdks/rust && cargo check --features gemini 2>&1 | head -30
```
Expected: errors about missing `gemini.rs` module (not about the constants or enum).

- [ ] **Step 7: Commit**

```bash
git add sdks/rust/Cargo.toml sdks/rust/src/models.rs sdks/rust/src/providers/mod.rs
git commit -m "feat(gemini): scaffold feature flag, model constants, Provider::Gemini variant"
```

---

## Task 2 — GeminiProvider struct + request builder

**Files:**
- Create: `sdks/rust/src/providers/gemini.rs`

- [ ] **Step 1: Write failing unit tests for `build_request`**

Create `sdks/rust/src/providers/gemini.rs` with the test module first:

```rust
use std::sync::atomic::{AtomicU64, Ordering};

use crate::error::MotosanError;
use crate::models::DEFAULT_GEMINI_MODEL;
use crate::providers::{
    extract_error_message, is_retryable_network_error, is_retryable_status, map_http_error,
    parse_retry_after, sleep_before_retry, ChatResponseBuilder, ProviderImpl,
};
use crate::retry::RetryPolicy;
use crate::stream::BoxStream;
use crate::types::{
    ChatRequest, ChatResponse, ContentBlock, ImageSource, Role, StopReason, StreamEvent,
    ToolCall, ToolChoice, Usage,
};
use async_trait::async_trait;
use eventsource_stream::Eventsource;
use futures_core::Stream;
use reqwest::Client;
use serde_json::{json, Value};
use std::pin::Pin;
use std::task::Poll;

const DEFAULT_MAX_TOKENS: u32 = 8192;
const BASE_URL: &str = "https://generativelanguage.googleapis.com/v1beta";

static TOOL_CALL_COUNTER: AtomicU64 = AtomicU64::new(0);

fn gen_tool_call_id() -> String {
    let n = TOOL_CALL_COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("call_{n}")
}

pub struct GeminiProvider {
    http: Client,
    api_key: String,
    model: String,
    base_url: String,
    retry_policy: RetryPolicy,
}

impl GeminiProvider {
    pub fn new(
        api_key: impl Into<String>,
        model: Option<String>,
        base_url: Option<String>,
    ) -> Self {
        Self {
            http: Client::new(),
            api_key: api_key.into(),
            model: model.unwrap_or_else(|| DEFAULT_GEMINI_MODEL.to_string()),
            base_url: base_url.unwrap_or_else(|| BASE_URL.to_string()),
            retry_policy: RetryPolicy::default(),
        }
    }

    pub fn with_retry_policy(mut self, policy: RetryPolicy) -> Self {
        self.retry_policy = policy;
        self
    }

    fn generate_url(&self, req: &ChatRequest) -> String {
        let model = req.model.as_deref().unwrap_or(&self.model);
        format!(
            "{}/models/{}:generateContent?key={}",
            self.base_url, model, self.api_key
        )
    }

    fn stream_url(&self, req: &ChatRequest) -> String {
        let model = req.model.as_deref().unwrap_or(&self.model);
        format!(
            "{}/models/{}:streamGenerateContent?alt=sse&key={}",
            self.base_url, model, self.api_key
        )
    }

    fn build_request(req: &ChatRequest, model: &str) -> Value {
        // --- contents ---
        let mut contents: Vec<Value> = Vec::new();
        let mut extracted_system: Option<String> = None;

        for message in &req.messages {
            match message.role {
                Role::System => {
                    extracted_system = Some(message.content.clone());
                }
                Role::User => {
                    let mut parts: Vec<Value> = Vec::new();
                    if !message.content.is_empty() {
                        parts.push(json!({"text": message.content}));
                    }
                    for block in &message.content_blocks {
                        match block {
                            ContentBlock::Text { text } => {
                                parts.push(json!({"text": text}));
                            }
                            ContentBlock::Image { source } => match source {
                                ImageSource::Base64 { media_type, data } => {
                                    parts.push(json!({
                                        "inlineData": {
                                            "mimeType": media_type,
                                            "data": data
                                        }
                                    }));
                                }
                                ImageSource::Url { url } => {
                                    parts.push(json!({"fileData": {"fileUri": url}}));
                                }
                            },
                            ContentBlock::Document { .. } => {
                                // Documents not supported; callers should reject earlier.
                            }
                        }
                    }
                    if parts.is_empty() {
                        parts.push(json!({"text": ""}));
                    }
                    contents.push(json!({"role": "user", "parts": parts}));
                }
                Role::Assistant => {
                    let mut parts: Vec<Value> = Vec::new();
                    if !message.content.is_empty() {
                        parts.push(json!({"text": message.content}));
                    }
                    for tc in &message.tool_calls {
                        parts.push(json!({
                            "functionCall": {
                                "name": tc.name,
                                "args": tc.input
                            }
                        }));
                    }
                    if parts.is_empty() {
                        parts.push(json!({"text": ""}));
                    }
                    contents.push(json!({"role": "model", "parts": parts}));
                }
                Role::Tool => {
                    if let Some(ref tool_call_id) = message.tool_call_id {
                        // Gemini tool results don't use the ID in the wire format;
                        // name must match the functionCall name. We embed the id
                        // as the function name fallback when name isn't available.
                        let name = tool_call_id.as_str();
                        let response: Value = serde_json::from_str(&message.content)
                            .unwrap_or_else(|_| json!({"result": message.content}));
                        contents.push(json!({
                            "role": "user",
                            "parts": [{"functionResponse": {"name": name, "response": response}}]
                        }));
                    }
                }
            }
        }

        // --- system instruction ---
        let system_text = req
            .system
            .as_deref()
            .or(extracted_system.as_deref())
            .unwrap_or("");

        // --- generation config ---
        let max_tokens = req.max_tokens.unwrap_or(DEFAULT_MAX_TOKENS);
        let mut gen_config = json!({"maxOutputTokens": max_tokens});
        if let Some(temp) = req.temperature {
            gen_config["temperature"] = json!(temp);
        }
        if let Some(ref stops) = req.stop_sequences {
            if !stops.is_empty() {
                gen_config["stopSequences"] = json!(stops);
            }
        }

        // --- tools ---
        let mut body = json!({
            "contents": contents,
            "generationConfig": gen_config,
        });

        if !system_text.is_empty() {
            body["systemInstruction"] = json!({"parts": [{"text": system_text}]});
        }

        if let Some(ref tools) = req.tools {
            if !tools.is_empty() {
                let declarations: Vec<Value> = tools
                    .iter()
                    .map(|t| {
                        let mut decl = json!({
                            "name": t.name,
                            "description": t.description.as_deref().unwrap_or(""),
                        });
                        if let Some(ref schema) = t.input_schema {
                            decl["parameters"] = schema.clone();
                        }
                        decl
                    })
                    .collect();
                body["tools"] = json!([{"functionDeclarations": declarations}]);

                // tool_choice → toolConfig
                let mode = match &req.tool_choice {
                    None | Some(ToolChoice::Auto) => "AUTO",
                    Some(ToolChoice::Required) => "ANY",
                    Some(ToolChoice::None) => {
                        body.as_object_mut().map(|m| m.remove("tools"));
                        "NONE"
                    }
                    Some(ToolChoice::Tool { .. }) => "ANY",
                };
                if mode != "NONE" {
                    let mut fc_config = json!({"mode": mode});
                    if let Some(ToolChoice::Tool { name }) = &req.tool_choice {
                        fc_config["allowedFunctionNames"] = json!([name]);
                    }
                    body["toolConfig"] = json!({"functionCallingConfig": fc_config});
                }
            }
        }

        // provider_options passthrough
        if let Some(po) = &req.provider_options {
            if let Some(map) = po.as_object() {
                for (k, v) in map {
                    body[k] = v.clone();
                }
            }
        }

        let _ = model; // model is encoded in the URL, not the body
        body
    }
}

// ---- Tests ----

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{Message, Tool};
    use serde_json::json;

    fn user_msg(text: &str) -> Message {
        Message::user(text)
    }

    fn assistant_msg(text: &str) -> Message {
        Message::assistant(text)
    }

    fn build(msgs: Vec<Message>) -> Value {
        let req = ChatRequest::builder().messages(msgs).build();
        GeminiProvider::build_request(&req, DEFAULT_GEMINI_MODEL)
    }

    #[test]
    fn simple_user_message() {
        let body = build(vec![user_msg("Hello")]);
        assert_eq!(body["contents"][0]["role"], "user");
        assert_eq!(body["contents"][0]["parts"][0]["text"], "Hello");
    }

    #[test]
    fn assistant_message_becomes_model_role() {
        let body = build(vec![user_msg("Hi"), assistant_msg("Hello back")]);
        assert_eq!(body["contents"][1]["role"], "model");
        assert_eq!(body["contents"][1]["parts"][0]["text"], "Hello back");
    }

    #[test]
    fn system_message_extracted_to_system_instruction() {
        let req = ChatRequest::builder()
            .messages(vec![user_msg("Hi")])
            .system("Be concise.")
            .build();
        let body = GeminiProvider::build_request(&req, DEFAULT_GEMINI_MODEL);
        assert_eq!(body["systemInstruction"]["parts"][0]["text"], "Be concise.");
        // system must not appear in contents
        assert_eq!(body["contents"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn tool_result_message_becomes_function_response() {
        let tool_msg = Message {
            role: Role::Tool,
            content: r#"{"result": "sunny"}"#.to_string(),
            tool_call_id: Some("get_weather".to_string()),
            ..Default::default()
        };
        let body = build(vec![user_msg("?"), tool_msg]);
        let part = &body["contents"][1]["parts"][0];
        assert_eq!(part["functionResponse"]["name"], "get_weather");
        assert_eq!(part["functionResponse"]["response"]["result"], "sunny");
    }

    #[test]
    fn tool_choice_required_maps_to_any() {
        let tool = Tool {
            name: "search".to_string(),
            description: Some("Search".to_string()),
            input_schema: Some(json!({"type": "object", "properties": {}})),
            cache: false,
        };
        let req = ChatRequest::builder()
            .messages(vec![user_msg("find it")])
            .tools(vec![tool])
            .tool_choice(ToolChoice::Required)
            .build();
        let body = GeminiProvider::build_request(&req, DEFAULT_GEMINI_MODEL);
        assert_eq!(body["toolConfig"]["functionCallingConfig"]["mode"], "ANY");
    }

    #[test]
    fn tool_choice_none_removes_tools() {
        let tool = Tool {
            name: "search".to_string(),
            description: None,
            input_schema: None,
            cache: false,
        };
        let req = ChatRequest::builder()
            .messages(vec![user_msg("hi")])
            .tools(vec![tool])
            .tool_choice(ToolChoice::None)
            .build();
        let body = GeminiProvider::build_request(&req, DEFAULT_GEMINI_MODEL);
        assert!(body.get("tools").is_none());
    }

    #[test]
    fn temperature_included_when_set() {
        let req = ChatRequest::builder()
            .messages(vec![user_msg("hi")])
            .temperature(0.3)
            .build();
        let body = GeminiProvider::build_request(&req, DEFAULT_GEMINI_MODEL);
        assert_eq!(body["generationConfig"]["temperature"], 0.3);
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

```bash
cd sdks/rust && cargo test --features gemini providers::gemini::tests 2>&1 | tail -20
```
Expected: compile error — `ProviderImpl` not yet implemented on `GeminiProvider`.

- [ ] **Step 3: Commit the scaffold**

```bash
git add sdks/rust/src/providers/gemini.rs
git commit -m "test(gemini): add request builder unit tests"
```

---

## Task 3 — Response parser + `chat()` method

**Files:**
- Modify: `sdks/rust/src/providers/gemini.rs`

- [ ] **Step 1: Write a failing test for `parse_response`**

Add to the `#[cfg(test)]` block in `gemini.rs`:

```rust
    #[test]
    fn parse_text_response() {
        let raw = json!({
            "candidates": [{
                "content": {"parts": [{"text": "Hello world"}], "role": "model"},
                "finishReason": "STOP"
            }],
            "usageMetadata": {"promptTokenCount": 10, "candidatesTokenCount": 5},
            "modelVersion": "gemini-2.0-flash"
        });
        let resp = GeminiProvider::parse_response(&raw, DEFAULT_GEMINI_MODEL);
        assert_eq!(resp.content, "Hello world");
        assert_eq!(resp.tool_calls.len(), 0);
        assert_eq!(resp.stop_reason, StopReason::EndTurn);
        assert_eq!(resp.usage.input_tokens, 10);
        assert_eq!(resp.usage.output_tokens, 5);
        assert_eq!(resp.model, "gemini-2.0-flash");
    }

    #[test]
    fn parse_tool_call_response() {
        let raw = json!({
            "candidates": [{
                "content": {
                    "parts": [{"functionCall": {"name": "search", "args": {"q": "rust"}}}],
                    "role": "model"
                },
                "finishReason": "STOP"
            }],
            "usageMetadata": {"promptTokenCount": 8, "candidatesTokenCount": 3},
            "modelVersion": "gemini-2.0-flash"
        });
        let resp = GeminiProvider::parse_response(&raw, DEFAULT_GEMINI_MODEL);
        assert_eq!(resp.tool_calls.len(), 1);
        assert_eq!(resp.tool_calls[0].name, "search");
        assert_eq!(resp.tool_calls[0].input["q"], "rust");
        assert_eq!(resp.stop_reason, StopReason::ToolUse);
    }

    #[test]
    fn parse_max_tokens_stop_reason() {
        let raw = json!({
            "candidates": [{
                "content": {"parts": [{"text": "truncated"}], "role": "model"},
                "finishReason": "MAX_TOKENS"
            }],
            "usageMetadata": {"promptTokenCount": 5, "candidatesTokenCount": 100}
        });
        let resp = GeminiProvider::parse_response(&raw, DEFAULT_GEMINI_MODEL);
        assert_eq!(resp.stop_reason, StopReason::MaxTokens);
    }
```

- [ ] **Step 2: Run tests to verify they fail**

```bash
cd sdks/rust && cargo test --features gemini parse 2>&1 | grep "error\|FAILED" | head -10
```
Expected: compile error — `parse_response` not yet defined.

- [ ] **Step 3: Implement `parse_response` in `gemini.rs`**

Add this `impl GeminiProvider` method (inside the existing `impl GeminiProvider` block, after `build_request`):

```rust
    fn parse_response(payload: &Value, default_model: &str) -> ChatResponse {
        let candidate = payload
            .get("candidates")
            .and_then(|c| c.get(0))
            .cloned()
            .unwrap_or(json!({}));

        let parts = candidate
            .get("content")
            .and_then(|c| c.get("parts"))
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();

        let mut text = String::new();
        let mut tool_calls: Vec<ToolCall> = Vec::new();

        for part in &parts {
            if let Some(t) = part.get("text").and_then(Value::as_str) {
                text.push_str(t);
            }
            if let Some(fc) = part.get("functionCall") {
                let name = fc
                    .get("name")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string();
                let input = fc.get("args").cloned().unwrap_or(json!({}));
                tool_calls.push(ToolCall {
                    id: gen_tool_call_id(),
                    name,
                    input,
                });
            }
        }

        let finish_reason = candidate
            .get("finishReason")
            .and_then(Value::as_str)
            .unwrap_or("STOP");

        let stop_reason = match finish_reason {
            "MAX_TOKENS" => StopReason::MaxTokens,
            _ if !tool_calls.is_empty() => StopReason::ToolUse,
            _ => StopReason::EndTurn,
        };

        let usage_meta = payload.get("usageMetadata").cloned().unwrap_or(json!({}));
        let input_tokens = usage_meta
            .get("promptTokenCount")
            .and_then(Value::as_u64)
            .unwrap_or(0) as u32;
        let output_tokens = usage_meta
            .get("candidatesTokenCount")
            .and_then(Value::as_u64)
            .unwrap_or(0) as u32;

        let model = payload
            .get("modelVersion")
            .and_then(Value::as_str)
            .unwrap_or(default_model)
            .to_string();

        ChatResponseBuilder::new(model)
            .content(text)
            .tool_calls(tool_calls)
            .stop_reason(stop_reason)
            .usage(input_tokens, output_tokens)
            .build()
    }
```

- [ ] **Step 4: Run tests — expect PASS**

```bash
cd sdks/rust && cargo test --features gemini parse 2>&1 | grep -E "ok|FAILED|error"
```
Expected: all `parse_*` tests pass.

- [ ] **Step 5: Implement the stub `ProviderImpl` so the file compiles**

Add this block after the `impl GeminiProvider` block:

```rust
#[async_trait]
impl ProviderImpl for GeminiProvider {
    async fn chat(&self, req: ChatRequest) -> Result<ChatResponse, MotosanError> {
        let model = req.model.clone().unwrap_or_else(|| self.model.clone());
        let url = self.generate_url(&req);
        let body = Self::build_request(&req, &model);

        let mut attempt = 0u32;
        loop {
            let result = self
                .http
                .post(&url)
                .header("content-type", "application/json")
                .json(&body)
                .send()
                .await;

            match result {
                Err(e) if is_retryable_network_error(&e) && attempt < self.retry_policy.max_retries => {
                    attempt += 1;
                    sleep_before_retry(&self.retry_policy, attempt, None).await;
                    continue;
                }
                Err(e) => return Err(MotosanError::Network(e.to_string())),
                Ok(resp) => {
                    let status = resp.status().as_u16();
                    if status != 200 {
                        let retry_after = parse_retry_after(resp.headers());
                        let payload: Value = resp.json().await.unwrap_or(json!({}));
                        let msg = extract_error_message(&payload, "Gemini API error");
                        if is_retryable_status(status) && attempt < self.retry_policy.max_retries {
                            attempt += 1;
                            sleep_before_retry(&self.retry_policy, attempt, retry_after).await;
                            continue;
                        }
                        return Err(map_http_error(status, msg));
                    }
                    let payload: Value = resp.json().await.map_err(|e| {
                        MotosanError::ProviderError(format!("failed to parse Gemini response: {e}"))
                    })?;
                    return Ok(Self::parse_response(&payload, &model));
                }
            }
        }
    }

    async fn stream(&self, _req: ChatRequest) -> Result<BoxStream, MotosanError> {
        Err(MotosanError::UnsupportedFeature(
            "Gemini streaming not yet implemented".to_string(),
        ))
    }
}
```

- [ ] **Step 6: Write a mockito integration test for `chat()`**

Add to the test module:

```rust
    #[cfg(test)]
    mod chat_tests {
        use super::*;

        #[tokio::test]
        async fn chat_returns_text_response() {
            let mut server = mockito::Server::new_async().await;
            let mock = server
                .mock("POST", mockito::Matcher::Regex(r"generateContent".to_string()))
                .with_status(200)
                .with_header("content-type", "application/json")
                .with_body(
                    r#"{
                        "candidates":[{"content":{"parts":[{"text":"Hi!"}],"role":"model"},"finishReason":"STOP"}],
                        "usageMetadata":{"promptTokenCount":5,"candidatesTokenCount":2},
                        "modelVersion":"gemini-2.0-flash"
                    }"#,
                )
                .create_async()
                .await;

            let provider = GeminiProvider::new("fake-key", None, Some(server.url()));
            let req = ChatRequest::builder()
                .messages(vec![Message::user("Hello")])
                .build();

            let resp = provider.chat(req).await.unwrap();
            assert_eq!(resp.content, "Hi!");
            assert_eq!(resp.stop_reason, StopReason::EndTurn);
            mock.assert_async().await;
        }

        #[tokio::test]
        async fn chat_retries_on_429() {
            let mut server = mockito::Server::new_async().await;
            let _m1 = server
                .mock("POST", mockito::Matcher::Regex(r"generateContent".to_string()))
                .with_status(429)
                .with_body(r#"{"error":{"message":"rate limited"}}"#)
                .create_async()
                .await;
            let _m2 = server
                .mock("POST", mockito::Matcher::Regex(r"generateContent".to_string()))
                .with_status(200)
                .with_body(
                    r#"{"candidates":[{"content":{"parts":[{"text":"ok"}],"role":"model"},"finishReason":"STOP"}],"usageMetadata":{"promptTokenCount":1,"candidatesTokenCount":1}}"#,
                )
                .create_async()
                .await;

            use crate::retry::RetryPolicy;
            let policy = RetryPolicy {
                max_retries: 2,
                base_delay_ms: 1,
                max_delay_ms: 10,
                jitter: false,
                respect_retry_after: false,
            };
            let provider =
                GeminiProvider::new("fake-key", None, Some(server.url())).with_retry_policy(policy);
            let req = ChatRequest::builder()
                .messages(vec![Message::user("hi")])
                .build();

            let resp = provider.chat(req).await.unwrap();
            assert_eq!(resp.content, "ok");
        }
    }
```

- [ ] **Step 7: Run all tests**

```bash
cd sdks/rust && cargo test --features gemini 2>&1 | grep -E "ok|FAILED|error\[" | head -30
```
Expected: all gemini tests pass.

- [ ] **Step 8: Commit**

```bash
git add sdks/rust/src/providers/gemini.rs
git commit -m "feat(gemini): add parse_response and chat() with retry"
```

---

## Task 4 — SSE stream adapter

**Files:**
- Modify: `sdks/rust/src/providers/gemini.rs`

The stream adapter converts Gemini SSE events to `StreamEvent`. Each `data:` line is a full `GenerateContentResponse` JSON. Text parts are deltas. A `functionCall` part in any event triggers `ToolCallStart → ToolCallArgs → ToolCallEnd` in one shot (Gemini delivers function calls complete, not chunked). The last event carries `finishReason` and `usageMetadata`.

- [ ] **Step 1: Write failing test for the stream adapter**

Add to `#[cfg(test)]` in `gemini.rs`:

```rust
    #[cfg(test)]
    mod stream_tests {
        use super::*;
        use futures_core::Stream;
        use std::pin::Pin;
        use tokio_stream::StreamExt;

        fn make_sse_stream(
            events: Vec<&'static str>,
        ) -> Pin<Box<dyn Stream<Item = Result<eventsource_stream::Event, eventsource_stream::EventStreamError<reqwest::Error>>> + Send>> {
            use futures_core::stream;
            let items: Vec<_> = events
                .into_iter()
                .map(|data| {
                    Ok(eventsource_stream::Event {
                        event: String::new(),
                        data: data.to_string(),
                        id: String::new(),
                        retry: None,
                    })
                })
                .collect();
            Box::pin(stream::iter(items))
        }

        #[tokio::test]
        async fn adapter_emits_text_then_done() {
            let sse = make_sse_stream(vec![
                r#"{"candidates":[{"content":{"parts":[{"text":"Hell"}],"role":"model"}}]}"#,
                r#"{"candidates":[{"content":{"parts":[{"text":"o!"}],"role":"model"},"finishReason":"STOP"}],"usageMetadata":{"promptTokenCount":3,"candidatesTokenCount":2}}"#,
            ]);
            let mut adapter = GeminiStreamAdapter {
                inner: sse,
                pending: std::collections::VecDeque::new(),
                current_stop_reason: None,
            };
            let mut events: Vec<StreamEvent> = Vec::new();
            while let Some(ev) = tokio_stream::StreamExt::next(&mut adapter).await {
                events.push(ev);
            }
            let texts: Vec<&str> = events
                .iter()
                .filter(|e| e.event_type == crate::types::StreamEventType::Text)
                .map(|e| e.content.as_str())
                .collect();
            assert_eq!(texts, vec!["Hell", "o!"]);
            let done = events.iter().find(|e| e.done).expect("no done event");
            assert_eq!(done.stop_reason, Some(StopReason::EndTurn));
        }

        #[tokio::test]
        async fn adapter_emits_tool_call_events() {
            let sse = make_sse_stream(vec![
                r#"{"candidates":[{"content":{"parts":[{"functionCall":{"name":"search","args":{"q":"rust"}}}],"role":"model"},"finishReason":"STOP"}],"usageMetadata":{"promptTokenCount":5,"candidatesTokenCount":3}}"#,
            ]);
            let mut adapter = GeminiStreamAdapter {
                inner: sse,
                pending: std::collections::VecDeque::new(),
                current_stop_reason: None,
            };
            let mut events: Vec<StreamEvent> = Vec::new();
            while let Some(ev) = tokio_stream::StreamExt::next(&mut adapter).await {
                events.push(ev);
            }
            let has_start = events
                .iter()
                .any(|e| e.event_type == crate::types::StreamEventType::ToolCallStart);
            let has_args = events
                .iter()
                .any(|e| e.event_type == crate::types::StreamEventType::ToolCallArgs);
            let has_end = events
                .iter()
                .any(|e| e.event_type == crate::types::StreamEventType::ToolCallEnd);
            assert!(has_start, "missing ToolCallStart");
            assert!(has_args, "missing ToolCallArgs");
            assert!(has_end, "missing ToolCallEnd");
            let done = events.iter().find(|e| e.done).expect("no done event");
            assert_eq!(done.stop_reason, Some(StopReason::ToolUse));
        }
    }
```

- [ ] **Step 2: Run tests to verify they fail**

```bash
cd sdks/rust && cargo test --features gemini stream_tests 2>&1 | grep "error\[" | head -10
```
Expected: compile error — `GeminiStreamAdapter` not defined.

- [ ] **Step 3: Implement `GeminiStreamAdapter`**

Add after the `impl ProviderImpl for GeminiProvider` block:

```rust
struct GeminiStreamAdapter {
    inner: Pin<
        Box<
            dyn Stream<
                    Item = Result<
                        eventsource_stream::Event,
                        eventsource_stream::EventStreamError<reqwest::Error>,
                    >,
                > + Send,
        >,
    >,
    pending: std::collections::VecDeque<StreamEvent>,
    current_stop_reason: Option<StopReason>,
}

impl Stream for GeminiStreamAdapter {
    type Item = StreamEvent;

    fn poll_next(
        mut self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> Poll<Option<Self::Item>> {
        if let Some(ev) = self.pending.pop_front() {
            return Poll::Ready(Some(ev));
        }

        loop {
            match self.inner.as_mut().poll_next(cx) {
                Poll::Ready(Some(Ok(event))) => {
                    if event.data.trim() == "[DONE]" || event.data.trim().is_empty() {
                        continue;
                    }
                    let payload: Value = match serde_json::from_str(&event.data) {
                        Ok(v) => v,
                        Err(_) => continue,
                    };

                    let candidate = match payload
                        .get("candidates")
                        .and_then(|c| c.get(0))
                    {
                        Some(c) => c.clone(),
                        None => continue,
                    };

                    let parts = candidate
                        .get("content")
                        .and_then(|c| c.get("parts"))
                        .and_then(Value::as_array)
                        .cloned()
                        .unwrap_or_default();

                    let finish_reason = candidate
                        .get("finishReason")
                        .and_then(Value::as_str);

                    let mut has_tool_calls = false;

                    for part in &parts {
                        if let Some(text) = part.get("text").and_then(Value::as_str) {
                            if !text.is_empty() {
                                self.pending.push_back(StreamEvent::text(text));
                            }
                        }
                        if let Some(fc) = part.get("functionCall") {
                            has_tool_calls = true;
                            let name = fc
                                .get("name")
                                .and_then(Value::as_str)
                                .unwrap_or("")
                                .to_string();
                            let args = fc.get("args").cloned().unwrap_or(json!({}));
                            let args_str = args.to_string();
                            let id = gen_tool_call_id();
                            self.pending
                                .push_back(StreamEvent::tool_call_start(&id, &name));
                            self.pending
                                .push_back(StreamEvent::tool_call_args_with_id(&id, &args_str));
                            self.pending
                                .push_back(StreamEvent::tool_call_end_with_id(id));
                        }
                    }

                    // usage
                    if let Some(usage_meta) = payload.get("usageMetadata") {
                        let input_tokens = usage_meta
                            .get("promptTokenCount")
                            .and_then(Value::as_u64)
                            .unwrap_or(0) as u32;
                        let output_tokens = usage_meta
                            .get("candidatesTokenCount")
                            .and_then(Value::as_u64)
                            .unwrap_or(0) as u32;
                        self.pending.push_back(StreamEvent::usage(Usage {
                            input_tokens,
                            output_tokens,
                            cache_creation_input_tokens: None,
                            cache_read_input_tokens: None,
                        }));
                    }

                    // terminal event
                    if let Some(reason) = finish_reason {
                        let stop_reason = match reason {
                            "MAX_TOKENS" => StopReason::MaxTokens,
                            _ if has_tool_calls => StopReason::ToolUse,
                            _ => StopReason::EndTurn,
                        };
                        self.pending
                            .push_back(StreamEvent::done_with_stop_reason(stop_reason));
                    }

                    if let Some(first) = self.pending.pop_front() {
                        return Poll::Ready(Some(first));
                    }
                    continue;
                }
                Poll::Ready(Some(Err(_))) => continue,
                Poll::Ready(None) => return Poll::Ready(None),
                Poll::Pending => return Poll::Pending,
            }
        }
    }
}
```

- [ ] **Step 4: Run stream tests**

```bash
cd sdks/rust && cargo test --features gemini stream_tests 2>&1 | grep -E "ok|FAILED"
```
Expected: both stream adapter tests pass.

- [ ] **Step 5: Commit**

```bash
git add sdks/rust/src/providers/gemini.rs
git commit -m "feat(gemini): implement GeminiStreamAdapter for SSE"
```

---

## Task 5 — Implement `stream()` method

**Files:**
- Modify: `sdks/rust/src/providers/gemini.rs`

- [ ] **Step 1: Write a failing mockito test for `stream()`**

Add to the `chat_tests` module inside `#[cfg(test)]`:

```rust
        #[tokio::test]
        async fn stream_emits_text_and_done() {
            let mut server = mockito::Server::new_async().await;
            let sse_body = concat!(
                "data: {\"candidates\":[{\"content\":{\"parts\":[{\"text\":\"Hi\"}],\"role\":\"model\"}}]}\n\n",
                "data: {\"candidates\":[{\"content\":{\"parts\":[{\"text\":\" there\"}],\"role\":\"model\"},\"finishReason\":\"STOP\"}],\"usageMetadata\":{\"promptTokenCount\":3,\"candidatesTokenCount\":2}}\n\n",
            );
            let mock = server
                .mock(
                    "POST",
                    mockito::Matcher::Regex(r"streamGenerateContent".to_string()),
                )
                .with_status(200)
                .with_header("content-type", "text/event-stream")
                .with_body(sse_body)
                .create_async()
                .await;

            let provider = GeminiProvider::new("fake-key", None, Some(server.url()));
            let req = ChatRequest::builder()
                .messages(vec![Message::user("Hello")])
                .build();
            let stream = provider.stream(req).await.unwrap();
            let resp = crate::stream::collect_stream(stream).await;

            assert_eq!(resp.content, "Hi there");
            assert_eq!(resp.stop_reason, StopReason::EndTurn);
            mock.assert_async().await;
        }
```

- [ ] **Step 2: Run to verify failure**

```bash
cd sdks/rust && cargo test --features gemini stream_emits_text_and_done 2>&1 | grep -E "FAILED|ok"
```
Expected: FAILED (returns `UnsupportedFeature`).

- [ ] **Step 3: Replace the stub `stream()` implementation**

In the `impl ProviderImpl for GeminiProvider` block, replace the stub `stream()`:

```rust
    async fn stream(&self, req: ChatRequest) -> Result<BoxStream, MotosanError> {
        let model = req.model.clone().unwrap_or_else(|| self.model.clone());
        let url = self.stream_url(&req);
        let body = Self::build_request(&req, &model);

        let mut attempt = 0u32;
        loop {
            let result = self
                .http
                .post(&url)
                .header("content-type", "application/json")
                .json(&body)
                .send()
                .await;

            match result {
                Err(e) if is_retryable_network_error(&e) && attempt < self.retry_policy.max_retries => {
                    attempt += 1;
                    sleep_before_retry(&self.retry_policy, attempt, None).await;
                    continue;
                }
                Err(e) => return Err(MotosanError::Network(e.to_string())),
                Ok(resp) => {
                    let status = resp.status().as_u16();
                    if status != 200 {
                        let retry_after = parse_retry_after(resp.headers());
                        let payload: Value = resp.json().await.unwrap_or(json!({}));
                        let msg = extract_error_message(&payload, "Gemini stream error");
                        if is_retryable_status(status) && attempt < self.retry_policy.max_retries {
                            attempt += 1;
                            sleep_before_retry(&self.retry_policy, attempt, retry_after).await;
                            continue;
                        }
                        return Err(map_http_error(status, msg));
                    }
                    let sse = resp.bytes_stream().eventsource();
                    let adapter = GeminiStreamAdapter {
                        inner: Box::pin(sse),
                        pending: std::collections::VecDeque::new(),
                        current_stop_reason: None,
                    };
                    return Ok(Box::pin(adapter));
                }
            }
        }
    }
```

- [ ] **Step 4: Run all gemini tests**

```bash
cd sdks/rust && cargo test --features gemini 2>&1 | grep -E "ok|FAILED|error\["
```
Expected: all pass.

- [ ] **Step 5: Commit**

```bash
git add sdks/rust/src/providers/gemini.rs
git commit -m "feat(gemini): implement stream() using GeminiStreamAdapter"
```

---

## Task 6 — Client dispatch integration

**Files:**
- Modify: `sdks/rust/src/client.rs`

Wire `Provider::Gemini` into `dispatch_chat`, `dispatch_stream_inner`, extend `stream_collect`/`stream_collect_with` cfg guards, add `build_gemini_provider`, and add `ClientBuilder` support.

- [ ] **Step 1: Add `Provider::Gemini` arm to `dispatch_chat`**

In `dispatch_chat` (around line 255, after the `GeminiCli` arm), add:

```rust
            Provider::Gemini => {
                #[cfg(feature = "gemini")]
                {
                    use crate::providers::ProviderImpl;
                    return self.build_gemini_provider().chat(request).await;
                }
                #[cfg(not(feature = "gemini"))]
                {
                    let _ = request;
                    return Err(Self::feature_not_enabled("gemini"));
                }
            }
```

- [ ] **Step 2: Add `Provider::Gemini` arm to `dispatch_stream_inner`**

In `dispatch_stream_inner` (after the `GeminiCli` arm, around line 362), add:

```rust
            Provider::Gemini => {
                #[cfg(feature = "gemini")]
                {
                    use crate::providers::ProviderImpl;
                    return self.build_gemini_provider().stream(request).await;
                }
                #[cfg(not(feature = "gemini"))]
                {
                    let _ = request;
                    return Err(Self::feature_not_enabled("gemini"));
                }
            }
```

- [ ] **Step 3: Add `build_gemini_provider` method**

After `build_gemini_cli_provider` (or any other `build_*` method), add:

```rust
    #[cfg(feature = "gemini")]
    fn build_gemini_provider(&self) -> crate::providers::gemini::GeminiProvider {
        crate::providers::gemini::GeminiProvider::new(
            self.api_key.clone(),
            self.model.clone(),
            None,
        )
        .with_retry_policy(self.retry_policy.clone())
    }
```

- [ ] **Step 4: Extend `stream_collect` and `stream_collect_with` cfg guards**

Both methods (around lines 112–156) have `#[cfg(any(feature = "anthropic", feature = "openai", feature = "minimax", feature = "ollama_native"))]`. Add `feature = "gemini"` to each:

```rust
    #[cfg(any(
        feature = "anthropic",
        feature = "openai",
        feature = "minimax",
        feature = "ollama_native",
        feature = "gemini",
    ))]
    pub async fn stream_collect(...
```

Apply the same change to `stream_collect_with`.

- [ ] **Step 5: Extend `feature_not_enabled` cfg guard**

The `feature_not_enabled` method has a guard like:
```rust
#[cfg(any(
    not(feature = "anthropic"),
    not(feature = "openai"),
    not(feature = "minimax"),
    not(feature = "ollama"),
    not(feature = "ollama_native"),
    not(feature = "claude-code"),
    not(feature = "codex-cli"),
    not(feature = "gemini-cli"),
))]
```
Add `not(feature = "gemini"),` to the list.

- [ ] **Step 6: Add `ClientBuilder` support**

Search for the `ClientBuilder` struct definition (look for a block with fields like `provider`, `api_key`, `model`). Add no new fields — `GeminiProvider` only needs `api_key`, `model`, and `retry_policy`, which the builder already captures. Just verify `ClientBuilder::gemini()` or a generic pattern exists. If the builder uses a `provider(Provider)` setter, document that callers use:
```rust
Client::builder()
    .provider(Provider::Gemini)
    .api_key(std::env::var("GOOGLE_API_KEY").unwrap())
    .build()
```
No code change needed if the builder already accepts `provider(Provider::Gemini)`.

- [ ] **Step 7: Verify the full build**

```bash
cd sdks/rust && cargo build --features gemini 2>&1 | grep "error\[" | head -20
```
Expected: no errors.

- [ ] **Step 8: Run all tests with the gemini feature**

```bash
cd sdks/rust && cargo test --features gemini 2>&1 | grep -E "test .* ok|FAILED" | head -40
```
Expected: all pass.

- [ ] **Step 9: Run check-rust to verify CI gate**

```bash
cd sdks/rust && cargo clippy --features gemini -- -D warnings 2>&1 | head -30
```
Fix any warnings before committing.

- [ ] **Step 10: Commit**

```bash
git add sdks/rust/src/client.rs
git commit -m "feat(gemini): wire Provider::Gemini into Client dispatch"
```

---

## Task 7 — End-to-end smoke test (optional live test)

This task is optional and requires a real `GOOGLE_API_KEY` in the environment.

**Files:**
- Modify: `sdks/rust/tests/` (add `gemini_live.rs` if a live test pattern exists)

- [ ] **Step 1: Check if a live test pattern exists**

```bash
ls /Users/daiwanwei/Projects/wade/motosan-ai/sdks/rust/tests/
```

If `anthropic_live.rs` or similar exists, mirror its pattern for Gemini.

- [ ] **Step 2: Write a minimal live smoke test**

```rust
// tests/gemini_live.rs  — only runs when GOOGLE_API_KEY is set
#[cfg(feature = "gemini")]
#[tokio::test]
async fn gemini_live_chat() {
    let Ok(key) = std::env::var("GOOGLE_API_KEY") else { return };
    let client = motosan_ai::Client::builder()
        .provider(motosan_ai::providers::Provider::Gemini)
        .api_key(key)
        .build();
    let resp = client
        .chat(vec![motosan_ai::types::Message::user("Say hello in one word.")])
        .await
        .unwrap();
    assert!(!resp.content.is_empty(), "empty response");
}
```

- [ ] **Step 3: Run it**

```bash
cd sdks/rust && GOOGLE_API_KEY=$(cat ~/.config/google_api_key) cargo test --features gemini gemini_live_chat -- --ignored 2>&1
```

- [ ] **Step 4: Commit if test file was created**

```bash
git add sdks/rust/tests/gemini_live.rs
git commit -m "test(gemini): add live smoke test gated on GOOGLE_API_KEY"
```

---

## Self-Review Checklist

**Spec coverage:**
- [x] `ProviderImpl::chat()` — Task 3
- [x] `ProviderImpl::stream()` — Task 5
- [x] Message role conversion (user/model/system/tool) — Task 2
- [x] systemInstruction field — Task 2
- [x] Tool declarations + toolConfig — Task 2
- [x] ToolChoice mapping (Auto/Required/None/Tool) — Task 2
- [x] Tool call ID generation — Task 2 (`gen_tool_call_id`)
- [x] Response parsing (text, tool calls, stop reason, usage) — Task 3
- [x] SSE stream adapter (text delta, functionCall, finishReason, usage) — Task 4
- [x] Retry logic (429, 5xx, network errors) — Task 3 + 5
- [x] Feature flag gating — Task 1
- [x] Client dispatch — Task 6
- [x] `stream_collect`/`stream_collect_with` work with Gemini — Task 6

**No placeholders found.**

**Type consistency:** `gen_tool_call_id()` used in both `parse_response` and `GeminiStreamAdapter`. `GeminiProvider::build_request` signature is `(&ChatRequest, &str) -> Value` throughout.
