//! Google Gemini CLI provider.
//!
//! Shells out to the `gemini` binary in headless mode
//! (`gemini -p "" -o stream-json ...`) and parses its NDJSON output into
//! motosan-ai's [`ChatResponse`] / [`BoxStream`] types. Uses the same
//! shape as [`claude_code`](super::claude_code) and
//! [`codex_cli`](super::codex_cli) so all three CLI backends are
//! interchangeable via `Box<dyn ProviderImpl>`.
//!
//! # Auth
//!
//! The `gemini` CLI manages its own auth state (personal Google
//! account, API key, or Vertex). motosan-ai does not pass any
//! credentials through — run `gemini auth` once beforehand.
//!
//! # System prompts
//!
//! Gemini CLI has no `--system-prompt` flag, so the system prompt is
//! merged into the stdin payload as a plain prefix (separated by a
//! blank line). This matches how the CLI treats `GEMINI.md` context.

pub mod prompt;
mod spawn;
mod stream_json;

pub use spawn::ApprovalMode;

use std::env;
use std::path::PathBuf;

use crate::error::MotosanError;
use crate::stream::BoxStream;
use crate::types::{ChatRequest, ChatResponse, StopReason};

/// Client that shells out to the `gemini` CLI binary.
#[derive(Debug, Clone)]
pub struct GeminiCliProvider {
    /// Path to the `gemini` binary. Defaults to `$GEMINI_CLI_PATH` or
    /// `"gemini"` (resolved via `PATH`).
    pub binary_path: PathBuf,
    /// Default model name, forwarded as `-m <model>`. Overridable per
    /// request via [`ChatRequest::model`](crate::types::ChatRequest::model).
    pub model: Option<String>,
    /// Whether to pass `--yolo` (auto-approve every tool call).
    pub yolo: bool,
    /// Whether to pass `--sandbox` / `-s`.
    pub sandbox: bool,
    /// Optional `--approval-mode` override.
    pub approval_mode: Option<ApprovalMode>,
    /// Additional workspace roots, forwarded as repeated
    /// `--include-directories <DIR>` flags.
    pub include_dirs: Vec<PathBuf>,
    /// Extensions to load, forwarded as repeated `-e <NAME>` flags.
    pub extensions: Vec<String>,
    /// MCP server allowlist, forwarded as repeated
    /// `--allowed-mcp-server-names <NAME>` flags.
    pub allowed_mcp_servers: Vec<String>,
    /// Session to resume, forwarded as `--resume <value>` (accepts
    /// `"latest"` or a numeric index).
    pub resume: Option<String>,
    /// Working directory for the spawned `gemini` process. When set, the child
    /// runs with this cwd (`Command::current_dir`) instead of inheriting the parent's.
    pub cwd: Option<PathBuf>,
}

impl GeminiCliProvider {
    /// Resolve the binary from `GEMINI_CLI_PATH` env or fall back to
    /// `"gemini"` in `PATH`.
    pub fn new() -> Self {
        let binary_path = env::var_os("GEMINI_CLI_PATH")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("gemini"));

