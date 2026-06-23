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


class SandboxMode(StrEnum):
    read_only = "read-only"
    workspace_write = "workspace-write"
    danger_full_access = "danger-full-access"


class LocalProvider(StrEnum):
    lmstudio = "lmstudio"
    ollama = "ollama"


class _RedactedEnvs(list):
    """list[tuple[str, str]] of (key, value) env pairs whose repr hides values."""

    def __repr__(self) -> str:
        return f"<{len(self)} redacted>"


@dataclass
class _CodexCliConfig:
    binary_path: str = "codex"
    agent_mode: bool = False
    dangerously_bypass_approvals_and_sandbox: bool = False
    sandbox: SandboxMode | None = None
    oss: bool = False
    local_provider: LocalProvider | None = None
    model: str | None = None
    profile: str | None = None
    cd: str | None = None
    add_dirs: list[str] = field(default_factory=list)
    ephemeral: bool = False
    enabled_features: list[str] = field(default_factory=list)
    disabled_features: list[str] = field(default_factory=list)
    config_overrides: list[tuple[str, str]] = field(default_factory=list)
    resume: str | None = None
    envs: _RedactedEnvs = field(default_factory=_RedactedEnvs)


def _model_to_forward(model: str) -> str | None:
    trimmed = model.strip()
    if not trimmed or trimmed.lower() == "default":
        return None
    return trimmed


def _messages_to_prompt(messages: list[Message]) -> tuple[str | None, str]:
    """Flatten messages into ``(system_prompt, user_prompt)`` for Codex stdin."""
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


def _compose_prompt(system: str | None, user: str) -> str:
    """Prepend Codex system instructions using Rust's labeled block format."""
    if system:
        return f"[system instructions]\n{system}\n\n[user]\n{user}"
    return user


def _parse_jsonl_line(line: str) -> list[StreamEvent]:
    """Parse one JSONL line from ``codex exec --json`` into stream events."""
    if not line:
        return []
    try:
        event = json.loads(line)
    except json.JSONDecodeError:
        return []

    event_type = event.get("type")

    if event_type == "item.completed":
        item = event.get("item") or {}
        if not isinstance(item, dict) or item.get("type") != "agent_message":
            return []
        text = item.get("text") or ""
        if not text:
            return []
        return [StreamEvent(content=text, done=False)]

    if event_type == "turn.completed":
        out: list[StreamEvent] = []
        usage_obj = event.get("usage")
        if isinstance(usage_obj, dict):
            cached = usage_obj.get("cached_input_tokens")
            out.append(
                StreamEvent(
                    content="",
                    done=False,
                    event_type="usage",
                    usage=Usage(
                        input_tokens=int(usage_obj.get("input_tokens") or 0),
                        output_tokens=int(usage_obj.get("output_tokens") or 0),
                        cache_read_input_tokens=int(cached) if cached is not None else None,
                    ),
                )
            )
        out.append(StreamEvent(content="", done=True))
        return out

    if event_type == "turn.failed":
        err = event.get("error")
        if isinstance(err, dict):
            msg = err.get("message") or json.dumps(err)
        elif err is not None:
            msg = str(err)
        else:
            msg = "codex turn failed"
        raise ProviderError(f"codex turn failed: {msg}")

    if event_type == "error":
        msg = event.get("message") or "codex error"
        raise ProviderError(f"codex error: {msg}")

    if event_type == "thread.started":
        thread_id = event.get("thread_id")
        if thread_id:
            return [StreamEvent(content="", done=False, session_id=thread_id)]
        return []

    return []


