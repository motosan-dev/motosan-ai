# Python SDK Phase 3c — Gemini CLI Provider Implementation Plan

> **Status:** ✅ **COMPLETE (2026-04-25)** — shipped as `motosan-ai` v0.9.2.
>
> **Errata:** none. Implementation matched the Rust canon and this plan's argv/parser guidance without post-implementation corrections.
>
> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Ship `GeminiCliClient` — a Python subprocess provider that shells out to Google's `gemini` CLI in headless `-o stream-json` mode, mirroring Rust's `GeminiCliProvider`. Full builder surface, NDJSON stream parsing, registered as `Provider.gemini_cli` in `Client` dispatch.

**Architecture:** Mirror Phase 3b's `CodexCliClient` pattern: `_GeminiCliConfig` dataclass holds builder state; fluent builder methods mutate it; `_build_args` emits the argv vector; `chat()` / `stream()` spawn `gemini -p "" -o stream-json [...args]`, write the prompt (with system text prepended) to stdin, and parse the NDJSON output (`init`, `message`, `result`).

**Tech Stack:** Python 3.11+, `asyncio.subprocess`, stdlib `json`. No new dependencies.

**Ships as:** `motosan-ai` v0.9.2.

---

## Reference material

- **Rust canon (verified before writing):**
  - [sdks/rust/src/providers/gemini_cli/mod.rs](sdks/rust/src/providers/gemini_cli/mod.rs) — `GeminiCliProvider` builder. 13 builder methods + `new` + `with_path`. Env var `GEMINI_CLI_PATH` at line 67. `merge_system_into_prompt` at line 279.
  - [sdks/rust/src/providers/gemini_cli/spawn.rs](sdks/rust/src/providers/gemini_cli/spawn.rs) — `common_args` (lines 122-176). `ApprovalMode` enum at line 33. **No trailing `-` argument** — prompt fed purely via stdin.
  - [sdks/rust/src/providers/gemini_cli/stream_json.rs](sdks/rust/src/providers/gemini_cli/stream_json.rs) — NDJSON parser. Event shapes: `init`, `message` (assistant deltas), `result` (terminal + stats).

- **Phase 3b reference:** [sdks/python/motosan_ai/providers/codex_cli.py](sdks/python/motosan_ai/providers/codex_cli.py) — same shape: dataclass + fluent builder + `_build_args` + `_parse_jsonl_line` + subprocess `chat`/`stream`. Reuse the skeleton.

- **Verified flag map (from spawn.rs):**

| Builder method | CLI args emitted | Notes |
|---|---|---|
| `model(str)` | `-m <value>` | Empty / whitespace / `"default"` (case-insensitive) skipped (matches Rust `model_to_forward`) |
| `yolo(bool)` | `--yolo` | |
| `sandbox(bool)` | `--sandbox` | |
| `approval_mode(ApprovalMode)` | `--approval-mode <flag>` | flag ∈ `default` / `auto_edit` / `yolo` / `plan` |
| `include_dir(path)` | repeating `--include-directories <path>` | Each call appends |
| `include_dirs(paths)` | repeating `--include-directories <path>` | Replaces full list |
| `extension(name)` | repeating `-e <name>` | Whitespace-only skipped |
| `extensions(names)` | repeating `-e <name>` | Replaces full list |
| `allowed_mcp_server(name)` | repeating `--allowed-mcp-server-names <name>` | Whitespace-only skipped |
| `allowed_mcp_servers(names)` | repeating `--allowed-mcp-server-names <name>` | Replaces full list |
| `resume(value)` | `--resume <trimmed_value>` | Whitespace-only skipped |

Argv prefix (always, before flags): `["-p", "", "-o", "stream-json"]`. **No trailing `-`** — Gemini CLI reads prompt from stdin without an explicit marker.

JSONL event handling (verified from stream_json.rs):
- `init` → ignored
- `message` with `role == "user"` → ignored (stdin echo)
- `message` with `role == "assistant"`, `delta == True`, non-empty `content` → emit text `StreamEvent`
- `result` with `status == "success"` (or missing) + `stats` present → emit `StreamEvent(usage)` (mapping `stats.cached` → `Usage.cache_read_input_tokens`) followed by `StreamEvent(done=True)`
- `result` with non-success `status` → raise `ProviderError`
- Everything else (unknown `type`, malformed JSON) → ignored

Stdin payload format (per `merge_system_into_prompt` mod.rs:279-284):
- If system prompt is non-empty: `f"{system}\n\n{user_prompt}"`
- Otherwise: `user_prompt` unchanged

---

## File Structure

| Path | Responsibility | Status |
|------|----------------|--------|
| `sdks/python/motosan_ai/providers/gemini_cli.py` | `GeminiCliClient`, `_GeminiCliConfig`, `ApprovalMode`, `_build_args`, `_parse_jsonl_line`, `_merge_system_into_prompt`, `chat`, `stream` | **Create** |
| `sdks/python/motosan_ai/providers/__init__.py` | Export `GeminiCliClient`, `ApprovalMode` | **Modify** |
| `sdks/python/motosan_ai/__init__.py` | Top-level export | **Modify** |
| `sdks/python/motosan_ai/client.py` | Register `Provider.gemini_cli`, classmethod `Client.gemini_cli()`, reuse `binary_path=` parameter | **Modify** |
| `sdks/python/tests/test_gemini_cli_flags.py` | Per-flag `_build_args` assertions + full-config order pin | **Create** |
| `sdks/python/tests/test_gemini_cli_stream.py` | JSONL parser + chat/stream behavior | **Create** |
| `sdks/python/tests/test_gemini_cli_dispatch.py` | `Provider.gemini_cli` dispatch + env-var resolution | **Create** |
| `sdks/python/tests/integration/test_gemini_cli_live.py` | Live tests behind `gemini` binary on PATH + `MOTOSAN_RUN_GEMINI_CLI_LIVE=1` | **Create** |
| `sdks/python/CHANGELOG.md` | v0.9.2 entry | **Modify** |
| `sdks/python/pyproject.toml` | Version bump 0.9.1 → 0.9.2 | **Modify** |

Design principles:
- **Mirror Phase 3b structure exactly.** Same shape worked twice (3a, 3b); keep the rhythm.
- **`ApprovalMode` via `StrEnum`** with values that are wire flags (`auto_edit`, etc.) — `args.append(mode.value)` is trivially correct.
- **Wire-flag strings grepped from `spawn.rs`, not memory** — Phase 3a/3b both demonstrated this matters. All flag strings in this plan have line refs.
- **Two-tier live test gate** — binary on PATH **plus** `MOTOSAN_RUN_GEMINI_CLI_LIVE=1`. Same as Phase 3b. Prevents incidental upstream auth/quota failures from breaking the unit gate.
- **`Client.gemini_cli(binary_path=...)` reuses Phase 3b's `Client.__init__(binary_path=...)` parameter** — no Client-side changes beyond the new dispatch branch and classmethod.

