mod spawn;
mod stream_json;

use std::env;
use std::path::PathBuf;

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
}

impl Default for ClaudeCodeClient {
    fn default() -> Self {
        Self::new()
    }
}
