from __future__ import annotations

from motosan_ai import Client, Provider
from motosan_ai.providers.claude_code import ClaudeCodeClient


def test_provider_enum_has_claude_code():
    assert Provider.claude_code == "claude_code"


def test_claude_code_does_not_require_api_key(monkeypatch):
    for env in ("ANTHROPIC_API_KEY", "CLAUDE_CODE_PATH"):
        monkeypatch.delenv(env, raising=False)
    client = Client(provider=Provider.claude_code)
    assert isinstance(client._provider, ClaudeCodeClient)
    assert client.api_key == ""


def test_claude_code_routing_with_explicit_binary_path():
    client = Client(provider="claude_code", binary_path="/opt/claude")
    assert isinstance(client._provider, ClaudeCodeClient)
    assert client._provider._config.binary_path == "/opt/claude"


def test_claude_code_path_env_var_resolved(monkeypatch):
    monkeypatch.setenv("CLAUDE_CODE_PATH", "/env/claude")
    client = Client(provider=Provider.claude_code)
    assert client._provider._config.binary_path == "/env/claude"


def test_claude_code_routing_cli_timeout_passthrough():
    client = Client(provider=Provider.claude_code, cli_timeout=5.0)
    assert client._provider._config.timeout_secs == 5.0


def test_claude_code_routing_cli_timeout_none_disables():
    client = Client(provider=Provider.claude_code, cli_timeout=None)
    assert client._provider._config.timeout_secs is None


def test_claude_code_routing_default_timeout_preserved():
    client = Client(provider=Provider.claude_code)
    assert client._provider._config.timeout_secs == 300.0


def test_client_claude_code_classmethod_resolves_to_provider():
    client = Client.claude_code()
    assert client.provider == Provider.claude_code
    assert isinstance(client._provider, ClaudeCodeClient)


def test_client_claude_code_classmethod_params_pass_through():
    client = Client.claude_code(binary_path="/opt/claude", model="sonnet", cli_timeout=7.5)
    assert client._provider._config.binary_path == "/opt/claude"
    assert client.model == "sonnet"
    assert client._provider._config.timeout_secs == 7.5
