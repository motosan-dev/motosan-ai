# AGENTS.md

Multi-provider AI SDK. Rust (`sdks/rust/`) + Python (`sdks/python/`) + TypeScript (`sdks/typescript/`). Independent idiomatic implementations — no shared runtime.

Rust v0.24.0 · Python v0.17.0 (PyPI) · TypeScript v0.14.0 (npm)

Python 0.13.0 adds CLI-runtime setters (`.cwd()`, session continuity via `session_id` + `resume()`, per-run `.env()/.envs()`, CLI tool-call stream events, configurable `.timeout()/.no_timeout()`) and a **breaking** fallible stream: HTTP provider `stream()` now raises `motosan_ai.error.StreamError` mid-stream instead of swallowing transport/parse faults (`collect_stream` propagates it; `Client.stream_with` does not retry after a mid-stream raise).

Python 0.14.0 and TypeScript 0.11.0 add the **chatgpt-codex** provider — a native ChatGPT-backend HTTP client over the OpenAI Responses API (`chatgpt.com/backend-api/codex/responses`; pre-obtained OAuth token + account id, no `api_key`). Python: `Client.chatgpt_codex(access_token, account_id, model, reasoning_effort=None)`; TypeScript: `Client.builder().chatgptCodex(accessToken, accountId, model?, { reasoningEffort })`. Mirrors the Rust `ChatGptCodexProvider`.

The M1 reliability releases: retry survives non-JSON 5xx bodies, mid-stream error frames, Claude Code terminal error results, and CLI child-process death surface as errors, parallel tool-call `index` and chatgpt-codex `item_id`→`call_id` are handled correctly, Rust/TypeScript streamed usage merges by replacement, Python streamed tool turns report the tool-use stop reason, and the TypeScript SSE reader cancels on abort and accepts CRLF.

Rust 0.23.0 / Python 0.16.0 / TypeScript 0.13.0 are the M2 retry releases: errors carry structured metadata (`status_code` / `retry_after` / `request_id`; Rust HTTP variants become struct variants — **breaking**), retry classification is status-based (408/409/429/>=500 plus transport errors), Retry-After honors integer-seconds and HTTP-date capped at 60 s, full jitter replaces the deterministic LCG, `RetryPolicy` gains an `on_retry` observer (and lands in Python as a dataclass threaded through chat and stream), Rust providers share one `send_with_retry` helper, and `specs/retry.md` is the normative cross-SDK retry contract (with one conformance suite per SDK).

Rust 0.24.0 / Python 0.17.0 / TypeScript 0.14.0 are the M3 stream-contract + timeout releases: a stream that ends without the provider's terminal event raises a typed error (Rust `MotosanError::IncompleteStream` — **breaking** enum addition; Python `IncompleteStreamError(StreamError)`; TypeScript `IncompleteStreamError extends StreamError`), retiring the v0.10.1 fabricated-terminal-`done` invariant for the neither-signal case (OpenAI-wire streams complete on `[DONE]` or a `finish_reason` chunk — either suffices; only EOF with neither errors); one timeout model (connect 10 s / read-idle 120 s / total opt-in) lands on all three builders; Rust builds the provider once with a shared `reqwest::Client`; TypeScript gains per-request `AbortSignal` + `CancelledError` (never retried) and a `readTimeoutStream` that actually throws; Python gains `Client.aclose()` / async context manager.

## Current Rust Tool Schema Note

Rust 0.20.0 keeps the 0.18 ToolSchema API and the public `motosan-agent-primitives` dependency at 0.4.0 so downstream bridge crates share the Reviewer-era primitive types. Rust 0.18.0 removes the optional `agent-tool` feature. `types::Tool` now
composes `motosan_agent_primitives::ToolSchema` (also re-exported as
`motosan_ai::ToolSchema`) with `#[serde(flatten)]` and
`Deref<Target = ToolSchema>`; `ChatRequestBuilder::tool_defs` is replaced by
`tool_schemas(&[ToolSchema])`.

## Where To Find Things

| What | Where |
|------|-------|
| Type definitions (source of truth) | `specs/types.md` |
| Rust SDK entry point | `sdks/rust/src/lib.rs` → `client.rs` |
| Python SDK entry point | `sdks/python/motosan_ai/client.py` |
| TypeScript SDK entry point | `sdks/typescript/src/index.ts` → `client.ts` |
| OAuth helper crates | `sdks/rust/crates/motosan-ai-oauth/`, `sdks/rust/crates/codex-oauth/`, `sdks/rust/crates/anthropic-oauth/` |
| Provider implementations | `sdks/rust/src/providers/`, `sdks/python/motosan_ai/providers/`, `sdks/typescript/src/providers/` |
| HTTP providers | Rust: `sdks/rust/src/providers/gemini.rs` (feature `gemini`), `sdks/rust/src/providers/gemini_code_assist.rs` (feature `gemini-code-assist`); Python: `sdks/python/motosan_ai/providers/gemini.py`, `gemini_code_assist.py`, `chatgpt_codex.py`; TypeScript: `sdks/typescript/src/providers/gemini.ts`, `chatgpt_codex.ts` |
| CLI backends | Rust: `sdks/rust/src/providers/claude_code/`, `codex_cli/`, `gemini_cli/`; Python: `sdks/python/motosan_ai/providers/claude_code.py`, `codex_cli.py`, `gemini_cli.py` |
| Rust format/lint config | `sdks/rust/rustfmt.toml`, `sdks/rust/.clippy.toml` |
| Python format/lint config | `sdks/python/ruff.toml` |
| Unified formatter config | `treefmt.toml` |
| CI workflows | `.github/workflows/ci-rust.yml`, `ci-python.yml`, `ci-typescript.yml` |
| Release workflows | `.github/workflows/publish-rust.yml`, `publish-python.yml`, `publish-typescript.yml`, `publish-motosan-ai-oauth.yml`, `publish-codex-oauth.yml`, `publish-anthropic-oauth.yml` |
| Dev shell & scripts | `devshell/default.nix`, `devshell/scripts.nix` |
| API reference for LLMs | `llms.txt` |

