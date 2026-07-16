use std::sync::atomic::{AtomicU64, Ordering};

use crate::error::MotosanError;
use crate::models::{DEFAULT_GEMINI_CODE_ASSIST_MODEL, GEMINI_CODE_ASSIST_BASE_URL};
use crate::providers::gemini::GeminiProvider;
use crate::providers::{
    extract_error_message, extract_request_id, map_http_error, parse_retry_after, send_with_retry,
    ProviderImpl,
};
use crate::retry::RetryPolicy;
use crate::stream::{collect_stream, BoxStream};
use crate::types::{ChatRequest, ChatResponse, StopReason, StreamEvent, Usage};
use async_trait::async_trait;
use eventsource_stream::Eventsource;
use futures_core::Stream;
use reqwest::Client;
use serde_json::{json, Value};
use std::collections::VecDeque;
use std::pin::Pin;
use std::task::Poll;
use std::time::{SystemTime, UNIX_EPOCH};

static REQUEST_COUNTER: AtomicU64 = AtomicU64::new(0);
static TOOL_CALL_COUNTER: AtomicU64 = AtomicU64::new(0);

fn gen_request_id() -> String {
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    let n = REQUEST_COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("motosan-{ts}-{n:09}")
}

fn gen_tool_call_id(name: &str) -> String {
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    let n = TOOL_CALL_COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("{name}_{ts}_{n}")
}

#[derive(Debug, Clone)]
pub struct GeminiCodeAssistProvider {
    http: Client,
    access_token: String,
    project_id: String,
    model: String,
    base_url: String,
    retry_policy: RetryPolicy,
}

impl GeminiCodeAssistProvider {
    pub fn new(
        access_token: impl Into<String>,
        project_id: impl Into<String>,
        model: Option<String>,
        base_url: Option<String>,
    ) -> Self {
        Self {
            http: Client::new(),
            access_token: access_token.into(),
            project_id: project_id.into(),
            model: model.unwrap_or_else(|| DEFAULT_GEMINI_CODE_ASSIST_MODEL.to_string()),
            base_url: base_url.unwrap_or_else(|| GEMINI_CODE_ASSIST_BASE_URL.to_string()),
            retry_policy: RetryPolicy::default(),
        }
    }

    pub fn with_retry_policy(mut self, policy: RetryPolicy) -> Self {
        self.retry_policy = policy;
        self
    }

    fn stream_url(&self) -> String {
        format!("{}/v1internal:streamGenerateContent?alt=sse", self.base_url)
    }

    fn build_envelope(&self, req: &ChatRequest) -> Value {
        let model = req.model.as_deref().unwrap_or(&self.model);
        // Reuse the public Gemini API request builder for the inner request fields
        let inner = GeminiProvider::build_request(req);
        json!({
            "project": self.project_id,
            "model": model,
            "request": inner,
            "userAgent": "motosan-ai",
            "requestId": gen_request_id(),
        })
    }

    fn apply_auth(&self, request: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        request
            .header("authorization", format!("Bearer {}", self.access_token))
            .header("user-agent", "google-cloud-sdk vscode_cloudshelleditor/0.1")
            .header("x-goog-api-client", "gl-node/22.17.0")
            .header(
                "client-metadata",
                r#"{"ideType":"IDE_UNSPECIFIED","platform":"PLATFORM_UNSPECIFIED","pluginType":"GEMINI"}"#,
            )
    }
}

#[async_trait]
impl ProviderImpl for GeminiCodeAssistProvider {
    fn capabilities(&self) -> crate::types::ProviderCapabilities {
        crate::types::ProviderCapabilities::with_image()
    }

    async fn chat(&self, req: ChatRequest) -> Result<ChatResponse, MotosanError> {
        let model = req.model.clone().unwrap_or_else(|| self.model.clone());
        let stream = self.stream(req).await?;
        let mut response = collect_stream(stream).await?;
        if response.model.is_empty() {
            response.model = model;
        }
        Ok(response)
    }

