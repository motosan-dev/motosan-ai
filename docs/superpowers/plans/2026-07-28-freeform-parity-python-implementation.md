# Freeform Tool Parity — Python Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Port Rust's native model / Freeform-custom-tool API to the Python SDK, and widen `specs/types.md` § Native Model API from a Rust-only contract to a cross-SDK one, so Python implements the spec rather than trailing it.

**Architecture:** A new pure codec module `motosan_ai/providers/responses.py` owns every byte of OpenAI-Responses wire encoding and decoding for the native path; the type module stays wire-free. Two providers — `ChatGptCodexProvider` (native by default) and `OpenAIProvider` (native only behind a `responses_api` opt-in) — call the codec and expose `model_chat` / `model_stream`. `Client` grows a `model_chat_with` / `model_stream_with` / `model_stream_collect_with` trio that **duck-types** the provider, because `BaseProvider` is subclassed by only 4 of the 11 Python providers.

**Tech Stack:** Python 3.11+, `httpx` (async), frozen `dataclasses` + union type aliases discriminated by `isinstance`, `StrEnum`, `pytest` (`asyncio_mode = "auto"`), `respx` for HTTP mocking, `ruff` + `mypy` as CI gates, `uv` as the runner.

## Global Constraints

- Baseline is `origin/main`. Never push code straight to `main`; every task group below ships as its own PR.
- Tracking issue is **#270**. Commit subjects use a bare conventional type — `feat:` / `fix:` / `refactor:` and nothing else, **no scope parentheses** — and end with `(#270)`. Documented in `AGENTS.md` § Commits. `docs:`, `test:`, `chore:` and `ci:` are **not** allowed here: a spec widening is a `feat:` because it extends the contract, and a new conformance suite is a `feat:` because it adds a gate that did not exist. **PR titles follow the same rule as commit subjects** — a PR title is what a reviewer reads first, and letting it drift from the commit defeats the convention.
- Every commit carries `Co-Authored-By: Claude Opus 5 (1M context) <noreply@anthropic.com>` as a **second** `-m` argument.
- Python gates, all run from `sdks/python/`: `uv run ruff check motosan_ai/`, `uv run ruff format --check motosan_ai/ tests/`, `uv run mypy motosan_ai/`, `uv run pytest tests/ -q --ignore=tests/integration/`. CI enforces mypy — the package is mypy-clean and must stay so.
- Repo-wide gates, run from the repository root: `treefmt --fail-on-change` and `python3 scripts/check-versions.py`.
- Run `uv sync --all-extras` in `sdks/python/` in any fresh worktree before pushing.
- Verify every push landed by SHA: `test "$(git ls-remote origin refs/heads/<branch> | cut -f1)" = "$(git rev-parse HEAD)"`.
- CLAUDE.md rules that bind this work: provider logic lives **only** under `providers/`; **no sync wrappers** in Python (callers use `asyncio.run()`); the `LlmClient` Protocol (`chat` / `stream` shapes, consumed by motosan-chat) must not break — the native methods are strictly additive.
- **ruff TC001 gotcha (verified with ruff 0.12.8 against this repo):** a first-party import used *only* in annotations is flagged TC001 — **unless** the same `from … import (…)` statement also contains a name that is used at runtime. Every new type name in this plan is added to an **existing** import block that already has a runtime-used name, so ruff stays clean. Do not split them into new import statements.
- **No version bumps in this plan.** Python 0.20.0 ships in the separate REL PR via `scripts/bump-version.py`; `scripts/check-versions.py` must stay green, which it does as long as no manifest is touched.
- Type names are fixed across all tasks. A name introduced in Task 3 is spelled identically in Task 16.

---

## File Structure

| File | Action | Responsibility |
|---|---|---|
| `specs/types.md` | Modify | Widen § Native Model API from a Rust-only to a cross-SDK normative contract; record D3's omitted fields and D4's Python error type; set an implementation-status line that does **not** claim Python/TS ship it yet. |
| `sdks/python/motosan_ai/error.py` | Modify | Add `UnsupportedFeatureError(InvalidRequestError)`. |
| `sdks/python/motosan_ai/types.py` | Modify | Native model value types: `FreeformTool*`, `ModelToolSpec*`, `ModelToolCall*`, `FunctionCallOutput*`, `ModelToolOutput*`, `ModelContext*`, `ModelChatRequest(+Builder)`, `ModelChatResponse`, `ModelStream*`. Zero wire encoding. |
| `sdks/python/motosan_ai/providers/responses.py` | **Create** | The shared OpenAI Responses codec: encoders, decoders, `build_model_request_body`, and the native SSE frame parser. Pure — no HTTP. |
| `sdks/python/motosan_ai/provider_base.py` | Modify | `ProviderCapabilities.supports_freeform_tools` + two named constructors; module-level `validate_model_request`; `BaseProvider.validate_model_request`. |
| `sdks/python/motosan_ai/_stream_collect.py` | Modify | Add `collect_model_stream` — the native collector. |
| `sdks/python/motosan_ai/providers/chatgpt_codex.py` | Modify | `capabilities` → freeform; `build_model_responses_body`; `model_stream`; `model_chat`. |
| `sdks/python/motosan_ai/providers/openai.py` | Modify | `responses_api` / `responses_url` opt-in; capability switch; `_responses_endpoint`; `validate_model_request`; `model_chat`; `model_stream`. |
| `sdks/python/motosan_ai/client.py` | Modify | `openai_responses_api` threaded through `__init__` and `Client.openai()`; `model_chat_with` / `model_stream_with` / `model_stream_collect_with`; duck-typed dispatch. |
| `sdks/python/motosan_ai/__init__.py` | Modify | Explicit imports + `__all__` entries for every new public symbol (twice: P1 symbols, then P2 symbols). |
| `sdks/python/tests/test_unsupported_feature_error.py` | **Create** | Pins the `UnsupportedFeatureError` → `InvalidRequestError` → `MotosanError` chain. |
| `sdks/python/tests/test_native_types.py` | **Create** | Pins the native value types and the request builder. |
| `sdks/python/tests/test_responses_codec.py` | **Create** | Pins the codec: encoders, decoders, body builder, SSE frame parser. |
| `sdks/python/tests/test_public_exports.py` | **Create** | Pins the package export surface — every native symbol importable from `motosan_ai` and listed in `__all__`. |
| `sdks/python/tests/test_native_capabilities.py` | **Create** | Pins the capability constructors and `validate_model_request`. |
| `sdks/python/tests/test_native_collect_stream.py` | **Create** | Pins `collect_model_stream`. |
| `sdks/python/tests/test_chatgpt_codex_native.py` | **Create** | Pins the Codex native body and native stream over `respx`. |
| `sdks/python/tests/test_openai_native.py` | **Create** | Pins the OpenAI Responses opt-in, native chat, native stream, and pre-network rejection. |
| `sdks/python/tests/test_client_native.py` | **Create** | Pins the `Client` native trio and duck-typed dispatch. |
| `sdks/python/tests/test_freeform_conformance.py` | **Create** | The Python half of the cross-SDK freeform conformance suite, anchored to `specs/types.md` § Native Model API. |

**Out of scope for this plan:** the TypeScript track (PRs T1/T2), the Rust and TypeScript halves of the conformance suite, and the release PR (`scripts/bump-version.py` handles versions).

### Task → PR map

| PR | Branch | Tasks | Milestone decisions implemented |
|---|---|---|---|
| **S** | `docs/freeform-spec-widen` | 1 | D3, D4, D8, D10 (step one of the two-step spec rule) |
| **P1** | `feat/freeform-python-types` | 2–9 | D2, D3, D4, plus the package-root export duty |
| **P2** | `feat/freeform-python-providers` | 10–15 | D1, D5, D6, D7, D8 |
| **C-PY** | `test/freeform-python-conformance` | 16 | D8, D9 |

---

### Task 1: Widen `specs/types.md` § Native Model API to a cross-SDK contract

**Files:**
- Modify: `specs/types.md:58` (the freeform capability row)
- Modify: `specs/types.md:107-222` (§ Native Model API)
- Test: none — this is the normative document; `python3 scripts/check-versions.py` is the machine gate.

**Interfaces:**
- Consumes: nothing.
- Produces: the normative wording every later task is written against — the `UnsupportedFeatureError` name (Task 2), the two `IncompleteStream` payload strings `openai ended without a terminal event` / `chatgpt-codex ended without a terminal event` (Tasks 13, 14, 16), and the statement that `thinking` / `mcp_servers` / `mcp_tool_configs` are **not** part of the native request in Python and TypeScript (Task 3).

- [ ] **Step 1: Write the failing test**

There is no unit test for a spec document. The verification is the repo-wide gate plus a grep that proves the two-step rule of D10 was honoured — S widens *what the API must do* and must **not** claim *which SDKs ship it*. Write this shell check and keep it for Step 2 and Step 4:

```bash
# From the repository root. Passes only after Step 3.
set -e
grep -q 'Implemented in Rust 0.26.0+. Python and TypeScript ports in progress — see #270.' specs/types.md
grep -q '^## Native Model API$' specs/types.md
# The spec must NOT yet claim Python/TypeScript ship the native API.
! grep -qE 'Native Model API \(Rust, v0\.26\.0\+\)' specs/types.md
! grep -qE 'Python 0\.20\.0|TypeScript 0\.16\.0' specs/types.md
grep -q 'UnsupportedFeatureError' specs/types.md
grep -q 'chatgpt-codex ended without a terminal event' specs/types.md
echo "spec widening OK"
```

- [ ] **Step 2: Run it to verify it fails**

Run: `cd /path/to/worktree && bash -c '<the script above>'`
Expected: FAIL at the first `grep -q`, because `specs/types.md:107` still reads `## Native Model API (Rust, v0.26.0+)` and the implementation-status line does not exist. The shell exits non-zero with no `spec widening OK` output.

- [ ] **Step 3: Implement**

Edit 1 — `specs/types.md:58`. Replace:

```
| `supports_freeform_tools` | `bool` | Provider accepts native Rust `ModelToolSpec::Freeform` / `ModelToolCall::Freeform` transport |
```

with:

```
| `supports_freeform_tools` | `bool` | Provider accepts native `ModelToolSpec::Freeform` / `ModelToolCall::Freeform` transport (Rust 0.26.0+; Python and TypeScript ports in progress — see #270) |
```

Edit 2 — `specs/types.md:107-112`. Replace the heading and its lead paragraph:

```
## Native Model API (Rust, v0.26.0+)

The legacy cross-SDK `ChatRequest`, `Tool`, `ToolCall`, `ChatResponse`, and
`StreamEvent` APIs remain function-tool-only. Rust v0.26.0 adds a parallel
native model API for providers that expose OpenAI Responses-style ordered input
items and custom tool calls.
```

with:

```
## Native Model API

> Implemented in Rust 0.26.0+. Python and TypeScript ports in progress — see #270.

The legacy cross-SDK `ChatRequest`, `Tool`, `ToolCall`, `ChatResponse`, and
`StreamEvent` APIs remain function-tool-only. Every SDK MUST expose a parallel
native model API for providers that speak OpenAI Responses-style ordered input
items and custom tool calls. The type shapes below are normative; each SDK
models them in its own idiom (Rust enums, Python variant dataclasses behind a
union alias, TypeScript discriminated unions), and each SDK's wire encoding
MUST live outside its type module — Rust `providers/responses.rs`, Python
`providers/responses.py`, TypeScript `serialize/responses.ts`.

Wire keys deliberately differ from field names and MUST be honoured exactly:
`ModelToolCall.id` ↔ wire `call_id` (deserialization accepts `call_id` **or**
`id`); `ModelChatRequest.max_tokens` ↔ body `max_output_tokens`;
`Tool.input_schema` ↔ tool-spec `parameters`. When building a request body,
system messages carried inside `context` MUST be hoisted into `instructions`
**and removed from `input`**, and `provider_options` MUST be shallow-merged
**last**, so a caller can override anything the encoder produced.
```

Edit 3 — insert a new subsection immediately **after** the "### Calls and outputs" block (after the `FunctionCallOutputPayload` code fence that ends at what is currently line 169), before "### Requests, responses, and streams":

```
### Fields the native request does not carry

`ModelChatRequest` MUST NOT accept extended thinking or MCP configuration.
Rust models `thinking`, `mcp_servers`, and `mcp_tool_configs` as fields that
exist only so validation can reject them; Python and TypeScript omit the
fields entirely, so a caller who reaches for one gets an attribute or type
error instead of a runtime rejection. Both spellings satisfy this contract.
Provider-specific reasoning controls travel through `provider_options`.

### Rejection error type

Rejections that happen before any network I/O — an unsupported native
Freeform spec or history, unsupported image or document content, a provider
with no native path — surface as:

| SDK | Spelling |
|-----|----------|
| Rust | `MotosanError::UnsupportedFeature(String)` |
| Python | `class UnsupportedFeatureError(InvalidRequestError)` |
| TypeScript | `export class UnsupportedFeatureError extends MotosanError` |

Python subclasses `InvalidRequestError` deliberately, as a migration softener:
existing `except InvalidRequestError` handlers keep working while callers that
must distinguish match the subclass. It stays non-retryable by inheritance.
```

Edit 4 — replace the "### Provider support" paragraph (currently lines 203-209):

```
### Provider support

OpenAI supports the native API only when Rust callers opt into
`ClientBuilder::openai_responses_api(true)` or
`OpenAIProvider::with_responses_api(true)`. ChatGPT Codex supports native
Freeform transport through its Responses endpoint by default. Unsupported
providers reject native Freeform specs or history with
`MotosanError::UnsupportedFeature` before network I/O.
```

with:

```
### Provider support

ChatGPT Codex supports native Freeform transport through its Responses
endpoint by default. OpenAI supports the native API **only** when the caller
opts in — Rust `ClientBuilder::openai_responses_api(true)` /
`OpenAIProvider::with_responses_api(true)`, Python
`Client(openai_responses_api=True)` / `Client.openai(openai_responses_api=True)`
/ `OpenAIProvider(responses_api=True)`, TypeScript
`ClientBuilder.openaiResponsesApi(true)` /
`OpenAIProvider.withResponsesApi(true)`. TypeScript's pre-existing
`withResponsesFallback` is a 404 recovery path and is **not** the native
opt-in; the two MUST stay distinguishable.

The ChatGPT Codex body overrides the caller: `store=false`,
`include=["reasoning.encrypted_content"]`, `parallel_tool_calls=true`, and
`tool_choice="auto"` **regardless of what the caller passed**. When a
reasoning effort resolves — per-request `provider_options["reasoning_effort"]`
first, provider default second, omitted if neither — the body carries
`reasoning = {"effort": <value>, "summary": "auto"}` and any top-level
`reasoning_effort` key MUST be removed, because the `provider_options`
shallow merge will have injected the raw key onto the body.

Every other provider rejects native Freeform specs or history before network
I/O with the rejection error type above.
```

Edit 5 — replace the "### Stream termination (native)" paragraph (currently lines 211-222):

```
### Stream termination (native)

`ModelStreamDelta` streams follow the [Stream termination
contract](#stream-termination-contract): exactly one `Done { stop_reason }`
delta per successfully completed stream, emitted when the wire delivers a
`response.completed` or `response.incomplete` terminal event. When the byte
stream ends (EOF) without either terminal, the adapter yields
`MotosanError::IncompleteStream` with the standard payload
`<provider> ended without a terminal event` (provider names: `openai`,
`chatgpt-codex`). `collect_model_stream` propagates that error; its
`stop_reason` heuristic applies only to streams that did deliver a terminal.
```

with:

```
### Stream termination (native)

`ModelStreamDelta` streams follow the [Stream termination
contract](#stream-termination-contract): exactly one `Done { stop_reason }`
delta per successfully completed stream, emitted when the wire delivers a
`response.completed` or `response.incomplete` terminal event. When the byte
stream ends (EOF) without either terminal, the adapter yields or raises the
`IncompleteStream` error (Rust `MotosanError::IncompleteStream`, Python
`IncompleteStreamError`, TypeScript `IncompleteStreamError`) with the standard
payload `<provider> ended without a terminal event`. The provider names on the
native path are exactly `openai` and `chatgpt-codex` — note the hyphen, which
differs from the underscore the legacy Python `chatgpt_codex` adapter uses.

Six further rules are contract, not implementation detail, and every SDK's
collector (`collect_model_stream` / `collectModelStream`) MUST reproduce them:

- `ToolCallDone` is **authoritative**. Accumulated `FunctionArguments` /
  `FreeformInput` deltas are display bookkeeping and MUST NOT be lowered into
  the returned call.
- Freeform `input` survives byte-for-byte: never parsed as JSON, never lowered
  into function-call `arguments`.
- `Usage` **replaces** rather than merges.
- `ThinkingDone` wins over accumulated thinking deltas, and an explicitly
  empty `ThinkingDone` payload resolves to no thinking at all.
- Pending deltas drain before a stored stream error surfaces.
- A read-idle timeout wraps the native stream on HTTP providers.

The collector propagates an `IncompleteStream` error; its `stop_reason`
heuristic applies only to streams that did deliver a terminal.
```

- [ ] **Step 4: Run tests**

Run: `cd /path/to/worktree && bash -c '<the script from Step 1>' && python3 scripts/check-versions.py && treefmt --fail-on-change`
Expected: PASS — `spec widening OK` printed, `check-versions.py` clean, `treefmt` reports no change.

- [ ] **Step 5: Commit**

```bash
git switch -c docs/freeform-spec-widen origin/main
git add specs/types.md
git commit -m "feat: widen the native model API spec to a cross-SDK contract (#270)" \
           -m "Co-Authored-By: Claude Opus 5 (1M context) <noreply@anthropic.com>"
git push -u origin docs/freeform-spec-widen
test "$(git ls-remote origin refs/heads/docs/freeform-spec-widen | cut -f1)" = "$(git rev-parse HEAD)"
gh pr create --base main --head docs/freeform-spec-widen \
  --title "feat: widen the native model API spec to a cross-SDK contract (#270)" \
  --body "PR S of #270. Widens the normative contract only; deliberately does not claim Python/TypeScript ship the native API — the REL PR rewrites the implementation-status line."
```

---

### Task 2: `UnsupportedFeatureError` in `error.py`

**Files:**
- Modify: `sdks/python/motosan_ai/error.py:61` (append after `StreamReadTimeoutError`)
- Test: `sdks/python/tests/test_unsupported_feature_error.py` (create)

**Interfaces:**
- Consumes: `InvalidRequestError`, `MotosanError` (existing, `error.py:1-25`).
- Produces: `motosan_ai.error.UnsupportedFeatureError` — raised by `validate_model_request` (Task 9), `OpenAIProvider.model_chat` / `model_stream` (Task 13), and `Client._dispatch_model_chat` / `model_stream_with` (Task 14).

- [ ] **Step 1: Write the failing test**

Create `sdks/python/tests/test_unsupported_feature_error.py`:

```python
from __future__ import annotations

import pytest

from motosan_ai.error import InvalidRequestError, MotosanError, UnsupportedFeatureError


def test_unsupported_feature_error_subclasses_invalid_request_error():
    assert issubclass(UnsupportedFeatureError, InvalidRequestError)
    assert issubclass(UnsupportedFeatureError, MotosanError)


def test_existing_invalid_request_handlers_still_catch_it():
    with pytest.raises(InvalidRequestError):
        raise UnsupportedFeatureError("provider does not support native freeform tools")


def test_callers_can_distinguish_the_subclass():
    with pytest.raises(UnsupportedFeatureError, match="freeform"):
        raise UnsupportedFeatureError("provider does not support native freeform tools")


def test_carries_the_motosan_error_metadata_fields():
    err = UnsupportedFeatureError("nope")
    assert err.status_code is None
    assert err.retry_after is None
    assert err.request_id is None
    assert str(err) == "nope"
```

- [ ] **Step 2: Run it to verify it fails**

Run: `cd sdks/python && uv run pytest tests/test_unsupported_feature_error.py -q`
Expected: FAIL with `ImportError: cannot import name 'UnsupportedFeatureError' from 'motosan_ai.error'` — collection error, 0 tests run.

- [ ] **Step 3: Implement**

Append to `sdks/python/motosan_ai/error.py`:

```python


class UnsupportedFeatureError(InvalidRequestError):
    """The provider cannot serve a feature the request asked for.

    Raised before any network I/O: native Freeform tool specs or history on a
    provider that does not support them, image/document content on a provider
    that does not accept it, or a native model request on a provider with no
    native path.

    Deliberately subclasses InvalidRequestError as a migration softener:
    existing ``except InvalidRequestError`` handlers keep working, while
    callers that must distinguish match this subclass. Non-retryable by
    inheritance. Mirrors Rust ``MotosanError::UnsupportedFeature`` and
    TypeScript ``UnsupportedFeatureError``.
    """
```

- [ ] **Step 4: Run tests**

Run: `cd sdks/python && uv run pytest tests/test_unsupported_feature_error.py -q && uv run ruff check motosan_ai/ && uv run mypy motosan_ai/`
Expected: PASS — 4 passed, ruff clean, mypy `Success: no issues found`.

- [ ] **Step 5: Commit**

```bash
git switch -c feat/freeform-python-types origin/main
git add sdks/python/motosan_ai/error.py sdks/python/tests/test_unsupported_feature_error.py
git commit -m "feat: add UnsupportedFeatureError to the Python SDK (#270)" \
           -m "Co-Authored-By: Claude Opus 5 (1M context) <noreply@anthropic.com>"
```

---

### Task 3: Native model value types in `types.py`

**Files:**
- Modify: `sdks/python/motosan_ai/types.py:506` (append after `ChatRequestBuilder.build`)
- Test: `sdks/python/tests/test_native_types.py` (create)

**Interfaces:**
- Consumes: existing `types.py` symbols — `Tool` (line 228), `Message` (121), `SystemBlock` (207), `ToolChoice` (236), `Usage` (318), `StopReason` (15), `Role` (8).
- Produces, all importable from `motosan_ai.types`: `ImageDetail`, `FreeformToolFormat`, `FreeformTool`, `ModelToolSpecFunction`, `ModelToolSpecFreeform`, `ModelToolSpec`, `ModelToolCallFunction`, `ModelToolCallFreeform`, `ModelToolCall`, `FunctionCallOutputInputText`, `FunctionCallOutputInputImage`, `FunctionCallOutputEncryptedContent`, `FunctionCallOutputContentItem`, `FunctionCallOutputText`, `FunctionCallOutputContent`, `FunctionCallOutputPayload`, `ModelToolOutputFunction`, `ModelToolOutputCustom`, `ModelToolOutput`, `ModelContextMessage`, `ModelContextToolCall`, `ModelContextToolOutput`, `ModelContextItem`, `ModelChatRequest`, `ModelChatResponse`, `ModelStreamText`, `ModelStreamThinkingDelta`, `ModelStreamThinkingDone`, `ModelStreamFunctionArguments`, `ModelStreamFreeformInput`, `ModelStreamToolCallDone`, `ModelStreamUsage`, `ModelStreamDone`, `ModelStreamDelta`.

**Decision context (D2, D3):** variant dataclasses behind a union alias, discriminated by `isinstance`, following the `McpToolConfig` precedent at `types.py:288-314`. Frozen for value types, non-frozen for `ModelChatRequest` / `ModelChatResponse`. **No** `thinking`, `mcp_servers`, or `mcp_tool_configs` on `ModelChatRequest` — those are Rust's reject-only fields and are deliberately omitted (D3). **No wire encoding in this module** — every `type` / `call_id` / `parameters` key lives in Task 5's codec.

- [ ] **Step 1: Write the failing test**

Create `sdks/python/tests/test_native_types.py`:

