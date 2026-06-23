import pytest
import respx
from httpx import Response

from motosan_ai.error import StreamError
from motosan_ai.providers.minimax import MinimaxProvider
from motosan_ai.types import ChatRequest, Message, StopReason


@pytest.mark.asyncio
@respx.mock
async def test_minimax_chat():
    route = respx.post("https://api.minimax.chat/v1/text/chatcompletion_v2").mock(
        return_value=Response(
            200,
            json={
                "model": "MiniMax-Text-01",
                "choices": [
                    {
                        "message": {
                            "content": "",
                            "tool_calls": [
                                {
                                    "id": "call_1",
                                    "function": {
                                        "name": "get_weather",
                                        "arguments": '{"city":"Taipei"}',
                                    },
                                }
                            ],
                        },
                        "finish_reason": "tool_calls",
                    }
                ],
                "usage": {"prompt_tokens": 1, "completion_tokens": 1},
            },
        )
    )
    provider = MinimaxProvider("test-key")
    resp = await provider.chat(ChatRequest(messages=[Message.user("weather")]))
    assert route.called
    assert resp.stop_reason == StopReason.tool_use


@pytest.mark.asyncio
@respx.mock
async def test_minimax_stream():
    sse = "\n".join(
        [
            'data: {"choices":[{"delta":{"content":"hel"}}]}',
            'data: {"choices":[{"delta":{"content":"lo"}}]}',
            "data: [DONE]",
            "",
        ]
    )
    route = respx.post("https://api.minimax.chat/v1/text/chatcompletion_v2").mock(
        return_value=Response(200, text=sse, headers={"content-type": "text/event-stream"})
    )
    provider = MinimaxProvider("test-key")
    events = [e async for e in provider.stream(ChatRequest(messages=[Message.user("hi")]))]
    assert route.called
    assert [e.content for e in events if not e.done] == ["hel", "lo"]
    assert events[-1].done is True


@respx.mock
@pytest.mark.asyncio
async def test_stream_raises_stream_error_not_network_error():
    sse = 'data: {"choices":[{"delta":{"content":"hi"}}]}\ndata: {not valid json\n'
    respx.post("https://api.minimax.chat/v1/text/chatcompletion_v2").mock(
        return_value=Response(200, text=sse, headers={"content-type": "text/event-stream"})
    )
    provider = MinimaxProvider("test-key")
    seen = []
    with pytest.raises(StreamError, match="malformed SSE chunk"):
        async for ev in provider.stream(ChatRequest(messages=[Message.user("hi")])):
            seen.append(ev)
    assert any(e.content == "hi" for e in seen)
