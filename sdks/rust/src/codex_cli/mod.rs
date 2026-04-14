//! OpenAI Codex CLI provider.
//!
//! This module wraps the `codex` CLI binary (from OpenAI's [Codex CLI][codex])
//! as a [`CodexCliClient`] that implements the same `chat`/`stream` surface as
//! the HTTP providers under [`crate::providers`].
//!
//! # How it works
//!
//! Unlike the HTTP providers which talk directly to a remote API, this client
//! shells out to `codex exec --json -`, writes the prompt to stdin, and parses
//! the newline-delimited JSON event stream on stdout. The CLI handles
//! authentication, tool execution, sandboxing, and approvals internally; we
//! just surface the final `agent_message` text plus token usage.
//!
//! # Why it lives outside `providers/`
//!
//! HTTP providers share request/response serialization machinery. A CLI-based
//! provider has a completely different architecture (subprocess lifetime,
//! stdin piping, JSONL parsing, exit codes), so it is kept as a standalone
//! top-level module alongside `claude_code`. Enable it with the
//! `codex-cli` Cargo feature.
//!
//! # Relationship to Codex CLI subcommands
//!
//! Only `codex exec` is supported today. The `resume` and `review` subcommands
//! will require a separate API surface and are out of scope.
//!
//! # Example
//!
//! ```ignore
//! use motosan_ai::{CodexCliClient, ChatRequestBuilder, Message, Role};
//! use motosan_ai::codex_cli::SandboxMode;
//!
//! let client = CodexCliClient::new()
//!     .sandbox(SandboxMode::WorkspaceWrite)
//!     .ephemeral(true);
//!
//! let request = ChatRequestBuilder::new()
//!     .user("Reply with 'pong'.")
//!     .build();
//!
//! let response = client.chat(request).await?;
//! println!("{}", response.content);
//! ```
//!
//! [codex]: https://developers.openai.com/codex/cli

pub mod prompt;
mod spawn;
mod stream_json;

use std::env;
use std::path::PathBuf;

use crate::error::MotosanError;
use crate::stream::BoxStream;
use crate::types::{ChatRequest, ChatResponse, StopReason};

pub use spawn::{LocalProvider, SandboxMode};

/// Client that shells out to the `codex` CLI binary (OpenAI Codex CLI).
///
/// Each call to [`chat`](Self::chat) or [`stream`](Self::stream) spawns a
/// fresh `codex exec --json -` subprocess, writes the composed prompt to
/// stdin, and parses the JSONL event stream on stdout. The CLI handles
/// authentication (via `CODEX_API_KEY` or `~/.codex/auth.json`), tool
/// execution, sandboxing, and approval prompts internally.
///
/// All fields are public so advanced callers can construct the struct
/// directly, but the builder methods ([`model`](Self::model),
/// [`sandbox`](Self::sandbox), etc.) are the intended construction path.
///
/// See the [module-level docs](crate::codex_cli) for a full example.
#[derive(Debug, Clone)]
pub struct CodexCliClient {
    /// Filesystem path to the `codex` binary. Defaults to `"codex"` (resolved
    /// through `PATH`) unless overridden by the `CODEX_PATH` env var or
    /// [`with_path`](Self::with_path).
    pub binary_path: PathBuf,

    /// When `true`, passes `--full-auto` so Codex can edit files and run
    /// shell commands without approval prompts. Equivalent in spirit to
    /// `ClaudeCodeClient::agent_mode`.
    ///
    /// Can coexist with an explicit [`sandbox`](Self::sandbox) — the Codex
    /// CLI reconciles the two.
    pub agent_mode: bool,

    /// Model name forwarded as `--model <value>`. `None` or the sentinel
    /// string `"default"` means "let Codex pick its configured default".
    pub model: Option<String>,

    /// Working directory forwarded as `--cd <path>`. Useful when the caller
    /// is running from a different directory than the target project.
    pub cd: Option<PathBuf>,

    /// Explicit sandbox policy (`--sandbox`). See [`SandboxMode`].
    pub sandbox: Option<SandboxMode>,

    /// Configuration profile name from `~/.codex/config.toml`, forwarded as
    /// `--profile <name>`. Blank / whitespace-only strings are ignored.
    pub profile: Option<String>,

