//! Anthropic Claude Code CLI provider.
//!
//! Shells out to the `claude` binary in headless mode
//! (`claude --print --output-format stream-json`) and parses its NDJSON
//! output into motosan-ai's [`ChatResponse`] / [`BoxStream`] types. Uses
//! the same shape as [`codex_cli`](super::codex_cli) and
//! [`gemini_cli`](super::gemini_cli) so all three CLI backends are
//! interchangeable via `Box<dyn ProviderImpl>`.
//!
//! # Streaming vs Blocking
//!
//! Unlike the HTTP providers (Anthropic, OpenAI, etc.) where `.chat()`
//! and `.stream()` are two views over the same SSE engine, CLI backends
//! spawn the binary in **different modes** for each path:
//!
//! - [`chat`](ClaudeCodeProvider) spawns `claude --print -` (no
//!   `--output-format`), waits for the subprocess to exit, collects the
//!   complete stdout, and returns a single [`ChatResponse`]. No
//!   intermediate events are surfaced. Token usage is `0`/`0` unless
//!   `agent_mode` is enabled (which adds `--output-format json`).
//! - [`stream`](ClaudeCodeProvider) spawns
//!   `claude --print --output-format stream-json --verbose -`, reads
//!   NDJSON lines from stdout as they arrive, and yields one
//!   [`StreamEvent`] per parsed line. The `--verbose` flag is **required**
//!   by `claude` ≥ 2.1.x for this format and is enforced by this crate.
//!
//! Callers should prefer `stream()` for any UI that benefits from
//! incremental output, and `chat()` only when they need the whole reply
//! as a single string.
//!
//! [`StreamEvent`]: crate::StreamEvent

pub mod prompt;
mod spawn;
mod stream_json;

pub use spawn::{EffortLevel, PermissionMode};

use std::env;
use std::path::PathBuf;

use crate::error::MotosanError;
use crate::providers::redacted_envs::RedactedEnvs;
use crate::stream::BoxStream;
use crate::types::{ChatRequest, ChatResponse, StopReason};

