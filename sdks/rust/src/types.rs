use base64::Engine;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Configuration for extended thinking (Anthropic).
///
/// When enabled the provider will include a `thinking` block in the request
/// and surface any thinking content in [`ChatResponse::thinking`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThinkingConfig {
    /// Token budget for extended thinking (Anthropic typical range: 1024-32000).
    pub budget_tokens: u32,
}

/// A system prompt block with optional cache control.
///
/// Use [`SystemBlock::new`] for a plain block and [`SystemBlock::cached`] for
/// a block that should be covered by Anthropic prompt caching.  When sent to
/// non-Anthropic providers the blocks are joined with newlines into a single
/// string and the `cache_control` flag is silently ignored.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemBlock {
    pub text: String,
    /// Shorthand for `cache_control: { type: "ephemeral" }` in the Anthropic
    /// API.  Non-Anthropic providers silently ignore this flag.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub cache_control: bool,
}

impl SystemBlock {
    /// Create a new system block without caching.
    pub fn new(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            cache_control: false,
        }
    }

    /// Create a new system block with prompt caching enabled.
    pub fn cached(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            cache_control: true,
        }
    }
}

/// Controls how the model selects which tool (if any) to call.
///
/// - `Auto` — the model decides whether to call a tool (default behavior).
/// - `Required` — the model must call at least one tool.
/// - `None` — the model must not call any tool.
/// - `Tool { name }` — the model must call the specific named tool.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ToolChoice {
    Auto,
    Required,
    None,
    Tool { name: String },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    User,
    Assistant,
    System,
    Tool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Message {
    pub role: Role,
    pub content: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub content_blocks: Vec<ContentBlock>,
    pub tool_call_id: Option<String>,
    pub tool_calls: Vec<ToolCall>,
    /// When `true`, the provider may apply prompt caching to this message's
    /// content (Anthropic `cache_control`).  Non-Anthropic providers silently
    /// ignore this flag.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub cache: bool,
}

impl Message {
    pub fn user(content: impl Into<String>) -> Self {
        Self {
            role: Role::User,
            content: content.into(),
            content_blocks: vec![],
            tool_call_id: None,
            tool_calls: Vec::new(),
            cache: false,
        }
    }

    /// Create a user message and mark it as cacheable (Anthropic prompt caching).
    pub fn user_with_cache(content: impl Into<String>) -> Self {
        Self {
            role: Role::User,
            content: content.into(),
            content_blocks: vec![],
            tool_call_id: None,
            tool_calls: Vec::new(),
            cache: true,
        }
    }

    pub fn assistant(content: impl Into<String>) -> Self {
        Self {
            role: Role::Assistant,
            content: content.into(),
            content_blocks: vec![],
            tool_call_id: None,
            tool_calls: Vec::new(),
            cache: false,
        }
    }

    pub fn assistant_with_tool_calls(
        content: impl Into<String>,
        tool_calls: Vec<ToolCall>,
    ) -> Self {
        Self {
            role: Role::Assistant,
            content: content.into(),
            content_blocks: vec![],
            tool_call_id: None,
            tool_calls,
            cache: false,
        }
    }

    pub fn system(content: impl Into<String>) -> Self {
        Self {
            role: Role::System,
            content: content.into(),
            content_blocks: vec![],
            tool_call_id: None,
            tool_calls: Vec::new(),
            cache: false,
        }
    }

    pub fn tool(content: impl Into<String>, tool_call_id: impl Into<String>) -> Self {
        Self::tool_result(tool_call_id, content)
    }

    pub fn tool_result(tool_call_id: impl Into<String>, content: impl Into<String>) -> Self {
        Self {
            role: Role::Tool,
            content: content.into(),
            content_blocks: vec![],
            tool_call_id: Some(tool_call_id.into()),
            tool_calls: Vec::new(),
            cache: false,
        }
    }

    /// Create a user message with an image (base64)
    pub fn user_with_image(text: &str, base64_data: &str, media_type: &str) -> Self {
        Self {
            role: Role::User,
            content: text.to_string(),
            content_blocks: vec![
                ContentBlock::Text {
                    text: text.to_string(),
                },
                ContentBlock::Image {
                    source: ImageSource::Base64 {
                        media_type: media_type.to_string(),
                        data: base64_data.to_string(),
                    },
                },
            ],
            tool_call_id: None,
            tool_calls: vec![],
            cache: false,
        }
    }

