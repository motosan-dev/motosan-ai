# Python SDK Catch-Up Roadmap

> **Status:** Master roadmap. Each phase ships an independently-releasable Python SDK version. Phase 1 has a detailed plan (see sibling file). Phases 2–4 will get their own detailed plans when the prior phase lands.
>
> **Progress:** Phase 1 ✅ complete (2026-04-24) — v0.6.0 landed, 168 tests pass, lint/format green. Phase 2 next.

**Goal:** Bring `motosan-ai` Python SDK (v0.5.0) to feature parity with Rust SDK (v0.14.0).

**Ship target:** Python v0.10.0+ in 4 successive releases (~8 weeks).

---

## Current Gap (v0.5.0 vs Rust v0.14.0)

### Missing providers
- `Gemini` (HTTP) · `Gemini Code Assist` (HTTP + OAuth PKCE) · `Codex CLI` · `Gemini CLI`

### Missing types
- `ContentBlock` / `ImageSource` / `DocumentSource` (→ no vision, no PDF)
- `SystemBlock` + `Message.cache` + `Tool.cache` (→ no prompt caching)
- `ToolChoice` (auto / required / none / tool(name))
- `ThinkingConfig` (extended thinking)
- `McpServerConfig` + `McpToolConfig`
- `ProviderCapabilities` + `ProviderImpl.validate_request()`

### Missing `ChatRequest` fields
`system_blocks`, `system_cache`, `tool_choice`, `mcp_servers`, `mcp_tool_configs`, `thinking`, `stop_sequences`

### Missing `ChatResponse` / `StreamEvent` / `Usage` fields
- `ChatResponse.thinking`
- `StreamEvent.stop_reason`, `StreamEvent.usage`, `StreamEventType` enum, `StreamEventType::Usage`
- `Usage.cache_creation_input_tokens`, `Usage.cache_read_input_tokens`
- `StopReason.stop_sequence` variant

### Missing `Client` API
- `chat_with(request)` / `stream_with(request)` / `stream_collect()` / `stream_collect_with()`
- `ChatRequest.builder()` fluent builder

---

## Phases

### Phase 1 — Type foundation (v0.6.0) — ✅ COMPLETE (2026-04-24)
**Additive type changes. Zero behavior change in existing providers.**

- Add `ContentBlock` / `ImageSource` / `DocumentSource` as discriminated unions.
- Add `SystemBlock`, `Tool.cache`, `Message.cache` + `.with_cache()`, `Message.user_with_image()`, `Message.user_with_pdf_*()`.
- Add `ToolChoice` (factory-constructed dataclass).
- Add `ThinkingConfig`, `McpServerConfig`, `McpToolConfig`.
- Extend `ChatRequest` with all Rust fields.
- Add `ChatRequest.builder()` returning `ChatRequestBuilder` with fluent API.
- Extend `ChatResponse` (`thinking`), `StreamEvent` (`stop_reason`, `usage`, `StreamEventType` enum), `Usage` (cache token fields), `StopReason` (`stop_sequence`).
- Add `ProviderCapabilities` + `ProviderImpl` Protocol with `capabilities()` + `validate_request()` defaults.
- Wire capabilities into existing providers: Anthropic=full, OpenAI=with_image, Minimax=with_image, Ollama-native=text_only, Claude Code CLI=text_only.
- **Does NOT change wire format or send new fields over the network yet.** Serializers stay untouched; new fields default to `None`/absent so existing tests pass.

**Exit criteria:** full `check-python` passes, all existing tests still green, new type tests cover every enum variant and serde-equivalent dict shape, Python and Rust types match `specs/types.md` SSOT.

See detailed plan: `2026-04-24-python-sdk-phase1-types.md`.

### Phase 2 — Anthropic feature depth + Gemini HTTP (v0.7.0 → v0.8.0) — ~2 weeks

**2a — Anthropic advanced (v0.7.0)**
- Serialize `content_blocks` on user messages (vision + PDF).
- Serialize `system_blocks` with `cache_control`; respect `system_cache` flag; serialize `Tool.cache` on last tool.
- Serialize `Message.cache` via `cache_control` on last content block.
- Serialize `tool_choice` as Anthropic format.
- Serialize `thinking` → `{"type":"enabled","budget_tokens":N}`; force `temperature=1.0` when enabled.
- Serialize `mcp_servers` + `mcp_tool_configs`; attach `anthropic-beta: mcp-client-2025-11-20` when MCP present.
- Parse `stop_reason: "stop_sequence"`; parse cache usage tokens into `Usage`; parse `thinking_delta` → `ChatResponse.thinking` (non-stream) or `StreamEvent` thinking pipe-through.
- Emit `StreamEvent.stop_reason` on terminal event; emit `StreamEvent.usage` from `message_start`/`message_delta`.
- Wire retry to respect `Retry-After`, stream per-chunk timeout.
- Live-test suite: vision, thinking, prompt-caching (input/read tokens verified), MCP roundtrip, stop_sequences.

**2b — Gemini HTTP (v0.8.0)**
- New `GeminiProvider` under `motosan_ai/providers/gemini.py`.
- API key header `x-goog-api-key`; default model `gemini-2.0-flash`.
- Serialize messages to `contents[]` with `role: "user"|"model"` + `parts[]` (text, `inlineData`, `fileData`).
- Tools → `tools.functionDeclarations[]`; tool calls ↔ `functionCall`/`functionResponse`.
- System prompt → `systemInstruction`.
- SSE streaming via `streamGenerateContent?alt=sse`.
- Capabilities: `with_image()`.
- Full mock test suite mirroring Anthropic's; live-test gate.
- Register `Provider.gemini` in `Client` dispatch.

