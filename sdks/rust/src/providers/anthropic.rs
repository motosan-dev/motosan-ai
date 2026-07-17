const DEFAULT_MAX_TOKENS: u32 = 8192;

use crate::error::MotosanError;
use crate::models::DEFAULT_ANTHROPIC_MODEL;
use crate::providers::{
    extract_error_message, extract_request_id, map_http_error, parse_retry_after, send_with_retry,
    ChatResponseBuilder, ProviderImpl,
};
use crate::retry::RetryPolicy;
use crate::stream::BoxStream;
use crate::types::{
    ChatRequest, ChatResponse, ContentBlock, DocumentSource, ImageSource, McpToolConfig,
    ProviderCapabilities, Role, StopReason, StreamEvent, SystemBlock, ThinkingConfig, ToolCall,
    ToolChoice,
};
use async_trait::async_trait;
use eventsource_stream::Eventsource;
use futures_core::Stream;
use reqwest::Client;
use serde_json::{json, Value};
use std::pin::Pin;
use std::task::Poll;
use tokio_stream::StreamExt;

#[derive(Debug, Clone)]
pub struct AnthropicProvider {
    http: Client,
    api_key: String,
    model: String,
    base_url: String,
    retry_policy: RetryPolicy,
    capabilities: ProviderCapabilities,
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
            capabilities: ProviderCapabilities::full(),
        }
    }

    pub fn with_retry_policy(mut self, retry_policy: RetryPolicy) -> Self {
        self.retry_policy = retry_policy;
        self
    }

    /// Replace the internal `reqwest::Client` with a caller-supplied one.
    /// `ClientBuilder::build()` uses this to hand every HTTP provider one
    /// shared, connect-timeout-configured client so all providers share a
    /// single connection pool instead of each constructing their own.
    pub fn with_http_client(mut self, http: Client) -> Self {
        self.http = http;
        self
    }

    pub fn with_capabilities(mut self, capabilities: ProviderCapabilities) -> Self {
        self.capabilities = capabilities;
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
                .header("user-agent", "claude-code/1.0.33")
                .header("x-app", "cli")
        } else {
            request.header("x-api-key", &self.api_key)
        }
    }

    fn build_beta_header(has_mcp: bool, is_oauth: bool, adaptive_thinking: bool) -> Option<String> {
        let mut betas = vec![];
        if is_oauth {
            betas.push("claude-code-20250219");
            betas.push("oauth-2025-04-20");
            betas.push("fine-grained-tool-streaming-2025-05-14");
            if !adaptive_thinking {
                betas.push("interleaved-thinking-2025-05-14");
            }
        }
        if has_mcp {
            betas.push("mcp-client-2025-11-20");
        }
        if betas.is_empty() {
            None
        } else {
            Some(betas.join(","))
        }
    }

    fn apply_beta_header(
        request: reqwest::RequestBuilder,
        has_mcp: bool,
        is_oauth: bool,
        adaptive_thinking: bool,
    ) -> reqwest::RequestBuilder {
        match Self::build_beta_header(has_mcp, is_oauth, adaptive_thinking) {
            Some(header) => request.header("anthropic-beta", header),
            None => request,
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
            "mcp_server_name": mcp_server_name,
        }),
        McpToolConfig::Allowed {
            mcp_server_name,
            allowed_tools,
        } => json!({
            "type": "mcp_toolset",
            "mcp_server_name": mcp_server_name,
            "allowed_tools": allowed_tools,
        }),
        McpToolConfig::Denied {
            mcp_server_name,
            denied_tools,
        } => json!({
            "type": "mcp_toolset",
            "mcp_server_name": mcp_server_name,
            "denied_tools": denied_tools,
        }),
    }
}

fn model_uses_adaptive_thinking(model: &str) -> bool {
    matches!(
        model,
        "claude-opus-4-8" | "claude-opus-4-7" | "claude-opus-4-6"
    )
}

