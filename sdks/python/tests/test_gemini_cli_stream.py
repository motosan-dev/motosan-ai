from __future__ import annotations

import asyncio
from unittest.mock import AsyncMock

import pytest

from motosan_ai.error import ProviderError
from motosan_ai.providers.gemini_cli import (
    GeminiCliClient,
    _merge_system_into_prompt,
    _messages_to_prompt,
    _parse_jsonl_line,
)
from motosan_ai.types import ChatRequest, Message


def test_init_event_without_session_id_dropped():
    assert _parse_jsonl_line('{"type":"init"}') == []


def test_init_event_yields_session_id():
    events = _parse_jsonl_line('{"type":"init","session_id":"s1"}')
    assert len(events) == 1
    assert events[0].session_id == "s1"
    assert events[0].done is False


def test_user_message_dropped():
    line = '{"type": "message", "role": "user", "content": "hi"}'
    assert _parse_jsonl_line(line) == []


def test_assistant_delta_emits_text():
    line = '{"type": "message", "role": "assistant", "content": "hello", "delta": true}'
    events = _parse_jsonl_line(line)
    assert len(events) == 1
    assert events[0].content == "hello"
    assert events[0].done is False
    assert events[0].event_type == "text"


def test_assistant_non_delta_dropped():
    line = '{"type": "message", "role": "assistant", "content": "hello", "delta": false}'
    assert _parse_jsonl_line(line) == []


def test_assistant_empty_content_dropped():
    line = '{"type": "message", "role": "assistant", "content": "", "delta": true}'
    assert _parse_jsonl_line(line) == []


def test_result_success_without_stats_emits_done_only():
    events = _parse_jsonl_line('{"type": "result", "status": "success"}')
    assert len(events) == 1
    assert events[0].done is True
    assert events[0].usage is None


def test_result_success_with_stats_emits_usage_then_done():
    line = (
        '{"type": "result", "status": "success", '
        '"stats": {"input_tokens": 50, "output_tokens": 20, "cached": 10}}'
    )
    events = _parse_jsonl_line(line)
    assert len(events) == 2
    usage_event, done_event = events
    assert usage_event.event_type == "usage"
    assert usage_event.usage is not None
    assert usage_event.usage.input_tokens == 50
    assert usage_event.usage.output_tokens == 20
    assert usage_event.usage.cache_read_input_tokens == 10
    assert usage_event.done is False
    assert done_event.done is True


def test_result_missing_status_treated_as_success():
    events = _parse_jsonl_line('{"type": "result"}')
    assert len(events) == 1
    assert events[0].done is True


def test_result_non_success_status_raises():
    with pytest.raises(ProviderError, match="result status: error"):
        _parse_jsonl_line('{"type": "result", "status": "error"}')


def test_unknown_event_dropped():
    assert _parse_jsonl_line('{"type": "future_event"}') == []


def test_malformed_json_returns_empty():
    assert _parse_jsonl_line("not json {") == []
    assert _parse_jsonl_line("") == []


def test_messages_to_prompt_single_user_returns_just_content():
    assert _messages_to_prompt([Message.user("hello")]) == (None, "hello")


def test_messages_to_prompt_extracts_system():
    out = _messages_to_prompt([Message.system("be terse"), Message.user("hello")])
    assert out == ("be terse", "hello")


def test_messages_to_prompt_multi_turn_labels():
    system, prompt = _messages_to_prompt(
        [Message.user("q1"), Message.assistant("a1"), Message.user("q2")]
    )
    assert system is None
    assert "q1" in prompt and "a1" in prompt and "q2" in prompt


def test_merge_system_prepends_with_double_newline():
    assert _merge_system_into_prompt("be helpful", "hello") == "be helpful\n\nhello"


def test_merge_system_none_passes_through():
    assert _merge_system_into_prompt(None, "hello") == "hello"


def test_merge_system_empty_passes_through():
    assert _merge_system_into_prompt("", "hello") == "hello"


class _FakeStdin:
    def __init__(self) -> None:
        self.written = b""

    def write(self, data: bytes) -> None:
        self.written += data

    async def drain(self) -> None: ...

    def close(self) -> None: ...

    async def wait_closed(self) -> None: ...


class _FakeStdout:
    def __init__(self, text: str) -> None:
        self._lines = [line.encode() for line in text.splitlines(True)]

    def __aiter__(self):
        self._iter = iter(self._lines)
        return self

    async def __anext__(self) -> bytes:
        try:
            return next(self._iter)
        except StopIteration as exc:
            raise StopAsyncIteration from exc