## Provider Serialization (READ BEFORE TOUCHING)

Anthropic and OpenAI use completely different wire formats. Mixing them up causes silent failures.

**Anthropic:**
- Tool calls: `content: [{"type":"tool_use","id":...,"name":...,"input":...}]`
- Tool result: `role:"user", content:[{"type":"tool_result","tool_use_id":...,"content":...}]`
- System: top-level `"system"` field — NOT in messages

**OpenAI:**
- Tool calls: `"tool_calls":[{"id":...,"type":"function","function":{"name":...,"arguments":"<JSON string>"}}]`
- `arguments` is a **JSON string**, not an object
- Tool result: `role:"tool", tool_call_id:..., content:...`

**MiniMax (Rust v0.14+):**
- Routed through Anthropic-compatible `/anthropic/v1/messages`
- Uses Anthropic wire format (same shape as above Anthropic section)

## Cross-SDK Consistency

These rules exist because motosan-chat and other downstream consumers depend on a stable interface:

1. `input` (not `args`, not `params`) for tool call payloads
2. `tool_call_id` (snake_case) in Rust/Python
3. `ChatResponse.tool_calls` is always `Vec`/`list` — never optional
4. `Message::tool_result(id, content)` constructor must exist in both SDKs
5. All providers implement: `chat()`, `stream()`, `chat_with()`, `stream_with()`
6. Python `Client` exposes Rust-parity helpers: `chat_with(request)`, `stream_with(request)`, `stream_collect(messages)`, `stream_collect_with(request)`; use `ChatRequest.builder()` with `*_with` for `thinking`, `tool_choice`, `mcp_servers`, `system_blocks`, and `stop_sequences`.
7. Python `Client.chat_sync()` is deprecated in v0.10.0 and should be removed in v0.11.0; use `asyncio.run(client.chat(...))` for sync entry points.

## Adding a New Provider (Rust)

1. Implement `ProviderImpl` in `sdks/rust/src/providers/<name>.rs`
2. Override `capabilities()` if the provider supports image or document content:
   ```rust
   fn capabilities(&self) -> ProviderCapabilities {
       ProviderCapabilities::with_image()  // or full() or text_only() (default)
   }
   ```
   — `text_only()` is the safe default; no override needed for text-only providers.
3. Add a `Provider::<Name>` variant and wire it into `dispatch_chat` / `dispatch_stream_inner` in `client.rs` (same 3-line pattern as existing arms).
4. Gate with `#[cfg(feature = "<name>")]` and add the feature to `Cargo.toml`.
5. Add mock tests in `tests/<name>_provider.rs` and vision tests in `tests/vision_<name>.rs` if the provider supports images.

## Architecture Decisions

**No premature abstraction** — SDKs share design but not code. Rust uses feature flags per provider. Python uses optional deps.

**ThinkStripper** — stateful, applied at `Client.stream()` level. Handles cross-chunk `<think>` tags. Do not strip at the provider level.

**Fallible streams (Rust 0.20+)** — `BoxStream` items are `Result<StreamEvent, MotosanError>`; consumers should `let event = item?` inside `while let Some(item) = stream.next().await` loops. Mid-stream provider/timeout errors surface as `Err`, not sentinel events.

**Stream read timeout** — two layers, both surfacing `Err(MotosanError::StreamReadTimeout(_))`: (1) the Client-level `ReadTimeoutStream` wrapper in `dispatch_stream()` (set via `Client::builder().stream_read_timeout_secs(_)`), wrapping any provider's BoxStream; (2) per-CLI-provider, a per-line read-stall deadline inside each CLI `drive_lines()` loop, set via the provider's `.timeout(dur)` / `.no_timeout()` (default Claude 300 s, Codex/Gemini 600 s).

**CLI provider capabilities (Rust 0.20+)** — `ClaudeCodeProvider` / `CodexCliProvider` / `GeminiCliProvider` share, beyond their per-CLI flags: `.cwd(dir)` (`Command::current_dir`; Codex uses `.cd()` → `--cd`); `.env(k,v)` / `.envs(iter)` per-run env injection (redacted from `Debug` via `RedactedEnvs` — never log env values); `.timeout(dur)` / `.no_timeout()`; `.resume(id)` session continuity (Codex `exec resume`, Gemini/Claude `--resume`) with the minted id surfaced on `StreamEvent::session_id` / `ChatResponse::session_id`. `stream()` surfaces CLI tool use as `ToolCallStart → ToolCallArgs → ToolCallEnd`; blocking `chat().tool_calls` stays empty.

## Releasing

Tag `rust-vX.Y.Z` triggers `publish-rust.yml` → crates.io. Tag `python-vX.Y.Z` triggers `publish-python.yml` → PyPI. Tag `ts-vX.Y.Z` triggers `publish-typescript.yml` → npm. OAuth helper crates use per-crate tags (`motosan-ai-oauth-vX.Y.Z`, `codex-oauth-vX.Y.Z`, `anthropic-oauth-vX.Y.Z`). Publish `motosan-ai-oauth` before wrapper crates that depend on its new version.

Update before tagging: CHANGELOGs, version in `Cargo.toml`/`pyproject.toml`, `AGENTS.md`, `llms.txt`, `skills/motosan-ai/SKILL.md`.