    /// When `true`, passes `--ephemeral` so Codex does not persist a session
    /// rollout file to disk. Recommended for CI and one-shot automation.
    pub ephemeral: bool,

    /// Forwarded as repeated `-c key=value`. Escape hatch for anything the
    /// typed builders don't cover (e.g. `model_provider`, `features.*`).
    /// The value portion is parsed as TOML by Codex itself; string values
    /// must include escaped quotes, e.g. `"\"low\""`.
    pub config_overrides: Vec<(String, String)>,

    /// Additional writable workspace roots, forwarded as repeated
    /// `--add-dir <DIR>`. Useful when Codex needs write access outside the
    /// `cd()` directory.
    pub add_dirs: Vec<PathBuf>,

    /// Feature names to enable, forwarded as repeated `--enable <FEATURE>`.
    /// Equivalent to `config_override("features.<name>", "true")` but
    /// more ergonomic.
    pub enabled_features: Vec<String>,

    /// Feature names to disable, forwarded as repeated `--disable <FEATURE>`.
    pub disabled_features: Vec<String>,

    /// When `true`, passes `--dangerously-bypass-approvals-and-sandbox`.
    /// **EXTREMELY DANGEROUS** — only use when the caller is already
    /// externally sandboxed (disposable container, ephemeral VM).
    pub dangerously_bypass_approvals_and_sandbox: bool,

    /// When `true`, passes `--oss` so Codex talks to a local open-source
    /// provider stack instead of the OpenAI cloud.
    pub oss: bool,

    /// Selects the local provider (`lmstudio` / `ollama`) when
    /// [`oss`](Self::oss) is enabled. See [`LocalProvider`].
    pub local_provider: Option<LocalProvider>,
}

