//! Flag wiring for the Claude Code CLI provider. Argv layout is built by
//! [`common_args`] and consumed by
//! [`ClaudeCodeProvider::stream`](super::ClaudeCodeProvider::stream), which
//! both `chat()` and `stream()` share since chat() delegates to stream
//! collection.

use std::ffi::OsString;
use std::path::PathBuf;
use std::time::Duration;

/// Permission mode forwarded as `--permission-mode <mode>`.
///
/// Mirrors Claude Code CLI's own `--permission-mode` choices. Selecting
/// anything other than [`PermissionMode::Default`] relaxes or replaces the
/// per-tool confirmation dialog Claude Code shows in interactive mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PermissionMode {
    /// Auto-accept edits, prompt for everything else.
    AcceptEdits,
    /// Full auto-mode (reads + writes + bash). Equivalent to `--auto`.
    Auto,
    /// Bypass all permission checks. Intended for sandboxed environments.
    BypassPermissions,
    /// Standard interactive prompting. Usually the right value under
    /// `--print`, where prompts are never shown anyway.
    Default,
    /// "Don't ask" — suppress confirmation dialogs without granting any
    /// extra privilege.
    DontAsk,
    /// Read-only plan mode.
    Plan,
}

impl PermissionMode {
    /// The exact string Claude Code CLI expects for `--permission-mode`.
    pub(crate) fn as_flag(&self) -> &'static str {
        match self {
            PermissionMode::AcceptEdits => "acceptEdits",
            PermissionMode::Auto => "auto",
            PermissionMode::BypassPermissions => "bypassPermissions",
            PermissionMode::Default => "default",
            PermissionMode::DontAsk => "dontAsk",
            PermissionMode::Plan => "plan",
        }
    }
}

/// Effort level forwarded as `--effort <level>`.
///
/// Controls how much reasoning budget Claude Code allocates to the
/// current session. Higher levels cost more tokens.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EffortLevel {
    Low,
    Medium,
    High,
    Max,
}

impl EffortLevel {
    pub(crate) fn as_flag(&self) -> &'static str {
        match self {
            EffortLevel::Low => "low",
            EffortLevel::Medium => "medium",
            EffortLevel::High => "high",
            EffortLevel::Max => "max",
        }
    }
}