```python
from __future__ import annotations

import dataclasses

from motosan_ai.types import (
    FreeformTool,
    FreeformToolFormat,
    FunctionCallOutputContent,
    FunctionCallOutputEncryptedContent,
    FunctionCallOutputInputImage,
    FunctionCallOutputInputText,
    FunctionCallOutputText,
    ImageDetail,
    Message,
    ModelChatRequest,
    ModelChatResponse,
    ModelContextMessage,
    ModelContextToolCall,
    ModelContextToolOutput,
    ModelStreamDone,
    ModelStreamFreeformInput,
    ModelStreamFunctionArguments,
    ModelStreamText,
    ModelStreamThinkingDelta,
    ModelStreamThinkingDone,
    ModelStreamToolCallDone,
    ModelStreamUsage,
    ModelToolCallFreeform,
    ModelToolCallFunction,
    ModelToolOutputCustom,
    ModelToolOutputFunction,
    ModelToolSpecFreeform,
    ModelToolSpecFunction,
    Role,
    StopReason,
    Tool,
    Usage,
)


def grammar_fixture() -> FreeformTool:
    return FreeformTool(
        name="exec",
        description="Run JavaScript",
        format=FreeformToolFormat(
            type="grammar", syntax="lark", definition="start: source"
        ),
    )


def test_freeform_format_is_mandatory_and_frozen():
    tool = grammar_fixture()
    assert tool.format.type == "grammar"
    assert tool.format.syntax == "lark"
    assert tool.format.definition == "start: source"
    # `format` has no default: constructing without it is a TypeError.
    try:
        FreeformTool(name="exec", description="Run JavaScript")  # type: ignore[call-arg]
    except TypeError as exc:
        assert "format" in str(exc)
    else:  # pragma: no cover - the type must not gain a default
        raise AssertionError("FreeformTool.format must be mandatory")
    assert dataclasses.is_dataclass(tool)
    assert tool == grammar_fixture()


def test_freeform_call_preserves_raw_input_verbatim():
    raw = "const x = {a: 1};\nconsole.log(`raw ${x.a}`);\n"
    call = ModelToolCallFreeform(id="call_js", name="exec", input=raw)
    assert call.input == raw
    assert call.input.encode() == raw.encode()
    assert call == ModelToolCallFreeform(id="call_js", name="exec", input=raw)


def test_function_call_and_freeform_call_are_distinct_variants():
    fn = ModelToolCallFunction(id="call_fn", name="sum", arguments='{"a":1}')
    ff = ModelToolCallFreeform(id="call_js", name="exec", input="console.log(1);")
    assert isinstance(fn, ModelToolCallFunction)
    assert not isinstance(fn, ModelToolCallFreeform)
    assert isinstance(ff, ModelToolCallFreeform)
    assert fn.arguments == '{"a":1}'
    assert ff.input == "console.log(1);"


def test_custom_output_carries_optional_call_identity():
    output = ModelToolOutputCustom(
        call_id="call_js", output=FunctionCallOutputText(text="stdout: 42"), name="exec"
    )
    assert output.call_id == "call_js"
    assert output.name == "exec"
    assert output.output == FunctionCallOutputText(text="stdout: 42")
    assert ModelToolOutputCustom(call_id="c", output=FunctionCallOutputText(text="")).name is None
    assert ModelToolOutputFunction(
        call_id="call_fn", output=FunctionCallOutputText(text='{"ok":true}')
    ).call_id == "call_fn"


def test_function_call_output_content_items():
    content = FunctionCallOutputContent(
        items=[
            FunctionCallOutputInputText(text="see this"),
            FunctionCallOutputInputImage(image_url="https://x.test/i.png", detail=ImageDetail.high),
            FunctionCallOutputEncryptedContent(encrypted_content="enc"),
        ]
    )
    assert len(content.items) == 3
    assert content.items[1].detail is ImageDetail.high
    assert FunctionCallOutputInputImage(image_url="u").detail is None
    assert ImageDetail.auto == "auto"
    assert ImageDetail.original == "original"


def test_native_context_preserves_mixed_item_order():
    request = ModelChatRequest(
        model="gpt-5.5-codex",
        context=[
            ModelContextMessage(message=Message.user("run it")),
            ModelContextToolCall(
                call=ModelToolCallFreeform(
                    id="call_js", name="exec", input="console.log(1);"
                )
            ),
            ModelContextToolOutput(
                output=ModelToolOutputCustom(
                    call_id="call_js",
                    output=FunctionCallOutputText(text="1\n"),
                    name="exec",
                )
            ),
        ],
        tool_specs=[ModelToolSpecFreeform(tool=grammar_fixture())],
    )

    assert request.model == "gpt-5.5-codex"
    assert len(request.context) == 3
    assert isinstance(request.context[0], ModelContextMessage)
    assert request.context[0].message.role == Role.user
    assert isinstance(request.context[1], ModelContextToolCall)
    assert isinstance(request.context[1].call, ModelToolCallFreeform)
    assert isinstance(request.context[2], ModelContextToolOutput)
    assert isinstance(request.context[2].output, ModelToolOutputCustom)


def test_model_chat_request_omits_the_reject_only_fields():
    # D3: thinking / mcp_servers / mcp_tool_configs exist in Rust only so
    # validation can reject them. Python omits them outright.
    names = {f.name for f in dataclasses.fields(ModelChatRequest)}
    assert "thinking" not in names
    assert "mcp_servers" not in names
    assert "mcp_tool_configs" not in names
    assert names == {
        "context",
        "tool_specs",
        "model",
        "system",
        "system_blocks",
        "system_cache",
        "temperature",
        "max_tokens",
        "tool_choice",
        "provider_options",
        "stop_sequences",
    }


def test_model_chat_request_defaults_are_independent():
    a = ModelChatRequest()
    b = ModelChatRequest()
    a.context.append(ModelContextMessage(message=Message.user("hi")))
    a.tool_specs.append(ModelToolSpecFunction(tool=Tool(name="sum")))
    assert b.context == []
    assert b.tool_specs == []
    assert a.model is None
    assert a.system_cache is False


def test_native_response_carries_freeform_calls_and_thinking():
    response = ModelChatResponse(
        content="answer",
        thinking="private reasoning",
        tool_calls=[
            ModelToolCallFreeform(id="call_js", name="exec", input="console.log(1);")
        ],
        model="gpt-5.5-codex",
        usage=Usage(input_tokens=10, output_tokens=5),
        stop_reason=StopReason.tool_use,
    )
    assert response.content == "answer"
    assert response.thinking == "private reasoning"
    assert len(response.tool_calls) == 1
    assert isinstance(response.tool_calls[0], ModelToolCallFreeform)
    assert response.session_id is None
    # Non-frozen: providers backfill `model` after collecting a stream.
    response.model = "backfilled"
    assert response.model == "backfilled"


def test_model_chat_response_defaults():
    response = ModelChatResponse()
    assert response.content == ""
    assert response.thinking is None
    assert response.tool_calls == []
    assert response.model == ""
    assert response.usage == Usage(0, 0)
    assert response.stop_reason == StopReason.end_turn


def test_model_stream_delta_variants():
    deltas = [
        ModelStreamText(delta="hi"),
        ModelStreamThinkingDelta(delta="think"),
        ModelStreamThinkingDone(thinking="think hard"),
        ModelStreamFunctionArguments(call_id="call_fn", delta='{"a"'),
        ModelStreamFreeformInput(call_id="call_js", delta="console."),
        ModelStreamToolCallDone(
            call=ModelToolCallFreeform(id="call_js", name="exec", input="console.log(1);")
        ),
        ModelStreamUsage(usage=Usage(2, 3)),
        ModelStreamDone(stop_reason=StopReason.tool_use),
    ]
    assert deltas[0].delta == "hi"
    assert deltas[2].thinking == "think hard"
    assert deltas[3].call_id == "call_fn"
    assert deltas[4].delta == "console."
    assert isinstance(deltas[5].call, ModelToolCallFreeform)
    assert deltas[6].usage.output_tokens == 3
    assert deltas[7].stop_reason == StopReason.tool_use
    assert len({type(d) for d in deltas}) == 8
```

- [ ] **Step 2: Run it to verify it fails**

Run: `cd sdks/python && uv run pytest tests/test_native_types.py -q`
Expected: FAIL with `ImportError: cannot import name 'FreeformTool' from 'motosan_ai.types'` — collection error, 0 tests run.

- [ ] **Step 3: Implement**

Append to `sdks/python/motosan_ai/types.py`:

```python


# ---------------------------------------------------------------------------
# Native model API — specs/types.md § Native Model API.
#
# Value types only. Every wire key ("type", "call_id", "parameters",
# "max_output_tokens", ...) lives in motosan_ai/providers/responses.py, never
# here. Variants are plain dataclasses behind a union alias and are
# discriminated with isinstance, following the McpToolConfig precedent above.
# ---------------------------------------------------------------------------


class ImageDetail(StrEnum):
    auto = "auto"
    low = "low"
    high = "high"
    original = "original"


@dataclass(frozen=True)
class FreeformToolFormat:
    type: str
    syntax: str
    definition: str


@dataclass(frozen=True)
class FreeformTool:
    """A Freeform ("custom") tool. ``format`` is mandatory.

    The wire tag ``"custom"`` is injected by the codec — it is never stored.
    """

    name: str
    description: str
    format: FreeformToolFormat


@dataclass(frozen=True)
class ModelToolSpecFunction:
    tool: Tool


@dataclass(frozen=True)
class ModelToolSpecFreeform:
    tool: FreeformTool


ModelToolSpec = ModelToolSpecFunction | ModelToolSpecFreeform


@dataclass(frozen=True)
class ModelToolCallFunction:
    """A function tool call. ``arguments`` is a JSON string."""

    id: str
    name: str
    arguments: str


@dataclass(frozen=True)
class ModelToolCallFreeform:
    """A Freeform tool call.

    ``input`` is raw model text (JavaScript, a DSL, ...). It MUST be preserved
    byte-for-byte: never parsed as JSON, never lowered into ``arguments``.
    """

    id: str
    name: str
    input: str


ModelToolCall = ModelToolCallFunction | ModelToolCallFreeform


@dataclass(frozen=True)
class FunctionCallOutputInputText:
    text: str


@dataclass(frozen=True)
class FunctionCallOutputInputImage:
    image_url: str
    detail: ImageDetail | None = None


@dataclass(frozen=True)
class FunctionCallOutputEncryptedContent:
    encrypted_content: str


FunctionCallOutputContentItem = (
    FunctionCallOutputInputText
    | FunctionCallOutputInputImage
    | FunctionCallOutputEncryptedContent
)


@dataclass(frozen=True)
class FunctionCallOutputText:
    text: str


@dataclass(frozen=True)
class FunctionCallOutputContent:
    items: list[FunctionCallOutputContentItem]


FunctionCallOutputPayload = FunctionCallOutputText | FunctionCallOutputContent


@dataclass(frozen=True)
class ModelToolOutputFunction:
    call_id: str
    output: FunctionCallOutputPayload


@dataclass(frozen=True)
class ModelToolOutputCustom:
    call_id: str
    output: FunctionCallOutputPayload
    name: str | None = None


ModelToolOutput = ModelToolOutputFunction | ModelToolOutputCustom


@dataclass(frozen=True)
class ModelContextMessage:
    message: Message


@dataclass(frozen=True)
class ModelContextToolCall:
    call: ModelToolCall


@dataclass(frozen=True)
class ModelContextToolOutput:
    output: ModelToolOutput


ModelContextItem = ModelContextMessage | ModelContextToolCall | ModelContextToolOutput


@dataclass
class ModelChatRequest:
    """A native model request.

    ``context`` preserves mixed message / tool-call / tool-output order, which
    is what makes byte-exact replay of Freeform inputs possible in multi-turn
    histories.

    Deliberately carries no ``thinking`` / ``mcp_servers`` / ``mcp_tool_configs``
    (milestone D3): native requests support neither extended thinking nor MCP.
    Provider-specific reasoning controls go through ``provider_options``.
    """

    context: list[ModelContextItem] = field(default_factory=list)
    tool_specs: list[ModelToolSpec] = field(default_factory=list)
    model: str | None = None
    system: str | None = None
    system_blocks: list[SystemBlock] | None = None
    system_cache: bool = False
    temperature: float | None = None
    max_tokens: int | None = None
    tool_choice: ToolChoice | None = None
    provider_options: dict[str, Any] | None = None
    stop_sequences: list[str] | None = None

    @classmethod
    def builder(cls) -> ModelChatRequestBuilder:
        return ModelChatRequestBuilder()


@dataclass
class ModelChatResponse:
    content: str = ""
    thinking: str | None = None
    tool_calls: list[ModelToolCall] = field(default_factory=list)
    model: str = ""
    usage: Usage = field(default_factory=lambda: Usage(0, 0))
    stop_reason: StopReason = StopReason.end_turn
    session_id: str | None = None


@dataclass(frozen=True)
class ModelStreamText:
    delta: str


@dataclass(frozen=True)
class ModelStreamThinkingDelta:
    delta: str


@dataclass(frozen=True)
class ModelStreamThinkingDone:
    thinking: str


@dataclass(frozen=True)
class ModelStreamFunctionArguments:
    call_id: str
    delta: str


@dataclass(frozen=True)
class ModelStreamFreeformInput:
    call_id: str
    delta: str


@dataclass(frozen=True)
class ModelStreamToolCallDone:
    """Authoritative completed call. Collectors discard accumulated deltas."""

    call: ModelToolCall


@dataclass(frozen=True)
class ModelStreamUsage:
    usage: Usage


@dataclass(frozen=True)
class ModelStreamDone:
    stop_reason: StopReason


ModelStreamDelta = (
    ModelStreamText
    | ModelStreamThinkingDelta
    | ModelStreamThinkingDone
    | ModelStreamFunctionArguments
    | ModelStreamFreeformInput
    | ModelStreamToolCallDone
    | ModelStreamUsage
    | ModelStreamDone
)
```

- [ ] **Step 4: Run tests**

Run: `cd sdks/python && uv run pytest tests/test_native_types.py -q && uv run ruff check motosan_ai/ && uv run ruff format --check motosan_ai/ tests/ && uv run mypy motosan_ai/`
Expected: PASS — 10 passed, ruff clean, format check clean, mypy `Success: no issues found`.

- [ ] **Step 5: Commit**

```bash
git add sdks/python/motosan_ai/types.py sdks/python/tests/test_native_types.py
git commit -m "feat: add native model value types to the Python SDK (#270)" \
           -m "Co-Authored-By: Claude Opus 5 (1M context) <noreply@anthropic.com>"
```

---

### Task 4: `ModelChatRequestBuilder` in `types.py`

**Files:**
- Modify: `sdks/python/motosan_ai/types.py` (append after `ModelStreamDelta` from Task 3)
- Test: `sdks/python/tests/test_native_types.py` (extend the file created in Task 3)

**Interfaces:**
- Consumes: `ModelChatRequest`, `ModelContextItem`, `ModelContextMessage`, `ModelContextToolCall`, `ModelContextToolOutput`, `ModelToolSpec`, `ModelToolCall`, `ModelToolOutput`, `SystemBlock`, `ToolChoice`, `Message` (Task 3 / existing).
- Produces: `ModelChatRequestBuilder`, reached through `ModelChatRequest.builder()`. Used by every provider and conformance test from Task 12 onward.

**Decision context (D2):** a separate fluent builder class with a `builder()` classmethod, mirroring `ChatRequestBuilder` at `types.py:371-506`. Every method returns `self`. Three convenience methods — `message`, `tool_call`, `tool_output` — wrap the raw context variants so callers rarely spell `ModelContextMessage(...)` by hand.

- [ ] **Step 1: Write the failing test**

Append to `sdks/python/tests/test_native_types.py`:

```python


def test_builder_populates_every_field():
    request = (
        ModelChatRequest.builder()
        .model("gpt-5.5-codex")
        .system("  be terse  ")
        .temperature(0.25)
        .max_tokens(512)
        .tool_choice(ToolChoice.required())
        .provider_options({"reasoning_effort": "high"})
        .stop("END")
        .stop("STOP")
        .tool_spec(ModelToolSpecFreeform(tool=grammar_fixture()))
        .message(Message.user("run js"))
        .tool_call(ModelToolCallFreeform(id="call_js", name="exec", input="console.log(1);"))
        .tool_output(
            ModelToolOutputCustom(
                call_id="call_js", output=FunctionCallOutputText(text="1\n"), name="exec"
            )
        )
        .build()
    )

    assert isinstance(request, ModelChatRequest)
    assert request.model == "gpt-5.5-codex"
    assert request.system == "  be terse  "  # trimming happens in the codec, not here
    assert request.temperature == 0.25
    assert request.max_tokens == 512
    assert request.tool_choice == ToolChoice.required()
    assert request.provider_options == {"reasoning_effort": "high"}
    assert request.stop_sequences == ["END", "STOP"]
    assert len(request.tool_specs) == 1
    assert isinstance(request.tool_specs[0], ModelToolSpecFreeform)
    assert [type(item) for item in request.context] == [
        ModelContextMessage,
        ModelContextToolCall,
        ModelContextToolOutput,
    ]


def test_builder_bulk_setters_replace_and_copy():
    items = [ModelContextMessage(message=Message.user("a"))]
    specs = [ModelToolSpecFunction(tool=Tool(name="sum"))]
    seqs = ["X"]
    blocks = [SystemBlock.new("sys")]

    request = (
        ModelChatRequest.builder()
        .context(items)
        .tool_specs(specs)
        .stop_sequences(seqs)
        .system_blocks(blocks)
        .build()
    )

    items.append(ModelContextMessage(message=Message.user("b")))
    specs.append(ModelToolSpecFunction(tool=Tool(name="other")))
    seqs.append("Y")
    blocks.append(SystemBlock.new("more"))

    assert len(request.context) == 1
    assert len(request.tool_specs) == 1
    assert request.stop_sequences == ["X"]
    assert request.system_blocks is not None
    assert len(request.system_blocks) == 1


def test_builder_context_item_and_system_cached_and_system_block():
    request = (
        ModelChatRequest.builder()
        .context_item(ModelContextMessage(message=Message.system("sys msg")))
        .system_cached("cached system")
        .system_block(SystemBlock.cached("block one"))
        .build()
    )
    assert len(request.context) == 1
    assert request.system == "cached system"
    assert request.system_cache is True
    assert request.system_blocks is not None
    assert request.system_blocks[0].cache_control is True


def test_builder_defaults_are_empty():
    request = ModelChatRequest.builder().build()
    assert request.context == []
    assert request.tool_specs == []
    assert request.model is None
    assert request.system is None
    assert request.system_blocks is None
    assert request.system_cache is False
    assert request.temperature is None
    assert request.max_tokens is None
    assert request.tool_choice is None
    assert request.provider_options is None
    assert request.stop_sequences is None
```

Add `SystemBlock` and `ToolChoice` to the existing `from motosan_ai.types import (...)` block at the top of `tests/test_native_types.py`, keeping the list alphabetically sorted (ruff `I001` enforces it on `tests/` through `ruff format --check`, and the repo keeps them tidy regardless).

- [ ] **Step 2: Run it to verify it fails**

Run: `cd sdks/python && uv run pytest tests/test_native_types.py -q`
Expected: FAIL with `AttributeError: type object 'ModelChatRequest' has no attribute 'builder'` — 4 failed, 10 passed.

- [ ] **Step 3: Implement**

Append to `sdks/python/motosan_ai/types.py`:

```python


class ModelChatRequestBuilder:
    """Fluent builder for ModelChatRequest.

    Mirrors ChatRequestBuilder. No ``thinking`` / ``mcp_*`` methods: the
    native request carries no such fields (milestone D3). Whitespace is NOT
    trimmed here — the codec trims when it assembles ``instructions``.
    """

    def __init__(self) -> None:
        self._context: list[ModelContextItem] = []
        self._tool_specs: list[ModelToolSpec] = []
        self._model: str | None = None
        self._system: str | None = None
        self._system_blocks: list[SystemBlock] | None = None
        self._system_cache: bool = False
        self._temperature: float | None = None
        self._max_tokens: int | None = None
        self._tool_choice: ToolChoice | None = None
        self._provider_options: dict[str, Any] | None = None
        self._stop_sequences: list[str] | None = None

    def context(self, context: list[ModelContextItem]) -> ModelChatRequestBuilder:
        self._context = list(context)
        return self

    def context_item(self, item: ModelContextItem) -> ModelChatRequestBuilder:
        self._context.append(item)
        return self

    def message(self, message: Message) -> ModelChatRequestBuilder:
        self._context.append(ModelContextMessage(message=message))
        return self

    def tool_call(self, call: ModelToolCall) -> ModelChatRequestBuilder:
        self._context.append(ModelContextToolCall(call=call))
        return self

    def tool_output(self, output: ModelToolOutput) -> ModelChatRequestBuilder:
        self._context.append(ModelContextToolOutput(output=output))
        return self

    def tool_specs(self, tool_specs: list[ModelToolSpec]) -> ModelChatRequestBuilder:
        self._tool_specs = list(tool_specs)
        return self

    def tool_spec(self, tool_spec: ModelToolSpec) -> ModelChatRequestBuilder:
        self._tool_specs.append(tool_spec)
        return self

    def model(self, model: str) -> ModelChatRequestBuilder:
        self._model = model
        return self

    def system(self, system: str) -> ModelChatRequestBuilder:
        self._system = system
        return self

    def system_cached(self, system: str) -> ModelChatRequestBuilder:
        self._system = system
        self._system_cache = True
        return self

    def system_block(self, block: SystemBlock) -> ModelChatRequestBuilder:
        if self._system_blocks is None:
            self._system_blocks = []
        self._system_blocks.append(block)
        return self

    def system_blocks(self, blocks: list[SystemBlock]) -> ModelChatRequestBuilder:
        self._system_blocks = list(blocks)
        return self

    def temperature(self, temperature: float) -> ModelChatRequestBuilder:
        self._temperature = temperature
        return self

    def max_tokens(self, max_tokens: int) -> ModelChatRequestBuilder:
        self._max_tokens = max_tokens
        return self

    def tool_choice(self, choice: ToolChoice) -> ModelChatRequestBuilder:
        self._tool_choice = choice
        return self

    def provider_options(self, options: dict[str, Any]) -> ModelChatRequestBuilder:
        self._provider_options = dict(options)
        return self

    def stop(self, sequence: str) -> ModelChatRequestBuilder:
        if self._stop_sequences is None:
            self._stop_sequences = []
        self._stop_sequences.append(sequence)
        return self

    def stop_sequences(self, sequences: list[str]) -> ModelChatRequestBuilder:
        self._stop_sequences = list(sequences)
        return self

    def build(self) -> ModelChatRequest:
        return ModelChatRequest(
            context=self._context,
            tool_specs=self._tool_specs,
            model=self._model,
            system=self._system,
            system_blocks=self._system_blocks,
            system_cache=self._system_cache,
            temperature=self._temperature,
            max_tokens=self._max_tokens,
            tool_choice=self._tool_choice,
            provider_options=self._provider_options,
            stop_sequences=self._stop_sequences,
        )
```

- [ ] **Step 4: Run tests**

Run: `cd sdks/python && uv run pytest tests/test_native_types.py -q && uv run ruff check motosan_ai/ && uv run ruff format --check motosan_ai/ tests/ && uv run mypy motosan_ai/`
Expected: PASS — 14 passed, ruff clean, format check clean, mypy `Success: no issues found`.

- [ ] **Step 5: Commit**

```bash
git add sdks/python/motosan_ai/types.py sdks/python/tests/test_native_types.py
git commit -m "feat: add ModelChatRequestBuilder to the Python SDK (#270)" \
           -m "Co-Authored-By: Claude Opus 5 (1M context) <noreply@anthropic.com>"
```

---

### Task 5: Responses codec — encoders

**Files:**
- Create: `sdks/python/motosan_ai/providers/responses.py`
- Test: `sdks/python/tests/test_responses_codec.py` (create)

**Interfaces:**
- Consumes: every type from Task 3, plus existing `Message`, `Role`, `TextBlock`, `ImageBlock`, `ImageSourceBase64`, `ImageSourceUrl`, `ToolChoice`.
- Produces: `encode_freeform_tool`, `encode_tool_spec`, `encode_tools`, `encode_tool_call`, `encode_function_call_output_content_item`, `encode_function_call_output_payload`, `encode_tool_output`, `tool_output_to_dict`, `encode_user_content`, `encode_message`, `encode_context_item`, `encode_input`, `encode_tool_choice`.

**Traps this task must reproduce exactly (source: Rust `sdks/rust/src/providers/responses.rs` and `sdks/rust/tests/core_types.rs`):**
1. Wire keys differ from field names: `ModelToolCall.id` → `call_id`; `Tool.input_schema` → `parameters`. `FreeformTool` gains a `type: "custom"` key it never stores.
2. `encode_tool_output` **drops `name`** on custom outputs. Rust's codec emits only `type` / `call_id` / `output`, and `responses_codec_encodes_function_and_custom_outputs` asserts `input[1].get("name").is_none()`. The identity-preserving variant is the separate `tool_output_to_dict`, which mirrors Rust's `impl Serialize for ModelToolOutput` and is what round-trips with `decode_tool_output` (Task 6).
3. `encode_message` returns a **list**, because one assistant message can expand into a text item plus N `function_call` items, and a system message expands into **nothing** (it is hoisted into `instructions` by Task 7).
4. Freeform `input` is copied verbatim. Never `json.loads` it, never write it to an `arguments` key.

- [ ] **Step 1: Write the failing test**

Create `sdks/python/tests/test_responses_codec.py`:

