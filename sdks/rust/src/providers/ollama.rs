use crate::error::MotosanError;
use crate::models::DEFAULT_OLLAMA_MODEL;
use crate::providers::{
    extract_error_message, is_retryable_network_error, is_retryable_status, map_http_error,
    parse_retry_after, sleep_before_retry, ChatResponseBuilder, ProviderImpl,
};
use crate::retry::RetryPolicy;
use crate::stream::BoxStream;
use crate::types::{ChatRequest, ChatResponse, Role, StopReason, StreamEvent, ToolCall};

use async_trait::async_trait;
use futures_core::Stream;
use reqwest::Client;
use serde_json::{json, Value};
use std::pin::Pin;
use std::task::Poll;

pub struct OllamaProvider {
    http: Client,
    model: String,
    base_url: String,
    think: Option<String>,
    keep_alive: Option<String>,
    num_ctx: Option<u32>,
    retry_policy: RetryPolicy,
}

impl OllamaProvider {
    pub fn new(model: impl Into<String>, base_url: impl Into<String>) -> Self {
        Self {
            http: Client::new(),
            model: model.into(),
            base_url: base_url.into(),
            think: None,
            keep_alive: None,
            num_ctx: None,
            retry_policy: RetryPolicy::default(),
        }
    }

    pub fn with_think(mut self, think: Option<String>) -> Self {
        self.think = think;
        self
    }

    pub fn with_keep_alive(mut self, keep_alive: Option<String>) -> Self {
        self.keep_alive = keep_alive;
        self
    }

    pub fn with_num_ctx(mut self, num_ctx: Option<u32>) -> Self {
        self.num_ctx = num_ctx;
        self
    }

    pub fn with_retry_policy(mut self, retry_policy: RetryPolicy) -> Self {
        self.retry_policy = retry_policy;
        self
    }

    fn endpoint(&self) -> String {
        format!("{}/api/chat", self.base_url.trim_end_matches('/'))
    }

