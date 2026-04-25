# Changelog

All notable changes to `motosan-ai` Python SDK are documented in this file.

## [0.9.0] - 2026-04-25

### Added — ClaudeCodeClient full flag surface parity with Rust v0.12.0+
- **Builder state consolidated** into internal `_ClaudeCodeConfig` dataclass; backward-compatible property shims preserve `_binary_path` / `_model` / `_agent_mode` access.
- **26 new fluent builder methods** covering string flags, list flags, MCP config, setting sources, sessions, plugin dirs, named agents, and budget controls.
- **Stream usage events** — Claude Code NDJSON `result` events with `usage` now emit `StreamEvent(event_type="usage")` before terminal `done`, matching Rust.

### Notes
- No breaking changes to `ClaudeCodeClient()` / `.model()` / `.agent_mode()`.
- Subprocess argv composition is aligned with the Rust `ClaudeCodeProvider` flag wiring for equivalent configs.
- Covers Phase 3a of the Python SDK catch-up roadmap; Codex CLI, Gemini CLI, and Gemini Code Assist OAuth remain in later 0.9.x phases.

## [0.8.2] - 2026-04-25

### Fixed
- **Anthropic extended thinking via OAuth** — the streaming SSE adapter silently dropped `thinking_delta` events, so `ChatResponse.thinking` was always `None` on OAuth tokens (which route through stream+collect). Now emits `StreamEvent(event_type="thinking")`; the OAuth `chat()` collector accumulates into `ChatResponse.thinking`.
- **Gemini default model** — `gemini-2.0-flash` was deprecated for new users (returns HTTP 404). Default bumped to `gemini-2.5-flash`. All mock-URL references, live tests, and parity conftest updated.
- **Live vision fixtures** — replaced 1×1 transparent PNG (~67 bytes) with a 64×64 solid-red PNG (187 bytes). Anthropic and Gemini both reject sub-minimum images; the old fixture returned `HTTP 400: Could not process image` / `Unable to process input image`.

### Added
- `test_stream_emits_thinking_deltas_as_thinking_event` + `test_oauth_chat_collects_thinking_from_stream` regression tests.

## [0.8.1] - 2026-04-24

### Added
- **Test infrastructure — drift detection**
  - `tests/_snapshots.py` helper: JSON-file snapshots with `UPDATE_SNAPSHOTS=1` regenerate mode.
  - `tests/parity/` cross-provider matrix tests for simple chat, `ToolChoice`, vision, and stream event contracts.
  - `tests/test_client_integration.py` provider dispatch matrix, env-var fallback, and retry end-to-end coverage.
  - Nightly CI workflow (`.github/workflows/ci-python-nightly.yml`) runs live integration tests against real provider APIs.

### Fixed
- **OpenAI vision serialization** — `OpenAIProvider._serialize_messages` now emits `content_blocks` as `image_url` parts (base64 → data URI, URL → raw URL). Previously, `Message.user_with_image(...)` silently dropped the image.

### Notes
- Snapshots are code-review artifacts. Any diff in `tests/snapshots/*.json` is a deliberate wire-format change and should be reviewed like a schema migration.
- Nightly live-test secrets must be configured in GitHub repo settings (`ANTHROPIC_API_KEY`, `OPENAI_API_KEY`, `GEMINI_API_KEY`, `MINIMAX_API_KEY`).

## [0.8.0] - 2026-04-24

### Added
- **`GeminiProvider`** — native HTTP client for Google's Generative Language REST API (`generativelanguage.googleapis.com/v1beta`).
  - `Provider.gemini` registered in `Client` dispatch.
  - `Client.gemini(api_key=..., model=..., base_url=...)` classmethod.
  - `GEMINI_API_KEY` env var loaded automatically.
  - Default model: `gemini-2.0-flash`.
  - Full feature coverage: text, vision (base64 + URL), tools (`functionDeclarations`), tool choice (AUTO / ANY / allowedFunctionNames), streaming (`streamGenerateContent?alt=sse`), system prompts (`systemInstruction`), stop sequences (`stopSequences`), usage reporting (`promptTokenCount` / `candidatesTokenCount`).
  - Capabilities: `with_image()` — document blocks raise `InvalidRequestError` before any HTTP call.
  - Tool call IDs are generated client-side. By convention, `Message.tool_result(tool_call_id=<function_name>, ...)` uses the function name as the ID for Gemini round-trips.
- **Live integration tests** (`tests/integration/test_gemini_live.py`): simple chat, vision, tool use, streaming.

### Notes
- Gemini does not support document (PDF) input; calls with `ContentBlock::Document` fail at validation time.
- No cache token accounting on Gemini — `Usage.cache_creation_input_tokens` / `cache_read_input_tokens` always `None`.
- See `docs/superpowers/plans/2026-04-24-python-sdk-catchup-roadmap.md` for the full catch-up roadmap.

