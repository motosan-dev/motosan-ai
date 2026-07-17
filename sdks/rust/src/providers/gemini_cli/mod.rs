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
//!
//! # Cancellation
//!
//! There is no explicit cancel handle. Spawned CLI children use
//! `kill_on_drop(true)`: dropping the `chat()` future or returned [`BoxStream`]
//! terminates the child process. Stream drivers drain stderr concurrently and
//! reap and validate the exit status before yielding terminal success. Use
//! [`GeminiCliProvider::timeout`] to bound read stalls.

pub mod prompt;
mod spawn;
mod stream_json;

pub use spawn::{ApprovalMode, DEFAULT_TIMEOUT};

use std::env;
use std::path::PathBuf;
use std::time::Duration;

use crate::error::MotosanError;
use crate::providers::redacted_envs::RedactedEnvs;
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
    /// Extra environment variables injected into the spawned `gemini` child,
    /// in insertion order. Use for a per-run secret bundle (e.g. GEMINI_API_KEY)
    /// without mutating the parent environment. Values are secrets (redacted in Debug).
    pub envs: RedactedEnvs,
    /// Working directory for the spawned `gemini` process. When set, the child
    /// runs with this cwd (`Command::current_dir`) instead of inheriting the parent's.
    pub cwd: Option<PathBuf>,
    /// Per-invocation timeout for `chat()` and the stream read loop.
    /// `None` disables the timeout.
    pub timeout: Option<Duration>,
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
            envs: RedactedEnvs::default(),
            cwd: None,
            timeout: Some(spawn::DEFAULT_TIMEOUT),
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

    /// Override the per-invocation timeout (applies to `chat()` and the
    /// `stream()` read loop).
    pub fn timeout(mut self, dur: Duration) -> Self {
        self.timeout = Some(dur);
        self
    }

    /// Disable the invocation timeout (run until the child exits).
    pub fn no_timeout(mut self) -> Self {
        self.timeout = None;
        self
    }

    /// Inject one environment variable into the spawned subprocess (repeatable).
    /// The value is a secret and is never logged.
    pub fn env(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.envs.push(key, value);
        self
    }

    /// Replace the full set of injected environment variables.
    ///
    /// This **replaces** the set (it does not append, unlike
    /// `std::process::Command::envs`). Use [`env`](Self::env) to add one.
    pub fn envs<I, K, V>(mut self, vars: I) -> Self
    where
        I: IntoIterator<Item = (K, V)>,
        K: Into<String>,
        V: Into<String>,
    {
        self.envs.replace_from(vars);
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
            envs: self.envs.to_vec(),
            cwd: self.cwd.clone(),
            timeout: self.timeout,
        }
    }

    /// Send a chat request by delegating to [`Self::stream`] and collecting
    /// the events with [`crate::stream::collect_stream`].
    ///
    /// Both paths share one `gemini -p "" -o stream-json` spawn/parse
    /// pipeline, so `content`, `thinking`, `tool_calls`, `usage`,
    /// `session_id`, and `stop_reason` are identical by construction. A
    /// successfully completed CLI turn always reports
    /// [`StopReason::EndTurn`]: [`ChatResponse::tool_calls`] records the
    /// tools the CLI already ran — never a request for the caller to
    /// execute them.
    ///
    /// Documented parity exception: `model` is backfilled from the request /
    /// provider configuration because stream events carry no model name.
    pub async fn chat(&self, request: ChatRequest) -> Result<ChatResponse, MotosanError> {
        let configured_model = request.model.clone().or_else(|| self.model.clone());
        let stream = self.stream(request).await?;
        let mut resp = crate::stream::collect_stream(stream).await?;
        if resp.model.is_empty() {
            resp.model = configured_model.unwrap_or_default();
        }
        Ok(resp)
    }

    /// Stream a chat request via `gemini -p "" -o stream-json`.
    ///
    /// Returns a [`BoxStream`] that yields [`StreamEvent`](crate::types::StreamEvent)
    /// items parsed from the NDJSON events Gemini CLI emits on stdout.
    pub async fn stream(&self, request: ChatRequest) -> Result<BoxStream, MotosanError> {
        use tokio::io::{AsyncWriteExt, BufReader};

        let (msg_system, user_prompt) = prompt::messages_to_prompt(&request.messages);
        let system_prompt = request.system.clone().or(msg_system);
        let stdin_payload = merge_system_into_prompt(system_prompt.as_deref(), &user_prompt);

        let config = self.build_spawn_config(request.model);

        let mut cmd = spawn::build_command(&config);

        let mut child = cmd.spawn().map_err(|e| MotosanError::ProviderError {
            message: format!("failed to spawn gemini CLI: {e}"),
            status_code: None,
            retry_after: None,
            request_id: None,
        })?;
        let mut stderr = crate::transport::cli::StderrCapture::start_child(&mut child);

        {
            let mut stdin = child
                .stdin
                .take()
                .ok_or_else(|| MotosanError::ProviderError {
                    message: "failed to open gemini CLI stdin".to_string(),
                    status_code: None,
                    retry_after: None,
                    request_id: None,
                })?;
            crate::transport::cli::poll_with_stderr(
                stdin.write_all(stdin_payload.as_bytes()),
                &mut stderr,
            )
            .await
            .map_err(|e| MotosanError::ProviderError {
                message: format!("failed to write to gemini stdin: {e}"),
                status_code: None,
                retry_after: None,
                request_id: None,
            })?;
        }

        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| MotosanError::ProviderError {
                message: "failed to open gemini CLI stdout".to_string(),
                status_code: None,
                retry_after: None,
                request_id: None,
            })?;

        let reader = BufReader::new(stdout);

        Ok(drive_lines_with_stderr(
            Some(child),
            reader,
            config.timeout,
            stderr,
        ))
    }
}