    fn build_request_body(&self, req: &ChatRequest, stream: bool) -> Value {
        let model = req.model.clone().unwrap_or_else(|| self.model.clone());

        let mut messages: Vec<Value> = Vec::new();

        // system_blocks takes priority over system string
        if let Some(blocks) = &req.system_blocks {
            let joined: String = blocks
                .iter()
                .map(|b| b.text.as_str())
                .collect::<Vec<_>>()
                .join("\n");
            let trimmed = joined.trim();
            if !trimmed.is_empty() {
                messages.push(json!({"role": "system", "content": trimmed}));
            }
        } else if let Some(system) = &req.system {
            let trimmed = system.trim();
            if !trimmed.is_empty() {
                messages.push(json!({"role": "system", "content": trimmed}));
            }
        }

        for message in &req.messages {
            match message.role {
                Role::System => {
                    let trimmed = message.content.trim();
                    if !trimmed.is_empty() {
                        messages.push(json!({"role": "system", "content": trimmed}));
                    }
                }
                Role::User => {
                    messages.push(json!({"role": "user", "content": message.content}));
                }
                Role::Assistant => {
                    if message.tool_calls.is_empty() {
                        messages.push(json!({"role": "assistant", "content": message.content}));
                    } else {
                        let tool_calls: Vec<Value> = message
                            .tool_calls
                            .iter()
                            .map(|tc| {
                                json!({
                                    "function": {
                                        "name": tc.name,
                                        "arguments": tc.input,
                                    }
                                })
                            })
                            .collect();
                        messages.push(json!({
                            "role": "assistant",
                            "content": message.content,
                            "tool_calls": tool_calls,
                        }));
                    }
                }
                Role::Tool => {
                    messages.push(json!({
                        "role": "tool",
                        "content": message.content,
                    }));
                }
            }
        }

        let mut body = json!({
            "model": model,
            "messages": messages,
            "stream": stream,
        });

        // Think mode: parse the user-supplied string into an appropriate
        // JSON value so callers can opt into either:
        //   - bool true/false (truthy / falsy synonyms)
        //   - string reasoning levels like "low" / "medium" / "high"
        //     (newer Ollama versions accept these)
        // Before 0.15.1 this hard-coded `true` for any non-None value,
        // silently flattening `ollama_think("no")` to bool true. Fixed in
        // 0.15.1 — see CHANGELOG.
        if let Some(think_str) = &self.think {
            let trimmed = think_str.trim();
            // Skip empty / whitespace-only inputs — caller almost
            // certainly meant "don't set this field" rather than "send
            // think=empty-string" (which Ollama would reject anyway).
            if !trimmed.is_empty() {
                body["think"] = match trimmed.to_ascii_lowercase().as_str() {
                    "true" | "yes" | "on" | "1" => json!(true),
                    "false" | "no" | "off" | "0" => json!(false),
                    _ => json!(trimmed),
                };
            }
        }

        if let Some(keep_alive) = &self.keep_alive {
            body["keep_alive"] = json!(keep_alive);
        }

        // Options object for temperature, num_ctx, etc.
        let mut options = serde_json::Map::new();
        if let Some(temperature) = req.temperature {
            options.insert("temperature".to_string(), json!(temperature));
        }
        if let Some(num_ctx) = self.num_ctx {
            options.insert("num_ctx".to_string(), json!(num_ctx));
        }
        if let Some(stop_sequences) = &req.stop_sequences {
            if !stop_sequences.is_empty() {
                options.insert("stop".to_string(), json!(stop_sequences));
            }
        }
        if !options.is_empty() {
            body["options"] = Value::Object(options);
        }

        // Tools
        if let Some(tools) = &req.tools {
            let mapped_tools: Vec<Value> = tools
                .iter()
                .map(|tool| {
                    json!({
                        "type": "function",
                        "function": {
                            "name": tool.name,
                            "description": tool.description,
                            "parameters": tool.input_schema,
                        }
                    })
                })
                .collect();
            if !mapped_tools.is_empty() {
                body["tools"] = json!(mapped_tools);
            }
        }

        // Provider options passthrough
        if let Some(provider_options) = &req.provider_options {
            if let Some(map) = provider_options.as_object() {
                for (key, value) in map {
                    body[key] = value.clone();
                }
            }
        }

        body
    }

    fn extract_tool_calls(message: &Value) -> Vec<ToolCall> {
        message
            .get("tool_calls")
            .and_then(Value::as_array)
            .map(|calls| {
                calls
                    .iter()
                    .enumerate()
                    .filter_map(|(idx, call)| {
                        let function = call.get("function")?;
                        let name = function.get("name").and_then(Value::as_str)?.to_string();
                        let arguments = function.get("arguments");
                        let input = match arguments {
                            Some(Value::String(s)) => {
                                serde_json::from_str(s).unwrap_or_else(|_| Value::String(s.clone()))
                            }
                            Some(val) => val.clone(),
                            None => json!({}),
                        };
                        // Ollama native API doesn't include an id field; generate one.
                        let id = call
                            .get("id")
                            .and_then(Value::as_str)
                            .map(|s| s.to_string())
                            .unwrap_or_else(|| format!("call_{}", idx));
                        Some(ToolCall { id, name, input })
                    })
                    .collect()
            })
            .unwrap_or_default()
    }
}

