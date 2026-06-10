//! Subprocess lifecycle and flag wiring for the Codex CLI provider.
//!
//! This module owns the translation from [`CodexCliProvider`](super::CodexCliProvider)
//! state to argv, plus the blocking [`invoke_cli`] path used by
//! [`CodexCliProvider::chat`](super::CodexCliProvider::chat). The streaming path
//! shares [`apply_common_args`] / [`common_args`] to stay in sync.

use std::ffi::OsString;
use std::path::PathBuf;
use std::time::Duration;

use tokio::io::AsyncWriteExt;
use tokio::process::Command;

use crate::error::MotosanError;
use crate::types::Usage;

use super::stream_json::{self, NdjsonAction};

/// Sandbox policy for Codex CLI, forwarded as `--sandbox <mode>`.
///
/// Codex restricts what shell commands the model is allowed to run based on
/// this setting. Pick the least-privileged option that still lets the task
/// make progress.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SandboxMode {
    /// Read-only file access, no writes or network. Safe default for
    /// pure-analysis tasks.
    ReadOnly,
    /// Read everywhere, write only within the workspace root. Matches the
    /// behavior Codex uses under `--full-auto`.
    WorkspaceWrite,
    /// No sandbox. Only use when the caller is already externally
    /// sandboxed (disposable container, ephemeral VM).
    DangerFullAccess,
}

impl SandboxMode {
    /// The exact string Codex CLI expects for `--sandbox`.
    pub(crate) fn as_flag(&self) -> &'static str {
        match self {
            SandboxMode::ReadOnly => "read-only",
            SandboxMode::WorkspaceWrite => "workspace-write",
            SandboxMode::DangerFullAccess => "danger-full-access",
        }
    }
}

/// Local OSS provider for `--local-provider <p>`.
///
/// Selects which on-device provider Codex should talk to when running with
/// `--oss`. Only meaningful in combination with [`CodexCliProvider::oss`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LocalProvider {
    /// LM Studio (`lmstudio`).
    LmStudio,
    /// Ollama (`ollama`).
    Ollama,
}

impl LocalProvider {
    pub(crate) fn as_flag(&self) -> &'static str {
        match self {
            LocalProvider::LmStudio => "lmstudio",
            LocalProvider::Ollama => "ollama",
        }
    }
}

/// Configuration snapshot used to spawn a single `codex` CLI invocation.
///
/// Built by [`CodexCliProvider::build_spawn_config`](super::CodexCliProvider)
/// from the client state plus any per-request overrides. Each field mirrors
/// a public field on [`CodexCliProvider`](super::CodexCliProvider); see that
/// type's docs for the semantics.
pub struct SpawnConfig {
    /// Filesystem path to the `codex` binary.
    pub binary_path: PathBuf,
    /// Whether to pass `--full-auto`.
    pub agent_mode: bool,
    /// Model name for `--model`. `None` / `"default"` / blank are skipped.
    pub model: Option<String>,
    /// Working directory for `--cd`.
    pub cd: Option<PathBuf>,
    /// Sandbox policy for `--sandbox`.
    pub sandbox: Option<SandboxMode>,
    /// Profile name for `--profile`.
    pub profile: Option<String>,
    /// Whether to pass `--ephemeral`.
    pub ephemeral: bool,
    /// Forwarded as repeated `-c key=value` arguments, in the order supplied.
    pub config_overrides: Vec<(String, String)>,
    /// Repeated `--add-dir <DIR>` — additional writable workspace roots.
    pub add_dirs: Vec<PathBuf>,
    /// Repeated `--enable <FEATURE>` — convenience for `-c features.<name>=true`.
    pub enabled_features: Vec<String>,
    /// Repeated `--disable <FEATURE>` — convenience for `-c features.<name>=false`.
    pub disabled_features: Vec<String>,
    /// `--dangerously-bypass-approvals-and-sandbox`. Only set when the
    /// caller is already externally sandboxed.
    pub dangerously_bypass_approvals_and_sandbox: bool,
    /// `--oss` — use the OSS provider stack instead of OpenAI cloud.
    pub oss: bool,
    /// `--local-provider <p>` — selects the OSS provider used with `--oss`.
    pub local_provider: Option<LocalProvider>,
    /// Extra environment variables for the spawned subprocess, applied via
    /// `Command::envs`. Carries per-run secrets; do not log values.
    pub envs: Vec<(String, String)>,
    /// Thread id to resume, forwarded as the `resume <id>` subcommand of
    /// `codex exec` (`codex exec resume <id> --json ...`). Blank → fresh thread.
    pub resume: Option<String>,
}

