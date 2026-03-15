import pytest

from motosan_ai import Client, Provider
from motosan_ai.providers.openai import OpenAIProvider
from motosan_ai.types import ChatRequest, Message, StopReason, Tool


class FakeCompletions:
    async def create(self, **kwargs):
        if kwargs.get("stream"):
            async def gen():
                yield {"choices": [{"delta": {"content": "hel"}, "finish_reason": None}]}
                yield {"choices": [{"delta": {"content": "lo"}, "finish_reason": None}]}
                yield {"choices": [{"delta": {}, "finish_reason": "stop"}]}
            return gen()
        # Check if tools were provided to return tool_calls response
        if kwargs.get("tools"):
            return {
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
            }
        return {
            "model": "llama3.2",
            "choices": [
                {
                    "message": {"content": "Hello from Ollama!", "tool_calls": None},
                    "finish_reason": "stop",
                }
            ],
            "usage": {"prompt_tokens": 5, "completion_tokens": 10},
        }


class FakeClient:
    def __init__(self):
        self.chat = type("x", (), {"completions": FakeCompletions()})


@pytest.fixture
def provider(monkeypatch):
    import motosan_ai.providers.openai as m

    class FakeAsyncOpenAI:
        def __init__(self, api_key, base_url=None):
            self.api_key = api_key
            self.base_url = base_url
            self.chat = FakeClient().chat

    monkeypatch.setitem(
        __import__("sys").modules, "openai", type("x", (), {"AsyncOpenAI": FakeAsyncOpenAI})
    )
    return OpenAIProvider(api_key="", model="llama3.2", base_url="http://localhost:11434/v1")


@pytest.mark.asyncio
async def test_ollama_chat(provider):
    resp = await provider.chat(ChatRequest(messages=[Message.user("hello")]))
    assert resp.stop_reason == StopReason.stop
    assert resp.content == "Hello from Ollama!"
    assert resp.model == "llama3.2"


@pytest.mark.asyncio
async def test_ollama_stream(provider):
    events = [e async for e in provider.stream(ChatRequest(messages=[Message.user("hi")]))]
    assert [e.content for e in events if not e.done] == ["hel", "lo"]
    assert events[-1].done is True


@pytest.mark.asyncio
async def test_ollama_tool_use(provider):
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


def test_ollama_client_factory(monkeypatch):
    import motosan_ai.providers.openai as m

    class FakeAsyncOpenAI:
        def __init__(self, api_key, base_url=None):
            self.api_key = api_key
            self.base_url = base_url
            self.chat = FakeClient().chat

    monkeypatch.setitem(
        __import__("sys").modules, "openai", type("x", (), {"AsyncOpenAI": FakeAsyncOpenAI})
    )

    client = Client.ollama()
    assert client.provider == Provider.ollama
    assert client.api_key == ""


def test_ollama_client_custom_base_url(monkeypatch):
    import motosan_ai.providers.openai as m

    class FakeAsyncOpenAI:
        def __init__(self, api_key, base_url=None):
            self.api_key = api_key
            self.base_url = base_url
            self.chat = FakeClient().chat

    monkeypatch.setitem(
        __import__("sys").modules, "openai", type("x", (), {"AsyncOpenAI": FakeAsyncOpenAI})
    )

    client = Client.ollama(base_url="http://remote:11434/v1", model="mistral")
    assert client.provider == Provider.ollama
    assert client.model == "mistral"


def test_ollama_no_api_key_required(monkeypatch):
    """Ollama should not require an API key."""
    import motosan_ai.providers.openai as m

    class FakeAsyncOpenAI:
        def __init__(self, api_key, base_url=None):
            self.chat = FakeClient().chat

    monkeypatch.setitem(
        __import__("sys").modules, "openai", type("x", (), {"AsyncOpenAI": FakeAsyncOpenAI})
    )

    # Should not raise ConfigError
    client = Client(provider=Provider.ollama)
    assert client.provider == Provider.ollama
