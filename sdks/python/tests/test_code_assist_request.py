from __future__ import annotations

import re

from motosan_ai.providers.gemini_code_assist import (
    GeminiCodeAssistProvider,
    _gen_request_id,
    _gen_tool_call_id,
)
from motosan_ai.types import ChatRequest, Message


def test_default_model_and_base_url():
    p = GeminiCodeAssistProvider("ya29.fake", "test-project")
    assert p.model == "gemini-2.5-flash"
    assert p.base_url == "https://cloudcode-pa.googleapis.com"


def test_explicit_model_and_base_url():
    p = GeminiCodeAssistProvider(
        "ya29.fake", "test-project", model="gemini-2.5-pro", base_url="https://mock.test"
    )
    assert p.model == "gemini-2.5-pro"
    assert p.base_url == "https://mock.test"


def test_stream_url_includes_v1internal_and_alt_sse():
    p = GeminiCodeAssistProvider("ya29.fake", "test-project")
    assert p._stream_url().endswith("/v1internal:streamGenerateContent?alt=sse")


def test_auth_headers_present():
    h = GeminiCodeAssistProvider("ya29.fake", "test-project")._headers()
    assert h["authorization"] == "Bearer ya29.fake"
    assert h["user-agent"] == "google-cloud-sdk vscode_cloudshelleditor/0.1"
    assert h["x-goog-api-client"] == "gl-node/22.17.0"
    assert "ideType" in h["client-metadata"]
    assert h["content-type"] == "application/json"


def test_request_id_format_and_unique():
    a = _gen_request_id()
    b = _gen_request_id()
    assert re.fullmatch(r"motosan-\d+-\d{9}", a)
    assert a != b


def test_tool_call_id_format():
    assert re.fullmatch(r"get_weather_\d+_\d+", _gen_tool_call_id("get_weather"))


def test_envelope_wraps_inner_body():
    p = GeminiCodeAssistProvider("ya29.fake", "myproj")
    env = p._build_envelope(ChatRequest(messages=[Message.user("hi")]))
    assert env["project"] == "myproj"
    assert env["model"] == "gemini-2.5-flash"
    assert env["userAgent"] == "motosan-ai"
    assert re.fullmatch(r"motosan-\d+-\d{9}", env["requestId"])
    assert "contents" in env["request"]
    assert "generationConfig" in env["request"]


def test_envelope_per_request_model_overrides_default():
    p = GeminiCodeAssistProvider("ya29.fake", "myproj")
    env = p._build_envelope(ChatRequest(messages=[Message.user("hi")], model="gemini-2.5-pro"))
    assert env["model"] == "gemini-2.5-pro"
