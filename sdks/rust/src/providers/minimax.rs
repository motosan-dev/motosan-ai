use crate::error::MotosanError;
use crate::models::DEFAULT_MINIMAX_MODEL;
use crate::providers::{
    extract_error_message, is_retryable_network_error, is_retryable_status, map_http_error,
    parse_retry_after, sleep_before_retry, ChatResponseBuilder, ProviderImpl,
};
use crate::retry::RetryPolicy;
use crate::stream::BoxStream;
use crate::types::{ChatRequest, ChatResponse, Role, StopReason, ToolCall};
use async_trait::async_trait;
use eventsource_stream::Eventsource;
use reqwest::Client;
use serde_json::{json, Value};
use tokio_stream::StreamExt;

pub struct MinimaxProvider {
    http: Client,
    api_key: String,
    model: String,
    base_url: String,
    expose_reasoning: bool,
    retry_policy: RetryPolicy,
}

impl MinimaxProvider {
    pub fn new(
        api_key: impl Into<String>,
        model: Option<String>,
        base_url: Option<String>,
    ) -> Self {
        Self {
            http: Client::new(),
            api_key: api_key.into(),
            model: model.unwrap_or_else(|| DEFAULT_MINIMAX_MODEL.to_string()),
            base_url: base_url.unwrap_or_else(|| "https://api.minimax.io/v1".to_string()),
            expose_reasoning: false,
            retry_policy: RetryPolicy::default(),
        }
    }

    pub fn with_expose_reasoning(mut self, expose_reasoning: bool) -> Self {
        self.expose_reasoning = expose_reasoning;
        self
    }

    pub fn with_retry_policy(mut self, retry_policy: RetryPolicy) -> Self {
        self.retry_policy = retry_policy;
        self
    }

    fn endpoint(&self) -> String {
        format!("{}/chat/completions", self.base_url.trim_end_matches('/'))
    }

    fn minimax_payload_error(payload: &Value) -> Option<(u16, String)> {
        let base_resp = payload.get("base_resp")?;
        let status_code = base_resp.get("status_code").and_then(Value::as_i64)?;
        if status_code == 0 {
            return None;
        }

        let message = base_resp
            .get("status_msg")
            .and_then(Value::as_str)
            .unwrap_or("minimax request failed")
            .to_string();

        let message_lower = message.to_ascii_lowercase();
        let mapped_status = match status_code {
            2049 => 401,
            1008 => 429,
            4000..=4999 => 400,
            5000..=5999 => 500,
            _ if message_lower.contains("invalid api key")
                || message_lower.contains("unauthorized")
                || message_lower.contains("authentication") =>
            {
                401
            }
            _ if message_lower.contains("insufficient balance")
                || message_lower.contains("quota")
                || message_lower.contains("rate limit")
                || message_lower.contains("too many") =>
            {
                429
            }
            _ => 500,
        };
        Some((mapped_status, message))
    }

    fn resolve_expose_reasoning(&self, req: &ChatRequest) -> bool {
        req.provider_options
            .as_ref()
            .and_then(Value::as_object)
            .and_then(|opts| opts.get("minimax_expose_reasoning"))
            .and_then(Value::as_bool)
            .unwrap_or(self.expose_reasoning)
    }

    fn strip_think_blocks(content: &str) -> String {
        let mut remaining = content;
        let mut sanitized = String::new();

        while let Some(open_idx) = remaining.find("<think>") {
            sanitized.push_str(&remaining[..open_idx]);
            let after_open = &remaining[open_idx + "<think>".len()..];
            if let Some(close_idx) = after_open.find("</think>") {
                remaining = &after_open[close_idx + "</think>".len()..];
            } else {
                remaining = "";
                break;
            }
        }

        sanitized.push_str(remaining);
        sanitized.trim().to_string()
    }