---

## Task 1: Module skeleton + `_GeminiCliConfig` dataclass + base argv

**Files:**
- Create: `sdks/python/motosan_ai/providers/gemini_cli.py`
- Create: `sdks/python/tests/test_gemini_cli_flags.py`

Minimal class + dataclass + binary path resolution + `gemini -p "" -o stream-json` base argv (no trailing `-`).

- [ ] **Step 1: Write failing test**

Create `sdks/python/tests/test_gemini_cli_flags.py`:

```python
from __future__ import annotations

import os

from motosan_ai.providers.gemini_cli import GeminiCliClient


def _args(client: GeminiCliClient) -> list[str]:
    return client._build_args()


def test_default_binary_path_is_gemini_or_env():
    c = GeminiCliClient()
    assert c._config.binary_path in ("gemini", os.environ.get("GEMINI_CLI_PATH", "gemini"))


def test_explicit_binary_path():
    c = GeminiCliClient(binary_path="/opt/gemini")
    assert c._config.binary_path == "/opt/gemini"


def test_with_path_classmethod():
    c = GeminiCliClient.with_path("/opt/gemini-2")
    assert c._config.binary_path == "/opt/gemini-2"


def test_minimal_args_emit_headless_prefix_no_trailing_dash():
    """Base argv: gemini -p "" -o stream-json (NO trailing `-`).

    Gemini CLI takes the prompt via stdin without an explicit `-` marker —
    distinct from Codex CLI / Claude Code which both use `-`.
    """
    c = GeminiCliClient(binary_path="gemini")
    assert _args(c) == ["gemini", "-p", "", "-o", "stream-json"]
```

- [ ] **Step 2: Run — should FAIL (module not found)**

Run: `cd sdks/python && uv run pytest tests/test_gemini_cli_flags.py -v`
Expected: `ModuleNotFoundError: No module named 'motosan_ai.providers.gemini_cli'`.

- [ ] **Step 3: Create `gemini_cli.py` skeleton**

Create `sdks/python/motosan_ai/providers/gemini_cli.py`:

```python
from __future__ import annotations

import os
from dataclasses import dataclass, field
from enum import StrEnum

from motosan_ai.provider_base import ProviderCapabilities


@dataclass
class _GeminiCliConfig:
    binary_path: str = "gemini"


class GeminiCliClient:
    """Client that shells out to Google's ``gemini`` CLI in headless mode.

    Mirrors Rust's ``GeminiCliProvider``. Builder methods land in subsequent
    tasks; this module starts with the binary-path skeleton + base argv.
    """

    capabilities: ProviderCapabilities = ProviderCapabilities.text_only()

    def __init__(self, binary_path: str | None = None) -> None:
        if binary_path is None:
            binary_path = os.environ.get("GEMINI_CLI_PATH", "gemini")
        self._config = _GeminiCliConfig(binary_path=binary_path)

    @classmethod
    def with_path(cls, path: str) -> GeminiCliClient:
        return cls(binary_path=path)

    def _build_args(self) -> list[str]:
        return [
            self._config.binary_path,
            "-p",
            "",
            "-o",
            "stream-json",
        ]
```

- [ ] **Step 4: Run — PASS**

Run: `cd sdks/python && uv run pytest tests/test_gemini_cli_flags.py -v`
Expected: 4 PASS.

- [ ] **Step 5: Commit**

```bash
git add sdks/python/motosan_ai/providers/gemini_cli.py sdks/python/tests/test_gemini_cli_flags.py
git commit -m "feat(python,gemini-cli): module skeleton + headless base argv (no trailing dash)"
```

---

## Task 2: `ApprovalMode` enum

**Files:**
- Modify: `sdks/python/motosan_ai/providers/gemini_cli.py`
- Modify: `sdks/python/tests/test_gemini_cli_flags.py`

Per Rust spawn.rs:46-56: four variants whose `.value` is the exact `--approval-mode` flag.

- [ ] **Step 1: Append failing tests**

```python
from motosan_ai.providers.gemini_cli import ApprovalMode


def test_approval_mode_values_match_cli_flags():
    assert ApprovalMode.default.value == "default"
    assert ApprovalMode.auto_edit.value == "auto_edit"
    assert ApprovalMode.yolo.value == "yolo"
    assert ApprovalMode.plan.value == "plan"
```

- [ ] **Step 2: Run — FAIL (ImportError)**

Run: `cd sdks/python && uv run pytest tests/test_gemini_cli_flags.py -v -k approval`
Expected: ImportError.

- [ ] **Step 3: Add enum**

In `gemini_cli.py`, after the `from enum import StrEnum` import:

```python
class ApprovalMode(StrEnum):
    default = "default"
    auto_edit = "auto_edit"
    yolo = "yolo"
    plan = "plan"
```

- [ ] **Step 4: Run — PASS**

Run: `cd sdks/python && uv run pytest tests/test_gemini_cli_flags.py -v`
Expected: 5 PASS.

- [ ] **Step 5: Commit**

```bash
git add sdks/python/motosan_ai/providers/gemini_cli.py sdks/python/tests/test_gemini_cli_flags.py
git commit -m "feat(python,gemini-cli): add ApprovalMode enum (default / auto_edit / yolo / plan)"
```

---

## Task 3: Boolean flags — `yolo`, `sandbox`

**Files:**
- Modify: `sdks/python/motosan_ai/providers/gemini_cli.py`
- Modify: `sdks/python/tests/test_gemini_cli_flags.py`

Two pure-boolean flags (spawn.rs:135-141).

- [ ] **Step 1: Append failing tests**

```python
def test_yolo_flag():
    c = GeminiCliClient(binary_path="gemini").yolo(True)
    assert "--yolo" in _args(c)


def test_yolo_absent_by_default():
    assert "--yolo" not in _args(GeminiCliClient(binary_path="gemini"))


def test_sandbox_flag():
    c = GeminiCliClient(binary_path="gemini").sandbox(True)
    assert "--sandbox" in _args(c)


def test_sandbox_absent_by_default():
    assert "--sandbox" not in _args(GeminiCliClient(binary_path="gemini"))
```

- [ ] **Step 2: Run — FAIL**

