//! Subprocess lifecycle and flag wiring for the Gemini CLI provider.
//!
//! Argv layout produced by [`common_args`]:
//!
//! ```text
//! gemini -p "" -o stream-json [-m <model>] [--yolo] [--sandbox] [--approval-mode <mode>]
//! ```
//!
//! The `-p ""` sentinel enables headless mode without burning the full
//! prompt on argv — the real prompt is fed via stdin, which Gemini CLI
//! appends to the `-p` value per its `--help` documentation. This keeps
//! us symmetric with the Claude Code / Codex CLI providers (which also
//! use stdin) and avoids argv quoting / `ARG_MAX` footguns.

use std::ffi::OsString;
use std::path::PathBuf;
use std::time::Duration;

use tokio::io::AsyncWriteExt;
use tokio::process::Command;

use crate::error::MotosanError;
use crate::types::Usage;

use super::stream_json::{self, NdjsonAction};

/// Approval mode forwarded as `--approval-mode <mode>`.
///
/// Mirrors Gemini CLI's own `--approval-mode` choices. Picking anything
/// other than [`ApprovalMode::Default`] lets the model execute tools
/// without per-call confirmation, so prefer the least-privileged option
/// that still lets the task make progress.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApprovalMode {
    /// Prompt for approval on every tool call. Rarely useful in
    /// headless mode, included for completeness.
    Default,
    /// Auto-approve edit tools only.
    AutoEdit,
    /// YOLO — auto-approve everything. Equivalent to `--yolo`.
    Yolo,
    /// Read-only plan mode.
    Plan,
}

impl ApprovalMode {
    /// The exact string Gemini CLI expects for `--approval-mode`.
    pub(crate) fn as_flag(&self) -> &'static str {
        match self {
            ApprovalMode::Default => "default",
            ApprovalMode::AutoEdit => "auto_edit",
            ApprovalMode::Yolo => "yolo",
            ApprovalMode::Plan => "plan",
        }
    }
}

/// Configuration snapshot used to spawn a single `gemini` CLI invocation.
///
/// Built by [`GeminiCliProvider::build_spawn_config`](super::GeminiCliProvider)
/// from client state plus any per-request overrides.
pub struct SpawnConfig {
    /// Filesystem path to the `gemini` binary.
    pub binary_path: PathBuf,
    /// Model name for `-m`. `None` / `"default"` / blank are skipped.
    pub model: Option<String>,
    /// Whether to pass `--yolo`.
    pub yolo: bool,
    /// Whether to pass `--sandbox` / `-s`.
    pub sandbox: bool,
    /// Optional `--approval-mode` override. Mutually layered with
    /// `yolo` — if both are set, both are forwarded and Gemini's own
    /// precedence rules win.
    pub approval_mode: Option<ApprovalMode>,
    /// Additional workspace roots, forwarded as repeated
    /// `--include-directories <DIR>` flags. Gemini CLI also accepts
    /// comma-separated values, but repeating the flag is safer for
    /// paths that contain commas.
    pub include_dirs: Vec<PathBuf>,
    /// Extensions to load, forwarded as repeated `-e <NAME>` flags.
    /// An empty list means "let the CLI load all available extensions"
    /// (Gemini's default when `-e` is omitted).
    pub extensions: Vec<String>,
    /// Allowlist of MCP server names, forwarded as repeated
    /// `--allowed-mcp-server-names <NAME>` flags. An empty list means
    /// "no explicit allowlist" — the CLI's own policy applies.
    pub allowed_mcp_servers: Vec<String>,
    /// Session to resume, forwarded as `--resume <value>`. Accepts
    /// `"latest"` or a numeric index matching `gemini --list-sessions`.
    /// Blank strings are skipped.
    pub resume: Option<String>,
    /// Extra environment variables for the spawned subprocess, applied via
    /// `Command::envs`. Carries per-run secrets; do not log values.
    pub envs: Vec<(String, String)>,
    /// Working directory for the spawned process; `None` inherits the parent's.
    pub cwd: Option<PathBuf>,
    /// Timeout for this invocation; `None` disables the timeout wrapper.
    pub timeout: Option<Duration>,
}

/// Default timeout for a single `gemini -p` invocation.
///
/// Matches the Codex CLI provider's 10-minute ceiling. Long turns with
/// tool calls can legitimately take minutes; this is a runaway-process
/// guard, not a latency budget.
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(600);