#[cfg(test)]
pub(crate) fn drive_lines<R>(
    mut child: Option<tokio::process::Child>,
    reader: R,
    read_timeout: Option<Duration>,
) -> BoxStream
where
    R: tokio::io::AsyncBufRead + Unpin + Send + 'static,
{
    let stderr = crate::transport::cli::StderrCapture::start(&mut child);
    drive_lines_with_stderr(child, reader, read_timeout, stderr)
}

fn drive_lines_with_stderr<R>(
    mut child: Option<tokio::process::Child>,
    reader: R,
    read_timeout: Option<Duration>,
    mut stderr: crate::transport::cli::StderrCapture,
) -> BoxStream
where
    R: tokio::io::AsyncBufRead + Unpin + Send + 'static,
{
    use tokio::io::AsyncBufReadExt;

    Box::pin(async_stream::stream! {
        let mut lines = reader.lines();
        loop {
            let next = match read_timeout {
                Some(dur) => match tokio::time::timeout(
                    dur,
                    crate::transport::cli::poll_with_stderr(
                        lines.next_line(),
                        &mut stderr,
                    ),
                ).await {
                    Ok(res) => res,
                    Err(_) => {
                        crate::transport::cli::terminate(&mut child, &mut stderr).await;
                        yield Err(MotosanError::StreamReadTimeout(dur.as_secs()));
                        break;
                    }
                },
                None => crate::transport::cli::poll_with_stderr(
                    lines.next_line(),
                    &mut stderr,
                ).await,
            };

            let line = match next {
                Ok(Some(line)) => line.trim().to_string(),
                // EOF/read error before a terminal event surfaces an abnormal child exit.
                Ok(None) => {
                    yield Err(crate::transport::cli::abnormal_exit_error(
                        &mut child,
                        &mut stderr,
                        CLI_LABEL,
                        None,
                    ).await);
                    break;
                }
                Err(e) => {
                    yield Err(crate::transport::cli::abnormal_exit_error(
                        &mut child,
                        &mut stderr,
                        CLI_LABEL,
                        Some(e),
                    ).await);
                    break;
                }
            };
            if line.is_empty() {
                continue;
            }

            match stream_json::parse_ndjson_line(&line) {
                Some(stream_json::NdjsonAction::Text(event)) => {
                    yield Ok(event);
                }
                Some(stream_json::NdjsonAction::SessionStarted(event)) => {
                    yield Ok(event);
                }
                Some(stream_json::NdjsonAction::ToolCalls(events)) => {
                    for event in events {
                        yield Ok(event);
                    }
                }
                Some(stream_json::NdjsonAction::Result { usage, done }) => {
                    let _ = done;
                    if let Err(err) = crate::transport::cli::validate_terminal_exit(
                        &mut child,
                        &mut stderr,
                        CLI_LABEL,
                    ).await {
                        yield Err(err);
                        break;
                    }
                    if let Some(usage_event) = usage {
                        yield Ok(usage_event);
                    }
                    yield Ok(crate::types::StreamEvent::done_with_stop_reason(StopReason::EndTurn));
                    break;
                }
                Some(stream_json::NdjsonAction::Error(msg)) => {
                    crate::transport::cli::terminate(&mut child, &mut stderr).await;
                    yield Err(MotosanError::ProviderError {
                        message: msg,
                        status_code: None,
                        retry_after: None,
                        request_id: None,
                    });
                    break;
                }
                None => {}
            }
        }
    })
}