impl CodexCliClient {
    /// Construct a client that resolves the `codex` binary from the
    /// `CODEX_PATH` environment variable, falling back to `"codex"` in
    /// `PATH`.
    ///
    /// All other fields start at their defaults: no agent mode, no model
    /// override, no sandbox, no profile, no config overrides.
    pub fn new() -> Self {
        let binary_path = env::var_os("CODEX_PATH")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("codex"));
        Self::with_path(binary_path)
    }

    /// Construct a client with an explicit binary path. Useful when multiple
    /// `codex` versions coexist on the system or in integration tests.
    pub fn with_path(path: PathBuf) -> Self {
        Self {
            binary_path: path,
            agent_mode: false,
            model: None,
            cd: None,
            sandbox: None,
            profile: None,
            ephemeral: false,
            config_overrides: Vec::new(),
            add_dirs: Vec::new(),
            enabled_features: Vec::new(),
            disabled_features: Vec::new(),
            dangerously_bypass_approvals_and_sandbox: false,
            oss: false,
            local_provider: None,
        }
    }

    /// Enable or disable agent mode, which passes `--full-auto` to Codex.
    ///
    /// `--full-auto` is a Codex convenience alias for
    /// `--sandbox workspace-write` with approval prompts suppressed. Use it
    /// when the caller is already externally sandboxed (e.g. ephemeral CI
    /// runner, disposable container).
    pub fn agent_mode(mut self, enabled: bool) -> Self {
        self.agent_mode = enabled;
        self
    }

    /// Set the model forwarded as `--model <value>`.
    ///
    /// The sentinel string `"default"` (case-insensitive) and empty /
    /// whitespace-only strings are treated as "no override" and skipped.
    pub fn model(mut self, model: impl Into<String>) -> Self {
        self.model = Some(model.into());
        self
    }

    /// Set the working directory forwarded as `--cd <dir>`.
    ///
    /// Codex treats this as the workspace root for file edits and shell
    /// commands.
    pub fn cd(mut self, dir: impl Into<PathBuf>) -> Self {
        self.cd = Some(dir.into());
        self
    }

    /// Set an explicit sandbox policy (`--sandbox`).
    ///
    /// Can coexist with [`agent_mode(true)`](Self::agent_mode); Codex CLI
    /// will reconcile the two flags at invocation time.
    pub fn sandbox(mut self, mode: SandboxMode) -> Self {
        self.sandbox = Some(mode);
        self
    }

    /// Select a configuration profile from `~/.codex/config.toml`
    /// (`--profile <name>`).
    ///
    /// Blank / whitespace-only values are silently ignored.
    pub fn profile(mut self, profile: impl Into<String>) -> Self {
        self.profile = Some(profile.into());
        self
    }

    /// Run without persisting session rollout files to disk (`--ephemeral`).
    pub fn ephemeral(mut self, enabled: bool) -> Self {
        self.ephemeral = enabled;
        self
    }

    /// Append a `-c key=value` override. Repeatable.
    ///
    /// The value is parsed as TOML by Codex CLI. For string values, include
    /// escaped quotes in the input, e.g.
    /// `.config_override("model_reasoning_effort", "\"low\"")`.
    pub fn config_override(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.config_overrides.push((key.into(), value.into()));
        self
    }

    /// Append an additional writable workspace root (`--add-dir`). Repeatable.
    pub fn add_dir(mut self, dir: impl Into<PathBuf>) -> Self {
        self.add_dirs.push(dir.into());
        self
    }

    /// Enable a feature (`--enable <FEATURE>`). Repeatable.
    ///
    /// Equivalent to `.config_override("features.<name>", "true")` but more
    /// ergonomic. Empty strings are ignored.
    pub fn enable_feature(mut self, feature: impl Into<String>) -> Self {
        self.enabled_features.push(feature.into());
        self
    }

    /// Disable a feature (`--disable <FEATURE>`). Repeatable.
    pub fn disable_feature(mut self, feature: impl Into<String>) -> Self {
        self.disabled_features.push(feature.into());
        self
    }

    /// Pass `--dangerously-bypass-approvals-and-sandbox`.
    ///
    /// **EXTREMELY DANGEROUS.** Only use when the caller is already
    /// externally sandboxed (disposable container, ephemeral VM, or
    /// otherwise isolated execution environment). The long method name is
    /// intentional — there is no convenience alias.
    pub fn dangerously_bypass_approvals_and_sandbox(mut self, enabled: bool) -> Self {
        self.dangerously_bypass_approvals_and_sandbox = enabled;
        self
    }

    /// Use the OSS provider stack instead of OpenAI cloud (`--oss`).
    ///
    /// Pair with [`local_provider`](Self::local_provider) to pick which
    /// local backend (LM Studio / Ollama) Codex should talk to.
    pub fn oss(mut self, enabled: bool) -> Self {
        self.oss = enabled;
        self
    }

    /// Select the local OSS provider (`--local-provider <p>`).
    ///
    /// Only meaningful when [`oss(true)`](Self::oss) is also set. See
    /// [`LocalProvider`] for the supported variants.
    pub fn local_provider(mut self, provider: LocalProvider) -> Self {
        self.local_provider = Some(provider);
        self
    }

    /// Build the internal [`spawn::SpawnConfig`] from this client's state.
    ///
    /// A per-request `request_model` (from [`ChatRequest::model`]) takes
    /// precedence over the client-level default.
    fn build_spawn_config(&self, request_model: Option<String>) -> spawn::SpawnConfig {
        spawn::SpawnConfig {
            binary_path: self.binary_path.clone(),
            agent_mode: self.agent_mode,
            model: request_model.or_else(|| self.model.clone()),
            cd: self.cd.clone(),
            sandbox: self.sandbox,
            profile: self.profile.clone(),
            ephemeral: self.ephemeral,
            config_overrides: self.config_overrides.clone(),
            add_dirs: self.add_dirs.clone(),
            enabled_features: self.enabled_features.clone(),
            disabled_features: self.disabled_features.clone(),
            dangerously_bypass_approvals_and_sandbox: self.dangerously_bypass_approvals_and_sandbox,
            oss: self.oss,
            local_provider: self.local_provider,
        }
    }

    /// Send a chat request by invoking `codex exec --json` and waiting for
    /// the subprocess to exit.
    ///
    /// # Behavior
    ///
    /// Codex may emit multiple `agent_message` items during a single turn —
    /// typically a preamble narrating tool use followed by the final answer.
    /// This method treats the **last** agent message as the response
    /// [`content`](ChatResponse::content) and folds any preceding agent
    /// messages into [`thinking`](ChatResponse::thinking), joined by blank
    /// lines. Tool calls run internally by the CLI are **not** surfaced as
    /// [`ChatResponse::tool_calls`] — that field is always empty.
    ///
    /// # Errors
    ///
    /// - [`MotosanError::ProviderError`] if the subprocess fails to spawn,
    ///   times out (600 s hard limit), exits non-zero, emits an `error` /
    ///   `turn.failed` event, or writes malformed JSONL.
    pub async fn chat(&self, request: ChatRequest) -> Result<ChatResponse, MotosanError> {
        let (msg_system, user_prompt) = prompt::messages_to_prompt(&request.messages);
        let system_prompt = request.system.clone().or(msg_system);
        let composed = prompt::compose_prompt(system_prompt.as_deref(), &user_prompt);

        let config = self.build_spawn_config(request.model.clone());

        let (content, thinking, usage) = spawn::invoke_cli(&config, &composed).await?;

        Ok(ChatResponse {
            content,
            thinking,
            tool_calls: vec![],
            model: config.model.unwrap_or_default(),
            usage,
            stop_reason: StopReason::EndTurn,
        })
    }

    /// Stream a chat request via `codex exec --json`, yielding
    /// [`StreamEvent`](crate::types::StreamEvent)s as Codex emits them.
    ///
    /// # Event shape
    ///
    /// Codex emits **complete** `agent_message` items (not token deltas), so
    /// each [`Text`](crate::types::StreamEventType::Text) event carries one
    /// finalized agent message — callers should not expect per-token
    /// streaming. The event order is:
    ///
    /// 1. Zero or more `Text` events (preamble + final answer),
    /// 2. Optional `Usage` event (from `turn.completed`),
    /// 3. A terminal `done` event.
    ///
    /// Non-`agent_message` items (reasoning, command execution, file
    /// changes, etc.) are filtered out.
    ///
    /// # Errors
    ///
    /// Returns [`MotosanError::ProviderError`] only if the subprocess fails
    /// to spawn or stdin cannot be opened. Runtime errors from Codex
    /// (`error` / `turn.failed` events) terminate the stream with a `done`
    /// event rather than returning `Err`, because the error has already
    /// been partially consumed by the caller at that point.
    pub async fn stream(&self, request: ChatRequest) -> Result<BoxStream, MotosanError> {
        use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
        use tokio::process::Command;

        let (msg_system, user_prompt) = prompt::messages_to_prompt(&request.messages);
        let system_prompt = request.system.clone().or(msg_system);
        let composed = prompt::compose_prompt(system_prompt.as_deref(), &user_prompt);
        let config = self.build_spawn_config(request.model.clone());

        let mut cmd = Command::new(&config.binary_path);
        cmd.arg("exec").arg("--json").arg("--skip-git-repo-check");
        spawn::apply_common_args(&mut cmd, &config);

        cmd.arg("-");
        cmd.kill_on_drop(true);
        cmd.stdin(std::process::Stdio::piped());
        cmd.stdout(std::process::Stdio::piped());
        cmd.stderr(std::process::Stdio::piped());

        let mut child = cmd
            .spawn()
            .map_err(|e| MotosanError::ProviderError(format!("failed to spawn codex CLI: {e}")))?;

        {
            let mut stdin = child.stdin.take().ok_or_else(|| {
                MotosanError::ProviderError("failed to open codex CLI stdin".to_string())
            })?;
            stdin.write_all(composed.as_bytes()).await.map_err(|e| {
                MotosanError::ProviderError(format!("failed to write to codex stdin: {e}"))
            })?;
        }

        let stdout = child.stdout.take().ok_or_else(|| {
            MotosanError::ProviderError("failed to open codex CLI stdout".to_string())
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
                        stream_json::NdjsonAction::Done { usage, done } => {
                            if let Some(usage_event) = usage {
                                yield usage_event;
                            }
                            yield done;
                            break;
                        }
                        stream_json::NdjsonAction::Error(_msg) => {
                            // Terminate the stream; callers can inspect
                            // exit status via their own monitoring if needed.
                            yield crate::types::StreamEvent::done();
                            break;
                        }
                    }
                }
            }

            let _ = child.wait().await;
        };

        Ok(Box::pin(stream) as BoxStream)
    }
}