### Phase 3 — CLI backends + OAuth (v0.9.0) — ~2 weeks

- **Codex CLI provider** — subprocess `codex exec --json`; parse JSONL events; map to `StreamEvent`; support sandbox/profile/config flags via builder. Mirror Rust `codex_cli` module structure.
- **Gemini CLI provider** — subprocess `gemini -p`; approval-mode handling; non-interactive error recovery. Mirror Rust `gemini_cli` module.
- **Expand Claude Code CLI provider** — add feature-flag parity with Rust (all CLI flags: `--resume`, `--session-id`, `--append-system-prompt`, `--allowed-tools`, etc.). Align `ChatResponse.usage` / `stop_reason` parsing.
- **Gemini Code Assist OAuth PKCE flow** — new `motosan_ai/oauth/google.py` module with PKCE + token refresh + `cloud-platform` scope. New `GeminiCodeAssistProvider` targeting `cloudcode-pa.googleapis.com` with the project-ID-wrapped request format.
- **Register all in `Provider` enum** + `Client` dispatch; env-var keys (`CODEX_CLI_PATH`, `GEMINI_CLI_PATH`, `GOOGLE_OAUTH_CLIENT_ID`, etc.).

### Phase 4 — Client API parity (v0.10.0) — ~1 week

- `Client.chat_with(request: ChatRequest)` / `Client.stream_with(request)` methods (full `ChatRequest` passthrough; no re-mapping).
- `Client.stream_collect(messages, **kwargs) -> ChatResponse` — streams and assembles.
- `Client.stream_collect_with(request) -> ChatResponse`.
- Fluent builder integration: `Client.chat(builder.build())` style documented.
- Drop/soft-deprecate `chat_sync()` from public surface — already removed from CLAUDE.md rule ("No sync wrappers in Python").
- Doc pass: `README.md`, `AGENTS.md`, `llms.txt`, `skills/motosan-ai/SKILL.md`.

---

## Architectural Decisions

### Discriminated unions in Python

Rust `enum` with `#[serde(tag = "type")]` maps cleanly to Python via `Literal` + `TypeAlias`:

```python
@dataclass
class TextBlock:
    text: str
    type: Literal["text"] = "text"

@dataclass
class ImageBlock:
    source: ImageSource
    type: Literal["image"] = "image"

ContentBlock = TextBlock | ImageBlock | DocumentBlock
```

Dispatch uses `match block: case TextBlock(text=t): ...` (Python 3.11+ — already our floor per `pyproject.toml`).

### `ToolChoice` as factory-constructed dataclass

Not a union — Rust serialization is `{"type": "<variant>"}` with optional `name`, so one `ToolChoice(type, name=None)` dataclass + classmethod factories keeps the public API small and mypy-friendly.

### Builder pattern

Python dataclasses already accept keyword-only args, but the Rust SDK uses fluent builder (`ChatRequest::builder().messages(...).system(...).build()`). Adding a `ChatRequestBuilder` class gives:
- Feature parity for docs / examples / code translation.
- Convenience methods (`system_cached`, `mcp_server` auto-adding `McpToolConfig::All`) that would otherwise be awkward as dataclass construction.

Keep the dataclass constructor usable — builder is additive.

### Provider trait → Python Protocol

Rust's `#[async_trait] pub trait ProviderImpl` becomes `typing.Protocol` with async methods. `validate_request()` can't live on a Protocol with a default implementation, so we add a concrete `BaseProvider` ABC with default `validate_request()` + abstract `chat`/`stream`, and have each provider inherit it. Matches Rust default-method behavior.

### Phased serialization rollout

Phase 1 adds types but does **not** serialize new fields to providers — existing wire format stays intact, existing tests pass unchanged. Phase 2 flips serialization on per-provider.

---

## Version Milestones

| Version | Phase | Scope | Gates |
|---------|-------|-------|-------|
| v0.6.0 | 1 | Types foundation | `check-python` + new type tests |
| v0.7.0 | 2a | Anthropic depth | Live-test suite incl. vision / thinking / caching / MCP |
| v0.8.0 | 2b | Gemini HTTP | Gemini live-test suite |
| v0.9.0 | 3 | CLI + OAuth | Codex / Gemini-CLI integration tests + OAuth flow test |
| v0.10.0 | 4 | Client API parity | Full `check-all` + docs sync |

---

## Non-Goals

- **No sync wrappers.** `chat_sync()` already exists for legacy reasons but no new sync APIs. Callers use `asyncio.run()`. (Per `CLAUDE.md`.)
- **No shared code between SDKs.** Each SDK is idiomatic. No FFI. (Per `CLAUDE.md`.)
- **No breaking `LlmClient` Protocol.** motosan-chat depends on it.
- **No bundling of provider SDKs.** Continue using `httpx` directly.
- **No `motosan-ai-oauth` Python port in Phase 3.** Gemini Code Assist OAuth is inline in the provider. A separate `motosan-ai-oauth` Python package is out of scope for this roadmap.

---

## Risks & Blockers

1. **Gemini wire format drift** — Google revises the Generative Language API shape periodically. Mitigation: generate live tests from schema, run pre-merge.
2. **OAuth token storage** — Python needs a keyring / file-based token cache. Mitigation: reuse `$HOME/.config/motosan-ai/tokens.json` layout from the Rust OAuth crate; document in Phase 3 plan.
3. **CLI subprocess flakiness on Windows** — Rust's CLI backends are tested on macOS/Linux only. Python will inherit. Mitigation: mark Windows-flaky tests with xfail; document in provider README.
4. **Phase 2a scope creep** — Anthropic has the most features. Mitigation: split into 2a (wire-format) and 2b (Gemini) releases, don't block each other.