```python
from __future__ import annotations

from motosan_ai.providers.responses import (
    encode_function_call_output_payload,
    encode_input,
    encode_message,
    encode_tool_call,
    encode_tool_choice,
    encode_tool_output,
    encode_tools,
    encode_user_content,
    tool_output_to_dict,
)
from motosan_ai.types import (
    FreeformTool,
    FreeformToolFormat,
    FunctionCallOutputContent,
    FunctionCallOutputEncryptedContent,
    FunctionCallOutputInputImage,
    FunctionCallOutputInputText,
    FunctionCallOutputText,
    ImageDetail,
    Message,
    ModelContextMessage,
    ModelContextToolCall,
    ModelContextToolOutput,
    ModelToolCallFreeform,
    ModelToolCallFunction,
    ModelToolOutputCustom,
    ModelToolOutputFunction,
    ModelToolSpecFreeform,
    ModelToolSpecFunction,
    Tool,
    ToolCall,
    ToolChoice,
)


def grammar_fixture() -> FreeformTool:
    return FreeformTool(
        name="exec",
        description="Run JavaScript",
        format=FreeformToolFormat(type="grammar", syntax="lark", definition="start: source"),
    )


def test_encodes_function_and_custom_tools():
    tools = encode_tools(
        [
            ModelToolSpecFunction(
                tool=Tool(
                    name="sum",
                    description="Add numbers",
                    input_schema={"type": "object", "properties": {"a": {"type": "number"}}},
                )
            ),
            ModelToolSpecFreeform(tool=grammar_fixture()),
        ]
    )

    assert tools[0]["type"] == "function"
    assert tools[0]["name"] == "sum"
    assert tools[0]["description"] == "Add numbers"
    # `input_schema` is spelled `parameters` on the wire.
    assert tools[0]["parameters"]["type"] == "object"
    assert "input_schema" not in tools[0]
    assert tools[1] == {
        "type": "custom",
        "name": "exec",
        "description": "Run JavaScript",
        "format": {"type": "grammar", "syntax": "lark", "definition": "start: source"},
    }


def test_encodes_tool_calls_with_call_id_key():
    fn = encode_tool_call(
        ModelToolCallFunction(id="call_fn", name="sum", arguments='{"a":1}')
    )
    assert fn == {
        "type": "function_call",
        "call_id": "call_fn",
        "name": "sum",
        "arguments": '{"a":1}',
    }
    assert "id" not in fn

    raw = '{"this":"looks like json"}\nconsole.log(\'but is JS\');'
    ff = encode_tool_call(ModelToolCallFreeform(id="call_js", name="exec", input=raw))
    assert ff == {
        "type": "custom_tool_call",
        "call_id": "call_js",
        "name": "exec",
        "input": raw,
    }
    assert ff["input"].encode() == raw.encode()
    assert "arguments" not in ff


def test_encodes_function_and_custom_outputs_and_drops_custom_name():
    encoded = encode_input(
        [
            ModelContextToolOutput(
                output=ModelToolOutputFunction(
                    call_id="call_fn", output=FunctionCallOutputText(text='{"ok":true}')
                )
            ),
            ModelContextToolOutput(
                output=ModelToolOutputCustom(
                    call_id="call_js",
                    output=FunctionCallOutputText(text="stdout"),
                    name="exec",
                )
            ),
        ]
    )

    assert encoded[0]["type"] == "function_call_output"
    assert encoded[0]["call_id"] == "call_fn"
    assert encoded[0]["output"] == '{"ok":true}'
    assert encoded[1]["type"] == "custom_tool_call_output"
    assert encoded[1]["call_id"] == "call_js"
    # The wire encoder deliberately drops `name` (Rust codec parity).
    assert "name" not in encoded[1]
    assert encoded[1]["output"] == "stdout"


def test_tool_output_to_dict_keeps_call_identity():
    output = ModelToolOutputCustom(
        call_id="call_js", output=FunctionCallOutputText(text="stdout: 42"), name="exec"
    )
    encoded = tool_output_to_dict(output)
    assert encoded["type"] == "custom_tool_call_output"
    assert encoded["call_id"] == "call_js"
    assert encoded["name"] == "exec"
    assert encoded["output"] == "stdout: 42"
    assert "name" not in tool_output_to_dict(
        ModelToolOutputCustom(call_id="c", output=FunctionCallOutputText(text=""))
    )
    assert "name" not in tool_output_to_dict(
        ModelToolOutputFunction(call_id="call_fn", output=FunctionCallOutputText(text="1"))
    )


def test_encodes_function_call_output_payload_shapes():
    assert encode_function_call_output_payload(FunctionCallOutputText(text="plain")) == "plain"
    assert encode_function_call_output_payload(
        FunctionCallOutputContent(
            items=[
                FunctionCallOutputInputText(text="hi"),
                FunctionCallOutputInputImage(image_url="https://x.test/i.png"),
                FunctionCallOutputInputImage(
                    image_url="https://x.test/j.png", detail=ImageDetail.low
                ),
                FunctionCallOutputEncryptedContent(encrypted_content="enc"),
            ]
        )
    ) == [
        {"type": "input_text", "text": "hi"},
        {"type": "input_image", "image_url": "https://x.test/i.png"},
        {"type": "input_image", "image_url": "https://x.test/j.png", "detail": "low"},
        {"type": "encrypted_content", "encrypted_content": "enc"},
    ]


def test_encodes_user_content_blocks_as_input_image():
    content = encode_user_content(Message.user_with_image("inspect", "abc123", "image/png"))
    assert content[0] == {"type": "input_text", "text": "inspect"}
    assert content[1] == {
        "type": "input_image",
        "image_url": "data:image/png;base64,abc123",
    }


def test_encodes_plain_user_message_and_document_only_message():
    assert encode_user_content(Message.user("hello")) == [
        {"type": "input_text", "text": "hello"}
    ]
    # Document blocks are not representable on the Responses wire; the encoder
    # falls back to the message's flat text rather than emitting nothing.
    pdf = Message.user_with_pdf_base64("read this", "abc")
    content = encode_user_content(pdf)
    assert content == [{"type": "input_text", "text": "read this"}]


def test_encode_message_expands_assistant_text_and_tool_calls():
    message = Message.assistant_with_tool_calls(
        "on it", [ToolCall(id="call_fn", name="sum", input={"a": 1})]
    )
    items = encode_message(message)
    assert items[0] == {
        "type": "message",
        "role": "assistant",
        "content": [{"type": "output_text", "text": "on it"}],
    }
    assert items[1]["type"] == "function_call"
    assert items[1]["call_id"] == "call_fn"
    assert items[1]["name"] == "sum"
    assert '"a"' in items[1]["arguments"]

    # No text -> no message item, only the call.
    only_call = encode_message(
        Message.assistant_with_tool_calls("", [ToolCall(id="c", name="n", input={})])
    )
    assert len(only_call) == 1
    assert only_call[0]["type"] == "function_call"


def test_encode_message_maps_tool_result_and_drops_system():
    assert encode_message(Message.system("be terse")) == []
    assert encode_message(Message.tool_result("call_fn", "42")) == [
        {"type": "function_call_output", "call_id": "call_fn", "output": "42"}
    ]
    # A tool message without a call id has nothing to attach to.
    assert encode_message(Message(role=Message.tool_result("x", "y").role, content="z")) == []


def test_encode_input_preserves_mixed_ordered_history_byte_exact():
    raw = '{"not":"function args"}\nvalue.not;\n'
    encoded = encode_input(
        [
            ModelContextMessage(message=Message.user("run js")),
            ModelContextToolCall(
                call=ModelToolCallFunction(id="call_fn", name="sum", arguments='{"a":1}')
            ),
            ModelContextToolOutput(
                output=ModelToolOutputFunction(
                    call_id="call_fn", output=FunctionCallOutputText(text="1")
                )
            ),
            ModelContextToolCall(
                call=ModelToolCallFreeform(id="call_js", name="exec", input=raw)
            ),
            ModelContextToolOutput(
                output=ModelToolOutputCustom(
                    call_id="call_js",
                    output=FunctionCallOutputText(text="function args"),
                    name="exec",
                )
            ),
        ]
    )

    assert [item["type"] for item in encoded] == [
        "message",
        "function_call",
        "function_call_output",
        "custom_tool_call",
        "custom_tool_call_output",
    ]
    assert encoded[3]["input"].encode() == raw.encode()
    assert "arguments" not in encoded[3]


def test_encode_tool_choice():
    assert encode_tool_choice(ToolChoice.auto()) == "auto"
    assert encode_tool_choice(ToolChoice.required()) == "required"
    assert encode_tool_choice(ToolChoice.none()) == "none"
    assert encode_tool_choice(ToolChoice.tool("run_js")) == {
        "type": "function",
        "name": "run_js",
    }
```

- [ ] **Step 2: Run it to verify it fails**

Run: `cd sdks/python && uv run pytest tests/test_responses_codec.py -q`
Expected: FAIL with `ModuleNotFoundError: No module named 'motosan_ai.providers.responses'` — collection error, 0 tests run.

- [ ] **Step 3: Implement**

Create `sdks/python/motosan_ai/providers/responses.py`:

```python
"""Shared OpenAI Responses codec for the native model API.

Port of Rust ``sdks/rust/src/providers/responses.rs``. Pure encoding and
decoding — no HTTP, no provider state. Consumed by
``motosan_ai/providers/openai.py`` (behind the Responses opt-in) and
``motosan_ai/providers/chatgpt_codex.py`` (native by default).

Normative contract: ``specs/types.md`` § Native Model API.
"""

from __future__ import annotations

import json
from collections.abc import Sequence
from typing import Any

from motosan_ai.types import (
    FreeformTool,
    FunctionCallOutputContent,
    FunctionCallOutputContentItem,
    FunctionCallOutputEncryptedContent,
    FunctionCallOutputInputImage,
    FunctionCallOutputInputText,
    FunctionCallOutputPayload,
    FunctionCallOutputText,
    ImageBlock,
    ImageSourceBase64,
    ImageSourceUrl,
    Message,
    ModelContextItem,
    ModelContextMessage,
    ModelContextToolCall,
    ModelToolCall,
    ModelToolCallFreeform,
    ModelToolCallFunction,
    ModelToolOutput,
    ModelToolOutputCustom,
    ModelToolOutputFunction,
    ModelToolSpec,
    ModelToolSpecFreeform,
    Role,
    TextBlock,
    ToolChoice,
)


def encode_freeform_tool(tool: FreeformTool) -> dict[str, Any]:
    """Wire shape for a Freeform tool spec.

    The ``type: "custom"`` key is injected here — ``FreeformTool`` never
    stores it.
    """
    return {
        "type": "custom",
        "name": tool.name,
        "description": tool.description,
        "format": {
            "type": tool.format.type,
            "syntax": tool.format.syntax,
            "definition": tool.format.definition,
        },
    }


def encode_tool_spec(spec: ModelToolSpec) -> dict[str, Any]:
    if isinstance(spec, ModelToolSpecFreeform):
        return encode_freeform_tool(spec.tool)
    return {
        "type": "function",
        "name": spec.tool.name,
        "description": spec.tool.description,
        # Wire key differs from the field name: input_schema -> parameters.
        "parameters": spec.tool.input_schema,
    }


def encode_tools(specs: Sequence[ModelToolSpec]) -> list[dict[str, Any]]:
    return [encode_tool_spec(spec) for spec in specs]


def encode_tool_call(call: ModelToolCall) -> dict[str, Any]:
    """Wire shape for a tool call. ``id`` is spelled ``call_id`` on the wire."""
    if isinstance(call, ModelToolCallFreeform):
        return {
            "type": "custom_tool_call",
            "call_id": call.id,
            "name": call.name,
            # Verbatim. Never parsed as JSON, never written to "arguments".
            "input": call.input,
        }
    return {
        "type": "function_call",
        "call_id": call.id,
        "name": call.name,
        "arguments": call.arguments,
    }


def encode_function_call_output_content_item(
    item: FunctionCallOutputContentItem,
) -> dict[str, Any]:
    if isinstance(item, FunctionCallOutputInputText):
        return {"type": "input_text", "text": item.text}
    if isinstance(item, FunctionCallOutputInputImage):
        encoded: dict[str, Any] = {"type": "input_image", "image_url": item.image_url}
        if item.detail is not None:
            encoded["detail"] = item.detail.value
        return encoded
    return {"type": "encrypted_content", "encrypted_content": item.encrypted_content}


def encode_function_call_output_payload(payload: FunctionCallOutputPayload) -> Any:
    """Text payloads encode to a bare string; content payloads to a list."""
    if isinstance(payload, FunctionCallOutputText):
        return payload.text
    return [encode_function_call_output_content_item(item) for item in payload.items]


def encode_tool_output(output: ModelToolOutput) -> dict[str, Any]:
    """Wire shape for a tool output item.

    TRAP: the custom arm deliberately DROPS ``name``. Rust's codec
    ``encode_tool_output`` emits only ``type`` / ``call_id`` / ``output``, and
    ``responses_codec_encodes_function_and_custom_outputs`` asserts the wire
    item has no ``name``. Use ``tool_output_to_dict`` when call identity must
    survive a round trip.
    """
    encoded_output = encode_function_call_output_payload(output.output)
    if isinstance(output, ModelToolOutputCustom):
        return {
            "type": "custom_tool_call_output",
            "call_id": output.call_id,
            "output": encoded_output,
        }
    return {
        "type": "function_call_output",
        "call_id": output.call_id,
        "output": encoded_output,
    }


def tool_output_to_dict(output: ModelToolOutput) -> dict[str, Any]:
    """Identity-preserving encoding: keeps ``name`` on custom outputs.

    Mirrors Rust's ``impl Serialize for ModelToolOutput`` rather than the
    codec's ``encode_tool_output``, and round-trips through
    ``decode_tool_output``. Not used to build request bodies.
    """
    encoded = encode_tool_output(output)
    if isinstance(output, ModelToolOutputCustom) and output.name is not None:
        encoded["name"] = output.name
    return encoded


def encode_user_content(message: Message) -> list[dict[str, Any]]:
    if not message.content_blocks:
        return [{"type": "input_text", "text": message.content}]

    content: list[dict[str, Any]] = []
    for block in message.content_blocks:
        if isinstance(block, TextBlock):
            content.append({"type": "input_text", "text": block.text})
        elif isinstance(block, ImageBlock):
            source = block.source
            if isinstance(source, ImageSourceBase64):
                content.append(
                    {
                        "type": "input_image",
                        "image_url": f"data:{source.media_type};base64,{source.data}",
                    }
                )
            elif isinstance(source, ImageSourceUrl):
                content.append({"type": "input_image", "image_url": source.url})
        # Document blocks have no Responses representation and are skipped.

    if not content:
        content.append({"type": "input_text", "text": message.content})
    return content


def encode_message(message: Message) -> list[dict[str, Any]]:
    """Expand one Message into zero or more Responses input items.

    System messages encode to nothing — ``build_model_request_body`` hoists
    them into ``instructions`` instead.
    """
    if message.role == Role.system:
        return []
    if message.role == Role.user:
        return [
            {"type": "message", "role": "user", "content": encode_user_content(message)}
        ]
    if message.role == Role.assistant:
        items: list[dict[str, Any]] = []
        if message.content:
            items.append(
                {
                    "type": "message",
                    "role": "assistant",
                    "content": [{"type": "output_text", "text": message.content}],
                }
            )
        items.extend(
            encode_tool_call(
                ModelToolCallFunction(
                    id=tool_call.id,
                    name=tool_call.name,
                    arguments=json.dumps(tool_call.input),
                )
            )
            for tool_call in message.tool_calls
        )
        return items
    if message.tool_call_id is None:
        return []
    return [
        encode_tool_output(
            ModelToolOutputFunction(
                call_id=message.tool_call_id,
                output=FunctionCallOutputText(text=message.content),
            )
        )
    ]


def encode_context_item(item: ModelContextItem) -> list[dict[str, Any]]:
    if isinstance(item, ModelContextMessage):
        return encode_message(item.message)
    if isinstance(item, ModelContextToolCall):
        return [encode_tool_call(item.call)]
    return [encode_tool_output(item.output)]


def encode_input(context: Sequence[ModelContextItem]) -> list[dict[str, Any]]:
    encoded: list[dict[str, Any]] = []
    for item in context:
        encoded.extend(encode_context_item(item))
    return encoded


def encode_tool_choice(choice: ToolChoice) -> Any:
    if choice.type == "tool":
        return {"type": "function", "name": choice.name}
    return choice.type
```

- [ ] **Step 4: Run tests**

Run: `cd sdks/python && uv run pytest tests/test_responses_codec.py -q && uv run ruff check motosan_ai/ && uv run ruff format --check motosan_ai/ tests/ && uv run mypy motosan_ai/`
Expected: PASS — 11 passed, ruff clean, format check clean, mypy `Success: no issues found`.

- [ ] **Step 5: Commit**

```bash
git add sdks/python/motosan_ai/providers/responses.py sdks/python/tests/test_responses_codec.py
git commit -m "feat: add Responses codec encoders to the Python SDK (#270)" \
           -m "Co-Authored-By: Claude Opus 5 (1M context) <noreply@anthropic.com>"
```

---

### Task 6: Responses codec — decoders and blocking-response assembly

**Files:**
- Modify: `sdks/python/motosan_ai/providers/responses.py` (append after `encode_tool_choice`)
- Test: `sdks/python/tests/test_responses_codec.py` (extend)

**Interfaces:**
- Consumes: the encoders from Task 5; `ModelChatResponse`, `Usage`, `StopReason`, `ImageDetail` from Task 3 / existing types.
- Produces: `decode_function_call_output_payload`, `decode_tool_call`, `decode_tool_output`, `decode_usage`, `stop_reason_from_status`, `decode_output_text`, `model_chat_response_from_output`. `decode_tool_call` is reused by the SSE parser (Task 8); `model_chat_response_from_output` is the OpenAI non-streaming decode path (Task 13).

**Traps this task must reproduce exactly:**
1. `decode_tool_call` accepts `call_id` **OR** `id` as the call identity, in that order.
2. Freeform `input` is returned verbatim — a payload that looks like JSON stays a string.
3. `stop_reason_from_status`: tool calls win over everything; then `"incomplete"` → `max_tokens`; `"completed"` or absent → `end_turn`; anything else (including `"failed"`) → `other`.
4. `decode_usage` accepts both the Responses spelling (`input_tokens` / `output_tokens`) and the Chat Completions spelling (`prompt_tokens` / `completion_tokens`), and only sets `cache_read_input_tokens` when `input_tokens_details.cached_tokens` is **greater than zero**.

- [ ] **Step 1: Write the failing test**

Append to `sdks/python/tests/test_responses_codec.py`:

```python


def test_decodes_function_and_custom_calls():
    assert decode_tool_call(
        {
            "type": "function_call",
            "call_id": "call_fn",
            "name": "sum",
            "arguments": '{"a":1}',
        }
    ) == ModelToolCallFunction(id="call_fn", name="sum", arguments='{"a":1}')

    assert decode_tool_call(
        {
            "type": "custom_tool_call",
            "call_id": "call_js",
            "name": "exec",
            "input": "const a = {raw: true};\n",
        }
    ) == ModelToolCallFreeform(
        id="call_js", name="exec", input="const a = {raw: true};\n"
    )


def test_decode_tool_call_accepts_id_when_call_id_is_absent():
    assert decode_tool_call(
        {"type": "function_call", "id": "fc_1", "name": "sum", "arguments": "{}"}
    ) == ModelToolCallFunction(id="fc_1", name="sum", arguments="{}")
    # call_id wins when both are present.
    assert decode_tool_call(
        {
            "type": "custom_tool_call",
            "id": "fc_1",
            "call_id": "call_js",
            "name": "exec",
            "input": "x",
        }
    ) == ModelToolCallFreeform(id="call_js", name="exec", input="x")


def test_decode_tool_call_returns_none_for_non_calls():
    assert decode_tool_call({"type": "message", "role": "assistant"}) is None
    assert decode_tool_call({"type": "reasoning", "summary": []}) is None
    assert decode_tool_call("not a dict") is None
    assert decode_tool_call({"type": "function_call", "name": "sum"}) is None
    assert decode_tool_call({"type": "function_call", "call_id": "c"}) is None


def test_decode_preserves_raw_custom_input_without_json_parsing():
    raw = '{"this":"looks like json"}\nconsole.log(\'but is JavaScript\');'
    decoded = decode_tool_call(
        {"type": "custom_tool_call", "call_id": "call_js", "name": "exec", "input": raw}
    )
    assert isinstance(decoded, ModelToolCallFreeform)
    assert decoded.input == raw
    assert decoded.input.encode() == raw.encode()


def test_tool_output_round_trips_with_call_identity():
    output = ModelToolOutputCustom(
        call_id="call_js", output=FunctionCallOutputText(text="stdout: 42"), name="exec"
    )
    assert decode_tool_output(tool_output_to_dict(output)) == output

    fn_output = ModelToolOutputFunction(
        call_id="call_fn", output=FunctionCallOutputText(text='{"ok":true}')
    )
    assert decode_tool_output(tool_output_to_dict(fn_output)) == fn_output
    assert decode_tool_output({"type": "message"}) is None
    assert decode_tool_output({"type": "function_call_output", "call_id": "c"}) is None


def test_decode_function_call_output_payload_content_items():
    decoded = decode_function_call_output_payload(
        [
            {"type": "input_text", "text": "hi"},
            {"type": "input_image", "image_url": "u", "detail": "high"},
            {"type": "encrypted_content", "encrypted_content": "enc"},
        ]
    )
    assert decoded == FunctionCallOutputContent(
        items=[
            FunctionCallOutputInputText(text="hi"),
            FunctionCallOutputInputImage(image_url="u", detail=ImageDetail.high),
            FunctionCallOutputEncryptedContent(encrypted_content="enc"),
        ]
    )
    assert decode_function_call_output_payload("plain") == FunctionCallOutputText(text="plain")


def test_stop_reason_from_status():
    assert stop_reason_from_status("completed", True) == StopReason.tool_use
    assert stop_reason_from_status("incomplete", True) == StopReason.tool_use
    assert stop_reason_from_status("completed", False) == StopReason.end_turn
    assert stop_reason_from_status(None, False) == StopReason.end_turn
    assert stop_reason_from_status("incomplete", False) == StopReason.max_tokens
    assert stop_reason_from_status("failed", False) == StopReason.other
    assert stop_reason_from_status("weird", False) == StopReason.other


def test_decode_usage_accepts_both_spellings():
    assert decode_usage({"input_tokens": 9, "output_tokens": 7}) == Usage(
        input_tokens=9, output_tokens=7
    )
    assert decode_usage({"prompt_tokens": 4, "completion_tokens": 5}) == Usage(
        input_tokens=4, output_tokens=5
    )
    assert decode_usage(None) == Usage(0, 0)
    assert decode_usage({}) == Usage(0, 0)

    cached = decode_usage(
        {"input_tokens": 9, "output_tokens": 7, "input_tokens_details": {"cached_tokens": 3}}
    )
    assert cached.cache_read_input_tokens == 3
    assert cached.cache_creation_input_tokens is None
    zero = decode_usage(
        {"input_tokens": 9, "output_tokens": 7, "input_tokens_details": {"cached_tokens": 0}}
    )
    assert zero.cache_read_input_tokens is None


def test_decode_output_text():
    assert (
        decode_output_text(
            {
                "type": "message",
                "role": "assistant",
                "content": [
                    {"type": "output_text", "text": "Hi "},
                    {"type": "refusal", "text": "ignored"},
                    {"type": "output_text", "text": "there"},
                ],
            }
        )
        == "Hi there"
    )
    assert decode_output_text({"type": "function_call"}) is None
    assert decode_output_text({"type": "message", "content": []}) is None


def test_model_chat_response_from_output_assembles_calls_thinking_and_usage():
    raw = "const x = {a: 1};\nconsole.log(x.a);\n"
    response = model_chat_response_from_output(
        {
            "model": "gpt-5.5-codex",
            "status": "completed",
            "output": [
                {
                    "type": "reasoning",
                    "summary": [{"text": "thought "}, {"content": "harder"}],
                },
                {
                    "type": "message",
                    "role": "assistant",
                    "content": [{"type": "output_text", "text": "ok"}],
                },
                {
                    "type": "custom_tool_call",
                    "call_id": "call_js",
                    "name": "exec",
                    "input": raw,
                },
            ],
            "usage": {"input_tokens": 9, "output_tokens": 7},
        },
        "fallback-model",
    )

    assert response.content == "ok"
    assert response.thinking == "thought harder"
    assert response.tool_calls == [
        ModelToolCallFreeform(id="call_js", name="exec", input=raw)
    ]
    assert response.model == "gpt-5.5-codex"
    assert response.usage == Usage(input_tokens=9, output_tokens=7)
    assert response.stop_reason == StopReason.tool_use
    assert response.session_id is None


def test_model_chat_response_from_output_defaults_and_output_text_field():
    response = model_chat_response_from_output(
        {"output_text": "flat text", "status": "incomplete"}, "fallback-model"
    )
    assert response.content == "flat text"
    assert response.model == "fallback-model"
    assert response.thinking is None
    assert response.tool_calls == []
    assert response.stop_reason == StopReason.max_tokens
    assert model_chat_response_from_output({}, "fallback-model").stop_reason == StopReason.end_turn
```

Extend the two import blocks at the top of `tests/test_responses_codec.py`:

```python
from motosan_ai.providers.responses import (
    decode_function_call_output_payload,
    decode_output_text,
    decode_tool_call,
    decode_tool_output,
    decode_usage,
    encode_function_call_output_payload,
    encode_input,
    encode_message,
    encode_tool_call,
    encode_tool_choice,
    encode_tool_output,
    encode_tools,
    encode_user_content,
    model_chat_response_from_output,
    stop_reason_from_status,
    tool_output_to_dict,
)
```

and add `StopReason` and `Usage` to the `from motosan_ai.types import (...)` block.

- [ ] **Step 2: Run it to verify it fails**

Run: `cd sdks/python && uv run pytest tests/test_responses_codec.py -q`
Expected: FAIL with `ImportError: cannot import name 'decode_function_call_output_payload' from 'motosan_ai.providers.responses'` — collection error, 0 tests run.

- [ ] **Step 3: Implement**

Append to `sdks/python/motosan_ai/providers/responses.py`:

```python


def decode_function_call_output_payload(value: Any) -> FunctionCallOutputPayload:
    if isinstance(value, list):
        items: list[FunctionCallOutputContentItem] = []
        for raw in value:
            if not isinstance(raw, dict):
                continue
            kind = raw.get("type")
            if kind == "input_text":
                items.append(FunctionCallOutputInputText(text=str(raw.get("text", ""))))
            elif kind == "input_image":
                detail = raw.get("detail")
                items.append(
                    FunctionCallOutputInputImage(
                        image_url=str(raw.get("image_url", "")),
                        detail=ImageDetail(detail) if isinstance(detail, str) else None,
                    )
                )
            elif kind == "encrypted_content":
                items.append(
                    FunctionCallOutputEncryptedContent(
                        encrypted_content=str(raw.get("encrypted_content", ""))
                    )
                )
        return FunctionCallOutputContent(items=items)
    if isinstance(value, str):
        return FunctionCallOutputText(text=value)
    return FunctionCallOutputText(text=json.dumps(value))


def decode_tool_call(item: Any) -> ModelToolCall | None:
    """Decode one Responses output item into a native tool call.

    Accepts ``call_id`` OR ``id`` as the call identity, in that order.
    Returns None for items that are not tool calls.
    """
    if not isinstance(item, dict):
        return None
    kind = item.get("type")
    if kind not in ("function_call", "custom_tool_call"):
        return None
    call_id = item.get("call_id")
    if not isinstance(call_id, str):
        call_id = item.get("id")
    if not isinstance(call_id, str):
        return None
    name = item.get("name")
    if not isinstance(name, str):
        return None
    if kind == "custom_tool_call":
        raw_input = item.get("input")
        return ModelToolCallFreeform(
            id=call_id,
            name=name,
            # Verbatim: never json.loads'd, however JSON-shaped it looks.
            input=raw_input if isinstance(raw_input, str) else "",
        )
    arguments = item.get("arguments")
    return ModelToolCallFunction(
        id=call_id,
        name=name,
        arguments=arguments if isinstance(arguments, str) else "",
    )


def decode_tool_output(item: Any) -> ModelToolOutput | None:
    if not isinstance(item, dict):
        return None
    kind = item.get("type")
    if kind not in ("function_call_output", "custom_tool_call_output"):
        return None
    call_id = item.get("call_id")
    if not isinstance(call_id, str) or "output" not in item:
        return None
    payload = decode_function_call_output_payload(item["output"])
    if kind == "custom_tool_call_output":
        name = item.get("name")
        return ModelToolOutputCustom(
            call_id=call_id,
            output=payload,
            name=name if isinstance(name, str) else None,
        )
    return ModelToolOutputFunction(call_id=call_id, output=payload)


def decode_usage(value: Any) -> Usage:
    """Accepts both the Responses and the Chat Completions token spellings."""
    if not isinstance(value, dict):
        return Usage(input_tokens=0, output_tokens=0)

    raw_input = value.get("input_tokens")
    if raw_input is None:
        raw_input = value.get("prompt_tokens")
    raw_output = value.get("output_tokens")
    if raw_output is None:
        raw_output = value.get("completion_tokens")

    cached: int | None = None
    details = value.get("input_tokens_details")
    if isinstance(details, dict):
        raw_cached = details.get("cached_tokens")
        if isinstance(raw_cached, int) and raw_cached > 0:
            cached = raw_cached

    return Usage(
        input_tokens=int(raw_input or 0),
        output_tokens=int(raw_output or 0),
        cache_creation_input_tokens=None,
        cache_read_input_tokens=cached,
    )


def stop_reason_from_status(status: str | None, has_tool_calls: bool) -> StopReason:
    if has_tool_calls:
        return StopReason.tool_use
    if status == "incomplete":
        return StopReason.max_tokens
    if status is None or status == "completed":
        return StopReason.end_turn
    return StopReason.other


def decode_output_text(item: Any) -> str | None:
    """Concatenate the ``output_text`` parts of a ``message`` output item."""
    if not isinstance(item, dict) or item.get("type") != "message":
        return None
    content = item.get("content")
    if not isinstance(content, list):
        return None
    parts: list[str] = []
    for part in content:
        if not isinstance(part, dict) or part.get("type") != "output_text":
            continue
        text = part.get("text")
        if isinstance(text, str):
            parts.append(text)
    return "".join(parts) or None


def model_chat_response_from_output(payload: Any, default_model: str) -> ModelChatResponse:
    """Decode a non-streaming Responses payload into a ModelChatResponse."""
    payload = payload if isinstance(payload, dict) else {}
    content = ""
    thinking: str | None = None
    tool_calls: list[ModelToolCall] = []

    output_text = payload.get("output_text")
    if isinstance(output_text, str):
        content += output_text

    output_items = payload.get("output")
    if isinstance(output_items, list):
        for item in output_items:
            text = decode_output_text(item)
            if text is not None:
                content += text
            if isinstance(item, dict) and item.get("type") == "reasoning":
                summary = item.get("summary")
                if isinstance(summary, list):
                    summary_parts: list[str] = []
                    for part in summary:
                        if not isinstance(part, dict):
                            continue
                        value = part.get("text")
                        if not isinstance(value, str):
                            value = part.get("content")
                        if isinstance(value, str):
                            summary_parts.append(value)
                    joined = "".join(summary_parts)
                    if joined:
                        thinking = joined
            call = decode_tool_call(item)
            if call is not None:
                tool_calls.append(call)

    status = payload.get("status")
    model = payload.get("model")
    return ModelChatResponse(
        content=content,
        thinking=thinking,
        tool_calls=tool_calls,
        model=model if isinstance(model, str) else default_model,
        usage=decode_usage(payload.get("usage")),
        stop_reason=stop_reason_from_status(
            status if isinstance(status, str) else None, bool(tool_calls)
        ),
        session_id=None,
    )
```