    fn first_non_empty_text(value: Option<&Value>) -> Option<&str> {
        value
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|text| !text.is_empty())
    }

    fn extract_chat_content(payload: &Value, expose_reasoning: bool) -> String {
        let message = payload
            .get("choices")
            .and_then(Value::as_array)
            .and_then(|choices| choices.first())
            .and_then(|choice| choice.get("message"));

        let content = Self::first_non_empty_text(message.and_then(|msg| msg.get("content")));
        let reasoning =
            Self::first_non_empty_text(message.and_then(|msg| msg.get("reasoning_content")));

        if expose_reasoning {
            return content.or(reasoning).unwrap_or_default().to_string();
        }

        if let Some(content_text) = content {
            let sanitized = Self::strip_think_blocks(content_text);
            if !sanitized.is_empty() {
                return sanitized;
            }
        }

        reasoning.map(Self::strip_think_blocks).unwrap_or_default()
    }

    fn extract_stream_delta_text(payload: &Value, expose_reasoning: bool) -> String {
        let delta = payload
            .get("choices")
            .and_then(Value::as_array)
            .and_then(|choices| choices.first())
            .and_then(|choice| choice.get("delta"));

        let content = Self::first_non_empty_text(delta.and_then(|d| d.get("content")));
        let reasoning = Self::first_non_empty_text(delta.and_then(|d| d.get("reasoning_content")));

        if expose_reasoning {
            return content.or(reasoning).unwrap_or_default().to_string();
        }

        if let Some(content_text) = content {
            let sanitized = Self::strip_think_blocks(content_text);
            if !sanitized.is_empty() {
                return sanitized;
            }
        }

        reasoning.map(Self::strip_think_blocks).unwrap_or_default()
    }
}

struct MinimaxRequestBuilder {
    req: ChatRequest,
    default_model: String,
    stream: bool,
}

impl MinimaxRequestBuilder {
    fn new(req: ChatRequest, default_model: String) -> Self {
        Self {
            req,
            default_model,
            stream: false,
        }
    }

    fn stream(mut self, stream: bool) -> Self {
        self.stream = stream;
        self
    }

    fn build(self) -> Value {
        let model = self
            .req
            .model
            .clone()
            .unwrap_or_else(|| self.default_model.clone());
        let mut system_parts = Vec::new();
        if let Some(system) = &self.req.system {
            let trimmed = system.trim();
            if !trimmed.is_empty() {
                system_parts.push(trimmed.to_string());
            }
        }

        let mut messages: Vec<(String, String, Option<String>)> = Vec::new();
        for message in &self.req.messages {
            match message.role {
                Role::System => {
                    let trimmed = message.content.trim();
                    if !trimmed.is_empty() {
                        system_parts.push(trimmed.to_string());
                    }
                }
                Role::User => messages.push(("user".to_string(), message.content.clone(), None)),
                Role::Assistant => {
                    messages.push(("assistant".to_string(), message.content.clone(), None))
                }
                Role::Tool => {
                    if let Some(tool_call_id) = &message.tool_call_id {
                        messages.push((
                            "tool".to_string(),
                            message.content.clone(),
                            Some(tool_call_id.clone()),
                        ));
                    }
                }
            }
        }

        if !system_parts.is_empty() {
            let merged_system = system_parts.join("\n\n");
            if let Some((_, content, _)) = messages.iter_mut().find(|(role, _, _)| role == "user") {
                *content = format!("{}\n\n{}", merged_system, content);
            } else {
                messages.insert(0, ("user".to_string(), merged_system, None));
            }
        }

        let messages: Vec<Value> = messages
            .into_iter()
            .map(|(role, content, tool_call_id)| {
                let mut message = json!({"role": role, "content": content});
                if let Some(tool_call_id) = tool_call_id {
                    message["tool_call_id"] = json!(tool_call_id);
                }
                message
            })
            .collect();

        let mut body = json!({
            "model": model,
            "messages": messages,
        });

        if self.stream {
            body["stream"] = json!(true);
        }
        if let Some(temperature) = self.req.temperature {
            body["temperature"] = json!(temperature);
        }
        if let Some(max_tokens) = self.req.max_tokens {
            body["max_tokens"] = json!(max_tokens);
        }
        if let Some(tools) = self.req.tools {
            let mapped_tools: Vec<Value> = tools
                .into_iter()
                .map(|tool| {
                    json!({
                        "type": "function",
                        "function": {
                            "name": tool.name,
                            "description": tool.description.unwrap_or_default(),
                            "parameters": tool.input_schema.unwrap_or_else(|| json!({"type":"object","properties":{}})),
                        }
                    })
                })
                .collect();
            if !mapped_tools.is_empty() {
                body["tools"] = json!(mapped_tools);
            }
        }
        if let Some(provider_options) = self.req.provider_options {
            if let Some(map) = provider_options.as_object() {
                for (key, value) in map {
                    if key == "minimax_expose_reasoning" {
                        continue;
                    }
                    body[key] = value.clone();
                }
            }
        }

        body
    }
}