fn apply_thinking_config(body: &mut Value, model: &str, thinking: &ThinkingConfig) {
    if model_uses_adaptive_thinking(model) {
        // Pi marks Opus 4.8/4.7/4.6 as `forceAdaptiveThinking`: Anthropic chooses
        // the thinking budget adaptively and rejects the older budget-token
        // shape. Preserve the summarized display default so OAuth callers
        // still receive `thinking_delta` events.
        body["thinking"] = json!({
            "type": "adaptive",
            "display": "summarized",
        });
        body["output_config"] = json!({"effort": "high"});
    } else {
        body["thinking"] = json!({
            "type": "enabled",
            "budget_tokens": thinking.budget_tokens,
            "display": "summarized",
        });
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
        if let Some(ref thinking) = self.req.thinking {
            if !model_uses_adaptive_thinking(&model) {
                // Budget-based extended thinking requires temperature=1.0.
                body["temperature"] = json!(1.0);
            }
            // Explicit `display: "summarized"` is required for the OAuth
            // (sk-ant-oat01-*) tier to actually emit `thinking_delta` SSE
            // events. Without it the OAuth flow defaults to
            // `display: "omitted"` regardless of model — Anthropic's docs
            // claim this default is model-dependent, but empirically the
            // Claude Code product surface forces `omitted` on OAuth
            // requests that don't opt in. Matches earendil-works/pi.
            apply_thinking_config(&mut body, &model, thinking);
        } else if let Some(temperature) = self.req.temperature {
            body["temperature"] = json!(temperature);
        }
        let max_tokens = self.req.max_tokens.unwrap_or(DEFAULT_MAX_TOKENS);
        body["max_tokens"] = json!(max_tokens);
        {
            let mut all_tools: Vec<Value> = Vec::new();
            if let Some(tools) = self.req.tools {
                for tool in tools {
                    let schema = tool.schema;
                    let mut obj = json!({
                        "name": schema.name,
                        "description": schema.description,
                        "input_schema": schema.input_schema,
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
    fn capabilities(&self) -> crate::types::ProviderCapabilities {
        self.capabilities
    }

    async fn chat(&self, req: ChatRequest) -> Result<ChatResponse, MotosanError> {
        let is_oauth = Self::is_setup_token(&self.api_key);

        // OAuth tokens require streaming + Claude Code identity.
        // Redirect to stream path and collect the full response.
        if is_oauth {
            let stream = self.stream(req).await?;
            let mut response = crate::stream::collect_stream(stream).await?;
            response.model = self.model.clone();
            return Ok(response);
        }

        let has_mcp = req.mcp_servers.as_ref().is_some_and(|s| !s.is_empty())
            || req.mcp_tool_configs.as_ref().is_some_and(|c| !c.is_empty());
        let body = AnthropicRequestBuilder::new(req, self.model.clone(), is_oauth).build();
        let adaptive_thinking = body["thinking"]["type"].as_str() == Some("adaptive");
        let response = send_with_retry(&self.retry_policy, || {
            let request = self
                .http
                .post(self.endpoint())
                .header("anthropic-version", "2023-06-01")
                .json(&body);
            let request = Self::apply_beta_header(request, has_mcp, is_oauth, adaptive_thinking);
            self.apply_auth(request)
        })
        .await?;

        let status = response.status();
        if !status.is_success() {
            let retry_after = parse_retry_after(response.headers());
            let request_id = extract_request_id(response.headers());
            let error_payload: Value = response.json().await.unwrap_or(json!({}));
            let message = extract_error_message(&error_payload, "anthropic request failed");
            let message = Self::with_auth_hint(
                status.as_u16(),
                message,
                Self::is_setup_token(&self.api_key),
            );
            return Err(map_http_error(
                status.as_u16(),
                message,
                retry_after,
                request_id,
            ));
        }

        let payload: Value =
            response
                .json()
                .await
                .map_err(|error| MotosanError::ProviderError {
                    message: error.to_string(),
                    status_code: None,
                    retry_after: None,
                    request_id: None,
                })?;

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
            if let Some(ref thinking) = req.thinking {
                if !model_uses_adaptive_thinking(&model) {
                    body["temperature"] = json!(1.0);
                }
                // See parallel non-streaming block above for why `display`
                // is explicitly set. Without it, the OAuth tier silently
                // defaults to `display: "omitted"` and the resulting SSE
                // stream contains only `signature_delta` (no `thinking_delta`).
                apply_thinking_config(&mut body, &model, thinking);
            } else if let Some(temperature) = req.temperature {
                body["temperature"] = json!(temperature);
            }
            {
                let mut all_tools: Vec<Value> = Vec::new();
                if let Some(tools) = req.tools {
                    for t in tools {
                        let schema = t.schema;
                        let mut obj = json!({
                            "name": schema.name,
                            "description": schema.description,
                            "input_schema": schema.input_schema,
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
        let adaptive_thinking = body["thinking"]["type"].as_str() == Some("adaptive");
        let response = send_with_retry(&self.retry_policy, || {
            let request = self
                .http
                .post(self.endpoint())
                .header("anthropic-version", "2023-06-01")
                .json(&body);
            let request = Self::apply_beta_header(request, has_mcp, is_oauth, adaptive_thinking);
            self.apply_auth(request)
        })
        .await?;

        let status = response.status();
        if !status.is_success() {
            let retry_after = parse_retry_after(response.headers());
            let request_id = extract_request_id(response.headers());
            let message = response
                .text()
                .await
                .unwrap_or_else(|_| "anthropic stream request failed".to_string());
            let message = Self::with_auth_hint(
                status.as_u16(),
                message,
                Self::is_setup_token(&self.api_key),
            );
            return Err(map_http_error(
                status.as_u16(),
                message,
                retry_after,
                request_id,
            ));
        }

        let raw_stream = response
            .bytes_stream()
            .chain(tokio_stream::once(Ok("\n".into())))
            .eventsource();
        let adapter = AnthropicStreamAdapter {
            inner: Box::pin(raw_stream),
            pending: std::collections::VecDeque::new(),
            current_tool_id: None,
            current_stop_reason: None,
            current_thinking_buf: None,
            saw_terminal: false,
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
    /// Captured from `message_delta.delta.stop_reason`; emitted on the
    /// terminal `message_stop` event so callers see the reason in the
    /// final `done` `StreamEvent`.
    current_stop_reason: Option<crate::types::StopReason>,
    /// Accumulator for the in-flight thinking block, if any.
    ///
    /// - `None` = not currently inside a `thinking` content block.
    /// - `Some(buf)` = open thinking block; each `thinking_delta` appends
    ///   to `buf`, and `content_block_stop` emits a `ThinkingDone` event
    ///   carrying `buf.clone()` and resets to `None`.
    ///
    /// `redacted_thinking` blocks are silently consumed and do **not**
    /// open this accumulator (we don't surface redacted content as
    /// thinking deltas).
    current_thinking_buf: Option<String>,
    /// True once `message_stop` (or a terminal error) has been yielded.
    saw_terminal: bool,
}

impl Stream for AnthropicStreamAdapter {
    type Item = Result<StreamEvent, MotosanError>;

    fn poll_next(
        mut self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> Poll<Option<Self::Item>> {
        // Drain any pending events first
        if let Some(event) = self.pending.pop_front() {
            return Poll::Ready(Some(Ok(event)));
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
                                return Poll::Ready(Some(Ok(StreamEvent::usage(
                                    crate::types::Usage {
                                        input_tokens,
                                        output_tokens,
                                        cache_creation_input_tokens: cache_creation,
                                        cache_read_input_tokens: cache_read,
                                    },
                                ))));
                            }
                            continue;
                        }
                        "message_delta" => {
                            // Anthropic carries the final stop_reason on
                            // `message_delta.delta.stop_reason`. Stash it so
                            // we can emit it on the terminal message_stop.
                            if let Some(reason) = payload
                                .get("delta")
                                .and_then(|d| d.get("stop_reason"))
                                .and_then(Value::as_str)
                            {
                                self.current_stop_reason = Some(match reason {
                                    "end_turn" => StopReason::EndTurn,
                                    "max_tokens" => StopReason::MaxTokens,
                                    "tool_use" => StopReason::ToolUse,
                                    "stop_sequence" => StopReason::StopSequence,
                                    "stop" => StopReason::Stop,
                                    _ => StopReason::Other,
                                });
                            }

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
                                return Poll::Ready(Some(Ok(StreamEvent::usage(
                                    crate::types::Usage {
                                        input_tokens,
                                        output_tokens,
                                        cache_creation_input_tokens: None,
                                        cache_read_input_tokens: None,
                                    },
                                ))));
                            }
                            continue;
                        }
                        "content_block_start" => {
                            let block = payload.get("content_block");
                            if let Some(block) = block {
                                let block_type =
                                    block.get("type").and_then(Value::as_str).unwrap_or("");
                                match block_type {
                                    "tool_use" => {
                                        let id = block
                                            .get("id")
                                            .and_then(Value::as_str)
                                            .unwrap_or_default();
                                        let name = block
                                            .get("name")
                                            .and_then(Value::as_str)
                                            .unwrap_or_default();
                                        self.current_tool_id = Some(id.to_string());
                                        return Poll::Ready(Some(Ok(
                                            StreamEvent::tool_call_start(id, name),
                                        )));
                                    }
                                    "thinking" => {
                                        // Open the thinking accumulator. Deltas append
                                        // to it; content_block_stop emits ThinkingDone
                                        // with the full text and clears it (Task 4).
                                        // No event is emitted at start — the loop-side
                                        // event protocol does not have a ThinkingStart.
                                        self.current_thinking_buf = Some(String::new());
                                    }
                                    "redacted_thinking" => {
                                        // Silently consume; we do not surface redacted
                                        // content. The block_stop will be a no-op
                                        // because current_thinking_buf stays None.
                                    }
                                    _ => {}
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
                                        return Poll::Ready(Some(Ok(
                                            StreamEvent::tool_call_args_with_id(id, partial),
                                        )));
                                    }
                                    continue;
                                }
                                Some("thinking_delta") => {
                                    // The thinking text lives in `delta.thinking`,
                                    // NOT `delta.text`. Accumulate into the buffer
                                    // (so content_block_stop can emit ThinkingDone
                                    // with the full text in Task 4) and forward as
                                    // a ThinkingDelta event.
                                    let text = delta
                                        .get("thinking")
                                        .and_then(Value::as_str)
                                        .unwrap_or_default();
                                    if text.is_empty() {
                                        continue;
                                    }
                                    if let Some(buf) = self.current_thinking_buf.as_mut() {
                                        buf.push_str(text);
                                    }
                                    return Poll::Ready(Some(Ok(StreamEvent::thinking_delta(
                                        text,
                                    ))));
                                }
                                Some("signature_delta") => {
                                    // Cryptographic signature for re-feeding thinking
                                    // blocks. Not surfaced in the streaming API (the
                                    // non-streaming ChatResponse.thinking field is
                                    // also signature-less). Silently consume.
                                    continue;
                                }
                                _ => {
                                    // text_delta or untyped delta with "text" field
                                    let text = delta
                                        .get("text")
                                        .and_then(Value::as_str)
                                        .unwrap_or_default();
                                    if !text.is_empty() {
                                        return Poll::Ready(Some(Ok(StreamEvent::text(text))));
                                    }
                                    continue;
                                }
                            }
                        }
                        "content_block_stop" => {
                            if let Some(id) = self.current_tool_id.take() {
                                return Poll::Ready(Some(Ok(StreamEvent::tool_call_end_with_id(
                                    id,
                                ))));
                            }
                            if let Some(buf) = self.current_thinking_buf.take() {
                                // Closing a thinking block: emit ThinkingDone with
                                // the full concatenated text. Note we emit even if
                                // buf is empty — consumers can distinguish "thinking
                                // block existed but produced nothing" from "no
                                // thinking block" by the presence/absence of the
                                // event. This matches the contract documented on
                                // StreamEventType::ThinkingDone.
                                return Poll::Ready(Some(Ok(StreamEvent::thinking_done(buf))));
                            }
                            continue;
                        }
                        "message_stop" => {
                            self.saw_terminal = true;
                            let done = match self.current_stop_reason.take() {
                                Some(reason) => StreamEvent::done_with_stop_reason(reason),
                                None => StreamEvent::done(),
                            };
                            return Poll::Ready(Some(Ok(done)));
                        }
                        "error" => {
                            self.saw_terminal = true;
                            let err_type =
                                payload["error"]["type"].as_str().unwrap_or("unknown_error");
                            let message = payload["error"]["message"]
                                .as_str()
                                .unwrap_or("unknown stream error");
                            return Poll::Ready(Some(Err(MotosanError::Stream(format!(
                                "anthropic stream error: {err_type}: {message}"
                            )))));
                        }
                        _ => continue,
                    }
                }
                Poll::Ready(Some(Err(e))) => {
                    self.saw_terminal = true;
                    return Poll::Ready(Some(Err(MotosanError::Stream(e.to_string()))));
                }
                Poll::Ready(None) => {
                    if !self.saw_terminal {
                        self.saw_terminal = true;
                        return Poll::Ready(Some(Err(MotosanError::IncompleteStream(
                            "anthropic ended without a terminal event".to_string(),
                        ))));
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

    #[tokio::test]
    async fn adapter_surfaces_inner_stream_error() {
        use eventsource_stream::EventStreamError;
        use tokio_stream::StreamExt;

        let utf8 = String::from_utf8(vec![0xff]).unwrap_err();
        let inner = tokio_stream::iter(vec![Err(EventStreamError::Utf8(utf8))]);
        let mut adapter = AnthropicStreamAdapter {
            inner: Box::pin(inner),
            pending: std::collections::VecDeque::new(),
            current_tool_id: None,
            current_stop_reason: None,
            current_thinking_buf: None,
            saw_terminal: false,
        };

        let item = adapter.next().await.expect("one item");
        assert!(matches!(item, Err(MotosanError::Stream(_))));
    }

    #[test]
    fn cached_user_message_serializes_cache_control_both_auth_modes() {
        // oauth does NOT branch the cached-USER path, so this pins oauth-invariance
        // of the user-cache breakpoint.
        for oauth in [false, true] {
            // Build a ChatRequest whose single user message has cache = true.
            let req = ChatRequest::builder()
                .message(crate::types::Message::user_with_cache("hi"))
                .build();
            let body =
                AnthropicRequestBuilder::new(req, "claude-opus-4-8".to_string(), oauth).build();

            let msgs = body["messages"].as_array().expect("messages array");
            let last = msgs.last().expect("a message");
            let blocks = last["content"]
                .as_array()
                .expect("content must be a block array");
            let cc = &blocks.last().expect("a content block")["cache_control"]["type"];
            assert_eq!(cc, "ephemeral", "oauth={oauth}");
        }
    }

    #[test]
    fn cached_multimodal_user_message_serializes_cache_control_on_last_block() {
        // The content_blocks (image/document/multimodal) user arm places
        // cache_control on the LAST block. oauth does NOT branch this arm, so
        // this pins oauth-invariance of the multimodal-user cache breakpoint.
        // A single text block is enough to hit the content_blocks arm.
        for oauth in [false, true] {
            let req = ChatRequest::builder()
                .message(
                    crate::types::Message::user_with_blocks(vec![
                        crate::types::ContentBlock::Text {
                            text: "describe this".to_string(),
                        },
                    ])
                    .with_cache(),
                )
                .build();
            let body =
                AnthropicRequestBuilder::new(req, "claude-opus-4-8".to_string(), oauth).build();

            let msgs = body["messages"].as_array().expect("messages array");
            let blocks = msgs.last().expect("a message")["content"]
                .as_array()
                .expect("content blocks");
            let cc = &blocks.last().expect("a content block")["cache_control"]["type"];
            assert_eq!(cc, "ephemeral", "oauth={oauth}");
        }
    }

    #[test]
    fn capabilities_are_full_by_default() {
        let p = AnthropicProvider::new("key", None, None);
        let caps = p.capabilities();
        assert!(caps.supports_image);
        assert!(caps.supports_document);
    }

    #[test]
    fn with_capabilities_overrides_default() {
        let p = AnthropicProvider::new("key", None, None)
            .with_capabilities(ProviderCapabilities::text_only());
        let caps = p.capabilities();
        assert!(!caps.supports_image);
        assert!(!caps.supports_document);
    }
}