#[async_trait]
impl ProviderImpl for OllamaProvider {
    async fn chat(&self, req: ChatRequest) -> Result<ChatResponse, MotosanError> {
        let body = self.build_request_body(&req, false);
        let mut attempt = 0;
        let payload: Value;

        loop {
            let response = match self.http.post(self.endpoint()).json(&body).send().await {
                Ok(response) => response,
                Err(error) => {
                    if attempt < self.retry_policy.max_retries && is_retryable_network_error(&error)
                    {
                        attempt += 1;
                        sleep_before_retry(&self.retry_policy, attempt, None).await;
                        continue;
                    }
                    return Err(MotosanError::Network(error.to_string()));
                }
            };

            let status = response.status();
            let retry_after = parse_retry_after(response.headers());
            let current_payload: Value = response
                .json()
                .await
                .map_err(|error| MotosanError::ProviderError(error.to_string()))?;

            if status.is_success() {
                payload = current_payload;
                break;
            }

            if attempt < self.retry_policy.max_retries && is_retryable_status(status.as_u16()) {
                attempt += 1;
                sleep_before_retry(&self.retry_policy, attempt, retry_after).await;
                continue;
            }

            let message = extract_error_message(&current_payload, "ollama request failed");
            return Err(map_http_error(status.as_u16(), message));
        }

        let message = payload.get("message");

        let content = message
            .and_then(|m| m.get("content"))
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();

        let thinking = message
            .and_then(|m| m.get("thinking"))
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();

        // If thinking is present and content is empty, use thinking as content
        let final_content = if content.is_empty() && !thinking.is_empty() {
            thinking
        } else if !thinking.is_empty() {
            format!("<think>{}</think>\n\n{}", thinking, content)
        } else {
            content
        };

        let tool_calls = message.map(Self::extract_tool_calls).unwrap_or_default();

        let stop_reason = if !tool_calls.is_empty() {
            StopReason::ToolUse
        } else {
            let done = payload
                .get("done")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            if done {
                StopReason::Stop
            } else {
                StopReason::Other
            }
        };

        let model = payload
            .get("model")
            .and_then(Value::as_str)
            .unwrap_or(DEFAULT_OLLAMA_MODEL)
            .to_string();

        let input_tokens = payload
            .get("prompt_eval_count")
            .and_then(Value::as_u64)
            .unwrap_or(0) as u32;
        let output_tokens = payload
            .get("eval_count")
            .and_then(Value::as_u64)
            .unwrap_or(0) as u32;

        Ok(ChatResponseBuilder::new(DEFAULT_OLLAMA_MODEL)
            .content(final_content)
            .tool_calls(tool_calls)
            .model(model)
            .usage(input_tokens, output_tokens)
            .stop_reason(stop_reason)
            .build())
    }

    async fn stream(&self, req: ChatRequest) -> Result<BoxStream, MotosanError> {
        let body = self.build_request_body(&req, true);
        let mut attempt = 0;

        let response = loop {
            let response = match self.http.post(self.endpoint()).json(&body).send().await {
                Ok(response) => response,
                Err(error) => {
                    if attempt < self.retry_policy.max_retries && is_retryable_network_error(&error)
                    {
                        attempt += 1;
                        sleep_before_retry(&self.retry_policy, attempt, None).await;
                        continue;
                    }
                    return Err(MotosanError::Network(error.to_string()));
                }
            };

            let status = response.status();
            if status.is_success() {
                break response;
            }

            let retry_after = parse_retry_after(response.headers());
            if attempt < self.retry_policy.max_retries && is_retryable_status(status.as_u16()) {
                attempt += 1;
                sleep_before_retry(&self.retry_policy, attempt, retry_after).await;
                continue;
            }

            let body_bytes = response.bytes().await.ok();
            if let Some(payload) = body_bytes
                .as_deref()
                .and_then(|bytes| serde_json::from_slice::<Value>(bytes).ok())
            {
                let message = extract_error_message(&payload, "ollama stream request failed");
                return Err(map_http_error(status.as_u16(), message));
            }

            let message = body_bytes
                .as_deref()
                .map(|bytes| String::from_utf8_lossy(bytes).trim().to_string())
                .filter(|message| !message.is_empty())
                .unwrap_or_else(|| "ollama stream request failed".to_string());
            return Err(map_http_error(status.as_u16(), message));
        };

        // NDJSON parsing: each line is a separate JSON object
        let byte_stream = response.bytes_stream();

        let ndjson_stream = NdjsonStream {
            inner: Box::pin(byte_stream),
            buffer: Vec::new(),
        };

        let adapter = OllamaStreamAdapter {
            inner: Box::pin(ndjson_stream),
            pending: std::collections::VecDeque::new(),
        };

        Ok(Box::pin(adapter))
    }
}

