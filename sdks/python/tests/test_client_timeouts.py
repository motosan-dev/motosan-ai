"""E4+E8 (M3): unified timeout model, StreamReadTimeoutError, client lifecycle."""

import asyncio

import httpx
import pytest
import respx

from motosan_ai import Client, Message, Provider
from motosan_ai.error import NetworkError, StreamReadTimeoutError
from motosan_ai.providers import (
    ChatGptCodexProvider,
    GeminiCodeAssistProvider,
    GeminiProvider,
    OpenAIProvider,
)
from motosan_ai.retry import RetryPolicy
from motosan_ai.types import ChatRequest, ChatResponse, StopReason, StreamEvent, Usage

_OPENAI_URL = "https://api.openai.com/v1/chat/completions"


class _TimeoutAfterChunks(httpx.AsyncByteStream):
    """Yield the given chunks, then raise httpx.ReadTimeout (idle expiry)."""

    def __init__(self, chunks: list[bytes]) -> None:
        self._chunks = chunks

    async def __aiter__(self):
        for chunk in self._chunks:
            yield chunk
        raise httpx.ReadTimeout("read timed out")


def test_provider_timeout_kwargs_map_to_httpx_timeout():
    provider = OpenAIProvider(api_key="k", connect_timeout=1.5, read_idle_timeout=3.0)
    assert provider._http.timeout == httpx.Timeout(connect=1.5, read=3.0, write=3.0, pool=1.5)


def test_minimax_30s_outlier_is_unified(monkeypatch):
    monkeypatch.setenv("MINIMAX_API_KEY", "m")
    client = Client(Provider.minimax)
    assert client._provider._client.timeout == httpx.Timeout(
        connect=10.0, read=120.0, write=120.0, pool=10.0
    )


@respx.mock
@pytest.mark.asyncio
async def test_mid_stream_read_timeout_is_distinct_and_never_retried(monkeypatch):
    monkeypatch.setenv("OPENAI_API_KEY", "k")
    chunk = b'data: {"choices":[{"delta":{"content":"hello-world-after-buffer"}}]}\n\n'
    route = respx.post(_OPENAI_URL).mock(
        return_value=httpx.Response(
            200,
            stream=_TimeoutAfterChunks([chunk]),
            headers={"content-type": "text/event-stream"},
        )
    )
    client = Client(Provider.openai, retry_policy=RetryPolicy(max_retries=3, base_delay=0.001))
    seen = []
    with pytest.raises(StreamReadTimeoutError, match="stream read timed out after 120"):
        async for event in client.stream([Message.user("hi")]):
            seen.append(event)
    assert "hello-world" in "".join(e.content for e in seen)
    assert route.call_count == 1


# Providers whose non-2xx error-body ``await resp.aread()`` sat outside the
# ReadTimeout-mapping scope at baseline.
_ERROR_BODY_TIMEOUT_CASES = [
    pytest.param(
        lambda: GeminiProvider(api_key="k"),
        "https://generativelanguage.googleapis.com/v1beta/models/gemini-2.5-flash:streamGenerateContent?alt=sse",
        id="gemini",
    ),
    pytest.param(
        lambda: GeminiCodeAssistProvider("ya29.fake", "myproj"),
        "https://cloudcode-pa.googleapis.com/v1internal:streamGenerateContent?alt=sse",
        id="gemini_code_assist",
    ),
    pytest.param(
        lambda: ChatGptCodexProvider("tok", "acct-123", "gpt-5.5", None),
        "https://chatgpt.com/backend-api/codex/responses",
        id="chatgpt_codex",
    ),
]


@respx.mock
@pytest.mark.asyncio
@pytest.mark.parametrize(("make_provider", "url"), _ERROR_BODY_TIMEOUT_CASES)
async def test_non_2xx_error_body_read_timeout_maps_to_stream_read_timeout(make_provider, url):
    respx.post(url).mock(return_value=httpx.Response(500, stream=_TimeoutAfterChunks([b"boom"])))
    with pytest.raises(StreamReadTimeoutError, match="stream read timed out after 120"):
        async for _ in make_provider().stream(ChatRequest(messages=[Message.user("hi")])):
            pass


class _SlowProvider:
    async def chat(self, request):
        await asyncio.sleep(0.5)
        return ChatResponse(content="late", usage=Usage(1, 1), stop_reason=StopReason.stop)

    async def stream(self, request):
        await asyncio.sleep(0.1)
        yield StreamEvent(content="slow", done=False)
        yield StreamEvent(content="", done=True)

    async def aclose(self):
        pass


@pytest.mark.asyncio
async def test_total_timeout_bounds_chat(monkeypatch):
    monkeypatch.setenv("OPENAI_API_KEY", "k")
    client = Client(Provider.openai, total_timeout=0.05)
    client._provider = _SlowProvider()
    with pytest.raises(NetworkError, match="total timeout of 0.05s exceeded"):
        await client.chat([Message.user("hi")])


@pytest.mark.asyncio
async def test_total_timeout_never_applies_to_streams(monkeypatch):
    monkeypatch.setenv("OPENAI_API_KEY", "k")
    client = Client(Provider.openai, total_timeout=0.05)
    client._provider = _SlowProvider()
    events = [e async for e in client.stream([Message.user("hi")])]
    assert [e.content for e in events] == ["slow", ""]


@pytest.mark.asyncio
async def test_aclose_closes_provider_pool(monkeypatch):
    monkeypatch.setenv("OPENAI_API_KEY", "k")
    client = Client(Provider.openai)
    assert client._provider._http.is_closed is False
    await client.aclose()
    assert client._provider._http.is_closed is True


@pytest.mark.asyncio
async def test_async_context_manager_round_trip(monkeypatch):
    monkeypatch.setenv("ANTHROPIC_API_KEY", "k")
    async with Client(Provider.anthropic) as client:
        assert client._provider._http.is_closed is False
    assert client._provider._http.is_closed is True


@pytest.mark.asyncio
async def test_cli_timeout_threading():
    assert Client(Provider.codex_cli, cli_timeout=60.0)._provider._config.timeout_secs == 60.0
    assert Client(Provider.gemini_cli, cli_timeout=None)._provider._config.timeout_secs is None
    assert Client(Provider.codex_cli)._provider._config.timeout_secs == 600.0
    await Client(Provider.gemini_cli).aclose()