    /// Create a user message with multiple content blocks
    pub fn user_with_blocks(blocks: Vec<ContentBlock>) -> Self {
        // Extract text from first text block for backward compat content field
        let content = blocks
            .iter()
            .find_map(|b| match b {
                ContentBlock::Text { text } => Some(text.clone()),
                _ => None,
            })
            .unwrap_or_default();

        Self {
            role: Role::User,
            content,
            content_blocks: blocks,
            tool_call_id: None,
            tool_calls: vec![],
            cache: false,
        }
    }

    /// Mark this message's content as cacheable (Anthropic prompt caching).
    ///
    /// Non-Anthropic providers silently ignore this flag.
    pub fn with_cache(mut self) -> Self {
        self.cache = true;
        self
    }

    /// Create a user message with a PDF document (base64-encoded).
    ///
    /// Supported by the Anthropic provider. Other providers will return
    /// an `UnsupportedFeature` error.
    pub fn user_with_pdf_base64(text: &str, base64_data: &str) -> Self {
        Self {
            role: Role::User,
            content: text.to_string(),
            content_blocks: vec![
                ContentBlock::Text {
                    text: text.to_string(),
                },
                ContentBlock::Document {
                    source: DocumentSource::Base64 {
                        media_type: "application/pdf".to_string(),
                        data: base64_data.to_string(),
                    },
                },
            ],
            tool_call_id: None,
            tool_calls: vec![],
            cache: false,
        }
    }

    /// Create a user message with a PDF document from a URL.
    ///
    /// Supported by the Anthropic provider. Other providers will return
    /// an `UnsupportedFeature` error.
    pub fn user_with_pdf_url(text: &str, url: &str) -> Self {
        Self {
            role: Role::User,
            content: text.to_string(),
            content_blocks: vec![
                ContentBlock::Text {
                    text: text.to_string(),
                },
                ContentBlock::Document {
                    source: DocumentSource::Url {
                        url: url.to_string(),
                    },
                },
            ],
            tool_call_id: None,
            tool_calls: vec![],
            cache: false,
        }
    }

    /// Create a user message with a PDF document from raw bytes.
    ///
    /// The bytes are automatically base64-encoded. Supported by the Anthropic
    /// provider. Other providers will return an `UnsupportedFeature` error.
    pub fn user_with_pdf_bytes(text: &str, bytes: &[u8]) -> Self {
        let encoded = base64::engine::general_purpose::STANDARD.encode(bytes);
        Self::user_with_pdf_base64(text, &encoded)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Tool {
    pub name: String,
    pub description: Option<String>,
    pub input_schema: Option<Value>,
    /// When `true`, the Anthropic provider will attach `cache_control` to this
    /// tool definition.  Non-Anthropic providers silently ignore this flag.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub cache: bool,
}

/// Server-side MCP server transport type.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum McpServerType {
    Url,
}

/// Configuration for a server-side MCP server.
///
/// When included in a `ChatRequest`, the provider connects to the MCP server
/// on the server side — the client never manages the MCP connection directly.
/// Currently supported by the Anthropic provider only.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct McpServerConfig {
    #[serde(rename = "type")]
    pub kind: McpServerType,
    pub url: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub authorization_token: Option<String>,
}

