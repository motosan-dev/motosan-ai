import json

import httpx
import pytest
import respx

from motosan_ai import Client, Provider
from motosan_ai.providers.ollama import OllamaProvider
from motosan_ai.types import ChatRequest, Message, StopReason, StreamEvent, Tool, ToolCall


BASE_URL = "http://localhost:11434"
CHAT_URL = f"{BASE_URL}/api/chat"


@pytest.fixture
def provider():
    return OllamaProvider(model="llama3.2", base_url=BASE_URL)


@pytest.fixture
def think_provider():
    return OllamaProvider(model="llama3.2", base_url=BASE_URL, think=True)


# --- Chat tests ---


@respx.mock
@pytest.mark.asyncio
async def test_chat_basic(provider):
    respx.post(CHAT_URL).mock(
        return_value=httpx.Response(
            200,
            json={
                "model": "llama3.2",
                "message": {"role": "assistant", "content": "Hello from Ollama!"},
                "done": True,
                "prompt_eval_count": 10,
                "eval_count": 20,
            },
        )
    )
    resp = await provider.chat(ChatRequest(messages=[Message.user("hello")]))
    assert resp.content == "Hello from Ollama!"
    assert resp.model == "llama3.2"
    assert resp.stop_reason == StopReason.stop
    assert resp.usage.input_tokens == 10
    assert resp.usage.output_tokens == 20


@respx.mock
@pytest.mark.asyncio
async def test_chat_with_system(provider):
    route = respx.post(CHAT_URL).mock(
        return_value=httpx.Response(
            200,
            json={
                "model": "llama3.2",
                "message": {"role": "assistant", "content": "I am helpful."},
                "done": True,
            },
        )
    )
    await provider.chat(
        ChatRequest(messages=[Message.user("hi")], system="You are helpful.")
    )
    body = json.loads(route.calls[0].request.content)
    assert body["messages"][0] == {"role": "system", "content": "You are helpful."}


