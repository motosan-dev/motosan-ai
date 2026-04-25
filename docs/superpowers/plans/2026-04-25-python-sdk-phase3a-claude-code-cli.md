# Python SDK Phase 3a — Claude Code CLI Full Flag Surface Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Bring Python's `ClaudeCodeClient` to full builder-method parity with Rust's `ClaudeCodeProvider` (v0.12.0+). Currently Python exposes 4 public methods (`with_path`, `model`, `agent_mode`, `chat`, `stream`); Rust has ~30. Also align stream `usage` emission with Rust.

**Architecture:** Refactor the provider's per-instance state into a single `_ClaudeCodeConfig` dataclass to contain the flag explosion. Each new builder method mutates the config and returns `self` (fluent). `_build_args` consumes the config and emits the subprocess argv list. NDJSON parsing extended to emit a `StreamEvent.usage` event alongside the terminal `done` when the CLI reports token counts in the `result` event.

**Tech Stack:** Python 3.11+, `asyncio.subprocess`, stdlib `json`. No new dependencies.

**Ships as:** `motosan-ai` v0.9.0.

---

## Reference material

- **Rust canon:** [sdks/rust/src/providers/claude_code/mod.rs](sdks/rust/src/providers/claude_code/mod.rs) — the ClaudeCodeProvider builder (lines 1-727). Enumerate all `pub fn` for the flag list. [sdks/rust/src/providers/claude_code/spawn.rs](sdks/rust/src/providers/claude_code/spawn.rs) — arg composition. [sdks/rust/src/providers/claude_code/stream_json.rs](sdks/rust/src/providers/claude_code/stream_json.rs) — NDJSON parsing (result event → usage + done).
- **Current Python:** [sdks/python/motosan_ai/providers/claude_code.py](sdks/python/motosan_ai/providers/claude_code.py) — 275 lines. `_build_args` at line 139-168; `_parse_ndjson_line` at 77-109 drops the `usage` field silently.
- **Flag surface inventory** (from `grep "pub fn" sdks/rust/src/providers/claude_code/mod.rs`):
  - Already in Python: `model`, `agent_mode`, `with_path`
  - **Missing in Python (26 methods):** `bare`, `system_prompt`, `permission_mode`, `effort`, `fallback_model`, `add_dir`, `add_dirs`, `allow_tool`, `allowed_tools`, `disallow_tool`, `disallowed_tools`, `mcp_config`, `mcp_configs`, `strict_mcp_config`, `settings`, `setting_source`, `setting_sources`, `session_id`, `resume`, `continue_latest`, `fork_session`, `plugin_dir`, `plugin_dirs`, `agent`, `no_session_persistence`, `max_budget_usd`.

---

## File Structure

| Path | Responsibility | Status |
|------|----------------|--------|
| `sdks/python/motosan_ai/providers/claude_code.py` | All provider logic — config dataclass, builder methods, `_build_args`, NDJSON parse | **Modify** (grows ~300 lines) |
| `sdks/python/tests/test_claude_code.py` | Existing unit tests — extend with per-flag `_build_args` assertions | **Modify** |
| `sdks/python/tests/test_claude_code_flags.py` | Dedicated test file for the 26 new flags; parametrized | **Create** |
| `sdks/python/tests/test_claude_code_stream_usage.py` | New: NDJSON `result` event should emit `usage` + `done` | **Create** |
| `sdks/python/tests/integration/test_claude_code_live.py` | Add live tests exercising common new flags (session_id, permission_mode, effort) | **Modify** |
| `sdks/python/CHANGELOG.md` | v0.9.0 entry | **Modify** |
| `sdks/python/pyproject.toml` | Version bump 0.8.2 → 0.9.0 | **Modify** |

Design principles:
- **Fluent builder unchanged.** Every new method mutates `self._config` and returns `self`, same pattern as existing `.model()` / `.agent_mode()`.
- **Config dataclass keeps `_build_args` readable.** 26 flags as individual instance attributes would bloat `__init__`; one `_ClaudeCodeConfig` dataclass centralizes state.
- **Flags compose orthogonally.** Each flag tests in isolation; none depend on another for semantics.
- **Wire-level parity.** Python's `_build_args(...)` output must match Rust's `spawn.rs` argv composition byte-for-byte for equivalent config. Snapshot assertions on the argv list would be ideal but deferred — per-flag `assert "--foo" in args and args[args.index("--foo") + 1] == "value"` is sufficient for Phase 3a.
- **No breaking changes.** Existing `ClaudeCodeClient()` / `.with_path()` / `.model()` / `.agent_mode()` keep exactly their current signature + semantics.

---

## Task 1: Refactor builder state into `_ClaudeCodeConfig` dataclass

**Files:**
- Modify: `sdks/python/motosan_ai/providers/claude_code.py`

