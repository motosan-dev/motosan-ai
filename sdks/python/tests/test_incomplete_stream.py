"""M3 stream-termination contract: EOF without the provider terminal event.

Every HTTP provider stream() must raise IncompleteStreamError (a StreamError
subclass - deliberate migration softener so existing ``except StreamError``
handlers keep catching truncation) when the upstream body ends without that
provider's terminal event. OpenAI wire (openai, minimax): [DONE] or a
finish_reason-bearing chunk - either suffices (amended 2026-07-17), so only
EOF with NEITHER signal raises. CLI providers are intentionally not covered:
since M1 they raise StreamError with returncode/stderr on child-process death.
"""

from __future__ import annotations

import json

import httpx
import pytest
import respx

from motosan_ai._stream_collect import collect_stream
from motosan_ai.error import IncompleteStreamError, StreamError
from motosan_ai.providers.anthropic import AnthropicProvider
from motosan_ai.providers.chatgpt_codex import ChatGptCodexProvider
from motosan_ai.providers.gemini import GeminiProvider
from motosan_ai.providers.gemini_code_assist import GeminiCodeAssistProvider
from motosan_ai.providers.minimax import MinimaxProvider
from motosan_ai.providers.ollama import OllamaProvider
from motosan_ai.providers.openai import OpenAIProvider
from motosan_ai.types import ChatRequest, Message, StopReason, StreamEvent


def _sse(*events: dict) -> str:
    return "\n".join(f"data: {json.dumps(e)}" for e in events) + "\n"


# Each case: provider factory, mocked endpoint, body ending after one text
# delta with NO terminal frame, and the <provider> token in the message.
_TRUNCATED = [
    pytest.param(
        lambda: AnthropicProvider("test-key", base_url="https://mock.anthropic.com"),
        "https://mock.anthropic.com/v1/messages",
        _sse(
            {
                "type": "content_block_delta",
                "index": 0,
                "delta": {"type": "text_delta", "text": "par"},
            }
        ),
        "anthropic",
        id="anthropic",
    ),
    pytest.param(
        lambda: OpenAIProvider("test-key", base_url="https://mock.openai.com"),
        "https://mock.openai.com/v1/chat/completions",
        _sse({"choices": [{"delta": {"content": "par"}, "finish_reason": None}]}),
        "openai",
        id="openai",
    ),
    pytest.param(
        lambda: MinimaxProvider("test-key"),
        "https://api.minimax.chat/v1/text/chatcompletion_v2",
        _sse({"choices": [{"delta": {"content": "par"}}]}),
        "minimax",
        id="minimax",
    ),
    pytest.param(
        lambda: GeminiProvider(api_key="test-key"),
        "https://generativelanguage.googleapis.com/v1beta/models/gemini-2.5-flash:streamGenerateContent?alt=sse",
        _sse({"candidates": [{"content": {"parts": [{"text": "par"}]}}]}),
        "gemini",
        id="gemini",
    ),
    pytest.param(
        lambda: GeminiCodeAssistProvider("ya29.fake", "myproj"),
        "https://cloudcode-pa.googleapis.com/v1internal:streamGenerateContent?alt=sse",
        _sse({"response": {"candidates": [{"content": {"parts": [{"text": "par"}]}}]}}),
        "gemini_code_assist",
        id="gemini_code_assist",
    ),
    pytest.param(
        lambda: ChatGptCodexProvider("tok", "acct-123", "gpt-5.5", None),
        "https://chatgpt.com/backend-api/codex/responses",
        _sse({"type": "response.output_text.delta", "delta": "par"}),
        "chatgpt_codex",
        id="chatgpt_codex",
    ),
    pytest.param(
        lambda: OllamaProvider(),
        "http://localhost:11434/api/chat",
        '{"message":{"content":"par"},"done":false}\n',
        "ollama",
        id="ollama",
    ),
]


@respx.mock
@pytest.mark.asyncio
@pytest.mark.parametrize(("make_provider", "url", "body", "name"), _TRUNCATED)
async def test_stream_eof_without_terminal_event_raises(make_provider, url, body, name):
    respx.post(url).mock(
        return_value=httpx.Response(200, text=body, headers={"content-type": "text/event-stream"})
    )
    seen = []
    with pytest.raises(
        IncompleteStreamError,
        match=f"incomplete stream: {name} ended without a terminal event",
    ):
        async for event in make_provider().stream(ChatRequest(messages=[Message.user("hi")])):
            seen.append(event)
    # Deltas received before EOF were still yielded, not swallowed.
    assert [e.content for e in seen if e.event_type == "text" and not e.done] == ["par"]


