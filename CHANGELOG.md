# Changelog

All notable changes to this project will be documented in this file.

## [rust-0.17.1 / python-0.12.1] — 2026-05-29

### Added

- Anthropic model catalog now includes `claude-opus-4-8`.
- Rust and Python live Opus 4.8 adaptive-thinking regression tests. Verified live with an `sk-ant-oat01-*` OAuth token.

### Changed

- Anthropic extended thinking for Opus 4.8/4.7/4.6 now follows pi's adaptive-thinking shape (`thinking.type = "adaptive"`, summarized display, `output_config.effort = "high"`) instead of the older budget-token shape. OAuth adaptive-thinking requests omit the legacy `interleaved-thinking` beta header, matching pi.
- Python budget-based Anthropic thinking now also sends `display: "summarized"`, matching Rust/pi OAuth thinking-stream behavior.

## [rust-0.17.0] — 2026-05-29

### Breaking (Rust only)

- **`motosan-agent-tool` dep bumped to 0.5** (M10 D-M10-4). `ToolDef` gained a required `internal_name: String` field. The `From<Tool> for ToolDef` conversion in `tool_compat.rs` now routes through `ToolDef::new(...)`, which sets `internal_name = name` automatically — the motosan-ai SDK has no host-side namespace concept, so the public `name` is the right identifier on both axes. `Tool ↔ ToolDef` round-trips remain lossless. Consumers of `motosan-ai` Rust SDK with `--features agent-tool` must bump their `motosan-agent-tool` dep alongside.

See [`sdks/rust/CHANGELOG.md`](sdks/rust/CHANGELOG.md) for the canonical Rust SDK changelog with full per-release detail.

## [rust-0.16.0] — 2026-05-26

### Breaking (Rust only)

- **`motosan-agent-tool` dep bumped to 0.4**. Consumers of `motosan-ai` Rust SDK with `--features agent-tool` must bump their `motosan-agent-tool` dep alongside. No public SDK signature changed at the type level; the semver bump reflects the transitive crate identity change.

See [`sdks/rust/CHANGELOG.md`](sdks/rust/CHANGELOG.md) for the canonical Rust SDK changelog with full per-release detail.

## [rust-0.15.5] — 2026-05-23

### Fixed

- **Anthropic provider sends `display: "summarized"` in the thinking config.** Without it the OAuth product surface (`sk-ant-oat01-*` tokens issued by Claude Code subscriptions) silently defaults the thinking display to `"omitted"` for all models — Anthropic accepts the request but returns zero `thinking_delta` SSE events. With the explicit `summarized` the OAuth tier behaves like direct API key callers and streams thinking content per-delta. Patch covers both non-streaming and streaming OAuth body builders (`sdks/rust/src/providers/anthropic.rs`). Verified end-to-end against `claude-sonnet-4-6` via a Claude Pro OAuth token.

## [rust-0.14.0] — 2026-04-21

### Breaking (Rust only)

- **MiniMax provider path switched to Anthropic-compatible API**: `Provider::Minimax` now routes through `AnthropicProvider` and sends requests to `/anthropic/v1/messages`.
- **Removed legacy `MinimaxProvider`** (`sdks/rust/src/providers/minimax.rs`).
- **Removed `ClientBuilder::minimax_expose_reasoning(bool)`**.
- **Removed `DEFAULT_MINIMAX_MODEL`** export.

### Added (Rust only)

- **`ClientBuilder::minimax_base_url(...)`** for MiniMax endpoint override. Defaults to `https://api.minimax.io/anthropic`; CN users can set `https://api.minimaxi.com/anthropic`.
- **`AnthropicProvider::with_capabilities(...)`** for instance-level capability overrides (used by MiniMax routing as text-only).

### Changed (Rust only)

- MiniMax default model is now `MiniMax-M2.7`.
- `minimax` Cargo feature now aliases `anthropic` (`minimax = ["anthropic"]`).

### Tests

- Added `sdks/rust/tests/anthropic_minimax_routing.rs`.
- Updated existing Rust tests to remove assumptions about the deleted OpenAI-compatible MiniMax path.

## [rust-0.13.1] — 2026-04-20

### Added (Rust only)

- **`ProviderCapabilities`** — new type in `types.rs` (`pub use motosan_ai::ProviderCapabilities`) with `supports_image: bool` and `supports_document: bool` fields. Named constructors: `text_only()`, `with_image()`, `full()`.