#[async_trait]
impl ProviderImpl for MinimaxProvider {
    async fn chat(&self, req: ChatRequest) -> Result<ChatResponse, MotosanError> {
        let expose_reasoning = self.resolve_expose_reasoning(&req);
        let body = MinimaxRequestBuilder::new(req, self.model.clone()).build();
        let mut attempt = 0;
        let payload: Value;
        loop {
            let response = match self
                .http
                .post(self.endpoint())
                .header("authorization", format!("Bearer {}", self.api_key))
                .json(&body)
                .send()
                .await
            {
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
                if let Some((mapped_status, message)) =
                    Self::minimax_payload_error(&current_payload)
                {
                    return Err(map_http_error(mapped_status, message));
                }
                payload = current_payload;
                break;
            }

            if attempt < self.retry_policy.max_retries && is_retryable_status(status.as_u16()) {
                attempt += 1;
                sleep_before_retry(&self.retry_policy, attempt, retry_after).await;
                continue;
            }

            let message = extract_error_message(&current_payload, "minimax request failed");
            return Err(map_http_error(status.as_u16(), message));
        }

        let content = Self::extract_chat_content(&payload, expose_reasoning);

        let tool_calls = payload
            .get("choices")
            .and_then(Value::as_array)
            .and_then(|choices| choices.first())
            .and_then(|choice| choice.get("message"))
            .and_then(|message| message.get("tool_calls"))
            .and_then(Value::as_array)
            .map(|calls| {
                calls
                    .iter()
                    .filter_map(|call| {
                        let id = call.get("id").and_then(Value::as_str)?.to_string();
                        let function = call.get("function")?;
                        let name = function.get("name").and_then(Value::as_str)?.to_string();
                        let arguments = function
                            .get("arguments")
                            .and_then(Value::as_str)
                            .unwrap_or("{}");
                        let input = serde_json::from_str(arguments)
                            .unwrap_or_else(|_| Value::String(arguments.to_string()));
                        Some(ToolCall { id, name, input })
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();

        let stop_reason = match payload
            .get("choices")
            .and_then(Value::as_array)
            .and_then(|choices| choices.first())
            .and_then(|choice| choice.get("finish_reason"))
            .and_then(Value::as_str)
        {
            Some("stop") => StopReason::Stop,
            Some("length") => StopReason::MaxTokens,
            Some("tool_calls") => StopReason::ToolUse,
            _ => StopReason::Other,
        };

        let model = payload
            .get("model")
            .and_then(Value::as_str)
            .unwrap_or(DEFAULT_MINIMAX_MODEL)
            .to_string();
        let input_tokens = payload
            .get("usage")
            .and_then(|usage| usage.get("prompt_tokens"))
            .and_then(Value::as_u64)
            .unwrap_or(0) as u32;
        let output_tokens = payload
            .get("usage")
            .and_then(|usage| usage.get("completion_tokens"))
            .and_then(Value::as_u64)
            .unwrap_or(0) as u32;

        Ok(ChatResponseBuilder::new(DEFAULT_MINIMAX_MODEL)
            .content(content)
            .tool_calls(tool_calls)
            .model(model)
            .usage(input_tokens, output_tokens)
            .stop_reason(stop_reason)
            .build())
    }

    async fn stream(&self, req: ChatRequest) -> Result<BoxStream, MotosanError> {
        let expose_reasoning = self.resolve_expose_reasoning(&req);
        let body = MinimaxRequestBuilder::new(req, self.model.clone())
            .stream(true)
            .build();
        let mut attempt = 0;
        let response = loop {
            let response = match self
                .http
                .post(self.endpoint())
                .header("authorization", format!("Bearer {}", self.api_key))
                .json(&body)
                .send()
                .await
            {
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

            let message = response
                .text()
                .await
                .unwrap_or_else(|_| "minimax stream request failed".to_string());
            return Err(map_http_error(status.as_u16(), message));
        };

        let parsed_stream = response
            .bytes_stream()
            .eventsource()
            .filter_map(move |event| match event {
                Ok(event) => {
                    if event.data.trim() == "[DONE]" {
                        return Some(crate::types::StreamEvent {
                            content: String::new(),
                            done: true,
                        });
                    }

                    let payload: Value = serde_json::from_str(&event.data).ok()?;
                    let text = Self::extract_stream_delta_text(&payload, expose_reasoning);
                    if text.is_empty() {
                        return None;
                    }
                    Some(crate::types::StreamEvent {
                        content: text,
                        done: false,
                    })
                }
                Err(_) => None,
            });

        Ok(Box::pin(parsed_stream))
    }
}