/// Stream adapter that parses Ollama NDJSON events and emits proper
/// 3-event sequences (Start + Args + End) for each tool call.
struct OllamaStreamAdapter {
    inner: Pin<Box<dyn Stream<Item = Result<String, MotosanError>> + Send>>,
    pending: std::collections::VecDeque<StreamEvent>,
}

impl Stream for OllamaStreamAdapter {
    type Item = Result<StreamEvent, MotosanError>;

    fn poll_next(
        mut self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> Poll<Option<Self::Item>> {
        if let Some(event) = self.pending.pop_front() {
            return Poll::Ready(Some(Ok(event)));
        }

        loop {
            match self.inner.as_mut().poll_next(cx) {
                Poll::Ready(Some(Ok(line))) => {
                    let payload: Value = match serde_json::from_str(&line) {
                        Ok(v) => v,
                        Err(_) => continue,
                    };

                    let done = payload
                        .get("done")
                        .and_then(Value::as_bool)
                        .unwrap_or(false);
                    if done {
                        return Poll::Ready(Some(Ok(StreamEvent::done())));
                    }

                    let message = payload.get("message");
                    let content = message
                        .and_then(|m| m.get("content"))
                        .and_then(Value::as_str)
                        .unwrap_or_default();
                    let thinking = message
                        .and_then(|m| m.get("thinking"))
                        .and_then(Value::as_str)
                        .unwrap_or_default();

                    let text = if !thinking.is_empty() && content.is_empty() {
                        thinking.to_string()
                    } else if !content.is_empty() {
                        content.to_string()
                    } else {
                        String::new()
                    };

                    if !text.is_empty() {
                        self.pending.push_back(StreamEvent::text(text));
                    }

                    // Emit 3-event sequence per tool call
                    let tool_calls = message
                        .map(OllamaProvider::extract_tool_calls)
                        .unwrap_or_default();
                    for tc in &tool_calls {
                        let args = serde_json::to_string(&tc.input).unwrap_or_default();
                        self.pending
                            .push_back(StreamEvent::tool_call_start(&tc.id, &tc.name));
                        self.pending
                            .push_back(StreamEvent::tool_call_args_with_id(&tc.id, args));
                        self.pending
                            .push_back(StreamEvent::tool_call_end_with_id(&tc.id));
                    }

                    if let Some(evt) = self.pending.pop_front() {
                        return Poll::Ready(Some(Ok(evt)));
                    }
                    continue;
                }
                Poll::Ready(Some(Err(e))) => {
                    // Inner NdjsonStream already yields a typed MotosanError; pass it
                    // through unchanged (re-wrapping would double the "stream error:" prefix).
                    return Poll::Ready(Some(Err(e)));
                }
                Poll::Ready(None) => return Poll::Ready(None),
                Poll::Pending => return Poll::Pending,
            }
        }
    }
}

/// A stream adapter that splits a byte stream on newline boundaries,
/// yielding complete lines (NDJSON).
struct NdjsonStream {
    inner: std::pin::Pin<
        Box<dyn futures_core::Stream<Item = Result<bytes::Bytes, reqwest::Error>> + Send>,
    >,
    buffer: Vec<u8>,
}

impl futures_core::Stream for NdjsonStream {
    type Item = Result<String, MotosanError>;

