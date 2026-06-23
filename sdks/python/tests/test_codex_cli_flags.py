from __future__ import annotations

import os

from motosan_ai.providers.codex_cli import CodexCliClient, LocalProvider, SandboxMode


def _args(client: CodexCliClient) -> list[str]:
    return client._build_args()


def test_default_binary_path_is_codex_or_env():
    c = CodexCliClient()
    assert c._config.binary_path in ("codex", os.environ.get("CODEX_PATH", "codex"))


def test_explicit_binary_path():
    c = CodexCliClient(binary_path="/opt/codex")
    assert c._config.binary_path == "/opt/codex"


def test_with_path_classmethod():
    c = CodexCliClient.with_path("/opt/codex-2")
    assert c._config.binary_path == "/opt/codex-2"


def test_minimal_args_emit_exec_json_skip_check():
    c = CodexCliClient(binary_path="codex")
    assert _args(c) == ["codex", "exec", "--json", "--skip-git-repo-check", "-"]


def test_sandbox_mode_values_match_cli_flags():
    assert SandboxMode.read_only.value == "read-only"
    assert SandboxMode.workspace_write.value == "workspace-write"
    assert SandboxMode.danger_full_access.value == "danger-full-access"


def test_local_provider_values_match_cli_flags():
    assert LocalProvider.lmstudio.value == "lmstudio"
    assert LocalProvider.ollama.value == "ollama"


def test_agent_mode_emits_full_auto():
    assert "--full-auto" in _args(CodexCliClient(binary_path="codex").agent_mode(True))


def test_agent_mode_absent_by_default():
    assert "--full-auto" not in _args(CodexCliClient(binary_path="codex"))


def test_dangerously_bypass_flag():
    c = CodexCliClient(binary_path="codex").dangerously_bypass_approvals_and_sandbox(True)
    assert "--dangerously-bypass-approvals-and-sandbox" in _args(c)


def test_oss_flag():
    assert "--oss" in _args(CodexCliClient(binary_path="codex").oss(True))


def test_ephemeral_flag():
    assert "--ephemeral" in _args(CodexCliClient(binary_path="codex").ephemeral(True))


def test_sandbox_flag_with_mode():
    args = _args(CodexCliClient(binary_path="codex").sandbox(SandboxMode.workspace_write))
    assert args[args.index("--sandbox") + 1] == "workspace-write"


def test_local_provider_flag():
    args = _args(CodexCliClient(binary_path="codex").local_provider(LocalProvider.ollama))
    assert args[args.index("--local-provider") + 1] == "ollama"


def test_model_flag_emitted():
    args = _args(CodexCliClient(binary_path="codex").model("gpt-5.1-codex"))
    assert args[args.index("--model") + 1] == "gpt-5.1-codex"


def test_model_default_sentinel_skipped():
    for sentinel in ("", "   ", "default", "Default", "DEFAULT"):
        c = CodexCliClient(binary_path="codex").model(sentinel)
        assert "--model" not in _args(c), f"unexpected --model for {sentinel!r}"


def test_request_empty_model_overrides_client_model_and_skips():
    c = CodexCliClient(binary_path="codex").model("gpt-5.1-codex")
    args = c._build_args(model="")
    assert "--model" not in args


def test_profile_flag_emitted():
    args = _args(CodexCliClient(binary_path="codex").profile("work"))
    assert args[args.index("--profile") + 1] == "work"


def test_profile_empty_skipped():
    assert "--profile" not in _args(CodexCliClient(binary_path="codex").profile("   "))


def test_cd_flag_emitted():
    args = _args(CodexCliClient(binary_path="codex").cd("/tmp/project"))
    assert args[args.index("--cd") + 1] == "/tmp/project"


def test_add_dir_appends():
    c = CodexCliClient(binary_path="codex").add_dir("/tmp/a").add_dir("/tmp/b")
    args = _args(c)
    occ = [args[i + 1] for i, a in enumerate(args) if a == "--add-dir"]
    assert occ == ["/tmp/a", "/tmp/b"]