Extend the `from motosan_ai.types import (...)` block at the top of `responses.py` with `ImageDetail`, `ModelChatResponse`, `StopReason`, and `Usage`, keeping the list alphabetically sorted.

- [ ] **Step 4: Run tests**

Run: `cd sdks/python && uv run pytest tests/test_responses_codec.py -q && uv run ruff check motosan_ai/ && uv run ruff format --check motosan_ai/ tests/ && uv run mypy motosan_ai/`
Expected: PASS — 21 passed, ruff clean, format check clean, mypy `Success: no issues found`.

- [ ] **Step 5: Commit**

```bash
git add sdks/python/motosan_ai/providers/responses.py sdks/python/tests/test_responses_codec.py
git commit -m "feat: add Responses codec decoders to the Python SDK (#270)" \
           -m "Co-Authored-By: Claude Opus 5 (1M context) <noreply@anthropic.com>"
```

---

### Task 7: Responses codec — `build_model_request_body`

**Files:**
- Modify: `sdks/python/motosan_ai/providers/responses.py` (append after `model_chat_response_from_output`)
- Test: `sdks/python/tests/test_responses_codec.py` (extend)

**Interfaces:**
- Consumes: `encode_input`, `encode_tools`, `encode_tool_choice` (Task 5); `ModelChatRequest`, `ModelContextMessage`, `Role`, `SystemBlock` (Task 3 / existing).
- Produces: `build_model_request_body(request, default_model, *, stream, default_instructions=None) -> dict[str, Any]`. Called by `ChatGptCodexProvider.build_model_responses_body` (Task 12) and `OpenAIProvider.model_chat` / `model_stream` (Task 13).

**Traps this task must reproduce exactly:**
1. `Role.system` messages inside `context` are hoisted into `instructions` **and removed from `input`**.
2. `provider_options` is shallow-merged **last**, so a caller can override anything the encoder produced — including `model`, `input`, and `tools`. This is also what makes Codex's `reasoning_effort` cleanup in Task 12 necessary.
3. `max_tokens` becomes `max_output_tokens`; `stop_sequences` becomes `stop`, and only when the list is non-empty.
4. `system_blocks` takes priority over the `system` string; both are trimmed and joined with `\n\n`, and the joined string is prefixed onto anything hoisted from `context`.
5. `default_instructions` is the fallback used only when nothing else produced instructions; when it is `None` the key is omitted entirely.

- [ ] **Step 1: Write the failing test**

Append to `sdks/python/tests/test_responses_codec.py`:

```python


def test_build_body_minimum_shape_and_stream_flag():
    request = ModelChatRequest.builder().message(Message.user("hi")).build()

    body = build_model_request_body(request, "gpt-test", stream=False)
    assert body["model"] == "gpt-test"
    assert body["input"] == [
        {"type": "message", "role": "user", "content": [{"type": "input_text", "text": "hi"}]}
    ]
    assert "stream" not in body
    assert "tools" not in body
    assert "instructions" not in body

    streamed = build_model_request_body(request, "gpt-test", stream=True)
    assert streamed["stream"] is True


def test_build_body_prefers_request_model_over_default():
    request = ModelChatRequest.builder().model("gpt-5.5-codex").build()
    assert build_model_request_body(request, "gpt-test", stream=False)["model"] == "gpt-5.5-codex"


def test_build_body_hoists_system_messages_out_of_input():
    request = (
        ModelChatRequest.builder()
        .message(Message.system("  first  "))
        .message(Message.user("run it"))
        .message(Message.system("second"))
        .build()
    )
    body = build_model_request_body(request, "gpt-test", stream=False)

    assert body["instructions"] == "first\n\nsecond"
    assert [item["type"] for item in body["input"]] == ["message"]
    assert body["input"][0]["role"] == "user"


def test_build_body_system_blocks_beat_system_string_and_prefix_hoisted():
    from_blocks = (
        ModelChatRequest.builder()
        .system("ignored")
        .system_block(SystemBlock.new("  block a  "))
        .system_block(SystemBlock.new(""))
        .system_block(SystemBlock.new("block b"))
        .message(Message.system("from context"))
        .build()
    )
    body = build_model_request_body(from_blocks, "gpt-test", stream=False)
    assert body["instructions"] == "block a\n\nblock b\n\nfrom context"

    from_string = ModelChatRequest.builder().system("  plain  ").build()
    assert (
        build_model_request_body(from_string, "gpt-test", stream=False)["instructions"] == "plain"
    )


def test_build_body_default_instructions_are_a_fallback_only():
    empty = ModelChatRequest.builder().message(Message.user("hi")).build()
    assert (
        build_model_request_body(
            empty, "gpt-test", stream=True, default_instructions="You are a helpful assistant."
        )["instructions"]
        == "You are a helpful assistant."
    )
    assert "instructions" not in build_model_request_body(empty, "gpt-test", stream=True)

    with_system = ModelChatRequest.builder().system("be terse").build()
    assert (
        build_model_request_body(
            with_system, "gpt-test", stream=True, default_instructions="You are a helpful assistant."
        )["instructions"]
        == "be terse"
    )


def test_build_body_scalar_fields_use_wire_key_names():
    request = (
        ModelChatRequest.builder()
        .temperature(0.5)
        .max_tokens(256)
        .tool_choice(ToolChoice.required())
        .stop("END")
        .build()
    )
    body = build_model_request_body(request, "gpt-test", stream=False)

    assert body["temperature"] == 0.5
    # max_tokens -> max_output_tokens on the wire.
    assert body["max_output_tokens"] == 256
    assert "max_tokens" not in body
    assert body["tool_choice"] == "required"
    assert body["stop"] == ["END"]

    forced = build_model_request_body(
        ModelChatRequest.builder().tool_choice(ToolChoice.tool("run_js")).build(),
        "gpt-test",
        stream=False,
    )
    assert forced["tool_choice"] == {"type": "function", "name": "run_js"}

    assert "stop" not in build_model_request_body(
        ModelChatRequest.builder().stop_sequences([]).build(), "gpt-test", stream=False
    )


def test_build_body_encodes_tool_specs():
    request = (
        ModelChatRequest.builder()
        .tool_spec(ModelToolSpecFreeform(tool=grammar_fixture()))
        .tool_spec(ModelToolSpecFunction(tool=Tool(name="sum", description="Add", input_schema={})))
        .build()
    )
    body = build_model_request_body(request, "gpt-test", stream=False)
    assert body["tools"][0]["type"] == "custom"
    assert body["tools"][0]["format"]["syntax"] == "lark"
    assert body["tools"][1]["type"] == "function"
    assert body["tools"][1]["name"] == "sum"


def test_build_body_shallow_merges_provider_options_last():
    request = (
        ModelChatRequest.builder()
        .model("gpt-5.5-codex")
        .temperature(0.1)
        .provider_options({"temperature": 0.9, "reasoning_effort": "high", "extra": True})
        .build()
    )
    body = build_model_request_body(request, "gpt-test", stream=False)

    # provider_options wins over everything the encoder produced.
    assert body["temperature"] == 0.9
    assert body["reasoning_effort"] == "high"
    assert body["extra"] is True
    assert body["model"] == "gpt-5.5-codex"
```

Add `build_model_request_body` to the `from motosan_ai.providers.responses import (...)` block and `ModelChatRequest` + `SystemBlock` to the `from motosan_ai.types import (...)` block at the top of `tests/test_responses_codec.py`.

- [ ] **Step 2: Run it to verify it fails**

Run: `cd sdks/python && uv run pytest tests/test_responses_codec.py -q`
Expected: FAIL with `ImportError: cannot import name 'build_model_request_body' from 'motosan_ai.providers.responses'` — collection error, 0 tests run.

- [ ] **Step 3: Implement**

Append to `sdks/python/motosan_ai/providers/responses.py`:

```python


def build_model_request_body(
    request: ModelChatRequest,
    default_model: str,
    *,
    stream: bool,
    default_instructions: str | None = None,
) -> dict[str, Any]:
    """Encode a ModelChatRequest into an OpenAI Responses request body.

    Two rules here are load-bearing and easy to miss:

    1. ``Role.system`` messages inside ``context`` are hoisted into
       ``instructions`` AND removed from ``input``.
    2. ``provider_options`` is shallow-merged LAST, so it can override
       anything this encoder produced. Callers that must win over
       ``provider_options`` (ChatGPT Codex does) have to apply their
       overrides after calling this function.
    """
    model = request.model or default_model

    instructions_parts: list[str] = []
    if request.system_blocks is not None:
        for block in request.system_blocks:
            trimmed = block.text.strip()
            if trimmed:
                instructions_parts.append(trimmed)
    elif request.system is not None:
        trimmed = request.system.strip()
        if trimmed:
            instructions_parts.append(trimmed)

    input_context: list[ModelContextItem] = []
    for item in request.context:
        if isinstance(item, ModelContextMessage) and item.message.role == Role.system:
            trimmed = item.message.content.strip()
            if trimmed:
                instructions_parts.append(trimmed)
            continue
        input_context.append(item)

    body: dict[str, Any] = {"model": model, "input": encode_input(input_context)}

    if stream:
        body["stream"] = True
    if request.tool_specs:
        body["tools"] = encode_tools(request.tool_specs)

    instructions = "\n\n".join(instructions_parts) if instructions_parts else default_instructions
    if instructions is not None:
        body["instructions"] = instructions

    if request.temperature is not None:
        body["temperature"] = request.temperature
    if request.max_tokens is not None:
        # Wire key differs from the field name.
        body["max_output_tokens"] = request.max_tokens
    if request.tool_choice is not None:
        body["tool_choice"] = encode_tool_choice(request.tool_choice)
    if request.stop_sequences:
        body["stop"] = list(request.stop_sequences)

    # Shallow merge, LAST.
    if request.provider_options:
        body.update(request.provider_options)

    return body
```

Add `ModelChatRequest` to the `from motosan_ai.types import (...)` block at the top of `responses.py`.

- [ ] **Step 4: Run tests**

Run: `cd sdks/python && uv run pytest tests/test_responses_codec.py -q && uv run ruff check motosan_ai/ && uv run ruff format --check motosan_ai/ tests/ && uv run mypy motosan_ai/`
Expected: PASS — 29 passed, ruff clean, format check clean, mypy `Success: no issues found`.

- [ ] **Step 5: Commit**

```bash
git add sdks/python/motosan_ai/providers/responses.py sdks/python/tests/test_responses_codec.py
git commit -m "feat: add build_model_request_body to the Responses codec (#270)" \
           -m "Co-Authored-By: Claude Opus 5 (1M context) <noreply@anthropic.com>"
```

---

### Task 8: Responses codec — native SSE frame parser

**Files:**
- Modify: `sdks/python/motosan_ai/providers/responses.py` (append after `build_model_request_body`)
- Test: `sdks/python/tests/test_responses_codec.py` (extend)

**Interfaces:**
- Consumes: `decode_tool_call`, `decode_usage`, `stop_reason_from_status` (Task 6); every `ModelStream*` variant (Task 3).
- Produces: `ModelStreamState` (dataclass: `item_to_call_id`, `saw_tool_call`, `saw_terminal`, `error`) and `parse_model_sse_event(data: str, state: ModelStreamState) -> list[ModelStreamDelta]`. Both are consumed by `ChatGptCodexProvider.model_stream` (Task 12) and `OpenAIProvider.model_stream` (Task 13).

**Why a new parser and not the existing one:** `providers/chatgpt_codex.py:55` `_parse_sse_event` maps the same wire onto the **legacy** `StreamEvent`, and it handles neither `response.custom_tool_call_input.delta`, nor `custom_tool_call` output items, nor `response.reasoning_text.done` / `response.reasoning_summary_text.done`, nor `response.incomplete`. It stays function-tool-only and is not touched by this plan.

**Traps this task must reproduce exactly:**
1. `call_id` resolution order in delta frames: event `call_id` → `item_id` looked up in the item→call map → the raw `item_id` as a last resort.
2. `response.output_item.added` **and** `response.output_item.done` both feed the item→call map, and both set `saw_tool_call`.
3. `response.completed` and `response.incomplete` are **both** terminals. Each emits an optional `ModelStreamUsage` followed by exactly one `ModelStreamDone`, and sets `saw_terminal`.
4. The usage delta is emitted only when at least one of the four usage fields is non-zero / non-`None`.
5. A `ThinkingDone` payload is emitted even when the text is the empty string — the collector, not the parser, decides that an empty block means "no thinking".
6. Empty frames, the `[DONE]` sentinel, malformed JSON, and non-object JSON are all skipped silently.
7. `error` / `response.failed` frames do not yield a delta: they set `state.error`, and the transport raises after draining whatever is already pending.

- [ ] **Step 1: Write the failing test**

Append to `sdks/python/tests/test_responses_codec.py`:

```python


def _frames(state, *payloads):
    out = []
    for payload in payloads:
        out.extend(parse_model_sse_event(json.dumps(payload), state))
    return out


def test_parse_text_and_thinking_frames():
    state = ModelStreamState()
    deltas = _frames(
        state,
        {"type": "response.output_text.delta", "delta": "Hi "},
        {"type": "response.output_text.delta", "delta": ""},
        {"type": "response.output_text.delta", "delta": "there"},
        {"type": "response.reasoning_text.delta", "delta": "think "},
        {"type": "response.reasoning_summary_text.delta", "delta": "hard"},
        {"type": "response.reasoning_text.done", "text": "think hard"},
        {"type": "response.reasoning_summary_text.done", "delta": "fallback key"},
    )
    assert deltas == [
        ModelStreamText(delta="Hi "),
        ModelStreamText(delta="there"),
        ModelStreamThinkingDelta(delta="think "),
        ModelStreamThinkingDelta(delta="hard"),
        ModelStreamThinkingDone(thinking="think hard"),
        ModelStreamThinkingDone(thinking="fallback key"),
    ]
    # An explicitly empty thinking block still produces a delta; the collector
    # is what turns it into "no thinking".
    assert parse_model_sse_event(
        json.dumps({"type": "response.reasoning_text.done", "text": ""}), ModelStreamState()
    ) == [ModelStreamThinkingDone(thinking="")]


def test_parse_freeform_input_deltas_and_authoritative_done():
    state = ModelStreamState()
    deltas = _frames(
        state,
        {
            "type": "response.custom_tool_call_input.delta",
            "call_id": "call_js",
            "delta": "console.",
        },
        {
            "type": "response.custom_tool_call_input.delta",
            "call_id": "call_js",
            "delta": "log(1);\n",
        },
        {
            "type": "response.output_item.done",
            "item": {
                "type": "custom_tool_call",
                "call_id": "call_js",
                "name": "exec",
                "input": "console.log(1);\n",
            },
        },
    )
    assert deltas == [
        ModelStreamFreeformInput(call_id="call_js", delta="console."),
        ModelStreamFreeformInput(call_id="call_js", delta="log(1);\n"),
        ModelStreamToolCallDone(
            call=ModelToolCallFreeform(id="call_js", name="exec", input="console.log(1);\n")
        ),
    ]
    assert state.saw_tool_call is True


def test_parse_resolves_call_id_through_the_item_map():
    state = ModelStreamState()
    _frames(
        state,
        {
            "type": "response.output_item.added",
            "item": {"type": "function_call", "id": "fc_1", "call_id": "call_fn", "name": "sum"},
        },
    )
    assert state.item_to_call_id == {"fc_1": "call_fn"}
    assert state.saw_tool_call is True

    # 1. event call_id wins.
    assert parse_model_sse_event(
        json.dumps(
            {
                "type": "response.function_call_arguments.delta",
                "call_id": "explicit",
                "item_id": "fc_1",
                "delta": "{",
            }
        ),
        state,
    ) == [ModelStreamFunctionArguments(call_id="explicit", delta="{")]
    # 2. item_id resolves through the map.
    assert parse_model_sse_event(
        json.dumps(
            {"type": "response.function_call_arguments.delta", "item_id": "fc_1", "delta": '"a"'}
        ),
        state,
    ) == [ModelStreamFunctionArguments(call_id="call_fn", delta='"a"')]
    # 3. unknown item_id falls through as itself.
    assert parse_model_sse_event(
        json.dumps(
            {"type": "response.function_call_arguments.delta", "item_id": "fc_9", "delta": "}"}
        ),
        state,
    ) == [ModelStreamFunctionArguments(call_id="fc_9", delta="}")]
    # 4. no id at all -> nothing.
    assert (
        parse_model_sse_event(
            json.dumps({"type": "response.function_call_arguments.delta", "delta": "}"}), state
        )
        == []
    )


def test_parse_completed_emits_usage_then_exactly_one_done():
    state = ModelStreamState()
    deltas = parse_model_sse_event(
        json.dumps(
            {
                "type": "response.completed",
                "response": {
                    "status": "completed",
                    "usage": {"input_tokens": 2, "output_tokens": 3},
                },
            }
        ),
        state,
    )
    assert deltas == [
        ModelStreamUsage(usage=Usage(input_tokens=2, output_tokens=3)),
        ModelStreamDone(stop_reason=StopReason.end_turn),
    ]
    assert state.saw_terminal is True
    assert sum(isinstance(d, ModelStreamDone) for d in deltas) == 1


def test_parse_incomplete_is_a_terminal_mapping_to_max_tokens():
    state = ModelStreamState()
    deltas = parse_model_sse_event(
        json.dumps(
            {
                "type": "response.incomplete",
                "response": {
                    "status": "incomplete",
                    "usage": {"input_tokens": 6, "output_tokens": 7},
                    "incomplete_details": {"reason": "max_output_tokens"},
                },
            }
        ),
        state,
    )
    assert deltas[-1] == ModelStreamDone(stop_reason=StopReason.max_tokens)
    assert state.saw_terminal is True


def test_parse_completed_after_tool_call_reports_tool_use_and_omits_zero_usage():
    state = ModelStreamState()
    state.saw_tool_call = True
    deltas = parse_model_sse_event(
        json.dumps({"type": "response.completed", "response": {"status": "completed"}}), state
    )
    assert deltas == [ModelStreamDone(stop_reason=StopReason.tool_use)]


def test_parse_skips_noise_frames():
    state = ModelStreamState()
    assert parse_model_sse_event("", state) == []
    assert parse_model_sse_event("   ", state) == []
    assert parse_model_sse_event("[DONE]", state) == []
    assert parse_model_sse_event("{not json", state) == []
    assert parse_model_sse_event("[1, 2]", state) == []
    assert parse_model_sse_event(json.dumps({"type": "response.created"}), state) == []
    assert state.error is None
    assert state.saw_terminal is False


def test_parse_records_stream_errors_without_yielding_a_delta():
    top_level = ModelStreamState()
    assert parse_model_sse_event(json.dumps({"type": "error", "message": "boom"}), top_level) == []
    assert top_level.error == "boom"

    nested = ModelStreamState()
    parse_model_sse_event(
        json.dumps({"type": "response.failed", "response": {"error": {"message": "nested"}}}),
        nested,
    )
    assert nested.error == "nested"

    sibling = ModelStreamState()
    parse_model_sse_event(
        json.dumps({"type": "error", "error": {"message": "sibling"}}), sibling
    )
    assert sibling.error == "sibling"

    bare = ModelStreamState()
    parse_model_sse_event(json.dumps({"type": "error"}), bare)
    assert bare.error == "responses stream error"
```

Extend the imports at the top of `tests/test_responses_codec.py`: add `import json` after `from __future__ import annotations`, add `ModelStreamState` and `parse_model_sse_event` to the `motosan_ai.providers.responses` block, and add `ModelStreamDone`, `ModelStreamFreeformInput`, `ModelStreamFunctionArguments`, `ModelStreamText`, `ModelStreamThinkingDelta`, `ModelStreamThinkingDone`, `ModelStreamToolCallDone`, `ModelStreamUsage` to the `motosan_ai.types` block.

- [ ] **Step 2: Run it to verify it fails**

Run: `cd sdks/python && uv run pytest tests/test_responses_codec.py -q`
Expected: FAIL with `ImportError: cannot import name 'parse_model_sse_event' from 'motosan_ai.providers.responses'` — collection error, 0 tests run.

- [ ] **Step 3: Implement**

Append to `sdks/python/motosan_ai/providers/responses.py`:

```python


@dataclass
class ModelStreamState:
    """Per-stream adapter state for ``parse_model_sse_event``.

    ``item_to_call_id`` maps Responses output-item ids (``fc_*``) to public
    call ids (``call_*``), because argument/input deltas arrive keyed by
    ``item_id``. ``saw_terminal`` lets the transport tell truncation from
    completion at EOF.
    """

    item_to_call_id: dict[str, str] = field(default_factory=dict)
    saw_tool_call: bool = False
    saw_terminal: bool = False
    error: str | None = None


def _remember_output_item(item: Any, state: ModelStreamState) -> None:
    if not isinstance(item, dict):
        return
    if item.get("type") not in ("function_call", "custom_tool_call"):
        return
    call_id = item.get("call_id")
    if not isinstance(call_id, str):
        return
    state.saw_tool_call = True
    item_id = item.get("id")
    if isinstance(item_id, str) and item_id:
        state.item_to_call_id[item_id] = call_id


def _call_id_from_event(chunk: dict[str, Any], state: ModelStreamState) -> str | None:
    """Event ``call_id`` -> mapped ``item_id`` -> raw ``item_id``."""
    call_id = chunk.get("call_id")
    if isinstance(call_id, str):
        return call_id
    item_id = chunk.get("item_id")
    if isinstance(item_id, str):
        return state.item_to_call_id.get(item_id, item_id)
    return None


def _stream_error_message(chunk: dict[str, Any]) -> str:
    message = chunk.get("message")
    if isinstance(message, str) and message:
        return message
    response = chunk.get("response")
    if isinstance(response, dict):
        error = response.get("error")
        if isinstance(error, dict) and isinstance(error.get("message"), str):
            nested = error["message"]
            if nested:
                return str(nested)
    error = chunk.get("error")
    if isinstance(error, dict) and isinstance(error.get("message"), str):
        sibling = error["message"]
        if sibling:
            return str(sibling)
    return "responses stream error"


def parse_model_sse_event(data: str, state: ModelStreamState) -> list[ModelStreamDelta]:
    """Map one Responses SSE ``data`` payload to zero or more ModelStreamDeltas.

    Pure apart from mutating ``state``. Port of Rust's
    ``ResponsesModelStreamAdapter::handle_event``. A fatal ``error`` /
    ``response.failed`` frame sets ``state.error`` and returns ``[]`` — the
    transport raises StreamError after draining the pending deltas.
    """
    text = data.strip()
    if not text or text == "[DONE]":
        return []
    try:
        chunk = json.loads(text)
    except json.JSONDecodeError:
        return []
    if not isinstance(chunk, dict):
        return []

    event_type = chunk.get("type")
    out: list[ModelStreamDelta] = []

    if event_type == "response.output_text.delta":
        delta = chunk.get("delta")
        if isinstance(delta, str) and delta:
            out.append(ModelStreamText(delta=delta))

    elif event_type in (
        "response.reasoning_text.delta",
        "response.reasoning_summary_text.delta",
    ):
        delta = chunk.get("delta")
        if isinstance(delta, str) and delta:
            out.append(ModelStreamThinkingDelta(delta=delta))

    elif event_type in (
        "response.reasoning_text.done",
        "response.reasoning_summary_text.done",
    ):
        thinking = chunk.get("text")
        if not isinstance(thinking, str):
            thinking = chunk.get("delta")
        if isinstance(thinking, str):
            out.append(ModelStreamThinkingDone(thinking=thinking))

    elif event_type == "response.output_item.added":
        _remember_output_item(chunk.get("item"), state)

    elif event_type == "response.function_call_arguments.delta":
        call_id = _call_id_from_event(chunk, state)
        delta = chunk.get("delta")
        if call_id is not None and isinstance(delta, str):
            out.append(ModelStreamFunctionArguments(call_id=call_id, delta=delta))

    elif event_type == "response.custom_tool_call_input.delta":
        call_id = _call_id_from_event(chunk, state)
        delta = chunk.get("delta")
        if call_id is not None and isinstance(delta, str):
            out.append(ModelStreamFreeformInput(call_id=call_id, delta=delta))

    elif event_type == "response.output_item.done":
        item = chunk.get("item")
        _remember_output_item(item, state)
        call = decode_tool_call(item)
        if call is not None:
            state.saw_tool_call = True
            out.append(ModelStreamToolCallDone(call=call))

    elif event_type in ("response.completed", "response.incomplete"):
        response = chunk.get("response")
        response = response if isinstance(response, dict) else {}
        usage = decode_usage(response.get("usage"))
        if (
            usage.input_tokens != 0
            or usage.output_tokens != 0
            or usage.cache_creation_input_tokens is not None
            or usage.cache_read_input_tokens is not None
        ):
            out.append(ModelStreamUsage(usage=usage))
        status = response.get("status")
        out.append(
            ModelStreamDone(
                stop_reason=stop_reason_from_status(
                    status if isinstance(status, str) else None, state.saw_tool_call
                )
            )
        )
        state.saw_terminal = True

    elif event_type in ("error", "response.failed"):
        state.error = _stream_error_message(chunk)

    return out
```

