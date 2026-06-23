from __future__ import annotations

from motosan_ai import Client, Provider
from motosan_ai.providers.gemini_cli import GeminiCliClient


def test_provider_enum_has_gemini_cli():
    assert Provider.gemini_cli == "gemini_cli"


def test_client_gemini_cli_classmethod_resolves_to_provider():
    client = Client.gemini_cli()
    assert client.provider == Provider.gemini_cli
    assert isinstance(client._provider, GeminiCliClient)


def test_client_gemini_cli_with_explicit_binary_path():
    client = Client.gemini_cli(binary_path="/opt/gemini")
    assert client._provider._config.binary_path == "/opt/gemini"


def test_gemini_cli_path_env_var_resolved(monkeypatch):
    monkeypatch.setenv("GEMINI_CLI_PATH", "/env/gemini")
    client = Client.gemini_cli()
    assert client._provider._config.binary_path == "/env/gemini"


def test_gemini_cli_does_not_require_api_key(monkeypatch):
    for env in ("GEMINI_API_KEY", "GEMINI_CLI_PATH"):
        monkeypatch.delenv(env, raising=False)
    client = Client(provider=Provider.gemini_cli)
    assert isinstance(client._provider, GeminiCliClient)


def test_cwd_setter():
    client = GeminiCliClient().cwd("/work/dir")
    assert client._config.cwd == "/work/dir"


def test_cwd_default_none():
    assert GeminiCliClient()._config.cwd is None


def test_env_appends_and_envs_replaces():
    client = GeminiCliClient().env("A", "1").env("B", "2")
    assert client._child_env()["A"] == "1" and client._child_env()["B"] == "2"
    client.envs({"C": "3"})
    child = client._child_env()
    assert child["C"] == "3" and "A" not in child


def test_env_repr_redacts():
    client = GeminiCliClient().env("GEMINI_API_KEY", "sk-x")
    r = repr(client._config)
    assert "<1 redacted>" in r and "sk-x" not in r


def test_default_timeout_is_600():
    assert GeminiCliClient()._config.timeout_secs == 600.0


def test_timeout_override_and_no_timeout():
    assert GeminiCliClient().timeout(5)._config.timeout_secs == 5
    assert GeminiCliClient().no_timeout()._config.timeout_secs is None