@respx.mock
@pytest.mark.asyncio
async def test_chat_tool_calls(provider):
    respx.post(CHAT_URL).mock(
        return_value=httpx.Response(
            200,
            json={
                "model": "llama3.2",
                "message": {
                    "role": "assistant",
                    "content": "",
                    "tool_calls": [
                        {
                            "function": {
                                "name": "get_weather",
                                "arguments": {"city": "Taipei"},
                            }
                        }
                    ],
                },
                "done": True,
            },
        )
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
    assert len(resp.tool_calls) == 1
    assert resp.tool_calls[0].name == "get_weather"
    assert resp.tool_calls[0].input == {"city": "Taipei"}
    # Ollama has no id — we generate uuid
    assert len(resp.tool_calls[0].id) > 0


@respx.mock
@pytest.mark.asyncio
async def test_chat_tool_calls_string_arguments(provider):
    """Tool call arguments may come as a JSON string."""
    respx.post(CHAT_URL).mock(
        return_value=httpx.Response(
            200,
            json={
                "model": "llama3.2",
                "message": {
                    "role": "assistant",
                    "content": "",
                    "tool_calls": [
                        {
                            "function": {
                                "name": "search",
                                "arguments": '{"q": "test"}',
                            }
                        }
                    ],
                },
                "done": True,
            },
        )
    )
    resp = await provider.chat(ChatRequest(messages=[Message.user("search")]))
    assert resp.tool_calls[0].input == {"q": "test"}


# --- Think mode tests ---


@respx.mock
@pytest.mark.asyncio
async def test_think_mode_request_body(think_provider):
    route = respx.post(CHAT_URL).mock(
        return_value=httpx.Response(
            200,
            json={
                "model": "llama3.2",
                "message": {"role": "assistant", "content": "result"},
                "done": True,
            },
        )
    )
    await think_provider.chat(ChatRequest(messages=[Message.user("think")]))
    body = json.loads(route.calls[0].request.content)
    assert body["think"] is True


# --- Stream tests ---


@respx.mock
@pytest.mark.asyncio
async def test_stream_basic(provider):
    ndjson = (
        '{"message":{"content":"hel"},"done":false}\n'
        '{"message":{"content":"lo"},"done":false}\n'
        '{"done":true}\n'
    )
    respx.post(CHAT_URL).mock(
        return_value=httpx.Response(200, text=ndjson)
    )
    events = [e async for e in provider.stream(ChatRequest(messages=[Message.user("hi")]))]
    texts = [e.content for e in events if not e.done]
    assert texts == ["hel", "lo"]
    assert events[-1].done is True


@respx.mock
@pytest.mark.asyncio
async def test_stream_thinking(think_provider):
    ndjson = (
        '{"message":{"thinking":"let me think..."},"done":false}\n'
        '{"message":{"content":"answer"},"done":false}\n'
        '{"done":true}\n'
    )
    respx.post(CHAT_URL).mock(
        return_value=httpx.Response(200, text=ndjson)
    )
    events = [e async for e in think_provider.stream(ChatRequest(messages=[Message.user("hi")]))]
    texts = [e.content for e in events if not e.done]
    assert texts == ["let me think...", "answer"]


# --- Build body tests ---


def test_build_body_keep_alive():
    p = OllamaProvider(keep_alive="5m")
    body = p._build_body(ChatRequest(messages=[Message.user("hi")]))
    assert body["keep_alive"] == "5m"


def test_build_body_num_ctx():
    p = OllamaProvider(num_ctx=4096)
    body = p._build_body(ChatRequest(messages=[Message.user("hi")]))
    assert body["options"]["num_ctx"] == 4096


def test_build_body_temperature():
    p = OllamaProvider()
    body = p._build_body(ChatRequest(messages=[Message.user("hi")], temperature=0.5))
    assert body["options"]["temperature"] == 0.5


def test_build_body_tools():
    p = OllamaProvider()
    tools = [Tool(name="fn", description="desc", input_schema={"type": "object"})]
    body = p._build_body(ChatRequest(messages=[Message.user("hi")], tools=tools))
    assert len(body["tools"]) == 1
    assert body["tools"][0]["function"]["name"] == "fn"


# --- Message serialization tests ---


def test_serialize_messages_with_tool_calls():
    tc = ToolCall(id="123", name="fn", input={"a": 1})
    msgs = [
        Message.user("hi"),
        Message.assistant_with_tool_calls("", [tc]),
        Message.tool_result("123", "result"),
    ]
    out = OllamaProvider._serialize_messages(msgs)
    assert out[0] == {"role": "user", "content": "hi"}
    # Ollama native: no "id" or "type" in tool_calls, arguments is dict not JSON string
    assert out[1]["tool_calls"][0]["function"]["arguments"] == {"a": 1}
    # Ollama native: tool result has no tool_call_id
    assert "tool_call_id" not in out[2]
    assert out[2] == {"role": "tool", "content": "result"}


# --- Client factory tests ---


def test_client_ollama_native_factory():
    client = Client.ollama(native=True)
    assert client.provider == Provider.ollama
    assert isinstance(client._provider, OllamaProvider)


def test_client_ollama_native_with_options():
    client = Client.ollama(
        native=True,
        model="mistral",
        base_url="http://remote:11434",
        think=True,
        keep_alive="10m",
        num_ctx=8192,
    )
    assert isinstance(client._provider, OllamaProvider)
    assert client._provider.model == "mistral"
    assert client._provider.base_url == "http://remote:11434"
    assert client._provider.think is True
    assert client._provider.keep_alive == "10m"
    assert client._provider.num_ctx == 8192


def test_client_ollama_compat_still_works(monkeypatch):
    """native=False (default) should still use OpenAIProvider."""
    import motosan_ai.providers.openai as m

    class FakeAsyncOpenAI:
        def __init__(self, api_key, base_url=None):
            self.chat = type("x", (), {"completions": type("c", (), {"create": None})})

    monkeypatch.setitem(
        __import__("sys").modules, "openai", type("x", (), {"AsyncOpenAI": FakeAsyncOpenAI})
    )

    from motosan_ai.providers.openai import OpenAIProvider
    client = Client.ollama()
    assert isinstance(client._provider, OpenAIProvider)


# --- Error handling ---


@respx.mock
@pytest.mark.asyncio
async def test_chat_error_response(provider):
    respx.post(CHAT_URL).mock(
        return_value=httpx.Response(500, text="internal error")
    )
    with pytest.raises(Exception, match="Ollama error 500"):
        await provider.chat(ChatRequest(messages=[Message.user("hi")]))