Add `from dataclasses import dataclass, field` under the `import json` line in `responses.py`, and add `ModelStreamDelta`, `ModelStreamDone`, `ModelStreamFreeformInput`, `ModelStreamFunctionArguments`, `ModelStreamText`, `ModelStreamThinkingDelta`, `ModelStreamThinkingDone`, `ModelStreamToolCallDone`, `ModelStreamUsage` to the `from motosan_ai.types import (...)` block.

- [ ] **Step 4: Run tests**

Run: `cd sdks/python && uv run pytest tests/test_responses_codec.py -q && uv run ruff check motosan_ai/ && uv run ruff format --check motosan_ai/ tests/ && uv run mypy motosan_ai/`
Expected: PASS — 37 passed, ruff clean, format check clean, mypy `Success: no issues found`.

- [ ] **Step 5: Commit**

```bash
git add sdks/python/motosan_ai/providers/responses.py sdks/python/tests/test_responses_codec.py
git commit -m "feat: add the native Responses SSE parser to the Python codec (#270)" \
           -m "Co-Authored-By: Claude Opus 5 (1M context) <noreply@anthropic.com>"
```

---

### Task 9: Package-root exports for the P1 symbols

**Files:**
- Modify: `sdks/python/motosan_ai/__init__.py:1-115`
- Test: `sdks/python/tests/test_public_exports.py` (create)

**Interfaces:**
- Consumes: every symbol produced by Tasks 2, 3, and 4.
- Produces: those symbols reachable as `motosan_ai.X` and listed in `motosan_ai.__all__`. Task 15 extends the same file and the same test with the P2 symbols.

**Why this is its own task:** `motosan_ai/__init__.py` re-exports through explicit imports and a hand-maintained `__all__` (54 entries today), so a type that is never listed is invisible to callers no matter how correct it is — unlike TypeScript, where `export * from './types.js'` picks new types up for free. TypeScript pins its export surface in `tests/index.test.ts`; Python has pinned nothing until now.

Note the codec module itself is **not** exported from the package root: it is provider-internal, reached as `motosan_ai.providers.responses`, matching the Rust `providers::responses` module path.

- [ ] **Step 1: Write the failing test**

Create `sdks/python/tests/test_public_exports.py`:

```python
"""Pin the package's public export surface.

motosan_ai/__init__.py re-exports through explicit imports and a
hand-maintained __all__, so a symbol that is never listed is invisible to
callers no matter how correct it is. TypeScript pins its exports in
tests/index.test.ts; this is the Python equivalent.
"""

from __future__ import annotations

import motosan_ai

NATIVE_MODEL_EXPORTS = [
    "FreeformTool",
    "FreeformToolFormat",
    "FunctionCallOutputContent",
    "FunctionCallOutputContentItem",
    "FunctionCallOutputEncryptedContent",
    "FunctionCallOutputInputImage",
    "FunctionCallOutputInputText",
    "FunctionCallOutputPayload",
    "FunctionCallOutputText",
    "ImageDetail",
    "ModelChatRequest",
    "ModelChatRequestBuilder",
    "ModelChatResponse",
    "ModelContextItem",
    "ModelContextMessage",
    "ModelContextToolCall",
    "ModelContextToolOutput",
    "ModelStreamDelta",
    "ModelStreamDone",
    "ModelStreamFreeformInput",
    "ModelStreamFunctionArguments",
    "ModelStreamText",
    "ModelStreamThinkingDelta",
    "ModelStreamThinkingDone",
    "ModelStreamToolCallDone",
    "ModelStreamUsage",
    "ModelToolCall",
    "ModelToolCallFreeform",
    "ModelToolCallFunction",
    "ModelToolOutput",
    "ModelToolOutputCustom",
    "ModelToolOutputFunction",
    "ModelToolSpec",
    "ModelToolSpecFreeform",
    "ModelToolSpecFunction",
    "UnsupportedFeatureError",
]


def test_native_symbols_are_importable_from_the_package_root():
    missing = [name for name in NATIVE_MODEL_EXPORTS if not hasattr(motosan_ai, name)]
    assert missing == []


def test_native_symbols_are_listed_in_dunder_all():
    missing = [name for name in NATIVE_MODEL_EXPORTS if name not in motosan_ai.__all__]
    assert missing == []


def test_dunder_all_is_sorted_and_free_of_duplicates():
    assert motosan_ai.__all__ == sorted(set(motosan_ai.__all__))


def test_every_all_entry_actually_resolves():
    unresolved = [name for name in motosan_ai.__all__ if not hasattr(motosan_ai, name)]
    assert unresolved == []
```

- [ ] **Step 2: Run it to verify it fails**

Run: `cd sdks/python && uv run pytest tests/test_public_exports.py -q`
Expected: FAIL — 2 failed, 2 passed. The first failure reads
`AssertionError: assert ['FreeformTool', 'FreeformToolFormat', ...] == []`, i.e. none of the native symbols are reachable as `motosan_ai.X` yet.

- [ ] **Step 3: Implement**

In `sdks/python/motosan_ai/__init__.py`, add `UnsupportedFeatureError` to the `from motosan_ai.error import (...)` block (alphabetically, after `StreamReadTimeoutError`), and add these names to the `from motosan_ai.types import (...)` block, keeping it alphabetically sorted:

```python
    FreeformTool,
    FreeformToolFormat,
    FunctionCallOutputContent,
    FunctionCallOutputContentItem,
    FunctionCallOutputEncryptedContent,
    FunctionCallOutputInputImage,
    FunctionCallOutputInputText,
    FunctionCallOutputPayload,
    FunctionCallOutputText,
    ImageDetail,
    ModelChatRequest,
    ModelChatRequestBuilder,
    ModelChatResponse,
    ModelContextItem,
    ModelContextMessage,
    ModelContextToolCall,
    ModelContextToolOutput,
    ModelStreamDelta,
    ModelStreamDone,
    ModelStreamFreeformInput,
    ModelStreamFunctionArguments,
    ModelStreamText,
    ModelStreamThinkingDelta,
    ModelStreamThinkingDone,
    ModelStreamToolCallDone,
    ModelStreamUsage,
    ModelToolCall,
    ModelToolCallFreeform,
    ModelToolCallFunction,
    ModelToolOutput,
    ModelToolOutputCustom,
    ModelToolOutputFunction,
    ModelToolSpec,
    ModelToolSpecFreeform,
    ModelToolSpecFunction,
```

Then add the same 35 names plus `"UnsupportedFeatureError"` to `__all__`, in sorted position. Concretely: the `FreeformTool` / `FreeformToolFormat` / `FunctionCallOutput*` block goes between `"DocumentSourceUrl"` and `"GeminiCliClient"`; `"ImageDetail"` goes between `"ImageBlock"` and `"ImageSource"`; the whole `Model*` block goes between `"Message"` and `"MotosanError"`; `"UnsupportedFeatureError"` goes between `"ToolChoice"` and `"Usage"`.

The resulting `__all__` must satisfy `motosan_ai.__all__ == sorted(set(motosan_ai.__all__))`, which the test enforces — if the position of any entry is uncertain, run `python3 -c "import motosan_ai, json; print(json.dumps(sorted(set(motosan_ai.__all__)), indent=4))"` and paste the result.

- [ ] **Step 4: Run tests**

Run: `cd sdks/python && uv run pytest tests/ -q --ignore=tests/integration/ && uv run ruff check motosan_ai/ && uv run ruff format --check motosan_ai/ tests/ && uv run mypy motosan_ai/`
Expected: PASS — the full non-integration suite green (all pre-existing tests plus the 4 new export tests, 14 native-type tests, 37 codec tests, 4 error tests), ruff clean, format check clean, mypy `Success: no issues found`.

- [ ] **Step 5: Commit and open PR P1**

```bash
git add sdks/python/motosan_ai/__init__.py sdks/python/tests/test_public_exports.py
git commit -m "feat: export the native model API from the Python package root (#270)" \
           -m "Co-Authored-By: Claude Opus 5 (1M context) <noreply@anthropic.com>"

cd sdks/python && uv sync --all-extras && cd ../..
treefmt --fail-on-change
python3 scripts/check-versions.py

git push -u origin feat/freeform-python-types
test "$(git ls-remote origin refs/heads/feat/freeform-python-types | cut -f1)" = "$(git rev-parse HEAD)"
gh pr create --base main --head feat/freeform-python-types \
  --title "feat: add the native model API types and Responses codec to Python (#270)" \
  --body "PR P1 of #270. Types + codec + UnsupportedFeatureError + package-root exports. No provider wiring — P2 follows."
```

---

### Task 10: Freeform capability flag and `validate_model_request`

**Files:**
- Modify: `sdks/python/motosan_ai/provider_base.py:11-27` (`ProviderCapabilities`)
- Modify: `sdks/python/motosan_ai/provider_base.py:43-53` (`BaseProvider`) and append the module-level validator after `validate_request`
- Modify: `sdks/python/tests/test_provider_capabilities.py:44-59` (extend the three constructor tests)
- Test: `sdks/python/tests/test_native_capabilities.py` (create)

**Interfaces:**
- Consumes: `UnsupportedFeatureError` (Task 2); `ModelChatRequest`, `ModelContextMessage`, `ModelContextToolCall`, `ModelContextToolOutput`, `ModelToolCallFreeform`, `ModelToolOutputCustom`, `ModelToolSpecFreeform` (Task 3).
- Produces: `ProviderCapabilities.supports_freeform_tools`, `ProviderCapabilities.with_freeform_tools()`, `ProviderCapabilities.with_image_and_freeform_tools()`, module-level `validate_model_request(request, capabilities)`, and `BaseProvider.validate_model_request(request)`. Consumed by Tasks 12, 13, and 14.

**Decision context (D5):** the new field defaults to `False` so that `ProviderCapabilities(supports_image=..., supports_document=...)` keeps working for existing callers. `full()` deliberately leaves freeform **False** — Rust does the same, and a provider that silently claimed freeform support it lacks is exactly the bug this flag exists to prevent.

**Expected flips:** `tests/test_provider_capabilities.py` asserts exact capability shapes. Adding the field does not break the existing `is False` / `is True` assertions, but the three constructor tests must grow an assertion for the new field, and two new constructor tests appear. These are expected flips, not failures.

- [ ] **Step 1: Write the failing test**

Create `sdks/python/tests/test_native_capabilities.py`:

```python
from __future__ import annotations

import pytest

from motosan_ai.error import InvalidRequestError, UnsupportedFeatureError
from motosan_ai.provider_base import ProviderCapabilities, validate_model_request
from motosan_ai.types import (
    FreeformTool,
    FreeformToolFormat,
    FunctionCallOutputText,
    Message,
    ModelChatRequest,
    ModelContextMessage,
    ModelContextToolCall,
    ModelContextToolOutput,
    ModelToolCallFreeform,
    ModelToolCallFunction,
    ModelToolOutputCustom,
    ModelToolOutputFunction,
    ModelToolSpecFreeform,
    ModelToolSpecFunction,
    Tool,
)

FREEFORM_SPEC = ModelToolSpecFreeform(
    tool=FreeformTool(
        name="exec",
        description="Run JavaScript",
        format=FreeformToolFormat(type="grammar", syntax="lark", definition="start: source"),
    )
)


def test_with_freeform_tools_constructor():
    caps = ProviderCapabilities.with_freeform_tools()
    assert caps.supports_image is False
    assert caps.supports_document is False
    assert caps.supports_freeform_tools is True


def test_with_image_and_freeform_tools_constructor():
    caps = ProviderCapabilities.with_image_and_freeform_tools()
    assert caps.supports_image is True
    assert caps.supports_document is False
    assert caps.supports_freeform_tools is True


def test_full_deliberately_leaves_freeform_false():
    # Rust parity: full() is image + document, never freeform.
    assert ProviderCapabilities.full().supports_freeform_tools is False
    assert ProviderCapabilities.text_only().supports_freeform_tools is False
    assert ProviderCapabilities.with_image().supports_freeform_tools is False


def test_freeform_field_defaults_to_false_for_positional_construction():
    assert ProviderCapabilities(supports_image=True, supports_document=True) == (
        ProviderCapabilities.full()
    )


def test_rejects_freeform_spec_when_unsupported():
    request = ModelChatRequest.builder().tool_spec(FREEFORM_SPEC).build()
    with pytest.raises(UnsupportedFeatureError, match="native freeform tools"):
        validate_model_request(request, ProviderCapabilities.with_image())


def test_rejects_freeform_history_call_when_unsupported():
    request = (
        ModelChatRequest.builder()
        .tool_call(ModelToolCallFreeform(id="call_js", name="exec", input="x"))
        .build()
    )
    with pytest.raises(UnsupportedFeatureError, match="native freeform tools"):
        validate_model_request(request, ProviderCapabilities.full())


def test_rejects_custom_history_output_when_unsupported():
    request = (
        ModelChatRequest.builder()
        .tool_output(
            ModelToolOutputCustom(call_id="call_js", output=FunctionCallOutputText(text="1"))
        )
        .build()
    )
    with pytest.raises(UnsupportedFeatureError, match="native freeform tools"):
        validate_model_request(request, ProviderCapabilities.text_only())


def test_accepts_freeform_when_supported():
    request = (
        ModelChatRequest.builder()
        .tool_spec(FREEFORM_SPEC)
        .tool_call(ModelToolCallFreeform(id="call_js", name="exec", input="x"))
        .tool_output(
            ModelToolOutputCustom(call_id="call_js", output=FunctionCallOutputText(text="1"))
        )
        .build()
    )
    validate_model_request(request, ProviderCapabilities.with_freeform_tools())


def test_function_only_history_is_accepted_everywhere():
    request = (
        ModelChatRequest.builder()
        .tool_spec(ModelToolSpecFunction(tool=Tool(name="sum")))
        .tool_call(ModelToolCallFunction(id="call_fn", name="sum", arguments="{}"))
        .tool_output(
            ModelToolOutputFunction(call_id="call_fn", output=FunctionCallOutputText(text="1"))
        )
        .message(Message.user("hi"))
        .build()
    )
    validate_model_request(request, ProviderCapabilities.text_only())


def test_rejects_image_and_document_context_blocks():
    image = ModelChatRequest.builder().message(
        Message.user_with_image("look", "abc", "image/png")
    ).build()
    with pytest.raises(UnsupportedFeatureError, match="image"):
        validate_model_request(image, ProviderCapabilities.with_freeform_tools())
    validate_model_request(image, ProviderCapabilities.with_image_and_freeform_tools())

    document = ModelChatRequest.builder().message(
        Message.user_with_pdf_base64("read", "abc")
    ).build()
    with pytest.raises(UnsupportedFeatureError, match="document"):
        validate_model_request(document, ProviderCapabilities.with_image())
    validate_model_request(document, ProviderCapabilities.full())


def test_rejection_is_catchable_as_invalid_request_error():
    request = ModelChatRequest.builder().tool_spec(FREEFORM_SPEC).build()
    with pytest.raises(InvalidRequestError):
        validate_model_request(request, ProviderCapabilities.text_only())


def test_base_provider_method_delegates_to_its_capabilities():
    from collections.abc import AsyncIterator

    from motosan_ai.provider_base import BaseProvider
    from motosan_ai.types import ChatRequest, ChatResponse, StreamEvent

    class _Freeform(BaseProvider):
        capabilities = ProviderCapabilities.with_freeform_tools()

        async def chat(self, request: ChatRequest) -> ChatResponse:  # pragma: no cover
            raise NotImplementedError

        async def stream(self, request: ChatRequest) -> AsyncIterator[StreamEvent]:
            if False:  # pragma: no cover
                yield StreamEvent(content="", done=True)
            raise NotImplementedError

    class _TextOnly(_Freeform):
        capabilities = ProviderCapabilities.text_only()

    request = ModelChatRequest.builder().tool_spec(FREEFORM_SPEC).build()
    _Freeform().validate_model_request(request)
    with pytest.raises(UnsupportedFeatureError, match="native freeform tools"):
        _TextOnly().validate_model_request(request)
```

Also extend the three existing constructor tests in `sdks/python/tests/test_provider_capabilities.py` (lines 44-59) so each asserts the new field:

```python
def test_text_only_capabilities():
    caps = ProviderCapabilities.text_only()
    assert caps.supports_image is False
    assert caps.supports_document is False
    assert caps.supports_freeform_tools is False


def test_with_image_capabilities():
    caps = ProviderCapabilities.with_image()
    assert caps.supports_image is True
    assert caps.supports_document is False
    assert caps.supports_freeform_tools is False


def test_full_capabilities():
    caps = ProviderCapabilities.full()
    assert caps.supports_image is True
    assert caps.supports_document is True
    assert caps.supports_freeform_tools is False
```

- [ ] **Step 2: Run it to verify it fails**

Run: `cd sdks/python && uv run pytest tests/test_native_capabilities.py -q`
Expected: FAIL with `ImportError: cannot import name 'validate_model_request' from 'motosan_ai.provider_base'` — collection error, 0 tests run.

- [ ] **Step 3: Implement**

Replace `sdks/python/motosan_ai/provider_base.py:11-27` with:

```python
@dataclass(frozen=True)
class ProviderCapabilities:
    supports_image: bool
    supports_document: bool
    # Defaulted so existing two-argument construction keeps working.
    supports_freeform_tools: bool = False

    @classmethod
    def text_only(cls) -> ProviderCapabilities:
        return cls(supports_image=False, supports_document=False, supports_freeform_tools=False)

    @classmethod
    def with_image(cls) -> ProviderCapabilities:
        return cls(supports_image=True, supports_document=False, supports_freeform_tools=False)

    @classmethod
    def with_freeform_tools(cls) -> ProviderCapabilities:
        return cls(supports_image=False, supports_document=False, supports_freeform_tools=True)

    @classmethod
    def with_image_and_freeform_tools(cls) -> ProviderCapabilities:
        return cls(supports_image=True, supports_document=False, supports_freeform_tools=True)

    @classmethod
    def full(cls) -> ProviderCapabilities:
        # Rust parity: full() is image + document and deliberately leaves
        # freeform False. A provider that claimed freeform support it lacks
        # is exactly what this flag exists to prevent.
        return cls(supports_image=True, supports_document=True, supports_freeform_tools=False)
```

Append after the existing `validate_request` function (currently ending at line 40):

```python


def validate_model_request(
    request: ModelChatRequest, capabilities: ProviderCapabilities
) -> None:
    """Reject native model requests the capabilities do not support, pre-network.

    Mirrors Rust ``ProviderImpl::validate_model_request`` minus the three
    reject-only fields (thinking / mcp_servers / mcp_tool_configs), which the
    Python ``ModelChatRequest`` deliberately does not carry (milestone D3).
    """
    has_freeform_spec = any(
        isinstance(spec, ModelToolSpecFreeform) for spec in request.tool_specs
    )
    has_freeform_history = any(
        (isinstance(item, ModelContextToolCall) and isinstance(item.call, ModelToolCallFreeform))
        or (
            isinstance(item, ModelContextToolOutput)
            and isinstance(item.output, ModelToolOutputCustom)
        )
        for item in request.context
    )
    if (has_freeform_spec or has_freeform_history) and not capabilities.supports_freeform_tools:
        raise UnsupportedFeatureError("provider does not support native freeform tools")

    for item in request.context:
        if not isinstance(item, ModelContextMessage):
            continue
        for block in item.message.content_blocks:
            if isinstance(block, ImageBlock) and not capabilities.supports_image:
                raise UnsupportedFeatureError("provider does not support image input")
            if isinstance(block, DocumentBlock) and not capabilities.supports_document:
                raise UnsupportedFeatureError("provider does not support document input")
```

Append to the `BaseProvider` class body (after `validate_request`, before the abstract methods):

```python
    def validate_model_request(self, request: ModelChatRequest) -> None:
        validate_model_request(request, self.capabilities)
```

Extend the imports at the top of `provider_base.py`: add `UnsupportedFeatureError` to the `from motosan_ai.error import ...` line, and add `ModelChatRequest`, `ModelContextMessage`, `ModelContextToolCall`, `ModelContextToolOutput`, `ModelToolCallFreeform`, `ModelToolOutputCustom`, `ModelToolSpecFreeform` to the `from motosan_ai.types import (...)` block, keeping it sorted.

- [ ] **Step 4: Run tests**

Run: `cd sdks/python && uv run pytest tests/test_native_capabilities.py tests/test_provider_capabilities.py tests/test_capability_enforcement.py -q && uv run ruff check motosan_ai/ && uv run mypy motosan_ai/`
Expected: PASS — 12 + 14 + 6 tests green, ruff clean, mypy `Success: no issues found`.

- [ ] **Step 5: Commit**

```bash
git switch -c feat/freeform-python-providers   # branch off the merged P1, or off feat/freeform-python-types
git add sdks/python/motosan_ai/provider_base.py \
        sdks/python/tests/test_native_capabilities.py \
        sdks/python/tests/test_provider_capabilities.py
git commit -m "feat: add freeform capability flag and native request validation (#270)" \
           -m "Co-Authored-By: Claude Opus 5 (1M context) <noreply@anthropic.com>"
```

---

### Task 11: `collect_model_stream` in `_stream_collect.py`

**Files:**
- Modify: `sdks/python/motosan_ai/_stream_collect.py:108` (append after `collect_stream`)
- Test: `sdks/python/tests/test_native_collect_stream.py` (create)

**Interfaces:**
- Consumes: every `ModelStream*` variant, `ModelChatResponse`, `ModelToolCall`, `ModelToolCallFreeform`, `Usage`, `StopReason` (Task 3).
- Produces: `collect_model_stream(deltas: AsyncIterator[ModelStreamDelta]) -> ModelChatResponse`. Consumed by `ChatGptCodexProvider.model_chat` (Task 12) and `Client.model_stream_collect_with` (Task 14), and re-exported from the package root in Task 15.

**Traps this task must reproduce exactly (source: Rust `collect_model_stream`, `sdks/rust/src/stream.rs:155-239`, and `sdks/rust/tests/native_collect_stream.rs`):**
1. `ToolCallDone` is **authoritative**. The matching accumulator entry is discarded, never merged into the returned call. A Freeform call whose deltas said `console.` + `log(1);` and whose `ToolCallDone` said something else returns what `ToolCallDone` said.
2. `Usage` **replaces**; it does not merge field-by-field the way the legacy `collect_stream` does at `_stream_collect.py:72-86`. This asymmetry is deliberate — Responses reports usage once, at the terminal.
3. `ThinkingDone` wins over accumulated deltas and clears the delta buffer, so a second thinking block starts fresh. An explicitly empty `ThinkingDone` resolves to `thinking=None`.
4. `model` is left empty — the stream carries no model name; the provider or `Client` backfills it.
5. Errors raised by the source iterator propagate uncollected (the M1 fallible-stream contract).

- [ ] **Step 1: Write the failing test**

Create `sdks/python/tests/test_native_collect_stream.py`:

```python
from __future__ import annotations

from collections.abc import AsyncIterator

import pytest

from motosan_ai._stream_collect import collect_model_stream
from motosan_ai.error import StreamError
from motosan_ai.types import (
    ModelStreamDelta,
    ModelStreamDone,
    ModelStreamFreeformInput,
    ModelStreamFunctionArguments,
    ModelStreamText,
    ModelStreamThinkingDelta,
    ModelStreamThinkingDone,
    ModelStreamToolCallDone,
    ModelStreamUsage,
    ModelToolCallFreeform,
    ModelToolCallFunction,
    StopReason,
    Usage,
)


async def _stream(*deltas: ModelStreamDelta) -> AsyncIterator[ModelStreamDelta]:
    for delta in deltas:
        yield delta


async def test_preserves_freeform_tool_call_and_usage():
    response = await collect_model_stream(
        _stream(
            ModelStreamFreeformInput(call_id="call_js", delta="console."),
            ModelStreamFreeformInput(call_id="call_js", delta="log(1);"),
            ModelStreamToolCallDone(
                call=ModelToolCallFreeform(
                    id="call_js", name="exec", input="console.log(1);"
                )
            ),
            ModelStreamUsage(usage=Usage(input_tokens=2, output_tokens=3)),
            ModelStreamDone(stop_reason=StopReason.tool_use),
        )
    )

    assert response.tool_calls == [
        ModelToolCallFreeform(id="call_js", name="exec", input="console.log(1);")
    ]
    assert response.stop_reason == StopReason.tool_use
    assert response.usage.output_tokens == 3
    assert response.model == ""
    assert response.content == ""
    assert response.thinking is None


async def test_tool_call_done_is_authoritative_over_accumulated_deltas():
    response = await collect_model_stream(
        _stream(
            ModelStreamFreeformInput(call_id="call_js", delta="WRONG"),
            ModelStreamFunctionArguments(call_id="call_fn", delta="ALSO WRONG"),
            ModelStreamToolCallDone(
                call=ModelToolCallFreeform(id="call_js", name="exec", input="RIGHT")
            ),
            ModelStreamToolCallDone(
                call=ModelToolCallFunction(id="call_fn", name="sum", arguments='{"a":1}')
            ),
            ModelStreamDone(stop_reason=StopReason.tool_use),
        )
    )

    assert response.tool_calls == [
        ModelToolCallFreeform(id="call_js", name="exec", input="RIGHT"),
        ModelToolCallFunction(id="call_fn", name="sum", arguments='{"a":1}'),
    ]


async def test_usage_replaces_rather_than_merges():
    response = await collect_model_stream(
        _stream(
            ModelStreamUsage(
                usage=Usage(input_tokens=100, output_tokens=100, cache_read_input_tokens=7)
            ),
            ModelStreamUsage(usage=Usage(input_tokens=0, output_tokens=5)),
            ModelStreamDone(stop_reason=StopReason.end_turn),
        )
    )
    assert response.usage == Usage(input_tokens=0, output_tokens=5)
    assert response.usage.cache_read_input_tokens is None


async def test_text_and_thinking_assembly():
    response = await collect_model_stream(
        _stream(
            ModelStreamThinkingDelta(delta="think "),
            ModelStreamThinkingDelta(delta="hard"),
            ModelStreamThinkingDone(thinking="think hard"),
            ModelStreamText(delta="ans"),
            ModelStreamText(delta="wer"),
            ModelStreamDone(stop_reason=StopReason.end_turn),
        )
    )
    assert response.content == "answer"
    assert response.thinking == "think hard"


async def test_thinking_done_does_not_duplicate_accumulated_deltas():
    response = await collect_model_stream(
        _stream(
            ModelStreamThinkingDelta(delta="same"),
            ModelStreamThinkingDone(thinking="same"),
            ModelStreamDone(stop_reason=StopReason.end_turn),
        )
    )
    assert response.thinking == "same"


async def test_thinking_falls_back_to_deltas_and_empty_done_means_none():
    from_deltas = await collect_model_stream(
        _stream(
            ModelStreamThinkingDelta(delta="A "),
            ModelStreamThinkingDelta(delta="B"),
            ModelStreamDone(stop_reason=StopReason.end_turn),
        )
    )
    assert from_deltas.thinking == "A B"

    empty_done = await collect_model_stream(
        _stream(
            ModelStreamThinkingDelta(delta="discarded"),
            ModelStreamThinkingDone(thinking=""),
            ModelStreamDone(stop_reason=StopReason.end_turn),
        )
    )
    assert empty_done.thinking is None


async def test_stop_reason_heuristic_only_applies_without_a_terminal():
    no_terminal = await collect_model_stream(_stream(ModelStreamText(delta="hi")))
    assert no_terminal.stop_reason == StopReason.end_turn

    tool_no_terminal = await collect_model_stream(
        _stream(
            ModelStreamToolCallDone(
                call=ModelToolCallFunction(id="c", name="n", arguments="{}")
            )
        )
    )
    assert tool_no_terminal.stop_reason == StopReason.tool_use

    explicit = await collect_model_stream(
        _stream(
            ModelStreamToolCallDone(
                call=ModelToolCallFunction(id="c", name="n", arguments="{}")
            ),
            ModelStreamDone(stop_reason=StopReason.max_tokens),
        )
    )
    assert explicit.stop_reason == StopReason.max_tokens


async def test_stream_errors_propagate_uncollected():
    async def _boom() -> AsyncIterator[ModelStreamDelta]:
        yield ModelStreamText(delta="partial")
        raise StreamError("boom")

    with pytest.raises(StreamError, match="boom"):
        await collect_model_stream(_boom())
```

