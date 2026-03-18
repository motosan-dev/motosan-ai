import json

import httpx
import pytest
import respx

from motosan_ai import Client, Provider
from motosan_ai.providers.openai import OpenAIProvider
from motosan_ai.types import ChatRequest, Message, StopReason, Tool


@pytest.fixture
def provider():
    return OpenAIProvider(api_key="", model="llama3.2", base_url="http://localhost:11434")


def _sse_lines(*events: dict) -> str:
    return "\n".join(f"data: {json.dumps(e)}" for e in events) + "\ndata: [DONE]\n"


@respx.mock
@pytest.mark.asyncio
async def test_ollama_chat(provider):
    respx.post("http://localhost:11434/v1/chat/completions").mock(
        return_value=httpx.Response(200, json={
            "model": "llama3.2",
            "choices": [
                {
                    "message": {"content": "Hello from Ollama!", "tool_calls": None},
                    "finish_reason": "stop",
                }
            ],
            "usage": {"prompt_tokens": 5, "completion_tokens": 10},
        })
    )

    resp = await provider.chat(ChatRequest(messages=[Message.user("hello")]))
    assert resp.stop_reason == StopReason.stop
    assert resp.content == "Hello from Ollama!"
    assert resp.model == "llama3.2"


@respx.mock
@pytest.mark.asyncio
async def test_ollama_stream(provider):
    sse = _sse_lines(
        {"choices": [{"delta": {"content": "hel"}, "finish_reason": None}]},
        {"choices": [{"delta": {"content": "lo"}, "finish_reason": None}]},
        {"choices": [{"delta": {}, "finish_reason": "stop"}]},
    )
    respx.post("http://localhost:11434/v1/chat/completions").mock(
        return_value=httpx.Response(200, text=sse, headers={"content-type": "text/event-stream"})
    )

    events = [e async for e in provider.stream(ChatRequest(messages=[Message.user("hi")]))]
    assert [e.content for e in events if not e.done] == ["hel", "lo"]
    assert events[-1].done is True


@respx.mock
@pytest.mark.asyncio
async def test_ollama_tool_use(provider):
    respx.post("http://localhost:11434/v1/chat/completions").mock(
        return_value=httpx.Response(200, json={
            "model": "llama3.2",
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
            "usage": {"prompt_tokens": 5, "completion_tokens": 10},
        })
    )

    tools = [
        Tool(
            name="get_weather",
            description="Get weather",
            input_schema={"type": "object", "properties": {"city": {"type": "string"}}},
        )
    ]
    resp = await provider.chat(ChatRequest(messages=[Message.user("weather?")], tools=tools))
    assert resp.stop_reason == StopReason.tool_use
    assert resp.tool_calls[0].name == "get_weather"
    assert resp.tool_calls[0].input == {"city": "Taipei"}


def test_ollama_client_factory():
    client = Client.ollama()
    assert client.provider == Provider.ollama
    assert client.api_key == ""


def test_ollama_client_custom_base_url():
    client = Client.ollama(base_url="http://remote:11434/v1", model="mistral")
    assert client.provider == Provider.ollama
    assert client.model == "mistral"


def test_ollama_no_api_key_required():
    """Ollama should not require an API key."""
    client = Client(provider=Provider.ollama)
    assert client.provider == Provider.ollama
