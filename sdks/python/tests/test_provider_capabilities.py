from __future__ import annotations

from collections.abc import AsyncIterator

import pytest

from motosan_ai.error import MotosanError
from motosan_ai.provider_base import BaseProvider, ProviderCapabilities
from motosan_ai.providers.anthropic import AnthropicProvider
from motosan_ai.providers.claude_code import ClaudeCodeClient
from motosan_ai.providers.codex_cli import CodexCliClient
from motosan_ai.providers.gemini_cli import GeminiCliClient
from motosan_ai.providers.gemini_code_assist import GeminiCodeAssistProvider
from motosan_ai.providers.minimax import MinimaxProvider
from motosan_ai.providers.ollama import OllamaProvider
from motosan_ai.providers.openai import OpenAIProvider
from motosan_ai.types import ChatRequest, ChatResponse, Message, StreamEvent


class _TextOnlyProvider(BaseProvider):
    capabilities = ProviderCapabilities.text_only()

    async def chat(self, req: ChatRequest) -> ChatResponse:  # pragma: no cover - unused
        raise NotImplementedError

    async def stream(self, req: ChatRequest) -> AsyncIterator[StreamEvent]:  # pragma: no cover
        if False:
            yield StreamEvent(content="", done=True)
        raise NotImplementedError


class _FullProvider(BaseProvider):
    capabilities = ProviderCapabilities.full()

    async def chat(self, req: ChatRequest) -> ChatResponse:  # pragma: no cover
        raise NotImplementedError

    async def stream(self, req: ChatRequest) -> AsyncIterator[StreamEvent]:  # pragma: no cover
        if False:
            yield StreamEvent(content="", done=True)
        raise NotImplementedError


def test_text_only_capabilities():
    caps = ProviderCapabilities.text_only()
    assert caps.supports_image is False
    assert caps.supports_document is False


def test_with_image_capabilities():
    caps = ProviderCapabilities.with_image()
    assert caps.supports_image is True
    assert caps.supports_document is False


def test_full_capabilities():
    caps = ProviderCapabilities.full()
    assert caps.supports_image is True
    assert caps.supports_document is True


def test_text_only_rejects_image():
    p = _TextOnlyProvider()
    req = ChatRequest(messages=[Message.user_with_image("x", "abc", "image/png")])
    with pytest.raises(MotosanError, match="image"):
        p.validate_request(req)


def test_text_only_rejects_document():
    p = _TextOnlyProvider()
    req = ChatRequest(messages=[Message.user_with_pdf_base64("x", "abc")])
    with pytest.raises(MotosanError, match="document"):
        p.validate_request(req)


def test_full_accepts_image_and_document():
    p = _FullProvider()
    img = ChatRequest(messages=[Message.user_with_image("x", "abc", "image/png")])
    doc = ChatRequest(messages=[Message.user_with_pdf_base64("x", "abc")])
    p.validate_request(img)
    p.validate_request(doc)


def test_plain_text_accepted_by_text_only():
    p = _TextOnlyProvider()
    p.validate_request(ChatRequest(messages=[Message.user("plain")]))


def test_anthropic_is_full_capability():
    p = AnthropicProvider(api_key="test")
    assert p.capabilities == ProviderCapabilities.full()


def test_openai_is_with_image():
    p = OpenAIProvider(api_key="test")
    assert p.capabilities == ProviderCapabilities.with_image()


def test_minimax_is_text_only():
    p = MinimaxProvider(api_key="test")
    assert p.capabilities == ProviderCapabilities.text_only()


def test_ollama_native_is_text_only():
    p = OllamaProvider(model="llama3.2")
    assert p.capabilities == ProviderCapabilities.text_only()


def test_claude_code_is_text_only():
    p = ClaudeCodeClient()
    assert p.capabilities == ProviderCapabilities.text_only()


def test_codex_cli_is_text_only():
    p = CodexCliClient()
    assert p.capabilities == ProviderCapabilities.text_only()


def test_gemini_cli_is_text_only():
    p = GeminiCliClient()
    assert p.capabilities == ProviderCapabilities.text_only()


def test_gemini_code_assist_is_with_image():
    p = GeminiCodeAssistProvider(access_token="ya29.fake", project_id="project")
    assert p.capabilities == ProviderCapabilities.with_image()
