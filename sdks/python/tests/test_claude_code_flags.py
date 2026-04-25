from __future__ import annotations

from motosan_ai.providers.claude_code import ClaudeCodeClient


def _args(client: ClaudeCodeClient) -> list[str]:
    return client._build_args(model=None, system_prompt=None)


def test_bare_flag_appears_when_enabled():
    assert "--bare" in _args(ClaudeCodeClient().bare(True))


def test_bare_flag_absent_by_default():
    assert "--bare" not in _args(ClaudeCodeClient())


def test_system_prompt_emits_system_prompt():
    c = ClaudeCodeClient().system_prompt("be terse")
    args = _args(c)
    assert args[args.index("--system-prompt") + 1] == "be terse"


def test_permission_mode_forwarded():
    c = ClaudeCodeClient().permission_mode("plan")
    args = _args(c)
    assert args[args.index("--permission-mode") + 1] == "plan"


def test_effort_forwarded():
    c = ClaudeCodeClient().effort("high")
    args = _args(c)
    assert args[args.index("--effort") + 1] == "high"


def test_fallback_model_forwarded():
    c = ClaudeCodeClient().fallback_model("haiku")
    args = _args(c)
    assert args[args.index("--fallback-model") + 1] == "haiku"


def test_settings_forwarded():
    c = ClaudeCodeClient().settings("/path/to/settings.json")
    args = _args(c)
    assert args[args.index("--settings") + 1] == "/path/to/settings.json"


def test_agent_forwarded():
    c = ClaudeCodeClient().agent("code-reviewer")
    args = _args(c)
    assert args[args.index("--agent") + 1] == "code-reviewer"


def test_empty_system_prompt_omitted():
    assert "--system-prompt" not in _args(ClaudeCodeClient().system_prompt("   "))


def test_add_dir_appends():
    c = ClaudeCodeClient().add_dir("/a").add_dir("/b")
    args = _args(c)
    occurrences = [i for i, a in enumerate(args) if a == "--add-dir"]
    assert len(occurrences) == 2
    assert args[occurrences[0] + 1] == "/a"
    assert args[occurrences[1] + 1] == "/b"


def test_add_dirs_replaces():
    c = ClaudeCodeClient().add_dir("/old").add_dirs(["/x", "/y"])
    args = _args(c)
    assert [args[i + 1] for i, a in enumerate(args) if a == "--add-dir"] == ["/x", "/y"]


def test_allow_tool_appends_variadic():
    args = _args(ClaudeCodeClient().allow_tool("Read").allow_tool("Write"))
    i = args.index("--allowed-tools")
    assert args[i + 1 : i + 3] == ["Read", "Write"]


def test_allowed_tools_replaces_variadic():
    args = _args(ClaudeCodeClient().allow_tool("Read").allowed_tools(["Bash", "Edit"]))
    i = args.index("--allowed-tools")
    assert args[i + 1 : i + 3] == ["Bash", "Edit"]


def test_disallow_tool_appends_variadic():
    args = _args(ClaudeCodeClient().disallow_tool("Bash").disallow_tool("Write"))
    i = args.index("--disallowed-tools")
    assert args[i + 1 : i + 3] == ["Bash", "Write"]


def test_disallowed_tools_replaces_variadic():
    args = _args(ClaudeCodeClient().disallow_tool("Read").disallowed_tools(["Bash"]))
    i = args.index("--disallowed-tools")
    assert args[i + 1] == "Bash"


def test_mcp_config_appends_variadic():
    args = _args(ClaudeCodeClient().mcp_config("/path/a.json").mcp_config("/path/b.json"))
    i = args.index("--mcp-config")
    assert args[i + 1 : i + 3] == ["/path/a.json", "/path/b.json"]


def test_mcp_configs_replaces_variadic():
    args = _args(ClaudeCodeClient().mcp_config("/old").mcp_configs(["/new1", "/new2"]))
    i = args.index("--mcp-config")
    assert args[i + 1 : i + 3] == ["/new1", "/new2"]


def test_strict_mcp_config_flag():
    assert "--strict-mcp-config" in _args(ClaudeCodeClient().strict_mcp_config(True))


def test_strict_mcp_config_absent_by_default():
    assert "--strict-mcp-config" not in _args(ClaudeCodeClient())


def test_setting_source_appends():
    args = _args(ClaudeCodeClient().setting_source("user").setting_source("project"))
    assert args[args.index("--setting-sources") + 1] == "user,project"


def test_setting_sources_replaces():
    args = _args(ClaudeCodeClient().setting_source("user").setting_sources(["project", "local"]))
    assert args[args.index("--setting-sources") + 1] == "project,local"


def test_session_id_forwarded():
    args = _args(ClaudeCodeClient().session_id("abc-123"))
    assert args[args.index("--session-id") + 1] == "abc-123"


def test_resume_forwarded():
    args = _args(ClaudeCodeClient().resume("session-42"))
    assert args[args.index("--resume") + 1] == "session-42"


def test_continue_latest_flag():
    assert "--continue" in _args(ClaudeCodeClient().continue_latest(True))


def test_fork_session_flag():
    assert "--fork-session" in _args(ClaudeCodeClient().fork_session(True))


def test_no_session_persistence_flag():
    assert "--no-session-persistence" in _args(ClaudeCodeClient().no_session_persistence(True))


def test_session_flags_absent_by_default():
    args = _args(ClaudeCodeClient())
    for flag in (
        "--continue",
        "--fork-session",
        "--no-session-persistence",
        "--session-id",
        "--resume",
    ):
        assert flag not in args


def test_plugin_dir_appends():
    c = ClaudeCodeClient().plugin_dir("/p1").plugin_dir("/p2")
    args = _args(c)
    assert [args[i + 1] for i, a in enumerate(args) if a == "--plugin-dir"] == ["/p1", "/p2"]


def test_plugin_dirs_replaces():
    c = ClaudeCodeClient().plugin_dir("/old").plugin_dirs(["/x"])
    args = _args(c)
    assert [args[i + 1] for i, a in enumerate(args) if a == "--plugin-dir"] == ["/x"]


def test_max_budget_usd_forwarded():
    args = _args(ClaudeCodeClient().max_budget_usd(12.5))
    assert args[args.index("--max-budget-usd") + 1] == "12.5"


def test_max_budget_usd_absent_by_default():
    assert "--max-budget-usd" not in _args(ClaudeCodeClient())