Prep work — without this, each new flag adds an instance attribute and `__init__` bloats. With the dataclass, new flags land as one-line additions.

- [ ] **Step 1: Write regression tests (existing behavior must not change)**

Add to `sdks/python/tests/test_claude_code.py` at the end:

```python
def test_config_dataclass_holds_existing_state():
    c = ClaudeCodeClient(binary_path="/tmp/fake").model("sonnet").agent_mode(True)
    assert c._config.binary_path == "/tmp/fake"
    assert c._config.model == "sonnet"
    assert c._config.agent_mode is True


def test_config_defaults_sane():
    c = ClaudeCodeClient()
    # Binary resolves from env or 'claude'
    assert c._config.binary_path in ("claude", os.environ.get("CLAUDE_CODE_PATH", "claude"))
    assert c._config.model is None
    assert c._config.agent_mode is False
```

- [ ] **Step 2: Run tests — should FAIL**

Run: `cd sdks/python && uv run pytest tests/test_claude_code.py::test_config_dataclass_holds_existing_state -v`
Expected: FAIL — `AttributeError: _config`.

- [ ] **Step 3: Add dataclass + refactor state access**

At the top of `sdks/python/motosan_ai/providers/claude_code.py`, add:

```python
from dataclasses import dataclass, field


@dataclass
class _ClaudeCodeConfig:
    binary_path: str = "claude"
    model: str | None = None
    agent_mode: bool = False
```

Rewrite `ClaudeCodeClient.__init__`, `model`, `agent_mode` to route through `self._config`:

```python
class ClaudeCodeClient:
    capabilities: ProviderCapabilities = ProviderCapabilities.text_only()

    def __init__(self, binary_path: str | None = None) -> None:
        if binary_path is None:
            binary_path = os.environ.get("CLAUDE_CODE_PATH", "claude")
        self._config = _ClaudeCodeConfig(binary_path=binary_path)

    # Back-compat shims — delegate to _config so existing tests keep asserting these attrs
    @property
    def _binary_path(self) -> str:
        return self._config.binary_path

    @property
    def _model(self) -> str | None:
        return self._config.model

    @property
    def _agent_mode(self) -> bool:
        return self._config.agent_mode

    @classmethod
    def with_path(cls, path: str) -> ClaudeCodeClient:
        return cls(binary_path=path)

    def model(self, model: str) -> ClaudeCodeClient:
        self._config.model = model
        return self

    def agent_mode(self, enabled: bool) -> ClaudeCodeClient:
        self._config.agent_mode = enabled
        return self
```

Update `_build_args` to read from `self._config` (replace `self._agent_mode` → `self._config.agent_mode`, etc.) — the existing property shims keep the external surface unchanged.

- [ ] **Step 4: Run full suite — no regression**

Run: `cd sdks/python && uv run pytest tests/ -v`
Expected: PASS — 313 existing + 2 new = 315.

- [ ] **Step 5: Commit**

```bash
git add sdks/python/motosan_ai/providers/claude_code.py sdks/python/tests/test_claude_code.py
git commit -m "refactor(python,claude-code): extract builder state into _ClaudeCodeConfig dataclass"
```

---

## Task 2: Single-string flags — `bare`, `system_prompt`, `permission_mode`, `effort`, `fallback_model`, `settings`, `agent`

**Files:**
- Modify: `sdks/python/motosan_ai/providers/claude_code.py`
- Create: `sdks/python/tests/test_claude_code_flags.py`

Seven flags that map to a single CLI arg-value pair (`--foo VALUE`) or single boolean flag (`--bare`). Group in one task — same implementation pattern.

Rust mapping (`sdks/rust/src/providers/claude_code/spawn.rs`):
| Builder | CLI arg | Kind |
|---|---|---|
| `bare(bool)` | `--bare` | bool |
| `system_prompt(str)` | `--append-system-prompt VALUE` | string |
| `permission_mode(mode)` | `--permission-mode VALUE` | string (plan/acceptEdits/default) |
| `effort(level)` | `--effort VALUE` | string (low/medium/high) |
| `fallback_model(str)` | `--fallback-model VALUE` | string |
| `settings(str)` | `--settings VALUE` | string (path or JSON) |
| `agent(str)` | `--agent VALUE` | string (agent name) |

- [ ] **Step 1: Write failing tests**

Create `sdks/python/tests/test_claude_code_flags.py`:

