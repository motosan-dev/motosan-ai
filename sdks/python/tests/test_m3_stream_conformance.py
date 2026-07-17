"""M3 milestone-Done conformance gates for Anthropic stream termination.

Cross-SDK mirrors:
- sdks/rust/tests/m3_stream_conformance.rs
- sdks/typescript/tests/m3-stream-conformance.test.ts
"""

import json

import httpx
import pytest
import respx

from motosan_ai.error import IncompleteStreamError, StreamError, StreamReadTimeoutError
from motosan_ai.providers.anthropic import AnthropicProvider
from motosan_ai.types import ChatRequest, Message


@pytest.fixture
def provider():
    return AnthropicProvider("test-key", base_url="https://mock.anthropic.com")


def _sse(*events: dict) -> str:
    return "\n".join(f"data: {json.dumps(e)}" for e in events) + "\n"


class _StallThenReadTimeout(httpx.AsyncByteStream):
    """One SSE chunk, then the socket goes silent past the read deadline."""

    def __init__(self, first_chunk: bytes) -> None:
        self._first_chunk = first_chunk

    async def __aiter__(self):
        yield self._first_chunk
        raise httpx.ReadTimeout("read timed out")


@respx.mock
@pytest.mark.asyncio
async def test_stream_eof_without_message_stop_raises_incomplete_stream(provider):
    sse = _sse(
        {"type": "message_start", "message": {"usage": {"input_tokens": 3, "output_tokens": 0}}},
        {"type": "content_block_delta", "index": 0, "delta": {"type": "text_delta", "text": "par"}},
    )
    respx.post("https://mock.anthropic.com/v1/messages").mock(
        return_value=httpx.Response(200, text=sse, headers={"content-type": "text/event-stream"})
    )

    seen = []
    with pytest.raises(
        IncompleteStreamError,
        match="incomplete stream: anthropic ended without a terminal event",
    ):
        async for event in provider.stream(ChatRequest(messages=[Message.user("hi")])):
            seen.append(event)

    assert any(event.content == "par" for event in seen)
    assert not any(event.done for event in seen)
    assert issubclass(IncompleteStreamError, StreamError)


@respx.mock
@pytest.mark.asyncio
async def test_stream_read_idle_timeout_raises_stream_read_timeout_error(provider):
    first = (
        b'data: {"type":"content_block_delta","index":0,'
        b'"delta":{"type":"text_delta","text":"par"}}\n\n'
    )
    respx.post("https://mock.anthropic.com/v1/messages").mock(
        return_value=httpx.Response(
            200,
            stream=_StallThenReadTimeout(first),
            headers={"content-type": "text/event-stream"},
        )
    )

    seen = []
    with pytest.raises(StreamReadTimeoutError):
        async for event in provider.stream(ChatRequest(messages=[Message.user("hi")])):
            seen.append(event)

    assert any(event.content == "par" for event in seen)
    assert not any(event.done for event in seen)
