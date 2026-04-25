from __future__ import annotations

import pytest

from motosan_ai import Client, Provider
from motosan_ai.error import ConfigError
from motosan_ai.providers.gemini_code_assist import GeminiCodeAssistProvider


def test_provider_enum_has_gemini_code_assist():
    assert Provider.gemini_code_assist == "gemini_code_assist"


def test_client_gemini_code_assist_classmethod():
    c = Client.gemini_code_assist(access_token="ya29.fake", project_id="myproj")
    assert c.provider == Provider.gemini_code_assist
    assert isinstance(c._provider, GeminiCodeAssistProvider)
    assert c._provider.access_token == "ya29.fake"
    assert c._provider.project_id == "myproj"


def test_client_gemini_code_assist_requires_project_id():
    with pytest.raises(ConfigError, match="project_id"):
        Client.gemini_code_assist(access_token="ya29.fake", project_id=None)


def test_client_gemini_code_assist_requires_access_token():
    with pytest.raises(ConfigError, match="access_token"):
        Client.gemini_code_assist(access_token=None, project_id="myproj")