impl Default for CodexCliClient {
    /// Equivalent to [`CodexCliClient::new`].
    fn default() -> Self {
        Self::new()
    }
}

/// Polymorphic dispatch via [`crate::providers::ProviderImpl`].
///
/// The `chat` and `stream` methods on this impl forward directly to the
/// inherent methods of the same name (via fully-qualified syntax to avoid
/// recursion). This lets `CodexCliClient` slot into any
/// `Box<dyn ProviderImpl>` / `&dyn ProviderImpl` consumer alongside the HTTP
/// providers without changing call sites.
#[async_trait::async_trait]
impl crate::providers::ProviderImpl for CodexCliClient {
    async fn chat(&self, req: ChatRequest) -> Result<ChatResponse, MotosanError> {
        CodexCliClient::chat(self, req).await
    }

    async fn stream(&self, req: ChatRequest) -> Result<BoxStream, MotosanError> {
        CodexCliClient::stream(self, req).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{ChatRequest, Message, Role};

    /// Compile-time + runtime check: `CodexCliClient` can be coerced into
    /// `Box<dyn ProviderImpl>` and used polymorphically. We don't actually
    /// invoke `chat`/`stream` here (that would spawn a subprocess); we just
    /// confirm the trait object can be constructed and the methods are
    /// addressable through the dyn dispatch.
    #[test]
    fn codex_cli_client_implements_provider_impl() {
        use crate::providers::ProviderImpl;
        let _client: Box<dyn ProviderImpl> = Box::new(CodexCliClient::new());
    }

    fn pong_request() -> ChatRequest {
        ChatRequest {
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
        }
    }

    #[tokio::test]
    #[ignore] // Requires `codex` CLI installed + auth; run with `cargo test --features codex-cli -- --ignored`
    async fn integration_chat_with_new_flags() {
        // Exercises sandbox + ephemeral + config_override end-to-end: if any
        // flag was malformed the CLI would exit non-zero and this test would fail.
        let client = CodexCliClient::new()
            .sandbox(SandboxMode::ReadOnly)
            .ephemeral(true)
            .config_override("model_reasoning_effort", "\"low\"");

        let resp = client
            .chat(pong_request())
            .await
            .expect("chat with new flags should succeed");
        assert!(
            resp.content.to_lowercase().contains("pong"),
            "expected 'pong' in content, got: {}",
            resp.content
        );
    }

    #[tokio::test]
    #[ignore] // Requires `codex` CLI installed + auth; run with `cargo test --features codex-cli -- --ignored`
    async fn integration_stream_roundtrip() {
        use crate::types::StreamEventType;
        use tokio_stream::StreamExt;

        let client = CodexCliClient::new();
        let mut stream = client
            .stream(pong_request())
            .await
            .expect("stream should start");

        let mut content = String::new();
        let mut got_usage = false;
        let mut got_done = false;
        while let Some(event) = stream.next().await {
            if event.done {
                got_done = true;
                break;
            }
            match event.event_type {
                StreamEventType::Text => content.push_str(&event.content),
                StreamEventType::Usage => {
                    if let Some(u) = event.usage {
                        // Real turns always report non-zero input tokens.
                        assert!(u.input_tokens > 0, "expected non-zero input_tokens");
                        got_usage = true;
                    }
                }
                _ => {}
            }
        }

        assert!(got_done, "stream should terminate with a done event");
        assert!(got_usage, "stream should surface a usage event");
        assert!(
            content.to_lowercase().contains("pong"),
            "expected 'pong' in streamed content, got: {content}"
        );
    }

    #[tokio::test]
    #[ignore] // Requires `codex` CLI installed + auth; run with `cargo test --features codex-cli -- --ignored`
    async fn integration_chat_roundtrip() {
        let client = CodexCliClient::new();
        let resp = client
            .chat(pong_request())
            .await
            .expect("chat should succeed");
        assert!(
            resp.content.to_lowercase().contains("pong"),
            "expected 'pong' in response, got: {}",
            resp.content
        );
    }
}