- **`ProviderImpl::capabilities()`** — new provided method (default: `text_only()`). Providers that support image or document content blocks override it: `AnthropicProvider` → `full()`, `OpenAIProvider` / `GeminiProvider` / `GeminiCodeAssistProvider` → `with_image()`. All others (MiniMax, Ollama, CLI backends) use the default and require no changes.

- **`ProviderImpl::validate_request()`** — new provided method. Iterates `ChatRequest.messages[*].content_blocks`, returns `Err(MotosanError::UnsupportedFeature(...))` for any `ContentBlock::Image` or `ContentBlock::Document` that the provider's `capabilities()` does not declare. Validation fires inside `LlmClient::dispatch_chat` and `dispatch_stream_inner` before any network call.

### Removed (Rust only)

- **`reject_document_blocks()`** internal helper — removed from `providers/mod.rs`, `openai.rs`, `minimax.rs`, `ollama.rs`. Superseded by the framework-level `validate_request()` check.

### Tests

- `sdks/rust/tests/vision_gemini.rs` — 6 mock tests covering `GeminiProvider` image serialization (`inlineData` / `fileData` / mixed text+image), mirroring `vision_anthropic.rs` and `vision_openai.rs`.
- `sdks/rust/tests/gemini_vision_live.rs` — live test for `GeminiProvider` image input (requires `GOOGLE_API_KEY`).

## [rust-0.13.0] — 2026-04-20

### Added (Rust only)

- **`Provider::Gemini`** — native HTTP client for the Google Generative AI REST API (`generativelanguage.googleapis.com`). Feature flag `gemini`. Authenticates via `x-goog-api-key` header. Supports `chat()` and `stream()` with full SSE adapter. Handles `system_blocks`, tool declarations, `ToolChoice`, image content blocks, stop sequences, `provider_options` passthrough, and retry on 429/5xx. `finishReason` maps: `STOP` → `EndTurn`, `MAX_TOKENS` → `MaxTokens`, anything else (e.g. `SAFETY`) → `Other`. Default model: `gemini-2.0-flash`. **Gemini-specific convention**: `Message::tool_result` must use the function name (not an opaque ID) as `tool_call_id` — the Gemini API requires `functionResponse.name` to be the function name.

- **`Provider::GeminiCodeAssist`** — native HTTP client for Google Cloud Code Assist (`cloudcode-pa.googleapis.com/v1internal`). Feature flag `gemini-code-assist` (depends on `gemini`). Authenticates via `Authorization: Bearer <ya29.* OAuth token>` obtained from the `motosan-ai-oauth` PKCE flow. Requires a GCP project ID (`ClientBuilder::gemini_code_assist_project_id()`). Only has a streaming endpoint; `chat()` is implemented internally as `stream()` + collect. Request wraps Gemini content in `{ project, model, request: {...}, requestId, userAgent }`. SSE response wrapped in `{ response: { candidates: [...] } }`. Uses API-provided tool call IDs when present; generates `{fn_name}_{ts}_{counter}` otherwise. Default model: `gemini-2.5-flash` (required for standard-tier accounts; `gemini-2.0-flash` is not available on `cloudcode-pa`). Billing: subscription-based (per seat), not per-token.

- **`motosan-ai-oauth` Gemini provider config** — `providers::gemini()` returns PKCE config for the Gemini CLI client credentials (`cloud-platform` scope). Used to obtain `ya29.*` tokens for `GeminiCodeAssist`.

## [rust-0.12.1] — 2026-04-19

### Added (Rust only)

- **`ClaudeCodeProvider.bare(bool)`** — forwards `--bare` to the spawned `claude` subprocess (skips hooks, plugins, auto-memory, keychain reads, and user/project settings discovery). Intended for daemon / server embeddings that must not inherit the operator's interactive Claude Code state. Leave `false` (default) for workflows that should pick up `~/.claude/` configuration.

## [0.11.1] — 2026-04-15

### Docs

- **Root `README.md`**: Providers table now lists Claude Code CLI and Codex CLI alongside HTTP providers. Features section gains a "Unified dispatch" bullet.
- **`skills/motosan-ai/SKILL.md`**: minimal Rust example now covers both HTTP and CLI backend paths through `Client::builder()`.
- **`llms.txt`**: `Provider` variant list updated to include `ClaudeCode` / `CodexCli`. Notes that CLI backends go through the same `client.chat()` / `client.stream()` API and that `api_key` is optional for those paths.

Pure docs patch — no code changes from v0.11.0.

## [0.11.0] — 2026-04-14

### Breaking

