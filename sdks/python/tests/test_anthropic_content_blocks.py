import json

import httpx
import pytest
import respx

from motosan_ai.providers.anthropic import AnthropicProvider
from motosan_ai.types import ChatRequest, Message


@pytest.fixture
def provider():
    return AnthropicProvider("test-key", base_url="https://mock.anthropic.com")


def _ok_response(text: str = "ok") -> httpx.Response:
    return httpx.Response(
        200,
        json={
            "model": "claude-sonnet-4-6",
            "stop_reason": "end_turn",
            "usage": {"input_tokens": 1, "output_tokens": 1},
            "content": [{"type": "text", "text": text}],
        },
    )


@respx.mock
@pytest.mark.asyncio
async def test_user_with_image_serializes_as_blocks(provider):
    route = respx.post("https://mock.anthropic.com/v1/messages").mock(return_value=_ok_response())
    req = ChatRequest(messages=[Message.user_with_image("describe", "JVBER", "image/png")])
    await provider.chat(req)

    body = json.loads(route.calls[0].request.content)
    assert body["messages"] == [
        {
            "role": "user",
            "content": [
                {"type": "text", "text": "describe"},
                {
                    "type": "image",
                    "source": {"type": "base64", "media_type": "image/png", "data": "JVBER"},
                },
            ],
        }
    ]


@respx.mock
@pytest.mark.asyncio
async def test_user_with_pdf_base64_serializes_as_document_block(provider):
    route = respx.post("https://mock.anthropic.com/v1/messages").mock(return_value=_ok_response())
    req = ChatRequest(messages=[Message.user_with_pdf_base64("summarize", "JVBERi0xLjQK")])
    await provider.chat(req)

    body = json.loads(route.calls[0].request.content)
    assert body["messages"][0]["content"][1] == {
        "type": "document",
        "source": {
            "type": "base64",
            "media_type": "application/pdf",
            "data": "JVBERi0xLjQK",
        },
    }


@respx.mock
@pytest.mark.asyncio
async def test_user_with_pdf_url_serializes_as_url_document(provider):
    route = respx.post("https://mock.anthropic.com/v1/messages").mock(return_value=_ok_response())
    req = ChatRequest(messages=[Message.user_with_pdf_url("x", "https://x.com/d.pdf")])
    await provider.chat(req)

    body = json.loads(route.calls[0].request.content)
    assert body["messages"][0]["content"][1] == {
        "type": "document",
        "source": {"type": "url", "url": "https://x.com/d.pdf"},
    }


@respx.mock
@pytest.mark.asyncio
async def test_plain_text_user_message_unchanged_regression(provider):
    route = respx.post("https://mock.anthropic.com/v1/messages").mock(return_value=_ok_response())
    await provider.chat(ChatRequest(messages=[Message.user("hi")]))

    body = json.loads(route.calls[0].request.content)
    assert body["messages"] == [{"role": "user", "content": "hi"}]
