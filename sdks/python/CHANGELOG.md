# Changelog

All notable changes to `motosan-ai` Python SDK are documented in this file.

## [0.4.1] - 2026-03-18

### Added
- **Retry with exponential backoff** for rate limit (429) errors — `Client.chat()` and `Client.stream()` auto-retry up to `max_retries` times (default 3) with 1s, 2s, 4s backoff (capped at 30s)
- `max_retries` parameter on `Client` constructor and all classmethods (`anthropic()`, `openai()`, `minimax()`, `ollama()`)
- `Retry-After` header parsing — uses server-suggested wait time when available
- New `motosan_ai.retry` module with `with_retry()` utility

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
