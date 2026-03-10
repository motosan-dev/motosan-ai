//! Anthropic Claude provider.
//!
//! Implements the `ProviderImpl` trait for Anthropic's Messages API.
//! Enabled via `features = ["anthropic"]`.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use crate::{
    error::MotosanError,
    stream::{BoxStream, StreamEvent},
    types::{ChatRequest, ChatResponse, Role, StopReason, Usage},
};
use super::ProviderImpl;

const DEFAULT_MODEL: &str = "claude-sonnet-4-5";
const API_BASE: &str = "https://api.anthropic.com";
const ANTHROPIC_VERSION: &str = "2023-06-01";

pub struct AnthropicProvider {
    api_key: String,
    client: reqwest::Client,
}

impl AnthropicProvider {
    pub fn new(api_key: impl Into<String>) -> Self {
        Self {
            api_key: api_key.into(),
            client: reqwest::Client::new(),
        }
    }
}

// ── Anthropic request/response types ─────────────────────────────────────────

#[derive(Serialize)]
struct AnthropicRequest {
    model: String,
    messages: Vec<AnthropicMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    system: Option<String>,
    max_tokens: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    stream: Option<bool>,
}

#[derive(Serialize, Deserialize)]
struct AnthropicMessage {
    role: String,
    content: String,
}

#[derive(Deserialize)]
struct AnthropicResponse {
    content: Vec<AnthropicContent>,
    model: String,
    stop_reason: Option<String>,
    usage: AnthropicUsage,
}

#[derive(Deserialize)]
struct AnthropicContent {
    #[serde(rename = "type")]
    content_type: String,
    text: Option<String>,
}

#[derive(Deserialize)]
struct AnthropicUsage {
    input_tokens: u32,
    output_tokens: u32,
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn to_anthropic_messages(req: &ChatRequest) -> (Vec<AnthropicMessage>, Option<String>) {
    let mut system: Option<String> = req.system.clone();
    let mut messages = Vec::new();

    for msg in &req.messages {
        match msg.role {
            // Anthropic separates system from messages
            Role::System => system = Some(msg.content.clone()),
            Role::User => messages.push(AnthropicMessage {
                role: "user".into(),
                content: msg.content.clone(),
            }),
            Role::Assistant => messages.push(AnthropicMessage {
                role: "assistant".into(),
                content: msg.content.clone(),
            }),
        }
    }

    (messages, system)
}

fn parse_stop_reason(s: Option<&str>) -> StopReason {
    match s {
        Some("end_turn") => StopReason::EndTurn,
        Some("max_tokens") => StopReason::MaxTokens,
        Some("tool_use") => StopReason::ToolUse,
        Some("stop_sequence") => StopReason::Stop,
        Some(other) => StopReason::Other(other.to_string()),
        None => StopReason::EndTurn,
    }
}

// ── ProviderImpl ──────────────────────────────────────────────────────────────

#[async_trait]
impl ProviderImpl for AnthropicProvider {
    async fn chat(&self, req: ChatRequest) -> Result<ChatResponse, MotosanError> {
        let (messages, system) = to_anthropic_messages(&req);

        let body = AnthropicRequest {
            model: req.model.clone().unwrap_or_else(|| DEFAULT_MODEL.to_string()),
            messages,
            system,
            max_tokens: req.max_tokens.unwrap_or(1024),
            temperature: req.temperature,
            stream: None,
        };

        let resp = self.client
            .post(format!("{API_BASE}/v1/messages"))
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", ANTHROPIC_VERSION)
            .header("content-type", "application/json")
            .json(&body)
            .send()
            .await?;

        let status = resp.status();
        if !status.is_success() {
            let msg = resp.text().await.unwrap_or_default();
            return match status.as_u16() {
                401 => Err(MotosanError::Auth(msg)),
                429 => Err(MotosanError::RateLimit { retry_after: None }),
                _ => Err(MotosanError::ProviderError { status: status.as_u16(), message: msg }),
            };
        }

        let data: AnthropicResponse = resp.json().await?;
        let content = data.content.iter()
            .filter(|c| c.content_type == "text")
            .filter_map(|c| c.text.as_deref())
            .collect::<Vec<_>>()
            .join("");

        Ok(ChatResponse {
            content,
            model: data.model,
            usage: Usage {
                input_tokens: data.usage.input_tokens,
                output_tokens: data.usage.output_tokens,
            },
            stop_reason: parse_stop_reason(data.stop_reason.as_deref()),
        })
    }

    async fn stream(&self, _req: ChatRequest) -> Result<BoxStream, MotosanError> {
        // TODO: implement SSE streaming — tracked in issue #4
        Err(MotosanError::Stream("Streaming not yet implemented".to_string()))
    }
}
