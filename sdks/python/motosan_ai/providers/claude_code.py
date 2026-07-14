from __future__ import annotations

import asyncio
import contextlib
import json
import os
from collections.abc import AsyncIterator
from dataclasses import dataclass, field

from motosan_ai.error import ProviderError, StreamError
from motosan_ai.provider_base import ProviderCapabilities
from motosan_ai.types import (
    ChatRequest,
    ChatResponse,
    Message,
    Role,
    StopReason,
    StreamEvent,
    Usage,
)


class _RedactedEnvs(list):
    """list[tuple[str, str]] of (key, value) env pairs whose repr hides values."""

    def __repr__(self) -> str:
        return f"<{len(self)} redacted>"


@dataclass
class _ClaudeCodeConfig:
    binary_path: str = "claude"
    model: str | None = None
    agent_mode: bool = False
    bare: bool = False
    system_prompt_flag: str | None = None
    permission_mode: str | None = None
    effort: str | None = None
    fallback_model: str | None = None
    settings: str | None = None
    agent: str | None = None
    add_dirs: list[str] = field(default_factory=list)
    allowed_tools: list[str] = field(default_factory=list)
    disallowed_tools: list[str] = field(default_factory=list)
    mcp_configs: list[str] = field(default_factory=list)
    strict_mcp_config: bool = False
    setting_sources: list[str] = field(default_factory=list)
    session_id: str | None = None
    resume: str | None = None
    continue_latest: bool = False
    fork_session: bool = False
    no_session_persistence: bool = False
    plugin_dirs: list[str] = field(default_factory=list)
    max_budget_usd: float | None = None
    cwd: str | None = None
    envs: _RedactedEnvs = field(default_factory=_RedactedEnvs)
    timeout_secs: float | None = 300.0


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


def _raise_on_error_result(event: dict) -> None:
    """Raise ``StreamError`` when a terminal ``result`` payload reports an error.

    Claude Code marks failed/truncated turns with ``is_error: true`` and an
    ``error_*`` subtype (e.g. ``error_max_turns``, ``error_during_execution``).
    Emitting them as clean completions would let agent loops consume a
    truncated turn as a genuine one, so surface them as ``StreamError``.
    """
    subtype = event.get("subtype")
    has_error_subtype = isinstance(subtype, str) and subtype.startswith("error")
    if not (event.get("is_error") or has_error_subtype):
        return
    result_text = event.get("result")
    parts = [p for p in (subtype, result_text) if isinstance(p, str) and p]
    detail = ": ".join(parts) if parts else "unknown error"
    raise StreamError(f"claude CLI reported an error result: {detail}")


def _parse_agent_json(raw: str) -> tuple[str, Usage, str | None]:
    """Parse JSON output from agent mode: ``result``, ``usage``, ``session_id``.

    Raises :class:`~motosan_ai.error.StreamError` when the payload reports an
    error result (``is_error: true`` or an ``error_*`` subtype).
    """
    try:
        v = json.loads(raw)
    except json.JSONDecodeError as e:
        raise ProviderError(f"failed to parse claude JSON output: {e}") from e

    if isinstance(v, dict):
        _raise_on_error_result(v)

    text = v.get("result", "")
    u = v.get("usage", {})
    sid = v.get("session_id")
    session_id = sid if isinstance(sid, str) and sid else None
    return (
        text,
        Usage(
            input_tokens=int(u.get("input_tokens", 0)),
            output_tokens=int(u.get("output_tokens", 0)),
        ),
        session_id,
    )


def _parse_ndjson_line(line: str) -> list[StreamEvent]:
    """Parse a single NDJSON line into zero, one, or two stream events.

    Claude Code ``result`` events may include token usage. When present, emit
    a usage event before the terminal done event, matching the Rust provider.
    """
    try:
        event = json.loads(line)
    except json.JSONDecodeError:
        return []

    event_type = event.get("type")

    if event_type == "assistant":
        message = event.get("message", {})
        content_blocks = message.get("content", [])
        events: list[StreamEvent] = []
        text_parts: list[str] = []

        def _flush_text() -> None:
            joined = "".join(text_parts)
            if joined:
                events.append(StreamEvent(content=joined, done=False))
            text_parts.clear()

        for block in content_blocks:
            if not isinstance(block, dict):
                continue
            btype = block.get("type")
            if btype == "text":
                t = block.get("text", "")
                if t:
                    text_parts.append(t)
            elif btype == "tool_use":
                _flush_text()
                tool_id = block.get("id", "")
                tool_name = block.get("name", "")
                tool_input = block.get("input", {})
                args = json.dumps(tool_input, separators=(",", ":"))
                events.append(
                    StreamEvent(
                        content="",
                        done=False,
                        event_type="tool_call_start",
                        tool_call_id=tool_id,
                        tool_call_name=tool_name,
                    )
                )
                events.append(
                    StreamEvent(
                        content="",
                        done=False,
                        event_type="tool_call_args",
                        tool_call_id=tool_id,
                        tool_call_args_delta=args,
                    )
                )
                events.append(
                    StreamEvent(
                        content="",
                        done=False,
                        event_type="tool_call_end",
                        tool_call_id=tool_id,
                    )
                )
            # thinking / other blocks ignored
        _flush_text()
        return events

    if event_type == "result":
        _raise_on_error_result(event)
        events_out: list[StreamEvent] = []
        sid = event.get("session_id")
        if isinstance(sid, str) and sid:
            events_out.append(StreamEvent(content="", done=False, session_id=sid))
        usage_obj = event.get("usage")
        if isinstance(usage_obj, dict):
            events_out.append(
                StreamEvent(
                    content="",
                    done=False,
                    event_type="usage",
                    usage=Usage(
                        input_tokens=int(usage_obj.get("input_tokens", 0) or 0),
                        output_tokens=int(usage_obj.get("output_tokens", 0) or 0),
                    ),
                )
            )
        events_out.append(StreamEvent(content="", done=True))
        return events_out

    return []