Run: `cd sdks/python && uv run pytest tests/test_gemini_cli_flags.py -v -k "yolo or sandbox"`
Expected: AttributeError on the missing methods.

- [ ] **Step 3: Extend `_GeminiCliConfig` + add builder methods + extend `_build_args`**

Extend dataclass:

```python
@dataclass
class _GeminiCliConfig:
    binary_path: str = "gemini"
    yolo: bool = False
    sandbox: bool = False
```

Add to `GeminiCliClient`:

```python
    def yolo(self, enabled: bool) -> GeminiCliClient:
        self._config.yolo = enabled
        return self

    def sandbox(self, enabled: bool) -> GeminiCliClient:
        self._config.sandbox = enabled
        return self
```

Replace `_build_args`:

```python
    def _build_args(self) -> list[str]:
        args: list[str] = [
            self._config.binary_path,
            "-p",
            "",
            "-o",
            "stream-json",
        ]
        if self._config.yolo:
            args.append("--yolo")
        if self._config.sandbox:
            args.append("--sandbox")
        return args
```

- [ ] **Step 4: Run — PASS**

Run: `cd sdks/python && uv run pytest tests/test_gemini_cli_flags.py -v`
Expected: 9 PASS.

- [ ] **Step 5: Commit**

```bash
git add sdks/python/motosan_ai/providers/gemini_cli.py sdks/python/tests/test_gemini_cli_flags.py
git commit -m "feat(python,gemini-cli): add yolo and sandbox boolean flags"
```

---

## Task 4: Single-value flags — `model`, `approval_mode`, `resume`

**Files:**
- Modify: `sdks/python/motosan_ai/providers/gemini_cli.py`
- Modify: `sdks/python/tests/test_gemini_cli_flags.py`

Per spawn.rs:128-146 + 167-173. `model` and `resume` apply skip-blank rules; `model` also skips case-insensitive `"default"`.

- [ ] **Step 1: Append failing tests**

```python
def test_model_flag_emitted():
    c = GeminiCliClient(binary_path="gemini").model("gemini-2.5-pro")
    args = _args(c)
    i = args.index("-m")
    assert args[i + 1] == "gemini-2.5-pro"


def test_model_default_sentinel_skipped():
    for sentinel in ("", "   ", "default", "Default", "DEFAULT"):
        c = GeminiCliClient(binary_path="gemini").model(sentinel)
        assert "-m" not in _args(c), f"unexpected -m for {sentinel!r}"


def test_approval_mode_flag():
    c = GeminiCliClient(binary_path="gemini").approval_mode(ApprovalMode.auto_edit)
    args = _args(c)
    i = args.index("--approval-mode")
    assert args[i + 1] == "auto_edit"


def test_resume_flag_emitted():
    c = GeminiCliClient(binary_path="gemini").resume("latest")
    args = _args(c)
    assert args[args.index("--resume") + 1] == "latest"


def test_resume_trims_value():
    c = GeminiCliClient(binary_path="gemini").resume("  3  ")
    args = _args(c)
    assert args[args.index("--resume") + 1] == "3"


def test_resume_blank_skipped():
    for blank in ("", "   ", "\t\n"):
        c = GeminiCliClient(binary_path="gemini").resume(blank)
        assert "--resume" not in _args(c)
```

- [ ] **Step 2: Run — FAIL**

Run: `cd sdks/python && uv run pytest tests/test_gemini_cli_flags.py -v -k "model or approval_mode_flag or resume"`
Expected: AttributeError.

- [ ] **Step 3: Extend dataclass + add helper + add methods + extend `_build_args`**

Add module-level helper:

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
class _GeminiCliConfig:
    binary_path: str = "gemini"
    yolo: bool = False
    sandbox: bool = False
    model: str | None = None
    approval_mode: ApprovalMode | None = None
    resume: str | None = None
```

Add builder methods:

```python
    def model(self, model: str) -> GeminiCliClient:
        self._config.model = model
        return self

    def approval_mode(self, mode: ApprovalMode) -> GeminiCliClient:
        self._config.approval_mode = mode
        return self

    def resume(self, session: str) -> GeminiCliClient:
        self._config.resume = session
        return self
```

Replace `_build_args` body — order matches Rust `common_args` (model before yolo/sandbox; approval_mode after; resume last):

```python
    def _build_args(self) -> list[str]:
        args: list[str] = [
            self._config.binary_path,
            "-p",
            "",
            "-o",
            "stream-json",
        ]
        if self._config.model is not None:
            forwarded = _model_to_forward(self._config.model)
            if forwarded:
                args.extend(["-m", forwarded])
        if self._config.yolo:
            args.append("--yolo")
        if self._config.sandbox:
            args.append("--sandbox")
        if self._config.approval_mode is not None:
            args.extend(["--approval-mode", self._config.approval_mode.value])
        if self._config.resume is not None:
            trimmed = self._config.resume.strip()
            if trimmed:
                args.extend(["--resume", trimmed])
        return args
```

- [ ] **Step 4: Run — PASS**

Run: `cd sdks/python && uv run pytest tests/test_gemini_cli_flags.py -v`
Expected: 19 PASS (5 + 4 + 10 new).

- [ ] **Step 5: Commit**

```bash
git add sdks/python/motosan_ai/providers/gemini_cli.py sdks/python/tests/test_gemini_cli_flags.py
git commit -m "feat(python,gemini-cli): add model, approval_mode, resume single-value flags"
```

---

## Task 5: List flags singular — `include_dir`, `extension`, `allowed_mcp_server`

**Files:**
- Modify: `sdks/python/motosan_ai/providers/gemini_cli.py`
- Modify: `sdks/python/tests/test_gemini_cli_flags.py`

Each call appends one entry. Per spawn.rs:148-165:
- `include_dir` → repeating `--include-directories <dir>`
- `extension` → repeating `-e <name>` (skip whitespace-only)
- `allowed_mcp_server` → repeating `--allowed-mcp-server-names <name>` (skip whitespace-only)

- [ ] **Step 1: Append failing tests**

```python
def test_include_dir_appends():
    c = (
        GeminiCliClient(binary_path="gemini")
        .include_dir("/proj/a")
        .include_dir("/proj/b")
    )
    args = _args(c)
    occ = [args[i + 1] for i, a in enumerate(args) if a == "--include-directories"]
    assert occ == ["/proj/a", "/proj/b"]


def test_extension_appends():
    c = (
        GeminiCliClient(binary_path="gemini")
        .extension("foo")
        .extension("bar")
    )
    args = _args(c)
    occ = [args[i + 1] for i, a in enumerate(args) if a == "-e"]
    assert occ == ["foo", "bar"]