- **`Provider` enum** gained `Provider::ClaudeCode` and `Provider::CodexCli` variants. Exhaustive `match` on `Provider` without a `_ =>` catch-all will fail to compile.
- **Removed deprecated `ClaudeCodeClient` / `CodexCliClient` type aliases** (the one-version grace period from v0.10.0 is over). Use `ClaudeCodeProvider` / `CodexCliProvider` directly.

### Added

- **`Client::builder()` now dispatches to CLI backends**, closing the v0.10.0 promise. A single `Client` can hold any backend (HTTP or CLI) and expose it through the unified `chat()` / `stream()` API. New setters: `.claude_code(ClaudeCodeProvider)` and `.codex_cli(CodexCliProvider)`, each accepting a pre-built provider instance.
- **`api_key` is now optional on `build()`** when the selected provider is a CLI backend. CLI backends authenticate via their own channels.
- **Live integration test** for the end-to-end dispatch path (`Client::builder().provider(Provider::CodexCli).build()` → `.chat()` → real `codex exec` spawn).

### Migration

```rust
// Before (v0.10.x)
use motosan_ai::CodexCliProvider;
let provider = CodexCliProvider::new().sandbox(SandboxMode::WorkspaceWrite);
provider.chat(request).await?;

// After (v0.11.0) — same thing works, or unified via Client::builder:
let client = Client::builder()
    .provider(Provider::CodexCli)
    .codex_cli(CodexCliProvider::new().sandbox(SandboxMode::WorkspaceWrite))
    .build()?;
client.chat(vec![Message::user("hi")]).await?;
```

### Tests

- 267 tests passing (was 264 in v0.10.1). +3 unit + 1 new live integration test verified against real `codex` binary.

## [0.10.1] — 2026-04-14

### Fixed

- **OpenAI / MiniMax streams now guarantee exactly one terminal `done` event** even when the upstream provider closes the connection without `[DONE]` and without any `finish_reason` chunk. Previously such streams terminated silently, hanging callers that wait for `done==true`. Adapters now track `done_emitted` and flush a final `done()` from the EOF branch when needed.

### Added

- **Regression tests** for the EOF flush guarantee (OpenAI + MiniMax) and for the historical double-`done` bug fixed in v0.9.0.
- **Codex CLI live integration test** (`integration_chat_with_v0_9_2_flags`) that real-spawns `codex exec` with `--add-dir` + `--enable fast_mode` + `--disable image_generation` + `--sandbox read-only` + `--ephemeral`. Catches flag-name regressions on Codex CLI upgrades.
- **`codex_cli` rustdoc example** is now compile-checked (`no_run` instead of `ignore`). Corrected to use the real `ChatRequest::builder()` API.

### Tests

- 264 tests passing (was 259 in v0.10.0). +4 unit + 1 newly compile-checked doc-test. One additional ignored live test for Codex flag plumbing.

## [0.10.0] — 2026-04-14

### Breaking

- **CLI backend types renamed**: `ClaudeCodeClient` → `ClaudeCodeProvider`, `CodexCliClient` → `CodexCliProvider`. Old names kept as `#[deprecated]` type aliases — existing code compiles with a warning. Both CLI backends moved from top-level modules into `providers/`, so every provider (HTTP + CLI) now lives under one umbrella.

### Migration

```rust
// Before
use motosan_ai::{ClaudeCodeClient, CodexCliClient};

// After
use motosan_ai::{ClaudeCodeProvider, CodexCliProvider};
```

### Why

- After v0.9.1's `impl ProviderImpl` for both CLI backends, the only remaining difference vs HTTP providers was naming and module path. v0.10.0 closes that gap so all providers are structurally identical.

## [0.9.2] — 2026-04-14

### Added

- **`CodexCliClient` exposes 6 more `codex exec` flags via typed builders**: `.add_dir()`, `.enable_feature()`, `.disable_feature()`, `.dangerously_bypass_approvals_and_sandbox()`, `.oss()`, `.local_provider(LocalProvider)`. New `LocalProvider` enum (`LmStudio` / `Ollama`). Pure additive — every existing call site still compiles. Closes the gap between what `codex exec --help` exposes and what the SDK wraps.

## [0.9.1] — 2026-04-14

### Added

- **`CodexCliClient` and `ClaudeCodeClient` now implement `ProviderImpl`.** Both CLI backends can now be used polymorphically via `Box<dyn ProviderImpl>` / `&dyn ProviderImpl` alongside the HTTP providers (Anthropic, OpenAI, MiniMax, Ollama). Pure additive change — existing inherent `chat()` / `stream()` calls still work. Unlocks downstream consumers (e.g. `motosan-chat`'s `MotosanAiClient`) that want a single trait object holding any backend.

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