/// Client that shells out to the `claude` CLI binary.
#[derive(Debug, Clone)]
pub struct ClaudeCodeProvider {
    /// Path to the `claude` binary. Defaults to `$CLAUDE_CODE_PATH` or
    /// `"claude"` (resolved via `PATH`).
    pub binary_path: PathBuf,
    /// Whether to pass `--dangerously-skip-permissions` and switch the
    /// blocking path to `--output-format json` (so usage can be parsed).
    pub agent_mode: bool,
    /// Whether to pass `--bare`. Skips hooks, plugins, auto-memory,
    /// keychain reads, and user/project settings discovery, so the
    /// subprocess does not inherit ambient Claude Code state from
    /// the operator's shell. Recommended `true` for daemon / server
    /// use; leave `false` for interactive workflows that should pick
    /// up the operator's `~/.claude/` configuration.
    ///
    /// # Interactions with other fields
    ///
    /// - **Auth**: under `--bare`, Anthropic auth is restricted to
    ///   `ANTHROPIC_API_KEY` or an `apiKeyHelper` declared inside
    ///   the file passed to [`settings`](Self::settings). OAuth
    ///   subscription and keychain reads are **never** consulted.
    ///   Callers running headless must export `ANTHROPIC_API_KEY`
    ///   or hand a `--settings` JSON before flipping this on, or
    ///   every request will fail with an auth error.
    /// - **Auto-discovered context is suppressed**: hooks,
    ///   `~/.claude/` user settings, project `.claude/` settings,
    ///   auto-memory, plugin sync, and `CLAUDE.md` auto-discovery
    ///   all stop. [`setting_sources`](Self::setting_sources) becomes
    ///   inert because the discovery step it controls is disabled —
    ///   the CLI accepts the flag without error, the values simply
    ///   have no effect.
    /// - **Explicit context still flows**: values you pass via
    ///   [`settings`](Self::settings), [`mcp_config`](Self::mcp_config),
    ///   [`add_dirs`](Self::add_dirs), [`plugin_dirs`](Self::plugin_dirs),
    ///   [`agent`](Self::agent), [`system_prompt`](Self::system_prompt),
    ///   and the message-extracted `--append-system-prompt` are
    ///   honoured normally — `--bare` only kills the *implicit*
    ///   discovery path, not what you hand the CLI yourself.
    /// - Skills still resolve via `/skill-name` invocation.
    pub bare: bool,
    /// Default model forwarded as `--model <name>`.
    pub model: Option<String>,
    /// Full system-prompt replacement (`--system-prompt`). Coexists
    /// with any message-extracted system prompt, which flows through
    /// `--append-system-prompt`.
    pub system_prompt: Option<String>,
    /// Optional `--permission-mode` override.
    pub permission_mode: Option<PermissionMode>,
    /// Optional `--effort` level.
    pub effort: Option<EffortLevel>,
    /// Optional `--fallback-model`.
    pub fallback_model: Option<String>,
    /// Extra workspace roots, forwarded as repeated `--add-dir`.
    pub add_dirs: Vec<PathBuf>,
    /// Tool allowlist, forwarded as `--allowed-tools a b c`.
    pub allowed_tools: Vec<String>,
    /// Tool denylist, forwarded as `--disallowed-tools a b c`.
    pub disallowed_tools: Vec<String>,
    /// MCP config file paths or inline JSON strings (`--mcp-config`).
    pub mcp_config: Vec<String>,
    /// Whether to pass `--strict-mcp-config`.
    pub strict_mcp_config: bool,
    /// `--settings <file-or-json>`.
    pub settings: Option<String>,
    /// `--setting-sources` values (joined with commas at argv time).
    pub setting_sources: Vec<String>,
    /// `--session-id <uuid>`.
    pub session_id: Option<String>,
    /// `-r, --resume <value>`.
    pub resume: Option<String>,
    /// `-c, --continue`.
    pub continue_latest: bool,
    /// `--fork-session`.
    pub fork_session: bool,
    /// Additional plugin directories (`--plugin-dir` repeated).
    pub plugin_dirs: Vec<PathBuf>,
    /// `--agent <name>`.
    pub agent: Option<String>,
    /// `--no-session-persistence`.
    pub no_session_persistence: bool,
    /// `--max-budget-usd <amount>`.
    pub max_budget_usd: Option<f64>,
    /// Extra environment variables injected into the spawned `claude` child,
    /// in insertion order. Use for a per-run secret bundle (e.g. ANTHROPIC_API_KEY)
    /// without mutating the parent environment. Values are secrets (redacted in Debug).
    pub envs: RedactedEnvs,
    /// Working directory for the spawned `claude` process. When set, the child
    /// runs with this cwd (`Command::current_dir`) instead of inheriting the
    /// parent's. The §6.2 `CliRuntime` cwd contract requires this.
    pub cwd: Option<PathBuf>,
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
            bare: false,
            model: None,
            system_prompt: None,
            permission_mode: None,
            effort: None,
            fallback_model: None,
            add_dirs: Vec::new(),
            allowed_tools: Vec::new(),
            disallowed_tools: Vec::new(),
            mcp_config: Vec::new(),
            strict_mcp_config: false,
            settings: None,
            setting_sources: Vec::new(),
            session_id: None,
            resume: None,
            continue_latest: false,
            fork_session: false,
            plugin_dirs: Vec::new(),
            agent: None,
            no_session_persistence: false,
            max_budget_usd: None,
            envs: RedactedEnvs::default(),
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

    /// Enable or disable agent mode (`--dangerously-skip-permissions`).
    pub fn agent_mode(mut self, enabled: bool) -> Self {
        self.agent_mode = enabled;
        self
    }

    /// Enable or disable `--bare`. When `true`, the spawned `claude`
    /// subprocess skips hooks, plugins, auto-memory, keychain reads,
    /// and user/project settings discovery — useful for daemons that
    /// must not inherit the operator's interactive Claude Code state.
    ///
    /// **Read [`Self::bare`] before flipping this on** — `--bare`
    /// restricts Anthropic auth to `ANTHROPIC_API_KEY` /
    /// `apiKeyHelper` (no OAuth, no keychain) and silently makes
    /// [`Self::setting_sources`] inert. Explicit `settings`,
    /// `mcp_config`, `add_dirs`, `plugin_dirs`, `agent`, and
    /// `system_prompt` still work as expected.
    pub fn bare(mut self, enabled: bool) -> Self {
        self.bare = enabled;
        self
    }