def test_extension_whitespace_skipped():
    c = GeminiCliClient(binary_path="gemini").extension("   ")
    assert "-e" not in _args(c)


def test_allowed_mcp_server_appends():
    c = (
        GeminiCliClient(binary_path="gemini")
        .allowed_mcp_server("srv-a")
        .allowed_mcp_server("srv-b")
    )
    args = _args(c)
    occ = [args[i + 1] for i, a in enumerate(args) if a == "--allowed-mcp-server-names"]
    assert occ == ["srv-a", "srv-b"]


def test_allowed_mcp_server_whitespace_skipped():
    c = GeminiCliClient(binary_path="gemini").allowed_mcp_server("\t  ")
    assert "--allowed-mcp-server-names" not in _args(c)
```

- [ ] **Step 2: Run — FAIL**

Run: `cd sdks/python && uv run pytest tests/test_gemini_cli_flags.py -v -k "include_dir_appends or extension_appends or extension_whitespace or allowed_mcp_server_appends or allowed_mcp_server_whitespace"`
Expected: AttributeError.

- [ ] **Step 3: Extend dataclass + methods + `_build_args`**

Dataclass:

```python
    include_dirs: list[str] = field(default_factory=list)
    extensions: list[str] = field(default_factory=list)
    allowed_mcp_servers: list[str] = field(default_factory=list)
```

Methods:

```python
    def include_dir(self, dir: str) -> GeminiCliClient:
        self._config.include_dirs.append(dir)
        return self

    def extension(self, name: str) -> GeminiCliClient:
        self._config.extensions.append(name)
        return self

    def allowed_mcp_server(self, name: str) -> GeminiCliClient:
        self._config.allowed_mcp_servers.append(name)
        return self
```

In `_build_args`, after the `--approval-mode` block and before `--resume`:

```python
        for d in self._config.include_dirs:
            args.extend(["--include-directories", d])
        for ext in self._config.extensions:
            if ext.strip():
                args.extend(["-e", ext])
        for name in self._config.allowed_mcp_servers:
            if name.strip():
                args.extend(["--allowed-mcp-server-names", name])
```

- [ ] **Step 4: Run — PASS**

Run: `cd sdks/python && uv run pytest tests/test_gemini_cli_flags.py -v`
Expected: 24 PASS.

- [ ] **Step 5: Commit**

```bash
git add sdks/python/motosan_ai/providers/gemini_cli.py sdks/python/tests/test_gemini_cli_flags.py
git commit -m "feat(python,gemini-cli): add include_dir, extension, allowed_mcp_server (singular)"
```

---

## Task 6: List flags plural (replace) — `include_dirs`, `extensions`, `allowed_mcp_servers`

**Files:**
- Modify: `sdks/python/motosan_ai/providers/gemini_cli.py`
- Modify: `sdks/python/tests/test_gemini_cli_flags.py`

Plural setters replace the full list. Matches Phase 3a's `add_dirs` / `allowed_tools` pattern.

- [ ] **Step 1: Append failing tests**

```python
def test_include_dirs_replaces():
    c = (
        GeminiCliClient(binary_path="gemini")
        .include_dir("/old")
        .include_dirs(["/new1", "/new2"])
    )
    args = _args(c)
    occ = [args[i + 1] for i, a in enumerate(args) if a == "--include-directories"]
    assert occ == ["/new1", "/new2"]


def test_extensions_replaces():
    c = (
        GeminiCliClient(binary_path="gemini")
        .extension("old")
        .extensions(["a", "b"])
    )
    args = _args(c)
    occ = [args[i + 1] for i, a in enumerate(args) if a == "-e"]
    assert occ == ["a", "b"]


def test_allowed_mcp_servers_replaces():
    c = (
        GeminiCliClient(binary_path="gemini")
        .allowed_mcp_server("old")
        .allowed_mcp_servers(["x", "y"])
    )
    args = _args(c)
    occ = [args[i + 1] for i, a in enumerate(args) if a == "--allowed-mcp-server-names"]
    assert occ == ["x", "y"]
```

- [ ] **Step 2: Run — FAIL**

Run: `cd sdks/python && uv run pytest tests/test_gemini_cli_flags.py -v -k "_replaces"`
Expected: AttributeError.

- [ ] **Step 3: Add plural methods**

Append to `GeminiCliClient`:

```python
    def include_dirs(self, dirs: list[str]) -> GeminiCliClient:
        self._config.include_dirs = list(dirs)
        return self

    def extensions(self, names: list[str]) -> GeminiCliClient:
        self._config.extensions = list(names)
        return self

    def allowed_mcp_servers(self, names: list[str]) -> GeminiCliClient:
        self._config.allowed_mcp_servers = list(names)
        return self
```

- [ ] **Step 4: Run — PASS**

Run: `cd sdks/python && uv run pytest tests/test_gemini_cli_flags.py -v`
Expected: 27 PASS.

- [ ] **Step 5: Commit**

```bash
git add sdks/python/motosan_ai/providers/gemini_cli.py sdks/python/tests/test_gemini_cli_flags.py
git commit -m "feat(python,gemini-cli): add include_dirs, extensions, allowed_mcp_servers (plural replace)"
```

---

## Task 7: Argv composition smoke test — full config

**Files:**
- Modify: `sdks/python/tests/test_gemini_cli_flags.py`

End-to-end argv ordering pin against Rust `common_args` (spawn.rs:122-176). This is the tripwire that prevented Phase 3a errata from recurring.

- [ ] **Step 1: Append the smoke test**

```python
def test_full_config_argv_order_matches_rust_common_args():
    """End-to-end argv shape pinned to Rust spawn.rs:122-176.

    Order: -m <model>, --yolo, --sandbox, --approval-mode <m>,
    --include-directories <d>..., -e <name>...,
    --allowed-mcp-server-names <name>..., --resume <session>
    """
    c = (
        GeminiCliClient(binary_path="gemini")
        .model("gemini-2.5-pro")
        .yolo(True)
        .sandbox(True)
        .approval_mode(ApprovalMode.auto_edit)
        .include_dir("/proj/a")
        .include_dir("/proj/b")
        .extension("ext1")
        .allowed_mcp_server("srv1")
        .resume("latest")
    )
    assert _args(c) == [
        "gemini",
        "-p",
        "",
        "-o",
        "stream-json",
        "-m",
        "gemini-2.5-pro",
        "--yolo",
        "--sandbox",
        "--approval-mode",
        "auto_edit",
        "--include-directories",
        "/proj/a",
        "--include-directories",
        "/proj/b",
        "-e",
        "ext1",
        "--allowed-mcp-server-names",
        "srv1",
        "--resume",
        "latest",
    ]
