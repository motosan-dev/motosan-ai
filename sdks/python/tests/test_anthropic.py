import pytest

from motosan_ai.providers.anthropic import AnthropicProvider
from motosan_ai.types import ChatRequest, Message, StopReason


class FakeMessagesAPI:
    def __init__(self):
        self.last_kwargs = None

    async def create(self, **kwargs):
        self.last_kwargs = kwargs
        if kwargs.get("stream"):
            async def gen():
                yield {"type": "content_block_delta", "delta": {"text": "hel"}}
                yield {"type": "content_block_delta", "delta": {"text": "lo"}}
                yield {"type": "message_stop"}
            return gen()
        return {
            "model": "claude-sonnet-4-5",
            "stop_reason": "tool_use",
            "usage": {"input_tokens": 1, "output_tokens": 1},
            "content": [
                {"type": "text", "text": "checking"},
                {"type": "tool_use", "id": "toolu_1", "name": "get_weather", "input": {"city": "Taipei"}},
            ],
        }


class FakeClient:
    def __init__(self):
        self.messages = FakeMessagesAPI()


@pytest.fixture
def provider(monkeypatch):
    import motosan_ai.providers.anthropic as m

    class FakeAsyncAnthropic:
        def __init__(self, api_key):
            self.messages = FakeClient().messages

    monkeypatch.setitem(__import__("sys").modules, "anthropic", type("x", (), {"AsyncAnthropic": FakeAsyncAnthropic}))
    return AnthropicProvider("test-key")


@pytest.mark.asyncio
async def test_anthropic_chat(provider):
    req = ChatRequest(messages=[Message.user("weather?")])
    resp = await provider.chat(req)
    assert resp.stop_reason == StopReason.tool_use
    assert resp.tool_calls[0].id == "toolu_1"


@pytest.mark.asyncio
async def test_anthropic_stream(provider):
    req = ChatRequest(messages=[Message.user("hi")])
    events = [e async for e in provider.stream(req)]
    assert [e.content for e in events if not e.done] == ["hel", "lo"]
    assert events[-1].done is True