    /// Set the default model.
    pub fn model(mut self, model: impl Into<String>) -> Self {
        self.model = Some(model.into());
        self
    }

    /// Set the full system-prompt replacement (`--system-prompt`).
    pub fn system_prompt(mut self, prompt: impl Into<String>) -> Self {
        self.system_prompt = Some(prompt.into());
        self
    }

    /// Set `--permission-mode`.
    pub fn permission_mode(mut self, mode: PermissionMode) -> Self {
        self.permission_mode = Some(mode);
        self
    }

    /// Set `--effort`.
    pub fn effort(mut self, level: EffortLevel) -> Self {
        self.effort = Some(level);
        self
    }

    /// Set `--fallback-model`.
    pub fn fallback_model(mut self, model: impl Into<String>) -> Self {
        self.fallback_model = Some(model.into());
        self
    }

    /// Append a single directory to `--add-dir`.
    pub fn add_dir(mut self, dir: impl Into<PathBuf>) -> Self {
        self.add_dirs.push(dir.into());
        self
    }

    /// Replace the full list of `--add-dir` entries.
    pub fn add_dirs(mut self, dirs: Vec<PathBuf>) -> Self {
        self.add_dirs = dirs;
        self
    }

    /// Append a single entry to `--allowed-tools`.
    pub fn allow_tool(mut self, name: impl Into<String>) -> Self {
        self.allowed_tools.push(name.into());
        self
    }

    /// Replace the full `--allowed-tools` list.
    pub fn allowed_tools(mut self, tools: Vec<String>) -> Self {
        self.allowed_tools = tools;
        self
    }

    /// Append a single entry to `--disallowed-tools`.
    pub fn disallow_tool(mut self, name: impl Into<String>) -> Self {
        self.disallowed_tools.push(name.into());
        self
    }

    /// Replace the full `--disallowed-tools` list.
    pub fn disallowed_tools(mut self, tools: Vec<String>) -> Self {
        self.disallowed_tools = tools;
        self
    }

    /// Append a single `--mcp-config` entry (path or inline JSON).
    pub fn mcp_config(mut self, config: impl Into<String>) -> Self {
        self.mcp_config.push(config.into());
        self
    }

    /// Replace the full `--mcp-config` list.
    pub fn mcp_configs(mut self, configs: Vec<String>) -> Self {
        self.mcp_config = configs;
        self
    }

    /// Toggle `--strict-mcp-config`.
    pub fn strict_mcp_config(mut self, enabled: bool) -> Self {
        self.strict_mcp_config = enabled;
        self
    }

    /// Set `--settings <file-or-json>`.
    pub fn settings(mut self, settings: impl Into<String>) -> Self {
        self.settings = Some(settings.into());
        self
    }

    /// Append a single `--setting-sources` entry (`user` / `project` / `local`).
    pub fn setting_source(mut self, source: impl Into<String>) -> Self {
        self.setting_sources.push(source.into());
        self
    }

    /// Replace the full `--setting-sources` list.
    pub fn setting_sources(mut self, sources: Vec<String>) -> Self {
        self.setting_sources = sources;
        self
    }

    /// Set `--session-id <uuid>`.
    pub fn session_id(mut self, uuid: impl Into<String>) -> Self {
        self.session_id = Some(uuid.into());
        self
    }

    /// Set `--resume <value>` (`"latest"` or a specific session ID).
    pub fn resume(mut self, value: impl Into<String>) -> Self {
        self.resume = Some(value.into());
        self
    }

    /// Toggle `--continue`.
    pub fn continue_latest(mut self, enabled: bool) -> Self {
        self.continue_latest = enabled;
        self
    }

    /// Toggle `--fork-session`.
    pub fn fork_session(mut self, enabled: bool) -> Self {
        self.fork_session = enabled;
        self
    }

    /// Append a single `--plugin-dir`.
    pub fn plugin_dir(mut self, path: impl Into<PathBuf>) -> Self {
        self.plugin_dirs.push(path.into());
        self
    }

    /// Replace the full `--plugin-dir` list.
    pub fn plugin_dirs(mut self, paths: Vec<PathBuf>) -> Self {
        self.plugin_dirs = paths;
        self
    }

    /// Set `--agent <name>`.
    pub fn agent(mut self, name: impl Into<String>) -> Self {
        self.agent = Some(name.into());
        self
    }