- [ ] **Step 2: Run it to verify it fails**

Run: `cd sdks/python && uv run pytest tests/test_native_collect_stream.py -q`
Expected: FAIL with `ImportError: cannot import name 'collect_model_stream' from 'motosan_ai._stream_collect'` — collection error, 0 tests run.

- [ ] **Step 3: Implement**

Append to `sdks/python/motosan_ai/_stream_collect.py`:

```python


async def collect_model_stream(deltas: AsyncIterator[ModelStreamDelta]) -> ModelChatResponse:
    """Collect a native model stream into one ModelChatResponse.

    Unlike ``collect_stream``, this carries native function and freeform tool
    calls without lowering freeform input into JSON arguments.

    Three rules differ from the legacy collector and are contract, not taste:
    ``ModelStreamToolCallDone`` is authoritative (accumulated deltas are
    discarded, never merged); ``ModelStreamUsage`` REPLACES rather than
    merging field-by-field; and ``model`` is left empty for the caller to
    backfill. A mid-stream provider error raised by ``deltas`` propagates out
    uncollected.
    """
    content = ""
    thinking_delta_buf = ""
    thinking_done_buf: str | None = None
    function_arguments: dict[str, str] = {}
    freeform_inputs: dict[str, str] = {}
    tool_calls: list[ModelToolCall] = []
    usage = Usage(0, 0)
    explicit_stop_reason: StopReason | None = None

    async for delta in deltas:
        if isinstance(delta, ModelStreamText):
            content += delta.delta
        elif isinstance(delta, ModelStreamThinkingDelta):
            thinking_delta_buf += delta.delta
        elif isinstance(delta, ModelStreamThinkingDone):
            # Carries the full text; wins over the accumulator. Clearing the
            # delta buffer lets a second block start fresh.
            thinking_done_buf = delta.thinking
            thinking_delta_buf = ""
        elif isinstance(delta, ModelStreamFunctionArguments):
            function_arguments[delta.call_id] = (
                function_arguments.get(delta.call_id, "") + delta.delta
            )
        elif isinstance(delta, ModelStreamFreeformInput):
            freeform_inputs[delta.call_id] = (
                freeform_inputs.get(delta.call_id, "") + delta.delta
            )
        elif isinstance(delta, ModelStreamToolCallDone):
            # Authoritative: drop the bookkeeping, keep what the provider sent.
            if isinstance(delta.call, ModelToolCallFreeform):
                freeform_inputs.pop(delta.call.id, None)
            else:
                function_arguments.pop(delta.call.id, None)
            tool_calls.append(delta.call)
        elif isinstance(delta, ModelStreamUsage):
            # Replaces, never merges.
            usage = delta.usage
        elif isinstance(delta, ModelStreamDone):
            explicit_stop_reason = delta.stop_reason
            break

    if thinking_done_buf is not None:
        thinking = thinking_done_buf or None
    else:
        thinking = thinking_delta_buf or None

    if explicit_stop_reason is not None:
        stop_reason = explicit_stop_reason
    else:
        stop_reason = StopReason.tool_use if tool_calls else StopReason.end_turn

    return ModelChatResponse(
        content=content,
        thinking=thinking,
        tool_calls=tool_calls,
        model="",
        usage=usage,
        stop_reason=stop_reason,
        session_id=None,
    )
```

Extend the `from motosan_ai.types import (...)` block at the top of `_stream_collect.py` with `ModelChatResponse`, `ModelStreamDelta`, `ModelStreamDone`, `ModelStreamFreeformInput`, `ModelStreamFunctionArguments`, `ModelStreamText`, `ModelStreamThinkingDelta`, `ModelStreamThinkingDone`, `ModelStreamToolCallDone`, `ModelStreamUsage`, `ModelToolCall`, `ModelToolCallFreeform`, keeping it sorted.

- [ ] **Step 4: Run tests**

Run: `cd sdks/python && uv run pytest tests/test_native_collect_stream.py -q && uv run ruff check motosan_ai/ && uv run ruff format --check motosan_ai/ tests/ && uv run mypy motosan_ai/`
Expected: PASS — 8 passed, ruff clean, format check clean, mypy `Success: no issues found`.

- [ ] **Step 5: Commit**

```bash
git add sdks/python/motosan_ai/_stream_collect.py sdks/python/tests/test_native_collect_stream.py
git commit -m "feat: add collect_model_stream to the Python SDK (#270)" \
           -m "Co-Authored-By: Claude Opus 5 (1M context) <noreply@anthropic.com>"
```

---

### Task 12: ChatGPT Codex native methods

**Files:**
- Modify: `sdks/python/motosan_ai/providers/chatgpt_codex.py:200` (the `capabilities` class attribute)
- Modify: `sdks/python/motosan_ai/providers/chatgpt_codex.py:379` (append `build_model_responses_body` after `_build_responses_body`)
- Modify: `sdks/python/motosan_ai/providers/chatgpt_codex.py:459` (append `model_stream` and `model_chat` after `chat`)
- Test: `sdks/python/tests/test_chatgpt_codex_native.py` (create)

**Interfaces:**
- Consumes: `build_model_request_body`, `parse_model_sse_event`, `ModelStreamState` (Tasks 7, 8); `collect_model_stream` (Task 11); `ProviderCapabilities.with_freeform_tools`, `BaseProvider.validate_model_request` (Task 10); the provider's existing `_bearer`, `_headers`, `_stream_url`, `_map_http_error`, `_reasoning_effort`, `_read_idle_timeout`.
- Produces: `ChatGptCodexProvider.build_model_responses_body(request) -> dict[str, Any]`, `ChatGptCodexProvider.model_stream(request) -> AsyncIterator[ModelStreamDelta]`, `ChatGptCodexProvider.model_chat(request) -> ModelChatResponse`. `Client` duck-types both methods in Task 14.

**Traps this task must reproduce exactly:**
1. **The body overrides the caller.** `store=False`, `include=["reasoning.encrypted_content"]`, `parallel_tool_calls=True`, and `tool_choice="auto"` are set **after** the codec ran — so they beat `provider_options` and they beat an explicit `request.tool_choice`.
2. **`reasoning_effort` normalization, both halves.** Effort resolves as per-request `provider_options["reasoning_effort"]` **first**, provider default second, omitted if neither. When one resolves, the body gets `reasoning = {"effort": <value>, "summary": "auto"}` **and** the raw top-level `reasoning_effort` key is removed — because Task 7's shallow merge injected it.
3. **The provider string is `chatgpt-codex`, with a hyphen.** The legacy adapter at `chatgpt_codex.py:433` raises `"incomplete stream: chatgpt_codex ended without a terminal event"` with an **underscore**. The native path must not copy that; `specs/types.md` pins the hyphen.
4. `model_chat` is `model_stream` + collect, because this provider has no non-streaming endpoint. It backfills `model` when the collected value is empty.
5. `capabilities` flips from `text_only()` to `with_freeform_tools()`. Image and document support stay `False`, so nothing on the legacy path changes.

- [ ] **Step 1: Write the failing test**

Create `sdks/python/tests/test_chatgpt_codex_native.py`:

```python
from __future__ import annotations

import json

import httpx
import pytest
import respx

from motosan_ai.error import IncompleteStreamError, ProviderError, StreamError
from motosan_ai.provider_base import ProviderCapabilities
from motosan_ai.providers.chatgpt_codex import ChatGptCodexProvider
from motosan_ai.types import (
    FreeformTool,
    FreeformToolFormat,
    FunctionCallOutputText,
    Message,
    ModelChatRequest,
    ModelToolCallFreeform,
    ModelToolOutputCustom,
    ModelToolSpecFreeform,
    StopReason,
    ToolChoice,
)

_URL = "https://chatgpt.com/backend-api/codex/responses"

FREEFORM_SPEC = ModelToolSpecFreeform(
    tool=FreeformTool(
        name="exec",
        description="Run JavaScript",
        format=FreeformToolFormat(type="grammar", syntax="lark", definition="start: source"),
    )
)


def _provider() -> ChatGptCodexProvider:
    return ChatGptCodexProvider("oauth-token", "acct-123", "gpt-5.5", None)


def _native_request() -> ModelChatRequest:
    return ModelChatRequest.builder().message(Message.user("run js")).tool_spec(FREEFORM_SPEC).build()


def _sse(*payloads: dict) -> str:
    return "\n".join(f"data: {json.dumps(p)}" for p in payloads) + "\n"


def test_capabilities_declare_freeform_but_not_image_or_document():
    assert _provider().capabilities == ProviderCapabilities.with_freeform_tools()


def test_native_body_has_the_codex_hard_overrides():
    body = _provider().build_model_responses_body(_native_request())

    assert body["model"] == "gpt-5.5"
    assert body["stream"] is True
    assert body["store"] is False
    assert body["include"] == ["reasoning.encrypted_content"]
    assert body["parallel_tool_calls"] is True
    assert body["tool_choice"] == "auto"
    assert body["instructions"] == "You are a helpful assistant."
    assert body["tools"][0]["type"] == "custom"
    assert body["tools"][0]["format"]["syntax"] == "lark"


def test_native_body_tool_choice_override_beats_the_caller():
    request = (
        ModelChatRequest.builder()
        .message(Message.user("hi"))
        .tool_choice(ToolChoice.required())
        .build()
    )
    assert _provider().build_model_responses_body(request)["tool_choice"] == "auto"


def test_native_body_per_request_effort_beats_the_provider_default():
    provider = _provider().reasoning_effort("low")
    request = (
        ModelChatRequest.builder()
        .message(Message.user("hi"))
        .provider_options({"reasoning_effort": "high"})
        .build()
    )
    body = provider.build_model_responses_body(request)

    assert body["reasoning"] == {"effort": "high", "summary": "auto"}
    # The shallow merge injected the raw key; it must never reach the wire.
    assert "reasoning_effort" not in body


def test_native_body_falls_back_to_the_provider_default_effort():
    body = _provider().reasoning_effort("medium").build_model_responses_body(
        ModelChatRequest.builder().message(Message.user("hi")).build()
    )
    assert body["reasoning"] == {"effort": "medium", "summary": "auto"}


def test_native_body_omits_reasoning_when_no_effort_resolves():
    body = _provider().build_model_responses_body(
        ModelChatRequest.builder().message(Message.user("hi")).build()
    )
    assert "reasoning" not in body
    assert "reasoning_effort" not in body


def test_native_body_hoists_system_messages_into_instructions():
    request = (
        ModelChatRequest.builder()
        .message(Message.system("be terse"))
        .message(Message.user("hi"))
        .build()
    )
    body = _provider().build_model_responses_body(request)
    assert body["instructions"] == "be terse"
    assert len(body["input"]) == 1
    assert body["input"][0]["role"] == "user"


@respx.mock
async def test_native_stream_decodes_custom_delta_and_done():
    respx.post(_URL).mock(
        return_value=httpx.Response(
            200,
            text=_sse(
                {
                    "type": "response.custom_tool_call_input.delta",
                    "call_id": "call_js",
                    "delta": "console.",
                },
                {
                    "type": "response.custom_tool_call_input.delta",
                    "call_id": "call_js",
                    "delta": "log(1);\n",
                },
                {
                    "type": "response.output_item.done",
                    "item": {
                        "type": "custom_tool_call",
                        "call_id": "call_js",
                        "name": "exec",
                        "input": "console.log(1);\n",
                    },
                },
                {
                    "type": "response.completed",
                    "response": {
                        "status": "completed",
                        "usage": {"input_tokens": 2, "output_tokens": 3},
                    },
                },
            ),
            headers={"content-type": "text/event-stream"},
        )
    )

    response = await _provider().model_chat(_native_request())

    assert response.tool_calls == [
        ModelToolCallFreeform(id="call_js", name="exec", input="console.log(1);\n")
    ]
    assert response.stop_reason == StopReason.tool_use
    assert response.usage.output_tokens == 3
    assert response.model == "gpt-5.5"


@respx.mock
async def test_native_stream_maps_response_incomplete_to_max_tokens():
    respx.post(_URL).mock(
        return_value=httpx.Response(
            200,
            text=_sse(
                {"type": "response.output_text.delta", "delta": "partial"},
                {
                    "type": "response.incomplete",
                    "response": {
                        "status": "incomplete",
                        "usage": {"input_tokens": 6, "output_tokens": 7},
                        "incomplete_details": {"reason": "max_output_tokens"},
                    },
                },
            ),
            headers={"content-type": "text/event-stream"},
        )
    )

    response = await _provider().model_chat(
        ModelChatRequest.builder().message(Message.user("short")).build()
    )
    assert response.content == "partial"
    assert response.stop_reason == StopReason.max_tokens
    assert response.usage.output_tokens == 7


@respx.mock
async def test_native_stream_eof_without_terminal_is_incomplete():
    respx.post(_URL).mock(
        return_value=httpx.Response(
            200,
            text=_sse(
                {
                    "type": "response.custom_tool_call_input.delta",
                    "call_id": "call_js",
                    "delta": "console.",
                }
            ),
            headers={"content-type": "text/event-stream"},
        )
    )

    # Note the hyphen: the native provider string is `chatgpt-codex`, not the
    # legacy adapter's `chatgpt_codex`.
    with pytest.raises(
        IncompleteStreamError,
        match="incomplete stream: chatgpt-codex ended without a terminal event",
    ):
        async for _ in _provider().model_stream(_native_request()):
            pass
    assert issubclass(IncompleteStreamError, StreamError)


@respx.mock
async def test_native_stream_sends_history_byte_exact():
    captured: dict = {}
    raw = '{"this":"looks like json"}\nconsole.log(\'but is JS\');'

    def _capture(request):
        captured["body"] = json.loads(request.content)
        captured["headers"] = dict(request.headers)
        return httpx.Response(
            200,
            text=_sse(
                {
                    "type": "response.completed",
                    "response": {
                        "status": "completed",
                        "usage": {"input_tokens": 1, "output_tokens": 1},
                    },
                }
            ),
            headers={"content-type": "text/event-stream"},
        )

    respx.post(_URL).mock(side_effect=_capture)

    request = (
        ModelChatRequest.builder()
        .message(Message.user("run js"))
        .tool_call(ModelToolCallFreeform(id="call_js", name="exec", input=raw))
        .tool_output(
            ModelToolOutputCustom(
                call_id="call_js", output=FunctionCallOutputText(text="done"), name="exec"
            )
        )
        .tool_spec(FREEFORM_SPEC)
        .build()
    )
    response = await _provider().model_chat(request)

    assert response.stop_reason == StopReason.end_turn
    body = captured["body"]
    assert [item["type"] for item in body["input"]] == [
        "message",
        "custom_tool_call",
        "custom_tool_call_output",
    ]
    assert body["input"][1]["input"] == raw
    assert body["tools"][0]["type"] == "custom"
    assert captured["headers"]["authorization"] == "Bearer oauth-token"
    assert captured["headers"]["chatgpt-account-id"] == "acct-123"
    assert captured["headers"]["openai-beta"] == "responses=experimental"


@respx.mock
async def test_native_stream_maps_http_errors():
    respx.post(_URL).mock(return_value=httpx.Response(500, text="boom"))
    with pytest.raises(ProviderError, match="HTTP 500"):
        async for _ in _provider().model_stream(_native_request()):
            pass


@respx.mock
async def test_native_stream_raises_on_a_stream_error_frame():
    respx.post(_URL).mock(
        return_value=httpx.Response(
            200,
            text=_sse(
                {"type": "response.output_text.delta", "delta": "partial"},
                {"type": "response.failed", "response": {"error": {"message": "upstream died"}}},
            ),
            headers={"content-type": "text/event-stream"},
        )
    )

    seen = []
    with pytest.raises(StreamError, match="upstream died"):
        async for delta in _provider().model_stream(_native_request()):
            seen.append(delta)
    # Pending deltas drain before the stored error surfaces.
    assert len(seen) == 1
```

- [ ] **Step 2: Run it to verify it fails**

Run: `cd sdks/python && uv run pytest tests/test_chatgpt_codex_native.py -q`
Expected: FAIL — 14 failed. The first failure is
`AssertionError: assert ProviderCapabilities(supports_image=False, supports_document=False, supports_freeform_tools=False) == ProviderCapabilities(supports_image=False, supports_document=False, supports_freeform_tools=True)`, and the body/stream tests fail with `AttributeError: 'ChatGptCodexProvider' object has no attribute 'build_model_responses_body'` / `'model_stream'`.

- [ ] **Step 3: Implement**

Replace `sdks/python/motosan_ai/providers/chatgpt_codex.py:200`:

```python
    capabilities: ProviderCapabilities = ProviderCapabilities.with_freeform_tools()
```

Append after `_build_responses_body` (i.e. after line 379):

```python
    def build_model_responses_body(self, request: ModelChatRequest) -> dict[str, Any]:
        """Encode a native ModelChatRequest into the ChatGPT-backend body.

        The four hard overrides below are applied AFTER the shared codec, and
        the codec already shallow-merged ``provider_options`` — so these beat
        both the caller's ``provider_options`` AND an explicit
        ``request.tool_choice``. That is deliberate Rust parity.
        """
        body = build_model_request_body(
            request,
            self.model,
            stream=True,
            default_instructions="You are a helpful assistant.",
        )
        body["store"] = False
        body["include"] = ["reasoning.encrypted_content"]
        body["tool_choice"] = "auto"
        body["parallel_tool_calls"] = True

        # Effort: per-request provider_options wins, then the provider default,
        # else the `reasoning` object is omitted entirely.
        effort: str | None = None
        if request.provider_options is not None:
            candidate = request.provider_options.get("reasoning_effort")
            if isinstance(candidate, str):
                effort = candidate
        if effort is None:
            effort = self._reasoning_effort
        if effort is not None:
            body["reasoning"] = {"effort": effort, "summary": "auto"}
            # The codec's shallow merge will have injected the raw key onto the
            # body. It must never reach the wire.
            body.pop("reasoning_effort", None)

        return body
```

Append after `chat` (i.e. after line 459):

```python
    async def model_stream(self, request: ModelChatRequest) -> AsyncIterator[ModelStreamDelta]:
        self.validate_model_request(request)
        body = self.build_model_responses_body(request)
        bearer = await self._bearer()
        try:
            resp = await self._http.send(
                self._http.build_request(
                    "POST", self._stream_url(), headers=self._headers(bearer), json=body
                ),
                stream=True,
            )
        except httpx.HTTPError as exc:
            raise NetworkError(str(exc)) from exc

        try:
            if not resp.is_success:
                error_body = await resp.aread()
                message = f"HTTP {resp.status_code}: {error_body.decode()}"
                retry_after = resp.headers.get("retry-after")
                if retry_after:
                    message = f"Retry-After: {retry_after}\n{message}"
                raise self._map_http_error(resp.status_code, message, resp.headers)

            state = ModelStreamState()
            try:
                async for line in resp.aiter_lines():
                    if not line.startswith("data: "):
                        continue
                    for delta in parse_model_sse_event(line[len("data: ") :], state):
                        yield delta
                        if isinstance(delta, ModelStreamDone):
                            return
                    if state.error is not None:
                        raise StreamError(state.error)

                # Provider string is `chatgpt-codex` (hyphen) on the native
                # path; the legacy adapter above uses `chatgpt_codex`.
                raise IncompleteStreamError(
                    "incomplete stream: chatgpt-codex ended without a terminal event"
                )
            except (StreamError, AuthError, RateLimitError, ProviderError, NetworkError):
                raise
            except httpx.ReadTimeout as exc:
                raise StreamReadTimeoutError(
                    f"stream read timed out after {self._read_idle_timeout}s"
                ) from exc
            except httpx.HTTPError as exc:
                raise StreamError(f"stream transport error: {exc}") from exc
        except httpx.ReadTimeout as exc:
            raise StreamReadTimeoutError(
                f"stream read timed out after {self._read_idle_timeout}s"
            ) from exc
        finally:
            await resp.aclose()

    async def model_chat(self, request: ModelChatRequest) -> ModelChatResponse:
        """Native chat = native stream + collect: there is no blocking endpoint."""
        response = await collect_model_stream(self.model_stream(request))
        if not response.model:
            response.model = request.model or self.model
        return response
```

Extend the imports at the top of `chatgpt_codex.py`:

```python
from motosan_ai._stream_collect import collect_model_stream, collect_stream
from motosan_ai.providers.responses import (
    ModelStreamState,
    build_model_request_body,
    parse_model_sse_event,
)
```

and add `ModelChatRequest`, `ModelChatResponse`, `ModelStreamDelta`, `ModelStreamDone` to the existing `from motosan_ai.types import (...)` block, keeping it sorted.

- [ ] **Step 4: Run tests**

Run: `cd sdks/python && uv run pytest tests/test_chatgpt_codex_native.py tests/test_chatgpt_codex_http.py tests/test_chatgpt_codex_request.py tests/test_chatgpt_codex_stream.py tests/test_chatgpt_codex_dispatch.py -q && uv run ruff check motosan_ai/ && uv run mypy motosan_ai/`
Expected: PASS — 14 new tests green and every pre-existing Codex test still green (the capability flip changes only `supports_freeform_tools`, so the legacy path is untouched), ruff clean, mypy `Success: no issues found`.

- [ ] **Step 5: Commit**

```bash
git add sdks/python/motosan_ai/providers/chatgpt_codex.py sdks/python/tests/test_chatgpt_codex_native.py
git commit -m "feat: add native model methods to the Python ChatGPT Codex provider (#270)" \
           -m "Co-Authored-By: Claude Opus 5 (1M context) <noreply@anthropic.com>"
```

---

### Task 13: OpenAI Responses opt-in and native methods

**Files:**
- Modify: `sdks/python/motosan_ai/providers/openai.py:53-89` (class attribute, `__init__`, endpoints)
- Modify: `sdks/python/motosan_ai/providers/openai.py:377` (append `validate_model_request`, `model_chat`, `model_stream`)
- Test: `sdks/python/tests/test_openai_native.py` (create)

**Interfaces:**
- Consumes: `build_model_request_body`, `model_chat_response_from_output`, `parse_model_sse_event`, `ModelStreamState` (Tasks 6, 7, 8); `validate_model_request`, `ProviderCapabilities.with_image_and_freeform_tools` (Task 10); `UnsupportedFeatureError` (Task 2); the provider's existing `_headers`, `_map_http_error`, `_response_error_message`, `_read_idle_timeout`.
- Produces: `OpenAIProvider(..., responses_api: bool = False, responses_url: str | None = None)`, `OpenAIProvider._responses_endpoint()`, `OpenAIProvider.validate_model_request(request)`, `OpenAIProvider.model_chat(request) -> ModelChatResponse`, `OpenAIProvider.model_stream(request) -> AsyncIterator[ModelStreamDelta]`. Threaded from `Client` in Task 14.

**Decision context (D1):** OpenAI's Python provider speaks only `/v1/chat/completions` today — the Responses path is genuinely new code. `OpenAIProvider` does **not** subclass `BaseProvider` (D6), so it needs its own `validate_model_request` method delegating to the module-level function; a default on the ABC would never reach it.

**Traps this task must reproduce exactly:**
1. **Validation runs before the opt-in check.** Rust's `model_chat` calls `validate_model_request` first, then rejects on `!responses_api`. Because capabilities without the opt-in are `with_image()` (freeform `False`), a request carrying freeform specs gets `"provider does not support native freeform tools"` — which is what the Rust test `native_custom_openai_chat_completions_rejects_before_http` asserts (`msg.contains("freeform")`). Reversing the order changes the message and breaks parity.
2. **No HTTP happens on rejection.** Both rejection paths must fire before any request is built.
3. `model_chat` is a genuine **non-streaming** POST decoded by `model_chat_response_from_output` — unlike Codex, which streams and collects.
4. The EOF payload is exactly `"incomplete stream: openai ended without a terminal event"`.
5. `capabilities` becomes an **instance** attribute because it depends on the opt-in; the class-level default stays for providers constructed without it.

- [ ] **Step 1: Write the failing test**

Create `sdks/python/tests/test_openai_native.py`:

