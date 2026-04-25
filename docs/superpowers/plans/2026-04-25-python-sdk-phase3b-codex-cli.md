# Python SDK Phase 3b — Codex CLI Provider Implementation Plan

> **Status:** ✅ **COMPLETE (2026-04-25)** — shipped as `motosan-ai` v0.9.1.
>
> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Ship `CodexCliClient` — a Python subprocess provider that shells out to OpenAI's `codex exec --json` CLI, mirroring Rust's `CodexCliProvider`. Full builder surface, JSONL stream parsing, registered as `Provider.codex_cli` in `Client` dispatch.

**Architecture:** Mirror the Phase 3a `ClaudeCodeClient` pattern: a `_CodexCliConfig` dataclass holds builder state; fluent builder methods mutate it; `_build_args` emits the argv vector; `chat()` and `stream()` spawn `codex exec --json --skip-git-repo-check ...args -` with the prompt on stdin. JSONL output is parsed event-by-event (`item.completed`, `turn.completed`, `turn.failed`, `error`).

**Tech Stack:** Python 3.11+, `asyncio.subprocess`, stdlib `json`. No new dependencies.

**Ships as:** `motosan-ai` v0.9.1.

---

## Reference material

- **Rust canon (verified before writing):**
  - [sdks/rust/src/providers/codex_cli/mod.rs](sdks/rust/src/providers/codex_cli/mod.rs) — `CodexCliProvider` builder. 11 builder methods + `new` + `with_path`.
  - [sdks/rust/src/providers/codex_cli/spawn.rs](sdks/rust/src/providers/codex_cli/spawn.rs) — `common_args` (lines 124-197) is the canonical argv composition. `SandboxMode` enum at line 26; `LocalProvider` enum at line 54.
  - [sdks/rust/src/providers/codex_cli/stream_json.rs](sdks/rust/src/providers/codex_cli/stream_json.rs) — JSONL event schema (`CodexStreamEvent` at line 25).
  - Base command: `codex exec --json --skip-git-repo-check ...args -` (spawn.rs:227).
  - Env var: `CODEX_PATH` (mod.rs:151), falls back to `"codex"` in PATH.

- **Phase 3a reference:** [sdks/python/motosan_ai/providers/claude_code.py](sdks/python/motosan_ai/providers/claude_code.py) — same shape: dataclass config + fluent builder + `_build_args`. Reuse this skeleton.

- **Verified flag map (from spawn.rs):**

| Builder method | CLI args emitted | Notes |
|---|---|---|
| `agent_mode(bool)` | `--full-auto` | Replaces several safety toggles in one flag |
| `dangerously_bypass_approvals_and_sandbox(bool)` | `--dangerously-bypass-approvals-and-sandbox` | Use only inside an external sandbox |
| `sandbox(SandboxMode)` | `--sandbox <flag>` | flag ∈ `read-only` / `workspace-write` / `danger-full-access` |
| `oss(bool)` | `--oss` | Use OSS provider stack instead of OpenAI cloud |
| `local_provider(LocalProvider)` | `--local-provider <flag>` | flag ∈ `lmstudio` / `ollama` |
| `model(str)` | `--model <value>` | Empty / whitespace / "default" (case-insensitive) skipped |
| `profile(str)` | `--profile <value>` | Empty / whitespace skipped |
| `cd(path)` | `--cd <path>` | Working directory |
| `add_dir(path)` | repeating `--add-dir <path>` | |
| `ephemeral(bool)` | `--ephemeral` | Don't persist session |
| `enable_feature(name)` | repeating `--enable <name>` | Empty skipped |
| `disable_feature(name)` | repeating `--disable <name>` | Empty skipped |
| `config_override(key, value)` | repeating `-c key=value` | Order-stable |

JSONL event handling (verified from stream_json.rs):
- `item.completed` where `item.type == "agent_message"` and `item.text` non-empty → emit text `StreamEvent`
- `turn.completed` → emit `StreamEvent(usage)` (when `usage` present) followed by `StreamEvent(done=True)`
- `turn.failed` (with `error` payload) / `error` (top-level, with `message`) → raise `ProviderError`
- Everything else (`thread.started`, `item.started`, etc.) → ignore

---

## File Structure

| Path | Responsibility | Status |
|------|----------------|--------|
| `sdks/python/motosan_ai/providers/codex_cli.py` | `CodexCliClient`, `_CodexCliConfig`, `SandboxMode`, `LocalProvider`, `_build_args`, `_parse_jsonl_line`, `chat`, `stream` | **Create** |
| `sdks/python/motosan_ai/providers/__init__.py` | Export `CodexCliClient`, `SandboxMode`, `LocalProvider` | **Modify** |
| `sdks/python/motosan_ai/__init__.py` | Top-level export of `CodexCliClient` + enums | **Modify** |
| `sdks/python/motosan_ai/client.py` | Register `Provider.codex_cli`, classmethod `Client.codex_cli()`, `CODEX_PATH` env var | **Modify** |
| `sdks/python/tests/test_codex_cli_flags.py` | Per-flag `_build_args` assertions | **Create** |
| `sdks/python/tests/test_codex_cli_stream.py` | JSONL parser + stream behavior | **Create** |
| `sdks/python/tests/test_codex_cli_dispatch.py` | `Provider.codex_cli` dispatch + env-var resolution | **Create** |
| `sdks/python/tests/integration/test_codex_cli_live.py` | Live tests behind `codex` binary on PATH | **Create** |
| `sdks/python/CHANGELOG.md` | v0.9.1 entry | **Modify** |
| `sdks/python/pyproject.toml` | Version bump 0.9.0 → 0.9.1 | **Modify** |