```

- [ ] **Step 2: Run — should PASS already**

Run: `cd sdks/python && uv run pytest tests/test_gemini_cli_flags.py::test_full_config_argv_order_matches_rust_common_args -v`
Expected: PASS — earlier tasks composed the argv in the right order. If FAIL, fix `_build_args` ordering until the assertion list passes byte-for-byte.

- [ ] **Step 3: Commit**

```bash
git add sdks/python/tests/test_gemini_cli_flags.py
git commit -m "test(python,gemini-cli): pin full-config argv order against Rust common_args"
```

---

## Task 8: JSONL parser — `_parse_jsonl_line`

**Files:**
- Modify: `sdks/python/motosan_ai/providers/gemini_cli.py`
- Create: `sdks/python/tests/test_gemini_cli_stream.py`

Parse one NDJSON line into 0, 1, or 2 events (text or usage+done) — or raise `ProviderError` on non-success `result`. Mirrors Rust `parse_ndjson_line` (stream_json.rs:93-130).

- [ ] **Step 1: Write failing tests**

Create `sdks/python/tests/test_gemini_cli_stream.py`:

```python
from __future__ import annotations

import pytest

from motosan_ai.error import ProviderError
from motosan_ai.providers.gemini_cli import _parse_jsonl_line


def test_init_event_dropped():
    line = '{"type": "init", "session_id": "s1"}'
    assert _parse_jsonl_line(line) == []


def test_user_message_dropped():
    """Stdin echo — the user prompt comes back as a `user` role message."""
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
    """Future Gemini versions might emit final non-delta messages; skip them
    so accumulated chunks aren't double-counted."""
    line = '{"type": "message", "role": "assistant", "content": "hello", "delta": false}'
    assert _parse_jsonl_line(line) == []


def test_assistant_empty_content_dropped():
    line = '{"type": "message", "role": "assistant", "content": "", "delta": true}'
    assert _parse_jsonl_line(line) == []


def test_result_success_without_stats_emits_done_only():
    line = '{"type": "result", "status": "success"}'
    events = _parse_jsonl_line(line)
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
    line = '{"type": "result"}'
    events = _parse_jsonl_line(line)
    assert len(events) == 1
    assert events[0].done is True


def test_result_non_success_status_raises():
    line = '{"type": "result", "status": "error"}'
    with pytest.raises(ProviderError, match="result status: error"):
        _parse_jsonl_line(line)


def test_unknown_event_dropped():
    line = '{"type": "future_event"}'
    assert _parse_jsonl_line(line) == []


def test_malformed_json_returns_empty():
    assert _parse_jsonl_line("not json {") == []
    assert _parse_jsonl_line("") == []
```

- [ ] **Step 2: Run — FAIL**

Run: `cd sdks/python && uv run pytest tests/test_gemini_cli_stream.py -v`
Expected: ImportError on `_parse_jsonl_line`.

- [ ] **Step 3: Implement parser**

Add to `gemini_cli.py` (top-level helper):

```python
import json

from motosan_ai.error import ProviderError
from motosan_ai.types import StreamEvent, Usage


def _parse_jsonl_line(line: str) -> list[StreamEvent]:
    """Parse one NDJSON line from ``gemini -o stream-json``.

    Returns 0, 1, or 2 events. Raises ``ProviderError`` for non-success
    `result` events. Mirrors Rust stream_json.rs::parse_ndjson_line.
    """
    if not line:
        return []
    try:
        event = json.loads(line)
    except json.JSONDecodeError:
        return []

    event_type = event.get("type")

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
            out.append(
                StreamEvent(
                    content="",
                    done=False,
                    event_type="usage",
                    usage=Usage(
                        input_tokens=int(stats.get("input_tokens") or 0),
                        output_tokens=int(stats.get("output_tokens") or 0),
                        cache_read_input_tokens=stats.get("cached"),
                    ),
                )
            )
        out.append(StreamEvent(content="", done=True))
        return out

    return []
```

- [ ] **Step 4: Run — PASS**

Run: `cd sdks/python && uv run pytest tests/test_gemini_cli_stream.py -v`
Expected: 11 PASS.

- [ ] **Step 5: Commit**

```bash
git add sdks/python/motosan_ai/providers/gemini_cli.py sdks/python/tests/test_gemini_cli_stream.py
git commit -m "feat(python,gemini-cli): NDJSON parser for init/message/result events"
```

---

## Task 9: Stdin payload helpers — `_messages_to_prompt` + `_merge_system_into_prompt`

**Files:**
- Modify: `sdks/python/motosan_ai/providers/gemini_cli.py`
- Modify: `sdks/python/tests/test_gemini_cli_stream.py`

Gemini CLI's headless mode reads the full prompt from stdin. The Rust provider merges system + user messages into one string with a `\n\n` separator (mod.rs:279-284).

- [ ] **Step 1: Append failing tests**

```python
from motosan_ai.providers.gemini_cli import (
    _merge_system_into_prompt,
    _messages_to_prompt,
)
from motosan_ai.types import Message, Role


def test_messages_to_prompt_single_user_returns_just_content():
    out = _messages_to_prompt([Message.user("hello")])
    assert out == (None, "hello")


def test_messages_to_prompt_extracts_system():
    out = _messages_to_prompt(
        [
            Message.system("be terse"),
            Message.user("hello"),
        ]
    )
    assert out == ("be terse", "hello")


def test_messages_to_prompt_multi_turn_labels():
    out = _messages_to_prompt(
        [
            Message.user("q1"),
            Message.assistant("a1"),
            Message.user("q2"),
        ]
    )
    system, prompt = out
    assert system is None
    assert "q1" in prompt and "a1" in prompt and "q2" in prompt


def test_merge_system_prepends_with_double_newline():
    assert _merge_system_into_prompt("be helpful", "hello") == "be helpful\n\nhello"


def test_merge_system_none_passes_through():
    assert _merge_system_into_prompt(None, "hello") == "hello"


def test_merge_system_empty_passes_through():
    assert _merge_system_into_prompt("", "hello") == "hello"
```

- [ ] **Step 2: Run — FAIL**

Run: `cd sdks/python && uv run pytest tests/test_gemini_cli_stream.py -v -k "messages_to_prompt or merge_system"`
Expected: ImportError.

- [ ] **Step 3: Add helpers**

In `gemini_cli.py` (top-level):

```python
from motosan_ai.types import ChatRequest, ChatResponse, Message, Role, StopReason