```python
from __future__ import annotations

import json

import httpx
import pytest
import respx

from motosan_ai.error import IncompleteStreamError, ProviderError, StreamError, UnsupportedFeatureError
from motosan_ai.provider_base import ProviderCapabilities
from motosan_ai.providers.openai import OpenAIProvider
from motosan_ai.types import (
    FreeformTool,
    FreeformToolFormat,
    FunctionCallOutputText,
    Message,
    ModelChatRequest,
    ModelToolCallFreeform,
    ModelToolOutputCustom,
    ModelToolSpecFreeform,
    StopReason,
)

_RESPONSES = "https://api.openai.com/v1/responses"

FREEFORM_SPEC = ModelToolSpecFreeform(
    tool=FreeformTool(
        name="exec",
        description="Run JavaScript",
        format=FreeformToolFormat(type="grammar", syntax="lark", definition="start: source"),
    )
)


def _native_provider() -> OpenAIProvider:
    return OpenAIProvider(api_key="test-key", model="gpt-5.5-codex", responses_api=True)


def _native_request() -> ModelChatRequest:
    return (
        ModelChatRequest.builder()
        .model("gpt-5.5-codex")
        .message(Message.user("run js"))
        .tool_spec(FREEFORM_SPEC)
        .build()
    )


def _sse(*payloads: dict) -> str:
    return "\n".join(f"data: {json.dumps(p)}" for p in payloads) + "\n"


def test_capabilities_switch_on_the_opt_in():
    assert OpenAIProvider(api_key="k").capabilities == ProviderCapabilities.with_image()
    assert (
        OpenAIProvider(api_key="k", responses_api=True).capabilities
        == ProviderCapabilities.with_image_and_freeform_tools()
    )


def test_responses_endpoint_defaults_and_override():
    assert OpenAIProvider(api_key="k")._responses_endpoint() == _RESPONSES
    assert (
        OpenAIProvider(api_key="k", base_url="https://proxy.test/")._responses_endpoint()
        == "https://proxy.test/v1/responses"
    )
    assert (
        OpenAIProvider(
            api_key="k", responses_url="https://mock.test/v1/responses/"
        )._responses_endpoint()
        == "https://mock.test/v1/responses"
    )
    # The chat endpoint is untouched by the opt-in.
    assert OpenAIProvider(api_key="k")._endpoint() == "https://api.openai.com/v1/chat/completions"


@respx.mock
async def test_chat_completions_rejects_freeform_before_any_http():
    route = respx.post(host="api.openai.com").mock(return_value=httpx.Response(500))
    with pytest.raises(UnsupportedFeatureError, match="freeform"):
        await OpenAIProvider(api_key="k").model_chat(_native_request())
    assert route.call_count == 0


@respx.mock
async def test_chat_completions_rejects_freeform_streams_before_any_http():
    route = respx.post(host="api.openai.com").mock(return_value=httpx.Response(500))
    with pytest.raises(UnsupportedFeatureError, match="freeform"):
        async for _ in OpenAIProvider(api_key="k").model_stream(_native_request()):
            pass
    assert route.call_count == 0


@respx.mock
async def test_chat_completions_rejects_plain_native_requests_with_the_opt_in_message():
    route = respx.post(host="api.openai.com").mock(return_value=httpx.Response(500))
    plain = ModelChatRequest.builder().message(Message.user("hi")).build()
    with pytest.raises(UnsupportedFeatureError, match="enable OpenAI Responses API"):
        await OpenAIProvider(api_key="k").model_chat(plain)
    with pytest.raises(UnsupportedFeatureError, match="enable OpenAI Responses API"):
        async for _ in OpenAIProvider(api_key="k").model_stream(plain):
            pass
    assert route.call_count == 0


@respx.mock
async def test_native_chat_posts_a_non_streaming_body_and_decodes_custom_calls():
    captured: dict = {}
    raw = "const x = {a: 1};\nconsole.log(x.a);\n"

    def _capture(request):
        captured["body"] = json.loads(request.content)
        captured["headers"] = dict(request.headers)
        return httpx.Response(
            200,
            json={
                "model": "gpt-5.5-codex",
                "status": "completed",
                "output": [
                    {
                        "type": "custom_tool_call",
                        "call_id": "call_js",
                        "name": "exec",
                        "input": raw,
                    }
                ],
                "usage": {"input_tokens": 9, "output_tokens": 7},
            },
        )

    respx.post(_RESPONSES).mock(side_effect=_capture)

    response = await _native_provider().model_chat(_native_request())

    assert response.tool_calls == [
        ModelToolCallFreeform(id="call_js", name="exec", input=raw)
    ]
    assert response.stop_reason == StopReason.tool_use
    assert response.usage.input_tokens == 9
    assert captured["headers"]["authorization"] == "Bearer test-key"
    assert "stream" not in captured["body"]
    assert captured["body"]["tools"][0]["type"] == "custom"
    assert captured["body"]["tools"][0]["format"]["definition"] == "start: source"


@respx.mock
async def test_native_chat_encodes_image_blocks():
    captured: dict = {}

    def _capture(request):
        captured["body"] = json.loads(request.content)
        return httpx.Response(
            200,
            json={
                "model": "gpt-5.5-codex",
                "status": "completed",
                "output": [
                    {
                        "type": "message",
                        "role": "assistant",
                        "content": [{"type": "output_text", "text": "ok"}],
                    }
                ],
                "usage": {"input_tokens": 1, "output_tokens": 1},
            },
        )

    respx.post(_RESPONSES).mock(side_effect=_capture)
    request = ModelChatRequest.builder().message(
        Message.user_with_image("inspect", "abc123", "image/png")
    ).build()

    response = await _native_provider().model_chat(request)

    assert response.content == "ok"
    content = captured["body"]["input"][0]["content"]
    assert content[0] == {"type": "input_text", "text": "inspect"}
    assert content[1] == {"type": "input_image", "image_url": "data:image/png;base64,abc123"}


@respx.mock
async def test_native_chat_replays_symmetric_history_byte_exact():
    captured: dict = {}
    raw = '{"this":"looks like json"}\nconsole.log(\'but is JS\');'

    def _capture(request):
        captured["body"] = json.loads(request.content)
        return httpx.Response(
            200,
            json={
                "model": "gpt-5.5-codex",
                "status": "completed",
                "output": [
                    {
                        "type": "message",
                        "role": "assistant",
                        "content": [{"type": "output_text", "text": "ok"}],
                    }
                ],
                "usage": {"input_tokens": 1, "output_tokens": 1},
            },
        )

    respx.post(_RESPONSES).mock(side_effect=_capture)

    request = (
        ModelChatRequest.builder()
        .message(Message.user("run js"))
        .tool_call(ModelToolCallFreeform(id="call_js", name="exec", input=raw))
        .tool_output(
            ModelToolOutputCustom(
                call_id="call_js", output=FunctionCallOutputText(text="done"), name="exec"
            )
        )
        .tool_spec(FREEFORM_SPEC)
        .build()
    )
    response = await _native_provider().model_chat(request)

    assert response.content == "ok"
    body = captured["body"]
    assert [item["type"] for item in body["input"]] == [
        "message",
        "custom_tool_call",
        "custom_tool_call_output",
    ]
    assert body["input"][1]["input"] == raw
    assert body["input"][1]["call_id"] == "call_js"
    assert body["input"][1]["name"] == "exec"


@respx.mock
async def test_native_chat_maps_http_errors():
    respx.post(_RESPONSES).mock(return_value=httpx.Response(500, text="boom"))
    with pytest.raises(ProviderError, match="HTTP 500"):
        await _native_provider().model_chat(_native_request())


@respx.mock
async def test_native_stream_decodes_custom_delta_and_done():
    from motosan_ai._stream_collect import collect_model_stream

    respx.post(_RESPONSES).mock(
        return_value=httpx.Response(
            200,
            text=_sse(
                {
                    "type": "response.custom_tool_call_input.delta",
                    "call_id": "call_js",
                    "delta": "console.",
                },
                {
                    "type": "response.custom_tool_call_input.delta",
                    "call_id": "call_js",
                    "delta": "log(1);\n",
                },
                {
                    "type": "response.output_item.done",
                    "item": {
                        "type": "custom_tool_call",
                        "call_id": "call_js",
                        "name": "exec",
                        "input": "console.log(1);\n",
                    },
                },
                {
                    "type": "response.completed",
                    "response": {
                        "status": "completed",
                        "usage": {"input_tokens": 2, "output_tokens": 3},
                    },
                },
            ),
            headers={"content-type": "text/event-stream"},
        )
    )

    response = await collect_model_stream(_native_provider().model_stream(_native_request()))

    assert response.tool_calls == [
        ModelToolCallFreeform(id="call_js", name="exec", input="console.log(1);\n")
    ]
    assert response.stop_reason == StopReason.tool_use
    assert response.usage.output_tokens == 3


@respx.mock
async def test_native_stream_sets_the_stream_flag():
    captured: dict = {}

    def _capture(request):
        captured["body"] = json.loads(request.content)
        return httpx.Response(
            200,
            text=_sse({"type": "response.completed", "response": {"status": "completed"}}),
            headers={"content-type": "text/event-stream"},
        )

    respx.post(_RESPONSES).mock(side_effect=_capture)
    async for _ in _native_provider().model_stream(_native_request()):
        pass
    assert captured["body"]["stream"] is True


@respx.mock
async def test_native_stream_eof_without_terminal_is_incomplete():
    respx.post(_RESPONSES).mock(
        return_value=httpx.Response(
            200,
            text=_sse(
                {"type": "response.output_text.delta", "delta": "hel"},
                {"type": "response.output_text.delta", "delta": "lo"},
            ),
            headers={"content-type": "text/event-stream"},
        )
    )

    seen = []
    with pytest.raises(
        IncompleteStreamError, match="incomplete stream: openai ended without a terminal event"
    ):
        async for delta in _native_provider().model_stream(_native_request()):
            seen.append(delta)
    assert len(seen) == 2
    assert issubclass(IncompleteStreamError, StreamError)
```

- [ ] **Step 2: Run it to verify it fails**

Run: `cd sdks/python && uv run pytest tests/test_openai_native.py -q`
Expected: FAIL with `ImportError: cannot import name 'UnsupportedFeatureError' from 'motosan_ai.error'` if Task 2 is missing, otherwise
`TypeError: OpenAIProvider.__init__() got an unexpected keyword argument 'responses_api'` — 13 failed.

- [ ] **Step 3: Implement**

Replace `sdks/python/motosan_ai/providers/openai.py:53-83` with:

```python
class OpenAIProvider:
    # Class-level default; __init__ replaces it per instance because the
    # native capability depends on the Responses opt-in.
    capabilities: ProviderCapabilities = ProviderCapabilities.with_image()

    def __init__(
        self,
        api_key: str,
        model: str | None = None,
        base_url: str | None = None,
        *,
        responses_api: bool = False,
        responses_url: str | None = None,
        connect_timeout: float = 10.0,
        read_idle_timeout: float = 120.0,
    ) -> None:
        self.api_key = api_key
        self.model = model or "gpt-4o"
        self.base_url = (base_url or _DEFAULT_BASE_URL).rstrip("/")
        self.responses_api = responses_api
        self.responses_url = responses_url.rstrip("/") if responses_url else None
        self.capabilities = (
            ProviderCapabilities.with_image_and_freeform_tools()
            if responses_api
            else ProviderCapabilities.with_image()
        )
        self._read_idle_timeout = read_idle_timeout
        self._http = httpx.AsyncClient(
            timeout=httpx.Timeout(
                connect=connect_timeout,
                read=read_idle_timeout,
                write=read_idle_timeout,
                pool=connect_timeout,
            )
        )

    async def aclose(self) -> None:
        """Close the underlying HTTP connection pool."""
        await self._http.aclose()

    def _endpoint(self) -> str:
        return f"{self.base_url}/v1/chat/completions"

    def _responses_endpoint(self) -> str:
        return self.responses_url or f"{self.base_url}/v1/responses"
```

Append at the end of the class (after `stream`, i.e. after line 377):

```python
    def validate_model_request(self, request: ModelChatRequest) -> None:
        # OpenAIProvider does NOT subclass BaseProvider, so it carries its own
        # method delegating to the shared validator.
        _validate_model_request(request, self.capabilities)

    async def model_chat(self, request: ModelChatRequest) -> ModelChatResponse:
        # Order matters: validation first, so a freeform request without the
        # opt-in reports "native freeform tools", not the opt-in message.
        self.validate_model_request(request)
        if not self.responses_api:
            raise UnsupportedFeatureError(
                "OpenAI Chat Completions does not support native model requests; "
                "enable OpenAI Responses API"
            )

        body = build_model_request_body(request, self.model, stream=False)
        try:
            resp = await self._http.post(
                self._responses_endpoint(), headers=self._headers(), json=body
            )
        except httpx.HTTPError as exc:
            raise NetworkError(str(exc)) from exc

        if not resp.is_success:
            message = self._response_error_message(resp.status_code, resp.headers, resp.text)
            raise self._map_http_error(resp.status_code, message, resp.headers)

        return model_chat_response_from_output(resp.json(), self.model)

    async def model_stream(self, request: ModelChatRequest) -> AsyncIterator[ModelStreamDelta]:
        self.validate_model_request(request)
        if not self.responses_api:
            raise UnsupportedFeatureError(
                "OpenAI Chat Completions does not support native model streams; "
                "enable OpenAI Responses API"
            )

        body = build_model_request_body(request, self.model, stream=True)
        try:
            resp = await self._http.send(
                self._http.build_request(
                    "POST", self._responses_endpoint(), headers=self._headers(), json=body
                ),
                stream=True,
            )
        except httpx.HTTPError as exc:
            raise NetworkError(str(exc)) from exc

        try:
            if not resp.is_success:
                error_body = await resp.aread()
                message = self._response_error_message(
                    resp.status_code, resp.headers, error_body.decode()
                )
                raise self._map_http_error(resp.status_code, message, resp.headers)

            state = ModelStreamState()
            async for line in resp.aiter_lines():
                if not line.startswith("data: "):
                    continue
                for delta in parse_model_sse_event(line[len("data: ") :], state):
                    yield delta
                    if isinstance(delta, ModelStreamDone):
                        return
                if state.error is not None:
                    raise StreamError(state.error)

            raise IncompleteStreamError("incomplete stream: openai ended without a terminal event")
        except StreamError:
            raise
        except (AuthError, RateLimitError, ProviderError, NetworkError):
            raise
        except httpx.ReadTimeout as exc:
            raise StreamReadTimeoutError(
                f"stream read timed out after {self._read_idle_timeout}s"
            ) from exc
        except httpx.HTTPError as exc:
            raise StreamError(f"stream transport error: {exc}") from exc
        finally:
            await resp.aclose()
```

Extend the imports at the top of `openai.py`:

```python
from motosan_ai.error import (
    AuthError,
    IncompleteStreamError,
    NetworkError,
    ProviderError,
    RateLimitError,
    StreamError,
    StreamReadTimeoutError,
    UnsupportedFeatureError,
)
from motosan_ai.provider_base import ProviderCapabilities
from motosan_ai.provider_base import validate_model_request as _validate_model_request
from motosan_ai.providers.responses import (
    ModelStreamState,
    build_model_request_body,
    model_chat_response_from_output,
    parse_model_sse_event,
)
```

and add `ModelChatRequest`, `ModelChatResponse`, `ModelStreamDelta`, `ModelStreamDone` to the existing `from motosan_ai.types import (...)` block, keeping it sorted.

- [ ] **Step 4: Run tests**

Run: `cd sdks/python && uv run pytest tests/test_openai_native.py tests/test_openai.py tests/test_openai_vision.py tests/test_ollama.py -q && uv run ruff check motosan_ai/ && uv run mypy motosan_ai/`
Expected: PASS — 13 new tests green and every pre-existing OpenAI / Ollama-over-OpenAI test still green (`responses_api` defaults to `False`, so the chat-completions path is unchanged), ruff clean, mypy `Success: no issues found`.

- [ ] **Step 5: Commit**

```bash
git add sdks/python/motosan_ai/providers/openai.py sdks/python/tests/test_openai_native.py
git commit -m "feat: add the OpenAI Responses opt-in and native methods to Python (#270)" \
           -m "Co-Authored-By: Claude Opus 5 (1M context) <noreply@anthropic.com>"
```

---

### Task 14: `Client` native trio, duck-typed dispatch, and the `openai_responses_api` flag

**Files:**
- Modify: `sdks/python/motosan_ai/client.py:68-91` (`__init__` signature)
- Modify: `sdks/python/motosan_ai/client.py:190-197` (the `Provider.openai` construction branch)
- Modify: `sdks/python/motosan_ai/client.py:231-245` (`Client.openai` classmethod)
- Modify: `sdks/python/motosan_ai/client.py:640` (append the native trio and its dispatch helpers)
- Test: `sdks/python/tests/test_client_native.py` (create)

**Interfaces:**
- Consumes: `validate_model_request` (Task 10); `collect_model_stream` (Task 11); the provider `model_chat` / `model_stream` methods (Tasks 12, 13); `UnsupportedFeatureError` (Task 2); the existing `RetryPolicy` machinery in `motosan_ai.retry`.
- Produces: `Client(..., openai_responses_api: bool = False)`, `Client.openai(..., openai_responses_api: bool = False)`, `Client.model_chat_with`, `Client.model_stream_with`, `Client.model_stream_collect_with`, and the private `_prepare_model_request` / `_dispatch_model_chat`.

**Decision context (D6, D7):** `BaseProvider` is subclassed by only 4 of the 11 Python providers — `OpenAIProvider`, `MinimaxProvider`, `OllamaProvider`, and the three CLI clients are structurally-typed classes. A default `model_chat` on the ABC would therefore never reach `OpenAIProvider`. `Client` must **duck-type**, exactly as the capability enforcement shipped in 0.19.0 does at `client.py:475`. Method names follow the existing `chat_with` / `stream_with` / `stream_collect_with` trio (D7).

**Traps this task must reproduce exactly:**
1. **No `ThinkStripper` on the native path.** Rust's `dispatch_model_stream` (`client.rs:336-351`) does not wrap the stripper; native streams carry thinking as explicit `ModelStreamThinkingDelta` / `ModelStreamThinkingDone`, so stripping `<think>` out of text would be wrong.
2. **Read-idle timeout.** Rust wraps the native stream in `ReadTimeoutModelStream`. Python gets the same behaviour from the provider's own `httpx.Timeout(read=read_idle_timeout)` plus the `httpx.ReadTimeout → StreamReadTimeoutError` mapping added in Tasks 12 and 13. Do **not** add a second wrapper in `Client`.
3. **The `openai_responses_api` flag is threaded only into the `Provider.openai` branch.** The `Provider.ollama` branch at `client.py:170` also constructs an `OpenAIProvider`; Ollama's OpenAI-compatible endpoint has no Responses API, so it must not receive the flag.
4. Retry semantics mirror the legacy pair: blocking calls retry through `with_retry`; streams retry only **before the first yielded delta**, because retrying mid-stream would replay already-delivered deltas.

- [ ] **Step 1: Write the failing test**

Create `sdks/python/tests/test_client_native.py`:

```python
from __future__ import annotations

import json
from collections.abc import AsyncIterator

import httpx
import pytest
import respx

from motosan_ai import Client
from motosan_ai.error import UnsupportedFeatureError
from motosan_ai.retry import RetryPolicy
from motosan_ai.types import (
    FreeformTool,
    FreeformToolFormat,
    Message,
    ModelChatRequest,
    ModelChatResponse,
    ModelStreamDelta,
    ModelStreamDone,
    ModelStreamText,
    ModelStreamToolCallDone,
    ModelStreamUsage,
    ModelToolCallFreeform,
    ModelToolSpecFreeform,
    StopReason,
    Usage,
)

FREEFORM_SPEC = ModelToolSpecFreeform(
    tool=FreeformTool(
        name="exec",
        description="Run JavaScript",
        format=FreeformToolFormat(type="grammar", syntax="lark", definition="start: source"),
    )
)


def _sse(*payloads: dict) -> str:
    return "\n".join(f"data: {json.dumps(p)}" for p in payloads) + "\n"


class _RecordingProvider:
    """Structurally-typed provider — deliberately NOT a BaseProvider subclass."""

    def __init__(self, capabilities=None) -> None:
        from motosan_ai.provider_base import ProviderCapabilities

        self.capabilities = capabilities or ProviderCapabilities.with_freeform_tools()
        self.seen: list[ModelChatRequest] = []

    async def model_chat(self, request: ModelChatRequest) -> ModelChatResponse:
        self.seen.append(request)
        return ModelChatResponse(content="native", model="", stop_reason=StopReason.end_turn)

    async def model_stream(self, request: ModelChatRequest) -> AsyncIterator[ModelStreamDelta]:
        self.seen.append(request)
        yield ModelStreamText(delta="nat")
        yield ModelStreamText(delta="ive")
        yield ModelStreamUsage(usage=Usage(input_tokens=1, output_tokens=2))
        yield ModelStreamDone(stop_reason=StopReason.end_turn)

    async def aclose(self) -> None:
        return None


class _NoNativeProvider:
    async def aclose(self) -> None:
        return None


def _client(provider_obj) -> Client:
    client = Client(provider="anthropic", api_key="k", model="client-model")
    client._provider = provider_obj
    return client


def test_openai_responses_api_flag_is_threaded_through_the_constructor():
    off = Client(provider="openai", api_key="k")
    on = Client(provider="openai", api_key="k", openai_responses_api=True)
    assert off._provider.responses_api is False
    assert on._provider.responses_api is True
    assert on._provider.capabilities.supports_freeform_tools is True


def test_openai_responses_api_flag_is_threaded_through_the_shortcut():
    assert Client.openai(api_key="k")._provider.responses_api is False
    assert Client.openai(api_key="k", openai_responses_api=True)._provider.responses_api is True


def test_ollama_over_openai_never_receives_the_responses_opt_in():
    client = Client(provider="ollama", model="llama3.2")
    assert client._provider.responses_api is False


async def test_model_chat_with_dispatches_and_backfills_the_model():
    provider = _RecordingProvider()
    client = _client(provider)

    response = await client.model_chat_with(
        ModelChatRequest.builder().message(Message.user("hi")).build()
    )

    assert response.content == "native"
    assert provider.seen[0].model == "client-model"


async def test_model_chat_with_keeps_an_explicit_request_model():
    provider = _RecordingProvider()
    response = await _client(provider).model_chat_with(
        ModelChatRequest.builder().model("explicit").message(Message.user("hi")).build()
    )
    assert provider.seen[0].model == "explicit"
    assert response.content == "native"


async def test_model_stream_with_yields_every_delta():
    deltas = [
        delta
        async for delta in _client(_RecordingProvider()).model_stream_with(
            ModelChatRequest.builder().message(Message.user("hi")).build()
        )
    ]
    assert deltas == [
        ModelStreamText(delta="nat"),
        ModelStreamText(delta="ive"),
        ModelStreamUsage(usage=Usage(input_tokens=1, output_tokens=2)),
        ModelStreamDone(stop_reason=StopReason.end_turn),
    ]


async def test_model_stream_collect_with_assembles_and_backfills_the_model():
    response = await _client(_RecordingProvider()).model_stream_collect_with(
        ModelChatRequest.builder().message(Message.user("hi")).build()
    )
    assert response.content == "native"
    assert response.usage == Usage(input_tokens=1, output_tokens=2)
    assert response.stop_reason == StopReason.end_turn
    assert response.model == "client-model"


async def test_native_dispatch_is_duck_typed_not_isinstance_based():
    client = _client(_NoNativeProvider())
    request = ModelChatRequest.builder().message(Message.user("hi")).build()

    with pytest.raises(UnsupportedFeatureError, match="native model requests"):
        await client.model_chat_with(request)
    with pytest.raises(UnsupportedFeatureError, match="native model streams"):
        async for _ in client.model_stream_with(request):
            pass


async def test_capabilities_are_enforced_before_native_dispatch():
    from motosan_ai.provider_base import ProviderCapabilities

    provider = _RecordingProvider(capabilities=ProviderCapabilities.with_image())
    client = _client(provider)
    request = ModelChatRequest.builder().message(Message.user("hi")).tool_spec(FREEFORM_SPEC).build()

    with pytest.raises(UnsupportedFeatureError, match="native freeform tools"):
        await client.model_chat_with(request)
    with pytest.raises(UnsupportedFeatureError, match="native freeform tools"):
        async for _ in client.model_stream_with(request):
            pass
    assert provider.seen == []


async def test_provider_without_capabilities_is_not_validated():
    # The LlmClient Protocol does not require `capabilities`; native
    # validation must be skipped, not crash, for such providers.
    provider = _RecordingProvider()
    del provider.capabilities
    client = _client(provider)
    response = await client.model_chat_with(
        ModelChatRequest.builder().tool_spec(FREEFORM_SPEC).build()
    )
    assert response.content == "native"


@respx.mock
async def test_end_to_end_over_the_chatgpt_codex_provider():
    respx.post("https://chatgpt.com/backend-api/codex/responses").mock(
        return_value=httpx.Response(
            200,
            text=_sse(
                {
                    "type": "response.output_item.done",
                    "item": {
                        "type": "custom_tool_call",
                        "call_id": "call_js",
                        "name": "exec",
                        "input": "text('captured');",
                    },
                },
                {
                    "type": "response.completed",
                    "response": {
                        "status": "completed",
                        "usage": {"input_tokens": 4, "output_tokens": 5},
                    },
                },
            ),
            headers={"content-type": "text/event-stream"},
        )
    )

    client = Client.chatgpt_codex(
        access_token="tok",
        account_id="acct-123",
        model="gpt-5.5",
        retry_policy=RetryPolicy(max_retries=0),
    )
    response = await client.model_stream_collect_with(
        ModelChatRequest.builder().message(Message.user("run js")).tool_spec(FREEFORM_SPEC).build()
    )

    assert response.tool_calls == [
        ModelToolCallFreeform(id="call_js", name="exec", input="text('captured');")
    ]
    assert response.model == "gpt-5.5"
    assert response.stop_reason == StopReason.tool_use


async def test_native_stream_does_not_strip_think_tags():
    class _ThinkProvider(_RecordingProvider):
        async def model_stream(
            self, request: ModelChatRequest
        ) -> AsyncIterator[ModelStreamDelta]:
            yield ModelStreamText(delta="<think>secret</think>visible")
            yield ModelStreamDone(stop_reason=StopReason.end_turn)

    response = await _client(_ThinkProvider()).model_stream_collect_with(
        ModelChatRequest.builder().message(Message.user("hi")).build()
    )
    # The native path carries thinking as explicit deltas, so text passes
    # through untouched — unlike Client.stream_with.
    assert response.content == "<think>secret</think>visible"


async def test_tool_call_done_survives_client_level_collection():
    class _ToolProvider(_RecordingProvider):
        async def model_stream(
            self, request: ModelChatRequest
        ) -> AsyncIterator[ModelStreamDelta]:
            yield ModelStreamToolCallDone(
                call=ModelToolCallFreeform(id="c", name="exec", input="raw();")
            )
            yield ModelStreamDone(stop_reason=StopReason.tool_use)

    response = await _client(_ToolProvider()).model_stream_collect_with(
        ModelChatRequest.builder().build()
    )
    assert response.tool_calls == [ModelToolCallFreeform(id="c", name="exec", input="raw();")]
```

- [ ] **Step 2: Run it to verify it fails**