Design principles:
- **Mirror Phase 3a structure exactly.** Reuse the pattern that worked: dataclass config + property shims (none needed here — fresh module) + fluent builders + flat `_build_args`.
- **Enums via `StrEnum`.** `SandboxMode("read-only")`, `LocalProvider("lmstudio")` — values are the wire flags so `args.append(mode.value)` is trivially correct.
- **Test wire shape per flag.** Every builder method has a corresponding `assert "--foo" in args` test before any code lands. No assumptions about flag spelling — every string is grepped from `spawn.rs`.
- **No live test as gate for unit suite.** Live tests skip cleanly if `codex` not on PATH; CI default green without the binary.
- **Provider name `codex_cli`.** Matches the snake_case StrEnum convention already used by `claude_code` (well, `claude_code` exports as `ClaudeCodeClient` and isn't in the Provider enum — but Codex CLI gets a Provider enum entry per roadmap).

---

## Task 1: Module skeleton + `_CodexCliConfig` dataclass

**Files:**
- Create: `sdks/python/motosan_ai/providers/codex_cli.py`
- Create: `sdks/python/tests/test_codex_cli_flags.py`

Minimal class + dataclass + binary path resolution. Mirrors `ClaudeCodeClient` opening shape.

- [ ] **Step 1: Write failing test**

Create `sdks/python/tests/test_codex_cli_flags.py`:

```python
from __future__ import annotations

import os

from motosan_ai.providers.codex_cli import CodexCliClient


def _args(client: CodexCliClient) -> list[str]:
    return client._build_args()


def test_default_binary_path_is_codex_or_env():
    c = CodexCliClient()
    assert c._config.binary_path in ("codex", os.environ.get("CODEX_PATH", "codex"))


def test_explicit_binary_path():
    c = CodexCliClient(binary_path="/opt/codex")
    assert c._config.binary_path == "/opt/codex"


def test_with_path_classmethod():
    c = CodexCliClient.with_path("/opt/codex-2")
    assert c._config.binary_path == "/opt/codex-2"


def test_minimal_args_emit_exec_json_skip_check():
    c = CodexCliClient(binary_path="codex")
    assert _args(c) == ["codex", "exec", "--json", "--skip-git-repo-check", "-"]
```

- [ ] **Step 2: Run — should FAIL (module not found)**

Run: `cd sdks/python && uv run pytest tests/test_codex_cli_flags.py -v`
Expected: `ModuleNotFoundError: No module named 'motosan_ai.providers.codex_cli'`.

- [ ] **Step 3: Create `codex_cli.py` skeleton**

Create `sdks/python/motosan_ai/providers/codex_cli.py`:

```python
from __future__ import annotations

import os
from dataclasses import dataclass, field
from enum import StrEnum

from motosan_ai.provider_base import ProviderCapabilities


@dataclass
class _CodexCliConfig:
    binary_path: str = "codex"


class CodexCliClient:
    """Client that shells out to OpenAI's ``codex exec --json`` CLI.

    Mirrors Rust's ``CodexCliProvider``. Builder methods land in subsequent
    tasks; this module starts with the binary-path skeleton.
    """

    capabilities: ProviderCapabilities = ProviderCapabilities.text_only()

    def __init__(self, binary_path: str | None = None) -> None:
        if binary_path is None:
            binary_path = os.environ.get("CODEX_PATH", "codex")
        self._config = _CodexCliConfig(binary_path=binary_path)

    @classmethod
    def with_path(cls, path: str) -> CodexCliClient:
        return cls(binary_path=path)

    def _build_args(self) -> list[str]:
        return [
            self._config.binary_path,
            "exec",
            "--json",
            "--skip-git-repo-check",
            "-",
        ]
```

- [ ] **Step 4: Run — PASS**

Run: `cd sdks/python && uv run pytest tests/test_codex_cli_flags.py -v`
Expected: 4 PASS.

- [ ] **Step 5: Commit**

```bash
git add sdks/python/motosan_ai/providers/codex_cli.py sdks/python/tests/test_codex_cli_flags.py
git commit -m "feat(python,codex-cli): module skeleton + _CodexCliConfig + binary path resolution"
```

---

## Task 2: `SandboxMode` + `LocalProvider` enums

**Files:**
- Modify: `sdks/python/motosan_ai/providers/codex_cli.py`
- Modify: `sdks/python/tests/test_codex_cli_flags.py`

Two `StrEnum`s. Values are the exact wire flags so `mode.value` plugs directly into argv.

- [ ] **Step 1: Append failing tests**

```python
from motosan_ai.providers.codex_cli import LocalProvider, SandboxMode


def test_sandbox_mode_values_match_cli_flags():
    assert SandboxMode.read_only.value == "read-only"
    assert SandboxMode.workspace_write.value == "workspace-write"
    assert SandboxMode.danger_full_access.value == "danger-full-access"


def test_local_provider_values_match_cli_flags():
    assert LocalProvider.lmstudio.value == "lmstudio"
    assert LocalProvider.ollama.value == "ollama"
```

- [ ] **Step 2: Run — FAIL (ImportError)**

Run: `cd sdks/python && uv run pytest tests/test_codex_cli_flags.py -v -k "sandbox or local"`
Expected: ImportError.

- [ ] **Step 3: Add enums**

In `codex_cli.py`, after the imports:

```python
class SandboxMode(StrEnum):
    read_only = "read-only"
    workspace_write = "workspace-write"
    danger_full_access = "danger-full-access"


class LocalProvider(StrEnum):
    lmstudio = "lmstudio"
    ollama = "ollama"
```

- [ ] **Step 4: Run — PASS**

Run: `cd sdks/python && uv run pytest tests/test_codex_cli_flags.py -v`
Expected: 6 PASS.

- [ ] **Step 5: Commit**

```bash
git add sdks/python/motosan_ai/providers/codex_cli.py sdks/python/tests/test_codex_cli_flags.py
git commit -m "feat(python,codex-cli): add SandboxMode and LocalProvider enums"
```

---

## Task 3: Boolean flags — `agent_mode`, `dangerously_bypass_approvals_and_sandbox`, `oss`, `ephemeral`

**Files:**
- Modify: `sdks/python/motosan_ai/providers/codex_cli.py`
- Modify: `sdks/python/tests/test_codex_cli_flags.py`

Four pure-boolean flags. From spawn.rs:
- `agent_mode(true)` → `--full-auto`
- `dangerously_bypass_approvals_and_sandbox(true)` → `--dangerously-bypass-approvals-and-sandbox`
- `oss(true)` → `--oss`
- `ephemeral(true)` → `--ephemeral`

- [ ] **Step 1: Append failing tests**

```python
def test_agent_mode_emits_full_auto():
    c = CodexCliClient(binary_path="codex").agent_mode(True)
    assert "--full-auto" in _args(c)


def test_agent_mode_absent_by_default():
    assert "--full-auto" not in _args(CodexCliClient(binary_path="codex"))


def test_dangerously_bypass_flag():
    c = CodexCliClient(binary_path="codex").dangerously_bypass_approvals_and_sandbox(True)
    assert "--dangerously-bypass-approvals-and-sandbox" in _args(c)


def test_oss_flag():
    c = CodexCliClient(binary_path="codex").oss(True)
    assert "--oss" in _args(c)


def test_ephemeral_flag():
    c = CodexCliClient(binary_path="codex").ephemeral(True)
    assert "--ephemeral" in _args(c)
```

- [ ] **Step 2: Run — FAIL**

Run: `cd sdks/python && uv run pytest tests/test_codex_cli_flags.py -v`
Expected: AttributeError on the missing methods.

- [ ] **Step 3: Extend `_CodexCliConfig` + add builder methods + extend `_build_args`**

Extend dataclass:

```python
@dataclass
class _CodexCliConfig:
    binary_path: str = "codex"
    agent_mode: bool = False
    dangerously_bypass_approvals_and_sandbox: bool = False
    oss: bool = False
    ephemeral: bool = False
```

Add to `CodexCliClient`:

```python
    def agent_mode(self, enabled: bool) -> CodexCliClient:
        self._config.agent_mode = enabled
        return self

    def dangerously_bypass_approvals_and_sandbox(self, enabled: bool) -> CodexCliClient:
        self._config.dangerously_bypass_approvals_and_sandbox = enabled
        return self

    def oss(self, enabled: bool) -> CodexCliClient:
        self._config.oss = enabled
        return self

    def ephemeral(self, enabled: bool) -> CodexCliClient:
        self._config.ephemeral = enabled
        return self
```

Replace `_build_args` to inject the flags **between** `--skip-git-repo-check` and `-`. Order matches Rust spawn.rs:

```python
    def _build_args(self) -> list[str]:
        args: list[str] = [
            self._config.binary_path,
            "exec",
            "--json",
            "--skip-git-repo-check",
        ]
        if self._config.agent_mode:
            args.append("--full-auto")
        if self._config.dangerously_bypass_approvals_and_sandbox:
            args.append("--dangerously-bypass-approvals-and-sandbox")
        if self._config.oss:
            args.append("--oss")
        if self._config.ephemeral:
            args.append("--ephemeral")
        args.append("-")
        return args
```

- [ ] **Step 4: Run — PASS**

Run: `cd sdks/python && uv run pytest tests/test_codex_cli_flags.py -v`
Expected: 11 PASS.

- [ ] **Step 5: Commit**

```bash
git add sdks/python/motosan_ai/providers/codex_cli.py sdks/python/tests/test_codex_cli_flags.py
git commit -m "feat(python,codex-cli): add agent_mode, dangerous_bypass, oss, ephemeral boolean flags"
```

---

## Task 4: Single-value flags — `sandbox`, `local_provider`, `model`, `profile`, `cd`

**Files:**
- Modify: `sdks/python/motosan_ai/providers/codex_cli.py`
- Modify: `sdks/python/tests/test_codex_cli_flags.py`

Five flags that take one value. `model` and `profile` apply the same skip-empty rule as Rust (`spawn.rs:206-217`): empty / whitespace / case-insensitive `"default"` → no flag emitted.

- [ ] **Step 1: Append failing tests**

```python
def test_sandbox_flag_with_mode():
    c = CodexCliClient(binary_path="codex").sandbox(SandboxMode.workspace_write)
    args = _args(c)
    i = args.index("--sandbox")
    assert args[i + 1] == "workspace-write"


def test_local_provider_flag():
    c = CodexCliClient(binary_path="codex").local_provider(LocalProvider.ollama)
    args = _args(c)
    i = args.index("--local-provider")
    assert args[i + 1] == "ollama"


def test_model_flag_emitted():
    c = CodexCliClient(binary_path="codex").model("gpt-5.1-codex")
    args = _args(c)
    i = args.index("--model")
    assert args[i + 1] == "gpt-5.1-codex"


def test_model_default_sentinel_skipped():
    for sentinel in ("", "   ", "default", "Default", "DEFAULT"):
        c = CodexCliClient(binary_path="codex").model(sentinel)
        assert "--model" not in _args(c), f"unexpected --model for {sentinel!r}"


def test_profile_flag_emitted():
    c = CodexCliClient(binary_path="codex").profile("work")
    args = _args(c)
    assert args[args.index("--profile") + 1] == "work"


def test_profile_empty_skipped():
    c = CodexCliClient(binary_path="codex").profile("   ")
    assert "--profile" not in _args(c)


def test_cd_flag_emitted():
    c = CodexCliClient(binary_path="codex").cd("/tmp/project")
    args = _args(c)
    assert args[args.index("--cd") + 1] == "/tmp/project"
```

- [ ] **Step 2: Run — FAIL**

Run: `cd sdks/python && uv run pytest tests/test_codex_cli_flags.py -v -k "sandbox_flag or local_provider_flag or model or profile or cd_flag"`
Expected: AttributeError.

- [ ] **Step 3: Add module helper + extend dataclass + builder methods + `_build_args`**

Add helper near the enums:

```python
def _model_to_forward(model: str) -> str | None:
    trimmed = model.strip()
    if not trimmed or trimmed.lower() == "default":
        return None
    return trimmed
```

Extend dataclass:

```python
@dataclass
class _CodexCliConfig:
    binary_path: str = "codex"
    agent_mode: bool = False
    dangerously_bypass_approvals_and_sandbox: bool = False
    oss: bool = False
    ephemeral: bool = False
    sandbox: SandboxMode | None = None
    local_provider: LocalProvider | None = None
    model: str | None = None
    profile: str | None = None
    cd: str | None = None
```

Add builder methods:

```python
    def sandbox(self, mode: SandboxMode) -> CodexCliClient:
        self._config.sandbox = mode
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

    def cd(self, dir: str) -> CodexCliClient:
        self._config.cd = dir
        return self
```

Replace `_build_args` body — order matches Rust `common_args`:

```python
    def _build_args(self) -> list[str]:
        args: list[str] = [
            self._config.binary_path,
            "exec",
            "--json",
            "--skip-git-repo-check",
        ]
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
        if self._config.model is not None:
            forwarded = _model_to_forward(self._config.model)
            if forwarded:
                args.extend(["--model", forwarded])
        if self._config.profile is not None and self._config.profile.strip():
            args.extend(["--profile", self._config.profile])
        if self._config.cd is not None:
            args.extend(["--cd", self._config.cd])
        if self._config.ephemeral:
            args.append("--ephemeral")
        args.append("-")
        return args
```

- [ ] **Step 4: Run — PASS**

Run: `cd sdks/python && uv run pytest tests/test_codex_cli_flags.py -v`
Expected: 18 PASS.

- [ ] **Step 5: Commit**

```bash
git add sdks/python/motosan_ai/providers/codex_cli.py sdks/python/tests/test_codex_cli_flags.py
git commit -m "feat(python,codex-cli): add sandbox, local_provider, model, profile, cd"
```

---

## Task 5: List flags — `add_dir`, `enable_feature`, `disable_feature`

**Files:**
- Modify: `sdks/python/motosan_ai/providers/codex_cli.py`
- Modify: `sdks/python/tests/test_codex_cli_flags.py`

Three repeating flags. `enable_feature` and `disable_feature` skip empty strings (matches spawn.rs:178, 185).

- [ ] **Step 1: Append failing tests**

```python
def test_add_dir_appends():
    c = CodexCliClient(binary_path="codex").add_dir("/tmp/a").add_dir("/tmp/b")
    args = _args(c)
    occ = [args[i + 1] for i, a in enumerate(args) if a == "--add-dir"]
    assert occ == ["/tmp/a", "/tmp/b"]


def test_enable_feature_appends():
    c = CodexCliClient(binary_path="codex").enable_feature("foo").enable_feature("bar")
    args = _args(c)
    occ = [args[i + 1] for i, a in enumerate(args) if a == "--enable"]
    assert occ == ["foo", "bar"]


def test_disable_feature_appends():
    c = CodexCliClient(binary_path="codex").disable_feature("baz")
    args = _args(c)
    occ = [args[i + 1] for i, a in enumerate(args) if a == "--disable"]
    assert occ == ["baz"]


def test_empty_features_skipped():
    c = (
        CodexCliClient(binary_path="codex")
        .enable_feature("")
        .disable_feature("   ")
    )
    args = _args(c)
    assert "--enable" not in args
    # Note: Rust skips only empty strings, not whitespace-only — match Rust exactly
    # The "   " case still emits --disable per Rust spawn.rs:184.
    # Adjust test if Rust semantics differ.
```

Hold on — verify Rust exact semantics for empty-feature skip. Per spawn.rs:177-189: `if !feature.is_empty()` — checks for empty string only, not trimmed. Whitespace-only feature names DO pass through. Update the test:

Replace the `test_empty_features_skipped` block above with:

```python
def test_empty_feature_skipped():
    c = CodexCliClient(binary_path="codex").enable_feature("")
    assert "--enable" not in _args(c)


def test_whitespace_feature_passes_through():
    """Rust spawn.rs:177 only skips empty strings, not whitespace-only."""
    c = CodexCliClient(binary_path="codex").disable_feature("   ")
    args = _args(c)
    assert "--disable" in args
    assert args[args.index("--disable") + 1] == "   "
```

- [ ] **Step 2: Run — FAIL**

Run: `cd sdks/python && uv run pytest tests/test_codex_cli_flags.py -v -k "add_dir or feature"`
Expected: AttributeError.

- [ ] **Step 3: Extend dataclass + methods + `_build_args`**

Dataclass:

```python
    add_dirs: list[str] = field(default_factory=list)
    enabled_features: list[str] = field(default_factory=list)
    disabled_features: list[str] = field(default_factory=list)
```

Methods:

```python
    def add_dir(self, dir: str) -> CodexCliClient:
        self._config.add_dirs.append(dir)
        return self

    def enable_feature(self, feature: str) -> CodexCliClient:
        self._config.enabled_features.append(feature)
        return self

    def disable_feature(self, feature: str) -> CodexCliClient:
        self._config.disabled_features.append(feature)
        return self
```

In `_build_args`, after the `cd` block and before `ephemeral`:

```python
        for d in self._config.add_dirs:
            args.extend(["--add-dir", d])
```

After `ephemeral`:

```python
        for feature in self._config.enabled_features:
            if feature != "":
                args.extend(["--enable", feature])
        for feature in self._config.disabled_features:
            if feature != "":
                args.extend(["--disable", feature])
```

- [ ] **Step 4: Run — PASS**

Run: `cd sdks/python && uv run pytest tests/test_codex_cli_flags.py -v`
Expected: ~22 PASS.

- [ ] **Step 5: Commit**

```bash
git add sdks/python/motosan_ai/providers/codex_cli.py sdks/python/tests/test_codex_cli_flags.py
git commit -m "feat(python,codex-cli): add add_dir, enable_feature, disable_feature (variadic)"
```

---

## Task 6: `config_override(key, value)` — repeating `-c key=value`

**Files:**
- Modify: `sdks/python/motosan_ai/providers/codex_cli.py`
- Modify: `sdks/python/tests/test_codex_cli_flags.py`

Each call appends a `-c key=value` pair. Order is stable (insertion order).

- [ ] **Step 1: Append failing tests**

```python
def test_config_override_emits_minus_c():
    c = (
        CodexCliClient(binary_path="codex")
        .config_override("model_reasoning_effort", "high")
        .config_override("approval_policy", "never")
    )
    args = _args(c)
    occ = [args[i + 1] for i, a in enumerate(args) if a == "-c"]
    assert occ == [
        "model_reasoning_effort=high",
        "approval_policy=never",
    ]


def test_config_override_handles_special_chars_in_value():
    c = CodexCliClient(binary_path="codex").config_override("env.MY_VAR", 'a "b" c')
    args = _args(c)
    i = args.index("-c")
    assert args[i + 1] == 'env.MY_VAR=a "b" c'
```

- [ ] **Step 2: Run — FAIL**

Run: `cd sdks/python && uv run pytest tests/test_codex_cli_flags.py -v -k config_override`
Expected: AttributeError.

- [ ] **Step 3: Extend**

Dataclass:

```python
    config_overrides: list[tuple[str, str]] = field(default_factory=list)
```

Method:

```python
    def config_override(self, key: str, value: str) -> CodexCliClient:
        self._config.config_overrides.append((key, value))
        return self
```

In `_build_args`, after the `disable_feature` loop:

```python
        for key, value in self._config.config_overrides:
            args.extend(["-c", f"{key}={value}"])
```

- [ ] **Step 4: Run — PASS**

Run: `cd sdks/python && uv run pytest tests/test_codex_cli_flags.py -v`
Expected: ~24 PASS.

- [ ] **Step 5: Commit**

```bash
git add sdks/python/motosan_ai/providers/codex_cli.py sdks/python/tests/test_codex_cli_flags.py
git commit -m "feat(python,codex-cli): add config_override(key, value) -> -c key=value"
```

---

## Task 7: Argv composition smoke test — full config

**Files:**
- Modify: `sdks/python/tests/test_codex_cli_flags.py`

One end-to-end test pinning the argv ordering for a maximally-configured client. Acts as a tripwire for any future reordering bug.

- [ ] **Step 1: Append the smoke test**

```python
def test_full_config_argv_order_matches_rust_common_args():
    """End-to-end argv shape — pins the order documented by Rust spawn.rs:124-197.

    Order: --full-auto, --dangerously-bypass-approvals-and-sandbox,
    --sandbox <m>, --oss, --local-provider <p>, --model <m>,
    --profile <p>, --cd <d>, --add-dir <d>..., --ephemeral,
    --enable <f>..., --disable <f>..., -c key=value...
    """
    c = (
        CodexCliClient(binary_path="codex")
        .agent_mode(True)
        .dangerously_bypass_approvals_and_sandbox(True)
        .sandbox(SandboxMode.workspace_write)
        .oss(True)
        .local_provider(LocalProvider.ollama)
        .model("gpt-5.1-codex")
        .profile("work")
        .cd("/tmp/proj")
        .add_dir("/tmp/extra1")
        .add_dir("/tmp/extra2")
        .ephemeral(True)
        .enable_feature("foo")
        .disable_feature("bar")
        .config_override("approval_policy", "never")
    )
    assert _args(c) == [
        "codex",
        "exec",
        "--json",
        "--skip-git-repo-check",
        "--full-auto",
        "--dangerously-bypass-approvals-and-sandbox",
        "--sandbox",
        "workspace-write",
        "--oss",
        "--local-provider",
        "ollama",
        "--model",
        "gpt-5.1-codex",
        "--profile",
        "work",
        "--cd",
        "/tmp/proj",
        "--add-dir",
        "/tmp/extra1",
        "--add-dir",
        "/tmp/extra2",
        "--ephemeral",
        "--enable",
        "foo",
        "--disable",
        "bar",
        "-c",
        "approval_policy=never",
        "-",
    ]
```

- [ ] **Step 2: Run — should PASS already**

Run: `cd sdks/python && uv run pytest tests/test_codex_cli_flags.py::test_full_config_argv_order_matches_rust_common_args -v`
Expected: PASS (the previous tasks built this argv composition correctly). If FAIL, the order in `_build_args` from Task 4-6 is wrong relative to Rust — fix `_build_args` to match the assertion list.

- [ ] **Step 3: Commit**

```bash
git add sdks/python/tests/test_codex_cli_flags.py
git commit -m "test(python,codex-cli): pin full-config argv order against Rust common_args"
```

---

## Task 8: JSONL parser — `_parse_jsonl_line`

**Files:**
- Modify: `sdks/python/motosan_ai/providers/codex_cli.py`
- Create: `sdks/python/tests/test_codex_cli_stream.py`

Parse a single line of `codex exec --json` output into 0, 1, or 2 `StreamEvent`s plus an error sentinel for `turn.failed` / `error`. Mirror Rust `NdjsonAction` semantics.

- [ ] **Step 1: Write failing tests**

Create `sdks/python/tests/test_codex_cli_stream.py`:

```python
from __future__ import annotations

import pytest

from motosan_ai.error import ProviderError
from motosan_ai.providers.codex_cli import _parse_jsonl_line
from motosan_ai.types import StreamEvent


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
    line = '{"type": "turn.completed"}'
    events = _parse_jsonl_line(line)
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
    line = '{"type": "thread.started", "thread_id": "t1"}'
    assert _parse_jsonl_line(line) == []


def test_malformed_json_returns_empty():
    assert _parse_jsonl_line("not json {") == []
    assert _parse_jsonl_line("") == []
```

- [ ] **Step 2: Run — FAIL**

Run: `cd sdks/python && uv run pytest tests/test_codex_cli_stream.py -v`
Expected: ImportError on `_parse_jsonl_line`.

- [ ] **Step 3: Implement parser**

Add to `codex_cli.py` (top-level helper, near `_model_to_forward`):

```python
import json

from motosan_ai.error import ProviderError
from motosan_ai.types import StreamEvent, Usage


def _parse_jsonl_line(line: str) -> list[StreamEvent]:
    """Parse one JSONL line from ``codex exec --json``.

    Returns 0, 1, or 2 events:
      * `item.completed` with `item.type=="agent_message"` and non-empty
        text → 1 text event
      * `turn.completed` → 1 done event, optionally preceded by a usage
        event when `usage` is present
      * Anything else (`thread.started`, `item.started`, non-message
        items, malformed JSON) → empty list

    Raises ``ProviderError`` for `turn.failed` and top-level `error`
    events.
    """
    if not line:
        return []
    try:
        event = json.loads(line)
    except json.JSONDecodeError:
        return []

    event_type = event.get("type")

    if event_type == "item.completed":
        item = event.get("item") or {}
        if item.get("type") != "agent_message":
            return []
        text = item.get("text") or ""
        if not text:
            return []
        return [StreamEvent(content=text, done=False)]

    if event_type == "turn.completed":
        out: list[StreamEvent] = []
        usage_obj = event.get("usage")
        if isinstance(usage_obj, dict):
            out.append(
                StreamEvent(
                    content="",
                    done=False,
                    event_type="usage",
                    usage=Usage(
                        input_tokens=int(usage_obj.get("input_tokens") or 0),
                        output_tokens=int(usage_obj.get("output_tokens") or 0),
                        cache_read_input_tokens=usage_obj.get("cached_input_tokens"),
                    ),
                )
            )
        out.append(StreamEvent(content="", done=True))
        return out

    if event_type == "turn.failed":
        err = event.get("error") or {}
        msg = err.get("message") if isinstance(err, dict) else None
        msg = msg or json.dumps(err) if err else "codex turn failed"
        raise ProviderError(f"codex turn failed: {msg}")

    if event_type == "error":
        msg = event.get("message") or "codex error"
        raise ProviderError(f"codex error: {msg}")

    return []
```

- [ ] **Step 4: Run — PASS**

Run: `cd sdks/python && uv run pytest tests/test_codex_cli_stream.py -v`
Expected: 9 PASS.

- [ ] **Step 5: Commit**

```bash
git add sdks/python/motosan_ai/providers/codex_cli.py sdks/python/tests/test_codex_cli_stream.py
git commit -m "feat(python,codex-cli): JSONL parser for item.completed / turn.completed / errors"
```

---

## Task 9: `chat()` — subprocess + stdin prompt + collect stream

**Files:**
- Modify: `sdks/python/motosan_ai/providers/codex_cli.py`
- Modify: `sdks/python/tests/test_codex_cli_stream.py`

Spawn `codex exec --json ...args -`, write the prompt to stdin, read stdout line-by-line, accumulate text from agent_message events, return a `ChatResponse`. Mirrors the existing `ClaudeCodeClient.chat`.

- [ ] **Step 1: Write failing tests using `MonkeyPatch` for subprocess**

Append to `sdks/python/tests/test_codex_cli_stream.py`:

```python
import asyncio
from unittest.mock import AsyncMock, MagicMock

from motosan_ai.providers.codex_cli import CodexCliClient
from motosan_ai.types import ChatRequest, Message


class _FakeProc:
    def __init__(self, stdout: str, returncode: int = 0, stderr: str = "") -> None:
        self._stdout = stdout.encode()
        self._stderr = stderr.encode()
        self.returncode = returncode

    async def communicate(self, input: bytes | None = None) -> tuple[bytes, bytes]:
        return self._stdout, self._stderr

    def kill(self) -> None: ...
    async def wait(self) -> int:
        return self.returncode


def _stub_subprocess(monkeypatch, fake: _FakeProc) -> None:
    async def fake_create(*args, **kwargs):
        return fake

    monkeypatch.setattr(asyncio.subprocess, "PIPE", -1, raising=False)
    monkeypatch.setattr(
        "asyncio.create_subprocess_exec",
        AsyncMock(return_value=fake),
    )


@pytest.mark.asyncio
async def test_chat_returns_concatenated_text(monkeypatch):
    jsonl = (
        '{"type": "item.completed", "item": {"type": "agent_message", "text": "Hello "}}\n'
        '{"type": "item.completed", "item": {"type": "agent_message", "text": "world."}}\n'
        '{"type": "turn.completed", "usage": {"input_tokens": 10, "output_tokens": 5}}\n'
    )
    _stub_subprocess(monkeypatch, _FakeProc(jsonl))

    client = CodexCliClient(binary_path="codex")
    req = ChatRequest(messages=[Message.user("hi")])
    resp = await client.chat(req)
    assert resp.content == "Hello world."
    assert resp.usage.input_tokens == 10
    assert resp.usage.output_tokens == 5


@pytest.mark.asyncio
async def test_chat_raises_on_nonzero_returncode(monkeypatch):
    _stub_subprocess(
        monkeypatch, _FakeProc("", returncode=2, stderr="codex: bad config\n")
    )
    client = CodexCliClient(binary_path="codex")
    with pytest.raises(ProviderError, match="bad config"):
        await client.chat(ChatRequest(messages=[Message.user("hi")]))
```

- [ ] **Step 2: Run — FAIL**

Run: `cd sdks/python && uv run pytest tests/test_codex_cli_stream.py -v -k "chat_returns or chat_raises"`
Expected: AttributeError on `client.chat` or NotImplementedError.

- [ ] **Step 3: Implement `chat()`**

Add to `CodexCliClient`:

```python
import asyncio

from motosan_ai.types import ChatRequest, ChatResponse, Role, StopReason

_TIMEOUT_SECS = 600  # match Rust spawn.rs::TIMEOUT_SECS


def _messages_to_prompt(messages: list[Message]) -> str:
    """Flatten messages into a single user prompt string for codex stdin."""
    parts: list[str] = []
    for m in messages:
        if m.role == Role.system:
            parts.append(f"[system]\n{m.content}")
        elif m.role == Role.user:
            parts.append(m.content if len(messages) == 1 else f"[user]\n{m.content}")
        elif m.role == Role.assistant:
            parts.append(f"[assistant]\n{m.content}")
        elif m.role == Role.tool:
            parts.append(f"[tool]\n{m.content}")
    return "\n\n".join(parts)


    async def chat(self, request: ChatRequest) -> ChatResponse:
        prompt = _messages_to_prompt(request.messages)
        args = self._build_args()

        proc = await asyncio.create_subprocess_exec(
            *args,
            stdin=asyncio.subprocess.PIPE,
            stdout=asyncio.subprocess.PIPE,
            stderr=asyncio.subprocess.PIPE,
        )
        try:
            stdout, stderr = await asyncio.wait_for(
                proc.communicate(prompt.encode()),
                timeout=_TIMEOUT_SECS,
            )
        except TimeoutError as exc:
            proc.kill()
            await proc.wait()
            raise ProviderError(f"codex CLI timed out after {_TIMEOUT_SECS}s") from exc

        if proc.returncode != 0:
            raise ProviderError(
                f"codex CLI exited with {proc.returncode}: {stderr.decode().strip()}"
            )

        content = ""
        usage = Usage(0, 0)
        for raw in stdout.decode().splitlines():
            for event in _parse_jsonl_line(raw):
                if event.event_type == "text" and event.content:
                    content += event.content
                if event.event_type == "usage" and event.usage is not None:
                    usage = event.usage

        return ChatResponse(
            content=content,
            model=request.model or "",
            usage=usage,
            stop_reason=StopReason.end_turn,
        )
```

Place `_messages_to_prompt`, `_TIMEOUT_SECS` at module scope; `chat` is a method on `CodexCliClient`.

- [ ] **Step 4: Run — PASS**

Run: `cd sdks/python && uv run pytest tests/test_codex_cli_stream.py -v`
Expected: PASS for both new chat tests.

- [ ] **Step 5: Commit**

```bash
git add sdks/python/motosan_ai/providers/codex_cli.py sdks/python/tests/test_codex_cli_stream.py
git commit -m "feat(python,codex-cli): chat() subprocess + JSONL collect"
```

---

## Task 10: `stream()` — yield events line-by-line

**Files:**
- Modify: `sdks/python/motosan_ai/providers/codex_cli.py`
- Modify: `sdks/python/tests/test_codex_cli_stream.py`

Same subprocess setup, but yield each event as it arrives instead of buffering.

- [ ] **Step 1: Append failing test**

```python
@pytest.mark.asyncio
async def test_stream_yields_events_in_order(monkeypatch):
    jsonl = (
        '{"type": "thread.started", "thread_id": "t1"}\n'
        '{"type": "item.completed", "item": {"type": "agent_message", "text": "hi"}}\n'
        '{"type": "turn.completed", "usage": {"input_tokens": 3, "output_tokens": 1}}\n'
    )
    _stub_subprocess(monkeypatch, _FakeProc(jsonl))

    client = CodexCliClient(binary_path="codex")
    events = [
        e
        async for e in client.stream(ChatRequest(messages=[Message.user("hi")]))
    ]

    text_events = [e for e in events if e.event_type == "text" and not e.done]
    usage_events = [e for e in events if e.event_type == "usage"]
    done_events = [e for e in events if e.done]

    assert [e.content for e in text_events] == ["hi"]
    assert len(usage_events) == 1 and usage_events[0].usage.input_tokens == 3
    assert len(done_events) == 1
    # Order: text → usage → done
    assert events.index(text_events[0]) < events.index(usage_events[0])
    assert events.index(usage_events[0]) < events.index(done_events[0])
```

- [ ] **Step 2: Run — FAIL**

Run: `cd sdks/python && uv run pytest tests/test_codex_cli_stream.py::test_stream_yields_events_in_order -v`
Expected: AttributeError.

- [ ] **Step 3: Implement `stream()`**

Add to `CodexCliClient`:

```python
from collections.abc import AsyncIterator
import contextlib

from motosan_ai.types import StreamEvent


    async def stream(self, request: ChatRequest) -> AsyncIterator[StreamEvent]:
        prompt = _messages_to_prompt(request.messages)
        args = self._build_args()

        proc = await asyncio.create_subprocess_exec(
            *args,
            stdin=asyncio.subprocess.PIPE,
            stdout=asyncio.subprocess.PIPE,
            stderr=asyncio.subprocess.PIPE,
        )

        try:
            assert proc.stdin is not None and proc.stdout is not None
            proc.stdin.write(prompt.encode())
            with contextlib.suppress(BrokenPipeError):
                await proc.stdin.drain()
            proc.stdin.close()

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
```

- [ ] **Step 4: Run — PASS**

Run: `cd sdks/python && uv run pytest tests/test_codex_cli_stream.py -v`
Expected: all stream tests PASS.

- [ ] **Step 5: Commit**

```bash
git add sdks/python/motosan_ai/providers/codex_cli.py sdks/python/tests/test_codex_cli_stream.py
git commit -m "feat(python,codex-cli): stream() yields events line-by-line"
```

---

## Task 11: Register `Provider.codex_cli` + `Client.codex_cli()`

**Files:**
- Modify: `sdks/python/motosan_ai/client.py`
- Modify: `sdks/python/motosan_ai/providers/__init__.py`
- Modify: `sdks/python/motosan_ai/__init__.py`
- Create: `sdks/python/tests/test_codex_cli_dispatch.py`

`Provider.codex_cli` enum entry + `Client.codex_cli()` classmethod. Unlike HTTP providers, no API key needed — just `CODEX_PATH` env var (which is purely the binary path, no auth).

- [ ] **Step 1: Write failing tests**

Create `sdks/python/tests/test_codex_cli_dispatch.py`:

```python
from __future__ import annotations

from motosan_ai import Client, Provider
from motosan_ai.providers.codex_cli import CodexCliClient


def test_provider_enum_has_codex_cli():
    assert Provider.codex_cli == "codex_cli"


def test_client_codex_cli_classmethod_resolves_to_provider():
    client = Client.codex_cli()
    assert client.provider == Provider.codex_cli
    assert isinstance(client._provider, CodexCliClient)


def test_client_codex_cli_with_explicit_binary_path():
    client = Client.codex_cli(binary_path="/opt/codex")
    assert client._provider._config.binary_path == "/opt/codex"


def test_codex_path_env_var_resolved(monkeypatch):
    monkeypatch.setenv("CODEX_PATH", "/env/codex")
    client = Client.codex_cli()
    assert client._provider._config.binary_path == "/env/codex"


def test_codex_cli_does_not_require_api_key(monkeypatch):
    """No API key — codex_cli is purely subprocess-based."""
    for env in ("OPENAI_API_KEY", "CODEX_PATH"):
        monkeypatch.delenv(env, raising=False)
    # Should not raise ConfigError
    client = Client(provider=Provider.codex_cli)
    assert isinstance(client._provider, CodexCliClient)
```

- [ ] **Step 2: Run — FAIL**

Run: `cd sdks/python && uv run pytest tests/test_codex_cli_dispatch.py -v`
Expected: AttributeError on `Provider.codex_cli`.

- [ ] **Step 3: Wire into `Client`**

Edit `sdks/python/motosan_ai/client.py`:

```python
class Provider(StrEnum):
    anthropic = "anthropic"
    openai = "openai"
    minimax = "minimax"
    ollama = "ollama"
    gemini = "gemini"
    codex_cli = "codex_cli"
```

Update the `Client.__init__` dispatch — add a branch for `codex_cli` that bypasses the API-key requirement:

```python
        if provider_value == Provider.codex_cli:
            from motosan_ai.providers.codex_cli import CodexCliClient

            # codex_cli is purely subprocess-based; no api_key required.
            self.api_key = ""
            self._provider = CodexCliClient(
                binary_path=base_url  # repurposed: pass as binary_path via base_url is awkward.
            )
            return
```

Wait — the `Client` constructor's existing parameters (`api_key`, `model`, `base_url`) don't fit `CodexCliClient(binary_path=...)`. Cleanest fix: add an optional `binary_path` parameter to `Client.__init__`. Inspect the existing signature and add it minimally:

Open `sdks/python/motosan_ai/client.py`, find the `__init__` signature, add `binary_path: str | None = None` after `base_url`:

```python
    def __init__(
        self,
        provider: Provider | str,
        api_key: str | None = None,
        model: str | None = None,
        base_url: str | None = None,
        binary_path: str | None = None,  # NEW: for CLI providers
        ...
```

Then in the dispatch branch:

```python
        if provider_value == Provider.codex_cli:
            from motosan_ai.providers.codex_cli import CodexCliClient

            self.provider = provider_value
            self.model = model
            self._max_retries = max_retries
            self.api_key = ""
            self._provider = CodexCliClient(binary_path=binary_path)
            return
```

Add classmethod:

```python
    @classmethod
    def codex_cli(
        cls,
        binary_path: str | None = None,
        model: str | None = None,
        max_retries: int = 3,
    ) -> Client:
        return cls(
            provider=Provider.codex_cli,
            binary_path=binary_path,
            model=model,
            max_retries=max_retries,
        )
```

Edit `sdks/python/motosan_ai/providers/__init__.py` — add `CodexCliClient`, `SandboxMode`, `LocalProvider` to imports + `__all__`.

Edit `sdks/python/motosan_ai/__init__.py` — same for top-level.

- [ ] **Step 4: Run — PASS**

Run: `cd sdks/python && uv run pytest tests/test_codex_cli_dispatch.py tests/test_client_integration.py -v`
Expected: all PASS — codex_cli dispatch tests green, no regression on client integration matrix.

- [ ] **Step 5: Commit**

```bash
git add sdks/python/motosan_ai/ sdks/python/tests/test_codex_cli_dispatch.py
git commit -m "feat(python,codex-cli): register Provider.codex_cli + Client.codex_cli()"
```

---

## Task 12: Live integration tests

**Files:**
- Create: `sdks/python/tests/integration/test_codex_cli_live.py`

Skip if `codex` not on PATH. Tests basic chat + stream + a single flag (sandbox).

- [ ] **Step 1: Create live test file**

```python
"""Live integration tests for CodexCliClient.

Set `MOTOSAN_RUN_CODEX_LIVE=1` to run. Skip when `codex` binary is not on PATH (or `CODEX_PATH` not pointing
at one), or when the preflight auth/model turn fails. `MOTOSAN_CODEX_MODEL`
overrides the live-test model (default `gpt-5.1-codex`). These test that
the subprocess wiring talks to the real codex CLI end-to-end.
"""

from __future__ import annotations

import os
import shutil

import pytest

from motosan_ai.providers.codex_cli import CodexCliClient, SandboxMode
from motosan_ai.types import ChatRequest, Message

_BINARY = os.environ.get("CODEX_PATH") or shutil.which("codex")

pytestmark = [
    pytest.mark.skipif(_BINARY is None, reason="codex binary not on PATH"),
    pytest.mark.asyncio,
]


async def test_live_chat_basic():
    client = CodexCliClient().sandbox(SandboxMode.read_only)
    resp = await client.chat(
        ChatRequest(messages=[Message.user("Reply with exactly: PONG")])
    )
    assert "PONG" in resp.content


async def test_live_stream_emits_text_then_done():
    client = CodexCliClient().sandbox(SandboxMode.read_only)
    events = []
    async for event in client.stream(
        ChatRequest(messages=[Message.user("Reply with: STREAM_OK")])
    ):
        events.append(event)

    text_content = "".join(
        e.content for e in events if e.event_type == "text" and not e.done
    )
    assert "STREAM_OK" in text_content
    assert events[-1].done is True


async def test_live_stream_emits_usage_event():
    client = CodexCliClient().sandbox(SandboxMode.read_only)
    events = []
    async for event in client.stream(
        ChatRequest(messages=[Message.user("Count to 3.")])
    ):
        events.append(event)

    usage_events = [e for e in events if e.event_type == "usage"]
    assert len(usage_events) >= 1
    assert usage_events[0].usage.input_tokens > 0
    assert usage_events[0].usage.output_tokens > 0
```

- [ ] **Step 2: Run live (manual — requires `codex`)**

Run: `cd sdks/python && uv run pytest tests/integration/test_codex_cli_live.py -v`
Expected: PASS if `codex` is on PATH; SKIP otherwise.

- [ ] **Step 3: Commit**

```bash
git add sdks/python/tests/integration/test_codex_cli_live.py
git commit -m "test(python,codex-cli): live integration tests for chat / stream / usage"
```

---

## Task 13: Release — CHANGELOG + version bump to 0.9.1

**Files:**
- Modify: `sdks/python/CHANGELOG.md`
- Modify: `sdks/python/pyproject.toml`

- [ ] **Step 1: Bump version**

Edit `sdks/python/pyproject.toml`:

```toml
version = "0.9.1"
```

- [ ] **Step 2: Prepend CHANGELOG entry**

Replace the date with the actual release day.

```markdown
## [0.9.1] - YYYY-MM-DD

### Added — `CodexCliClient` (Phase 3b)
- New subprocess provider mirroring Rust's `CodexCliProvider`. Spawns `codex exec --json --skip-git-repo-check` and parses JSONL events (`item.completed`, `turn.completed`, `turn.failed`, `error`).
- 13 fluent builder methods cover the full Rust flag surface:
  - Booleans: `agent_mode` (`--full-auto`), `dangerously_bypass_approvals_and_sandbox`, `oss`, `ephemeral`
  - Single-value: `sandbox(SandboxMode)`, `local_provider(LocalProvider)`, `model`, `profile`, `cd`
  - Repeating: `add_dir`, `enable_feature`, `disable_feature`, `config_override(key, value)` → `-c key=value`
- `SandboxMode` (`read_only` / `workspace_write` / `danger_full_access`) and `LocalProvider` (`lmstudio` / `ollama`) `StrEnum`s — values are the wire flags.
- `Provider.codex_cli` registered in `Client` dispatch; new `Client.codex_cli()` classmethod and `binary_path=` parameter on `Client.__init__`.
- `CODEX_PATH` env var resolves the binary location (matches Rust default).
- Stream emits `StreamEvent(usage)` before terminal `done` when `turn.completed` carries usage; `cached_input_tokens` maps to `Usage.cache_read_input_tokens`.
- Live integration tests added under `tests/integration/test_codex_cli_live.py` (skip when `codex` not on PATH).

### Notes
- No API key required — `Provider.codex_cli` is purely subprocess-based; the `codex` binary handles its own auth.
- Argv composition order matches Rust `spawn.rs::common_args` byte-for-byte; pinned by `test_full_config_argv_order_matches_rust_common_args`.
- Phase 3c (Gemini CLI) and 3d (Gemini Code Assist OAuth) ship in subsequent 0.9.x releases.
```

- [ ] **Step 3: Run the full gate**

Run: `cd sdks/python && uv run ruff check motosan_ai/ && uv run ruff format --check motosan_ai/ tests/ && uv run pytest tests/ -q --ignore=tests/integration/`
Expected: all pass.

- [ ] **Step 4: Commit**

```bash
git add sdks/python/pyproject.toml sdks/python/CHANGELOG.md
git commit -m "chore(python): release v0.9.1 — CodexCliClient (Phase 3b)"
```

---

## Final Self-Review Checklist

Before declaring Phase 3b done, verify:

- [ ] `cd sdks/python && uv run pytest tests/ -v` — all tests pass (target: ~390+ passing).
- [ ] `check-python` gate (ruff + format + pytest) passes.
- [ ] Every `pub fn` in [Rust codex_cli/mod.rs](sdks/rust/src/providers/codex_cli/mod.rs) has a Python equivalent — cross-check via `grep "pub fn " sdks/rust/src/providers/codex_cli/mod.rs | sed 's/.*pub fn //; s/(.*//'` against `grep "def " sdks/python/motosan_ai/providers/codex_cli.py`. Excluding `new` (replaced by `__init__`), 12 builder methods + `with_path` should match.
- [ ] `_build_args` output for the maximally-configured client matches Rust `common_args` byte-for-byte. Pinned by `test_full_config_argv_order_matches_rust_common_args`.
- [ ] JSONL parser raises `ProviderError` on `turn.failed` / `error`; everything else returns 0/1/2 events.
- [ ] Stream `usage` event emitted exactly once per stream when `turn.completed` carries usage; precedes terminal `done`.
- [ ] `Provider.codex_cli` resolves without an API key; `Client(provider=Provider.codex_cli)` succeeds with no env.
- [ ] Live tests pass against real `codex` binary (when available).
- [ ] Version in `pyproject.toml` is `0.9.1` and `CHANGELOG.md` has a matching entry.
- [ ] No `TODO` / `FIXME` / placeholder strings introduced.

If any box is unchecked, fix before tagging/publishing.

---

## What Phase 3b does NOT do

- ❌ Gemini CLI provider — Phase 3c.
- ❌ Gemini Code Assist OAuth + HTTP — Phase 3d (most complex; needs new `motosan_ai/oauth/google.py`).
- ❌ Codex CLI MCP server hosting — out of scope; the SDK targets `codex exec` only, not `codex mcp` subcommands.
- ❌ `OPENAI_API_KEY` validation — `codex` handles its own auth via the user's `~/.codex/auth.toml` or `OPENAI_API_KEY` (Codex reads it directly). The Python provider doesn't read or validate it.
- ❌ Snapshot-based argv testing — per-flag `in args` plus the full-config order pin (Task 7) is sufficient.
- ❌ Tool-call surfacing on stream — Codex emits tool-call items as `item.type == "command_execution"` etc. Per Rust stream_json.rs, only `agent_message` items become text. Tool-call items are silently dropped (matches Rust). If callers need them, that's a follow-up.

All non-goals tracked in the roadmap doc.