    async fn stream(&self, req: ChatRequest) -> Result<BoxStream, MotosanError> {
        let url = self.stream_url();
        let body = self.build_envelope(&req);

        let response = send_with_retry(&self.retry_policy, || {
            self.apply_auth(
                self.http
                    .post(&url)
                    .header("content-type", "application/json"),
            )
            .json(&body)
        })
        .await?;

        let status = response.status().as_u16();
        if status != 200 {
            let retry_after = parse_retry_after(response.headers());
            let request_id = extract_request_id(response.headers());
            let payload: Value = response.json().await.unwrap_or(json!({}));
            let msg = extract_error_message(&payload, "Gemini Code Assist error");
            return Err(map_http_error(status, msg, retry_after, request_id));
        }

        let sse = response.bytes_stream().eventsource();
        let adapter = CodeAssistStreamAdapter {
            inner: Box::pin(sse),
            pending: VecDeque::new(),
            seen_tool_ids: std::collections::HashSet::new(),
        };
        Ok(Box::pin(adapter))
    }
}

struct CodeAssistStreamAdapter {
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
    pending: VecDeque<StreamEvent>,
    /// Track tool call IDs we've seen to detect duplicates (API may reuse IDs).
    seen_tool_ids: std::collections::HashSet<String>,
}

impl Stream for CodeAssistStreamAdapter {
    type Item = Result<StreamEvent, MotosanError>;