class CodexCliClient:
    """Client that shells out to OpenAI's ``codex exec --json`` CLI."""

    capabilities: ProviderCapabilities = ProviderCapabilities.text_only()

    def __init__(self, binary_path: str | None = None) -> None:
        if binary_path is None:
            binary_path = os.environ.get("CODEX_PATH", "codex")
        self._config = _CodexCliConfig(binary_path=binary_path)

    @classmethod
    def with_path(cls, path: str) -> CodexCliClient:
        return cls(binary_path=path)

    def agent_mode(self, enabled: bool) -> CodexCliClient:
        self._config.agent_mode = enabled
        return self

    def dangerously_bypass_approvals_and_sandbox(self, enabled: bool) -> CodexCliClient:
        self._config.dangerously_bypass_approvals_and_sandbox = enabled
        return self

    def sandbox(self, mode: SandboxMode) -> CodexCliClient:
        self._config.sandbox = mode
        return self

    def oss(self, enabled: bool) -> CodexCliClient:
        self._config.oss = enabled
        return self

    def local_provider(self, provider: LocalProvider) -> CodexCliClient:
        self._config.local_provider = provider
        return self

    def model(self, model: str) -> CodexCliClient:
        self._config.model = model
        return self

    def profile(self, profile: str) -> CodexCliClient:
        self._config.profile = profile
        return self

    def cd(self, directory: str) -> CodexCliClient:
        self._config.cd = directory
        return self

    def add_dir(self, directory: str) -> CodexCliClient:
        self._config.add_dirs.append(directory)
        return self

    def ephemeral(self, enabled: bool) -> CodexCliClient:
        self._config.ephemeral = enabled
        return self

    def enable_feature(self, feature: str) -> CodexCliClient:
        self._config.enabled_features.append(feature)
        return self

    def disable_feature(self, feature: str) -> CodexCliClient:
        self._config.disabled_features.append(feature)
        return self

    def config_override(self, key: str, value: str) -> CodexCliClient:
        self._config.config_overrides.append((key, value))
        return self

    def resume(self, thread_id: str) -> CodexCliClient:
        """Resume a previous Codex thread (`codex exec resume <id>`)."""
        self._config.resume = thread_id
        return self

    def env(self, key: str, value: str) -> CodexCliClient:
        """Inject one env var into the spawned child (repeatable). Value is a secret."""
        self._config.envs.append((key, value))
        return self

    def envs(self, vars: dict[str, str]) -> CodexCliClient:
        """Replace the full set of injected env vars (does not append)."""
        self._config.envs = _RedactedEnvs(vars.items())
        return self

    def _subprocess_env(self) -> dict[str, str] | None:
        if not self._config.envs:
            return None
        merged = dict(os.environ)
        for key, value in self._config.envs:
            merged[key] = value
        return merged

    def _build_args(self, model: str | None = None) -> list[str]:
        args: list[str] = [self._config.binary_path, "exec"]
        if self._config.resume is not None and self._config.resume.strip():
            args.extend(["resume", self._config.resume.strip()])
        args.extend(["--json", "--skip-git-repo-check"])
        if self._config.agent_mode:
            args.append("--full-auto")
        if self._config.dangerously_bypass_approvals_and_sandbox:
            args.append("--dangerously-bypass-approvals-and-sandbox")
        if self._config.sandbox is not None:
            args.extend(["--sandbox", self._config.sandbox.value])
        if self._config.oss:
            args.append("--oss")
        if self._config.local_provider is not None:
            args.extend(["--local-provider", self._config.local_provider.value])
        effective_model = self._config.model if model is None else model
        if effective_model is not None:
            forwarded = _model_to_forward(effective_model)
            if forwarded:
                args.extend(["--model", forwarded])
        if self._config.profile is not None and self._config.profile.strip():
            args.extend(["--profile", self._config.profile])
        if self._config.cd is not None:
            args.extend(["--cd", self._config.cd])
        for directory in self._config.add_dirs:
            args.extend(["--add-dir", directory])
        if self._config.ephemeral:
            args.append("--ephemeral")
        for feature in self._config.enabled_features:
            if feature != "":
                args.extend(["--enable", feature])
        for feature in self._config.disabled_features:
            if feature != "":
                args.extend(["--disable", feature])
        for key, value in self._config.config_overrides:
            args.extend(["-c", f"{key}={value}"])
        args.append("-")
        return args

    async def chat(self, request: ChatRequest) -> ChatResponse:
        msg_system, user_prompt = _messages_to_prompt(request.messages)
        prompt = _compose_prompt(request.system or msg_system, user_prompt)
        args = self._build_args(model=request.model)

        proc = await asyncio.create_subprocess_exec(
            *args,
            stdin=asyncio.subprocess.PIPE,
            stdout=asyncio.subprocess.PIPE,
            stderr=asyncio.subprocess.PIPE,
            env=self._subprocess_env(),
        )
        try:
            stdout, stderr = await asyncio.wait_for(
                proc.communicate(prompt.encode()), timeout=_TIMEOUT_SECS
            )
        except TimeoutError as exc:
            proc.kill()
            await proc.wait()
            raise ProviderError(f"codex CLI timed out after {_TIMEOUT_SECS}s") from exc

        if proc.returncode != 0:
            raise ProviderError(
                f"codex CLI exited with {proc.returncode}: {stderr.decode().strip()}"
            )

        content_parts: list[str] = []
        usage = Usage(0, 0)
        session_id: str | None = None
        for raw in stdout.decode().splitlines():
            for event in _parse_jsonl_line(raw):
                if event.session_id is not None and session_id is None:
                    session_id = event.session_id
                elif event.event_type == "text" and event.content:
                    content_parts.append(event.content)
                elif event.event_type == "usage" and event.usage is not None:
                    usage = event.usage

        return ChatResponse(
            content="".join(content_parts),
            # Codex CLI runs tools internally and does not surface SDK tool calls.
            tool_calls=[],
            model=request.model or self._config.model or "",
            usage=usage,
            stop_reason=StopReason.end_turn,
            session_id=session_id,
        )

    async def stream(self, request: ChatRequest) -> AsyncIterator[StreamEvent]:
        msg_system, user_prompt = _messages_to_prompt(request.messages)
        prompt = _compose_prompt(request.system or msg_system, user_prompt)
        args = self._build_args(model=request.model)

        proc = await asyncio.create_subprocess_exec(
            *args,
            stdin=asyncio.subprocess.PIPE,
            stdout=asyncio.subprocess.PIPE,
            stderr=asyncio.subprocess.PIPE,
            env=self._subprocess_env(),
        )

        try:
            assert proc.stdin is not None and proc.stdout is not None
            proc.stdin.write(prompt.encode())
            with contextlib.suppress(BrokenPipeError, ConnectionResetError):
                await proc.stdin.drain()
            proc.stdin.close()
            with contextlib.suppress(BrokenPipeError, ConnectionResetError, AttributeError):
                await proc.stdin.wait_closed()

            async for raw in proc.stdout:
                line = raw.decode().rstrip("\n")
                for event in _parse_jsonl_line(line):
                    yield event
                    if event.done:
                        return
        finally:
            if proc.returncode is None:
                with contextlib.suppress(ProcessLookupError):
                    proc.kill()
                await proc.wait()
