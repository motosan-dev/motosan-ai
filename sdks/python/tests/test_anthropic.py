import pytest

from motosan_ai.providers.anthropic import AnthropicProvider
from motosan_ai.types import ChatRequest, Message, StopReason, Tool


class FakeMessagesAPI:
    def __init__(self):
        self.last_kwargs = None

    async def create(self, **kwargs):
        self.last_kwargs = kwargs
        if kwargs.get("stream"):
            if kwargs.get("tools"):
                async def tool_gen():
                    yield {"type": "content_block_start", "index": 0, "content_block": {"type": "text", "text": ""}}
                    yield {"type": "content_block_delta", "index": 0, "delta": {"type": "text_delta", "text": "Let me check"}}
                    yield {"type": "content_block_stop", "index": 0}
                    yield {"type": "content_block_start", "index": 1, "content_block": {"type": "tool_use", "id": "toolu_1", "name": "get_weather"}}
                    yield {"type": "content_block_delta", "index": 1, "delta": {"type": "input_json_delta", "partial_json": '{"city":'}}
                    yield {"type": "content_block_delta", "index": 1, "delta": {"type": "input_json_delta", "partial_json": '"Taipei"}'}}
                    yield {"type": "content_block_stop", "index": 1}
                    yield {"type": "message_stop"}
                return tool_gen()
            async def gen():
                yield {"type": "content_block_delta", "index": 0, "delta": {"type": "text_delta", "text": "hel"}}
                yield {"type": "content_block_delta", "index": 0, "delta": {"type": "text_delta", "text": "lo"}}
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


@pytest.mark.asyncio
async def test_anthropic_stream_tool_use(provider):
    tools = [Tool(name="get_weather", description="Get weather", input_schema={"type": "object", "properties": {"city": {"type": "string"}}})]
    req = ChatRequest(messages=[Message.user("weather in Taipei?")], tools=tools)
    events = [e async for e in provider.stream(req)]

    # Should have text events
    text_events = [e for e in events if e.event_type == "text" and not e.done]
    assert any(e.content == "Let me check" for e in text_events)

    # Should have tool_call_start
    starts = [e for e in events if e.event_type == "tool_call_start"]
    assert len(starts) == 1
    assert starts[0].tool_call_id == "toolu_1"
    assert starts[0].tool_call_name == "get_weather"

    # Should have tool_call_args — with correct id
    args_events = [e for e in events if e.event_type == "tool_call_args"]
    assert len(args_events) == 2
    full_args = "".join(e.tool_call_args_delta for e in args_events)
    assert full_args == '{"city":"Taipei"}'
    assert args_events[0].tool_call_id == "toolu_1"
    assert args_events[1].tool_call_id == "toolu_1"

    # Should have tool_call_end — with correct id
    ends = [e for e in events if e.event_type == "tool_call_end"]
    assert len(ends) == 1
    assert ends[0].tool_call_id == "toolu_1"

    # Should end with done=True
    assert events[-1].done is True

    # Verify tools were sent in the request
    assert "tools" in provider._client.messages.last_kwargs
