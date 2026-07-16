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
//! kills and reaps the child process. Stream drivers own and reap the child at the tail;
//! use [`GeminiCliProvider::timeout`] to bound runtime. A stalled stream read
//! yields `Err(MotosanError::StreamReadTimeout(_))` and terminates the stream.

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
        use tokio::io::{AsyncWriteExt, BufReader};
        use tokio::process::Command;

        let (msg_system, user_prompt) = prompt::messages_to_prompt(&request.messages);
        let system_prompt = request.system.clone().or(msg_system);
        let stdin_payload = merge_system_into_prompt(system_prompt.as_deref(), &user_prompt);

        let config = self.build_spawn_config(request.model);

        let mut cmd = Command::new(&config.binary_path);
        if let Some(dir) = &config.cwd {
            cmd.current_dir(dir);
        }
        cmd.envs(config.envs.iter().map(|(k, v)| (k, v)));
        cmd.args(spawn::common_args(&config));
        cmd.kill_on_drop(true);
        cmd.stdin(std::process::Stdio::piped());
        cmd.stdout(std::process::Stdio::piped());
        cmd.stderr(std::process::Stdio::piped());

        let mut child = cmd.spawn().map_err(|e| MotosanError::ProviderError {
            message: format!("failed to spawn gemini CLI: {e}"),
            status_code: None,
            retry_after: None,
            request_id: None,
        })?;

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
            stdin
                .write_all(stdin_payload.as_bytes())
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

        Ok(drive_lines(Some(child), reader, config.timeout))
    }
}

pub(crate) fn drive_lines<R>(
    mut child: Option<tokio::process::Child>,
    reader: R,
    read_timeout: Option<Duration>,
) -> BoxStream
where
    R: tokio::io::AsyncBufRead + Unpin + Send + 'static,
{
    use tokio::io::AsyncBufReadExt;

    Box::pin(async_stream::stream! {
        let mut lines = reader.lines();
        let mut saw_tool_call = false;

        loop {
            let next = match read_timeout {
                Some(dur) => match tokio::time::timeout(dur, lines.next_line()).await {
                    Ok(res) => res,
                    Err(_) => {
                        reap_child(&mut child, true).await;
                        yield Err(MotosanError::StreamReadTimeout(dur.as_secs()));
                        break;
                    }
                },
                None => lines.next_line().await,
            };

            let line = match next {
                Ok(Some(line)) => line.trim().to_string(),
                // EOF/read error before a terminal event surfaces an abnormal child exit.
                Ok(None) => {
                    yield Err(abnormal_exit_error(&mut child, None).await);
                    break;
                }
                Err(e) => {
                    yield Err(abnormal_exit_error(&mut child, Some(e)).await);
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
                    saw_tool_call = true;
                    for event in events {
                        yield Ok(event);
                    }
                }
                Some(stream_json::NdjsonAction::Result { usage, done }) => {
                    let _ = done;
                    // Reaped at the loop tail AFTER the terminal yields below.
                    if let Some(usage_event) = usage {
                        yield Ok(usage_event);
                    }
                    yield Ok(crate::types::StreamEvent::done_with_stop_reason(super::cli_terminal_stop_reason(saw_tool_call)));
                    break;
                }
                Some(stream_json::NdjsonAction::Error(msg)) => {
                    reap_child(&mut child, true).await;
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

        reap_child(&mut child, false).await;
    })
}

async fn reap_child(child: &mut Option<tokio::process::Child>, kill: bool) {
    if let Some(mut c) = child.take() {
        if kill {
            // SIGKILL unblocks the child immediately, so wait() returns promptly.
            let _ = c.start_kill();
            let _ = c.wait().await;
        } else {
            // Cooperative reap (success/EOF path) — but never hang the stream: if
            // the child does not exit promptly (e.g. blocked on an undrained stderr
            // pipe), SIGKILL it so the stream can always terminate.
            if tokio::time::timeout(Duration::from_secs(5), c.wait())
                .await
                .is_err()
            {
                let _ = c.start_kill();
                let _ = c.wait().await;
            }
        }
    }
}

const CLI_LABEL: &str = "gemini CLI";

async fn abnormal_exit_error(
    child: &mut Option<tokio::process::Child>,
    read_err: Option<std::io::Error>,
) -> MotosanError {
    use std::future::{poll_fn, Future};
    use std::pin::Pin;
    use std::task::Poll;
    use tokio::io::{AsyncRead, ReadBuf};

    let mut status = "unknown".to_string();
    let mut stderr_excerpt = String::new();

    if let Some(mut c) = child.take() {
        let mut stderr = c.stderr.take();
        let mut stderr_buf = Vec::with_capacity(8000);
        let mut read_buf = [0_u8; 8192];
        let mut child_done = false;
        let mut stderr_done = stderr.is_none();
        let mut wait_failed = false;

        let collection_timed_out = {
            let wait = c.wait();
            tokio::pin!(wait);
            let collect = poll_fn(|cx| {
                if !child_done {
                    match wait.as_mut().poll(cx) {
                        Poll::Ready(Ok(exit)) => {
                            child_done = true;
                            status = exit
                                .code()
                                .map_or_else(|| exit.to_string(), |code| code.to_string());
                        }
                        Poll::Ready(Err(_)) => {
                            wait_failed = true;
                            return Poll::Ready(());
                        }
                        Poll::Pending => {}
                    }
                }

                if !stderr_done {
                    if let Some(pipe) = stderr.as_mut() {
                        let mut reads = 0;
                        loop {
                            let read = {
                                let mut buf = ReadBuf::new(&mut read_buf);
                                match Pin::new(&mut *pipe).poll_read(cx, &mut buf) {
                                    Poll::Ready(Ok(())) => Poll::Ready(Ok(buf.filled().len())),
                                    Poll::Ready(Err(err)) => Poll::Ready(Err(err)),
                                    Poll::Pending => Poll::Pending,
                                }
                            };
                            match read {
                                Poll::Ready(Ok(0)) | Poll::Ready(Err(_)) => {
                                    stderr_done = true;
                                    break;
                                }
                                Poll::Ready(Ok(n)) => {
                                    let retained = (8000 - stderr_buf.len()).min(n);
                                    stderr_buf.extend_from_slice(&read_buf[..retained]);
                                    reads += 1;
                                    if reads == 16 {
                                        cx.waker().wake_by_ref();
                                        break;
                                    }
                                }
                                Poll::Pending => break,
                            }
                        }
                    }
                }

                if child_done && stderr_done {
                    Poll::Ready(())
                } else {
                    Poll::Pending
                }
            });

            tokio::time::timeout(Duration::from_secs(5), collect)
                .await
                .is_err()
        };

        if wait_failed || (collection_timed_out && !child_done) {
            let _ = c.start_kill();
            if let Ok(exit) = c.wait().await {
                status = exit
                    .code()
                    .map_or_else(|| exit.to_string(), |code| code.to_string());
            }
        }

        stderr_excerpt = String::from_utf8_lossy(&stderr_buf)
            .trim()
            .chars()
            .take(2000)
            .collect();
    }

    let detail = if !stderr_excerpt.is_empty() {
        stderr_excerpt
    } else if let Some(err) = read_err {
        format!("stdout read error: {err}")
    } else {
        "stream ended before a terminal event".to_string()
    };

    MotosanError::Stream(format!(
        "{CLI_LABEL} exited unexpectedly (status {status}): {detail}"
    ))
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
