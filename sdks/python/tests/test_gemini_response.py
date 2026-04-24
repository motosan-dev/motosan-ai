import httpx
import pytest
import respx

from motosan_ai.providers.gemini import GeminiProvider
from motosan_ai.types import ChatRequest, Message, StopReason


@pytest.fixture
def provider():
    return GeminiProvider(api_key="test-key")


def _url():
    return (
        "https://generativelanguage.googleapis.com/v1beta/models/gemini-2.5-flash:generateContent"
    )


@respx.mock
@pytest.mark.asyncio
async def test_chat_parses_text_response(provider):
    respx.post(_url()).mock(
        return_value=httpx.Response(
            200,
            json={
                "candidates": [
                    {
                        "content": {"parts": [{"text": "Hello!"}], "role": "model"},
                        "finishReason": "STOP",
                    }
                ],
                "usageMetadata": {"promptTokenCount": 5, "candidatesTokenCount": 2},
                "modelVersion": "gemini-2.5-flash",
            },
        )
    )
    resp = await provider.chat(ChatRequest(messages=[Message.user("Hi")]))
    assert resp.content == "Hello!"
    assert resp.stop_reason == StopReason.end_turn
    assert resp.usage.input_tokens == 5
    assert resp.usage.output_tokens == 2
    assert resp.model == "gemini-2.5-flash"


@respx.mock
@pytest.mark.asyncio
async def test_chat_parses_function_call_as_tool_call(provider):
    respx.post(_url()).mock(
        return_value=httpx.Response(
            200,
            json={
                "candidates": [
                    {
                        "content": {
                            "parts": [
                                {
                                    "functionCall": {
                                        "name": "get_weather",
                                        "args": {"city": "Taipei"},
                                    }
                                }
                            ]
                        },
                        "finishReason": "STOP",
                    }
                ],
                "usageMetadata": {"promptTokenCount": 10, "candidatesTokenCount": 5},
                "modelVersion": "gemini-2.5-flash",
            },
        )
    )
    resp = await provider.chat(ChatRequest(messages=[Message.user("weather?")]))
    assert len(resp.tool_calls) == 1
    assert resp.tool_calls[0].name == "get_weather"
    assert resp.tool_calls[0].input == {"city": "Taipei"}
    assert resp.tool_calls[0].id
    assert resp.stop_reason == StopReason.tool_use


@respx.mock
@pytest.mark.asyncio
async def test_chat_max_tokens_finish_reason(provider):
    respx.post(_url()).mock(
        return_value=httpx.Response(
            200,
            json={
                "candidates": [
                    {"content": {"parts": [{"text": "trun"}]}, "finishReason": "MAX_TOKENS"}
                ],
                "usageMetadata": {"promptTokenCount": 1, "candidatesTokenCount": 100},
            },
        )
    )
    resp = await provider.chat(ChatRequest(messages=[Message.user("long")]))
    assert resp.stop_reason == StopReason.max_tokens


@respx.mock
@pytest.mark.asyncio
async def test_chat_sends_x_goog_api_key_header(provider):
    route = respx.post(_url()).mock(
        return_value=httpx.Response(
            200,
            json={
                "candidates": [{"content": {"parts": [{"text": "hi"}]}, "finishReason": "STOP"}],
                "usageMetadata": {},
            },
        )
    )
    await provider.chat(ChatRequest(messages=[Message.user("hi")]))
    assert route.calls[0].request.headers["x-goog-api-key"] == "test-key"
