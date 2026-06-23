from __future__ import annotations

import asyncio
import os
from unittest.mock import AsyncMock

import pytest

from motosan_ai.error import ProviderError
from motosan_ai.providers.codex_cli import (
    CodexCliClient,
    _compose_prompt,
    _messages_to_prompt,
    _parse_jsonl_line,
)
from motosan_ai.types import ChatRequest, Message


def test_agent_message_emits_text_event():
    line = '{"type": "item.completed", "item": {"type": "agent_message", "text": "hello"}}'
    events = _parse_jsonl_line(line)
    assert len(events) == 1
    assert events[0].content == "hello"
    assert events[0].done is False
    assert events[0].event_type == "text"


def test_agent_message_with_empty_text_dropped():
    line = '{"type": "item.completed", "item": {"type": "agent_message", "text": ""}}'
    assert _parse_jsonl_line(line) == []


def test_non_agent_message_item_dropped():
    line = '{"type": "item.completed", "item": {"type": "reasoning", "text": "thinking"}}'
    assert _parse_jsonl_line(line) == []


def test_turn_completed_without_usage_emits_done_only():
    events = _parse_jsonl_line('{"type": "turn.completed"}')
    assert len(events) == 1
    assert events[0].done is True
    assert events[0].usage is None


def test_turn_completed_with_usage_emits_usage_then_done():
    line = (
        '{"type": "turn.completed", '
        '"usage": {"input_tokens": 50, "output_tokens": 20, "cached_input_tokens": 10}}'
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


def test_turn_failed_raises_provider_error():
    line = '{"type": "turn.failed", "error": {"message": "model failed"}}'
    with pytest.raises(ProviderError, match="model failed"):
        _parse_jsonl_line(line)


def test_top_level_error_raises_provider_error():
    line = '{"type": "error", "message": "auth failed"}'
    with pytest.raises(ProviderError, match="auth failed"):
        _parse_jsonl_line(line)


def test_unknown_event_dropped():
    # thread.started now parses (F3); use a still-unmodeled event type.
    assert _parse_jsonl_line('{"type":"item.started","item_id":"abc"}') == []


def test_thread_started_captures_session_id():
    events = _parse_jsonl_line('{"type":"thread.started","thread_id":"th_abc"}')
    assert len(events) == 1
    assert events[0].session_id == "th_abc"
    assert events[0].done is False
    assert events[0].content == ""


def test_thread_started_without_id_dropped():
    assert _parse_jsonl_line('{"type":"thread.started"}') == []


def test_malformed_json_returns_empty():
    assert _parse_jsonl_line("not json {") == []
    assert _parse_jsonl_line("") == []


def test_messages_to_prompt_extracts_system_and_user_prompt():
    system, prompt = _messages_to_prompt([Message.system("be terse"), Message.user("hello")])
    assert system == "be terse"
    assert prompt == "hello"


def test_messages_to_prompt_multi_turn_labels_non_system_messages():
    system, prompt = _messages_to_prompt(
        [Message.user("q1"), Message.assistant("a1"), Message.user("q2")]
    )
    assert system is None
    assert "[user]\nq1" in prompt
    assert "[assistant]\na1" in prompt
    assert "[user]\nq2" in prompt


def test_compose_prompt_uses_rust_system_instructions_label():
    assert _compose_prompt("be terse", "hello") == (
        "[system instructions]\nbe terse\n\n[user]\nhello"
    )


def test_compose_prompt_without_system_passes_through():
    assert _compose_prompt(None, "hello") == "hello"
    assert _compose_prompt("", "hello") == "hello"


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
        '{"type": "item.completed", "item": {"type": "agent_message", "text": "Hello "}}\n'
        '{"type": "item.completed", "item": {"type": "agent_message", "text": "world."}}\n'
        '{"type": "turn.completed", "usage": {"input_tokens": 10, "output_tokens": 5}}\n'
    )
    _stub_subprocess(monkeypatch, _FakeProc(jsonl))

    client = CodexCliClient(binary_path="codex")
    resp = await client.chat(ChatRequest(messages=[Message.user("hi")]))
    assert resp.content == "Hello world."
    assert resp.usage.input_tokens == 10
    assert resp.usage.output_tokens == 5