/// Hard timeout for a single `codex exec` invocation.
///
/// Codex turns involving many tool calls can take minutes; 10 minutes is a
/// conservative upper bound that still prevents runaway processes.
const TIMEOUT_SECS: u64 = 600;

/// Build the argv fragment shared between the blocking
/// [`invoke_cli`] path and [`CodexCliProvider::stream`](super::CodexCliProvider::stream).
///
/// Returns a `Vec<OsString>` rather than mutating a `Command` so the
/// config-to-argv mapping is pure and unit-testable. Callers append the
/// result via `cmd.args(common_args(config))`.
///
/// The argument order is stable and documented by the `common_args_*`
/// unit tests — changing it may break callers that grep the spawned
/// command line for debugging.
pub(crate) fn common_args(config: &SpawnConfig) -> Vec<OsString> {
    let mut args: Vec<OsString> = Vec::new();

    if config.agent_mode {
        args.push("--full-auto".into());
    }

    if config.dangerously_bypass_approvals_and_sandbox {
        args.push("--dangerously-bypass-approvals-and-sandbox".into());
    }

    if let Some(mode) = config.sandbox {
        args.push("--sandbox".into());
        args.push(mode.as_flag().into());
    }

    if config.oss {
        args.push("--oss".into());
    }

    if let Some(provider) = config.local_provider {
        args.push("--local-provider".into());
        args.push(provider.as_flag().into());
    }

    if let Some(ref model) = config.model {
        if let Some(m) = model_to_forward(model) {
            args.push("--model".into());
            args.push(m.into());
        }
    }

    if let Some(ref profile) = config.profile {
        if !profile.trim().is_empty() {
            args.push("--profile".into());
            args.push(profile.into());
        }
    }

    if let Some(ref dir) = config.cd {
        args.push("--cd".into());
        args.push(dir.into());
    }

    for dir in &config.add_dirs {
        args.push("--add-dir".into());
        args.push(dir.into());
    }

    if config.ephemeral {
        args.push("--ephemeral".into());
    }

    for feature in &config.enabled_features {
        if !feature.is_empty() {
            args.push("--enable".into());
            args.push(feature.into());
        }
    }

    for feature in &config.disabled_features {
        if !feature.is_empty() {
            args.push("--disable".into());
            args.push(feature.into());
        }
    }

    for (key, value) in &config.config_overrides {
        args.push("-c".into());
        args.push(format!("{key}={value}").into());
    }

    args
}

/// Convenience wrapper that pushes [`common_args`] onto an existing
/// [`Command`].
pub(crate) fn apply_common_args(cmd: &mut Command, config: &SpawnConfig) {
    cmd.args(common_args(config));
}

/// Push the `exec` subcommand (plus `resume <id>` when resuming) onto `cmd`.
/// Shared by the blocking and streaming spawn sites so the surface is identical.
pub(crate) fn push_exec_subcommand(cmd: &mut Command, config: &SpawnConfig) {
    cmd.arg("exec");
    if let Some(ref id) = config.resume {
        let trimmed = id.trim();
        if !trimmed.is_empty() {
            cmd.arg("resume");
            cmd.arg(trimmed);
        }
    }
}

/// Normalize a user-supplied model string for `--model <value>`.
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

/// Assemble a ready-to-spawn [`Command`] for a blocking `codex exec` call.
///
/// Sets `--json --skip-git-repo-check`, applies the shared flags via
/// [`apply_common_args`], forwards `-` so the prompt is read from stdin,
/// and pipes all three std streams. `kill_on_drop` ensures the child is
/// reaped if the future is cancelled mid-flight.
fn build_command(config: &SpawnConfig) -> Command {
    let mut cmd = Command::new(&config.binary_path);
    cmd.envs(config.envs.iter().map(|(k, v)| (k, v)));
    push_exec_subcommand(&mut cmd, config);
    cmd.arg("--json").arg("--skip-git-repo-check");
    apply_common_args(&mut cmd, config);
    cmd.arg("-"); // stdin

    cmd.kill_on_drop(true);
    cmd.stdin(std::process::Stdio::piped());
    cmd.stdout(std::process::Stdio::piped());
    cmd.stderr(std::process::Stdio::piped());
    cmd
}