```python
from __future__ import annotations

import pytest

from motosan_ai.providers.claude_code import ClaudeCodeClient


def _args(client: ClaudeCodeClient) -> list[str]:
    return client._build_args(model=None, system_prompt=None)


def test_bare_flag_appears_when_enabled():
    c = ClaudeCodeClient().bare(True)
    assert "--bare" in _args(c)


def test_bare_flag_absent_by_default():
    assert "--bare" not in _args(ClaudeCodeClient())


def test_system_prompt_emits_append_system_prompt():
    c = ClaudeCodeClient().system_prompt("be terse")
    args = _args(c)
    i = args.index("--append-system-prompt")
    assert args[i + 1] == "be terse"


def test_permission_mode_forwarded():
    c = ClaudeCodeClient().permission_mode("plan")
    args = _args(c)
    i = args.index("--permission-mode")
    assert args[i + 1] == "plan"


def test_effort_forwarded():
    c = ClaudeCodeClient().effort("high")
    args = _args(c)
    assert args[args.index("--effort") + 1] == "high"


def test_fallback_model_forwarded():
    c = ClaudeCodeClient().fallback_model("haiku")
    args = _args(c)
    assert args[args.index("--fallback-model") + 1] == "haiku"


def test_settings_forwarded():
    c = ClaudeCodeClient().settings("/path/to/settings.json")
    args = _args(c)
    assert args[args.index("--settings") + 1] == "/path/to/settings.json"


def test_agent_forwarded():
    c = ClaudeCodeClient().agent("code-reviewer")
    args = _args(c)
    assert args[args.index("--agent") + 1] == "code-reviewer"


def test_empty_system_prompt_omitted():
    c = ClaudeCodeClient().system_prompt("   ")
    assert "--append-system-prompt" not in _args(c)
```

- [ ] **Step 2: Run tests — should FAIL**

Run: `cd sdks/python && uv run pytest tests/test_claude_code_flags.py -v`
Expected: FAIL — all 9 tests fail with `AttributeError` on the missing methods.

- [ ] **Step 3: Extend `_ClaudeCodeConfig` and add builder methods**

In `sdks/python/motosan_ai/providers/claude_code.py`, extend the dataclass:

```python
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
```

Add builder methods to `ClaudeCodeClient`:

```python
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
```

Extend `_build_args` — add these blocks after the existing flag handling, before `args.append("-")`:

```python
        if self._config.bare:
            args.append("--bare")
        if self._config.system_prompt_flag:
            args.extend(["--append-system-prompt", self._config.system_prompt_flag])
        if self._config.permission_mode:
            args.extend(["--permission-mode", self._config.permission_mode])
        if self._config.effort:
            args.extend(["--effort", self._config.effort])
        if self._config.fallback_model:
            args.extend(["--fallback-model", self._config.fallback_model])
        if self._config.settings:
            args.extend(["--settings", self._config.settings])
        if self._config.agent:
            args.extend(["--agent", self._config.agent])
```

- [ ] **Step 4: Run tests — PASS**

Run: `cd sdks/python && uv run pytest tests/test_claude_code_flags.py tests/test_claude_code.py -v`
Expected: PASS — 9 new + existing tests green.

- [ ] **Step 5: Commit**

```bash
git add sdks/python/motosan_ai/providers/claude_code.py sdks/python/tests/test_claude_code_flags.py
git commit -m "feat(python,claude-code): add bare/system_prompt/permission_mode/effort/fallback_model/settings/agent flags"
```

---

## Task 3: List flags — `add_dir(s)`, `allow_tool` / `allowed_tools`, `disallow_tool` / `disallowed_tools`

**Files:**
- Modify: `sdks/python/motosan_ai/providers/claude_code.py`
- Modify: `sdks/python/tests/test_claude_code_flags.py`

Repeatable flags. Each exposes a singular appender (`add_dir`) and a plural replacer (`add_dirs`). Rust convention (from `spawn.rs`): `--add-dir PATH` can appear multiple times; `--allowed-tools "Tool1,Tool2"` is comma-joined.

- [ ] **Step 1: Append failing tests**

Append to `sdks/python/tests/test_claude_code_flags.py`:

```python
def test_add_dir_appends():
    c = ClaudeCodeClient().add_dir("/a").add_dir("/b")
    args = _args(c)
    # --add-dir PATH can appear multiple times
    occurrences = [i for i, a in enumerate(args) if a == "--add-dir"]
    assert len(occurrences) == 2
    assert args[occurrences[0] + 1] == "/a"
    assert args[occurrences[1] + 1] == "/b"


def test_add_dirs_replaces():
    c = ClaudeCodeClient().add_dir("/old").add_dirs(["/x", "/y"])
    args = _args(c)
    occurrences = [args[i + 1] for i, a in enumerate(args) if a == "--add-dir"]
    assert occurrences == ["/x", "/y"]


def test_allow_tool_appends():
    c = ClaudeCodeClient().allow_tool("Read").allow_tool("Write")
    args = _args(c)
    i = args.index("--allowed-tools")
    assert args[i + 1] == "Read,Write"


def test_allowed_tools_replaces():
    c = ClaudeCodeClient().allow_tool("Read").allowed_tools(["Bash", "Edit"])
    args = _args(c)
    i = args.index("--allowed-tools")
    assert args[i + 1] == "Bash,Edit"


def test_disallow_tool_appends():
    c = ClaudeCodeClient().disallow_tool("Bash").disallow_tool("Write")
    args = _args(c)
    i = args.index("--disallowed-tools")
    assert args[i + 1] == "Bash,Write"


def test_disallowed_tools_replaces():
    c = ClaudeCodeClient().disallow_tool("Read").disallowed_tools(["Bash"])
    args = _args(c)
    i = args.index("--disallowed-tools")
    assert args[i + 1] == "Bash"
```