        Self {
            binary_path,
            model: None,
            yolo: false,
            sandbox: false,
            approval_mode: None,
            include_dirs: Vec::new(),
            extensions: Vec::new(),
            allowed_mcp_servers: Vec::new(),
            resume: None,
            cwd: None,
        }
    }

    /// Use an explicit binary path.
    pub fn with_path(path: PathBuf) -> Self {
        Self {
            binary_path: path,
            ..Self::new()
        }
    }

    /// Set the default model.
    pub fn model(mut self, model: impl Into<String>) -> Self {
        self.model = Some(model.into());
        self
    }

    /// Enable `--yolo` (auto-approve everything).
    pub fn yolo(mut self, enabled: bool) -> Self {
        self.yolo = enabled;
        self
    }

    /// Enable `--sandbox`.
    pub fn sandbox(mut self, enabled: bool) -> Self {
        self.sandbox = enabled;
        self
    }

    /// Set the approval mode.
    pub fn approval_mode(mut self, mode: ApprovalMode) -> Self {
        self.approval_mode = Some(mode);
        self
    }

    /// Append an additional workspace root (`--include-directories`).
    /// Call repeatedly to include multiple directories.
    pub fn include_dir(mut self, dir: impl Into<PathBuf>) -> Self {
        self.include_dirs.push(dir.into());
        self
    }

    /// Replace the full set of additional workspace roots.
    pub fn include_dirs(mut self, dirs: Vec<PathBuf>) -> Self {
        self.include_dirs = dirs;
        self
    }

    /// Add a single extension name (`-e <name>`).
    pub fn extension(mut self, name: impl Into<String>) -> Self {
        self.extensions.push(name.into());
        self
    }

    /// Replace the full list of extensions to load.
    pub fn extensions(mut self, names: Vec<String>) -> Self {
        self.extensions = names;
        self
    }

    /// Allow a single MCP server name
    /// (`--allowed-mcp-server-names <name>`).
    pub fn allowed_mcp_server(mut self, name: impl Into<String>) -> Self {
        self.allowed_mcp_servers.push(name.into());
        self
    }

    /// Replace the full MCP server allowlist.
    pub fn allowed_mcp_servers(mut self, names: Vec<String>) -> Self {
        self.allowed_mcp_servers = names;
        self
    }

    /// Resume a previous session by forwarding this value verbatim to
    /// `--resume <value>`. Known Gemini CLI values include `latest` and
    /// numeric indexes; captured `session_id` values are forwarded unchanged,
    /// but arbitrary-id resume requires live CLI verification.
    pub fn resume(mut self, session: impl Into<String>) -> Self {
        self.resume = Some(session.into());
        self
    }

    /// Set the working directory for the spawned process (`Command::current_dir`).
    pub fn cwd(mut self, dir: impl Into<PathBuf>) -> Self {
        self.cwd = Some(dir.into());
        self
    }

    fn build_spawn_config(&self, request_model: Option<String>) -> spawn::SpawnConfig {
        spawn::SpawnConfig {
            binary_path: self.binary_path.clone(),
            model: request_model.or_else(|| self.model.clone()),
            yolo: self.yolo,
            sandbox: self.sandbox,
            approval_mode: self.approval_mode,
            include_dirs: self.include_dirs.clone(),
            extensions: self.extensions.clone(),
            allowed_mcp_servers: self.allowed_mcp_servers.clone(),
            resume: self.resume.clone(),
            cwd: self.cwd.clone(),
        }
    }

    /// Send a chat request by invoking the `gemini` CLI as a subprocess.
    pub async fn chat(&self, request: ChatRequest) -> Result<ChatResponse, MotosanError> {
        let (msg_system, user_prompt) = prompt::messages_to_prompt(&request.messages);
        let system_prompt = request.system.clone().or(msg_system);
        let stdin_payload = merge_system_into_prompt(system_prompt.as_deref(), &user_prompt);

        let config = self.build_spawn_config(request.model);
        let (text, usage, session_id) = spawn::invoke_cli(&config, &stdin_payload).await?;

        Ok(ChatResponse {
            content: text,
            thinking: None,
            tool_calls: vec![],
            model: config.model.unwrap_or_default(),
            usage,
            stop_reason: StopReason::EndTurn,
            session_id,
        })
    }

    /// Stream a chat request via `gemini -p "" -o stream-json`.
    ///
    /// Returns a [`BoxStream`] that yields [`StreamEvent`](crate::types::StreamEvent)
    /// items parsed from the NDJSON events Gemini CLI emits on stdout.
    pub async fn stream(&self, request: ChatRequest) -> Result<BoxStream, MotosanError> {
        use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
        use tokio::process::Command;

        let (msg_system, user_prompt) = prompt::messages_to_prompt(&request.messages);
        let system_prompt = request.system.clone().or(msg_system);
        let stdin_payload = merge_system_into_prompt(system_prompt.as_deref(), &user_prompt);

        let config = self.build_spawn_config(request.model);

        let mut cmd = Command::new(&config.binary_path);
        if let Some(dir) = &config.cwd {
            cmd.current_dir(dir);
        }
        cmd.args(spawn::common_args(&config));
        cmd.kill_on_drop(true);
        cmd.stdin(std::process::Stdio::piped());
        cmd.stdout(std::process::Stdio::piped());
        cmd.stderr(std::process::Stdio::piped());

        let mut child = cmd
            .spawn()
            .map_err(|e| MotosanError::ProviderError(format!("failed to spawn gemini CLI: {e}")))?;

        {
            let mut stdin = child.stdin.take().ok_or_else(|| {
                MotosanError::ProviderError("failed to open gemini CLI stdin".to_string())
            })?;
            stdin
                .write_all(stdin_payload.as_bytes())
                .await
                .map_err(|e| {
                    MotosanError::ProviderError(format!("failed to write to gemini stdin: {e}"))
                })?;
        }

        let stdout = child.stdout.take().ok_or_else(|| {
            MotosanError::ProviderError("failed to open gemini CLI stdout".to_string())
        })?;

        let reader = BufReader::new(stdout);

        let stream = async_stream::stream! {
            let mut lines = reader.lines();

            while let Ok(Some(line)) = lines.next_line().await {
                let line = line.trim().to_string();
                if line.is_empty() {
                    continue;
                }

                match stream_json::parse_ndjson_line(&line) {
                    Some(stream_json::NdjsonAction::Text(event)) => {
                        yield event;
                    }
                    Some(stream_json::NdjsonAction::SessionStarted(event)) => {
                        yield event;
                    }
                    Some(stream_json::NdjsonAction::Result { usage, done }) => {
                        if let Some(usage_event) = usage {
                            yield usage_event;
                        }
                        yield done;
                        break;
                    }
                    Some(stream_json::NdjsonAction::Error(_msg)) => {
                        // StreamEvent has no error variant; other CLI
                        // providers simply drop the stream here. Callers
                        // that need the error surface should use `chat()`.
                        break;
                    }
                    None => {}
                }
            }

            let _ = child.wait().await;
        };

        Ok(Box::pin(stream) as BoxStream)
    }
}

