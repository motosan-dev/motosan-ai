from unittest.mock import AsyncMock, patch

import pytest

from motosan_ai.providers.claude_code import ClaudeCodeClient
from motosan_ai.types import ChatRequest, Message, Role


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