/// Normalize a user-supplied model string for `-m <value>`.
///
/// Returns the trimmed value or `None` if the caller effectively meant
/// "don't override": empty strings, whitespace-only strings, and the
/// case-insensitive sentinel `"default"`.
pub(crate) fn model_to_forward(model: &str) -> Option<&str> {
    let trimmed = model.trim();
    if trimmed.is_empty() || trimmed.eq_ignore_ascii_case("default") {
        None
    } else {
        Some(trimmed)
    }
}

/// Build the argv fragment for both blocking and streaming paths.
///
/// The `-p ""` / `-o stream-json` prefix is always emitted so a
/// [`SpawnConfig::default_like`] (empty-everything) still produces a
/// runnable headless command.
///
/// Argument order is stable and locked by the `common_args_*` tests.
pub(crate) fn common_args(config: &SpawnConfig) -> Vec<OsString> {
    // Headless mode prefix is unconditional: empty `-p` value so the
    // real prompt flows via stdin, and stream-json output so both the
    // blocking and streaming paths share one parser.
    let mut args: Vec<OsString> = vec!["-p".into(), "".into(), "-o".into(), "stream-json".into()];

    if let Some(ref model) = config.model {
        if let Some(m) = model_to_forward(model) {
            args.push("-m".into());
            args.push(m.into());
        }
    }

    if config.yolo {
        args.push("--yolo".into());
    }

    if config.sandbox {
        args.push("--sandbox".into());
    }

    if let Some(mode) = config.approval_mode {
        args.push("--approval-mode".into());
        args.push(mode.as_flag().into());
    }

    for dir in &config.include_dirs {
        args.push("--include-directories".into());
        args.push(dir.into());
    }

    for ext in &config.extensions {
        if !ext.trim().is_empty() {
            args.push("-e".into());
            args.push(ext.into());
        }
    }

    for name in &config.allowed_mcp_servers {
        if !name.trim().is_empty() {
            args.push("--allowed-mcp-server-names".into());
            args.push(name.into());
        }
    }

    if let Some(ref session) = config.resume {
        let trimmed = session.trim();
        if !trimmed.is_empty() {
            args.push("--resume".into());
            args.push(trimmed.into());
        }
    }

    args
}

/// Assemble a ready-to-spawn [`Command`] for a blocking `gemini` call.
fn build_command(config: &SpawnConfig) -> Command {
    let mut cmd = Command::new(&config.binary_path);
    if let Some(dir) = &config.cwd {
        cmd.current_dir(dir);
    }
    cmd.envs(config.envs.iter().map(|(k, v)| (k, v)));
    cmd.args(common_args(config));

    cmd.kill_on_drop(true);
    cmd.stdin(std::process::Stdio::piped());
    cmd.stdout(std::process::Stdio::piped());
    cmd.stderr(std::process::Stdio::piped());
    cmd
}

/// Spawn `gemini`, feed it `prompt` on stdin, and collect the turn.
///
/// Returns `(content, usage, session_id)` where `content` is the concatenation of
/// every `assistant` delta chunk and `usage` is the token tally from
/// the terminal `result` event. Gemini CLI does not expose a separate
/// reasoning stream in headless output, so there is no `thinking`
/// field to populate.
///
/// # Errors
///
/// Returns [`MotosanError::ProviderError`] if the subprocess fails to
/// spawn, times out after [`DEFAULT_TIMEOUT`], exits non-zero, or emits a
/// `result` event with a non-`success` status.
pub async fn invoke_cli(
    config: &SpawnConfig,
    prompt: &str,
) -> Result<(String, Usage, Option<String>), MotosanError> {
    let mut cmd = build_command(config);

    let mut child = cmd
        .spawn()
        .map_err(|e| MotosanError::ProviderError(format!("failed to spawn gemini CLI: {e}")))?;

    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(prompt.as_bytes()).await.map_err(|e| {
            MotosanError::ProviderError(format!("failed to write to gemini stdin: {e}"))
        })?;
    }

    let result = match config.timeout {
        Some(dur) => tokio::time::timeout(dur, child.wait_with_output())
            .await
            .map_err(|_| {
                MotosanError::ProviderError(format!(
                    "gemini CLI timed out after {} seconds",
                    dur.as_secs()
                ))
            })?
            .map_err(|e| MotosanError::ProviderError(format!("gemini CLI process error: {e}")))?,
        None => child
            .wait_with_output()
            .await
            .map_err(|e| MotosanError::ProviderError(format!("gemini CLI process error: {e}")))?,
    };

    if !result.status.success() {
        let stderr = String::from_utf8_lossy(&result.stderr);
        return Err(MotosanError::ProviderError(format!(
            "gemini CLI exited with {}: {}",
            result.status,
            stderr.trim()
        )));
    }

    let stdout = String::from_utf8_lossy(&result.stdout);
    parse_collected_stream(&stdout)
}

