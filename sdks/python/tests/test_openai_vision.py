import json

import httpx
import pytest
import respx

from motosan_ai.providers.openai import OpenAIProvider
from motosan_ai.types import ChatRequest, ImageBlock, ImageSourceUrl, Message, TextBlock


@pytest.fixture
def provider():
    return OpenAIProvider("test-key", base_url="https://mock.openai.com")


def _ok() -> httpx.Response:
    return httpx.Response(
        200,
        json={
            "id": "chatcmpl-1",
            "object": "chat.completion",
            "model": "gpt-4o",
            "choices": [
                {
                    "message": {"role": "assistant", "content": "ok"},
                    "finish_reason": "stop",
                    "index": 0,
                }
            ],
            "usage": {"prompt_tokens": 1, "completion_tokens": 1, "total_tokens": 2},
        },
    )


@respx.mock
@pytest.mark.asyncio
async def test_user_with_image_base64_becomes_image_url_data_uri(provider):
    route = respx.post("https://mock.openai.com/v1/chat/completions").mock(return_value=_ok())
    req = ChatRequest(messages=[Message.user_with_image("look", "JVBER", "image/png")])
    await provider.chat(req)

    body = json.loads(route.calls[0].request.content)
    assert body["messages"][-1] == {
        "role": "user",
        "content": [
            {"type": "text", "text": "look"},
            {"type": "image_url", "image_url": {"url": "data:image/png;base64,JVBER"}},
        ],
    }


@respx.mock
@pytest.mark.asyncio
async def test_image_url_becomes_image_url_with_raw_url(provider):
    route = respx.post("https://mock.openai.com/v1/chat/completions").mock(return_value=_ok())
    msg = Message.user_with_blocks(
        [TextBlock(text="see"), ImageBlock(source=ImageSourceUrl(url="https://x.com/i.png"))]
    )
    await provider.chat(ChatRequest(messages=[msg]))

    body = json.loads(route.calls[0].request.content)
    assert body["messages"][-1]["content"] == [
        {"type": "text", "text": "see"},
        {"type": "image_url", "image_url": {"url": "https://x.com/i.png"}},
    ]


@respx.mock
@pytest.mark.asyncio
async def test_plain_text_user_unchanged(provider):
    route = respx.post("https://mock.openai.com/v1/chat/completions").mock(return_value=_ok())
    await provider.chat(ChatRequest(messages=[Message.user("hi")]))
    body = json.loads(route.calls[0].request.content)
    assert body["messages"][-1]["content"] == "hi"