/// Merge the system prompt onto the user prompt for stdin delivery.
///
/// Gemini CLI has no `--system-prompt` equivalent, so we prepend the
/// system text followed by a blank line. An empty or missing system
/// prompt passes the user prompt through unchanged.
fn merge_system_into_prompt(system: Option<&str>, user: &str) -> String {
    match system {
        Some(s) if !s.is_empty() => format!("{s}\n\n{user}"),
        _ => user.to_string(),
    }
}

impl Default for GeminiCliProvider {
    fn default() -> Self {
        Self::new()
    }
}

/// Polymorphic dispatch via [`super::ProviderImpl`].
#[async_trait::async_trait]
impl super::ProviderImpl for GeminiCliProvider {
    async fn chat(&self, req: ChatRequest) -> Result<ChatResponse, MotosanError> {
        GeminiCliProvider::chat(self, req).await
    }

    async fn stream(&self, req: ChatRequest) -> Result<BoxStream, MotosanError> {
        GeminiCliProvider::stream(self, req).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{ChatRequest, Message, Role};

    /// Compile-time + runtime check that `GeminiCliProvider` can be
    /// coerced into `Box<dyn ProviderImpl>` alongside the HTTP and
    /// sibling CLI providers. No subprocess is spawned.
    #[test]
    fn gemini_cli_client_implements_provider_impl() {
        use crate::providers::ProviderImpl;
        let _client: Box<dyn ProviderImpl> = Box::new(GeminiCliProvider::new());
    }

    #[test]
    fn cwd_builder_threads_into_spawn_config() {
        let cfg = GeminiCliProvider::new()
            .cwd("/work/dir")
            .build_spawn_config(None);
        assert_eq!(cfg.cwd.as_deref(), Some(std::path::Path::new("/work/dir")));
    }

    #[test]
    fn merge_system_into_prompt_prepends_non_empty_system() {
        assert_eq!(
            merge_system_into_prompt(Some("you are helpful"), "hello"),
            "you are helpful\n\nhello"
        );
    }

    #[test]
    fn merge_system_into_prompt_passes_through_without_system() {
        assert_eq!(merge_system_into_prompt(None, "hello"), "hello");
        assert_eq!(merge_system_into_prompt(Some(""), "hello"), "hello");
    }

    #[tokio::test]
    #[ignore] // Requires the `gemini` CLI installed and authenticated; run manually with `cargo test --features gemini-cli -- --ignored`
    async fn integration_chat_roundtrip() {
        let client = GeminiCliProvider::new();
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