Run: `cd sdks/python && uv run pytest tests/test_client_native.py -q`
Expected: FAIL with `TypeError: Client.__init__() got an unexpected keyword argument 'openai_responses_api'` on the first three tests and `AttributeError: 'Client' object has no attribute 'model_chat_with'` on the rest — 14 failed.

- [ ] **Step 3: Implement**

Edit 1 — add a keyword-only parameter to `Client.__init__`, immediately after `ollama_num_ctx: int | None = None,` (`client.py:84`):

```python
        openai_responses_api: bool = False,
```

Edit 2 — replace the `Provider.openai` construction branch (`client.py:190-197`):

```python
            elif provider_value == Provider.openai:
                self._provider = OpenAIProvider(
                    api_key=self.api_key,
                    model=model,
                    base_url=base_url,
                    # Native opt-in. Deliberately NOT threaded into the
                    # Provider.ollama branch above: Ollama's OpenAI-compatible
                    # endpoint has no Responses API.
                    responses_api=openai_responses_api,
                    connect_timeout=connect_timeout,
                    read_idle_timeout=read_idle_timeout,
                )
```

Edit 3 — replace the `Client.openai` classmethod (`client.py:231-245`):

```python
    @classmethod
    def openai(
        cls,
        api_key: str | None = None,
        model: str | None = None,
        max_retries: int = 3,
        retry_policy: RetryPolicy | None = None,
        *,
        openai_responses_api: bool = False,
    ) -> Client:
        return cls(
            provider=Provider.openai,
            api_key=api_key,
            model=model,
            max_retries=max_retries,
            retry_policy=retry_policy,
            openai_responses_api=openai_responses_api,
        )
```

Edit 4 — append at the end of the `Client` class (after `stream_collect_with`, `client.py:640`):

```python
    def _prepare_model_request(self, request: ModelChatRequest) -> ModelChatRequest:
        """Backfill the model and run capability enforcement before dispatch."""
        if request.model is None and self.model is not None:
            request = replace(request, model=self.model)

        caps = getattr(self._provider, "capabilities", None)
        if caps is not None:
            _validate_model_request(request, caps)
        return request

    async def model_chat_with(self, request: ModelChatRequest) -> ModelChatResponse:
        """Send a fully-built native ModelChatRequest.

        The native counterpart of ``chat_with``. ``total_timeout`` bounds this
        call (retries included), never stream consumption.
        """
        request = self._prepare_model_request(request)
        if self._total_timeout is None:
            return await self._dispatch_model_chat(request)
        try:
            async with asyncio.timeout(self._total_timeout):
                return await self._dispatch_model_chat(request)
        except TimeoutError as exc:
            raise NetworkError(f"total timeout of {self._total_timeout}s exceeded") from exc

    async def _dispatch_model_chat(self, request: ModelChatRequest) -> ModelChatResponse:
        # Duck-typed on purpose: BaseProvider is subclassed by only 4 of the 11
        # providers, so a default on the ABC would never reach OpenAIProvider
        # or the CLI clients. Mirrors the 0.19.0 capability enforcement.
        model_chat = getattr(self._provider, "model_chat", None)
        if model_chat is None:
            raise UnsupportedFeatureError("provider does not support native model requests")

        if self._retry_policy.max_retries > 0:
            from motosan_ai.retry import with_retry

            return await with_retry(lambda: model_chat(request), policy=self._retry_policy)
        return await model_chat(request)

    async def model_stream_with(
        self, request: ModelChatRequest
    ) -> AsyncIterator[ModelStreamDelta]:
        """Stream a fully-built native ModelChatRequest.

        No ThinkStripper here: native streams carry thinking as explicit
        ModelStreamThinkingDelta / ModelStreamThinkingDone deltas, and the
        read-idle timeout already lives in the provider's httpx client.
        """
        request = self._prepare_model_request(request)
        model_stream = getattr(self._provider, "model_stream", None)
        if model_stream is None:
            raise UnsupportedFeatureError("provider does not support native model streams")

        policy = self._retry_policy
        last_error: MotosanError | None = None
        max_attempts = policy.max_retries + 1 if policy.max_retries > 0 else 1
        for attempt in range(max_attempts):
            yielded = False
            try:
                async for delta in model_stream(request):
                    yielded = True
                    yield delta
                return
            except (RateLimitError, NetworkError, ProviderError) as e:
                from motosan_ai.retry import (
                    RetryEvent,
                    _is_retryable,
                    compute_delay,
                    retry_cause,
                )

                # Once any delta has been emitted a mid-stream error must
                # propagate verbatim; retrying would replay delivered deltas.
                if yielded or not _is_retryable(e):
                    raise
                last_error = e
                if attempt >= policy.max_retries:
                    break
                wait = compute_delay(policy, attempt + 1, e.retry_after)
                if policy.on_retry is not None:
                    policy.on_retry(
                        RetryEvent(attempt=attempt + 1, delay=wait, cause=retry_cause(e))
                    )
                logger.warning(
                    "Retryable native stream error (attempt %d/%d), retrying in %.1fs: %s",
                    attempt + 1,
                    policy.max_retries,
                    wait,
                    type(e).__name__,
                )
                await asyncio.sleep(wait)
        raise last_error  # type: ignore[misc]

    async def model_stream_collect_with(self, request: ModelChatRequest) -> ModelChatResponse:
        """Stream a native request and assemble the full ModelChatResponse."""
        from motosan_ai._stream_collect import collect_model_stream

        model_hint = request.model or self.model or ""
        response = await collect_model_stream(self.model_stream_with(request))
        if not response.model:
            response.model = model_hint
        return response
```

Extend the imports at the top of `client.py`:

```python
from motosan_ai.error import (
    ConfigError,
    MotosanError,
    NetworkError,
    ProviderError,
    RateLimitError,
    UnsupportedFeatureError,
)
from motosan_ai.provider_base import validate_model_request as _validate_model_request
from motosan_ai.provider_base import validate_request as _validate_request
```

and add `ModelChatRequest`, `ModelChatResponse`, `ModelStreamDelta` to the existing `from motosan_ai.types import ChatRequest, ChatResponse, Message, StreamEvent, Tool` line, keeping it sorted.

- [ ] **Step 4: Run tests**

Run: `cd sdks/python && uv run pytest tests/ -q --ignore=tests/integration/ && uv run ruff check motosan_ai/ && uv run ruff format --check motosan_ai/ tests/ && uv run mypy motosan_ai/`
Expected: PASS — the whole non-integration suite green, ruff clean, format check clean, mypy `Success: no issues found`.

- [ ] **Step 5: Commit**

```bash
git add sdks/python/motosan_ai/client.py sdks/python/tests/test_client_native.py
git commit -m "feat: add the native model trio and duck-typed dispatch to Client (#270)" \
           -m "Co-Authored-By: Claude Opus 5 (1M context) <noreply@anthropic.com>"
```

---

### Task 15: Package-root exports for the P2 symbols

**Files:**
- Modify: `sdks/python/motosan_ai/__init__.py` (the `_stream_collect` import line and `__all__`)
- Test: `sdks/python/tests/test_public_exports.py` (extend)

**Interfaces:**
- Consumes: `collect_model_stream` (Task 11).
- Produces: `motosan_ai.collect_model_stream`, plus a test that pins the capability constructors and the provider-side native methods so a half-wired port cannot merge.

- [ ] **Step 1: Write the failing test**

Append to `sdks/python/tests/test_public_exports.py`:

```python


P2_EXPORTS = ["collect_model_stream"]


def test_p2_symbols_are_importable_and_listed():
    for name in P2_EXPORTS:
        assert hasattr(motosan_ai, name), f"{name} is not importable from motosan_ai"
        assert name in motosan_ai.__all__, f"{name} is missing from __all__"


def test_collect_model_stream_is_the_native_collector():
    from motosan_ai._stream_collect import collect_model_stream

    assert motosan_ai.collect_model_stream is collect_model_stream


def test_capability_constructors_are_reachable_from_the_package_root():
    caps = motosan_ai.ProviderCapabilities
    assert caps.with_freeform_tools().supports_freeform_tools is True
    assert caps.with_image_and_freeform_tools().supports_image is True
    assert caps.full().supports_freeform_tools is False


def test_native_provider_methods_are_wired():
    from motosan_ai.providers.chatgpt_codex import ChatGptCodexProvider
    from motosan_ai.providers.openai import OpenAIProvider

    for provider_cls in (ChatGptCodexProvider, OpenAIProvider):
        assert hasattr(provider_cls, "model_chat"), provider_cls.__name__
        assert hasattr(provider_cls, "model_stream"), provider_cls.__name__
    assert hasattr(motosan_ai.Client, "model_chat_with")
    assert hasattr(motosan_ai.Client, "model_stream_with")
    assert hasattr(motosan_ai.Client, "model_stream_collect_with")
```

- [ ] **Step 2: Run it to verify it fails**

Run: `cd sdks/python && uv run pytest tests/test_public_exports.py -q`
Expected: FAIL — 1 failed, 7 passed, with
`AssertionError: collect_model_stream is not importable from motosan_ai`.

- [ ] **Step 3: Implement**

In `sdks/python/motosan_ai/__init__.py`, replace line 2:

```python
from motosan_ai._stream_collect import collect_model_stream, collect_stream
```

and add `"collect_model_stream"` to `__all__` immediately **before** `"collect_stream"` (sorted order: `collect_model_stream` < `collect_stream` because `m` < `s`).

- [ ] **Step 4: Run tests**

Run: `cd sdks/python && uv run pytest tests/ -q --ignore=tests/integration/ && uv run ruff check motosan_ai/ && uv run ruff format --check motosan_ai/ tests/ && uv run mypy motosan_ai/`
Expected: PASS — full non-integration suite green, ruff clean, format check clean, mypy `Success: no issues found`.

- [ ] **Step 5: Commit and open PR P2**

```bash
git add sdks/python/motosan_ai/__init__.py sdks/python/tests/test_public_exports.py
git commit -m "feat: export collect_model_stream from the Python package root (#270)" \
           -m "Co-Authored-By: Claude Opus 5 (1M context) <noreply@anthropic.com>"

cd sdks/python && uv sync --all-extras && cd ../..
treefmt --fail-on-change
python3 scripts/check-versions.py

git push -u origin feat/freeform-python-providers
test "$(git ls-remote origin refs/heads/feat/freeform-python-providers | cut -f1)" = "$(git rev-parse HEAD)"
gh pr create --base main --head feat/freeform-python-providers \
  --title "feat: wire the native model API into the Python providers and Client (#270)" \
  --body "PR P2 of #270. Capabilities, collect_model_stream, ChatGPT Codex + OpenAI native methods (with the Responses opt-in), and the Client native trio with duck-typed dispatch."
```

---

### Task 16: Python half of the freeform conformance suite

**Files:**
- Create: `sdks/python/tests/test_freeform_conformance.py`
- Test: the file itself — this task ships a gate, not a feature.

**Interfaces:**
- Consumes: everything from Tasks 2-15. Adds no production code.
- Produces: `sdks/python/tests/test_freeform_conformance.py`, the Python member of the cross-SDK trio. Its Rust sibling (`sdks/rust/tests/freeform_conformance.rs`) and TypeScript sibling (`sdks/typescript/tests/freeform-conformance.test.ts`) ship in the same PR **C** but are out of scope for this plan.

**Decision context (D9):** M2 shipped `*retry-conformance*` and M3 shipped `*m3-stream-conformance*` in all three SDKs, anchored to `specs/`. This is the third such suite, anchored to `specs/types.md` § Native Model API. It covers D8's stream contract plus ordered history replay and pre-network rejection. Every expected value below comes from the Rust tests that already pin the behaviour — no fixtures are invented.

- [ ] **Step 1: Write the failing test**

Create `sdks/python/tests/test_freeform_conformance.py`:

```python
"""Freeform / native-model-API conformance gates.

Anchored to specs/types.md § Native Model API. Cross-SDK mirrors:
- sdks/rust/tests/freeform_conformance.rs
- sdks/typescript/tests/freeform-conformance.test.ts

Expected values come from the Rust tests that already pin this behaviour
(tests/core_types.rs, tests/openai_provider.rs, tests/chatgpt_codex.rs,
tests/native_collect_stream.rs). Do not invent new fixtures here.
"""

from __future__ import annotations

import json
from collections.abc import AsyncIterator

import httpx
import pytest
import respx

from motosan_ai._stream_collect import collect_model_stream
from motosan_ai.error import IncompleteStreamError, StreamError, UnsupportedFeatureError
from motosan_ai.providers.chatgpt_codex import ChatGptCodexProvider
from motosan_ai.providers.openai import OpenAIProvider
from motosan_ai.providers.responses import decode_tool_call, encode_input, encode_tool_call
from motosan_ai.types import (
    FreeformTool,
    FreeformToolFormat,
    FunctionCallOutputText,
    Message,
    ModelChatRequest,
    ModelStreamDelta,
    ModelStreamDone,
    ModelStreamFreeformInput,
    ModelStreamText,
    ModelStreamThinkingDelta,
    ModelStreamThinkingDone,
    ModelStreamToolCallDone,
    ModelStreamUsage,
    ModelToolCallFreeform,
    ModelToolCallFunction,
    ModelToolOutputCustom,
    ModelToolOutputFunction,
    ModelToolSpecFreeform,
    StopReason,
    Usage,
)

_CODEX_URL = "https://chatgpt.com/backend-api/codex/responses"
_OPENAI_RESPONSES = "https://api.openai.com/v1/responses"

FREEFORM_SPEC = ModelToolSpecFreeform(
    tool=FreeformTool(
        name="exec",
        description="Run JavaScript",
        format=FreeformToolFormat(type="grammar", syntax="lark", definition="start: source"),
    )
)


def _sse(*payloads: dict) -> str:
    return "\n".join(f"data: {json.dumps(p)}" for p in payloads) + "\n"


def _native_request() -> ModelChatRequest:
    return (
        ModelChatRequest.builder().message(Message.user("run js")).tool_spec(FREEFORM_SPEC).build()
    )


async def _stream(*deltas: ModelStreamDelta) -> AsyncIterator[ModelStreamDelta]:
    for delta in deltas:
        yield delta


# --- Freeform input survives byte-for-byte -------------------------------


def test_freeform_input_is_never_parsed_as_json_or_lowered_into_arguments():
    raw = '{"this":"looks like json"}\nconsole.log(\'but is JavaScript\');'
    encoded = encode_tool_call(ModelToolCallFreeform(id="call_js", name="exec", input=raw))

    assert encoded["type"] == "custom_tool_call"
    assert encoded["input"] == raw
    assert encoded["input"].encode() == raw.encode()
    assert "arguments" not in encoded

    decoded = decode_tool_call(encoded)
    assert decoded == ModelToolCallFreeform(id="call_js", name="exec", input=raw)


def test_ordered_mixed_history_replays_in_order():
    raw = '{"not":"function args"}\nvalue.not;\n'
    request = (
        ModelChatRequest.builder()
        .message(Message.user("run js"))
        .tool_call(ModelToolCallFunction(id="call_fn", name="sum", arguments='{"a":1}'))
        .tool_output(
            ModelToolOutputFunction(call_id="call_fn", output=FunctionCallOutputText(text="1"))
        )
        .tool_call(ModelToolCallFreeform(id="call_js", name="exec", input=raw))
        .tool_output(
            ModelToolOutputCustom(
                call_id="call_js",
                output=FunctionCallOutputText(text="function args"),
                name="exec",
            )
        )
        .build()
    )

    encoded = encode_input(request.context)
    assert [item["type"] for item in encoded] == [
        "message",
        "function_call",
        "function_call_output",
        "custom_tool_call",
        "custom_tool_call_output",
    ]
    assert encoded[3]["input"].encode() == raw.encode()
    assert "arguments" not in encoded[3]


# --- Collector contract (specs/types.md § Stream termination (native)) ----


async def test_tool_call_done_is_authoritative():
    response = await collect_model_stream(
        _stream(
            ModelStreamFreeformInput(call_id="call_js", delta="console."),
            ModelStreamFreeformInput(call_id="call_js", delta="log(1);"),
            ModelStreamToolCallDone(
                call=ModelToolCallFreeform(id="call_js", name="exec", input="console.log(1);")
            ),
            ModelStreamUsage(usage=Usage(input_tokens=2, output_tokens=3)),
            ModelStreamDone(stop_reason=StopReason.tool_use),
        )
    )
    assert response.tool_calls == [
        ModelToolCallFreeform(id="call_js", name="exec", input="console.log(1);")
    ]
    assert response.stop_reason == StopReason.tool_use
    assert response.usage.output_tokens == 3


async def test_usage_replaces_rather_than_merges():
    response = await collect_model_stream(
        _stream(
            ModelStreamUsage(usage=Usage(input_tokens=99, output_tokens=99)),
            ModelStreamUsage(usage=Usage(input_tokens=0, output_tokens=5)),
            ModelStreamDone(stop_reason=StopReason.end_turn),
        )
    )
    assert response.usage == Usage(input_tokens=0, output_tokens=5)


async def test_thinking_done_wins_over_accumulated_deltas():
    response = await collect_model_stream(
        _stream(
            ModelStreamThinkingDelta(delta="think "),
            ModelStreamThinkingDelta(delta="hard"),
            ModelStreamThinkingDone(thinking="think hard"),
            ModelStreamText(delta="answer"),
            ModelStreamDone(stop_reason=StopReason.end_turn),
        )
    )
    assert response.thinking == "think hard"
    assert response.content == "answer"


# --- Exactly one terminal per completed stream ---------------------------


@respx.mock
async def test_exactly_one_done_per_successfully_completed_stream():
    respx.post(_CODEX_URL).mock(
        return_value=httpx.Response(
            200,
            text=_sse(
                {"type": "response.output_text.delta", "delta": "hi"},
                {
                    "type": "response.completed",
                    "response": {
                        "status": "completed",
                        "usage": {"input_tokens": 1, "output_tokens": 1},
                    },
                },
            ),
            headers={"content-type": "text/event-stream"},
        )
    )

    provider = ChatGptCodexProvider("tok", "acct-123", "gpt-5.5", None)
    deltas = [delta async for delta in provider.model_stream(_native_request())]
    assert sum(isinstance(d, ModelStreamDone) for d in deltas) == 1
    assert isinstance(deltas[-1], ModelStreamDone)


@respx.mock
async def test_response_incomplete_is_a_received_terminal():
    respx.post(_CODEX_URL).mock(
        return_value=httpx.Response(
            200,
            text=_sse(
                {"type": "response.output_text.delta", "delta": "partial"},
                {
                    "type": "response.incomplete",
                    "response": {
                        "status": "incomplete",
                        "usage": {"input_tokens": 6, "output_tokens": 7},
                    },
                },
            ),
            headers={"content-type": "text/event-stream"},
        )
    )

    provider = ChatGptCodexProvider("tok", "acct-123", "gpt-5.5", None)
    response = await provider.model_chat(
        ModelChatRequest.builder().message(Message.user("short")).build()
    )
    assert response.content == "partial"
    assert response.stop_reason == StopReason.max_tokens
    assert response.usage.output_tokens == 7


# --- EOF without a terminal, both provider strings -----------------------


@respx.mock
async def test_codex_eof_without_terminal_raises_incomplete_stream():
    respx.post(_CODEX_URL).mock(
        return_value=httpx.Response(
            200,
            text=_sse(
                {
                    "type": "response.custom_tool_call_input.delta",
                    "call_id": "call_js",
                    "delta": "console.",
                }
            ),
            headers={"content-type": "text/event-stream"},
        )
    )

    provider = ChatGptCodexProvider("tok", "acct-123", "gpt-5.5", None)
    with pytest.raises(IncompleteStreamError) as exc:
        await collect_model_stream(provider.model_stream(_native_request()))
    assert str(exc.value) == "incomplete stream: chatgpt-codex ended without a terminal event"


@respx.mock
async def test_openai_eof_without_terminal_raises_incomplete_stream():
    respx.post(_OPENAI_RESPONSES).mock(
        return_value=httpx.Response(
            200,
            text=_sse(
                {"type": "response.output_text.delta", "delta": "hel"},
                {"type": "response.output_text.delta", "delta": "lo"},
            ),
            headers={"content-type": "text/event-stream"},
        )
    )

    provider = OpenAIProvider(api_key="k", model="gpt-5.5-codex", responses_api=True)
    with pytest.raises(IncompleteStreamError) as exc:
        await collect_model_stream(provider.model_stream(_native_request()))
    assert str(exc.value) == "incomplete stream: openai ended without a terminal event"


def test_incomplete_stream_error_is_a_stream_error():
    assert issubclass(IncompleteStreamError, StreamError)


# --- Pending deltas drain before a stored stream error surfaces ----------


@respx.mock
async def test_pending_deltas_drain_before_a_stream_error():
    respx.post(_CODEX_URL).mock(
        return_value=httpx.Response(
            200,
            text=_sse(
                {"type": "response.output_text.delta", "delta": "before"},
                {"type": "response.failed", "response": {"error": {"message": "upstream died"}}},
            ),
            headers={"content-type": "text/event-stream"},
        )
    )

    provider = ChatGptCodexProvider("tok", "acct-123", "gpt-5.5", None)
    seen: list[ModelStreamDelta] = []
    with pytest.raises(StreamError, match="upstream died"):
        async for delta in provider.model_stream(_native_request()):
            seen.append(delta)
    assert seen == [ModelStreamText(delta="before")]


# --- Pre-network rejection ------------------------------------------------


@respx.mock
async def test_unsupported_provider_rejects_freeform_before_network():
    route = respx.post(host="api.openai.com").mock(return_value=httpx.Response(500))
    provider = OpenAIProvider(api_key="k")  # no Responses opt-in

    with pytest.raises(UnsupportedFeatureError, match="native freeform tools"):
        await provider.model_chat(_native_request())
    with pytest.raises(UnsupportedFeatureError, match="native freeform tools"):
        async for _ in provider.model_stream(_native_request()):
            pass
    assert route.call_count == 0
    assert isinstance(UnsupportedFeatureError("x"), Exception)


def test_capability_matrix_matches_the_spec():
    from motosan_ai.provider_base import ProviderCapabilities

    codex = ChatGptCodexProvider("tok", "acct-123", "gpt-5.5", None)
    assert codex.capabilities == ProviderCapabilities.with_freeform_tools()

    plain_openai = OpenAIProvider(api_key="k")
    assert plain_openai.capabilities == ProviderCapabilities.with_image()

    responses_openai = OpenAIProvider(api_key="k", responses_api=True)
    assert responses_openai.capabilities == ProviderCapabilities.with_image_and_freeform_tools()

    # full() deliberately leaves freeform false.
    assert ProviderCapabilities.full().supports_freeform_tools is False
```

- [ ] **Step 2: Run it to verify it fails**

The suite is a regression gate over Tasks 2-15, so it must be shown to fail when the ported surface is absent rather than passing vacuously. Temporarily remove the codec and re-run:

Run:
```bash
cd sdks/python
git stash push --include-untracked -- motosan_ai/providers/responses.py
uv run pytest tests/test_freeform_conformance.py -q ; git stash pop
```
Expected: FAIL with `ModuleNotFoundError: No module named 'motosan_ai.providers.responses'` — collection error, 0 tests run. `git stash pop` restores the codec.

- [ ] **Step 3: Implement**

No production code. This task ships the gate only; every behaviour it asserts was implemented in Tasks 2-15. If any assertion fails, the corresponding task's implementation has diverged from `specs/types.md` — fix the implementation, not the test.

- [ ] **Step 4: Run tests**

Run: `cd sdks/python && uv run pytest tests/test_freeform_conformance.py -q && uv run pytest tests/ -q --ignore=tests/integration/ && uv run ruff check motosan_ai/ && uv run ruff format --check motosan_ai/ tests/ && uv run mypy motosan_ai/`
Expected: PASS — 13 conformance tests green, full non-integration suite green, ruff clean, format check clean, mypy `Success: no issues found`.

- [ ] **Step 5: Commit and open PR C-PY**

```bash
git switch -c test/freeform-python-conformance   # branch off the merged P2
git add sdks/python/tests/test_freeform_conformance.py
git commit -m "feat: add the Python freeform conformance suite (#270)" \
           -m "Co-Authored-By: Claude Opus 5 (1M context) <noreply@anthropic.com>"

cd sdks/python && uv sync --all-extras && cd ../..
treefmt --fail-on-change
python3 scripts/check-versions.py

git push -u origin test/freeform-python-conformance
test "$(git ls-remote origin refs/heads/test/freeform-python-conformance | cut -f1)" = "$(git rev-parse HEAD)"
gh pr create --base main --head test/freeform-python-conformance \
  --title "feat: add the Python freeform conformance suite (#270)" \
  --body "Python half of PR C for #270, anchored to specs/types.md § Native Model API. The Rust and TypeScript halves ship alongside it."
```

---

## Done criteria

The Python track is complete when all of the following hold on `origin/main`:

- [ ] `specs/types.md` § Native Model API is a cross-SDK contract carrying the implementation-status line `Implemented in Rust 0.26.0+. Python and TypeScript ports in progress — see #270.` — and does **not** yet claim Python or TypeScript ship it.
- [ ] `from motosan_ai import ModelChatRequest, ModelToolCallFreeform, collect_model_stream, UnsupportedFeatureError` works, and `tests/test_public_exports.py` passes.
- [ ] `Client.chatgpt_codex(...).model_stream_collect_with(request)` round-trips a Freeform call byte-for-byte.
- [ ] `Client.openai(api_key=..., openai_responses_api=True).model_chat_with(request)` reaches `/v1/responses`; without the flag the same request raises `UnsupportedFeatureError` **before** any HTTP call.
- [ ] Both EOF payloads are exact: `incomplete stream: openai ended without a terminal event` and `incomplete stream: chatgpt-codex ended without a terminal event`.
- [ ] From `sdks/python/`: `uv run ruff check motosan_ai/`, `uv run ruff format --check motosan_ai/ tests/`, `uv run mypy motosan_ai/`, and `uv run pytest tests/ -q --ignore=tests/integration/` are all green.
- [ ] From the repository root: `treefmt --fail-on-change` and `python3 scripts/check-versions.py` are green.
- [ ] Four PRs merged in order: **S** → **P1** → **P2** → **C-PY** (C-PY also carries the Rust and TypeScript conformance files, which are out of scope for this plan).

**Not done here, deliberately:** the TypeScript track (PRs T1/T2) and the release (PR REL — Python 0.20.0 / TypeScript 0.16.0, produced by `scripts/bump-version.py`; REL is also what rewrites the spec's implementation-status line to the shipped versions and widens the Provider-support paragraph).