class ClaudeCodeClient:
    """Client that shells out to the ``claude`` CLI binary."""

    capabilities: ProviderCapabilities = ProviderCapabilities.text_only()

    def __init__(self, binary_path: str | None = None) -> None:
        if binary_path is None:
            binary_path = os.environ.get("CLAUDE_CODE_PATH", "claude")
        self._config = _ClaudeCodeConfig(binary_path=binary_path)

    @property
    def _binary_path(self) -> str:
        return self._config.binary_path

    @_binary_path.setter
    def _binary_path(self, value: str) -> None:
        self._config.binary_path = value

    @property
    def _model(self) -> str | None:
        return self._config.model

    @_model.setter
    def _model(self, value: str | None) -> None:
        self._config.model = value

    @property
    def _agent_mode(self) -> bool:
        return self._config.agent_mode

    @_agent_mode.setter
    def _agent_mode(self, value: bool) -> None:
        self._config.agent_mode = value

    @classmethod
    def with_path(cls, path: str) -> ClaudeCodeClient:
        """Create a client with an explicit binary path."""
        return cls(binary_path=path)

    def model(self, model: str) -> ClaudeCodeClient:
        """Set the model to forward to the CLI."""
        self._config.model = model
        return self

    def agent_mode(self, enabled: bool) -> ClaudeCodeClient:
        """Enable or disable agent mode."""
        self._config.agent_mode = enabled
        return self

    def bare(self, enabled: bool) -> ClaudeCodeClient:
        self._config.bare = enabled
        return self

    def system_prompt(self, prompt: str) -> ClaudeCodeClient:
        self._config.system_prompt_flag = prompt if prompt.strip() else None
        return self

    def permission_mode(self, mode: str) -> ClaudeCodeClient:
        self._config.permission_mode = mode
        return self

    def effort(self, level: str) -> ClaudeCodeClient:
        self._config.effort = level
        return self

    def fallback_model(self, model: str) -> ClaudeCodeClient:
        self._config.fallback_model = model
        return self

    def settings(self, settings: str) -> ClaudeCodeClient:
        self._config.settings = settings
        return self

    def agent(self, name: str) -> ClaudeCodeClient:
        self._config.agent = name
        return self

    def add_dir(self, path: str) -> ClaudeCodeClient:
        self._config.add_dirs.append(path)
        return self

    def add_dirs(self, paths: list[str]) -> ClaudeCodeClient:
        self._config.add_dirs = list(paths)
        return self

    def allow_tool(self, name: str) -> ClaudeCodeClient:
        self._config.allowed_tools.append(name)
        return self

    def allowed_tools(self, tools: list[str]) -> ClaudeCodeClient:
        self._config.allowed_tools = list(tools)
        return self

    def disallow_tool(self, name: str) -> ClaudeCodeClient:
        self._config.disallowed_tools.append(name)
        return self

    def disallowed_tools(self, tools: list[str]) -> ClaudeCodeClient:
        self._config.disallowed_tools = list(tools)
        return self

    def mcp_config(self, config: str) -> ClaudeCodeClient:
        self._config.mcp_configs.append(config)
        return self

    def mcp_configs(self, configs: list[str]) -> ClaudeCodeClient:
        self._config.mcp_configs = list(configs)
        return self

    def strict_mcp_config(self, enabled: bool) -> ClaudeCodeClient:
        self._config.strict_mcp_config = enabled
        return self

    def setting_source(self, source: str) -> ClaudeCodeClient:
        self._config.setting_sources.append(source)
        return self

    def setting_sources(self, sources: list[str]) -> ClaudeCodeClient:
        self._config.setting_sources = list(sources)
        return self

    def session_id(self, uuid: str) -> ClaudeCodeClient:
        self._config.session_id = uuid
        return self

    def resume(self, value: str) -> ClaudeCodeClient:
        self._config.resume = value
        return self

    def continue_latest(self, enabled: bool) -> ClaudeCodeClient:
        self._config.continue_latest = enabled
        return self

    def fork_session(self, enabled: bool) -> ClaudeCodeClient:
        self._config.fork_session = enabled
        return self

    def no_session_persistence(self, enabled: bool) -> ClaudeCodeClient:
        self._config.no_session_persistence = enabled
        return self

    def plugin_dir(self, path: str) -> ClaudeCodeClient:
        self._config.plugin_dirs.append(path)
        return self

    def plugin_dirs(self, paths: list[str]) -> ClaudeCodeClient:
        self._config.plugin_dirs = list(paths)
        return self

    def max_budget_usd(self, amount: float) -> ClaudeCodeClient:
        self._config.max_budget_usd = amount
        return self

    def cwd(self, directory: str) -> ClaudeCodeClient:
        """Set the working directory for the spawned ``claude`` subprocess."""
        self._config.cwd = directory
        return self

    def env(self, key: str, value: str) -> ClaudeCodeClient:
        """Inject one environment variable into the spawned subprocess (repeatable).

        The value is treated as a secret and is redacted from ``repr``.
        """
        self._config.envs.append((key, value))
        return self

    def envs(self, vars: dict[str, str]) -> ClaudeCodeClient:
        """Replace the full set of injected environment variables (does not append)."""
        self._config.envs = _RedactedEnvs(vars.items())
        return self

    def timeout(self, secs: float) -> ClaudeCodeClient:
        """Override the per-invocation timeout (``chat`` and the stream read loop)."""
        self._config.timeout_secs = secs
        return self

    def no_timeout(self) -> ClaudeCodeClient:
        """Disable the invocation timeout (run until the child exits)."""
        self._config.timeout_secs = None
        return self

    def _subprocess_env(self) -> dict[str, str] | None:
        """Child env: a copy of os.environ overlaid with injected vars.

        Returns ``None`` when nothing is injected so the child inherits the
        parent environment unchanged (and os.environ is never mutated).
        """
        if not self._config.envs:
            return None
        merged = dict(os.environ)
        for key, value in self._config.envs:
            merged[key] = value
        return merged

    def _build_args(
        self,
        *,
        model: str | None,
        system_prompt: str | None,
        output_format: str | None = None,
    ) -> list[str]:
        args = [self._config.binary_path, "--print"]

        if output_format is not None:
            args.extend(["--output-format", output_format])
            if output_format == "stream-json":
                args.append("--verbose")
        elif self._config.agent_mode:
            args.extend(["--output-format", "json"])

        if self._config.bare:
            args.append("--bare")

        if self._config.agent_mode:
            args.append("--dangerously-skip-permissions")

        if self._config.permission_mode:
            args.extend(["--permission-mode", self._config.permission_mode])

        effective_model = self._config.model if model is None else model
        if effective_model:
            forwarded = _model_to_forward(effective_model)
            if forwarded:
                args.extend(["--model", forwarded])

        if self._config.fallback_model:
            forwarded = _model_to_forward(self._config.fallback_model)
            if forwarded:
                args.extend(["--fallback-model", forwarded])

        if self._config.effort:
            args.extend(["--effort", self._config.effort])

        if self._config.system_prompt_flag:
            args.extend(["--system-prompt", self._config.system_prompt_flag])

        if system_prompt and system_prompt.strip():
            args.extend(["--append-system-prompt", system_prompt])

        if self._config.agent and self._config.agent.strip():
            args.extend(["--agent", self._config.agent])

        for directory in self._config.add_dirs:
            args.extend(["--add-dir", directory])

        allowed_tools = [tool for tool in self._config.allowed_tools if tool.strip()]
        if allowed_tools:
            args.append("--allowed-tools")
            args.extend(allowed_tools)

        disallowed_tools = [tool for tool in self._config.disallowed_tools if tool.strip()]
        if disallowed_tools:
            args.append("--disallowed-tools")
            args.extend(disallowed_tools)

        mcp_configs = [config for config in self._config.mcp_configs if config.strip()]
        if mcp_configs:
            args.append("--mcp-config")
            args.extend(mcp_configs)

        if self._config.strict_mcp_config:
            args.append("--strict-mcp-config")

        if self._config.settings and self._config.settings.strip():
            args.extend(["--settings", self._config.settings])

        setting_sources = [
            source.strip() for source in self._config.setting_sources if source.strip()
        ]
        if setting_sources:
            args.extend(["--setting-sources", ",".join(setting_sources)])

        for directory in self._config.plugin_dirs:
            args.extend(["--plugin-dir", directory])

        if self._config.session_id and self._config.session_id.strip():
            args.extend(["--session-id", self._config.session_id])

        if self._config.resume and self._config.resume.strip():
            args.extend(["--resume", self._config.resume.strip()])

        if self._config.continue_latest:
            args.append("--continue")

        if self._config.fork_session:
            args.append("--fork-session")

        if self._config.no_session_persistence:
            args.append("--no-session-persistence")

        if (
            self._config.max_budget_usd is not None
            and self._config.max_budget_usd >= 0.0
            and self._config.max_budget_usd != float("inf")
            and self._config.max_budget_usd != float("-inf")
        ):
            args.extend(["--max-budget-usd", str(self._config.max_budget_usd)])

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
            cwd=self._config.cwd,
            env=self._subprocess_env(),
        )

        timeout = self._config.timeout_secs
        try:
            if timeout is None:
                stdout, stderr = await proc.communicate(user_prompt.encode())
            else:
                stdout, stderr = await asyncio.wait_for(
                    proc.communicate(user_prompt.encode()),
                    timeout=timeout,
                )
        except TimeoutError as exc:
            proc.kill()
            await proc.wait()
            raise ProviderError(f"claude CLI timed out after {timeout} seconds") from exc

        if proc.returncode != 0:
            raise ProviderError(
                f"claude CLI exited with {proc.returncode}: {stderr.decode().strip()}"
            )

        raw = stdout.decode()

        if self._agent_mode:
            text, usage, session_id = _parse_agent_json(raw)
        else:
            text = raw.strip()
            usage = Usage(input_tokens=0, output_tokens=0)
            session_id = None

        effective_model = request.model or self._config.model or ""

        return ChatResponse(
            content=text,
            tool_calls=[],
            model=effective_model,
            usage=usage,
            stop_reason=StopReason.end_turn,
            session_id=session_id,
        )

    async def stream(self, request: ChatRequest) -> AsyncIterator[StreamEvent]:
        """Stream via ``claude --print --output-format stream-json``, yielding NDJSON events.

        Raises :class:`~motosan_ai.error.ProviderError` if no output is received
        within the configured ``timeout_secs`` (default 300s; disabled by
        :meth:`no_timeout`).
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
            cwd=self._config.cwd,
            env=self._subprocess_env(),
        )

        # Write prompt to stdin then close it
        assert proc.stdin is not None
        proc.stdin.write(user_prompt.encode())
        await proc.stdin.drain()
        proc.stdin.close()
        await proc.stdin.wait_closed()

        assert proc.stdout is not None
        timeout = self._config.timeout_secs
        saw_tool_call = False
        saw_done = False
        try:
            while True:
                if timeout is None:
                    raw_line = await proc.stdout.readline()
                else:
                    try:
                        raw_line = await asyncio.wait_for(proc.stdout.readline(), timeout=timeout)
                    except TimeoutError as exc:
                        raise ProviderError(
                            f"claude CLI stream read timed out after {timeout}s"
                        ) from exc
                if not raw_line:
                    if saw_done:
                        break
                    # EOF before the terminal event: the child crashed or
                    # closed stdout early — raise instead of truncating.
                    returncode = proc.returncode
                    if returncode is None:
                        with contextlib.suppress(TimeoutError):
                            returncode = await asyncio.wait_for(proc.wait(), timeout=5.0)
                    stderr_excerpt = ""
                    if proc.stderr is not None:
                        with contextlib.suppress(TimeoutError, OSError):
                            stderr_bytes = await asyncio.wait_for(proc.stderr.read(), timeout=5.0)
                            stderr_excerpt = stderr_bytes.decode(errors="replace").strip()[-2048:]
                    raise StreamError(
                        f"claude CLI exited unexpectedly "
                        f"(returncode {returncode}): {stderr_excerpt}"
                    )
                line = raw_line.decode().strip()
                if not line:
                    continue
                for event in _parse_ndjson_line(line):
                    if event.event_type in (
                        "tool_call_start",
                        "tool_call_args",
                        "tool_call_end",
                    ):
                        saw_tool_call = True
                    if event.done:
                        saw_done = True
                        event.stop_reason = (
                            StopReason.tool_use if saw_tool_call else StopReason.end_turn
                        )
                    yield event
                    if event.done:
                        return
        finally:
            # Ensure the child process is always cleaned up, even if the caller
            # breaks out of the async-for loop early or an exception is raised.
            # Guard against ProcessLookupError when the process has already exited
            # (the normal completion path).
            with contextlib.suppress(ProcessLookupError):
                proc.kill()
            await proc.wait()