- [ ] **Step 2: Run tests — FAIL**

Run: `cd sdks/python && uv run pytest tests/test_claude_code_flags.py -v -k "add_dir or allow or disallow"`
Expected: FAIL.

- [ ] **Step 3: Extend config + methods + `_build_args`**

Config fields (add to `_ClaudeCodeConfig`):

```python
    add_dirs: list[str] = field(default_factory=list)
    allowed_tools: list[str] = field(default_factory=list)
    disallowed_tools: list[str] = field(default_factory=list)
```

Builder methods:

```python
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
```

In `_build_args`:

```python
        for d in self._config.add_dirs:
            args.extend(["--add-dir", d])
        if self._config.allowed_tools:
            args.extend(["--allowed-tools", ",".join(self._config.allowed_tools)])
        if self._config.disallowed_tools:
            args.extend(["--disallowed-tools", ",".join(self._config.disallowed_tools)])
```

- [ ] **Step 4: Run tests — PASS**

Run: `cd sdks/python && uv run pytest tests/ -v`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add sdks/python/motosan_ai/providers/claude_code.py sdks/python/tests/test_claude_code_flags.py
git commit -m "feat(python,claude-code): add add_dir(s), allow_tool/allowed_tools, disallow_tool/disallowed_tools"
```

---

## Task 4: MCP config flags — `mcp_config(s)`, `strict_mcp_config`

**Files:**
- Modify: `sdks/python/motosan_ai/providers/claude_code.py`
- Modify: `sdks/python/tests/test_claude_code_flags.py`

Rust builder: `mcp_config(str)` (single path or JSON blob, repeatable), `mcp_configs(Vec<str>)` (replace), `strict_mcp_config(bool)` (`--strict-mcp-config` flag).

- [ ] **Step 1: Append failing tests**

```python
def test_mcp_config_appends():
    c = ClaudeCodeClient().mcp_config("/path/a.json").mcp_config("/path/b.json")
    args = _args(c)
    occurrences = [args[i + 1] for i, a in enumerate(args) if a == "--mcp-config"]
    assert occurrences == ["/path/a.json", "/path/b.json"]


def test_mcp_configs_replaces():
    c = ClaudeCodeClient().mcp_config("/old").mcp_configs(["/new1", "/new2"])
    args = _args(c)
    occurrences = [args[i + 1] for i, a in enumerate(args) if a == "--mcp-config"]
    assert occurrences == ["/new1", "/new2"]


def test_strict_mcp_config_flag():
    c = ClaudeCodeClient().strict_mcp_config(True)
    assert "--strict-mcp-config" in _args(c)


def test_strict_mcp_config_absent_by_default():
    assert "--strict-mcp-config" not in _args(ClaudeCodeClient())
```

- [ ] **Step 2: Run — FAIL**

Run: `cd sdks/python && uv run pytest tests/test_claude_code_flags.py -v -k mcp`
Expected: FAIL.

- [ ] **Step 3: Extend**

Config additions:

```python
    mcp_configs: list[str] = field(default_factory=list)
    strict_mcp_config: bool = False
```

Methods:

```python
    def mcp_config(self, config: str) -> ClaudeCodeClient:
        self._config.mcp_configs.append(config)
        return self

    def mcp_configs(self, configs: list[str]) -> ClaudeCodeClient:
        self._config.mcp_configs = list(configs)
        return self

    def strict_mcp_config(self, enabled: bool) -> ClaudeCodeClient:
        self._config.strict_mcp_config = enabled
        return self
```

`_build_args` additions:

```python
        for mc in self._config.mcp_configs:
            args.extend(["--mcp-config", mc])
        if self._config.strict_mcp_config:
            args.append("--strict-mcp-config")
