pub mod prompt;
mod spawn;
mod stream_json;

use std::env;
use std::path::PathBuf;

use crate::error::MotosanError;
use crate::stream::BoxStream;
use crate::types::{ChatRequest, ChatResponse, StopReason};

/// Client that shells out to the `claude` CLI binary.
#[derive(Debug, Clone)]
pub struct ClaudeCodeProvider {
    pub binary_path: PathBuf,
    pub agent_mode: bool,
    pub model: Option<String>,
}

impl ClaudeCodeProvider {
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

    /// Stream a chat request via `claude --print --output-format stream-json`.
    ///
    /// Returns a `BoxStream` that yields `StreamEvent` items parsed from
    /// newline-delimited JSON events emitted by the CLI.
    pub async fn stream(&self, request: ChatRequest) -> Result<BoxStream, MotosanError> {
        use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
        use tokio::process::Command;

        let (msg_system, user_prompt) = prompt::messages_to_prompt(&request.messages);
        let system_prompt = request.system.or(msg_system);
        let model = request.model.or_else(|| self.model.clone());

        let mut cmd = Command::new(&self.binary_path);
        cmd.arg("--print").arg("--output-format").arg("stream-json");

        if self.agent_mode {
            cmd.arg("--dangerously-skip-permissions");
        }

        if let Some(ref m) = model {
            if let Some(m) = spawn::model_to_forward(m) {
                cmd.arg("--model").arg(m);
            }
        }

        if let Some(ref sp) = system_prompt {
            if !sp.is_empty() {
                cmd.arg("--append-system-prompt").arg(sp);
            }
        }

        cmd.arg("-");
        cmd.kill_on_drop(true);
        cmd.stdin(std::process::Stdio::piped());
        cmd.stdout(std::process::Stdio::piped());
        cmd.stderr(std::process::Stdio::piped());

        let mut child = cmd
            .spawn()
            .map_err(|e| MotosanError::ProviderError(format!("failed to spawn claude CLI: {e}")))?;

        // Write prompt to stdin then close it
        {
            let mut stdin = child.stdin.take().ok_or_else(|| {
                MotosanError::ProviderError("failed to open claude CLI stdin".to_string())
            })?;
            stdin.write_all(user_prompt.as_bytes()).await.map_err(|e| {
                MotosanError::ProviderError(format!("failed to write to claude stdin: {e}"))
            })?;
            // stdin dropped here, sending EOF
        }

        let stdout = child.stdout.take().ok_or_else(|| {
            MotosanError::ProviderError("failed to open claude CLI stdout".to_string())
        })?;

        let reader = BufReader::new(stdout);

        let stream = async_stream::stream! {
            let mut lines = reader.lines();

            while let Ok(Some(line)) = lines.next_line().await {
                let line = line.trim().to_string();
                if line.is_empty() {
                    continue;
                }

                if let Some(action) = stream_json::parse_ndjson_line(&line) {
                    match action {
                        stream_json::NdjsonAction::Text(event) => {
                            yield event;
                        }
                        stream_json::NdjsonAction::Result { usage, done } => {
                            if let Some(usage_event) = usage {
                                yield usage_event;
                            }
                            yield done;
                            break;
                        }
                    }
                }
            }

            // Wait for child to exit
            let _ = child.wait().await;
        };

        Ok(Box::pin(stream) as BoxStream)
    }
}

impl Default for ClaudeCodeProvider {
    fn default() -> Self {
        Self::new()
    }
}

/// Polymorphic dispatch via [`super::ProviderImpl`].
///
/// Forwards to the inherent `chat` / `stream` methods (via fully-qualified
/// call syntax to avoid recursion). Lets `ClaudeCodeProvider` plug into any
/// `Box<dyn ProviderImpl>` / `&dyn ProviderImpl` consumer alongside the HTTP
/// providers.
#[async_trait::async_trait]
impl super::ProviderImpl for ClaudeCodeProvider {
    async fn chat(&self, req: ChatRequest) -> Result<ChatResponse, MotosanError> {
        ClaudeCodeProvider::chat(self, req).await
    }

    async fn stream(&self, req: ChatRequest) -> Result<BoxStream, MotosanError> {
        ClaudeCodeProvider::stream(self, req).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{ChatRequest, Message, Role};

    /// Compile-time + runtime check that `ClaudeCodeProvider` can be coerced
    /// into `Box<dyn ProviderImpl>` and used polymorphically alongside the
    /// HTTP providers. No subprocess is spawned.
    #[test]
    fn claude_code_client_implements_provider_impl() {
        use crate::providers::ProviderImpl;
        let _client: Box<dyn ProviderImpl> = Box::new(ClaudeCodeProvider::new());
    }

    #[tokio::test]
    #[ignore] // Requires `claude` CLI installed; run manually with `cargo test --features claude-code -- --ignored`
    async fn integration_chat_roundtrip() {
        let client = ClaudeCodeProvider::new();
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
