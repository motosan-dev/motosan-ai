# Changelog

All notable changes to `motosan-ai` Python SDK are documented in this file.

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
