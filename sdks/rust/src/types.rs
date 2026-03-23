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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatRequest {
    pub messages: Vec<Message>,
    pub model: Option<String>,
    pub system: Option<String>,
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thinking: Option<ThinkingConfig>,
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
    system_cache: bool,
    temperature: Option<f32>,
    max_tokens: Option<u32>,
    tools: Option<Vec<Tool>>,
    tool_choice: Option<ToolChoice>,
    provider_options: Option<Value>,
    mcp_servers: Option<Vec<McpServerConfig>>,
    thinking: Option<ThinkingConfig>,
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
    pub fn mcp_server(mut self, server: McpServerConfig) -> Self {
        self.mcp_servers.get_or_insert_with(Vec::new).push(server);
        self
    }

    /// Set the full list of server-side MCP server configurations.
    pub fn mcp_servers(mut self, servers: Vec<McpServerConfig>) -> Self {
        self.mcp_servers = Some(servers);
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

    pub fn build(self) -> ChatRequest {
        ChatRequest {
            messages: self.messages,
            model: self.model,
            system: self.system,
            system_cache: self.system_cache,
            temperature: self.temperature,
            max_tokens: self.max_tokens,
            tools: self.tools,
            tool_choice: self.tool_choice,
            provider_options: self.provider_options,
            mcp_servers: self.mcp_servers,
            thinking: self.thinking,
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
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ImageSource {
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
        }
    }
}
