import pytest

from motosan_ai.providers.openai import OpenAIProvider
from motosan_ai.types import ChatRequest, Message, StopReason


class FakeCompletions:
    async def create(self, **kwargs):
        if kwargs.get("stream"):
            async def gen():
                yield {"choices": [{"delta": {"content": "hel"}, "finish_reason": None}]}
                yield {"choices": [{"delta": {"content": "lo"}, "finish_reason": None}]}
                yield {"choices": [{"delta": {}, "finish_reason": "stop"}]}
            return gen()
        return {
            "model": "gpt-4o",
            "choices": [
                {
                    "message": {
                        "content": "",
                        "tool_calls": [
                            {
                                "id": "call_1",
                                "function": {"name": "get_weather", "arguments": '{"city":"Taipei"}'},
                            }
                        ],
                    },
                    "finish_reason": "tool_calls",
                }
            ],
            "usage": {"prompt_tokens": 1, "completion_tokens": 1},
        }


class FakeClient:
    def __init__(self):
        self.chat = type("x", (), {"completions": FakeCompletions()})


@pytest.fixture
def provider(monkeypatch):
    import motosan_ai.providers.openai as m

    class FakeAsyncOpenAI:
        def __init__(self, api_key):
            self.chat = FakeClient().chat

    monkeypatch.setitem(__import__("sys").modules, "openai", type("x", (), {"AsyncOpenAI": FakeAsyncOpenAI}))
    return OpenAIProvider("test-key")


@pytest.mark.asyncio
async def test_openai_chat(provider):
    resp = await provider.chat(ChatRequest(messages=[Message.user("weather?")]))
    assert resp.stop_reason == StopReason.tool_use
    assert resp.tool_calls[0].name == "get_weather"


@pytest.mark.asyncio
async def test_openai_stream(provider):
    events = [e async for e in provider.stream(ChatRequest(messages=[Message.user("hi")]))]
    assert [e.content for e in events if not e.done] == ["hel", "lo"]
    assert events[-1].done is True