/// Controls which tools from an MCP server are available to the model.
///
/// Serialized as `{ "type": "mcp_toolset", "server_label": "...", ... }` in the
/// Anthropic `tools` array (API version `mcp-client-2025-11-20`).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum McpToolConfig {
    /// Expose all tools from the named MCP server.
    All { mcp_server_name: String },
    /// Expose only the listed tools from the named MCP server.
    Allowed {
        mcp_server_name: String,
        allowed_tools: Vec<String>,
    },
    /// Expose all tools except the listed ones from the named MCP server.
    Denied {
        mcp_server_name: String,
        denied_tools: Vec<String>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatRequest {
    pub messages: Vec<Message>,
    pub model: Option<String>,
    pub system: Option<String>,
    /// Array of system prompt blocks with per-block cache control.
    ///
    /// When set, this takes priority over the plain `system` string.  The
    /// Anthropic provider serializes these as an array of text blocks; other
    /// providers join the block texts with newlines into a single string.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub system_blocks: Option<Vec<SystemBlock>>,
    /// When `true`, the Anthropic provider serializes the system prompt with
    /// `cache_control: { type: "ephemeral" }`.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub system_cache: bool,
    pub temperature: Option<f32>,
    pub max_tokens: Option<u32>,
    pub tools: Option<Vec<Tool>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_choice: Option<ToolChoice>,
    pub provider_options: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mcp_servers: Option<Vec<McpServerConfig>>,
    /// Per-server tool filtering for server-side MCP.
    ///
    /// Each entry becomes a `{ "type": "mcp_toolset", ... }` item in the
    /// Anthropic `tools` array.  When `mcp_servers` is set but this field is
    /// `None`, the builder auto-populates an `All` entry for every server.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mcp_tool_configs: Option<Vec<McpToolConfig>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thinking: Option<ThinkingConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stop_sequences: Option<Vec<String>>,
}

impl ChatRequest {
    pub fn builder() -> ChatRequestBuilder {
        ChatRequestBuilder::default()
    }
}

#[derive(Debug, Default, Clone)]
pub struct ChatRequestBuilder {
    messages: Vec<Message>,
    model: Option<String>,
    system: Option<String>,
    system_blocks: Option<Vec<SystemBlock>>,
    system_cache: bool,
    temperature: Option<f32>,
    max_tokens: Option<u32>,
    tools: Option<Vec<Tool>>,
    tool_choice: Option<ToolChoice>,
    provider_options: Option<Value>,
    mcp_servers: Option<Vec<McpServerConfig>>,
    mcp_tool_configs: Option<Vec<McpToolConfig>>,
    thinking: Option<ThinkingConfig>,
    stop_sequences: Option<Vec<String>>,
}

impl ChatRequestBuilder {
    pub fn messages(mut self, messages: Vec<Message>) -> Self {
        self.messages = messages;
        self
    }

    pub fn message(mut self, message: Message) -> Self {
        self.messages.push(message);
        self
    }

    pub fn model(mut self, model: impl Into<String>) -> Self {
        self.model = Some(model.into());
        self
    }

    pub fn system(mut self, system: impl Into<String>) -> Self {
        self.system = Some(system.into());
        self
    }

    /// Set the system prompt and mark it as cacheable (Anthropic prompt caching).
    ///
    /// When sent to Anthropic, the system prompt will be serialized as a content
    /// block with `cache_control: { type: "ephemeral" }`.  Non-Anthropic
    /// providers silently ignore the caching hint.
    pub fn system_cached(mut self, system: impl Into<String>) -> Self {
        self.system = Some(system.into());
        self.system_cache = true;
        self
    }

    /// Append a single [`SystemBlock`] to the system blocks list.
    ///
    /// Can be called multiple times to accumulate blocks.  When
    /// `system_blocks` is set it takes priority over the plain `system`
    /// string for the Anthropic provider.  Other providers flatten the blocks
    /// into a newline-joined string.
    pub fn system_block(mut self, block: SystemBlock) -> Self {
        self.system_blocks.get_or_insert_with(Vec::new).push(block);
        self
    }

    /// Set the full list of system blocks, replacing any previously added.
    ///
    /// When `system_blocks` is set it takes priority over the plain `system`
    /// string for the Anthropic provider.  Other providers flatten the blocks
    /// into a newline-joined string.
    pub fn system_blocks(mut self, blocks: Vec<SystemBlock>) -> Self {
        self.system_blocks = Some(blocks);
        self
    }

    pub fn temperature(mut self, temperature: f32) -> Self {
        self.temperature = Some(temperature);
        self
    }

    pub fn max_tokens(mut self, max_tokens: u32) -> Self {
        self.max_tokens = Some(max_tokens);
        self
    }

    pub fn tools(mut self, tools: Vec<Tool>) -> Self {
        self.tools = Some(tools);
        self
    }

    /// Set the tools and mark the last tool as cacheable (Anthropic prompt
    /// caching).
    ///
    /// Per the Anthropic API, `cache_control` is placed on the **last** tool in
    /// the list so that the entire tools array is covered by a single cache
    /// breakpoint.  Non-Anthropic providers silently ignore the caching hint.
    pub fn tools_cached(mut self, mut tools: Vec<Tool>) -> Self {
        if let Some(last) = tools.last_mut() {
            last.cache = true;
        }
        self.tools = Some(tools);
        self
    }

    pub fn tool_choice(mut self, choice: ToolChoice) -> Self {
        self.tool_choice = Some(choice);
        self
    }

    #[cfg(feature = "agent-tool")]
    pub fn tool_defs(mut self, defs: &[motosan_agent_tool::ToolDef]) -> Self {
        self.tools = Some(defs.iter().cloned().map(Tool::from).collect());
        self
    }

    pub fn provider_options(mut self, provider_options: Value) -> Self {
        self.provider_options = Some(provider_options);
        self
    }

    /// Add a single server-side MCP server configuration.
    ///
    /// Also auto-adds a [`McpToolConfig::All`] entry so that all tools from
    /// the server are exposed by default.  Use [`mcp_tool_config`] afterwards
    /// to override with allowed/denied lists.
    pub fn mcp_server(mut self, server: McpServerConfig) -> Self {
        self.mcp_tool_configs
            .get_or_insert_with(Vec::new)
            .push(McpToolConfig::All {
                mcp_server_name: server.name.clone(),
            });
        self.mcp_servers.get_or_insert_with(Vec::new).push(server);
        self
    }

    /// Set the full list of server-side MCP server configurations.
    ///
    /// Also auto-populates [`McpToolConfig::All`] entries for each server.
    /// Use [`mcp_tool_config`] or [`mcp_tool_configs`] afterwards to override.
    pub fn mcp_servers(mut self, servers: Vec<McpServerConfig>) -> Self {
        self.mcp_tool_configs = Some(
            servers
                .iter()
                .map(|s| McpToolConfig::All {
                    mcp_server_name: s.name.clone(),
                })
                .collect(),
        );
        self.mcp_servers = Some(servers);
        self
    }

    /// Add a single MCP tool configuration, overriding the auto-generated
    /// `All` entry for the same server name (if any).
    pub fn mcp_tool_config(mut self, config: McpToolConfig) -> Self {
        let name = match &config {
            McpToolConfig::All { mcp_server_name }
            | McpToolConfig::Allowed {
                mcp_server_name, ..
            }
            | McpToolConfig::Denied {
                mcp_server_name, ..
            } => mcp_server_name.clone(),
        };
        let configs = self.mcp_tool_configs.get_or_insert_with(Vec::new);
        // Replace existing entry for same server name
        if let Some(pos) = configs.iter().position(|c| match c {
            McpToolConfig::All { mcp_server_name }
            | McpToolConfig::Allowed {
                mcp_server_name, ..
            }
            | McpToolConfig::Denied {
                mcp_server_name, ..
            } => *mcp_server_name == name,
        }) {
            configs[pos] = config;
        } else {
            configs.push(config);
        }
        self
    }

    /// Set the full list of MCP tool configurations, replacing any previously
    /// added (including auto-generated ones).
    pub fn mcp_tool_configs(mut self, configs: Vec<McpToolConfig>) -> Self {
        self.mcp_tool_configs = Some(configs);
        self
    }

    /// Enable extended thinking with a token budget.
    ///
    /// When thinking is enabled on the Anthropic provider, temperature is
    /// automatically forced to 1.0 (an Anthropic API constraint).
    pub fn thinking(mut self, budget_tokens: u32) -> Self {
        self.thinking = Some(ThinkingConfig { budget_tokens });
        self
    }

    /// Add a single stop sequence. Can be called multiple times to accumulate sequences.
    pub fn stop(mut self, sequence: impl Into<String>) -> Self {
        self.stop_sequences
            .get_or_insert_with(Vec::new)
            .push(sequence.into());
        self
    }

    /// Set the full list of stop sequences, replacing any previously added.
    pub fn stop_sequences(mut self, sequences: Vec<String>) -> Self {
        self.stop_sequences = Some(sequences);
        self
    }

    pub fn build(self) -> ChatRequest {
        ChatRequest {
            messages: self.messages,
            model: self.model,
            system: self.system,
            system_blocks: self.system_blocks,
            system_cache: self.system_cache,
            temperature: self.temperature,
            max_tokens: self.max_tokens,
            tools: self.tools,
            tool_choice: self.tool_choice,
            provider_options: self.provider_options,
            mcp_servers: self.mcp_servers,
            mcp_tool_configs: self.mcp_tool_configs,
            thinking: self.thinking,
            stop_sequences: self.stop_sequences,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatResponse {
    pub content: String,
    /// Raw thinking content when extended thinking is enabled.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thinking: Option<String>,
    pub tool_calls: Vec<ToolCall>,
    pub model: String,
    pub usage: Usage,
    pub stop_reason: StopReason,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub input: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Usage {
    pub input_tokens: u32,
    pub output_tokens: u32,
    /// Tokens written to the prompt cache (Anthropic only).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_creation_input_tokens: Option<u32>,
    /// Tokens read from the prompt cache (Anthropic only).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_read_input_tokens: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum StopReason {
    EndTurn,
    MaxTokens,
    ToolUse,
    Stop,
    StopSequence,
    Other,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum StreamEventType {
    #[default]
    Text,
    ToolCallStart,
    ToolCallArgs,
    ToolCallEnd,
    Usage,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ContentBlock {
    Text { text: String },
    Image { source: ImageSource },
    Document { source: DocumentSource },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ImageSource {
    Base64 { media_type: String, data: String },
    Url { url: String },
}

/// Source for a document content block (e.g. PDF).
///
/// Currently supported by the Anthropic provider only. Other providers will
/// return an `UnsupportedFeature` error when a `Document` block is encountered.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum DocumentSource {
    Base64 { media_type: String, data: String },
    Url { url: String },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct StreamEvent {
    pub content: String,
    pub done: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_call_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_call_args_delta: Option<String>,
    #[serde(default)]
    pub event_type: StreamEventType,
    /// Token usage reported via `message_start` or `message_delta` SSE events.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usage: Option<Usage>,
    /// Final stop reason, attached to the terminal `done` event when the
    /// provider reports one (Anthropic `message_delta.stop_reason`, OpenAI
    /// `finish_reason`, etc.). `None` on intermediate events.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stop_reason: Option<StopReason>,
}

impl StreamEvent {
    pub fn text(content: impl Into<String>) -> Self {
        Self {
            content: content.into(),
            done: false,
            tool_call_id: None,
            tool_call_name: None,
            tool_call_args_delta: None,
            event_type: StreamEventType::Text,
            usage: None,
            stop_reason: None,
        }
    }

    pub fn done() -> Self {
        Self {
            content: String::new(),
            done: true,
            tool_call_id: None,
            tool_call_name: None,
            tool_call_args_delta: None,
            event_type: StreamEventType::Text,
            usage: None,
            stop_reason: None,
        }
    }

    /// Build a terminal `done` event carrying a stop reason.
    pub fn done_with_stop_reason(stop_reason: StopReason) -> Self {
        Self {
            content: String::new(),
            done: true,
            tool_call_id: None,
            tool_call_name: None,
            tool_call_args_delta: None,
            event_type: StreamEventType::Text,
            usage: None,
            stop_reason: Some(stop_reason),
        }
    }

    pub fn usage(usage: Usage) -> Self {
        Self {
            content: String::new(),
            done: false,
            tool_call_id: None,
            tool_call_name: None,
            tool_call_args_delta: None,
            event_type: StreamEventType::Usage,
            usage: Some(usage),
            stop_reason: None,
        }
    }

    pub fn tool_call_start(id: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            content: String::new(),
            done: false,
            tool_call_id: Some(id.into()),
            tool_call_name: Some(name.into()),
            tool_call_args_delta: None,
            event_type: StreamEventType::ToolCallStart,
            usage: None,
            stop_reason: None,
        }
    }

    pub fn tool_call_args(delta: impl Into<String>) -> Self {
        Self {
            content: String::new(),
            done: false,
            tool_call_id: None,
            tool_call_name: None,
            tool_call_args_delta: Some(delta.into()),
            event_type: StreamEventType::ToolCallArgs,
            usage: None,
            stop_reason: None,
        }
    }

    pub fn tool_call_args_with_id(id: impl Into<String>, delta: impl Into<String>) -> Self {
        Self {
            content: String::new(),
            done: false,
            tool_call_id: Some(id.into()),
            tool_call_name: None,
            tool_call_args_delta: Some(delta.into()),
            event_type: StreamEventType::ToolCallArgs,
            usage: None,
            stop_reason: None,
        }
    }

    pub fn tool_call_end() -> Self {
        Self {
            content: String::new(),
            done: false,
            tool_call_id: None,
            tool_call_name: None,
            tool_call_args_delta: None,
            event_type: StreamEventType::ToolCallEnd,
            usage: None,
            stop_reason: None,
        }
    }

    pub fn tool_call_end_with_id(id: impl Into<String>) -> Self {
        Self {
            content: String::new(),
            done: false,
            tool_call_id: Some(id.into()),
            tool_call_name: None,
            tool_call_args_delta: None,
            event_type: StreamEventType::ToolCallEnd,
            usage: None,
            stop_reason: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn document_source_base64_serde_roundtrip() {
        let source = DocumentSource::Base64 {
            media_type: "application/pdf".to_string(),
            data: "JVBERi0xLjQK".to_string(),
        };
        let json = serde_json::to_value(&source).unwrap();
        assert_eq!(json["type"], "base64");
        assert_eq!(json["media_type"], "application/pdf");
        assert_eq!(json["data"], "JVBERi0xLjQK");

        let deserialized: DocumentSource = serde_json::from_value(json).unwrap();
        assert_eq!(deserialized, source);
    }

    #[test]
    fn document_source_url_serde_roundtrip() {
        let source = DocumentSource::Url {
            url: "https://example.com/doc.pdf".to_string(),
        };
        let json = serde_json::to_value(&source).unwrap();
        assert_eq!(json["type"], "url");
        assert_eq!(json["url"], "https://example.com/doc.pdf");

        let deserialized: DocumentSource = serde_json::from_value(json).unwrap();
        assert_eq!(deserialized, source);
    }

    #[test]
    fn content_block_document_serde() {
        let block = ContentBlock::Document {
            source: DocumentSource::Base64 {
                media_type: "application/pdf".to_string(),
                data: "JVBERi0xLjQK".to_string(),
            },
        };
        let json = serde_json::to_value(&block).unwrap();
        assert_eq!(json["type"], "document");
        assert_eq!(json["source"]["type"], "base64");
        assert_eq!(json["source"]["media_type"], "application/pdf");
    }

    #[test]
    fn user_with_pdf_base64_creates_correct_message() {
        let msg = Message::user_with_pdf_base64("Summarize this", "JVBERi0xLjQK");
        assert_eq!(msg.role, Role::User);
        assert_eq!(msg.content, "Summarize this");
        assert_eq!(msg.content_blocks.len(), 2);

        match &msg.content_blocks[0] {
            ContentBlock::Text { text } => assert_eq!(text, "Summarize this"),
            _ => panic!("Expected Text block"),
        }
        match &msg.content_blocks[1] {
            ContentBlock::Document { source } => match source {
                DocumentSource::Base64 { media_type, data } => {
                    assert_eq!(media_type, "application/pdf");
                    assert_eq!(data, "JVBERi0xLjQK");
                }
                _ => panic!("Expected Base64 source"),
            },
            _ => panic!("Expected Document block"),
        }
    }

    #[test]
    fn user_with_pdf_url_creates_correct_message() {
        let msg = Message::user_with_pdf_url("Analyze this", "https://example.com/doc.pdf");
        assert_eq!(msg.role, Role::User);
        assert_eq!(msg.content, "Analyze this");
        assert_eq!(msg.content_blocks.len(), 2);

        match &msg.content_blocks[1] {
            ContentBlock::Document { source } => match source {
                DocumentSource::Url { url } => {
                    assert_eq!(url, "https://example.com/doc.pdf");
                }
                _ => panic!("Expected Url source"),
            },
            _ => panic!("Expected Document block"),
        }
    }

    #[test]
    fn user_with_pdf_bytes_auto_encodes_base64() {
        let fake_pdf = b"%PDF-1.4\n";
        let msg = Message::user_with_pdf_bytes("Read this", fake_pdf);
        assert_eq!(msg.content_blocks.len(), 2);

        match &msg.content_blocks[1] {
            ContentBlock::Document { source } => match source {
                DocumentSource::Base64 { media_type, data } => {
                    assert_eq!(media_type, "application/pdf");
                    // Verify the base64 decodes back to original bytes
                    let decoded = base64::engine::general_purpose::STANDARD
                        .decode(data)
                        .unwrap();
                    assert_eq!(decoded, fake_pdf);
                }
                _ => panic!("Expected Base64 source"),
            },
            _ => panic!("Expected Document block"),
        }
    }

    #[test]
    fn anthropic_document_block_serialization() {
        // Simulate what the Anthropic provider does: serialize a message with
        // document content blocks into the expected JSON structure.
        let msg = Message::user_with_pdf_base64("Summarize this contract", "JVBERi0xLjQK");

        let blocks: Vec<serde_json::Value> = msg
            .content_blocks
            .iter()
            .map(|block| match block {
                ContentBlock::Text { text } => {
                    serde_json::json!({"type": "text", "text": text})
                }
                ContentBlock::Image { source } => match source {
                    ImageSource::Base64 { media_type, data } => serde_json::json!({
                        "type": "image",
                        "source": {"type": "base64", "media_type": media_type, "data": data}
                    }),
                    ImageSource::Url { url } => serde_json::json!({
                        "type": "image",
                        "source": {"type": "url", "url": url}
                    }),
                },
                ContentBlock::Document { source } => match source {
                    DocumentSource::Base64 { media_type, data } => serde_json::json!({
                        "type": "document",
                        "source": {"type": "base64", "media_type": media_type, "data": data}
                    }),
                    DocumentSource::Url { url } => serde_json::json!({
                        "type": "document",
                        "source": {"type": "url", "url": url}
                    }),
                },
            })
            .collect();

        let message_json = serde_json::json!({
            "role": "user",
            "content": blocks,
        });

        // Verify the structure matches Anthropic's expected format
        let content = message_json["content"].as_array().unwrap();
        assert_eq!(content.len(), 2);

        assert_eq!(content[0]["type"], "text");
        assert_eq!(content[0]["text"], "Summarize this contract");

        assert_eq!(content[1]["type"], "document");
        assert_eq!(content[1]["source"]["type"], "base64");
        assert_eq!(content[1]["source"]["media_type"], "application/pdf");
        assert_eq!(content[1]["source"]["data"], "JVBERi0xLjQK");
    }

    #[test]
    fn system_block_new_defaults_cache_control_false() {
        let block = SystemBlock::new("Hello");
        assert_eq!(block.text, "Hello");
        assert!(!block.cache_control);
    }

    #[test]
    fn system_block_cached_sets_cache_control_true() {
        let block = SystemBlock::cached("Cached prompt");
        assert_eq!(block.text, "Cached prompt");
        assert!(block.cache_control);
    }

    #[test]
    fn system_block_serde_roundtrip() {
        let block = SystemBlock::cached("test");
        let json = serde_json::to_value(&block).unwrap();
        assert_eq!(json["text"], "test");
        assert_eq!(json["cache_control"], true);

        let plain = SystemBlock::new("plain");
        let json = serde_json::to_value(&plain).unwrap();
        assert_eq!(json["text"], "plain");
        // cache_control should be skipped when false
        assert!(json.get("cache_control").is_none());
    }

    #[test]
    fn builder_system_block_appends() {
        let req = ChatRequest::builder()
            .system_block(SystemBlock::cached("Base instructions"))
            .system_block(SystemBlock::new("Dynamic context"))
            .message(Message::user("Hello"))
            .build();

        let blocks = req.system_blocks.unwrap();
        assert_eq!(blocks.len(), 2);
        assert_eq!(blocks[0].text, "Base instructions");
        assert!(blocks[0].cache_control);
        assert_eq!(blocks[1].text, "Dynamic context");
        assert!(!blocks[1].cache_control);
    }

    #[test]
    fn builder_system_blocks_replaces() {
        let req = ChatRequest::builder()
            .system_block(SystemBlock::new("Will be replaced"))
            .system_blocks(vec![SystemBlock::cached("A"), SystemBlock::new("B")])
            .message(Message::user("Hi"))
            .build();

        let blocks = req.system_blocks.unwrap();
        assert_eq!(blocks.len(), 2);
        assert_eq!(blocks[0].text, "A");
        assert_eq!(blocks[1].text, "B");
    }

    #[test]
    fn builder_system_blocks_not_set_by_default() {
        let req = ChatRequest::builder()
            .system("Plain system")
            .message(Message::user("Hi"))
            .build();

        assert!(req.system_blocks.is_none());
        assert_eq!(req.system.as_deref(), Some("Plain system"));
    }
}
