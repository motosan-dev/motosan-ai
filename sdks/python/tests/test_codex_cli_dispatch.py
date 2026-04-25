from __future__ import annotations

from motosan_ai import Client, Provider
from motosan_ai.providers.codex_cli import CodexCliClient


def test_provider_enum_has_codex_cli():
    assert Provider.codex_cli == "codex_cli"


def test_client_codex_cli_classmethod_resolves_to_provider():
    client = Client.codex_cli()
    assert client.provider == Provider.codex_cli
    assert isinstance(client._provider, CodexCliClient)


def test_client_codex_cli_with_explicit_binary_path():
    client = Client.codex_cli(binary_path="/opt/codex")
    assert client._provider._config.binary_path == "/opt/codex"


def test_codex_path_env_var_resolved(monkeypatch):
    monkeypatch.setenv("CODEX_PATH", "/env/codex")
    client = Client.codex_cli()
    assert client._provider._config.binary_path == "/env/codex"


def test_codex_cli_does_not_require_api_key(monkeypatch):
    for env in ("OPENAI_API_KEY", "CODEX_PATH"):
        monkeypatch.delenv(env, raising=False)
    client = Client(provider=Provider.codex_cli)
    assert isinstance(client._provider, CodexCliClient)