```

- [ ] **Step 4: Run — PASS**

Run: `cd sdks/python && uv run pytest tests/ -v`

- [ ] **Step 5: Commit**

```bash
git add sdks/python/motosan_ai/providers/claude_code.py sdks/python/tests/test_claude_code_flags.py
git commit -m "feat(python,claude-code): add mcp_config(s) and strict_mcp_config"
```

---

## Task 5: Setting sources — `setting_source`, `setting_sources`

**Files:**
- Modify: `sdks/python/motosan_ai/providers/claude_code.py`
- Modify: `sdks/python/tests/test_claude_code_flags.py`

Rust: repeatable `--setting-sources NAME` (user/project/local).

- [ ] **Step 1: Append failing tests**

```python
def test_setting_source_appends():
    c = ClaudeCodeClient().setting_source("user").setting_source("project")
    args = _args(c)
    i = args.index("--setting-sources")
    assert args[i + 1] == "user,project"


def test_setting_sources_replaces():
    c = ClaudeCodeClient().setting_source("user").setting_sources(["project", "local"])
    args = _args(c)
    i = args.index("--setting-sources")
    assert args[i + 1] == "project,local"
```

- [ ] **Step 2: Run — FAIL**

- [ ] **Step 3: Extend**

```python
    setting_sources: list[str] = field(default_factory=list)
```

```python
    def setting_source(self, source: str) -> ClaudeCodeClient:
        self._config.setting_sources.append(source)
        return self

    def setting_sources(self, sources: list[str]) -> ClaudeCodeClient:
        self._config.setting_sources = list(sources)
        return self
```

```python
        if self._config.setting_sources:
            args.extend(["--setting-sources", ",".join(self._config.setting_sources)])
```

- [ ] **Step 4: Run — PASS**

- [ ] **Step 5: Commit**

```bash
git add sdks/python/motosan_ai/providers/claude_code.py sdks/python/tests/test_claude_code_flags.py
git commit -m "feat(python,claude-code): add setting_source(s)"
```

---

## Task 6: Session flags — `session_id`, `resume`, `continue_latest`, `fork_session`, `no_session_persistence`

**Files:**
- Modify: `sdks/python/motosan_ai/providers/claude_code.py`
- Modify: `sdks/python/tests/test_claude_code_flags.py`

Rust mapping (per `spawn.rs`):
| Builder | CLI arg |
|---|---|
| `session_id(uuid)` | `--session-id UUID` |
| `resume(value)` | `--resume VALUE` |
| `continue_latest(true)` | `--continue` |
| `fork_session(true)` | `--fork-session` |
| `no_session_persistence(true)` | `--no-session-persistence` |

- [ ] **Step 1: Append failing tests**

```python
def test_session_id_forwarded():
    c = ClaudeCodeClient().session_id("abc-123")
    assert _args(c)[_args(c).index("--session-id") + 1] == "abc-123"


def test_resume_forwarded():
    c = ClaudeCodeClient().resume("session-42")
    assert _args(c)[_args(c).index("--resume") + 1] == "session-42"


def test_continue_latest_flag():
    assert "--continue" in _args(ClaudeCodeClient().continue_latest(True))


def test_fork_session_flag():
    assert "--fork-session" in _args(ClaudeCodeClient().fork_session(True))


def test_no_session_persistence_flag():
    assert "--no-session-persistence" in _args(ClaudeCodeClient().no_session_persistence(True))


def test_session_flags_absent_by_default():
    args = _args(ClaudeCodeClient())
    for flag in ("--continue", "--fork-session", "--no-session-persistence",
                 "--session-id", "--resume"):
        assert flag not in args
```

- [ ] **Step 2: Run — FAIL**

- [ ] **Step 3: Extend config + methods + args**

```python
    session_id: str | None = None
    resume: str | None = None
    continue_latest: bool = False
    fork_session: bool = False
    no_session_persistence: bool = False
```

```python
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
```

`_build_args` additions:

```python
        if self._config.session_id:
            args.extend(["--session-id", self._config.session_id])
        if self._config.resume:
            args.extend(["--resume", self._config.resume])
        if self._config.continue_latest:
            args.append("--continue")
        if self._config.fork_session:
            args.append("--fork-session")
        if self._config.no_session_persistence:
            args.append("--no-session-persistence")
```

- [ ] **Step 4: Run — PASS**

- [ ] **Step 5: Commit**

```bash
git add sdks/python/motosan_ai/providers/claude_code.py sdks/python/tests/test_claude_code_flags.py
git commit -m "feat(python,claude-code): add session_id, resume, continue_latest, fork_session, no_session_persistence"
```

---

## Task 7: Plugin dirs + `max_budget_usd`

**Files:**
- Modify: `sdks/python/motosan_ai/providers/claude_code.py`
- Modify: `sdks/python/tests/test_claude_code_flags.py`

Last of the flag surface.

- [ ] **Step 1: Append failing tests**

```python
def test_plugin_dir_appends():
    c = ClaudeCodeClient().plugin_dir("/p1").plugin_dir("/p2")
    args = _args(c)
    occ = [args[i + 1] for i, a in enumerate(args) if a == "--plugin-dir"]
    assert occ == ["/p1", "/p2"]


