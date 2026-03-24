const DEFAULT_MAX_TOKENS: u32 = 4096;

use crate::error::MotosanError;
use crate::models::DEFAULT_ANTHROPIC_MODEL;
use crate::providers::{
    extract_error_message, is_retryable_network_error, is_retryable_status, map_http_error,
    parse_retry_after, sleep_before_retry, ChatResponseBuilder, ProviderImpl,
};
use crate::retry::RetryPolicy;
use crate::stream::BoxStream;
use crate::types::{
    ChatRequest, ChatResponse, ContentBlock, DocumentSource, ImageSource, McpToolConfig, Role,
    StopReason, StreamEvent, SystemBlock, ToolCall, ToolChoice,
};
use async_trait::async_trait;
use eventsource_stream::Eventsource;
use futures_core::Stream;
use reqwest::Client;
use serde_json::{json, Value};
use std::pin::Pin;
use std::task::Poll;

pub struct AnthropicProvider {
    http: Client,
    api_key: String,
    model: String,
    base_url: String,
    retry_policy: RetryPolicy,
}

impl AnthropicProvider {
    pub fn new(
        api_key: impl Into<String>,
        model: Option<String>,
        base_url: Option<String>,
    ) -> Self {
        Self {
            http: Client::new(),
            api_key: api_key.into(),
            model: model.unwrap_or_else(|| DEFAULT_ANTHROPIC_MODEL.to_string()),
            base_url: base_url.unwrap_or_else(|| "https://api.anthropic.com".to_string()),
            retry_policy: RetryPolicy::default(),
        }
    }

    pub fn with_retry_policy(mut self, retry_policy: RetryPolicy) -> Self {
        self.retry_policy = retry_policy;
        self
    }

    fn endpoint(&self) -> String {
        format!("{}/v1/messages", self.base_url)
    }

    fn is_setup_token(token: &str) -> bool {
        token.starts_with("sk-ant-oat01-")
    }

    fn apply_auth(&self, request: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        if Self::is_setup_token(&self.api_key) {
            request
                .header("authorization", format!("Bearer {}", self.api_key))
                .header(
                    "anthropic-beta",
                    "claude-code-20250219,oauth-2025-04-20,fine-grained-tool-streaming-2025-05-14,interleaved-thinking-2025-05-14",
                )
                .header("user-agent", "claude-code/1.0.33")
                .header("x-app", "cli")
        } else {
            request.header("x-api-key", &self.api_key)
        }
    }

    fn apply_beta_header(
        request: reqwest::RequestBuilder,
        has_mcp: bool,
    ) -> reqwest::RequestBuilder {
        if has_mcp {
            request.header("anthropic-beta", "mcp-client-2025-11-20")
        } else {
            request
        }
    }

    fn with_auth_hint(status_code: u16, message: String, is_setup_token: bool) -> String {
        if status_code != 401 {
            return message;
        }

        if is_setup_token {
            format!(
                "{message}. Hint: setup tokens (sk-ant-oat01-*) require Authorization: Bearer and anthropic-beta: oauth-2025-04-20"
            )
        } else {
            format!(
                "{message}. Hint: Anthropic API keys use x-api-key; setup tokens (sk-ant-oat01-*) use Authorization: Bearer plus anthropic-beta: oauth-2025-04-20"
            )
        }
    }
}

/// Serialize a [`ContentBlock`] to the Anthropic JSON format.
fn serialize_content_block(block: &ContentBlock) -> Value {
    match block {
        ContentBlock::Text { text } => json!({"type": "text", "text": text}),
        ContentBlock::Image { source } => match source {
            ImageSource::Base64 { media_type, data } => json!({
                "type": "image",
                "source": {"type": "base64", "media_type": media_type, "data": data}
            }),
            ImageSource::Url { url } => json!({
                "type": "image",
                "source": {"type": "url", "url": url}
            }),
        },
        ContentBlock::Document { source } => match source {
            DocumentSource::Base64 { media_type, data } => json!({
                "type": "document",
                "source": {"type": "base64", "media_type": media_type, "data": data}
            }),
            DocumentSource::Url { url } => json!({
                "type": "document",
                "source": {"type": "url", "url": url}
            }),
        },
    }
}