def _messages_to_prompt(messages: list[Message]) -> tuple[str | None, str]:
    """Flatten messages into ``(system_prompt, user_prompt)``.

    The first system message is extracted; remaining messages are
    role-labeled and joined with blank lines to preserve turn boundaries.
    """
    system: str | None = None
    for m in messages:
        if m.role == Role.system:
            system = m.content
            break

    non_system = [m for m in messages if m.role != Role.system]

    if len(non_system) <= 1:
        prompt = non_system[0].content if non_system else ""
    else:
        labels = {
            Role.user: "[user]",
            Role.assistant: "[assistant]",
            Role.tool: "[tool]",
        }
        parts: list[str] = []
        for m in non_system:
            label = labels.get(m.role, "[user]")
            parts.append(f"{label}\n{m.content}")
        prompt = "\n\n".join(parts)

    return system, prompt


def _merge_system_into_prompt(system: str | None, user: str) -> str:
    """Prepend system text with a blank-line separator (mod.rs:279-284)."""
    if system:
        return f"{system}\n\n{user}"
    return user
```

- [ ] **Step 4: Run — PASS**

Run: `cd sdks/python && uv run pytest tests/test_gemini_cli_stream.py -v`
Expected: 17 PASS.

- [ ] **Step 5: Commit**

```bash
git add sdks/python/motosan_ai/providers/gemini_cli.py sdks/python/tests/test_gemini_cli_stream.py
git commit -m "feat(python,gemini-cli): _messages_to_prompt + _merge_system_into_prompt helpers"
```

---

## Task 10: `chat()` — subprocess + stdin payload + collect

**Files:**
- Modify: `sdks/python/motosan_ai/providers/gemini_cli.py`
- Modify: `sdks/python/tests/test_gemini_cli_stream.py`

Spawn `gemini -p "" -o stream-json [...args]`, write the merged prompt to stdin, read stdout, accumulate text, return `ChatResponse`. Same pattern as `CodexCliClient.chat`.

- [ ] **Step 1: Append failing tests**

```python
import asyncio
from unittest.mock import AsyncMock


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
    monkeypatch.setattr(
        "asyncio.create_subprocess_exec",
        AsyncMock(return_value=fake),
    )


from motosan_ai.providers.gemini_cli import GeminiCliClient
from motosan_ai.types import ChatRequest


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
async def test_chat_raises_on_nonzero_returncode(monkeypatch):
    _stub_subprocess(
        monkeypatch, _FakeProc("", returncode=2, stderr="gemini: bad config\n")
    )
    client = GeminiCliClient(binary_path="gemini")
    with pytest.raises(ProviderError, match="bad config"):
        await client.chat(ChatRequest(messages=[Message.user("hi")]))


@pytest.mark.asyncio
async def test_chat_raises_on_result_failure(monkeypatch):
    jsonl = '{"type": "result", "status": "rate_limited"}\n'
    _stub_subprocess(monkeypatch, _FakeProc(jsonl))
    client = GeminiCliClient(binary_path="gemini")
    with pytest.raises(ProviderError, match="rate_limited"):
        await client.chat(ChatRequest(messages=[Message.user("hi")]))
```

- [ ] **Step 2: Run — FAIL**

Run: `cd sdks/python && uv run pytest tests/test_gemini_cli_stream.py -v -k "chat_returns or chat_raises"`
Expected: AttributeError on `client.chat`.

- [ ] **Step 3: Implement `chat()`**

Add to `gemini_cli.py`:

```python
import asyncio


_TIMEOUT_SECS = 600  # match Rust spawn.rs::TIMEOUT_SECS


    async def chat(self, request: ChatRequest) -> ChatResponse:
        system, user_prompt = _messages_to_prompt(request.messages)
        if request.system:
            system = request.system
        stdin_payload = _merge_system_into_prompt(system, user_prompt)
        args = self._build_args()

        proc = await asyncio.create_subprocess_exec(
            *args,
            stdin=asyncio.subprocess.PIPE,
            stdout=asyncio.subprocess.PIPE,
            stderr=asyncio.subprocess.PIPE,
        )
        try:
            stdout, stderr = await asyncio.wait_for(
                proc.communicate(stdin_payload.encode()),
                timeout=_TIMEOUT_SECS,
            )
        except TimeoutError as exc:
            proc.kill()
            await proc.wait()
            raise ProviderError(f"gemini CLI timed out after {_TIMEOUT_SECS}s") from exc

        if proc.returncode != 0:
            raise ProviderError(
                f"gemini CLI exited with {proc.returncode}: {stderr.decode().strip()}"
            )

        content = ""
        usage = Usage(0, 0)
        for raw in stdout.decode().splitlines():
            for event in _parse_jsonl_line(raw):
                if event.event_type == "text" and event.content:
                    content += event.content
                elif event.event_type == "usage" and event.usage is not None:
                    usage = event.usage

        return ChatResponse(
            content=content,
            model=request.model or "",
            usage=usage,
            stop_reason=StopReason.end_turn,
        )
```

- [ ] **Step 4: Run — PASS**

Run: `cd sdks/python && uv run pytest tests/test_gemini_cli_stream.py -v`
Expected: 20 PASS.

- [ ] **Step 5: Commit**

```bash
git add sdks/python/motosan_ai/providers/gemini_cli.py sdks/python/tests/test_gemini_cli_stream.py
git commit -m "feat(python,gemini-cli): chat() subprocess + stdin payload + JSONL collect"
```

---

## Task 11: `stream()` — yield events line-by-line

**Files:**
- Modify: `sdks/python/motosan_ai/providers/gemini_cli.py`
- Modify: `sdks/python/tests/test_gemini_cli_stream.py`

Same subprocess setup, yield each parsed event as it arrives.

- [ ] **Step 1: Append failing test**

```python
@pytest.mark.asyncio
async def test_stream_yields_events_in_order(monkeypatch):
    jsonl = (
        '{"type": "init"}\n'
        '{"type": "message", "role": "user", "content": "hi"}\n'
        '{"type": "message", "role": "assistant", "content": "hi", "delta": true}\n'
        '{"type": "result", "status": "success", '
        '"stats": {"input_tokens": 3, "output_tokens": 1}}\n'
    )
    _stub_subprocess(monkeypatch, _FakeProc(jsonl))

    client = GeminiCliClient(binary_path="gemini")
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
    assert events.index(text_events[0]) < events.index(usage_events[0])
    assert events.index(usage_events[0]) < events.index(done_events[0])