def test_plugin_dirs_replaces():
    c = ClaudeCodeClient().plugin_dir("/old").plugin_dirs(["/x"])
    args = _args(c)
    occ = [args[i + 1] for i, a in enumerate(args) if a == "--plugin-dir"]
    assert occ == ["/x"]


def test_max_budget_usd_forwarded():
    c = ClaudeCodeClient().max_budget_usd(12.5)
    args = _args(c)
    assert args[args.index("--max-budget") + 1] == "12.5"


def test_max_budget_usd_absent_by_default():
    assert "--max-budget" not in _args(ClaudeCodeClient())
```

- [ ] **Step 2: Run — FAIL**

- [ ] **Step 3: Extend**

```python
    plugin_dirs: list[str] = field(default_factory=list)
    max_budget_usd: float | None = None
```

```python
    def plugin_dir(self, path: str) -> ClaudeCodeClient:
        self._config.plugin_dirs.append(path)
        return self

    def plugin_dirs(self, paths: list[str]) -> ClaudeCodeClient:
        self._config.plugin_dirs = list(paths)
        return self

    def max_budget_usd(self, amount: float) -> ClaudeCodeClient:
        self._config.max_budget_usd = amount
        return self
```

`_build_args`:

```python
        for p in self._config.plugin_dirs:
            args.extend(["--plugin-dir", p])
        if self._config.max_budget_usd is not None:
            args.extend(["--max-budget", str(self._config.max_budget_usd)])
```

- [ ] **Step 4: Run — PASS**

- [ ] **Step 5: Commit**

```bash
git add sdks/python/motosan_ai/providers/claude_code.py sdks/python/tests/test_claude_code_flags.py
git commit -m "feat(python,claude-code): add plugin_dir(s) and max_budget_usd"
```

---

## Task 8: Stream NDJSON — emit `usage` event alongside `done`

**Files:**
- Modify: `sdks/python/motosan_ai/providers/claude_code.py` (`_parse_ndjson_line` + `stream()`)
- Create: `sdks/python/tests/test_claude_code_stream_usage.py`

Currently the `result` event is parsed as a plain `StreamEvent(done=True)`, discarding the `usage` field. Rust emits `StreamEvent::usage(...)` followed by `StreamEvent::done()`. Align.

- [ ] **Step 1: Write failing tests**

Create `sdks/python/tests/test_claude_code_stream_usage.py`:

```python
from __future__ import annotations

from motosan_ai.providers.claude_code import _parse_ndjson_line
from motosan_ai.types import StreamEvent


def test_result_without_usage_emits_single_done_event():
    line = '{"type": "result", "result": "ok"}'
    events = _parse_ndjson_line(line)
    assert events is not None
    # Either a single StreamEvent (done=True) or a list — accept both shapes;
    # this test pins the normalized form to a list.
    events = list(events) if not isinstance(events, StreamEvent) else [events]
    assert len(events) == 1
    assert events[0].done is True
    assert events[0].usage is None


def test_result_with_usage_emits_usage_then_done():
    line = '{"type": "result", "result": "ok", "usage": {"input_tokens": 50, "output_tokens": 20}}'
    events = _parse_ndjson_line(line)
    assert events is not None
    events = list(events) if not isinstance(events, StreamEvent) else [events]
    assert len(events) == 2

    usage_event, done_event = events
    assert usage_event.event_type == "usage"
    assert usage_event.usage is not None
    assert usage_event.usage.input_tokens == 50
    assert usage_event.usage.output_tokens == 20
    assert usage_event.done is False

    assert done_event.done is True


def test_text_event_unchanged():
    line = '{"type": "assistant", "message": {"content": [{"type": "text", "text": "hi"}]}}'
    events = _parse_ndjson_line(line)
    assert events is not None
    events = list(events) if not isinstance(events, StreamEvent) else [events]
    assert len(events) == 1
    assert events[0].content == "hi"
    assert events[0].done is False