@respx.mock
@pytest.mark.asyncio
async def test_codex_done_sentinel_alone_is_not_terminal():
    # [DONE] without response.completed is still truncation for chatgpt_codex.
    body = _sse({"type": "response.output_text.delta", "delta": "par"}) + "data: [DONE]\n"
    respx.post("https://chatgpt.com/backend-api/codex/responses").mock(
        return_value=httpx.Response(200, text=body, headers={"content-type": "text/event-stream"})
    )
    p = ChatGptCodexProvider("tok", "acct-123", "gpt-5.5", None)
    with pytest.raises(IncompleteStreamError, match="chatgpt_codex"):
        async for _ in p.stream(ChatRequest(messages=[Message.user("hi")])):
            pass


@respx.mock
@pytest.mark.asyncio
async def test_openai_finish_reason_then_eof_completes_with_stashed_stop_reason():
    # Amended M3 rule (2026-07-17): finish_reason is the SEMANTIC terminal.
    # EOF after it is a COMPLETE stream even though the [DONE] transport
    # epilogue never arrived. The stream ends with a done event carrying the
    # stashed StopReason; no error is raised.
    body = _sse(
        {"choices": [{"delta": {"content": "par"}, "finish_reason": None}]},
        {"choices": [{"delta": {}, "finish_reason": "stop"}]},
    )
    respx.post("https://mock.openai.com/v1/chat/completions").mock(
        return_value=httpx.Response(200, text=body, headers={"content-type": "text/event-stream"})
    )
    p = OpenAIProvider("test-key", base_url="https://mock.openai.com")
    events = [e async for e in p.stream(ChatRequest(messages=[Message.user("hi")]))]
    assert [e.content for e in events if e.event_type == "text" and not e.done] == ["par"]
    dones = [e for e in events if e.done]
    assert len(dones) == 1 and events[-1] is dones[0]
    assert dones[0].stop_reason == StopReason.stop


@respx.mock
@pytest.mark.asyncio
async def test_minimax_finish_reason_then_eof_completes():
    # MiniMax (Python OpenAI-compatible wire) follows the same either-suffices
    # rule: a finish_reason chunk is the semantic terminal. Its done event
    # carries no stop_reason, matching the [DONE] branch; the collector
    # heuristic fills it (E2).
    body = _sse(
        {"choices": [{"delta": {"content": "par"}}]},
        {"choices": [{"delta": {}, "finish_reason": "stop"}]},
    )
    respx.post("https://api.minimax.chat/v1/text/chatcompletion_v2").mock(
        return_value=httpx.Response(200, text=body, headers={"content-type": "text/event-stream"})
    )
    p = MinimaxProvider("test-key")
    events = [e async for e in p.stream(ChatRequest(messages=[Message.user("hi")]))]
    assert [e.content for e in events if e.content] == ["par"]
    assert events[-1].done and events[-1].stop_reason is None


@respx.mock
@pytest.mark.asyncio
async def test_openai_finish_reason_then_done_completes_with_stashed_stop_reason():
    # Transport-epilogue path: the finish_reason-derived stop_reason is
    # stashed and emitted with the [DONE] done event (either terminal signal
    # suffices; here [DONE] arrives and carries the stash).
    body = (
        _sse(
            {"choices": [{"delta": {"content": "hi"}, "finish_reason": None}]},
            {"choices": [{"delta": {}, "finish_reason": "stop"}]},
        )
        + "data: [DONE]\n"
    )
    respx.post("https://mock.openai.com/v1/chat/completions").mock(
        return_value=httpx.Response(200, text=body, headers={"content-type": "text/event-stream"})
    )
    p = OpenAIProvider("test-key", base_url="https://mock.openai.com")
    events = [e async for e in p.stream(ChatRequest(messages=[Message.user("hi")]))]
    dones = [e for e in events if e.done]
    assert len(dones) == 1 and events[-1] is dones[0]
    assert dones[0].stop_reason == StopReason.stop


def test_incomplete_stream_error_is_stream_error_subclass():
    # E1 migration softener: pre-existing ``except StreamError`` call sites keep
    # catching truncation; MotosanError metadata kwargs are inherited.
    err = IncompleteStreamError("incomplete stream: openai ended without a terminal event")
    assert isinstance(err, StreamError)
    assert err.status_code is None and err.retry_after is None and err.request_id is None


def test_incomplete_stream_error_exported_from_top_level():
    import motosan_ai

    assert motosan_ai.IncompleteStreamError is IncompleteStreamError


@pytest.mark.asyncio
async def test_collect_stream_propagates_incomplete_stream_error():
    # E2: collector keeps M1 fallible-stream propagation - no fallback, no
    # partial ChatResponse. Sibling: test_client_stream_collect.py::
    # test_collect_stream_propagates_mid_stream_raise.
    async def _truncated():
        yield StreamEvent(content="partial", done=False)
        raise IncompleteStreamError("incomplete stream: anthropic ended without a terminal event")

    with pytest.raises(IncompleteStreamError):
        await collect_stream(_truncated())