## [0.7.0] - 2026-04-24

### Added — Anthropic wire-format parity with Rust SDK
- **Vision & PDF input** — user messages with `content_blocks` now serialize as Anthropic content-block arrays, including image/document base64 and URL sources.
- **Prompt caching** — `Message.cache`, `SystemBlock[]`, `system_cache=True`, and `Tool.cache` now emit Anthropic `cache_control`; cache creation/read usage tokens are parsed.
- **ToolChoice** — `auto`, `required` (Anthropic `any`), `none` (removes tools), and `tool(name)` are supported.
- **Extended thinking** — `ThinkingConfig` serializes as enabled thinking, forces `temperature=1.0`, and non-stream responses parse thinking blocks into `ChatResponse.thinking`.
- **MCP server-side tools** — `mcp_servers` and `mcp_tool_configs` serialize for Anthropic; `anthropic-beta: mcp-client-2025-11-20` is attached when needed.
- **Stop sequences** — `stop_sequences` serialize and `StopReason.stop_sequence` is parsed.
- **Stream enhancements** — `StreamEvent.usage` is emitted from Anthropic `message_start` / `message_delta`, and terminal done events carry `stop_reason` when provided.

### Changed
- `AnthropicProvider` now inherits `BaseProvider`; `validate_request()` runs before HTTP work in `chat()` and `stream()`.
- Anthropic request building now uses one unified serializer for OAuth and standard-key paths.

### Notes
- Only Anthropic gained Phase 2a wire-format support in this release. Other providers remain scheduled in later roadmap phases.

## [0.6.0] - 2026-04-24

### Added
- **Type foundation for Rust SDK parity** — additive-only. No wire-format changes yet.
- `ContentBlock` discriminated union (`TextBlock` / `ImageBlock` / `DocumentBlock`).
- `ImageSource` (`ImageSourceBase64` / `ImageSourceUrl`) and `DocumentSource` (`DocumentSourceBase64` / `DocumentSourceUrl`).
- `Message.user_with_image()`, `Message.user_with_blocks()`, `Message.user_with_pdf_base64()`, `Message.user_with_pdf_url()`, `Message.user_with_pdf_bytes()`.
- `Message.cache` field + `Message.user_with_cache()` + `Message.with_cache()`.
- `SystemBlock` with `SystemBlock.new()` / `SystemBlock.cached()` factories.
- `Tool.cache` field.
- `ToolChoice` with `auto()` / `required()` / `none()` / `tool(name)` factories.
- `ThinkingConfig` (budget_tokens) for extended thinking.
- `McpServerConfig` and `McpToolConfig*` (All / Allowed / Denied) for server-side MCP.
- `ChatRequest` fields: `system_blocks`, `system_cache`, `tool_choice`, `mcp_servers`, `mcp_tool_configs`, `thinking`, `stop_sequences`.
- `ChatRequest.builder()` returning `ChatRequestBuilder` (fluent API parity with Rust SDK).
- `ChatResponse.thinking` field.
- `Usage.cache_creation_input_tokens` and `Usage.cache_read_input_tokens`.
- `StopReason.stop_sequence` variant.
- `StreamEventType` enum; `StreamEvent.usage` and `StreamEvent.stop_reason` fields.
- `ProviderCapabilities` (`text_only` / `with_image` / `full`) declared on each provider.
- `BaseProvider` ABC with default `validate_request()` enforcing capabilities.

### Changed
- Capability declarations per provider: `Anthropic` = `full`, `OpenAI` = `with_image`, `Minimax` = `with_image`, `Ollama` = `text_only`, `ClaudeCodeClient` = `text_only`.

### Notes
- **No new wire-format behavior in 0.6.0.** Providers still serialize request bodies as before. Phase 2 (v0.7.0+) will wire `content_blocks`, `system_blocks`, `tool_choice`, `thinking`, and MCP config into the Anthropic and Gemini providers.
- See `docs/superpowers/plans/2026-04-24-python-sdk-catchup-roadmap.md` for the full catch-up roadmap.

## [0.5.0] - 2026-04-05

