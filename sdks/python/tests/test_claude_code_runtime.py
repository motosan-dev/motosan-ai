import os
from unittest.mock import AsyncMock, patch

import pytest

from motosan_ai._stream_collect import collect_stream
from motosan_ai.error import StreamError
from motosan_ai.providers.claude_code import ClaudeCodeClient
from motosan_ai.types import ChatRequest, Message, Role, StopReason, ToolCall


def _make_proc(stdout: bytes = b'{"type":"result","result":"hi","is_error":false}\n'):
    """AsyncMock claude proc. `stdout` feeds BOTH proc.communicate() (chat)
    and proc.stdout.readline() (stream): readline pops one `\n`-terminated
    line per call, then b''."""
    proc = AsyncMock()
    proc.communicate = AsyncMock(return_value=(stdout, b""))
    lines = stdout.splitlines(keepends=True)
    proc.stdout = AsyncMock()
    proc.stdout.readline = AsyncMock(side_effect=[*lines, b""])
    proc.stdin = AsyncMock()
    proc.stdin.write = lambda b: None
    proc.stdin.close = lambda: None
    proc.returncode = 0
    proc.kill = lambda: None
    proc.wait = AsyncMock(return_value=0)
    return proc


@pytest.mark.asyncio
async def test_chat_passes_cwd_to_subprocess():
    proc = _make_proc()
    with patch(
        "motosan_ai.providers.claude_code.asyncio.create_subprocess_exec",
        new=AsyncMock(return_value=proc),
    ) as spawn:
        client = ClaudeCodeClient().agent_mode(True).cwd("/work")
        req = ChatRequest(messages=[Message(role=Role.user, content="hi")])
        await client.chat(req)
        assert spawn.call_args.kwargs["cwd"] == "/work"


@pytest.mark.asyncio
async def test_chat_merges_env_over_os_environ(monkeypatch):
    monkeypatch.setenv("PATH", "/usr/bin")
    proc = _make_proc()
    spawn = AsyncMock(return_value=proc)
    monkeypatch.setattr("motosan_ai.providers.claude_code.asyncio.create_subprocess_exec", spawn)
    client = ClaudeCodeClient().env("K", "v")  # agent_mode if needed for parse
    await client.chat(ChatRequest(messages=[Message(role=Role.user, content="hi")]))
    env = spawn.call_args.kwargs["env"]
    assert env["K"] == "v"
    assert env["PATH"] == "/usr/bin"  # inherited (merge, not replace)
    assert "K" not in os.environ  # parent unmutated


@pytest.mark.asyncio
async def test_chat_env_none_when_empty(monkeypatch):
    proc = _make_proc()
    spawn = AsyncMock(return_value=proc)
    monkeypatch.setattr("motosan_ai.providers.claude_code.asyncio.create_subprocess_exec", spawn)
    await ClaudeCodeClient().chat(ChatRequest(messages=[Message(role=Role.user, content="hi")]))
    assert spawn.call_args.kwargs["env"] is None


@pytest.mark.asyncio
async def test_stream_tool_use_terminal_is_end_turn(monkeypatch):
    stdout = (
        b'{"type":"assistant","message":{"content":['
        b'{"type":"tool_use","id":"toolu_01","name":"Read","input":{}}]}}\n'
        b'{"type":"result","result":"done"}\n'
    )
    proc = _make_proc(stdout=stdout)
    monkeypatch.setattr(
        "motosan_ai.providers.claude_code.asyncio.create_subprocess_exec",
        AsyncMock(return_value=proc),
    )
    events = [
        ev
        async for ev in ClaudeCodeClient().stream(
            ChatRequest(messages=[Message(role=Role.user, content="hi")])
        )
    ]
    done = [e for e in events if e.done][-1]
    # F4: CLI backends execute tools internally; a completed turn is
    # always end_turn, never a tool_use request.
    assert done.stop_reason == StopReason.end_turn


@pytest.mark.asyncio
async def test_stream_child_death_raises_stream_error(monkeypatch):
    # One valid assistant event, then EOF with exit code 1 and stderr "boom".
    stdout = b'{"type":"assistant","message":{"content":[{"type":"text","text":"par"}]}}\n'
    proc = _make_proc(stdout=stdout)
    proc.returncode = 1
    proc.wait = AsyncMock(return_value=1)
    proc.stderr = AsyncMock()
    proc.stderr.read = AsyncMock(return_value=b"boom\n")
    monkeypatch.setattr(
        "motosan_ai.providers.claude_code.asyncio.create_subprocess_exec",
        AsyncMock(return_value=proc),
    )
    events = []
    with pytest.raises(StreamError, match="returncode 1") as excinfo:
        async for ev in ClaudeCodeClient().stream(
            ChatRequest(messages=[Message(role=Role.user, content="hi")])
        ):
            events.append(ev)

    assert "boom" in str(excinfo.value)
    assert [e.content for e in events] == ["par"]
    assert not any(e.done for e in events)