class _FakeProc:
    def __init__(self, stdout: str, returncode: int | None = 0, stderr: str = "") -> None:
        self._stdout_bytes = stdout.encode()
        self._stderr = stderr.encode()
        self.returncode = returncode
        self.stdin = _FakeStdin()
        self.stdout = _FakeStdout(stdout)
        self.killed = False

    async def communicate(self, input: bytes | None = None) -> tuple[bytes, bytes]:
        if input is not None:
            self.stdin.write(input)
        return self._stdout_bytes, self._stderr

    def kill(self) -> None:
        self.killed = True
        self.returncode = -9

    async def wait(self) -> int:
        if self.returncode is None:
            self.returncode = 0
        return self.returncode


def _stub_subprocess(monkeypatch, fake: _FakeProc) -> None:
    monkeypatch.setattr(asyncio.subprocess, "PIPE", -1, raising=False)
    monkeypatch.setattr("asyncio.create_subprocess_exec", AsyncMock(return_value=fake))


@pytest.mark.asyncio
async def test_chat_returns_concatenated_text(monkeypatch):
    jsonl = (
        '{"type": "init", "session_id": "s1"}\n'
        '{"type": "message", "role": "user", "content": "hi"}\n'
        '{"type": "message", "role": "assistant", "content": "Hello ", "delta": true}\n'
        '{"type": "message", "role": "assistant", "content": "world.", "delta": true}\n'
        '{"type": "result", "status": "success", '
        '"stats": {"input_tokens": 10, "output_tokens": 5}}\n'
    )
    _stub_subprocess(monkeypatch, _FakeProc(jsonl))

    client = GeminiCliClient(binary_path="gemini")
    resp = await client.chat(ChatRequest(messages=[Message.user("hi")]))
    assert resp.content == "Hello world."
    assert resp.usage.input_tokens == 10
    assert resp.usage.output_tokens == 5


@pytest.mark.asyncio
async def test_chat_captures_session_id(monkeypatch):
    jsonl = (
        '{"type": "init", "session_id": "s1"}\n'
        '{"type": "message", "role": "assistant", "content": "hi", "delta": true}\n'
        '{"type": "result", "status": "success"}\n'
    )
    _stub_subprocess(monkeypatch, _FakeProc(jsonl))
    resp = await GeminiCliClient(binary_path="gemini").chat(
        ChatRequest(messages=[Message.user("hi")])
    )
    assert resp.session_id == "s1"


@pytest.mark.asyncio
async def test_chat_raises_on_nonzero_returncode(monkeypatch):
    _stub_subprocess(monkeypatch, _FakeProc("", returncode=2, stderr="gemini: bad config\n"))
    client = GeminiCliClient(binary_path="gemini")
    with pytest.raises(ProviderError, match="bad config"):
        await client.chat(ChatRequest(messages=[Message.user("hi")]))


@pytest.mark.asyncio
async def test_chat_raises_on_result_failure(monkeypatch):
    _stub_subprocess(monkeypatch, _FakeProc('{"type": "result", "status": "rate_limited"}\n'))
    client = GeminiCliClient(binary_path="gemini")
    with pytest.raises(ProviderError, match="rate_limited"):
        await client.chat(ChatRequest(messages=[Message.user("hi")]))


@pytest.mark.asyncio
async def test_stream_yields_events_in_order(monkeypatch):
    jsonl = (
        '{"type": "init"}\n'
        '{"type": "message", "role": "user", "content": "hi"}\n'
        '{"type": "message", "role": "assistant", "content": "hi", "delta": true}\n'
        '{"type": "result", "status": "success", '
        '"stats": {"input_tokens": 3, "output_tokens": 1}}\n'
    )
    _stub_subprocess(monkeypatch, _FakeProc(jsonl, returncode=None))

    client = GeminiCliClient(binary_path="gemini")
    events = [e async for e in client.stream(ChatRequest(messages=[Message.user("hi")]))]

    text_events = [e for e in events if e.event_type == "text" and not e.done]
    usage_events = [e for e in events if e.event_type == "usage"]
    done_events = [e for e in events if e.done]

    assert [e.content for e in text_events] == ["hi"]
    assert len(usage_events) == 1
    assert usage_events[0].usage is not None
    assert usage_events[0].usage.input_tokens == 3
    assert len(done_events) == 1
    assert events.index(text_events[0]) < events.index(usage_events[0])
    assert events.index(usage_events[0]) < events.index(done_events[0])


@pytest.mark.asyncio
async def test_chat_passes_cwd_to_subprocess(monkeypatch):
    monkeypatch.setattr(asyncio.subprocess, "PIPE", -1, raising=False)
    fake = _FakeProc(
        '{"type": "message", "role": "assistant", "content": "hi", "delta": true}\n'
        '{"type": "result", "status": "success"}\n'
    )
    spawn = AsyncMock(return_value=fake)
    monkeypatch.setattr("asyncio.create_subprocess_exec", spawn)
    client = GeminiCliClient().cwd("/work/dir")
    await client.chat(ChatRequest(messages=[Message.user("hi")]))
    assert spawn.call_args.kwargs["cwd"] == "/work/dir"