/// Accumulate the NDJSON stream-json output into `(content, usage, session_id)`.
///
/// Assistant deltas are concatenated in arrival order. The last
/// `result` event with `stats` wins for usage. A non-success `result`
/// is surfaced as an error.
fn parse_collected_stream(raw: &str) -> Result<(String, Usage, Option<String>), MotosanError> {
    let mut content = String::new();
    let mut session_id: Option<String> = None;
    let mut usage = Usage {
        input_tokens: 0,
        output_tokens: 0,
        cache_creation_input_tokens: None,
        cache_read_input_tokens: None,
    };

    for line in raw.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        match stream_json::parse_ndjson_line(line) {
            Some(NdjsonAction::Text(event)) => content.push_str(&event.content),
            Some(NdjsonAction::SessionStarted(event)) if session_id.is_none() => {
                session_id = event.session_id;
            }
            Some(NdjsonAction::SessionStarted(_)) => {}
            Some(NdjsonAction::ToolCalls(_)) => {}
            Some(NdjsonAction::Result {
                usage: Some(event), ..
            }) => {
                if let Some(u) = event.usage {
                    usage = u;
                }
            }
            Some(NdjsonAction::Result { usage: None, .. }) => {}
            Some(NdjsonAction::Error(msg)) => {
                return Err(MotosanError::ProviderError(format!("gemini CLI: {msg}")));
            }
            None => {}
        }
    }

    Ok((content, usage, session_id))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn empty_config() -> SpawnConfig {
        SpawnConfig {
            binary_path: PathBuf::from("gemini"),
            model: None,
            yolo: false,
            sandbox: false,
            approval_mode: None,
            include_dirs: Vec::new(),
            extensions: Vec::new(),
            allowed_mcp_servers: Vec::new(),
            resume: None,
            envs: Vec::new(),
            cwd: None,
            timeout: Some(DEFAULT_TIMEOUT),
        }
    }

    fn args_as_strings(args: &[OsString]) -> Vec<String> {
        args.iter()
            .map(|a| a.to_string_lossy().into_owned())
            .collect()
    }

    #[test]
    fn build_command_sets_current_dir_when_cwd_present() {
        let cfg = SpawnConfig {
            cwd: Some(PathBuf::from("/work/dir")),
            ..empty_config()
        };
        assert_eq!(
            build_command(&cfg).as_std().get_current_dir(),
            Some(std::path::Path::new("/work/dir"))
        );
        assert_eq!(
            build_command(&empty_config()).as_std().get_current_dir(),
            None
        );
    }

    #[test]
    fn build_command_injects_envs() {
        let cfg = SpawnConfig {
            envs: vec![("GEMINI_API_KEY".to_string(), "sk-secret".to_string())],
            ..empty_config()
        };
        let std_cmd = build_command(&cfg);
        let std_cmd = std_cmd.as_std();
        let found = std_cmd.get_envs().any(|(k, v)| {
            k.to_string_lossy() == "GEMINI_API_KEY"
                && v.map(|v| v.to_string_lossy().into_owned()) == Some("sk-secret".to_string())
        });
        assert!(found, "env must be injected");
        let argv: Vec<String> = std_cmd
            .get_args()
            .map(|a| a.to_string_lossy().into_owned())
            .collect();
        assert!(
            !argv.iter().any(|a| a.contains("sk-secret")),
            "secret must not leak into argv"
        );
    }

    #[test]
    fn common_args_empty_config_emits_headless_prefix() {
        assert_eq!(
            args_as_strings(&common_args(&empty_config())),
            vec!["-p", "", "-o", "stream-json"]
        );
    }

    #[test]
    fn common_args_forwards_model_when_non_default() {
        let cfg = SpawnConfig {
            model: Some("gemini-2.5-pro".to_string()),
            ..empty_config()
        };
        assert_eq!(
            args_as_strings(&common_args(&cfg)),
            vec!["-p", "", "-o", "stream-json", "-m", "gemini-2.5-pro"]
        );
    }

    #[test]
    fn common_args_skips_default_and_empty_model() {
        for value in ["default", "DEFAULT", "", "   "] {
            let cfg = SpawnConfig {
                model: Some(value.to_string()),
                ..empty_config()
            };
            assert_eq!(
                args_as_strings(&common_args(&cfg)),
                vec!["-p", "", "-o", "stream-json"],
                "model={value:?} should not append -m"
            );
        }
    }

    #[test]
    fn common_args_emits_yolo_flag() {
        let cfg = SpawnConfig {
            yolo: true,
            ..empty_config()
        };
        assert_eq!(
            args_as_strings(&common_args(&cfg)),
            vec!["-p", "", "-o", "stream-json", "--yolo"]
        );
    }

    #[test]
    fn common_args_emits_sandbox_flag() {
        let cfg = SpawnConfig {
            sandbox: true,
            ..empty_config()
        };
        assert_eq!(
            args_as_strings(&common_args(&cfg)),
            vec!["-p", "", "-o", "stream-json", "--sandbox"]
        );
    }

    #[test]
    fn common_args_approval_modes_map_to_cli_values() {
        for (mode, expected) in [
            (ApprovalMode::Default, "default"),
            (ApprovalMode::AutoEdit, "auto_edit"),
            (ApprovalMode::Yolo, "yolo"),
            (ApprovalMode::Plan, "plan"),
        ] {
            let cfg = SpawnConfig {
                approval_mode: Some(mode),
                ..empty_config()
            };
            assert_eq!(
                args_as_strings(&common_args(&cfg)),
                vec!["-p", "", "-o", "stream-json", "--approval-mode", expected],
                "approval_mode {mode:?}"
            );
        }
    }

    #[test]
    fn common_args_forwards_include_dirs_in_order() {
        let cfg = SpawnConfig {
            include_dirs: vec![PathBuf::from("/tmp/a"), PathBuf::from("/tmp/b")],
            ..empty_config()
        };
        assert_eq!(
            args_as_strings(&common_args(&cfg)),
            vec![
                "-p",
                "",
                "-o",
                "stream-json",
                "--include-directories",
                "/tmp/a",
                "--include-directories",
                "/tmp/b",
            ]
        );
    }

    #[test]
    fn common_args_forwards_extensions_in_order() {
        let cfg = SpawnConfig {
            extensions: vec!["foo".to_string(), "bar".to_string()],
            ..empty_config()
        };
        assert_eq!(
            args_as_strings(&common_args(&cfg)),
            vec!["-p", "", "-o", "stream-json", "-e", "foo", "-e", "bar"]
        );
    }

    #[test]
    fn common_args_skips_blank_extensions() {
        let cfg = SpawnConfig {
            extensions: vec!["".to_string(), "   ".to_string(), "ok".to_string()],
            ..empty_config()
        };
        assert_eq!(
            args_as_strings(&common_args(&cfg)),
            vec!["-p", "", "-o", "stream-json", "-e", "ok"]
        );
    }

    #[test]
    fn common_args_forwards_allowed_mcp_servers_in_order() {
        let cfg = SpawnConfig {
            allowed_mcp_servers: vec!["fs".to_string(), "web".to_string()],
            ..empty_config()
        };
        assert_eq!(
            args_as_strings(&common_args(&cfg)),
            vec![
                "-p",
                "",
                "-o",
                "stream-json",
                "--allowed-mcp-server-names",
                "fs",
                "--allowed-mcp-server-names",
                "web",
            ]
        );
    }

    #[test]
    fn common_args_skips_blank_allowed_mcp_servers() {
        let cfg = SpawnConfig {
            allowed_mcp_servers: vec!["".to_string(), "fs".to_string()],
            ..empty_config()
        };
        assert_eq!(
            args_as_strings(&common_args(&cfg)),
            vec![
                "-p",
                "",
                "-o",
                "stream-json",
                "--allowed-mcp-server-names",
                "fs"
            ]
        );
    }

    #[test]
    fn common_args_forwards_resume_value() {
        let cfg = SpawnConfig {
            resume: Some("latest".to_string()),
            ..empty_config()
        };
        assert_eq!(
            args_as_strings(&common_args(&cfg)),
            vec!["-p", "", "-o", "stream-json", "--resume", "latest"]
        );
    }

    #[test]
    fn common_args_skips_blank_resume() {
        for value in ["", "   ", "\t"] {
            let cfg = SpawnConfig {
                resume: Some(value.to_string()),
                ..empty_config()
            };
            assert_eq!(
                args_as_strings(&common_args(&cfg)),
                vec!["-p", "", "-o", "stream-json"],
                "resume={value:?}"
            );
        }
    }

    #[test]
    fn common_args_full_loadout_order_is_stable() {
        let cfg = SpawnConfig {
            binary_path: PathBuf::from("gemini"),
            model: Some("gemini-2.5-flash".to_string()),
            yolo: true,
            sandbox: true,
            approval_mode: Some(ApprovalMode::Yolo),
            include_dirs: vec![PathBuf::from("/tmp/extra")],
            extensions: vec!["fast".to_string()],
            allowed_mcp_servers: vec!["fs".to_string()],
            resume: Some("latest".to_string()),
            envs: Vec::new(),
            cwd: None,
            timeout: Some(DEFAULT_TIMEOUT),
        };
        assert_eq!(
            args_as_strings(&common_args(&cfg)),
            vec![
                "-p",
                "",
                "-o",
                "stream-json",
                "-m",
                "gemini-2.5-flash",
                "--yolo",
                "--sandbox",
                "--approval-mode",
                "yolo",
                "--include-directories",
                "/tmp/extra",
                "-e",
                "fast",
                "--allowed-mcp-server-names",
                "fs",
                "--resume",
                "latest",
            ]
        );
    }

    #[test]
    fn model_to_forward_returns_trimmed() {
        assert_eq!(model_to_forward("gemini-2.5-pro"), Some("gemini-2.5-pro"));
        assert_eq!(
            model_to_forward("  gemini-2.5-flash  "),
            Some("gemini-2.5-flash")
        );
    }

    #[test]
    fn model_to_forward_skips_default_and_empty() {
        assert_eq!(model_to_forward("default"), None);
        assert_eq!(model_to_forward("DEFAULT"), None);
        assert_eq!(model_to_forward(""), None);
        assert_eq!(model_to_forward("   "), None);
    }

    #[test]
    fn parse_collected_stream_accumulates_deltas_and_usage() {
        let raw = concat!(
            r#"{"type":"init","session_id":"s","model":"auto-gemini-3"}"#,
            "\n",
            r#"{"type":"message","role":"user","content":"hi"}"#,
            "\n",
            r#"{"type":"message","role":"assistant","content":"pon","delta":true}"#,
            "\n",
            r#"{"type":"message","role":"assistant","content":"g","delta":true}"#,
            "\n",
            r#"{"type":"result","status":"success","stats":{"input_tokens":10,"output_tokens":5,"cached":3}}"#,
            "\n",
        );
        let (content, usage, session_id) = parse_collected_stream(raw).expect("should parse");
        assert_eq!(content, "pong");
        assert_eq!(usage.input_tokens, 10);
        assert_eq!(usage.output_tokens, 5);
        assert_eq!(usage.cache_read_input_tokens, Some(3));
        // Blocking-path (chat()) session-id readback: the `init` event's id must
        // be captured by the collector, not dropped.
        assert_eq!(session_id.as_deref(), Some("s"));
    }

    #[test]
    fn parse_collected_stream_surfaces_non_success_result() {
        let raw = r#"{"type":"result","status":"failed"}"#;
        let err = parse_collected_stream(raw).unwrap_err();
        let MotosanError::ProviderError(msg) = err else {
            panic!("expected ProviderError");
        };
        assert!(msg.contains("failed"));
    }

    #[test]
    fn parse_collected_stream_ignores_blank_lines() {
        let raw = "\n\n   \n";
        let (content, usage, _session_id) = parse_collected_stream(raw).expect("should parse");
        assert_eq!(content, "");
        assert_eq!(usage.input_tokens, 0);
    }
}
