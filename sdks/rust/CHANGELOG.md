# Changelog

All notable changes to `motosan-ai` Rust SDK are documented in this file.

## [0.3.3] - 2026-03-18

### Fixed
- **Anthropic OAuth `chat()` tool_calls**: OAuth path now correctly collects `ToolCallStart`/`ToolCallArgs`/`ToolCallEnd` stream events into `ChatResponse.tool_calls` (previously returned empty)
- **Anthropic OAuth system prompt**: system prompt now sent as separate blocks (Claude Code prefix with `cache_control` + user system without) instead of merged single block (fixes `invalid_request_error`)
- **Mock test header matching**: OAuth `anthropic-beta` header uses regex match instead of exact string

### Added
- **Live integration tests** (`tests/anthropic_live.rs`): 7 tests hitting real Anthropic API with OAuth token — chat, stream, system prompt, temperature, tool use (single + multi-turn), stream + tool use
- **Pre-push gate** (`scripts/pre-push-gate.sh`): blocks push unless unit + live tests pass

## [0.2.0] - 2026-03-15

### Added
- **Ollama provider** (`ollama` feature): connect to local or remote Ollama instances
  - Phase 1: `Provider::Ollama` via OpenAI-compatible endpoint (`/v1/chat/completions`)
  - Phase 2: `OllamaProvider` native implementation using `POST /api/chat` with NDJSON streaming
  - `think` mode: enable reasoning on qwen3-thinking, deepseek-r1 and other thinking models
  - `keep_alive`: control how long the model stays loaded in VRAM
  - `num_ctx`: override context window size via Modelfile options
  - `ollama_base_url()` builder: point to remote Ollama instance
  - `ollama_native(true)` builder: switch to native `/api/chat` endpoint
  - `ollama_think()`, `ollama_keep_alive()`, `ollama_num_ctx()` builder methods
  - `NdjsonStream`: custom `futures::Stream` adapter for NDJSON line parsing
  - Tool calls: auto-generates `call_{idx}` id when Ollama native omits it
  - `DEFAULT_OLLAMA_MODEL = "llama3.2"` in `models.rs`
- `feature = "full"` now includes `ollama`

## [0.1.4] - 2026-03-15

### Added
- `Client::stream_with(request: ChatRequest)` — stream with full `ChatRequest` (system, max_tokens, tools, temperature)

### Fixed
- Anthropic provider: `max_tokens` now defaults to `4096` when not set (Anthropic API requires this field; previously caused HTTP 400)


## [0.1.3] - 2026-03-11

### Added
- Multi-turn tool use support (fixes #72–#75):
  - `Message.tool_calls: Vec<ToolCall>` — carry tool calls in assistant messages
  - `Message::assistant_with_tool_calls()` constructor
  - `Message::tool_result()` / `Message::tool()` constructors for `Role::Tool`
- Anthropic: serialize assistant `tool_use` blocks in multi-turn requests
- OpenAI/MiniMax: serialize assistant `tool_calls` field in multi-turn requests

### Fixed
- Multi-turn tool use conversations now correctly reconstruct conversation history

## [0.1.1] - 2026-03-11

### Added
- MiniMax compatibility improvements:
  - Migrated to OpenAI-compatible MiniMax endpoint (`/chat/completions`).
  - Added payload-level `base_resp` error mapping with better auth/rate-limit/request semantics.
  - Added optional reasoning exposure control and default `<think>...</think>` stripping.
  - Added fallback to `reasoning_content` for chat and stream parsing.
  - Merged MiniMax system prompts into first user message for better endpoint compatibility.
- OpenAI provider enhancements:
  - Structured stream error parsing and empty-stream-chunk suppression.
  - `reasoning_content` fallback for chat and stream parsing.
  - Configurable auth style (`Bearer`, `x-api-key`, custom header).
  - Optional `/v1/responses` fallback when `/v1/chat/completions` returns `404`.
- `ClientBuilder` OpenAI options:
  - `openai_auth_bearer`, `openai_auth_x_api_key`, `openai_auth_custom_header`.
  - `openai_responses_fallback`.

### Changed
- Updated MiniMax default model to `MiniMax-M2.5-highspeed`.
- Expanded `MINIMAX_MODELS` catalog with M2.5/M2.1/M2 family entries.
- Expanded Rust README with OpenAI and MiniMax advanced behavior/configuration notes.

## [0.1.0] - 2026-03-10

### Added
- Feature-gated provider support: Anthropic, OpenAI, MiniMax (`anthropic`, `openai`, `minimax`, `full`).
- Unified core types: `Message`, `ChatRequest`, `ChatResponse`, `Usage`, `StopReason`, `StreamEvent`.
- `Message` helper constructors and `ChatRequestBuilder` for ergonomic request construction.
- `Client` APIs: `chat`, `chat_with`, and `stream`.
- Provider implementations for chat + streaming on Anthropic/OpenAI/MiniMax.
- Shared provider mapping utilities and robust SSE parsing behavior.
- Integration tests for provider happy paths, streaming behavior, and error mapping.
- Configurable retry policy (`RetryPolicy`) with exponential backoff, optional jitter, and `Retry-After` support.
- Rust CI workflow (`fmt`, `clippy`, `test`).

### Changed
- Centralized model defaults and model catalog in `src/models.rs`.
- Migrated SDK error type to `thiserror`-based `MotosanError`.
- Set MSRV to Rust `1.82` and added CI lane to validate no-feature builds/tests.

### Notes
- Default model baselines are maintained in `src/models.rs` and can be overridden via `ClientBuilder::model(...)` or `ChatRequest::builder().model(...)`.