```

- [ ] **Step 2: Run — FAIL**

Run: `cd sdks/python && uv run pytest tests/test_gemini_cli_stream.py::test_stream_yields_events_in_order -v`
Expected: AttributeError.

- [ ] **Step 3: Implement `stream()`**

Add to `GeminiCliClient`:

```python
from collections.abc import AsyncIterator
import contextlib


    async def stream(self, request: ChatRequest) -> AsyncIterator[StreamEvent]:
        system, user_prompt = _messages_to_prompt(request.messages)
        if request.system:
            system = request.system
        stdin_payload = _merge_system_into_prompt(system, user_prompt)
        args = self._build_args()

        proc = await asyncio.create_subprocess_exec(
            *args,
            stdin=asyncio.subprocess.PIPE,
            stdout=asyncio.subprocess.PIPE,
            stderr=asyncio.subprocess.PIPE,
        )
        try:
            assert proc.stdin is not None and proc.stdout is not None
            proc.stdin.write(stdin_payload.encode())
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

Run: `cd sdks/python && uv run pytest tests/test_gemini_cli_stream.py -v`
Expected: 21 PASS.

- [ ] **Step 5: Commit**

```bash
git add sdks/python/motosan_ai/providers/gemini_cli.py sdks/python/tests/test_gemini_cli_stream.py
git commit -m "feat(python,gemini-cli): stream() yields events line-by-line"
```

---

## Task 12: Register `Provider.gemini_cli` + `Client.gemini_cli()`

**Files:**
- Modify: `sdks/python/motosan_ai/client.py`
- Modify: `sdks/python/motosan_ai/providers/__init__.py`
- Modify: `sdks/python/motosan_ai/__init__.py`
- Create: `sdks/python/tests/test_gemini_cli_dispatch.py`

Reuse Phase 3b's `Client.__init__(binary_path=...)` parameter; add a dispatch branch and classmethod.

- [ ] **Step 1: Write failing tests**

Create `sdks/python/tests/test_gemini_cli_dispatch.py`:

```python
from __future__ import annotations

from motosan_ai import Client, Provider
from motosan_ai.providers.gemini_cli import GeminiCliClient


def test_provider_enum_has_gemini_cli():
    assert Provider.gemini_cli == "gemini_cli"


def test_client_gemini_cli_classmethod_resolves_to_provider():
    client = Client.gemini_cli()
    assert client.provider == Provider.gemini_cli
    assert isinstance(client._provider, GeminiCliClient)


def test_client_gemini_cli_with_explicit_binary_path():
    client = Client.gemini_cli(binary_path="/opt/gemini")
    assert client._provider._config.binary_path == "/opt/gemini"


def test_gemini_cli_path_env_var_resolved(monkeypatch):
    monkeypatch.setenv("GEMINI_CLI_PATH", "/env/gemini")
    client = Client.gemini_cli()
    assert client._provider._config.binary_path == "/env/gemini"


def test_gemini_cli_does_not_require_api_key(monkeypatch):
    """No API key — gemini_cli is purely subprocess-based."""
    for env in ("GEMINI_API_KEY", "GEMINI_CLI_PATH"):
        monkeypatch.delenv(env, raising=False)
    client = Client(provider=Provider.gemini_cli)
    assert isinstance(client._provider, GeminiCliClient)
```

- [ ] **Step 2: Run — FAIL**

Run: `cd sdks/python && uv run pytest tests/test_gemini_cli_dispatch.py -v`
Expected: AttributeError on `Provider.gemini_cli`.

- [ ] **Step 3: Wire into `Client`**

Edit `sdks/python/motosan_ai/client.py`. Add to the `Provider` enum:

```python
class Provider(StrEnum):
    anthropic = "anthropic"
    openai = "openai"
    minimax = "minimax"
    ollama = "ollama"
    gemini = "gemini"
    codex_cli = "codex_cli"
    gemini_cli = "gemini_cli"
```

In `Client.__init__`, add a branch alongside the existing `codex_cli` branch:

```python
        if provider_value == Provider.gemini_cli:
            from motosan_ai.providers.gemini_cli import GeminiCliClient

            self.provider = provider_value
            self.model = model
            self._max_retries = max_retries
            self.api_key = ""
            self._provider = GeminiCliClient(binary_path=binary_path)
            return
```

Add classmethod:

```python
    @classmethod
    def gemini_cli(
        cls,
        binary_path: str | None = None,
        model: str | None = None,
        max_retries: int = 3,
    ) -> Client:
        return cls(
            provider=Provider.gemini_cli,
            binary_path=binary_path,
            model=model,
            max_retries=max_retries,
        )
```

Edit `sdks/python/motosan_ai/providers/__init__.py` — add `GeminiCliClient`, `ApprovalMode`:

```python
from .gemini_cli import ApprovalMode, GeminiCliClient
```

Add to `__all__`:

```python
"ApprovalMode",
"GeminiCliClient",
```

Edit `sdks/python/motosan_ai/__init__.py` — same top-level exports.

- [ ] **Step 4: Run — PASS**

Run: `cd sdks/python && uv run pytest tests/test_gemini_cli_dispatch.py tests/test_client_integration.py -v`
Expected: all PASS.

- [ ] **Step 5: Commit**

```bash
git add sdks/python/motosan_ai/ sdks/python/tests/test_gemini_cli_dispatch.py
git commit -m "feat(python,gemini-cli): register Provider.gemini_cli + Client.gemini_cli()"
```

---

## Task 13: Live integration tests + release (CHANGELOG + v0.9.2)

**Files:**
- Create: `sdks/python/tests/integration/test_gemini_cli_live.py`
- Modify: `sdks/python/CHANGELOG.md`
- Modify: `sdks/python/pyproject.toml`

- [ ] **Step 1: Create live test file**

