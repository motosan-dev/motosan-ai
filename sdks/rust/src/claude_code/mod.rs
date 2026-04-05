pub mod prompt;
mod spawn;
mod stream_json;

use std::env;
use std::path::PathBuf;

use crate::error::MotosanError;
use crate::types::{ChatRequest, ChatResponse, StopReason};

/// Client that shells out to the `claude` CLI binary.
#[derive(Debug, Clone)]
pub struct ClaudeCodeClient {
    pub binary_path: PathBuf,
    pub agent_mode: bool,
    pub model: Option<String>,
}

impl ClaudeCodeClient {
    /// Resolve the binary from `CLAUDE_CODE_PATH` env or fall back to `"claude"` in `PATH`.
    pub fn new() -> Self {
        let binary_path = env::var_os("CLAUDE_CODE_PATH")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("claude"));

        Self {
            binary_path,
            agent_mode: false,
            model: None,
        }
    }

    /// Use an explicit binary path.
    pub fn with_path(path: PathBuf) -> Self {
        Self {
            binary_path: path,
            agent_mode: false,
            model: None,
        }
    }

    /// Enable or disable agent mode.
    pub fn agent_mode(mut self, enabled: bool) -> Self {
        self.agent_mode = enabled;
        self
    }

    /// Set the model to use.
    pub fn model(mut self, model: impl Into<String>) -> Self {
        self.model = Some(model.into());
        self
    }

    /// Send a chat request by invoking the `claude` CLI as a subprocess.
    pub async fn chat(&self, request: ChatRequest) -> Result<ChatResponse, MotosanError> {
        // Extract system prompt: prefer request.system, fall back to first system message
        let (msg_system, user_prompt) = prompt::messages_to_prompt(&request.messages);
        let system_prompt = request.system.or(msg_system);

        let config = spawn::SpawnConfig {
            binary_path: self.binary_path.clone(),
            agent_mode: self.agent_mode,
            model: request.model.or_else(|| self.model.clone()),
            system_prompt,
        };

        let (text, usage) = spawn::invoke_cli(&config, &user_prompt).await?;

        Ok(ChatResponse {
            content: text,
            thinking: None,
            tool_calls: vec![],
            model: config.model.unwrap_or_default(),
            usage,
            stop_reason: StopReason::EndTurn,
        })
    }
}

impl Default for ClaudeCodeClient {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{ChatRequest, Message, Role};

    #[tokio::test]
    #[ignore] // Requires `claude` CLI installed; run manually with `cargo test --features claude-code -- --ignored`
    async fn integration_chat_roundtrip() {
        let client = ClaudeCodeClient::new();
        let request = ChatRequest {
            messages: vec![Message {
                role: Role::User,
                content: "Reply with only the word 'pong'.".to_string(),
                content_blocks: vec![],
                tool_call_id: None,
                tool_calls: vec![],
                cache: false,
            }],
            model: None,
            system: None,
            system_blocks: None,
            system_cache: false,
            temperature: None,
            max_tokens: None,
            tools: None,
            tool_choice: None,
            provider_options: None,
            mcp_servers: None,
            mcp_tool_configs: None,
            thinking: None,
            stop_sequences: None,
        };

        let resp = client.chat(request).await.expect("chat should succeed");
        assert!(
            resp.content.to_lowercase().contains("pong"),
            "expected 'pong' in response, got: {}",
            resp.content
        );
    }
}