### Added
- **`ClaudeCodeClient`** — fourth LLM backend that shells out to the `claude` CLI binary (parity with Rust SDK `claude-code` feature).
  - `ClaudeCodeClient()` resolves binary from `CLAUDE_CODE_PATH` env or `"claude"` in `PATH`.
  - `ClaudeCodeClient.with_path(path)` — explicit binary path.
  - `.model(model)` — forwards `--model <value>` when non-empty and not `"default"` (case-insensitive, trimmed).
  - `.agent_mode(bool)` — enables `--dangerously-skip-permissions` + JSON output parsing.
  - `async chat(request) -> ChatResponse` — subprocess via `--print`, 300 s timeout.
  - `async stream(request) -> AsyncIterator[StreamEvent]` — NDJSON streaming via `--print --output-format stream-json --verbose`, per-line 300 s timeout, subprocess always cleaned up via `try/finally`.
  - `ClaudeCodeClient` exported from `motosan_ai` top-level.

## [0.4.2] - 2026-03-18

### Added
- **Retry with exponential backoff** for transient errors — `Client.chat()` and `Client.stream()` auto-retry on 429, 5xx, and network errors (default `max_retries=3`)
- `max_retries` parameter on `Client` constructor and all classmethods (`anthropic()`, `openai()`, `minimax()`, `ollama()`)
- `Retry-After` header parsing — uses server-suggested wait time when available
- New `motosan_ai.retry` module with `with_retry()` and `_is_retryable()` utilities

### Changed
- **Retry defaults aligned with Rust SDK** — `initial_backoff=0.1s`, `max_backoff=2.0s` (was 1s/30s)
- **Retry scope expanded** — now retries `RateLimitError` (429), `ProviderError` (5xx), and `NetworkError` (timeout/connection), matching Rust SDK behavior

## [0.4.1] - 2026-03-18

(Superseded by 0.4.2 — retry only covered 429, defaults misaligned with Rust)

## [0.4.0] - 2026-03-18

### Changed
- **Remove official SDK dependencies**: Anthropic and OpenAI providers rewritten to use `httpx` directly instead of `anthropic` and `openai` packages — zero official provider SDK dependencies
- **pyproject.toml**: `anthropic`, `openai`, `ollama` optional deps now all resolve to `httpx>=0.27`
- **All mock tests** migrated from `monkeypatch` + `FakeClient` to `respx` HTTP mocking
- **Anthropic default model** updated to `claude-sonnet-4-6`
- **OAuth streaming**: OAuth `chat()` now internally streams (Anthropic requires streaming for OAuth tokens) and collects into `ChatResponse`
- **OAuth system prompt**: sent as array of blocks with Claude Code prefix + `cache_control`
- **OAuth user messages**: serialized as content blocks (`[{"type": "text", "text": ...}]`) per Anthropic OAuth requirements

### Fixed
- **Anthropic OAuth `chat()` tool_calls**: OAuth path now correctly collects tool_call stream events into `ChatResponse.tool_calls` with proper `stop_reason=tool_use` (previously returned empty)
- **Anthropic OAuth system prompt**: system prompt now sent as separate blocks (Claude Code prefix with `cache_control` + user system without) instead of merged single string (fixes `invalid_request_error`)

### Added
- **Live integration tests** (`tests/integration/test_anthropic_live.py`): 7 tests hitting real Anthropic API with OAuth token — chat, stream, system prompt, temperature, tool use (single + multi-turn), stream + tool use
- **Pre-push gate** (`scripts/pre-push-gate.sh`): blocks push unless unit + live tests pass

## [0.3.3] - 2026-03-16

(Published to PyPI with anthropic SDK-based providers — superseded by 0.4.0)

## [0.2.1] - 2026-03-15

### Added
- **Ollama provider** (`Provider.Ollama`): connect to local or remote Ollama instances
  - Phase 1: OpenAI-compatible endpoint (`/v1/chat/completions`) — zero new dependencies
  - Phase 2: `OllamaProvider` native via `POST /api/chat` with NDJSON streaming
  - `think=True`: enable reasoning mode (qwen3-thinking, deepseek-r1, etc.)
  - `keep_alive`: control VRAM retention after request
  - `num_ctx`: override context window size
  - `ollama_base_url()`: point to remote Ollama instance (default: `http://localhost:11434`)
  - `ollama_native(True)`: switch to native `/api/chat` endpoint
  - `ollama_think()`, `ollama_keep_alive()`, `ollama_num_ctx()` builder methods
  - Tool calls: auto-generates uuid when Ollama native omits `id`
  - Default model: `llama3.2`

## [0.2.0] - 2026-03-12

### Added
- Initial Python SDK release
- `Provider.Anthropic`, `Provider.OpenAI`, `Provider.Minimax`
- Async `chat()`, `chat_with()`, `stream()` with full `ChatRequest` parameters
- `Message.assistant_with_tool_calls()`, `Message.tool_result()` for multi-turn tool use
- `ToolCall`, `StreamEvent`, `Usage`, `StopReason` types
- Pydantic-free implementation (dataclasses + TypedDict)
- `ClientBuilder` with provider, model, api_key, system, max_tokens, temperature