/// Spawn `codex exec`, feed it `prompt` on stdin, and collect the turn.
///
/// Returns `(content, thinking, usage, session_id)` where:
/// - `content` is the **last** `agent_message` item (Codex's final answer),
/// - `thinking` is all prior `agent_message` items joined with blank lines
///   (preamble + tool narration), or `None` if only one message was emitted,
/// - `usage` is the token tally from `turn.completed`.
///
/// # Errors
///
/// Returns [`MotosanError::ProviderError`] if the subprocess fails to
/// spawn, times out after [`TIMEOUT_SECS`], exits non-zero, emits an
/// `error` / `turn.failed` event, or the output cannot be parsed.
pub async fn invoke_cli(
    config: &SpawnConfig,
    prompt: &str,
) -> Result<(String, Option<String>, Usage, Option<String>), MotosanError> {
    let mut cmd = build_command(config);

    let mut child = cmd
        .spawn()
        .map_err(|e| MotosanError::ProviderError(format!("failed to spawn codex CLI: {e}")))?;

    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(prompt.as_bytes()).await.map_err(|e| {
            MotosanError::ProviderError(format!("failed to write to codex stdin: {e}"))
        })?;
    }

    let result = tokio::time::timeout(Duration::from_secs(TIMEOUT_SECS), child.wait_with_output())
        .await
        .map_err(|_| {
            MotosanError::ProviderError(format!("codex CLI timed out after {TIMEOUT_SECS} seconds"))
        })?
        .map_err(|e| MotosanError::ProviderError(format!("codex CLI process error: {e}")))?;

    if !result.status.success() {
        let stderr = String::from_utf8_lossy(&result.stderr);
        return Err(MotosanError::ProviderError(format!(
            "codex CLI exited with {}: {}",
            result.status,
            stderr.trim()
        )));
    }

    let stdout = String::from_utf8_lossy(&result.stdout);
    parse_collected_stream(&stdout)
}