    fn poll_next(
        mut self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> Poll<Option<Self::Item>> {
        if let Some(ev) = self.pending.pop_front() {
            return Poll::Ready(Some(Ok(ev)));
        }

        loop {
            match self.inner.as_mut().poll_next(cx) {
                Poll::Ready(Some(Ok(event))) => {
                    if event.data.trim() == "[DONE]" || event.data.trim().is_empty() {
                        continue;
                    }
                    let chunk: Value = match serde_json::from_str(&event.data) {
                        Ok(v) => v,
                        Err(_) => continue,
                    };

                    // Unwrap outer "response" field
                    let response_data = match chunk.get("response") {
                        Some(r) => r.clone(),
                        None => continue,
                    };

                    let candidate = match response_data.get("candidates").and_then(|c| c.get(0)) {
                        Some(c) => c.clone(),
                        None => continue,
                    };

                    let parts = candidate
                        .get("content")
                        .and_then(|c| c.get("parts"))
                        .and_then(Value::as_array)
                        .cloned()
                        .unwrap_or_default();

                    let finish_reason = candidate.get("finishReason").and_then(Value::as_str);

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

                            // Use API-provided ID if present and not a duplicate
                            let provided_id = fc
                                .get("id")
                                .and_then(Value::as_str)
                                .filter(|id| !id.is_empty());
                            let id = match provided_id {
                                Some(id) if !self.seen_tool_ids.contains(id) => id.to_string(),
                                _ => gen_tool_call_id(&name),
                            };
                            self.seen_tool_ids.insert(id.clone());

                            self.pending
                                .push_back(StreamEvent::tool_call_start(&id, &name));
                            self.pending
                                .push_back(StreamEvent::tool_call_args_with_id(&id, &args_str));
                            self.pending
                                .push_back(StreamEvent::tool_call_end_with_id(id));
                        }
                    }

                    // Usage
                    if let Some(usage_meta) = response_data.get("usageMetadata") {
                        let prompt_tokens = usage_meta
                            .get("promptTokenCount")
                            .and_then(Value::as_u64)
                            .unwrap_or(0) as u32;
                        let cached_tokens = usage_meta
                            .get("cachedContentTokenCount")
                            .and_then(Value::as_u64)
                            .unwrap_or(0) as u32;
                        let output_tokens = usage_meta
                            .get("candidatesTokenCount")
                            .and_then(Value::as_u64)
                            .unwrap_or(0) as u32;
                        // input_tokens excludes cached tokens (matching pi-mono behaviour)
                        let input_tokens = prompt_tokens.saturating_sub(cached_tokens);
                        self.pending.push_back(StreamEvent::usage(Usage {
                            input_tokens,
                            output_tokens,
                            cache_creation_input_tokens: None,
                            cache_read_input_tokens: if cached_tokens > 0 {
                                Some(cached_tokens)
                            } else {
                                None
                            },
                        }));
                    }

                    // Terminal event
                    if let Some(reason) = finish_reason {
                        let stop_reason = match reason {
                            "STOP" if has_tool_calls => StopReason::ToolUse,
                            "STOP" => StopReason::EndTurn,
                            "MAX_TOKENS" => StopReason::MaxTokens,
                            _ if has_tool_calls => StopReason::ToolUse,
                            _ => StopReason::Other,
                        };
                        self.pending
                            .push_back(StreamEvent::done_with_stop_reason(stop_reason));
                    }

                    if let Some(first) = self.pending.pop_front() {
                        return Poll::Ready(Some(Ok(first)));
                    }
                    continue;
                }
                Poll::Ready(Some(Err(e))) => {
                    return Poll::Ready(Some(Err(MotosanError::Stream(e.to_string()))));
                }
                Poll::Ready(None) => return Poll::Ready(None),
                Poll::Pending => return Poll::Pending,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::Message;

    fn provider() -> GeminiCodeAssistProvider {
        GeminiCodeAssistProvider::new("ya29.fake", "test-project", None, None)
    }

    #[test]
    fn capabilities_support_image_only() {
        let p = GeminiCodeAssistProvider::new("ya29.fake", "test-project", None, None);
        let caps = p.capabilities();
        assert!(caps.supports_image);
        assert!(!caps.supports_document);
    }

    #[test]
    fn stream_url_format() {
        let p = provider();
        assert!(p.stream_url().contains("cloudcode-pa.googleapis.com"));
        assert!(p.stream_url().contains("v1internal:streamGenerateContent"));
        assert!(p.stream_url().contains("alt=sse"));
    }

    #[test]
    fn envelope_wraps_inner_request() {
        let req = ChatRequest::builder()
            .messages(vec![Message::user("hello")])
            .system("be helpful")
            .build();
        let body = provider().build_envelope(&req);
        assert_eq!(body["project"], "test-project");
        assert_eq!(body["model"], DEFAULT_GEMINI_CODE_ASSIST_MODEL);
        assert_eq!(body["userAgent"], "motosan-ai");
        assert!(body["requestId"].as_str().unwrap().starts_with("motosan-"));
        // Inner request has contents and systemInstruction
        assert!(body["request"]["contents"].is_array());
        assert_eq!(
            body["request"]["systemInstruction"]["parts"][0]["text"],
            "be helpful"
        );
    }

    #[test]
    fn envelope_uses_request_model_override() {
        let req = ChatRequest::builder()
            .messages(vec![Message::user("hi")])
            .model("gemini-2.5-pro")
            .build();
        let body = provider().build_envelope(&req);
        assert_eq!(body["model"], "gemini-2.5-pro");
    }

    #[test]
    fn request_ids_are_unique() {
        let p = provider();
        let req = ChatRequest::builder()
            .messages(vec![Message::user("hi")])
            .build();
        let id1 = p.build_envelope(&req)["requestId"]
            .as_str()
            .unwrap()
            .to_string();
        let id2 = p.build_envelope(&req)["requestId"]
            .as_str()
            .unwrap()
            .to_string();
        assert_ne!(id1, id2);
    }

    mod stream_tests {
        use super::*;
        use futures_core::Stream;
        use tokio_stream::StreamExt;

        fn make_sse(
            data: &str,
        ) -> Pin<
            Box<
                dyn Stream<
                        Item = Result<
                            eventsource_stream::Event,
                            eventsource_stream::EventStreamError<reqwest::Error>,
                        >,
                    > + Send,
            >,
        > {
            let items = vec![Ok(eventsource_stream::Event {
                event: String::new(),
                data: data.to_string(),
                id: String::new(),
                retry: None,
            })];
            Box::pin(tokio_stream::iter(items))
        }

        #[tokio::test]
        async fn adapter_surfaces_inner_stream_error() {
            use eventsource_stream::EventStreamError;

            let utf8 = String::from_utf8(vec![0xff]).unwrap_err();
            let inner = tokio_stream::iter(vec![Err(EventStreamError::Utf8(utf8))]);
            let mut adapter = CodeAssistStreamAdapter {
                inner: Box::pin(inner),
                pending: VecDeque::new(),
                seen_tool_ids: std::collections::HashSet::new(),
            };

            let item = adapter.next().await.expect("one item");
            assert!(matches!(item, Err(MotosanError::Stream(_))));
        }

        #[tokio::test]
        async fn adapter_emits_text_from_response_wrapper() {
            let json = r#"{"response":{"candidates":[{"content":{"parts":[{"text":"hi!"}],"role":"model"},"finishReason":"STOP"}],"usageMetadata":{"promptTokenCount":3,"candidatesTokenCount":2}}}"#;
            let mut adapter = CodeAssistStreamAdapter {
                inner: make_sse(json),
                pending: VecDeque::new(),
                seen_tool_ids: std::collections::HashSet::new(),
            };
            let events: Vec<_> = (&mut adapter).collect::<Result<Vec<_>, _>>().await.unwrap();
            let text: String = events
                .iter()
                .filter(|e| e.event_type == crate::types::StreamEventType::Text)
                .map(|e| e.content.as_str())
                .collect();
            assert_eq!(text, "hi!");
            let done = events.iter().find(|e| e.done).expect("no done event");
            assert_eq!(done.stop_reason, Some(StopReason::EndTurn));
        }

        #[tokio::test]
        async fn adapter_uses_api_provided_tool_id() {
            let json = r#"{"response":{"candidates":[{"content":{"parts":[{"functionCall":{"name":"search","args":{"q":"rust"},"id":"api-provided-id"}}],"role":"model"},"finishReason":"STOP"}],"usageMetadata":{"promptTokenCount":5,"candidatesTokenCount":3}}}"#;
            let mut adapter = CodeAssistStreamAdapter {
                inner: make_sse(json),
                pending: VecDeque::new(),
                seen_tool_ids: std::collections::HashSet::new(),
            };
            let events: Vec<_> = (&mut adapter).collect::<Result<Vec<_>, _>>().await.unwrap();
            let start = events
                .iter()
                .find(|e| e.event_type == crate::types::StreamEventType::ToolCallStart)
                .unwrap();
            assert_eq!(start.tool_call_id.as_deref(), Some("api-provided-id"));
        }

        #[tokio::test]
        async fn adapter_generates_id_when_absent() {
            let json = r#"{"response":{"candidates":[{"content":{"parts":[{"functionCall":{"name":"calc","args":{}}}],"role":"model"},"finishReason":"STOP"}],"usageMetadata":{"promptTokenCount":2,"candidatesTokenCount":1}}}"#;
            let mut adapter = CodeAssistStreamAdapter {
                inner: make_sse(json),
                pending: VecDeque::new(),
                seen_tool_ids: std::collections::HashSet::new(),
            };
            let events: Vec<_> = (&mut adapter).collect::<Result<Vec<_>, _>>().await.unwrap();
            let start = events
                .iter()
                .find(|e| e.event_type == crate::types::StreamEventType::ToolCallStart)
                .unwrap();
            let id = start.tool_call_id.as_deref().unwrap_or("");
            assert!(!id.is_empty());
            assert!(id.starts_with("calc_"));
        }
    }
}