    /// Toggle `--no-session-persistence`.
    pub fn no_session_persistence(mut self, enabled: bool) -> Self {
        self.no_session_persistence = enabled;
        self
    }

    /// Set `--max-budget-usd <amount>`. Non-finite or negative values
    /// are dropped at argv-build time.
    pub fn max_budget_usd(mut self, amount: f64) -> Self {
        self.max_budget_usd = Some(amount);
        self
    }

    /// Set the working directory for the spawned process (`Command::current_dir`).
    pub fn cwd(mut self, dir: impl Into<PathBuf>) -> Self {
        self.cwd = Some(dir.into());
        self
    }

    /// Inject one environment variable into the spawned subprocess (repeatable).
    /// The value is a secret and is never logged.
    pub fn env(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.envs.push(key, value);
        self
    }

    /// Replace the full set of injected environment variables.
    pub fn envs<I, K, V>(mut self, vars: I) -> Self
    where
        I: IntoIterator<Item = (K, V)>,
        K: Into<String>,
        V: Into<String>,
    {
        self.envs.replace_from(vars);
        self
    }

    fn build_spawn_config(
        &self,
        request_model: Option<String>,
        append_system_prompt: Option<String>,
    ) -> spawn::SpawnConfig {
        spawn::SpawnConfig {
            binary_path: self.binary_path.clone(),
            agent_mode: self.agent_mode,
            bare: self.bare,
            model: request_model.or_else(|| self.model.clone()),
            append_system_prompt,
            system_prompt: self.system_prompt.clone(),
            permission_mode: self.permission_mode,
            effort: self.effort,
            fallback_model: self.fallback_model.clone(),
            add_dirs: self.add_dirs.clone(),
            allowed_tools: self.allowed_tools.clone(),
            disallowed_tools: self.disallowed_tools.clone(),
            mcp_config: self.mcp_config.clone(),
            strict_mcp_config: self.strict_mcp_config,
            settings: self.settings.clone(),
            setting_sources: self.setting_sources.clone(),
            session_id: self.session_id.clone(),
            resume: self.resume.clone(),
            continue_latest: self.continue_latest,
            fork_session: self.fork_session,
            plugin_dirs: self.plugin_dirs.clone(),
            agent: self.agent.clone(),
            no_session_persistence: self.no_session_persistence,
            max_budget_usd: self.max_budget_usd,
            envs: self.envs.to_vec(),
            cwd: self.cwd.clone(),
        }
    }

