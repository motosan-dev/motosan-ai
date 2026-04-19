from __future__ import annotations

import asyncio
import json
import os
from collections.abc import AsyncIterator

from motosan_ai.error import ProviderError
from motosan_ai.types import (
    ChatRequest,
    ChatResponse,
    Message,
    Role,
    StopReason,
    StreamEvent,
    Usage,
)

_TIMEOUT_SECS = 300


def _model_to_forward(model: str) -> str | None:
    """Return the trimmed model string to forward as ``--model``, or ``None``.

    Skips empty strings, whitespace-only strings, and the sentinel value
    ``"default"`` (case-insensitive).
    """
    trimmed = model.strip()
    if not trimmed or trimmed.lower() == "default":
        return None
    return trimmed


def _messages_to_prompt(messages: list[Message]) -> tuple[str | None, str]:
    """Flatten messages into ``(system_prompt, user_prompt)`` for ``claude --print``."""
    system: str | None = None
    for m in messages:
        if m.role == Role.system:
            system = m.content
            break

    non_system = [m for m in messages if m.role != Role.system]

    if len(non_system) <= 1:
        prompt = non_system[0].content if non_system else ""
    else:
        parts: list[str] = []
        for m in non_system:
            label = {
                Role.user: "[user]",
                Role.assistant: "[assistant]",
                Role.tool: "[tool]",
            }[m.role]
            parts.append(f"{label}\n{m.content}")
        prompt = "\n\n".join(parts)

    return system, prompt


def _parse_agent_json(raw: str) -> tuple[str, Usage]:
    """Parse JSON output from agent mode, extracting ``result`` and ``usage``."""
    try:
        v = json.loads(raw)
    except json.JSONDecodeError as e:
        raise ProviderError(f"failed to parse claude JSON output: {e}") from e

    text = v.get("result", "")
    u = v.get("usage", {})
    return text, Usage(
        input_tokens=int(u.get("input_tokens", 0)),
        output_tokens=int(u.get("output_tokens", 0)),
    )


def _parse_ndjson_line(line: str) -> StreamEvent | None:
    """Parse a single NDJSON line into a ``StreamEvent`` or ``None``.

    The ``claude --print --output-format stream-json --verbose`` CLI emits
    events with ``type`` of ``"assistant"`` (containing message content) and
    ``"result"`` (final summary with usage).  Text is nested inside
    ``message.content[]`` blocks with ``type == "text"``.
    """
    try:
        event = json.loads(line)
    except json.JSONDecodeError:
        return None

    event_type = event.get("type")

    if event_type == "assistant":
        message = event.get("message", {})
        content_blocks = message.get("content", [])
        parts: list[str] = []
        for block in content_blocks:
            if isinstance(block, dict) and block.get("type") == "text":
                t = block.get("text", "")
                if t:
                    parts.append(t)
        text = "".join(parts)
        if not text:
            return None
        return StreamEvent(content=text, done=False)

    if event_type == "result":
        return StreamEvent(content="", done=True)

    return None