/// Configuration snapshot used to spawn a single `claude` CLI invocation.
///
/// Built by [`ClaudeCodeProvider::build_spawn_config`](super::ClaudeCodeProvider)
/// from client state plus any per-request overrides. Every field mirrors
/// a public field on [`ClaudeCodeProvider`](super::ClaudeCodeProvider); see
/// that type's docs for the semantics.
pub struct SpawnConfig {
    /// Filesystem path to the `claude` binary.
    pub binary_path: PathBuf,
    /// Whether to pass `--dangerously-skip-permissions`.
    pub agent_mode: bool,
    /// Whether to pass `--bare`. Skips hooks, plugins, auto-memory,
    /// keychain reads, and user/project settings discovery so the
    /// subprocess does not inherit ambient Claude Code state.
    /// Under `--bare`, Anthropic auth is restricted to
    /// `ANTHROPIC_API_KEY` or an `apiKeyHelper` declared inside
    /// `settings`; `setting_sources` is inert because the discovery
    /// it controls is disabled. Explicit `settings`, `mcp_config`,
    /// `add_dirs`, `plugin_dirs`, `agent`, and `system_prompt` are
    /// still honoured. See [`super::ClaudeCodeProvider::bare`] for
    /// the public-API documentation.
    pub bare: bool,
    /// Model name for `--model`. `None` / `"default"` / blank are skipped.
    pub model: Option<String>,
    /// Text appended via `--append-system-prompt`. Usually populated
    /// from a `role: system` message in [`ChatRequest`](crate::types::ChatRequest).
    pub append_system_prompt: Option<String>,
    /// Full system prompt replacement via `--system-prompt`. Takes
    /// precedence over Claude Code's built-in defaults and coexists
    /// with `append_system_prompt` (Claude applies append on top).
    pub system_prompt: Option<String>,
    /// Optional `--permission-mode` override.
    pub permission_mode: Option<PermissionMode>,
    /// Optional `--effort` level.
    pub effort: Option<EffortLevel>,
    /// Optional `--fallback-model` name used when the primary model is
    /// overloaded. Only works under `--print`.
    pub fallback_model: Option<String>,
    /// Additional directories forwarded as repeated `--add-dir <DIR>` flags.
    pub add_dirs: Vec<PathBuf>,
    /// Tool allowlist. Forwarded as `--allowed-tools a b c` (variadic).
    /// Blank entries are skipped.
    pub allowed_tools: Vec<String>,
    /// Tool denylist. Forwarded as `--disallowed-tools a b c`.
    /// Blank entries are skipped.
    pub disallowed_tools: Vec<String>,
    /// MCP config file paths or inline JSON strings. Forwarded as
    /// `--mcp-config a b c`.
    pub mcp_config: Vec<String>,
    /// `--strict-mcp-config` — only use MCP servers from `mcp_config`.
    pub strict_mcp_config: bool,
    /// `--settings <file-or-json>` — either a path to a settings JSON
    /// file or an inline JSON string.
    pub settings: Option<String>,
    /// `--setting-sources <csv>` — comma-separated sources to load.
    /// Each entry should be one of `user`, `project`, `local`.
    pub setting_sources: Vec<String>,
    /// `--session-id <uuid>` — reuse a specific session ID.
    pub session_id: Option<String>,
    /// `-r, --resume <value>` — resume by session ID or `"latest"`.
    pub resume: Option<String>,
    /// `-c, --continue` — continue the most recent conversation in the
    /// current directory.
    pub continue_latest: bool,
    /// `--fork-session` — create a new session ID on resume.
    pub fork_session: bool,
    /// Additional plugin directories (`--plugin-dir <path>` repeated).
    pub plugin_dirs: Vec<PathBuf>,
    /// `--agent <name>` — named agent override.
    pub agent: Option<String>,
    /// `--no-session-persistence` — don't save the session to disk.
    /// Only meaningful under `--print`.
    pub no_session_persistence: bool,
    /// `--max-budget-usd <amount>` — dollar cap for API calls during
    /// this session. Only meaningful under `--print`.
    pub max_budget_usd: Option<f64>,
    /// Extra environment variables for the spawned subprocess, applied via
    /// `Command::envs`. Carries per-run secrets; do not log values.
    pub envs: Vec<(String, String)>,
    /// Working directory for the spawned process; `None` inherits the parent's.
    pub cwd: Option<PathBuf>,
    /// Timeout for this invocation; `None` disables the timeout wrapper.
    pub timeout: Option<Duration>,
}

pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(300);

/// Returns the trimmed model string to forward as `--model <value>`, or `None` if
/// the value should be skipped.
///
/// Skips empty strings, whitespace-only strings, and the sentinel value `"default"`
/// (case-insensitive). Returning the trimmed value directly prevents callers from
/// accidentally forwarding padded whitespace to the CLI.
pub(crate) fn model_to_forward(model: &str) -> Option<&str> {
    let trimmed = model.trim();
    if trimmed.is_empty() || trimmed.eq_ignore_ascii_case("default") {
        None
    } else {
        Some(trimmed)
    }
}

