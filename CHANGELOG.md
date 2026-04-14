# Changelog

All notable changes to this project will be documented in this file.

## [0.9.0] — 2026-04-14

### Added

- **`StreamEvent.stop_reason`** — terminal stream events now carry the provider-reported stop reason. All three HTTP providers (Anthropic, OpenAI, MiniMax) populate it on the final `done` event. `collect_stream` honors the explicit reason and falls back to its existing tool-calls heuristic only when none is reported.
- **`StreamEvent::done_with_stop_reason(reason)`** constructor for adapters.
- **Live integration tests** for OpenAI and MiniMax (`tests/openai_live.rs`, `tests/minimax_live.rs`) — read `OPENAI_API_KEY` / `MINIMAX_API_KEY` and skip silently if absent. Verified end-to-end against production endpoints.

### Fixed

- **OpenAI/MiniMax streams emitted two `done` events** (pre-existing bug surfaced by the new live tests). One came from the `finish_reason` chunk (carrying `stop_reason`), one from `[DONE]` (carrying nothing). Callers using `events.last()` got the wrong one. Adapters now emit exactly one terminal `done` event, with `stop_reason` always attached when reported.
- **EOF flush** for OpenAI-compatible proxies that skip the `[DONE]` sentinel — adapters now emit a final `done` event from the upstream end-of-stream branch instead of terminating silently.

### Changed

- **`StreamEvent` gained one public field** (`stop_reason: Option<StopReason>`). Struct-literal constructors must add `stop_reason: None`; `StreamEvent::text` / `done` / `usage` / `tool_call_*` constructor users are unaffected.

### Tests

- 250 tests passing (was 229 in v0.8.0). New unit coverage for every stop reason variant across all three providers, EOF flush paths, and live tests against real Anthropic / OpenAI / MiniMax APIs.

## [0.8.0] — 2026-04-14

### Breaking

- **`OpenAIProvider` URL handling redesigned** — `base_url` parameter removed; replaced by full-URL `chat_url` + `responses_url` fields set via `.with_chat_url(...)` / `.with_responses_url(...)` builder methods. `ClientBuilder` gains matching `.openai_chat_url(...)` / `.openai_responses_url(...)` setters. Pass the literal URL you want POSTed — no more silent `/v1/chat/completions` injection. See `sdks/rust/CHANGELOG.md` for migration examples.

### Why

- Old heuristics double-appended `/v1` for providers like Groq (`https://api.groq.com/openai/v1`) or blocked full-URL proxies entirely.
- `endpoint()` and `responses_endpoint()` had inconsistent logic (one used string heuristics, the other didn't).
- New behavior aligns with `openai-python` / `openai-node`: the caller owns the URL, the SDK just POSTs.

### Docs

- Root `README.md`, `sdks/rust/README.md`, `llms.txt`, and `skills/motosan-ai/SKILL.md` all document the new `.openai_chat_url(...)` / `.with_chat_url(...)` pattern and explicitly call out Groq / DeepSeek / Together / self-hosted proxy support.
- 229 tests passing (unchanged from v0.7.0); 28 integration test call sites migrated to the new API.

## [0.7.0] — 2026-04-14

### Added

- **CodexCliClient** — new backend that shells out to OpenAI's `codex exec --json`, parallel to `ClaudeCodeClient`. Requires `--features codex-cli`.
  - `CodexCliClient::chat()` — blocking subprocess, parses JSONL events, splits multi-message turns into `content` (last `agent_message`) + `thinking` (preamble).
  - `CodexCliClient::stream()` — yields `StreamEvent`s as Codex emits complete `agent_message` items.
  - Builder: `.model()`, `.sandbox(SandboxMode)`, `.profile()`, `.ephemeral()`, `.cd()`, `.agent_mode()`, `.config_override(k, v)` (repeatable `-c key=value` escape hatch).
  - `SandboxMode` enum re-exported from `motosan_ai::codex_cli::SandboxMode`: `ReadOnly` / `WorkspaceWrite` / `DangerFullAccess`.
- **Feature flag** — `codex-cli` feature in `sdks/rust/Cargo.toml`.
- **Docs** — root README, `sdks/rust/README.md`, `skills/motosan-ai/SKILL.md`, `llms.txt`, and `AGENTS.md` all document the new backend alongside `ClaudeCodeClient`.

### Limitations

- `CodexCliClient` does not surface `tool_calls` — tools run inside the Codex sandbox and are not reported.
- Only `codex exec` is supported; `resume` and `review` subcommands are out of scope.
- No native system-prompt flag: system prompts are prepended to the user prompt as a labeled `[system instructions]` block.

## [0.6.0] — 2026-04-05

### Added

- **ClaudeCodeClient** — new backend that shells out to the `claude` CLI binary, enabling chat and streaming without an API key. Requires `--features claude-code`.
  - `ClaudeCodeClient::chat()` — blocking subprocess invocation returning `ChatResponse`
  - `ClaudeCodeClient::stream()` — NDJSON streaming via `--output-format stream-json`
  - Configurable binary path (`CLAUDE_CODE_PATH` env or builder), agent mode, model override
- **README** — documented all three LlmClient backends (API key, OAuth token, Claude Code CLI)
- **Feature flag** — `claude-code` feature in Cargo.toml gating the new backend

### Limitations

- `ClaudeCodeClient` does not support tool calling — `tool_calls` is always an empty vec
- Requires `claude` CLI installed and authenticated locally