@pytest.mark.asyncio
async def test_chat_no_timeout_skips_wait_for(monkeypatch):
    proc = _make_proc()
    monkeypatch.setattr(
        "motosan_ai.providers.claude_code.asyncio.create_subprocess_exec",
        AsyncMock(return_value=proc),
    )

    def _boom(*a, **k):
        raise AssertionError("wait_for must not be called under no_timeout")

    monkeypatch.setattr("motosan_ai.providers.claude_code.asyncio.wait_for", _boom)
    client = ClaudeCodeClient().no_timeout()
    await client.chat(ChatRequest(messages=[Message(role=Role.user, content="hi")]))


@pytest.mark.asyncio
async def test_stream_error_result_raises_stream_error(monkeypatch):
    stdout = (
        b'{"type":"assistant","message":{"content":[{"type":"text","text":"partial"}]}}\n'
        b'{"type":"result","subtype":"error_max_turns","is_error":true}\n'
    )
    proc = _make_proc(stdout=stdout)
    monkeypatch.setattr(
        "motosan_ai.providers.claude_code.asyncio.create_subprocess_exec",
        AsyncMock(return_value=proc),
    )
    events = []
    with pytest.raises(StreamError, match="error_max_turns"):
        async for ev in ClaudeCodeClient().stream(
            ChatRequest(messages=[Message(role=Role.user, content="hi")])
        ):
            events.append(ev)
    # partial text already yielded is kept; no fabricated clean done event
    assert [e.content for e in events] == ["partial"]
    assert not any(e.done for e in events)


@pytest.mark.asyncio
async def test_chat_agent_mode_error_result_raises_stream_error(monkeypatch):
    stdout = (
        b'{"type":"result","subtype":"error_during_execution","is_error":true,"result":"boom"}\n'
    )
    proc = _make_proc(stdout=stdout)
    monkeypatch.setattr(
        "motosan_ai.providers.claude_code.asyncio.create_subprocess_exec",
        AsyncMock(return_value=proc),
    )
    client = ClaudeCodeClient().agent_mode(True)
    with pytest.raises(StreamError, match="error_during_execution"):
        await client.chat(ChatRequest(messages=[Message(role=Role.user, content="hi")]))


@pytest.mark.asyncio
async def test_chat_stream_parity_tool_turn_is_end_turn(monkeypatch):
    """F4: chat() == collect_stream(stream()) for a text + tool-call + terminal
    transcript. Both paths report stop_reason end_turn and surface the
    executed-tool record; model backfill is the one allowed difference."""
    stdout = (
        b'{"type":"assistant","message":{"content":['
        b'{"type":"text","text":"let me read it"},'
        b'{"type":"tool_use","id":"toolu_01","name":"Read","input":{"path":"/tmp/x"}}]}}\n'
        b'{"type":"result","result":"done","session_id":"sess_1",'
        b'"usage":{"input_tokens":7,"output_tokens":3}}\n'
    )
    monkeypatch.setattr(
        "motosan_ai.providers.claude_code.asyncio.create_subprocess_exec",
        AsyncMock(side_effect=[_make_proc(stdout=stdout), _make_proc(stdout=stdout)]),
    )
    client = ClaudeCodeClient()
    request = ChatRequest(messages=[Message(role=Role.user, content="hi")])

    chat_resp = await client.chat(request)
    streamed = await collect_stream(client.stream(request))

    expected_tool_calls = [ToolCall(id="toolu_01", name="Read", input={"path": "/tmp/x"})]
    assert chat_resp.tool_calls == expected_tool_calls
    assert chat_resp.stop_reason == StopReason.end_turn
    assert streamed.stop_reason == StopReason.end_turn
    # Parity, field by field (model exempt: chat backfills it from config).
    assert chat_resp.content == streamed.content
    assert chat_resp.thinking == streamed.thinking
    assert chat_resp.tool_calls == streamed.tool_calls
    assert chat_resp.stop_reason == streamed.stop_reason
    assert chat_resp.usage == streamed.usage
    assert chat_resp.session_id == streamed.session_id
    assert chat_resp.content == "let me read it"
    assert chat_resp.usage.input_tokens == 7
    assert chat_resp.usage.output_tokens == 3
    assert chat_resp.session_id == "sess_1"