def test_enable_feature_appends():
    c = CodexCliClient(binary_path="codex").enable_feature("foo").enable_feature("bar")
    args = _args(c)
    occ = [args[i + 1] for i, a in enumerate(args) if a == "--enable"]
    assert occ == ["foo", "bar"]


def test_disable_feature_appends():
    args = _args(CodexCliClient(binary_path="codex").disable_feature("baz"))
    occ = [args[i + 1] for i, a in enumerate(args) if a == "--disable"]
    assert occ == ["baz"]


def test_empty_feature_skipped():
    assert "--enable" not in _args(CodexCliClient(binary_path="codex").enable_feature(""))


def test_whitespace_feature_passes_through():
    args = _args(CodexCliClient(binary_path="codex").disable_feature("   "))
    assert "--disable" in args
    assert args[args.index("--disable") + 1] == "   "


def test_config_override_emits_minus_c():
    c = (
        CodexCliClient(binary_path="codex")
        .config_override("model_reasoning_effort", "high")
        .config_override("approval_policy", "never")
    )
    args = _args(c)
    occ = [args[i + 1] for i, a in enumerate(args) if a == "-c"]
    assert occ == ["model_reasoning_effort=high", "approval_policy=never"]


def test_config_override_handles_special_chars_in_value():
    args = _args(CodexCliClient(binary_path="codex").config_override("env.MY_VAR", 'a "b" c'))
    assert args[args.index("-c") + 1] == 'env.MY_VAR=a "b" c'


def test_full_config_argv_order_matches_rust_common_args():
    c = (
        CodexCliClient(binary_path="codex")
        .agent_mode(True)
        .dangerously_bypass_approvals_and_sandbox(True)
        .sandbox(SandboxMode.workspace_write)
        .oss(True)
        .local_provider(LocalProvider.ollama)
        .model("gpt-5.1-codex")
        .profile("work")
        .cd("/tmp/proj")
        .add_dir("/tmp/extra1")
        .add_dir("/tmp/extra2")
        .ephemeral(True)
        .enable_feature("foo")
        .disable_feature("bar")
        .config_override("approval_policy", "never")
    )
    assert _args(c) == [
        "codex",
        "exec",
        "--json",
        "--skip-git-repo-check",
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
        "/tmp/proj",
        "--add-dir",
        "/tmp/extra1",
        "--add-dir",
        "/tmp/extra2",
        "--ephemeral",
        "--enable",
        "foo",
        "--disable",
        "bar",
        "-c",
        "approval_policy=never",
        "-",
    ]


def test_codex_has_no_os_cwd_setter():
    # Codex's working-directory mechanism is the --cd argv flag (cd()),
    # not asyncio create_subprocess_exec(cwd=). Confirm no cwd() builder.
    assert not hasattr(CodexCliClient(binary_path="codex"), "cwd")


def test_resume_inserts_exec_resume_subcommand():
    args = CodexCliClient(binary_path="codex").resume("th_abc")._build_args()
    assert args[:4] == ["codex", "exec", "resume", "th_abc"]
    assert "--json" in args


def test_resume_blank_skipped():
    args = CodexCliClient(binary_path="codex").resume("   ")._build_args()
    assert "resume" not in args


def test_env_appends_and_envs_replaces():
    client = CodexCliClient(binary_path="codex").env("A", "1").env("B", "2")
    assert list(client._config.envs) == [("A", "1"), ("B", "2")]
    client.envs({"C": "3"})
    assert list(client._config.envs) == [("C", "3")]


def test_env_repr_redacts_secret():
    client = CodexCliClient(binary_path="codex").env("OPENAI_API_KEY", "sk-secret")
    r = repr(client._config)
    assert "<1 redacted>" in r and "sk-secret" not in r


def test_env_not_in_argv():
    args = CodexCliClient(binary_path="codex").env("OPENAI_API_KEY", "sk-secret")._build_args()
    assert all("sk-secret" not in a for a in args)
