# Changelog

All notable changes to `motosan-ai` Rust SDK are documented in this file.

## [0.7.0] - 2026-04-14

### Added
- **`codex-cli` feature**: `CodexCliClient` — shells out to OpenAI's `codex exec --json` as a fifth LLM backend, alongside the four HTTP providers and `ClaudeCodeClient`.
  - `CodexCliClient::new()` resolves the binary from `CODEX_PATH` env or `"codex"` in `PATH`.
  - `CodexCliClient::chat(request)` — spawns `codex exec --json --skip-git-repo-check -`, writes the prompt to stdin, parses the JSONL event stream, and returns a `ChatResponse`. Treats the last `agent_message` as `content` and folds prior agent messages (preamble / tool narration) into `thinking`.
  - `CodexCliClient::stream(request)` — same spawn, yields `StreamEvent`s as Codex emits them. Codex produces complete `agent_message` items (not token deltas), so each text event is one finalized message.
  - Builder flags: `.model(m)` (`--model`), `.sandbox(SandboxMode)` (`--sandbox`), `.profile(name)` (`--profile`), `.ephemeral(bool)` (`--ephemeral`), `.cd(dir)` (`--cd`), `.agent_mode(bool)` (`--full-auto`), `.config_override(key, value)` (repeatable `-c key=value`).
  - `SandboxMode` enum: `ReadOnly` / `WorkspaceWrite` / `DangerFullAccess`.
  - 600-second hard timeout on subprocess invocation, `kill_on_drop` for cancel-safety.
- **Comprehensive rustdoc** for the `codex_cli` module: module-level overview, per-field docs on `CodexCliClient`, error contracts on `chat` / `stream`, full event-schema documentation on `stream_json.rs`.

### Limitations
- `CodexCliClient` does not surface `tool_calls` — Codex runs shell, file edits, and MCP tools inside its own sandbox; those invocations are not reported as crate-level tool calls.
- Only `codex exec` is supported. `codex exec resume` (session continuation) and `codex review` are out of scope.
- Codex CLI has no native `--system` flag; system prompts are prepended to the user prompt as a labeled `[system instructions]` block.

## [0.6.0] - 2026-04-05

### Added
- **`claude-code` feature**: `ClaudeCodeClient` — shells out to the `claude` CLI binary as a fourth LLM backend.
  - `ClaudeCodeClient::new()` resolves binary from `CLAUDE_CODE_PATH` env or `"claude"` in `PATH`.
  - `ClaudeCodeClient::chat(request)` — blocking subprocess via `--print`, supports `agent_mode` with JSON output parsing.
  - `ClaudeCodeClient::stream(request)` — NDJSON streaming via `--print --output-format stream-json`, yields `StreamEvent` items.
  - `.model(model)` builder: forwards `--model <value>` when non-empty and not `"default"` (case-insensitive); skips otherwise.
  - `.agent_mode(bool)` builder: enables `--dangerously-skip-permissions`.
  - Resolves binary path from `CLAUDE_CODE_PATH` env var with fallback to `"claude"`.

### Changed
- `DEFAULT_MAX_TOKENS` raised from `4096` to `8192` for the Anthropic provider.

## [0.5.4] - 2026-03-31

### Changed
- Upgrade `motosan-agent-tool` dependency from 0.2 to 0.3.

## [0.5.3] - 2026-03-30

### Fixed
- Fix `cargo fmt` formatting in `client.rs` that blocked CI publish for v0.5.2.

## [0.5.2] - 2026-03-30

### Added
- Configurable **stream read timeout** via `ClientBuilder::stream_read_timeout_secs(secs)` — terminates SSE streams that stop sending events mid-stream, preventing indefinite hangs (#155).
- `MotosanError::StreamReadTimeout` error variant for timeout-specific error handling.

### Fixed
- `ThinkStripper`: split on UTF-8 char boundaries to avoid panic on multi-byte characters.

## [0.5.1] - 2026-03-24

### Fixed
- Merge `anthropic-beta` headers into a single header when OAuth + MCP are both active (#149).
- `has_mcp` now checks both `mcp_servers` and `mcp_tool_configs` (#150).
- `mcp_toolset` serialization uses `mcp_server_name` instead of `server_label` (#153).

## [0.5.0] - 2026-03-24

### Added
- `agent-tool` feature gate with `motosan-agent-tool` integration (`From<ToolDef> for Tool`, optional dependency).
- `collect_stream()` helper and `Client::stream_collect` methods for buffering stream into `ChatResponse`.
- `ToolChoice` enum for controlling tool selection (`Auto`, `Any`, `None`, `Specific`).
- First-class extended thinking support in `ChatRequest`.
- Server-side MCP support in `ChatRequest`.

### Fixed
- Capture usage tokens from stream events in OAuth collect path.
- Fail-fast on missing `tool_call_id` + clarify Null args handling.

## [0.4.0] - 2026-03-21

### Added
- **Vision / Multimodal content support** — send images alongside text in messages
  - `ContentBlock` enum: `Text { text }` and `Image { source }` variants
  - `ImageSource` enum: `Base64 { media_type, data }` and `Url { url }` variants
  - `Message::user_with_image(text, base64_data, media_type)` — create a message with text + base64 image
  - `Message::user_with_blocks(blocks)` — create a message with arbitrary content blocks
  - `Message.content_blocks: Vec<ContentBlock>` field (backward compatible, defaults to empty)
- **Anthropic provider**: serializes `content_blocks` as `{"type": "image", "source": {"type": "base64", ...}}` format (works with both API key and OAuth streaming path)
- **OpenAI provider**: serializes `content_blocks` as `{"type": "image_url", "image_url": {"url": "data:...;base64,..."}}` format

### Fixed
- **Anthropic OAuth streaming path**: content_blocks now correctly serialized in the OAuth streaming code path (previously only the non-streaming path handled them)

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