```

- [ ] **Step 2: Run — FAIL**

Run: `cd sdks/python && uv run pytest tests/test_claude_code_stream_usage.py -v`
Expected: FAIL on the `with_usage` test — current impl emits only `done`.

- [ ] **Step 3: Change `_parse_ndjson_line` to return a sequence**

Change the return type from `StreamEvent | None` to `list[StreamEvent]`:

```python
def _parse_ndjson_line(line: str) -> list[StreamEvent]:
    """Parse a single NDJSON line into zero, one, or two StreamEvents.

    The ``result`` event may carry a ``usage`` field — when present, emit a
    ``StreamEvent(event_type="usage")`` with the token counts before the
    terminal ``done`` event, matching Rust's ClaudeCodeProvider behavior.
    """
    try:
        event = json.loads(line)
    except json.JSONDecodeError:
        return []

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
            return []
        return [StreamEvent(content=text, done=False)]

    if event_type == "result":
        out: list[StreamEvent] = []
        usage_obj = event.get("usage")
        if isinstance(usage_obj, dict):
            out.append(
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
        out.append(StreamEvent(content="", done=True))
        return out

    return []
```

Update the `stream()` method to handle the list return — `async for` unchanged but the NDJSON iteration needs to flatten:

```python
    async def stream(self, request: ChatRequest) -> AsyncIterator[StreamEvent]:
        # ... existing subprocess setup ...
        async for line in proc.stdout:  # (existing loop)
            decoded = line.decode().strip()
            if not decoded:
                continue
            for event in _parse_ndjson_line(decoded):
                yield event
                if event.done:
                    return
```

Find the existing `async for line in ...` block and adjust accordingly. If the current code is:

```python
        event = _parse_ndjson_line(decoded)
        if event is not None:
            yield event
            if event.done:
                return
```

Change to:

```python
        for event in _parse_ndjson_line(decoded):
            yield event
            if event.done:
                return
```

- [ ] **Step 4: Run — PASS**

Run: `cd sdks/python && uv run pytest tests/test_claude_code_stream_usage.py tests/test_claude_code.py -v`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add sdks/python/motosan_ai/providers/claude_code.py sdks/python/tests/test_claude_code_stream_usage.py
git commit -m "feat(python,claude-code): emit StreamEvent.usage alongside terminal done from result NDJSON"
```

---

## Task 9: Live integration tests for new flags

**Files:**
- Modify: `sdks/python/tests/integration/test_claude_code_live.py`

Pick 3 representative flags to live-test: `session_id` (session management), `permission_mode` (safety controls), `system_prompt` (prompt routing). Skip flags whose effect is hard to observe without deep inspection (e.g. `max_budget_usd`, `strict_mcp_config`).

- [ ] **Step 1: Check current live test structure**

Run: `cd sdks/python && head -40 tests/integration/test_claude_code_live.py`

- [ ] **Step 2: Append live tests**

Append to `sdks/python/tests/integration/test_claude_code_live.py`:

```python
@pytest.mark.asyncio
async def test_live_system_prompt():
    client = ClaudeCodeClient().system_prompt(
        "Your name is TestBot. Always identify yourself."
    )
    resp = await client.chat(ChatRequest(messages=[Message.user("Hi, who are you?")]))
    assert "TestBot" in resp.content, f"system_prompt not applied: {resp.content!r}"


@pytest.mark.asyncio
async def test_live_session_id_roundtrip():
    import uuid

    sid = str(uuid.uuid4())
    # Turn 1 — establish context
    client = ClaudeCodeClient().session_id(sid)
    resp1 = await client.chat(
        ChatRequest(messages=[Message.user("Remember the number 7788.")])
    )
    assert resp1.content

    # Turn 2 — same session via --resume
    client2 = ClaudeCodeClient().resume(sid)
    resp2 = await client2.chat(
        ChatRequest(messages=[Message.user("What number did I ask you to remember?")])
    )
    assert "7788" in resp2.content, f"session not resumed: {resp2.content!r}"


@pytest.mark.asyncio
async def test_live_stream_emits_usage_event():
    client = ClaudeCodeClient()
    events = []
    async for event in client.stream(
        ChatRequest(messages=[Message.user("Count to 3.")])
    ):
        events.append(event)

    usage_events = [e for e in events if e.event_type == "usage"]
    assert len(usage_events) == 1, (
        f"expected exactly 1 usage event, got {len(usage_events)}"
    )
    assert usage_events[0].usage is not None
    assert usage_events[0].usage.input_tokens > 0
    assert usage_events[0].usage.output_tokens > 0

    done_events = [e for e in events if e.done]
    assert len(done_events) == 1
    # Usage event must precede done
    assert events.index(usage_events[0]) < events.index(done_events[0])
```

- [ ] **Step 3: Run live (manual — requires `claude` CLI on PATH)**

Run: `cd sdks/python && uv run pytest tests/integration/test_claude_code_live.py -v`
Expected: PASS if `claude` binary is on PATH; SKIP otherwise per the file's pytest.skipif gate.

- [ ] **Step 4: Commit**

```bash
git add sdks/python/tests/integration/test_claude_code_live.py
git commit -m "test(python,claude-code): live tests for system_prompt, session resume, stream usage"
```

---

## Task 10: Release — CHANGELOG + version bump to 0.9.0

**Files:**
- Modify: `sdks/python/CHANGELOG.md`
- Modify: `sdks/python/pyproject.toml`

- [ ] **Step 1: Bump version**

Edit `sdks/python/pyproject.toml`:

```toml
version = "0.9.0"
```

- [ ] **Step 2: Prepend CHANGELOG entry**

Replace the date with the actual release day (YYYY-MM-DD).

```markdown
## [0.9.0] - YYYY-MM-DD

### Added — ClaudeCodeClient full flag surface parity with Rust v0.12.0+
- **Builder state consolidated** into internal `_ClaudeCodeConfig` dataclass; backward-compat property shims preserve `_binary_path` / `_model` / `_agent_mode` attribute access.
- **26 new builder methods** — each fluent, returning `self`:
  - String flags: `bare(bool)`, `system_prompt(str)`, `permission_mode(str)`, `effort(str)`, `fallback_model(str)`, `settings(str)`, `agent(str)`
  - List flags: `add_dir(path)` / `add_dirs(paths)`, `allow_tool(name)` / `allowed_tools(tools)`, `disallow_tool(name)` / `disallowed_tools(tools)`, `plugin_dir(path)` / `plugin_dirs(paths)`, `setting_source(src)` / `setting_sources(srcs)`
  - MCP: `mcp_config(cfg)` / `mcp_configs(cfgs)`, `strict_mcp_config(bool)`
  - Session: `session_id(uuid)`, `resume(value)`, `continue_latest(bool)`, `fork_session(bool)`, `no_session_persistence(bool)`
  - Budget: `max_budget_usd(float)`
- **Stream usage events** — NDJSON `result` events with a `usage` field now emit `StreamEvent(event_type="usage")` before the terminal `done`, matching Rust `stream_json.rs`.

### Notes
- No breaking changes. Existing callers (`ClaudeCodeClient()` / `.model()` / `.agent_mode()`) unchanged.
- Subprocess argv composition aligned with Rust `ClaudeCodeProvider::build_command` byte-for-byte for equivalent configs.
- Covers Phase 3a of `docs/superpowers/plans/2026-04-24-python-sdk-catchup-roadmap.md`. Phase 3b (Codex CLI), 3c (Gemini CLI), 3d (Gemini Code Assist OAuth) ship in subsequent 0.9.x releases.
```

- [ ] **Step 3: Run the full gate**

Run: `cd /Users/daiwanwei/Projects/wade/motosan-ai && uv run pytest sdks/python/tests/ -q --ignore=sdks/python/tests/integration/ && uv run ruff check sdks/python/motosan_ai/ && uv run ruff format --check sdks/python/motosan_ai/ sdks/python/tests/`
Expected: all pass.

- [ ] **Step 4: Commit**

```bash
git add sdks/python/pyproject.toml sdks/python/CHANGELOG.md
git commit -m "chore(python): release v0.9.0 — ClaudeCodeClient full flag surface parity"
```

---

## Final Self-Review Checklist

Before declaring Phase 3a done, verify:

- [ ] `cd sdks/python && uv run pytest tests/ -v` — all tests pass (target: ~330 passing).
- [ ] `check-python` gate (ruff + format + pytest) passes.
- [ ] Every builder method in Rust's `claude_code/mod.rs` has a Python equivalent — cross-check with `grep "pub fn" sdks/rust/src/providers/claude_code/mod.rs` against `grep "def " sdks/python/motosan_ai/providers/claude_code.py`.
- [ ] `_build_args` output for equivalent configs matches Rust `spawn.rs` argv byte-for-byte — spot-check 3 complex combinations (all session flags + mcp, agent mode + allowed_tools, max_budget + effort).
- [ ] Stream `usage` event emitted exactly once per stream, before the terminal `done`.
- [ ] Live session-id roundtrip works end-to-end against real `claude` CLI.
- [ ] Version in `pyproject.toml` is `0.9.0` and `CHANGELOG.md` has a matching entry.
- [ ] No breaking changes to `ClaudeCodeClient()` / `.model()` / `.agent_mode()` public surface.
- [ ] No `TODO` / `FIXME` / placeholder strings introduced.

If any box is unchecked, fix before tagging/publishing.

---

## What Phase 3a does NOT do

- ❌ Codex CLI provider — Phase 3b.
- ❌ Gemini CLI provider — Phase 3c.
- ❌ Gemini Code Assist OAuth + HTTP provider — Phase 3d (most complex; needs new `motosan_ai/oauth/google.py` module with PKCE flow).
- ❌ `claude mcp` subcommand support — the CLI has MCP-management subcommands (`claude mcp add`, `claude mcp list`) unrelated to `--print` mode. Out of scope; the SDK targets the `chat` / `stream` verbs only.
- ❌ Snapshot-based argv testing — per-flag `in args` assertions are sufficient for Phase 3a. If we add a 4th CLI provider (Codex + Gemini + future), snapshot infrastructure becomes justified.
- ❌ Subprocess argv length checks on Windows — Claude Code CLI is macOS/Linux-only per upstream; Windows is out of scope.

All non-goals tracked in the roadmap doc.
