import pytest

from motosan_ai.providers.openai import OpenAIProvider
from motosan_ai.types import ChatRequest, Message, StopReason, Tool


class FakeCompletions:
    def __init__(self):
        self.last_kwargs = None

    async def create(self, **kwargs):
        self.last_kwargs = kwargs
        if kwargs.get("stream"):
            if kwargs.get("tools"):
                async def tool_gen():
                    yield {"choices": [{"delta": {"content": None, "tool_calls": [{"index": 0, "id": "call_1", "function": {"name": "get_weather", "arguments": ""}}]}, "finish_reason": None}]}
                    yield {"choices": [{"delta": {"content": None, "tool_calls": [{"index": 0, "id": None, "function": {"name": None, "arguments": '{"city":'}}]}, "finish_reason": None}]}
                    yield {"choices": [{"delta": {"content": None, "tool_calls": [{"index": 0, "id": None, "function": {"name": None, "arguments": '"Taipei"}'}}]}, "finish_reason": None}]}
                    yield {"choices": [{"delta": {}, "finish_reason": "tool_calls"}]}
                return tool_gen()
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
        self._completions = FakeCompletions()
        self.chat = type("x", (), {"completions": self._completions})


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


@pytest.mark.asyncio
async def test_openai_stream_tool_use(provider):
    tools = [Tool(name="get_weather", description="Get weather", input_schema={"type": "object", "properties": {"city": {"type": "string"}}})]
    req = ChatRequest(messages=[Message.user("weather in Taipei?")], tools=tools)
    events = [e async for e in provider.stream(req)]

    # Should have tool_call_start
    starts = [e for e in events if e.event_type == "tool_call_start"]
    assert len(starts) == 1
    assert starts[0].tool_call_id == "call_1"
    assert starts[0].tool_call_name == "get_weather"

    # Should have tool_call_args
    args_events = [e for e in events if e.event_type == "tool_call_args"]
    full_args = "".join(e.tool_call_args_delta for e in args_events)
    assert full_args == '{"city":"Taipei"}'

    # Should have tool_call_end
    ends = [e for e in events if e.event_type == "tool_call_end"]
    assert len(ends) == 1

    # Should end with done=True
    assert events[-1].done is True

    # Verify tools were sent in the request
    assert "tools" in provider._client.chat.completions.last_kwargs
