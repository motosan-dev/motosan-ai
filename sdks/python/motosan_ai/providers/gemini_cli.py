from __future__ import annotations

import asyncio
import contextlib
import json
import os
from collections.abc import AsyncIterator
from dataclasses import dataclass, field
from enum import StrEnum

from motosan_ai.error import ProviderError
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

_TIMEOUT_SECS = 600


class ApprovalMode(StrEnum):
    default = "default"
    auto_edit = "auto_edit"
    yolo = "yolo"
    plan = "plan"


class _RedactedEnvs:
    """Ordered env-var pairs whose ``repr`` never prints values (secrets)."""

    def __init__(self, pairs: list[tuple[str, str]] | None = None) -> None:
        self._pairs: list[tuple[str, str]] = list(pairs) if pairs else []

    def push(self, key: str, value: str) -> None:
        self._pairs.append((key, value))

    def replace_from(self, vars: dict[str, str]) -> None:
        self._pairs = [(str(k), str(v)) for k, v in vars.items()]

    def as_dict(self) -> dict[str, str]:
        return dict(self._pairs)

    def __len__(self) -> int:
        return len(self._pairs)

    def __repr__(self) -> str:
        return f"<{len(self._pairs)} redacted>"


@dataclass
class _GeminiCliConfig:
    binary_path: str = "gemini"
    model: str | None = None
    yolo: bool = False
    sandbox: bool = False
    approval_mode: ApprovalMode | None = None
    include_dirs: list[str] = field(default_factory=list)
    extensions: list[str] = field(default_factory=list)
    allowed_mcp_servers: list[str] = field(default_factory=list)
    resume: str | None = None
    cwd: str | None = None
    envs: _RedactedEnvs = field(default_factory=_RedactedEnvs)


def _model_to_forward(model: str) -> str | None:
    trimmed = model.strip()
    if not trimmed or trimmed.lower() == "default":
        return None
    return trimmed


def _messages_to_prompt(messages: list[Message]) -> tuple[str | None, str]:
    """Flatten messages into ``(system_prompt, user_prompt)``."""
    system: str | None = None
    for message in messages:
        if message.role == Role.system:
            system = message.content
            break

    non_system = [message for message in messages if message.role != Role.system]
    if len(non_system) <= 1:
        prompt = non_system[0].content if non_system else ""
    else:
        labels = {
            Role.user: "[user]",
            Role.assistant: "[assistant]",
            Role.tool: "[tool]",
        }
        prompt = "\n\n".join(
            f"{labels.get(message.role, '[user]')}\n{message.content}" for message in non_system
        )
    return system, prompt


def _merge_system_into_prompt(system: str | None, user: str) -> str:
    if system:
        return f"{system}\n\n{user}"
    return user


def _parse_jsonl_line(line: str) -> list[StreamEvent]:
    """Parse one NDJSON line from ``gemini -o stream-json``."""
    if not line:
        return []
    try:
        event = json.loads(line)
    except json.JSONDecodeError:
        return []

    event_type = event.get("type")

    if event_type == "init":
        session_id = event.get("session_id")
        if isinstance(session_id, str) and session_id:
            return [StreamEvent(content="", done=False, session_id=session_id)]
        return []

    if event_type == "tool_use":
        tool_id = event.get("tool_id") or ""
        tool_name = event.get("tool_name") or "tool_use"
        parameters = event.get("parameters")
        # Compact separators (finding 8) — matches Rust + claude/codex CLI providers.
        args_json = (
            json.dumps(parameters, separators=(",", ":")) if isinstance(parameters, dict) else "{}"
        )
        return [
            StreamEvent(
                content="",
                done=False,
                event_type="tool_call_start",
                tool_call_id=tool_id,
                tool_call_name=tool_name,
            ),
            StreamEvent(
                content="",
                done=False,
                event_type="tool_call_args",
                tool_call_id=tool_id,
                tool_call_args_delta=args_json,
            ),
            StreamEvent(
                content="",
                done=False,
                event_type="tool_call_end",
                tool_call_id=tool_id,
            ),
        ]

    if event_type == "message":
        role = event.get("role")
        delta = event.get("delta", False)
        content = event.get("content", "")
        if role != "assistant" or not delta or not content:
            return []
        return [StreamEvent(content=content, done=False)]

    if event_type == "result":
        status = event.get("status", "success")
        if status != "success":
            raise ProviderError(f"gemini CLI result status: {status}")
        out: list[StreamEvent] = []
        stats = event.get("stats")
        if isinstance(stats, dict):
            cached = stats.get("cached")
            out.append(
                StreamEvent(
                    content="",
                    done=False,
                    event_type="usage",
                    usage=Usage(
                        input_tokens=int(stats.get("input_tokens") or 0),
                        output_tokens=int(stats.get("output_tokens") or 0),
                        cache_read_input_tokens=int(cached) if cached is not None else None,
                    ),
                )
            )
        out.append(StreamEvent(content="", done=True))
        return out

    return []


