import json

import httpx
import pytest
import respx

from motosan_ai.providers.openai import OpenAIProvider
from motosan_ai.types import ChatRequest, Message, StopReason, Tool


@pytest.fixture
def provider():
    return OpenAIProvider("test-key", base_url="https://mock.openai.com")


def _sse_lines(*events: dict) -> str:
    return "\n".join(f"data: {json.dumps(e)}" for e in events) + "\ndata: [DONE]\n"


# ---------------------------------------------------------------------------
# chat
# ---------------------------------------------------------------------------


@respx.mock
@pytest.mark.asyncio
async def test_openai_chat(provider):
    respx.post("https://mock.openai.com/v1/chat/completions").mock(
        return_value=httpx.Response(
            200,
            json={
                "model": "gpt-4o",
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
                "usage": {"prompt_tokens": 10, "completion_tokens": 5},
            },
        )
    )

    resp = await provider.chat(ChatRequest(messages=[Message.user("weather?")]))
    assert resp.stop_reason == StopReason.tool_use
    assert resp.tool_calls[0].name == "get_weather"
    assert resp.tool_calls[0].input == {"city": "Taipei"}


# ---------------------------------------------------------------------------
# stream
# ---------------------------------------------------------------------------


@respx.mock
@pytest.mark.asyncio
async def test_openai_stream(provider):
    sse = _sse_lines(
        {"choices": [{"delta": {"content": "hel"}, "finish_reason": None}]},
        {"choices": [{"delta": {"content": "lo"}, "finish_reason": None}]},
        {"choices": [{"delta": {}, "finish_reason": "stop"}]},
    )
    respx.post("https://mock.openai.com/v1/chat/completions").mock(
        return_value=httpx.Response(200, text=sse, headers={"content-type": "text/event-stream"})
    )

    events = [e async for e in provider.stream(ChatRequest(messages=[Message.user("hi")]))]
    assert [e.content for e in events if not e.done] == ["hel", "lo"]
    assert events[-1].done is True


@respx.mock
@pytest.mark.asyncio
async def test_openai_stream_tool_use(provider):
    sse = _sse_lines(
        {
            "choices": [
                {
                    "delta": {
                        "content": None,
                        "tool_calls": [
                            {
                                "index": 0,
                                "id": "call_1",
                                "function": {"name": "get_weather", "arguments": ""},
                            }
                        ],
                    },
                    "finish_reason": None,
                }
            ]
        },
        {
            "choices": [
                {
                    "delta": {
                        "content": None,
                        "tool_calls": [
                            {
                                "index": 0,
                                "id": None,
                                "function": {"name": None, "arguments": '{"city":'},
                            }
                        ],
                    },
                    "finish_reason": None,
                }
            ]
        },
        {
            "choices": [
                {
                    "delta": {
                        "content": None,
                        "tool_calls": [
                            {
                                "index": 0,
                                "id": None,
                                "function": {"name": None, "arguments": '"Taipei"}'},
                            }
                        ],
                    },
                    "finish_reason": None,
                }
            ]
        },
        {"choices": [{"delta": {}, "finish_reason": "tool_calls"}]},
    )
    respx.post("https://mock.openai.com/v1/chat/completions").mock(
        return_value=httpx.Response(200, text=sse, headers={"content-type": "text/event-stream"})
    )

    tools = [
        Tool(
            name="get_weather",
            description="Get weather",
            input_schema={"type": "object", "properties": {"city": {"type": "string"}}},
        )
    ]
    req = ChatRequest(messages=[Message.user("weather in Taipei?")], tools=tools)
    events = [e async for e in provider.stream(req)]

    starts = [e for e in events if e.event_type == "tool_call_start"]
    assert len(starts) == 1
    assert starts[0].tool_call_id == "call_1"
    assert starts[0].tool_call_name == "get_weather"

    args_events = [e for e in events if e.event_type == "tool_call_args"]
    full_args = "".join(e.tool_call_args_delta for e in args_events)
    assert full_args == '{"city":"Taipei"}'

    ends = [e for e in events if e.event_type == "tool_call_end"]
    assert len(ends) == 1

    assert events[-1].done is True


# ---------------------------------------------------------------------------
# Auth header
# ---------------------------------------------------------------------------


@respx.mock
@pytest.mark.asyncio
async def test_openai_uses_bearer_auth(provider):
    route = respx.post("https://mock.openai.com/v1/chat/completions").mock(
        return_value=httpx.Response(
            200,
            json={
                "model": "gpt-4o",
                "choices": [{"message": {"content": "hi"}, "finish_reason": "stop"}],
                "usage": {"prompt_tokens": 1, "completion_tokens": 1},
            },
        )
    )

    await provider.chat(ChatRequest(messages=[Message.user("hi")]))
    assert route.calls[0].request.headers["authorization"] == "Bearer test-key"


# ---------------------------------------------------------------------------
# Errors
# ---------------------------------------------------------------------------


@respx.mock
@pytest.mark.asyncio
async def test_openai_401_raises_auth_error(provider):
    respx.post("https://mock.openai.com/v1/chat/completions").mock(
        return_value=httpx.Response(401, json={"error": {"message": "invalid key"}})
    )
    from motosan_ai.error import AuthError

    with pytest.raises(AuthError):
        await provider.chat(ChatRequest(messages=[Message.user("hi")]))