@pytest.mark.asyncio
async def test_chat_prefers_request_system_over_message_system(monkeypatch):
    jsonl = '{"type": "turn.completed"}\n'
    fake = _FakeProc(jsonl)
    _stub_subprocess(monkeypatch, fake)

    client = CodexCliClient(binary_path="codex")
    await client.chat(
        ChatRequest(
            messages=[Message.system("message system"), Message.user("hi")],
            system="request system",
        )
    )

    assert fake.stdin.written.decode() == ("[system instructions]\nrequest system\n\n[user]\nhi")


@pytest.mark.asyncio
async def test_chat_raises_on_nonzero_returncode(monkeypatch):
    _stub_subprocess(monkeypatch, _FakeProc("", returncode=2, stderr="codex: bad config\n"))
    client = CodexCliClient(binary_path="codex")
    with pytest.raises(ProviderError, match="bad config"):
        await client.chat(ChatRequest(messages=[Message.user("hi")]))


@pytest.mark.asyncio
async def test_stream_uses_request_system(monkeypatch):
    jsonl = '{"type": "turn.completed"}\n'
    fake = _FakeProc(jsonl, returncode=None)
    _stub_subprocess(monkeypatch, fake)

    client = CodexCliClient(binary_path="codex")
    _ = [
        event
        async for event in client.stream(
            ChatRequest(messages=[Message.user("hi")], system="stream system")
        )
    ]

    assert fake.stdin.written.decode() == ("[system instructions]\nstream system\n\n[user]\nhi")


@pytest.mark.asyncio
async def test_stream_yields_events_in_order(monkeypatch):
    jsonl = (
        '{"type": "thread.started", "thread_id": "t1"}\n'
        '{"type": "item.completed", "item": {"type": "agent_message", "text": "hi"}}\n'
        '{"type": "turn.completed", "usage": {"input_tokens": 3, "output_tokens": 1}}\n'
    )
    _stub_subprocess(monkeypatch, _FakeProc(jsonl, returncode=None))

    client = CodexCliClient(binary_path="codex")
    events = [e async for e in client.stream(ChatRequest(messages=[Message.user("hi")]))]

    # thread.started now emits a session event (F3); exclude it from text events.
    session_events = [e for e in events if e.session_id is not None]
    text_events = [
        e for e in events if e.event_type == "text" and not e.done and e.session_id is None
    ]
    usage_events = [e for e in events if e.event_type == "usage"]
    done_events = [e for e in events if e.done]

    assert len(session_events) == 1
    assert session_events[0].session_id == "t1"
    assert [e.content for e in text_events] == ["hi"]
    assert len(usage_events) == 1
    assert usage_events[0].usage is not None
    assert usage_events[0].usage.input_tokens == 3
    assert len(done_events) == 1
    assert events.index(text_events[0]) < events.index(usage_events[0])
    assert events.index(usage_events[0]) < events.index(done_events[0])


@pytest.mark.asyncio
async def test_chat_passes_env_to_subprocess(monkeypatch):
    monkeypatch.setenv("PATH", "/usr/bin")
    monkeypatch.setattr(asyncio.subprocess, "PIPE", -1, raising=False)
    fake = _FakeProc(
        '{"type": "item.completed", "item": {"type": "agent_message", "text": "hi"}}\n'
        '{"type": "turn.completed"}\n'
    )
    spawn = AsyncMock(return_value=fake)
    monkeypatch.setattr("asyncio.create_subprocess_exec", spawn)
    await (
        CodexCliClient(binary_path="codex")
        .env("K", "v")
        .chat(ChatRequest(messages=[Message.user("hi")]))
    )
    env = spawn.call_args.kwargs["env"]
    assert env["K"] == "v" and env["PATH"] == "/usr/bin"
    assert "K" not in os.environ