const CLI_LABEL: &str = "gemini CLI";

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

    /// F4 parity: one fake-CLI transcript through `chat()` and through
    /// `collect_stream(stream())` must agree on content / thinking /
    /// tool_calls / stop_reason / usage / session_id. `model` is the one
    /// documented exception: `chat()` backfills it from provider config.
    #[cfg(unix)]
    mod chat_stream_parity {
        use super::*;
        use crate::types::StopReason;

        const TRANSCRIPT: &str = concat!(
            r#"{"type":"init","session_id":"sess_9"}"#,
            "\n",
            r#"{"type":"message","role":"assistant","content":"Sure, ","delta":true}"#,
            "\n",
            r#"{"type":"tool_use","tool_id":"read_1","tool_name":"read_file","parameters":{"file_path":"Cargo.toml"}}"#,
            "\n",
            r#"{"type":"message","role":"assistant","content":"done.","delta":true}"#,
            "\n",
            r#"{"type":"result","status":"success","stats":{"input_tokens":9,"output_tokens":4,"cached":1}}"#,
        );

        fn parity_request(prompt: &str) -> ChatRequest {
            ChatRequest {
                messages: vec![Message {
                    role: Role::User,
                    content: prompt.to_string(),
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

        /// Write an executable fake `gemini` that ignores its argv, drains
        /// stdin, and plays back `body` on stdout.
        fn write_fake_cli(test_name: &str, body: &str) -> std::path::PathBuf {
            use std::io::Write;
            use std::os::unix::fs::PermissionsExt;
            let path = std::env::temp_dir().join(format!(
                "motosan-fake-gemini-{test_name}-{}",
                std::process::id()
            ));
            let mut f = std::fs::File::create(&path).expect("create fake CLI");
            write!(
                f,
                "#!/bin/sh\ncat > /dev/null\ncat <<'NDJSON'\n{body}\nNDJSON\n"
            )
            .expect("write fake CLI");
            f.set_permissions(std::fs::Permissions::from_mode(0o755))
                .expect("chmod fake CLI");
            path
        }

        #[tokio::test]
        async fn chat_equals_collected_stream_and_reports_end_turn() {
            let bin = write_fake_cli("parity", TRANSCRIPT);
            let provider = || GeminiCliProvider::with_path(bin.clone()).model("gemini-test");

            let chat_resp = provider()
                .chat(parity_request("hi"))
                .await
                .expect("chat should succeed");
            let stream = provider()
                .stream(parity_request("hi"))
                .await
                .expect("stream should start");
            let collected = crate::stream::collect_stream(stream)
                .await
                .expect("collect should succeed");
            let _ = std::fs::remove_file(&bin);

            // F4: chat()'s tool_calls = the executed-tool record from the CLI.
            assert_eq!(
                chat_resp.tool_calls.len(),
                1,
                "chat() must surface the CLI's executed-tool record"
            );
            assert_eq!(chat_resp.tool_calls[0].id, "read_1");
            assert_eq!(chat_resp.tool_calls[0].name, "read_file");
            assert_eq!(
                chat_resp.tool_calls[0].input,
                serde_json::json!({"file_path": "Cargo.toml"})
            );
            assert_eq!(chat_resp.tool_calls, collected.tool_calls);

            // F4: a completed CLI turn ALWAYS reports end_turn.
            assert_eq!(chat_resp.stop_reason, StopReason::EndTurn);
            assert_eq!(collected.stop_reason, StopReason::EndTurn);

            assert_eq!(chat_resp.content, "Sure, done.");
            assert_eq!(chat_resp.content, collected.content);
            assert_eq!(chat_resp.thinking, None);
            assert_eq!(chat_resp.thinking, collected.thinking);
            assert_eq!(chat_resp.usage.input_tokens, 9);
            assert_eq!(chat_resp.usage.output_tokens, 4);
            assert_eq!(chat_resp.usage.cache_read_input_tokens, Some(1));
            assert_eq!(chat_resp.usage, collected.usage);
            assert_eq!(chat_resp.session_id.as_deref(), Some("sess_9"));
            assert_eq!(chat_resp.session_id, collected.session_id);

            // Documented F4 parity exception: model backfill from config.
            assert_eq!(chat_resp.model, "gemini-test");
            assert_eq!(collected.model, "");
        }

        #[tokio::test]
        async fn chat_times_out_via_stream_read_timeout() {
            let bin = write_fake_cli("stall", "");
            std::fs::write(&bin, "#!/bin/sh\nsleep 30\n").expect("write stall script");
            let provider = GeminiCliProvider::with_path(bin.clone())
                .timeout(std::time::Duration::from_millis(50));
            let result = provider.chat(parity_request("hi")).await;
            let _ = std::fs::remove_file(&bin);
            match result {
                Err(crate::error::MotosanError::StreamReadTimeout(_)) => {}
                other => panic!("expected StreamReadTimeout, got {other:?}"),
            }
        }

        #[tokio::test]
        async fn lifecycle_rejects_nonzero_exit_after_success_terminal() {
            let bin = write_fake_cli("late-exit", "");
            std::fs::write(
                &bin,
                format!(
                    "#!/bin/sh\ncat > /dev/null\ncat <<'NDJSON'\n{TRANSCRIPT}\nNDJSON\n\
                     printf 'late gemini failure\\n' >&2\nsleep 0.2\nexit 7\n"
                ),
            )
            .expect("write late-exit script");
            let result = GeminiCliProvider::with_path(bin.clone())
                .chat(parity_request("hi"))
                .await;
            let _ = std::fs::remove_file(&bin);

            match result {
                Err(crate::error::MotosanError::Stream(message)) => {
                    assert!(message.contains("status"), "got: {message}");
                    assert!(message.contains('7'), "got: {message}");
                    assert!(message.contains("late gemini failure"), "got: {message}");
                }
                other => panic!("expected Stream error for exit 7, got {other:?}"),
            }
        }

        #[tokio::test]
        async fn lifecycle_drains_large_stderr_before_success_terminal() {
            let bin = write_fake_cli("large-stderr", "");
            std::fs::write(
                &bin,
                format!(
                    "#!/bin/sh\ncat > /dev/null\nyes x | head -c 1048576 >&2\n\
                     cat <<'NDJSON'\n{TRANSCRIPT}\nNDJSON\n"
                ),
            )
            .expect("write large-stderr script");
            let result = GeminiCliProvider::with_path(bin.clone())
                .timeout(std::time::Duration::from_secs(10))
                .chat(parity_request("hi"))
                .await;
            let _ = std::fs::remove_file(&bin);

            let response = result.expect("large stderr must not block stdout");
            assert_eq!(response.stop_reason, StopReason::EndTurn);
        }
    }

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
    fn default_timeout_is_set_and_overridable() {
        use std::time::Duration;

        let p = GeminiCliProvider::new();
        assert_eq!(p.timeout, Some(spawn::DEFAULT_TIMEOUT));
        let cfg = p.timeout(Duration::from_secs(5)).build_spawn_config(None);
        assert_eq!(cfg.timeout, Some(Duration::from_secs(5)));
        assert_eq!(
            GeminiCliProvider::new()
                .no_timeout()
                .build_spawn_config(None)
                .timeout,
            None
        );
    }

    #[tokio::test]
    async fn stream_stall_yields_timeout_error() {
        use std::time::Duration;
        use tokio::io::BufReader;
        use tokio_stream::StreamExt;

        let (_w, r) = tokio::io::duplex(64);
        let reader = BufReader::new(r);
        let mut s = super::drive_lines(
            None::<tokio::process::Child>,
            reader,
            Some(Duration::from_millis(50)),
        );
        match s.next().await {
            Some(Err(crate::error::MotosanError::StreamReadTimeout(_))) => {}
            other => panic!("expected StreamReadTimeout, got {other:?}"),
        }
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn premature_child_exit_surfaces_status_and_stderr() {
        use std::process::Stdio;
        use std::time::Duration;
        use tokio::io::BufReader;
        use tokio::process::Command;
        use tokio_stream::StreamExt;

        let mut child = Command::new("sh")
            .arg("-c")
            .arg("printf '%s\n' '{\"type\":\"message\",\"role\":\"assistant\",\"content\":\"hello\",\"delta\":true}'; echo boom >&2; exit 1")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .expect("spawn child");
        let stdout = child.stdout.take().expect("child stdout");
        let mut stream = super::drive_lines(
            Some(child),
            BufReader::new(stdout),
            Some(Duration::from_secs(10)),
        );

        let first = stream
            .next()
            .await
            .expect("first event")
            .expect("text event");
        assert_eq!(first.content, "hello");
        assert!(!first.done);
        match stream.next().await {
            Some(Err(crate::error::MotosanError::Stream(msg))) => {
                assert!(msg.contains("exited unexpectedly"), "got: {msg}");
                assert!(msg.contains("status 1"), "got: {msg}");
                assert!(msg.contains("boom"), "got: {msg}");
            }
            other => panic!("expected Stream error, got {other:?}"),
        }
        assert!(stream.next().await.is_none());
    }

    #[tokio::test]
    async fn stream_surfaces_provider_error_as_err_item() {
        use std::io::Cursor;
        use tokio::io::BufReader;
        use tokio_stream::StreamExt;

        let raw = b"{\"type\":\"message\",\"role\":\"assistant\",\"content\":\"partial\",\"delta\":true}\n{\"type\":\"result\",\"status\":\"failed\"}\n";
        let mut s = super::drive_lines(
            None::<tokio::process::Child>,
            BufReader::new(Cursor::new(&raw[..])),
            None,
        );
        let mut last = None;
        while let Some(item) = s.next().await {
            last = Some(item);
        }
        assert!(
            matches!(
                last,
                Some(Err(crate::error::MotosanError::ProviderError { .. }))
            ),
            "a provider-error line must surface as a terminal Err item, got {last:?}"
        );
    }

    #[test]
    fn env_builder_threads_and_debug_redacts() {
        let p = GeminiCliProvider::new().env("GEMINI_API_KEY", "sk-super-secret");
        assert_eq!(
            p.build_spawn_config(None).envs,
            vec![("GEMINI_API_KEY".to_string(), "sk-super-secret".to_string())]
        );
        let dbg = format!("{p:?}");
        assert!(
            !dbg.contains("sk-super-secret"),
            "Debug leaked secret: {dbg}"
        );
        assert!(dbg.contains("<1 redacted>"), "got: {dbg}");
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