/// Build the argv fragment consumed by
/// [`ClaudeCodeProvider::stream`](super::ClaudeCodeProvider::stream), which
/// both `chat()` and `stream()` share.
///
/// Returns a `Vec<OsString>` rather than mutating a `Command` so the
/// config-to-argv mapping is pure and unit-testable. Callers append the
/// result via `cmd.args(common_args(config))` **after** pushing the
/// path-specific `--output-format` / `--print` flags.
///
/// The argument order is stable and documented by the `common_args_*`
/// unit tests — changing it may break callers that grep the spawned
/// command line for debugging.
pub(crate) fn common_args(config: &SpawnConfig) -> Vec<OsString> {
    let mut args: Vec<OsString> = Vec::new();

    if config.bare {
        args.push("--bare".into());
    }

    if config.agent_mode {
        args.push("--dangerously-skip-permissions".into());
    }

    if let Some(mode) = config.permission_mode {
        args.push("--permission-mode".into());
        args.push(mode.as_flag().into());
    }

    if let Some(ref model) = config.model {
        if let Some(m) = model_to_forward(model) {
            args.push("--model".into());
            args.push(m.into());
        }
    }

    if let Some(ref fallback) = config.fallback_model {
        if let Some(m) = model_to_forward(fallback) {
            args.push("--fallback-model".into());
            args.push(m.into());
        }
    }

    if let Some(level) = config.effort {
        args.push("--effort".into());
        args.push(level.as_flag().into());
    }

    if let Some(ref sp) = config.system_prompt {
        if !sp.is_empty() {
            args.push("--system-prompt".into());
            args.push(sp.into());
        }
    }

    if let Some(ref sp) = config.append_system_prompt {
        if !sp.is_empty() {
            args.push("--append-system-prompt".into());
            args.push(sp.into());
        }
    }

    if let Some(ref agent) = config.agent {
        if !agent.trim().is_empty() {
            args.push("--agent".into());
            args.push(agent.into());
        }
    }

    for dir in &config.add_dirs {
        args.push("--add-dir".into());
        args.push(dir.into());
    }

    if !config.allowed_tools.is_empty() {
        let mut any = false;
        for t in &config.allowed_tools {
            if !t.trim().is_empty() {
                if !any {
                    args.push("--allowed-tools".into());
                    any = true;
                }
                args.push(t.into());
            }
        }
    }

    if !config.disallowed_tools.is_empty() {
        let mut any = false;
        for t in &config.disallowed_tools {
            if !t.trim().is_empty() {
                if !any {
                    args.push("--disallowed-tools".into());
                    any = true;
                }
                args.push(t.into());
            }
        }
    }

    if !config.mcp_config.is_empty() {
        let mut any = false;
        for cfg in &config.mcp_config {
            if !cfg.trim().is_empty() {
                if !any {
                    args.push("--mcp-config".into());
                    any = true;
                }
                args.push(cfg.into());
            }
        }
    }

    if config.strict_mcp_config {
        args.push("--strict-mcp-config".into());
    }

    if let Some(ref s) = config.settings {
        if !s.trim().is_empty() {
            args.push("--settings".into());
            args.push(s.into());
        }
    }

    if !config.setting_sources.is_empty() {
        let joined = config
            .setting_sources
            .iter()
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join(",");
        if !joined.is_empty() {
            args.push("--setting-sources".into());
            args.push(joined.into());
        }
    }

    for dir in &config.plugin_dirs {
        args.push("--plugin-dir".into());
        args.push(dir.into());
    }

    if let Some(ref id) = config.session_id {
        if !id.trim().is_empty() {
            args.push("--session-id".into());
            args.push(id.into());
        }
    }

    if let Some(ref value) = config.resume {
        let trimmed = value.trim();
        if !trimmed.is_empty() {
            args.push("--resume".into());
            args.push(trimmed.into());
        }
    }

    if config.continue_latest {
        args.push("--continue".into());
    }

    if config.fork_session {
        args.push("--fork-session".into());
    }

    if config.no_session_persistence {
        args.push("--no-session-persistence".into());
    }

    if let Some(amount) = config.max_budget_usd {
        if amount.is_finite() && amount >= 0.0 {
            args.push("--max-budget-usd".into());
            args.push(amount.to_string().into());
        }
    }

    args
}

#[cfg(test)]
mod tests {
    use super::*;