/// Serialize a slice of [`SystemBlock`]s to the Anthropic JSON array format.
fn serialize_system_blocks(blocks: &[SystemBlock]) -> Value {
    let arr: Vec<Value> = blocks
        .iter()
        .map(|b| {
            let mut obj = json!({"type": "text", "text": b.text});
            if b.cache_control {
                obj["cache_control"] = json!({"type": "ephemeral"});
            }
            obj
        })
        .collect();
    json!(arr)
}

/// Serialize a [`McpToolConfig`] to the Anthropic `mcp_toolset` JSON format.
fn serialize_mcp_tool_config(config: &McpToolConfig) -> Value {
    match config {
        McpToolConfig::All { mcp_server_name } => json!({
            "type": "mcp_toolset",
            "server_label": mcp_server_name,
        }),
        McpToolConfig::Allowed {
            mcp_server_name,
            allowed_tools,
        } => json!({
            "type": "mcp_toolset",
            "server_label": mcp_server_name,
            "allowed_tools": allowed_tools,
        }),
        McpToolConfig::Denied {
            mcp_server_name,
            denied_tools,
        } => json!({
            "type": "mcp_toolset",
            "server_label": mcp_server_name,
            "denied_tools": denied_tools,
        }),
    }
}

struct AnthropicRequestBuilder {
    req: ChatRequest,
    default_model: String,
    stream: bool,
    oauth: bool,
}