/// Parse accumulated JSONL output into `(content, thinking, usage, session_id)`.
///
/// `content` is the last `agent_message` item. Any earlier `agent_message`
/// items become `thinking` (joined with blank lines). Codex often emits a
/// preamble message before running tools and a separate final answer; this
/// split matches how other providers expose reasoning vs final answer.
fn parse_collected_stream(
    raw: &str,
) -> Result<(String, Option<String>, Usage, Option<String>), MotosanError> {
    let mut agent_messages: Vec<String> = Vec::new();
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
            Some(NdjsonAction::Text(event)) => agent_messages.push(event.content),
            Some(NdjsonAction::SessionStarted(event)) if session_id.is_none() => {
                session_id = event.session_id;
            }
            Some(NdjsonAction::SessionStarted(_)) => {}
            Some(NdjsonAction::Done {
                usage: Some(event), ..
            }) => {
                if let Some(collected) = event.usage {
                    usage = collected;
                }
            }
            Some(NdjsonAction::Done { usage: None, .. }) => {}
            Some(NdjsonAction::Error(msg)) => {
                return Err(MotosanError::ProviderError(format!("codex CLI: {msg}")));
            }
            None => {}
        }
    }

    let content = agent_messages.pop().unwrap_or_default();
    let thinking = if agent_messages.is_empty() {
        None
    } else {
        Some(agent_messages.join("\n\n"))
    };

    Ok((content, thinking, usage, session_id))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn empty_config() -> SpawnConfig {
        SpawnConfig {
            binary_path: PathBuf::from("codex"),
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
            envs: Vec::new(),
            resume: None,
        }
    }

    fn args_as_strings(args: &[OsString]) -> Vec<String> {
        args.iter()
            .map(|a| a.to_string_lossy().into_owned())
            .collect()
    }

    #[test]
    fn common_args_empty_config_produces_nothing() {
        assert!(common_args(&empty_config()).is_empty());
    }

    #[test]
    fn build_command_injects_envs() {
        let cfg = SpawnConfig {
            envs: vec![("OPENAI_API_KEY".to_string(), "sk-secret".to_string())],
            ..empty_config()
        };
        let std_cmd = build_command(&cfg);
        let std_cmd = std_cmd.as_std();
        let found = std_cmd.get_envs().any(|(k, v)| {
            k.to_string_lossy() == "OPENAI_API_KEY"
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
    fn common_args_agent_mode_emits_full_auto() {
        let cfg = SpawnConfig {
            agent_mode: true,
            ..empty_config()
        };
        assert_eq!(args_as_strings(&common_args(&cfg)), vec!["--full-auto"]);
    }

    #[test]
    fn common_args_sandbox_modes_map_to_cli_values() {
        for (mode, expected) in [
            (SandboxMode::ReadOnly, "read-only"),
            (SandboxMode::WorkspaceWrite, "workspace-write"),
            (SandboxMode::DangerFullAccess, "danger-full-access"),
        ] {
            let cfg = SpawnConfig {
                sandbox: Some(mode),
                ..empty_config()
            };
            assert_eq!(
                args_as_strings(&common_args(&cfg)),
                vec!["--sandbox", expected],
                "sandbox {mode:?}"
            );
        }
    }

    #[test]
    fn common_args_forwards_model_when_non_default() {
        let cfg = SpawnConfig {
            model: Some("gpt-5.1-codex".to_string()),
            ..empty_config()
        };
        assert_eq!(
            args_as_strings(&common_args(&cfg)),
            vec!["--model", "gpt-5.1-codex"]
        );
    }

    #[test]
    fn common_args_skips_default_and_empty_model() {
        for value in ["default", "DEFAULT", "", "   "] {
            let cfg = SpawnConfig {
                model: Some(value.to_string()),
                ..empty_config()
            };
            assert!(
                common_args(&cfg).is_empty(),
                "model={value:?} should produce no args"
            );
        }
    }

    #[test]
    fn common_args_forwards_profile_when_non_empty() {
        let cfg = SpawnConfig {
            profile: Some("work".to_string()),
            ..empty_config()
        };
        assert_eq!(
            args_as_strings(&common_args(&cfg)),
            vec!["--profile", "work"]
        );
    }

    #[test]
    fn common_args_skips_blank_profile() {
        let cfg = SpawnConfig {
            profile: Some("   ".to_string()),
            ..empty_config()
        };
        assert!(common_args(&cfg).is_empty());
    }

    #[test]
    fn common_args_forwards_cd() {
        let cfg = SpawnConfig {
            cd: Some(PathBuf::from("/tmp/project")),
            ..empty_config()
        };
        assert_eq!(
            args_as_strings(&common_args(&cfg)),
            vec!["--cd", "/tmp/project"]
        );
    }

    #[test]
    fn common_args_emits_ephemeral_flag() {
        let cfg = SpawnConfig {
            ephemeral: true,
            ..empty_config()
        };
        assert_eq!(args_as_strings(&common_args(&cfg)), vec!["--ephemeral"]);
    }

    #[test]
    fn common_args_forwards_config_overrides_in_order() {
        let cfg = SpawnConfig {
            config_overrides: vec![
                ("model_reasoning_effort".to_string(), "\"low\"".to_string()),
                ("features.foo".to_string(), "true".to_string()),
            ],
            ..empty_config()
        };
        assert_eq!(
            args_as_strings(&common_args(&cfg)),
            vec![
                "-c",
                "model_reasoning_effort=\"low\"",
                "-c",
                "features.foo=true",
            ]
        );
    }

    #[test]
    fn common_args_combined_order_is_stable() {
        let cfg = SpawnConfig {
            agent_mode: true,
            sandbox: Some(SandboxMode::WorkspaceWrite),
            model: Some("sonnet".to_string()),
            profile: Some("work".to_string()),
            cd: Some(PathBuf::from("/x")),
            ephemeral: true,
            config_overrides: vec![("k".to_string(), "v".to_string())],
            ..empty_config()
        };
        assert_eq!(
            args_as_strings(&common_args(&cfg)),
            vec![
                "--full-auto",
                "--sandbox",
                "workspace-write",
                "--model",
                "sonnet",
                "--profile",
                "work",
                "--cd",
                "/x",
                "--ephemeral",
                "-c",
                "k=v",
            ]
        );
    }

    #[test]
    fn common_args_emits_dangerously_bypass() {
        let cfg = SpawnConfig {
            dangerously_bypass_approvals_and_sandbox: true,
            ..empty_config()
        };
        assert_eq!(
            args_as_strings(&common_args(&cfg)),
            vec!["--dangerously-bypass-approvals-and-sandbox"]
        );
    }

    #[test]
    fn common_args_emits_oss_and_local_provider() {
        let cfg = SpawnConfig {
            oss: true,
            local_provider: Some(LocalProvider::Ollama),
            ..empty_config()
        };
        assert_eq!(
            args_as_strings(&common_args(&cfg)),
            vec!["--oss", "--local-provider", "ollama"]
        );
    }

    #[test]
    fn local_provider_lmstudio_serializes_correctly() {
        let cfg = SpawnConfig {
            oss: true,
            local_provider: Some(LocalProvider::LmStudio),
            ..empty_config()
        };
        assert_eq!(
            args_as_strings(&common_args(&cfg)),
            vec!["--oss", "--local-provider", "lmstudio"]
        );
    }

    #[test]
    fn common_args_forwards_add_dirs_in_order() {
        let cfg = SpawnConfig {
            add_dirs: vec![PathBuf::from("/tmp/a"), PathBuf::from("/tmp/b")],
            ..empty_config()
        };
        assert_eq!(
            args_as_strings(&common_args(&cfg)),
            vec!["--add-dir", "/tmp/a", "--add-dir", "/tmp/b"]
        );
    }

    #[test]
    fn common_args_forwards_enabled_and_disabled_features() {
        let cfg = SpawnConfig {
            enabled_features: vec!["foo".to_string(), "bar".to_string()],
            disabled_features: vec!["baz".to_string()],
            ..empty_config()
        };
        assert_eq!(
            args_as_strings(&common_args(&cfg)),
            vec!["--enable", "foo", "--enable", "bar", "--disable", "baz",]
        );
    }

    #[test]
    fn common_args_skips_blank_feature_names() {
        let cfg = SpawnConfig {
            enabled_features: vec!["".to_string(), "ok".to_string()],
            disabled_features: vec!["".to_string()],
            ..empty_config()
        };
        assert_eq!(args_as_strings(&common_args(&cfg)), vec!["--enable", "ok"]);
    }

    #[test]
    fn resume_inserts_exec_resume_subcommand() {
        let cfg = SpawnConfig {
            resume: Some("th_abc123".to_string()),
            ..empty_config()
        };
        let argv: Vec<String> = build_command(&cfg)
            .as_std()
            .get_args()
            .map(|a| a.to_string_lossy().into_owned())
            .collect();
        assert_eq!(&argv[0..4], &["exec", "resume", "th_abc123", "--json"]);
        assert_eq!(argv.last().map(String::as_str), Some("-"));
    }

    #[test]
    fn no_resume_keeps_bare_exec() {
        let argv: Vec<String> = build_command(&empty_config())
            .as_std()
            .get_args()
            .map(|a| a.to_string_lossy().into_owned())
            .collect();
        assert_eq!(&argv[0..3], &["exec", "--json", "--skip-git-repo-check"]);
        assert!(!argv.iter().any(|a| a == "resume"));
    }

    #[test]
    fn common_args_full_loadout_order_is_stable() {
        // Every flag set; locks the documented order so future edits to
        // `common_args` can't silently reorder the spawned command line.
        let cfg = SpawnConfig {
            binary_path: PathBuf::from("codex"),
            agent_mode: true,
            dangerously_bypass_approvals_and_sandbox: true,
            sandbox: Some(SandboxMode::WorkspaceWrite),
            oss: true,
            local_provider: Some(LocalProvider::Ollama),
            model: Some("gpt-5.1-codex".to_string()),
            profile: Some("work".to_string()),
            cd: Some(PathBuf::from("/x")),
            add_dirs: vec![PathBuf::from("/tmp/extra")],
            ephemeral: true,
            enabled_features: vec!["fast".to_string()],
            disabled_features: vec!["slow".to_string()],
            config_overrides: vec![("k".to_string(), "v".to_string())],
            envs: Vec::new(),
            resume: None,
        };
        assert_eq!(
            args_as_strings(&common_args(&cfg)),
            vec![
                "--full-auto",
                "--dangerously-bypass-approvals-and-sandbox",
                "--sandbox",
                "workspace-write",
                "--oss",
                "--local-provider",
                "ollama",
                "--model",
                "gpt-5.1-codex",
                "--profile",
                "work",
                "--cd",
                "/x",
                "--add-dir",
                "/tmp/extra",
                "--ephemeral",
                "--enable",
                "fast",
                "--disable",
                "slow",
                "-c",
                "k=v",
            ]
        );
    }

    #[test]
    fn model_to_forward_returns_trimmed() {
        assert_eq!(model_to_forward("gpt-5.1-codex"), Some("gpt-5.1-codex"));
        assert_eq!(model_to_forward("  gpt-5.1  "), Some("gpt-5.1"));
    }

    #[test]
    fn model_to_forward_skips_default_and_empty() {
        assert_eq!(model_to_forward("default"), None);
        assert_eq!(model_to_forward("DEFAULT"), None);
        assert_eq!(model_to_forward(""), None);
        assert_eq!(model_to_forward("   "), None);
    }

    #[test]
    fn last_agent_message_is_content_rest_is_thinking() {
        let raw = concat!(
            r#"{"type":"thread.started","thread_id":"t1"}"#,
            "\n",
            r#"{"type":"item.completed","item":{"id":"i1","type":"agent_message","text":"Let me think..."}}"#,
            "\n",
            r#"{"type":"item.completed","item":{"id":"i2","type":"agent_message","text":"Checking the docs."}}"#,
            "\n",
            r#"{"type":"item.completed","item":{"id":"i3","type":"agent_message","text":"pong"}}"#,
            "\n",
            r#"{"type":"turn.completed","usage":{"input_tokens":10,"output_tokens":5,"cached_input_tokens":3}}"#,
            "\n",
        );
        let (content, thinking, usage, _session_id) =
            parse_collected_stream(raw).expect("should parse");
        assert_eq!(content, "pong");
        assert_eq!(
            thinking.as_deref(),
            Some("Let me think...\n\nChecking the docs.")
        );
        assert_eq!(usage.input_tokens, 10);
        assert_eq!(usage.output_tokens, 5);
        assert_eq!(usage.cache_read_input_tokens, Some(3));
    }

    #[test]
    fn single_agent_message_has_no_thinking() {
        let raw = concat!(
            r#"{"type":"item.completed","item":{"id":"i1","type":"agent_message","text":"pong"}}"#,
            "\n",
            r#"{"type":"turn.completed"}"#,
            "\n",
        );
        let (content, thinking, _, _session_id) =
            parse_collected_stream(raw).expect("should parse");
        assert_eq!(content, "pong");
        assert_eq!(thinking, None);
    }

    #[test]
    fn parse_collected_stream_captures_thread_id() {
        let raw = concat!(
            r#"{"type":"thread.started","thread_id":"th_xyz"}"#,
            "\n",
            r#"{"type":"item.completed","item":{"id":"i1","type":"agent_message","text":"pong"}}"#,
            "\n",
            r#"{"type":"turn.completed"}"#,
            "\n",
        );
        let (content, _thinking, _usage, session_id) = parse_collected_stream(raw).expect("parse");
        assert_eq!(content, "pong");
        assert_eq!(session_id.as_deref(), Some("th_xyz"));
    }

    #[test]
    fn parse_collected_stream_surfaces_error() {
        let raw = r#"{"type":"error","message":"nope"}"#;
        let err = parse_collected_stream(raw).unwrap_err();
        let MotosanError::ProviderError(msg) = err else {
            panic!("expected ProviderError");
        };
        assert!(msg.contains("nope"));
    }

    #[test]
    fn parse_collected_stream_ignores_blank_lines() {
        let raw = "\n\n   \n";
        let (content, thinking, _, _session_id) =
            parse_collected_stream(raw).expect("should parse");
        assert_eq!(content, "");
        assert_eq!(thinking, None);
    }
}