    fn empty_config() -> SpawnConfig {
        SpawnConfig {
            binary_path: PathBuf::from("claude"),
            agent_mode: false,
            bare: false,
            model: None,
            append_system_prompt: None,
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
    fn model_to_forward_returns_trimmed_named_models() {
        assert_eq!(model_to_forward("sonnet"), Some("sonnet"));
        assert_eq!(model_to_forward("opus"), Some("opus"));
        assert_eq!(
            model_to_forward("claude-sonnet-4-6"),
            Some("claude-sonnet-4-6")
        );
        assert_eq!(model_to_forward("  sonnet  "), Some("sonnet"));
    }

    #[test]
    fn model_to_forward_skips_default() {
        assert_eq!(model_to_forward("default"), None);
        assert_eq!(model_to_forward("Default"), None);
        assert_eq!(model_to_forward("DEFAULT"), None);
        assert_eq!(model_to_forward("  default  "), None);
    }

    #[test]
    fn model_to_forward_skips_empty_and_whitespace() {
        assert_eq!(model_to_forward(""), None);
        assert_eq!(model_to_forward("   "), None);
        assert_eq!(model_to_forward("\t"), None);
    }

    #[test]
    fn common_args_empty_config_produces_nothing() {
        assert!(common_args(&empty_config()).is_empty());
    }

    #[test]
    fn common_args_agent_mode_emits_skip_permissions() {
        let cfg = SpawnConfig {
            agent_mode: true,
            ..empty_config()
        };
        assert_eq!(
            args_as_strings(&common_args(&cfg)),
            vec!["--dangerously-skip-permissions"]
        );
    }

    #[test]
    fn common_args_bare_emits_bare_flag() {
        let cfg = SpawnConfig {
            bare: true,
            ..empty_config()
        };
        assert_eq!(args_as_strings(&common_args(&cfg)), vec!["--bare"]);
    }

    #[test]
    fn common_args_bare_precedes_agent_mode() {
        let cfg = SpawnConfig {
            bare: true,
            agent_mode: true,
            ..empty_config()
        };
        assert_eq!(
            args_as_strings(&common_args(&cfg)),
            vec!["--bare", "--dangerously-skip-permissions"]
        );
    }

    #[test]
    fn common_args_forwards_model_when_non_default() {
        let cfg = SpawnConfig {
            model: Some("sonnet".to_string()),
            ..empty_config()
        };
        assert_eq!(
            args_as_strings(&common_args(&cfg)),
            vec!["--model", "sonnet"]
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
    fn common_args_forwards_fallback_model() {
        let cfg = SpawnConfig {
            fallback_model: Some("opus".to_string()),
            ..empty_config()
        };
        assert_eq!(
            args_as_strings(&common_args(&cfg)),
            vec!["--fallback-model", "opus"]
        );
    }

    #[test]
    fn common_args_permission_modes_map_to_cli_values() {
        for (mode, expected) in [
            (PermissionMode::AcceptEdits, "acceptEdits"),
            (PermissionMode::Auto, "auto"),
            (PermissionMode::BypassPermissions, "bypassPermissions"),
            (PermissionMode::Default, "default"),
            (PermissionMode::DontAsk, "dontAsk"),
            (PermissionMode::Plan, "plan"),
        ] {
            let cfg = SpawnConfig {
                permission_mode: Some(mode),
                ..empty_config()
            };
            assert_eq!(
                args_as_strings(&common_args(&cfg)),
                vec!["--permission-mode", expected],
                "permission_mode {mode:?}"
            );
        }
    }

    #[test]
    fn common_args_effort_levels_map_to_cli_values() {
        for (level, expected) in [
            (EffortLevel::Low, "low"),
            (EffortLevel::Medium, "medium"),
            (EffortLevel::High, "high"),
            (EffortLevel::Max, "max"),
        ] {
            let cfg = SpawnConfig {
                effort: Some(level),
                ..empty_config()
            };
            assert_eq!(
                args_as_strings(&common_args(&cfg)),
                vec!["--effort", expected],
                "effort {level:?}"
            );
        }
    }

    #[test]
    fn common_args_forwards_system_prompt_and_append() {
        let cfg = SpawnConfig {
            system_prompt: Some("be terse".to_string()),
            append_system_prompt: Some("also be helpful".to_string()),
            ..empty_config()
        };
        assert_eq!(
            args_as_strings(&common_args(&cfg)),
            vec![
                "--system-prompt",
                "be terse",
                "--append-system-prompt",
                "also be helpful",
            ]
        );
    }

    #[test]
    fn common_args_skips_empty_system_prompts() {
        let cfg = SpawnConfig {
            system_prompt: Some(String::new()),
            append_system_prompt: Some(String::new()),
            ..empty_config()
        };
        assert!(common_args(&cfg).is_empty());
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
    fn common_args_allowed_tools_emits_variadic() {
        let cfg = SpawnConfig {
            allowed_tools: vec!["Bash(git *)".to_string(), "Edit".to_string()],
            ..empty_config()
        };
        assert_eq!(
            args_as_strings(&common_args(&cfg)),
            vec!["--allowed-tools", "Bash(git *)", "Edit"]
        );
    }

    #[test]
    fn common_args_disallowed_tools_emits_variadic() {
        let cfg = SpawnConfig {
            disallowed_tools: vec!["WebFetch".to_string()],
            ..empty_config()
        };
        assert_eq!(
            args_as_strings(&common_args(&cfg)),
            vec!["--disallowed-tools", "WebFetch"]
        );
    }

    #[test]
    fn common_args_skips_blank_tool_entries() {
        let cfg = SpawnConfig {
            allowed_tools: vec!["".to_string(), "   ".to_string(), "Edit".to_string()],
            ..empty_config()
        };
        assert_eq!(
            args_as_strings(&common_args(&cfg)),
            vec!["--allowed-tools", "Edit"]
        );
    }

    #[test]
    fn common_args_mcp_config_emits_variadic_with_strict() {
        let cfg = SpawnConfig {
            mcp_config: vec!["a.json".to_string(), "b.json".to_string()],
            strict_mcp_config: true,
            ..empty_config()
        };
        assert_eq!(
            args_as_strings(&common_args(&cfg)),
            vec!["--mcp-config", "a.json", "b.json", "--strict-mcp-config",]
        );
    }

    #[test]
    fn common_args_forwards_settings_and_sources() {
        let cfg = SpawnConfig {
            settings: Some("/tmp/settings.json".to_string()),
            setting_sources: vec!["user".to_string(), "project".to_string()],
            ..empty_config()
        };
        assert_eq!(
            args_as_strings(&common_args(&cfg)),
            vec![
                "--settings",
                "/tmp/settings.json",
                "--setting-sources",
                "user,project",
            ]
        );
    }

    #[test]
    fn common_args_setting_sources_filters_blanks() {
        let cfg = SpawnConfig {
            setting_sources: vec!["".to_string(), "  ".to_string(), "local".to_string()],
            ..empty_config()
        };
        assert_eq!(
            args_as_strings(&common_args(&cfg)),
            vec!["--setting-sources", "local"]
        );
    }

    #[test]
    fn common_args_session_resume_flags() {
        let cfg = SpawnConfig {
            session_id: Some("11111111-2222-3333-4444-555555555555".to_string()),
            resume: Some("latest".to_string()),
            continue_latest: true,
            fork_session: true,
            ..empty_config()
        };
        assert_eq!(
            args_as_strings(&common_args(&cfg)),
            vec![
                "--session-id",
                "11111111-2222-3333-4444-555555555555",
                "--resume",
                "latest",
                "--continue",
                "--fork-session",
            ]
        );
    }

    #[test]
    fn common_args_skips_blank_resume_and_session_id() {
        let cfg = SpawnConfig {
            session_id: Some("".to_string()),
            resume: Some("   ".to_string()),
            ..empty_config()
        };
        assert!(common_args(&cfg).is_empty());
    }

    #[test]
    fn common_args_forwards_plugin_dirs_and_agent() {
        let cfg = SpawnConfig {
            plugin_dirs: vec![PathBuf::from("/plug/a"), PathBuf::from("/plug/b")],
            agent: Some("reviewer".to_string()),
            ..empty_config()
        };
        assert_eq!(
            args_as_strings(&common_args(&cfg)),
            vec![
                "--agent",
                "reviewer",
                "--plugin-dir",
                "/plug/a",
                "--plugin-dir",
                "/plug/b",
            ]
        );
    }

    #[test]
    fn common_args_budget_and_persistence_flags() {
        let cfg = SpawnConfig {
            no_session_persistence: true,
            max_budget_usd: Some(2.5),
            ..empty_config()
        };
        assert_eq!(
            args_as_strings(&common_args(&cfg)),
            vec!["--no-session-persistence", "--max-budget-usd", "2.5"]
        );
    }

    #[test]
    fn common_args_skips_negative_and_nonfinite_budget() {
        for amount in [-1.0, f64::NAN, f64::INFINITY] {
            let cfg = SpawnConfig {
                max_budget_usd: Some(amount),
                ..empty_config()
            };
            assert!(
                common_args(&cfg).is_empty(),
                "amount={amount} should be skipped"
            );
        }
    }

    #[test]
    fn common_args_full_loadout_order_is_stable() {
        // Every flag set. Locks the documented order so future edits
        // can't silently reorder the spawned command line.
        let cfg = SpawnConfig {
            binary_path: PathBuf::from("claude"),
            agent_mode: true,
            bare: true,
            model: Some("sonnet".to_string()),
            append_system_prompt: Some("append".to_string()),
            system_prompt: Some("base".to_string()),
            permission_mode: Some(PermissionMode::Plan),
            effort: Some(EffortLevel::High),
            fallback_model: Some("opus".to_string()),
            add_dirs: vec![PathBuf::from("/x")],
            allowed_tools: vec!["Edit".to_string()],
            disallowed_tools: vec!["WebFetch".to_string()],
            mcp_config: vec!["mcp.json".to_string()],
            strict_mcp_config: true,
            settings: Some("settings.json".to_string()),
            setting_sources: vec!["user".to_string()],
            session_id: Some("sid-123".to_string()),
            resume: Some("latest".to_string()),
            continue_latest: true,
            fork_session: true,
            plugin_dirs: vec![PathBuf::from("/plug")],
            agent: Some("reviewer".to_string()),
            no_session_persistence: true,
            max_budget_usd: Some(3.0),
            envs: Vec::new(),
            cwd: None,
            timeout: Some(DEFAULT_TIMEOUT),
        };
        assert_eq!(
            args_as_strings(&common_args(&cfg)),
            vec![
                "--bare",
                "--dangerously-skip-permissions",
                "--permission-mode",
                "plan",
                "--model",
                "sonnet",
                "--fallback-model",
                "opus",
                "--effort",
                "high",
                "--system-prompt",
                "base",
                "--append-system-prompt",
                "append",
                "--agent",
                "reviewer",
                "--add-dir",
                "/x",
                "--allowed-tools",
                "Edit",
                "--disallowed-tools",
                "WebFetch",
                "--mcp-config",
                "mcp.json",
                "--strict-mcp-config",
                "--settings",
                "settings.json",
                "--setting-sources",
                "user",
                "--plugin-dir",
                "/plug",
                "--session-id",
                "sid-123",
                "--resume",
                "latest",
                "--continue",
                "--fork-session",
                "--no-session-persistence",
                "--max-budget-usd",
                "3",
            ]
        );
    }
}