class ClaudeCodeClient:
    """Client that shells out to the ``claude`` CLI binary."""

    def __init__(self, binary_path: str | None = None) -> None:
        if binary_path is None:
            binary_path = os.environ.get("CLAUDE_CODE_PATH", "claude")
        self._binary_path = binary_path
        self._agent_mode = False
        self._model: str | None = None

    @classmethod
    def with_path(cls, path: str) -> ClaudeCodeClient:
        """Create a client with an explicit binary path."""
        return cls(binary_path=path)

    def model(self, model: str) -> ClaudeCodeClient:
        """Set the model to forward to the CLI."""
        self._model = model
        return self

    def agent_mode(self, enabled: bool) -> ClaudeCodeClient:
        """Enable or disable agent mode."""
        self._agent_mode = enabled
        return self

    def _build_args(
        self,
        *,
        model: str | None,
        system_prompt: str | None,
        output_format: str | None = None,
    ) -> list[str]:
        args = [self._binary_path, "--print"]

        if self._agent_mode:
            args.append("--dangerously-skip-permissions")
            if output_format is None:
                args.extend(["--output-format", "json"])

        if output_format is not None:
            args.extend(["--output-format", output_format])
            if output_format == "stream-json":
                args.append("--verbose")

        effective_model = model or self._model
        if effective_model:
            forwarded = _model_to_forward(effective_model)
            if forwarded:
                args.extend(["--model", forwarded])

        if system_prompt and system_prompt.strip():
            args.extend(["--append-system-prompt", system_prompt])

        args.append("-")
        return args

    async def chat(self, request: ChatRequest) -> ChatResponse:
        """Invoke ``claude --print`` and collect the output."""
        msg_system, user_prompt = _messages_to_prompt(request.messages)
        system_prompt = request.system or msg_system

        args = self._build_args(
            model=request.model,
            system_prompt=system_prompt,
        )

        proc = await asyncio.create_subprocess_exec(
            *args,
            stdin=asyncio.subprocess.PIPE,
            stdout=asyncio.subprocess.PIPE,
            stderr=asyncio.subprocess.PIPE,
        )

        try:
            stdout, stderr = await asyncio.wait_for(
                proc.communicate(user_prompt.encode()),
                timeout=_TIMEOUT_SECS,
            )
        except TimeoutError as exc:
            proc.kill()
            await proc.wait()
            raise ProviderError(f"claude CLI timed out after {_TIMEOUT_SECS} seconds") from exc

        if proc.returncode != 0:
            raise ProviderError(
                f"claude CLI exited with {proc.returncode}: {stderr.decode().strip()}"
            )

        raw = stdout.decode()

        if self._agent_mode:
            text, usage = _parse_agent_json(raw)
        else:
            text = raw.strip()
            usage = Usage(input_tokens=0, output_tokens=0)

        effective_model = request.model or self._model or ""

        return ChatResponse(
            content=text,
            tool_calls=[],
            model=effective_model,
            usage=usage,
            stop_reason=StopReason.end_turn,
        )

    async def stream(self, request: ChatRequest) -> AsyncIterator[StreamEvent]:
        """Stream via ``claude --print --output-format stream-json``, yielding NDJSON events.

        Raises :class:`~motosan_ai.error.ProviderError` if no output is received
        within ``_TIMEOUT_SECS`` seconds.
        """
        msg_system, user_prompt = _messages_to_prompt(request.messages)
        system_prompt = request.system or msg_system

        args = self._build_args(
            model=request.model,
            system_prompt=system_prompt,
            output_format="stream-json",
        )

        proc = await asyncio.create_subprocess_exec(
            *args,
            stdin=asyncio.subprocess.PIPE,
            stdout=asyncio.subprocess.PIPE,
            stderr=asyncio.subprocess.PIPE,
        )

        # Write prompt to stdin then close it
        assert proc.stdin is not None
        proc.stdin.write(user_prompt.encode())
        await proc.stdin.drain()
        proc.stdin.close()
        await proc.stdin.wait_closed()

        assert proc.stdout is not None
        try:
            while True:
                try:
                    raw_line = await asyncio.wait_for(proc.stdout.readline(), timeout=_TIMEOUT_SECS)
                except TimeoutError as exc:
                    raise ProviderError(
                        f"claude CLI stream timed out after {_TIMEOUT_SECS} seconds"
                    ) from exc
                if not raw_line:
                    break
                line = raw_line.decode().strip()
                if not line:
                    continue
                event = _parse_ndjson_line(line)
                if event is not None:
                    yield event
                    if event.done:
                        break
        finally:
            # Ensure the child process is always cleaned up, even if the caller
            # breaks out of the async-for loop early or an exception is raised.
            # Guard against ProcessLookupError when the process has already exited
            # (the normal completion path).
            try:
                proc.kill()
            except ProcessLookupError:
                pass
            await proc.wait()