    fn poll_next(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Self::Item>> {
        use std::task::Poll;

        loop {
            // Check if we already have a complete line in the buffer
            if let Some(newline_pos) = self.buffer.iter().position(|&b| b == b'\n') {
                let line_bytes: Vec<u8> = self.buffer.drain(..=newline_pos).collect();
                let line = String::from_utf8_lossy(&line_bytes).trim().to_string();
                if line.is_empty() {
                    continue;
                }
                return Poll::Ready(Some(Ok(line)));
            }

            // No complete line yet, poll for more bytes
            match self.inner.as_mut().poll_next(cx) {
                Poll::Ready(Some(Ok(bytes))) => {
                    self.buffer.extend_from_slice(&bytes);
                    // Loop back to check for newline
                }
                Poll::Ready(Some(Err(e))) => {
                    return Poll::Ready(Some(Err(MotosanError::Stream(e.to_string()))));
                }
                Poll::Ready(None) => {
                    // Stream ended; emit any remaining data in buffer
                    if !self.buffer.is_empty() {
                        let remaining = String::from_utf8_lossy(&self.buffer).trim().to_string();
                        self.buffer.clear();
                        if !remaining.is_empty() {
                            return Poll::Ready(Some(Ok(remaining)));
                        }
                    }
                    return Poll::Ready(None);
                }
                Poll::Pending => return Poll::Pending,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::ChatRequest;

    fn req() -> ChatRequest {
        ChatRequest::builder()
            .message(crate::types::Message::user("hi"))
            .build()
    }

    #[tokio::test]
    async fn adapter_surfaces_inner_stream_error() {
        use tokio_stream::StreamExt;

        let inner = tokio_stream::iter(vec![Err(MotosanError::Stream("boom".to_string()))]);
        let mut adapter = OllamaStreamAdapter {
            inner: Box::pin(inner),
            pending: std::collections::VecDeque::new(),
        };

        let item = adapter.next().await.expect("one item");
        assert!(matches!(item, Err(MotosanError::Stream(msg)) if msg.contains("boom")));
    }

    #[test]
    fn think_truthy_strings_serialize_as_bool_true() {
        for input in &["true", "yes", "on", "1", "YES", "True", "  yes  "] {
            let provider =
                OllamaProvider::new("llama3", "http://x").with_think(Some(input.to_string()));
            let body = provider.build_request_body(&req(), false);
            assert_eq!(
                body["think"],
                serde_json::json!(true),
                "input {input:?} should serialize as bool true, got {:?}",
                body["think"]
            );
        }
    }

    #[test]
    fn think_falsy_strings_serialize_as_bool_false() {
        for input in &["false", "no", "off", "0", "NO", "False"] {
            let provider =
                OllamaProvider::new("llama3", "http://x").with_think(Some(input.to_string()));
            let body = provider.build_request_body(&req(), false);
            assert_eq!(
                body["think"],
                serde_json::json!(false),
                "input {input:?} should serialize as bool false, got {:?}",
                body["think"]
            );
        }
    }

    #[test]
    fn think_other_strings_pass_through_verbatim() {
        for input in &["low", "medium", "high", "custom-value"] {
            let provider =
                OllamaProvider::new("llama3", "http://x").with_think(Some(input.to_string()));
            let body = provider.build_request_body(&req(), false);
            assert_eq!(
                body["think"],
                serde_json::json!(input),
                "input {input:?} should pass through as string, got {:?}",
                body["think"]
            );
        }
    }

    #[test]
    fn think_not_set_omits_field_entirely() {
        let provider = OllamaProvider::new("llama3", "http://x").with_think(None);
        let body = provider.build_request_body(&req(), false);
        assert!(
            body.get("think").is_none(),
            "think field should be absent when not set; got body: {body}"
        );
    }

    #[test]
    fn think_empty_or_whitespace_only_omits_field_entirely() {
        // Defensive: callers passing "" or "   " almost certainly mean
        // "don't set this field" rather than "send think=empty-string".
        // Emitting a JSON empty string would be rejected by Ollama as
        // an unknown think value. Treat as unset.
        for input in &["", " ", "   ", "\t", "\n", "\t  \n"] {
            let provider =
                OllamaProvider::new("llama3", "http://x").with_think(Some(input.to_string()));
            let body = provider.build_request_body(&req(), false);
            assert!(
                body.get("think").is_none(),
                "input {input:?} (trimmed empty) should omit the think field; \
                 got body: {body}"
            );
        }
    }
}