```python
"""Live integration tests for GeminiCliClient.

Skip unless `gemini` is on PATH (or `GEMINI_CLI_PATH` points at it) AND
`MOTOSAN_RUN_GEMINI_CLI_LIVE=1` is set. Two-tier gating prevents CI from
incidentally calling the upstream Gemini CLI without explicit opt-in.
"""

from __future__ import annotations

import os
import shutil

import pytest

from motosan_ai.providers.gemini_cli import ApprovalMode, GeminiCliClient
from motosan_ai.types import ChatRequest, Message

_BINARY = os.environ.get("GEMINI_CLI_PATH") or shutil.which("gemini")
_RUN = os.environ.get("MOTOSAN_RUN_GEMINI_CLI_LIVE") == "1"

pytestmark = [
    pytest.mark.skipif(_BINARY is None, reason="gemini binary not on PATH"),
    pytest.mark.skipif(not _RUN, reason="set MOTOSAN_RUN_GEMINI_CLI_LIVE=1 to run"),
    pytest.mark.asyncio,
]


async def test_live_chat_basic():
    client = GeminiCliClient().approval_mode(ApprovalMode.plan)
    resp = await client.chat(
        ChatRequest(messages=[Message.user("Reply with exactly: PONG")])
    )
    assert "PONG" in resp.content


async def test_live_stream_emits_text_then_done():
    client = GeminiCliClient().approval_mode(ApprovalMode.plan)
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
    client = GeminiCliClient().approval_mode(ApprovalMode.plan)
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

- [ ] **Step 2: Bump version**

Edit `sdks/python/pyproject.toml`:

```toml
version = "0.9.2"
```

- [ ] **Step 3: Prepend CHANGELOG entry**

Replace the date with the actual release day.

```markdown
## [0.9.2] - YYYY-MM-DD

### Added — `GeminiCliClient` (Phase 3c)
- New subprocess provider mirroring Rust's `GeminiCliProvider`. Spawns `gemini -p "" -o stream-json [...args]` and parses NDJSON events (`init`, `message`, `result`).
- 11 fluent builder methods cover the full Rust flag surface:
  - Booleans: `yolo` (`--yolo`), `sandbox` (`--sandbox`)
  - Single-value: `model` (`-m`), `approval_mode(ApprovalMode)` (`--approval-mode`), `resume` (`--resume`)
  - Repeating singular: `include_dir` (`--include-directories`), `extension` (`-e`), `allowed_mcp_server` (`--allowed-mcp-server-names`)
  - Repeating plural (replace): `include_dirs`, `extensions`, `allowed_mcp_servers`
- `ApprovalMode` `StrEnum` (`default` / `auto_edit` / `yolo` / `plan`) — values are wire flags.
- `Provider.gemini_cli` registered in `Client` dispatch; new `Client.gemini_cli()` classmethod. Reuses the `binary_path=` parameter on `Client.__init__` introduced in v0.9.1.
- `GEMINI_CLI_PATH` env var resolves the binary location (matches Rust default).
- Stream emits `StreamEvent(usage)` before terminal `done` when `result` carries `stats`; `stats.cached` maps to `Usage.cache_read_input_tokens`.
- System prompt merged into stdin payload via `\n\n` separator (matches Rust `merge_system_into_prompt`).
- Live integration tests under `tests/integration/test_gemini_cli_live.py` (two-tier gate: binary on PATH **plus** `MOTOSAN_RUN_GEMINI_CLI_LIVE=1`).

### Notes
- No API key required — `Provider.gemini_cli` is purely subprocess-based; the `gemini` binary handles its own auth.
- Argv composition order matches Rust `spawn.rs::common_args` byte-for-byte; pinned by `test_full_config_argv_order_matches_rust_common_args`.
- **Distinct from Codex CLI**: Gemini CLI takes the prompt purely via stdin with no trailing `-` argv marker.
- Phase 3d (Gemini Code Assist OAuth + HTTP) ships in v0.9.3.
```

- [ ] **Step 4: Run the full gate**

Run: `cd sdks/python && uv run ruff check motosan_ai/ && uv run ruff format --check motosan_ai/ tests/ && uv run pytest tests/ -q --ignore=tests/integration/`
Expected: all pass.

- [ ] **Step 5: Commit**

```bash
git add sdks/python/tests/integration/test_gemini_cli_live.py sdks/python/CHANGELOG.md sdks/python/pyproject.toml
git commit -m "chore(python): release v0.9.2 — GeminiCliClient (Phase 3c)"
```

---

## Final Self-Review Checklist

Before declaring Phase 3c done, verify:

- [ ] `cd sdks/python && uv run pytest tests/ -v` — all tests pass (target: ~440+ passing).
- [ ] `check-python` gate passes (ruff + format + pytest).
- [ ] Every `pub fn` in [Rust gemini_cli/mod.rs](sdks/rust/src/providers/gemini_cli/mod.rs) has a Python equivalent — cross-check via `grep "pub fn " sdks/rust/src/providers/gemini_cli/mod.rs | sed 's/.*pub fn //; s/(.*//'` against `grep "def " sdks/python/motosan_ai/providers/gemini_cli.py`. Excluding `new` (replaced by `__init__`), 11 builder methods + `with_path` should match.
- [ ] `_build_args` output for the maximally-configured client matches Rust `common_args` byte-for-byte. Pinned by `test_full_config_argv_order_matches_rust_common_args`.
- [ ] **Argv has no trailing `-`** — Gemini CLI distinct from Codex / Claude Code on this point.
- [ ] JSONL parser raises `ProviderError` on non-success `result.status`; everything else returns 0/1/2 events.
- [ ] Stream `usage` event emitted exactly once per stream when `result` carries `stats`; precedes terminal `done`.
- [ ] `Provider.gemini_cli` resolves without an API key.
- [ ] `MOTOSAN_RUN_GEMINI_CLI_LIVE=1` two-tier gate works (skipped without it; runs with it).
- [ ] Version in `pyproject.toml` is `0.9.2` and `CHANGELOG.md` has a matching entry.
- [ ] No `TODO` / `FIXME` / placeholder strings introduced.

If any box is unchecked, fix before tagging/publishing.

---

## What Phase 3c does NOT do

- ❌ Gemini Code Assist OAuth + HTTP — Phase 3d. Distinct from Gemini CLI: Code Assist talks to `cloudcode-pa.googleapis.com` directly via HTTP with a Google OAuth token, no subprocess.
- ❌ Gemini CLI tool-call surfacing — Gemini CLI emits tool execution as `message` events with role `"tool"` (or similar non-assistant role). Per Rust stream_json.rs, only `assistant`+`delta` content surfaces as text. Tool roundtripping out of scope.
- ❌ MCP server hosting via `gemini` CLI subcommands — out of scope; SDK targets headless `-p "" -o stream-json` mode only.
- ❌ Snapshot-based argv test — per-flag `in args` plus the full-config order pin (Task 7) is sufficient. Phase 3a/3b set the precedent.
- ❌ Async tool roundtrips — current `chat`/`stream` accept tools-free `ChatRequest`s and ignore `request.tools`. If callers pass tools, they're silently dropped (matches Rust). A follow-up could feed tool definitions via a Gemini CLI extension config, but that's outside the v0.9.2 scope.

All non-goals tracked in the roadmap doc.