    /// Send a chat request by invoking the `claude` CLI as a subprocess.
    pub async fn chat(&self, request: ChatRequest) -> Result<ChatResponse, MotosanError> {
        // Extract system prompt: prefer request.system, fall back to first system message.
        let (msg_system, user_prompt) = prompt::messages_to_prompt(&request.messages);
        let append_system_prompt = request.system.or(msg_system);

        let config = self.build_spawn_config(request.model, append_system_prompt);

        let (text, usage, session_id) = spawn::invoke_cli(&config, &user_prompt).await?;

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

    /// Stream a chat request via `claude --print --output-format stream-json`.
    ///
    /// Returns a `BoxStream` that yields `StreamEvent` items parsed from
    /// newline-delimited JSON events emitted by the CLI.
    pub async fn stream(&self, request: ChatRequest) -> Result<BoxStream, MotosanError> {
        use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
        use tokio::process::Command;

        let (msg_system, user_prompt) = prompt::messages_to_prompt(&request.messages);
        let append_system_prompt = request.system.or(msg_system);

        let config = self.build_spawn_config(request.model, append_system_prompt);

        let mut cmd = Command::new(&config.binary_path);
        if let Some(dir) = &config.cwd {
            cmd.current_dir(dir);
        }
        cmd.envs(config.envs.iter().map(|(k, v)| (k, v)));
        // `--verbose` is required by `claude` ≥ 2.1.x when combining `--print`
        // with `--output-format=stream-json`. Without it the CLI exits with
        // "Error: When using --print, --output-format=stream-json requires
        // --verbose" and emits zero NDJSON — our stream then yields no events
        // and capo's downstream consumer sees nothing.
        cmd.arg("--print")
            .arg("--output-format")
            .arg("stream-json")
            .arg("--verbose");
        cmd.args(spawn::common_args(&config));
        cmd.arg("-");

        cmd.kill_on_drop(true);
        cmd.stdin(std::process::Stdio::piped());
        cmd.stdout(std::process::Stdio::piped());
        cmd.stderr(std::process::Stdio::piped());

        let mut child = cmd
            .spawn()
            .map_err(|e| MotosanError::ProviderError(format!("failed to spawn claude CLI: {e}")))?;

        // Write prompt to stdin then close it.
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
                            yield Ok(event);
                        }
                        stream_json::NdjsonAction::Result { usage, done, session_id } => {
                            let _ = done;
                            if let Some(id) = session_id {
                                yield Ok(crate::types::StreamEvent::session_started(id));
                            }
                            if let Some(usage_event) = usage {
                                yield Ok(usage_event);
                            }
                            yield Ok(crate::types::StreamEvent::done_with_stop_reason(StopReason::EndTurn));
                            break;
                        }
                        stream_json::NdjsonAction::Error(msg) => {
                            yield Err(MotosanError::ProviderError(msg));
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

    #[test]
    fn cwd_builder_threads_into_spawn_config() {
        let provider = ClaudeCodeProvider::new().cwd("/work/dir");
        let cfg = provider.build_spawn_config(None, None);
        assert_eq!(cfg.cwd.as_deref(), Some(std::path::Path::new("/work/dir")));
    }

    #[test]
    fn env_builder_threads_and_debug_redacts() {
        let p = ClaudeCodeProvider::new().env("ANTHROPIC_API_KEY", "sk-super-secret");
        assert_eq!(
            p.build_spawn_config(None, None).envs,
            vec![(
                "ANTHROPIC_API_KEY".to_string(),
                "sk-super-secret".to_string()
            )]
        );
        let dbg = format!("{p:?}");
        assert!(
            !dbg.contains("sk-super-secret"),
            "Debug leaked secret: {dbg}"
        );
        assert!(dbg.contains("<1 redacted>"), "got: {dbg}");
    }

    #[test]
    fn builder_methods_populate_spawn_config() {
        let provider = ClaudeCodeProvider::new()
            .agent_mode(true)
            .bare(true)
            .model("sonnet")
            .system_prompt("be terse")
            .permission_mode(PermissionMode::Plan)
            .effort(EffortLevel::High)
            .fallback_model("opus")
            .add_dir("/tmp/a")
            .add_dir("/tmp/b")
            .allow_tool("Edit")
            .disallow_tool("WebFetch")
            .mcp_config("mcp.json")
            .strict_mcp_config(true)
            .settings("settings.json")
            .setting_source("user")
            .session_id("sid-123")
            .resume("latest")
            .continue_latest(true)
            .fork_session(true)
            .plugin_dir("/plug")
            .agent("reviewer")
            .no_session_persistence(true)
            .max_budget_usd(3.0);

        let cfg = provider.build_spawn_config(None, Some("append".to_string()));
        assert!(cfg.agent_mode);
        assert!(cfg.bare);
        assert_eq!(cfg.model.as_deref(), Some("sonnet"));
        assert_eq!(cfg.system_prompt.as_deref(), Some("be terse"));
        assert_eq!(cfg.append_system_prompt.as_deref(), Some("append"));
        assert_eq!(cfg.permission_mode, Some(PermissionMode::Plan));
        assert_eq!(cfg.effort, Some(EffortLevel::High));
        assert_eq!(cfg.fallback_model.as_deref(), Some("opus"));
        assert_eq!(
            cfg.add_dirs,
            vec![PathBuf::from("/tmp/a"), PathBuf::from("/tmp/b")]
        );
        assert_eq!(cfg.allowed_tools, vec!["Edit".to_string()]);
        assert_eq!(cfg.disallowed_tools, vec!["WebFetch".to_string()]);
        assert_eq!(cfg.mcp_config, vec!["mcp.json".to_string()]);
        assert!(cfg.strict_mcp_config);
        assert_eq!(cfg.settings.as_deref(), Some("settings.json"));
        assert_eq!(cfg.setting_sources, vec!["user".to_string()]);
        assert_eq!(cfg.session_id.as_deref(), Some("sid-123"));
        assert_eq!(cfg.resume.as_deref(), Some("latest"));
        assert!(cfg.continue_latest);
        assert!(cfg.fork_session);
        assert_eq!(cfg.plugin_dirs, vec![PathBuf::from("/plug")]);
        assert_eq!(cfg.agent.as_deref(), Some("reviewer"));
        assert!(cfg.no_session_persistence);
        assert_eq!(cfg.max_budget_usd, Some(3.0));
    }

    /// Build a minimal user-only ChatRequest for integration tests.
    fn test_request(prompt: &str) -> ChatRequest {
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

    /// End-to-end verification that `ClaudeCodeProvider::system_prompt(...)`
    /// actually reaches Claude Code and shapes the reply. Uses a distinctive
    /// system prompt (one emoji only) so a pass/fail is unambiguous even
    /// against model non-determinism.
    #[tokio::test]
    #[ignore]
    async fn integration_system_prompt_replacement() {
        let client = ClaudeCodeProvider::new()
            .system_prompt("Always reply with exactly one emoji, nothing else.");

        let resp = client
            .chat(test_request("Greet me."))
            .await
            .expect("chat should succeed");
        let content = resp.content.trim();
        assert!(
            content.chars().count() <= 4,
            "expected an emoji-length reply, got: {content:?}"
        );
        assert!(
            content.chars().any(|c| c as u32 > 0x2000),
            "expected a non-ASCII character (emoji), got: {content:?}"
        );
    }

    /// End-to-end verification that `.bare(true)` is accepted by the
    /// installed `claude` CLI and a plain `--print` turn still completes.
    /// Catches a future CLI rename of the flag. Requires `ANTHROPIC_API_KEY`
    /// in the environment because `--bare` disables OAuth / keychain auth.
    #[tokio::test]
    #[ignore]
    async fn integration_bare_flag_accepted_by_cli() {
        let client = ClaudeCodeProvider::new().bare(true);

        let resp = client
            .chat(test_request("Reply with only the word 'pong'."))
            .await
            .expect("chat should succeed under --bare");
        assert!(
            resp.content.to_lowercase().contains("pong"),
            "expected 'pong' in response, got: {}",
            resp.content
        );
    }

    /// End-to-end verification that `.permission_mode(Plan)`, `.effort(Low)`,
    /// and `.model("sonnet")` are simultaneously accepted by Claude Code
    /// in `--print` mode. A plain Q&A should still complete under plan mode
    /// because no tools are invoked.
    #[tokio::test]
    #[ignore]
    async fn integration_permission_effort_and_model_combo() {
        let client = ClaudeCodeProvider::new()
            .model("sonnet")
            .permission_mode(PermissionMode::Plan)
            .effort(EffortLevel::Low);

        let resp = client
            .chat(test_request("What is 2+2? Reply with just the number."))
            .await
            .expect("chat should succeed");
        assert!(
            resp.content.contains('4'),
            "expected '4' in response, got: {}",
            resp.content
        );
    }

    /// End-to-end verification that `.add_dir()`, `.no_session_persistence()`,
    /// and `.max_budget_usd()` all survive argv construction and are accepted
    /// by the CLI alongside a normal `--print` turn.
    #[tokio::test]
    #[ignore]
    async fn integration_workspace_and_budget_flags() {
        let tmp = std::env::temp_dir().join("motosan-claude-addtest");
        std::fs::create_dir_all(&tmp).ok();
        let client = ClaudeCodeProvider::new()
            .add_dir(tmp)
            .no_session_persistence(true)
            .max_budget_usd(2.5);

        let resp = client
            .chat(test_request("Reply with only the word 'pong'."))
            .await
            .expect("chat should succeed");
        assert!(
            resp.content.to_lowercase().contains("pong"),
            "expected 'pong' in response, got: {}",
            resp.content
        );
    }

    /// End-to-end verification that `.allow_tool()` / `.disallow_tool()`
    /// variadic forwarding produces a command line Claude accepts.
    #[tokio::test]
    #[ignore]
    async fn integration_tool_allow_deny_flags() {
        let client = ClaudeCodeProvider::new()
            .allow_tool("Edit")
            .allow_tool("Read")
            .disallow_tool("WebFetch");

        let resp = client
            .chat(test_request("Reply with only the word 'pong'."))
            .await
            .expect("chat should succeed");
        assert!(
            resp.content.to_lowercase().contains("pong"),
            "expected 'pong' in response, got: {}",
            resp.content
        );
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