class GeminiCliClient:
    """Client that shells out to Google's ``gemini`` CLI in headless mode."""

    capabilities: ProviderCapabilities = ProviderCapabilities.text_only()

    def __init__(self, binary_path: str | None = None) -> None:
        if binary_path is None:
            binary_path = os.environ.get("GEMINI_CLI_PATH", "gemini")
        self._config = _GeminiCliConfig(binary_path=binary_path)

    @classmethod
    def with_path(cls, path: str) -> GeminiCliClient:
        return cls(binary_path=path)

    def model(self, model: str) -> GeminiCliClient:
        self._config.model = model
        return self

    def yolo(self, enabled: bool) -> GeminiCliClient:
        self._config.yolo = enabled
        return self

    def sandbox(self, enabled: bool) -> GeminiCliClient:
        self._config.sandbox = enabled
        return self

    def approval_mode(self, mode: ApprovalMode) -> GeminiCliClient:
        self._config.approval_mode = mode
        return self

    def include_dir(self, directory: str) -> GeminiCliClient:
        self._config.include_dirs.append(directory)
        return self

    def include_dirs(self, directories: list[str]) -> GeminiCliClient:
        self._config.include_dirs = list(directories)
        return self

    def extension(self, name: str) -> GeminiCliClient:
        self._config.extensions.append(name)
        return self

    def extensions(self, names: list[str]) -> GeminiCliClient:
        self._config.extensions = list(names)
        return self

    def allowed_mcp_server(self, name: str) -> GeminiCliClient:
        self._config.allowed_mcp_servers.append(name)
        return self

    def allowed_mcp_servers(self, names: list[str]) -> GeminiCliClient:
        self._config.allowed_mcp_servers = list(names)
        return self

    def resume(self, session: str) -> GeminiCliClient:
        """Resume a prior Gemini session (`--resume <id>`).

        Resuming an arbitrary externally supplied id is validated only by the
        skip-guarded live test; the captured-id round-trip is the supported path.
        """
        self._config.resume = session
        return self

    def cwd(self, directory: str) -> GeminiCliClient:
        """Run the spawned ``gemini`` process in ``directory`` (subprocess cwd)."""
        self._config.cwd = directory
        return self

    def env(self, key: str, value: str) -> GeminiCliClient:
        """Inject one environment variable into the child (repeatable; value is a secret)."""
        self._config.envs.push(key, value)
        return self

    def envs(self, vars: dict[str, str]) -> GeminiCliClient:
        """Replace the full set of injected environment variables (does not append)."""
        self._config.envs.replace_from(vars)
        return self

    def _child_env(self) -> dict[str, str] | None:
        if len(self._config.envs) == 0:
            return None
        merged = dict(os.environ)
        merged.update(self._config.envs.as_dict())
        return merged

    def _build_args(self, model: str | None = None) -> list[str]:
        args: list[str] = [self._config.binary_path, "-p", "", "-o", "stream-json"]

        effective_model = self._config.model if model is None else model
        if effective_model is not None:
            forwarded = _model_to_forward(effective_model)
            if forwarded:
                args.extend(["-m", forwarded])
        if self._config.yolo:
            args.append("--yolo")
        if self._config.sandbox:
            args.append("--sandbox")
        if self._config.approval_mode is not None:
            args.extend(["--approval-mode", self._config.approval_mode.value])
        for directory in self._config.include_dirs:
            args.extend(["--include-directories", directory])
        for extension in self._config.extensions:
            if extension.strip():
                args.extend(["-e", extension])
        for name in self._config.allowed_mcp_servers:
            if name.strip():
                args.extend(["--allowed-mcp-server-names", name])
        if self._config.resume is not None:
            trimmed = self._config.resume.strip()
            if trimmed:
                args.extend(["--resume", trimmed])
        return args

    async def chat(self, request: ChatRequest) -> ChatResponse:
        system, user_prompt = _messages_to_prompt(request.messages)
        if request.system:
            system = request.system
        stdin_payload = _merge_system_into_prompt(system, user_prompt)
        args = self._build_args(model=request.model)

        proc = await asyncio.create_subprocess_exec(
            *args,
            stdin=asyncio.subprocess.PIPE,
            stdout=asyncio.subprocess.PIPE,
            stderr=asyncio.subprocess.PIPE,
            cwd=self._config.cwd,
            env=self._child_env(),
        )
        try:
            stdout, stderr = await asyncio.wait_for(
                proc.communicate(stdin_payload.encode()), timeout=_TIMEOUT_SECS
            )
        except TimeoutError as exc:
            proc.kill()
            await proc.wait()
            raise ProviderError(f"gemini CLI timed out after {_TIMEOUT_SECS}s") from exc

        if proc.returncode != 0:
            raise ProviderError(
                f"gemini CLI exited with {proc.returncode}: {stderr.decode().strip()}"
            )

        content_parts: list[str] = []
        usage = Usage(0, 0)
        session_id: str | None = None
        for raw in stdout.decode().splitlines():
            for event in _parse_jsonl_line(raw):
                if event.session_id and session_id is None:
                    session_id = event.session_id
                elif event.event_type == "text" and event.content:
                    content_parts.append(event.content)
                elif event.event_type == "usage" and event.usage is not None:
                    usage = event.usage

        return ChatResponse(
            content="".join(content_parts),
            tool_calls=[],
            model=request.model or self._config.model or "",
            usage=usage,
            stop_reason=StopReason.end_turn,
            session_id=session_id,
        )

    async def stream(self, request: ChatRequest) -> AsyncIterator[StreamEvent]:
        system, user_prompt = _messages_to_prompt(request.messages)
        if request.system:
            system = request.system
        stdin_payload = _merge_system_into_prompt(system, user_prompt)
        args = self._build_args(model=request.model)

        proc = await asyncio.create_subprocess_exec(
            *args,
            stdin=asyncio.subprocess.PIPE,
            stdout=asyncio.subprocess.PIPE,
            stderr=asyncio.subprocess.PIPE,
            cwd=self._config.cwd,
            env=self._child_env(),
        )
        saw_tool_call = False
        try:
            assert proc.stdin is not None and proc.stdout is not None
            proc.stdin.write(stdin_payload.encode())
            with contextlib.suppress(BrokenPipeError, ConnectionResetError):
                await proc.stdin.drain()
            proc.stdin.close()
            with contextlib.suppress(BrokenPipeError, ConnectionResetError, AttributeError):
                await proc.stdin.wait_closed()

            while True:
                raw = await proc.stdout.readline()
                if not raw:
                    break
                line = raw.decode().rstrip("\n")
                for event in _parse_jsonl_line(line):
                    if event.event_type == "tool_call_start":
                        saw_tool_call = True
                    if event.done:
                        event.stop_reason = (
                            StopReason.tool_use if saw_tool_call else StopReason.end_turn
                        )
                    yield event
                    if event.done:
                        return
        finally:
            if proc.returncode is None:
                with contextlib.suppress(ProcessLookupError):
                    proc.kill()
                await proc.wait()