impl AnthropicRequestBuilder {
    fn new(req: ChatRequest, default_model: String, oauth: bool) -> Self {
        Self {
            req,
            default_model,
            stream: false,
            oauth,
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
        let explicit_system = self.req.system.clone();
        let mut extracted_systems = Vec::new();
        let mut messages = Vec::new();

        for message in &self.req.messages {
            match message.role {
                Role::System => extracted_systems.push(message.content.clone()),
                Role::User => {
                    if !message.content_blocks.is_empty() {
                        // Use structured content blocks (vision/multimodal/document)
                        let mut blocks: Vec<Value> = message
                            .content_blocks
                            .iter()
                            .map(serialize_content_block)
                            .collect();
                        // Apply cache_control to the last content block
                        if message.cache {
                            if let Some(last) = blocks.last_mut() {
                                last["cache_control"] = json!({"type": "ephemeral"});
                            }
                        }
                        messages.push(json!({"role": "user", "content": blocks}));
                    } else if message.cache {
                        // Cached plain-text message: serialize as content block with cache_control
                        messages.push(json!({"role": "user", "content": [{"type": "text", "text": message.content, "cache_control": {"type": "ephemeral"}}]}));
                    } else if self.oauth {
                        messages.push(json!({"role": "user", "content": [{"type": "text", "text": message.content}]}));
                    } else {
                        messages.push(json!({"role": "user", "content": message.content}));
                    }
                }
                Role::Assistant => {
                    if message.tool_calls.is_empty() {
                        if message.cache {
                            messages.push(json!({"role": "assistant", "content": [{"type": "text", "text": message.content, "cache_control": {"type": "ephemeral"}}]}));
                        } else {
                            messages.push(json!({"role": "assistant", "content": message.content}));
                        }
                    } else {
                        let mut blocks = Vec::new();
                        if !message.content.is_empty() {
                            blocks.push(json!({"type": "text", "text": message.content}));
                        }
                        for tool_call in &message.tool_calls {
                            blocks.push(json!({
                                "type": "tool_use",
                                "id": tool_call.id,
                                "name": tool_call.name,
                                "input": tool_call.input,
                            }));
                        }
                        if message.cache {
                            if let Some(last) = blocks.last_mut() {
                                last["cache_control"] = json!({"type": "ephemeral"});
                            }
                        }
                        messages.push(json!({"role": "assistant", "content": blocks}));
                    }
                }
                Role::Tool => {
                    if let Some(tool_use_id) = &message.tool_call_id {
                        messages.push(json!({
                            "role": "user",
                            "content": [{
                                "type": "tool_result",
                                "tool_use_id": tool_use_id,
                                "content": message.content,
                            }],
                        }));
                    }
                }
            }
        }

        let system = explicit_system.or_else(|| {
            if extracted_systems.is_empty() {
                None
            } else {
                Some(extracted_systems.join("\n"))
            }
        });

        let mut body = json!({
            "model": model,
            "messages": messages,
        });

        if self.stream {
            body["stream"] = json!(true);
        }
        // Priority: system_blocks > system string.
        if let Some(ref blocks) = self.req.system_blocks {
            if !blocks.is_empty() {
                body["system"] = serialize_system_blocks(blocks);
            }
        } else if let Some(system_prompt) = system {
            if self.oauth || self.req.system_cache {
                body["system"] = json!([{"type": "text", "text": system_prompt, "cache_control": {"type": "ephemeral"}}]);
            } else {
                body["system"] = json!(system_prompt);
            }
        }
        // When thinking is enabled, Anthropic requires temperature=1.0.
        if self.req.thinking.is_some() {
            body["temperature"] = json!(1.0);
        } else if let Some(temperature) = self.req.temperature {
            body["temperature"] = json!(temperature);
        }
        let max_tokens = self.req.max_tokens.unwrap_or(DEFAULT_MAX_TOKENS);
        body["max_tokens"] = json!(max_tokens);
        if let Some(ref thinking) = self.req.thinking {
            body["thinking"] = json!({
                "type": "enabled",
                "budget_tokens": thinking.budget_tokens,
            });
        }
        {
            let mut all_tools: Vec<Value> = Vec::new();
            if let Some(tools) = self.req.tools {
                for tool in tools {
                    let mut obj = json!({
                        "name": tool.name,
                        "description": tool.description.unwrap_or_default(),
                        "input_schema": tool.input_schema.unwrap_or_else(|| json!({"type":"object","properties":{}})),
                    });
                    if tool.cache {
                        obj["cache_control"] = json!({"type": "ephemeral"});
                    }
                    all_tools.push(obj);
                }
            }
            if let Some(ref mcp_tool_configs) = self.req.mcp_tool_configs {
                for config in mcp_tool_configs {
                    all_tools.push(serialize_mcp_tool_config(config));
                }
            }
            if !all_tools.is_empty() {
                body["tools"] = json!(all_tools);
            }
        }
        if let Some(tool_choice) = &self.req.tool_choice {
            match tool_choice {
                ToolChoice::Auto => {
                    body["tool_choice"] = json!({"type": "auto"});
                }
                ToolChoice::Required => {
                    body["tool_choice"] = json!({"type": "any"});
                }
                ToolChoice::None => {
                    // Anthropic doesn't have a "none" tool_choice; remove tools to prevent calls
                    body.as_object_mut().map(|m| m.remove("tools"));
                }
                ToolChoice::Tool { name } => {
                    body["tool_choice"] = json!({"type": "tool", "name": name});
                }
            }
        }
        if let Some(ref stop_sequences) = self.req.stop_sequences {
            if !stop_sequences.is_empty() {
                body["stop_sequences"] = json!(stop_sequences);
            }
        }
        if let Some(mcp_servers) = &self.req.mcp_servers {
            let servers: Vec<Value> = mcp_servers
                .iter()
                .map(|s| {
                    let mut obj = json!({
                        "type": s.kind,
                        "url": s.url,
                        "name": s.name,
                    });
                    if let Some(token) = &s.authorization_token {
                        obj["authorization_token"] = json!(token);
                    }
                    obj
                })
                .collect();
            if !servers.is_empty() {
                body["mcp_servers"] = json!(servers);
            }
        }
        if let Some(provider_options) = self.req.provider_options {
            if let Some(map) = provider_options.as_object() {
                for (key, value) in map {
                    body[key] = value.clone();
                }
            }
        }

        body
    }
}

#[async_trait]
impl ProviderImpl for AnthropicProvider {
    async fn chat(&self, req: ChatRequest) -> Result<ChatResponse, MotosanError> {
        let is_oauth = Self::is_setup_token(&self.api_key);

        // OAuth tokens require streaming + Claude Code identity.
        // Redirect to stream path and collect the full response.
        if is_oauth {
            let stream = self.stream(req).await?;
            let mut response = crate::stream::collect_stream(stream).await;
            response.model = self.model.clone();
            return Ok(response);
        }

        let has_mcp = req.mcp_servers.as_ref().is_some_and(|s| !s.is_empty())
            || req.mcp_tool_configs.as_ref().is_some_and(|c| !c.is_empty());
        let body = AnthropicRequestBuilder::new(req, self.model.clone(), is_oauth).build();
        let mut attempt = 0;
        let payload: Value;
        loop {
            let request = self
                .http
                .post(self.endpoint())
                .header("anthropic-version", "2023-06-01")
                .json(&body);
            let request = Self::apply_beta_header(request, has_mcp);
            let response = match self.apply_auth(request).send().await {
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

            let message = extract_error_message(&current_payload, "anthropic request failed");
            let message = Self::with_auth_hint(
                status.as_u16(),
                message,
                Self::is_setup_token(&self.api_key),
            );
            return Err(map_http_error(status.as_u16(), message));
        }

        let content_blocks = payload.get("content").and_then(Value::as_array);

        let content = content_blocks
            .map(|items| {
                items
                    .iter()
                    .filter(|item| item.get("type").and_then(Value::as_str) != Some("thinking"))
                    .filter_map(|item| item.get("text").and_then(Value::as_str))
                    .collect::<Vec<_>>()
                    .join("")
            })
            .unwrap_or_default();

        let thinking = content_blocks.and_then(|items| {
            let parts: Vec<&str> = items
                .iter()
                .filter(|item| item.get("type").and_then(Value::as_str) == Some("thinking"))
                .filter_map(|item| item.get("thinking").and_then(Value::as_str))
                .collect();
            if parts.is_empty() {
                None
            } else {
                Some(parts.join(""))
            }
        });

        let tool_calls = payload
            .get("content")
            .and_then(Value::as_array)
            .map(|items| {
                items
                    .iter()
                    .filter(|item| item.get("type").and_then(Value::as_str) == Some("tool_use"))
                    .map(|item| ToolCall {
                        id: item
                            .get("id")
                            .and_then(Value::as_str)
                            .unwrap_or_default()
                            .to_string(),
                        name: item
                            .get("name")
                            .and_then(Value::as_str)
                            .unwrap_or_default()
                            .to_string(),
                        input: item.get("input").cloned().unwrap_or_else(|| json!({})),
                    })
                    .filter(|call| !call.id.is_empty() && !call.name.is_empty())
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();

        let model = payload
            .get("model")
            .and_then(Value::as_str)
            .unwrap_or(DEFAULT_ANTHROPIC_MODEL)
            .to_string();

        let usage_obj = payload.get("usage");
        let input_tokens = usage_obj
            .and_then(|u| u.get("input_tokens"))
            .and_then(Value::as_u64)
            .unwrap_or(0) as u32;
        let output_tokens = usage_obj
            .and_then(|u| u.get("output_tokens"))
            .and_then(Value::as_u64)
            .unwrap_or(0) as u32;
        let cache_creation_input_tokens = usage_obj
            .and_then(|u| u.get("cache_creation_input_tokens"))
            .and_then(Value::as_u64)
            .map(|v| v as u32);
        let cache_read_input_tokens = usage_obj
            .and_then(|u| u.get("cache_read_input_tokens"))
            .and_then(Value::as_u64)
            .map(|v| v as u32);

        let stop_reason = match payload.get("stop_reason").and_then(Value::as_str) {
            Some("end_turn") => StopReason::EndTurn,
            Some("max_tokens") => StopReason::MaxTokens,
            Some("tool_use") => StopReason::ToolUse,
            Some("stop_sequence") => StopReason::StopSequence,
            Some("stop") => StopReason::Stop,
            _ => StopReason::Other,
        };

        Ok(ChatResponseBuilder::new(DEFAULT_ANTHROPIC_MODEL)
            .content(content)
            .thinking(thinking)
            .tool_calls(tool_calls)
            .model(model)
            .usage(input_tokens, output_tokens)
            .cache_usage(cache_creation_input_tokens, cache_read_input_tokens)
            .stop_reason(stop_reason)
            .build())
    }

    async fn stream(&self, req: ChatRequest) -> Result<BoxStream, MotosanError> {
        let is_oauth = Self::is_setup_token(&self.api_key);
        let has_mcp = req.mcp_servers.as_ref().is_some_and(|s| !s.is_empty())
            || req.mcp_tool_configs.as_ref().is_some_and(|c| !c.is_empty());
        let body = if is_oauth {
            // OAuth: build body manually with system as array of blocks
            let (messages, extracted_system) = {
                let mut msgs = Vec::new();
                let mut sys_parts = Vec::new();
                for message in &req.messages {
                    match message.role {
                        Role::System => sys_parts.push(message.content.clone()),
                        Role::User => {
                            if !message.content_blocks.is_empty() {
                                let mut blocks: Vec<Value> = message
                                    .content_blocks
                                    .iter()
                                    .map(serialize_content_block)
                                    .collect();
                                if message.cache {
                                    if let Some(last) = blocks.last_mut() {
                                        last["cache_control"] = json!({"type": "ephemeral"});
                                    }
                                }
                                msgs.push(json!({"role": "user", "content": blocks}));
                            } else if message.cache {
                                msgs.push(json!({"role": "user", "content": [{"type": "text", "text": message.content, "cache_control": {"type": "ephemeral"}}]}));
                            } else {
                                msgs.push(json!({"role": "user", "content": [{"type": "text", "text": message.content}]}));
                            }
                        }
                        Role::Assistant => {
                            if message.tool_calls.is_empty() {
                                if message.cache {
                                    msgs.push(json!({"role": "assistant", "content": [{"type": "text", "text": message.content, "cache_control": {"type": "ephemeral"}}]}));
                                } else {
                                    msgs.push(
                                        json!({"role": "assistant", "content": message.content}),
                                    );
                                }
                            } else {
                                let mut blocks = Vec::new();
                                if !message.content.is_empty() {
                                    blocks.push(json!({"type": "text", "text": message.content}));
                                }
                                for tc in &message.tool_calls {
                                    blocks.push(json!({"type": "tool_use", "id": tc.id, "name": tc.name, "input": tc.input}));
                                }
                                if message.cache {
                                    if let Some(last) = blocks.last_mut() {
                                        last["cache_control"] = json!({"type": "ephemeral"});
                                    }
                                }
                                msgs.push(json!({"role": "assistant", "content": blocks}));
                            }
                        }
                        Role::Tool => {
                            if let Some(tool_use_id) = &message.tool_call_id {
                                msgs.push(json!({"role": "user", "content": [{"type": "tool_result", "tool_use_id": tool_use_id, "content": message.content}]}));
                            }
                        }
                    }
                }
                let sys = if sys_parts.is_empty() {
                    None
                } else {
                    Some(sys_parts.join("\n"))
                };
                (msgs, sys)
            };
            let prefix = "You are Claude Code, Anthropic's official CLI for Claude.";
            let mut oauth_system_blocks = vec![
                json!({"type": "text", "text": prefix, "cache_control": {"type": "ephemeral"}}),
            ];
            if let Some(ref blocks) = req.system_blocks {
                // Use explicit system_blocks — serialize each with its own cache_control.
                for b in blocks {
                    let mut obj = json!({"type": "text", "text": b.text});
                    if b.cache_control {
                        obj["cache_control"] = json!({"type": "ephemeral"});
                    }
                    oauth_system_blocks.push(obj);
                }
            } else {
                let user_system = req.system.or(extracted_system);
                if let Some(s) = user_system {
                    oauth_system_blocks.push(json!({"type": "text", "text": s}));
                }
            }
            let model = req.model.unwrap_or_else(|| self.model.clone());
            let max_tokens = req.max_tokens.unwrap_or(DEFAULT_MAX_TOKENS);
            let mut body = json!({
                "model": model,
                "messages": messages,
                "max_tokens": max_tokens,
                "stream": true,
                "system": oauth_system_blocks,
            });
            if req.thinking.is_some() {
                body["temperature"] = json!(1.0);
            } else if let Some(temperature) = req.temperature {
                body["temperature"] = json!(temperature);
            }
            if let Some(ref thinking) = req.thinking {
                body["thinking"] = json!({
                    "type": "enabled",
                    "budget_tokens": thinking.budget_tokens,
                });
            }
            {
                let mut all_tools: Vec<Value> = Vec::new();
                if let Some(tools) = req.tools {
                    for t in tools {
                        let mut obj = json!({
                            "name": t.name,
                            "description": t.description.unwrap_or_default(),
                            "input_schema": t.input_schema.unwrap_or_else(|| json!({"type":"object","properties":{}})),
                        });
                        if t.cache {
                            obj["cache_control"] = json!({"type": "ephemeral"});
                        }
                        all_tools.push(obj);
                    }
                }
                if let Some(ref mcp_tool_configs) = req.mcp_tool_configs {
                    for config in mcp_tool_configs {
                        all_tools.push(serialize_mcp_tool_config(config));
                    }
                }
                if !all_tools.is_empty() {
                    body["tools"] = json!(all_tools);
                }
            }
            if let Some(tool_choice) = &req.tool_choice {
                match tool_choice {
                    ToolChoice::Auto => {
                        body["tool_choice"] = json!({"type": "auto"});
                    }
                    ToolChoice::Required => {
                        body["tool_choice"] = json!({"type": "any"});
                    }
                    ToolChoice::None => {
                        body.as_object_mut().map(|m| m.remove("tools"));
                    }
                    ToolChoice::Tool { name } => {
                        body["tool_choice"] = json!({"type": "tool", "name": name});
                    }
                }
            }
            if let Some(ref stop_sequences) = req.stop_sequences {
                if !stop_sequences.is_empty() {
                    body["stop_sequences"] = json!(stop_sequences);
                }
            }
            if let Some(mcp_servers) = &req.mcp_servers {
                let servers: Vec<Value> = mcp_servers
                    .iter()
                    .map(|s| {
                        let mut obj = json!({
                            "type": s.kind,
                            "url": s.url,
                            "name": s.name,
                        });
                        if let Some(token) = &s.authorization_token {
                            obj["authorization_token"] = json!(token);
                        }
                        obj
                    })
                    .collect();
                if !servers.is_empty() {
                    body["mcp_servers"] = json!(servers);
                }
            }
            if let Some(po) = req.provider_options {
                if let Some(map) = po.as_object() {
                    for (k, v) in map {
                        body[k] = v.clone();
                    }
                }
            }
            body
        } else {
            AnthropicRequestBuilder::new(req, self.model.clone(), false)
                .stream(true)
                .build()
        };
        let mut attempt = 0;
        let response = loop {
            let request = self
                .http
                .post(self.endpoint())
                .header("anthropic-version", "2023-06-01")
                .json(&body);
            let request = Self::apply_beta_header(request, has_mcp);
            let response = match self.apply_auth(request).send().await {
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
                .unwrap_or_else(|_| "anthropic stream request failed".to_string());
            let message = Self::with_auth_hint(
                status.as_u16(),
                message,
                Self::is_setup_token(&self.api_key),
            );
            return Err(map_http_error(status.as_u16(), message));
        };

        let raw_stream = response.bytes_stream().eventsource();
        let adapter = AnthropicStreamAdapter {
            inner: Box::pin(raw_stream),
            pending: std::collections::VecDeque::new(),
            current_tool_id: None,
        };

        Ok(Box::pin(adapter))
    }
}

/// Stream adapter that parses Anthropic SSE events including tool_use blocks.
struct AnthropicStreamAdapter {
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
    current_tool_id: Option<String>,
}

impl Stream for AnthropicStreamAdapter {
    type Item = StreamEvent;

    fn poll_next(
        mut self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> Poll<Option<Self::Item>> {
        // Drain any pending events first
        if let Some(event) = self.pending.pop_front() {
            return Poll::Ready(Some(event));
        }

        loop {
            match self.inner.as_mut().poll_next(cx) {
                Poll::Ready(Some(Ok(event))) => {
                    let payload: Value = match serde_json::from_str(&event.data) {
                        Ok(v) => v,
                        Err(_) => continue,
                    };
                    let event_type = match payload.get("type").and_then(Value::as_str) {
                        Some(t) => t,
                        None => continue,
                    };

                    match event_type {
                        "message_start" => {
                            if let Some(usage) = payload.get("message").and_then(|m| m.get("usage"))
                            {
                                let input_tokens = usage
                                    .get("input_tokens")
                                    .and_then(Value::as_u64)
                                    .unwrap_or(0)
                                    as u32;
                                let output_tokens = usage
                                    .get("output_tokens")
                                    .and_then(Value::as_u64)
                                    .unwrap_or(0)
                                    as u32;
                                let cache_creation = usage
                                    .get("cache_creation_input_tokens")
                                    .and_then(Value::as_u64)
                                    .map(|v| v as u32);
                                let cache_read = usage
                                    .get("cache_read_input_tokens")
                                    .and_then(Value::as_u64)
                                    .map(|v| v as u32);
                                return Poll::Ready(Some(StreamEvent::usage(
                                    crate::types::Usage {
                                        input_tokens,
                                        output_tokens,
                                        cache_creation_input_tokens: cache_creation,
                                        cache_read_input_tokens: cache_read,
                                    },
                                )));
                            }
                            continue;
                        }
                        "message_delta" => {
                            if let Some(usage) = payload.get("usage") {
                                let input_tokens = usage
                                    .get("input_tokens")
                                    .and_then(Value::as_u64)
                                    .unwrap_or(0)
                                    as u32;
                                let output_tokens = usage
                                    .get("output_tokens")
                                    .and_then(Value::as_u64)
                                    .unwrap_or(0)
                                    as u32;
                                return Poll::Ready(Some(StreamEvent::usage(
                                    crate::types::Usage {
                                        input_tokens,
                                        output_tokens,
                                        cache_creation_input_tokens: None,
                                        cache_read_input_tokens: None,
                                    },
                                )));
                            }
                            continue;
                        }
                        "content_block_start" => {
                            let block = payload.get("content_block");
                            if let Some(block) = block {
                                if block.get("type").and_then(Value::as_str) == Some("tool_use") {
                                    let id =
                                        block.get("id").and_then(Value::as_str).unwrap_or_default();
                                    let name = block
                                        .get("name")
                                        .and_then(Value::as_str)
                                        .unwrap_or_default();
                                    self.current_tool_id = Some(id.to_string());
                                    return Poll::Ready(Some(StreamEvent::tool_call_start(
                                        id, name,
                                    )));
                                }
                            }
                            continue;
                        }
                        "content_block_delta" => {
                            let delta = match payload.get("delta") {
                                Some(d) => d,
                                None => continue,
                            };
                            let delta_type = delta.get("type").and_then(Value::as_str);

                            match delta_type {
                                Some("input_json_delta") => {
                                    let partial = delta
                                        .get("partial_json")
                                        .and_then(Value::as_str)
                                        .unwrap_or_default();
                                    if !partial.is_empty() {
                                        let id =
                                            self.current_tool_id.as_deref().unwrap_or_default();
                                        return Poll::Ready(Some(
                                            StreamEvent::tool_call_args_with_id(id, partial),
                                        ));
                                    }
                                    continue;
                                }
                                _ => {
                                    // text_delta or untyped delta with "text" field
                                    let text = delta
                                        .get("text")
                                        .and_then(Value::as_str)
                                        .unwrap_or_default();
                                    if !text.is_empty() {
                                        return Poll::Ready(Some(StreamEvent::text(text)));
                                    }
                                    continue;
                                }
                            }
                        }
                        "content_block_stop" => {
                            if let Some(id) = self.current_tool_id.take() {
                                return Poll::Ready(Some(StreamEvent::tool_call_end_with_id(id)));
                            }
                            continue;
                        }
                        "message_stop" => {
                            return Poll::Ready(Some(StreamEvent::done()));
                        }
                        _ => continue,
                    }
                }
                Poll::Ready(Some(Err(_))) => continue,
                Poll::Ready(None) => return Poll::Ready(None),
                Poll::Pending => return Poll::Pending,
            }
        }
    }
}
