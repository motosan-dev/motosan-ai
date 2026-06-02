# AGENTS.md

Multi-provider AI SDK. Rust (`sdks/rust/`) + Python (`sdks/python/`). Independent idiomatic implementations — no shared runtime.

Rust v0.19.0 · Python v0.12.1 (PyPI)

## Current Rust Tool Schema Note

Rust 0.19.0 keeps the 0.18 ToolSchema API and bumps the public `motosan-agent-primitives` dependency to 0.4.0 so downstream bridge crates share the Reviewer-era primitive types. Rust 0.18.0 removes the optional `agent-tool` feature. `types::Tool` now
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
| OAuth helper crates | `sdks/rust/crates/motosan-ai-oauth/`, `sdks/rust/crates/codex-oauth/`, `sdks/rust/crates/anthropic-oauth/` |
| Provider implementations | `sdks/rust/src/providers/`, `sdks/python/motosan_ai/providers/` |
| HTTP providers | Rust: `sdks/rust/src/providers/gemini.rs` (feature `gemini`), `sdks/rust/src/providers/gemini_code_assist.rs` (feature `gemini-code-assist`); Python: `sdks/python/motosan_ai/providers/gemini.py`, `gemini_code_assist.py` |
| CLI backends | Rust: `sdks/rust/src/providers/claude_code/`, `codex_cli/`, `gemini_cli/`; Python: `sdks/python/motosan_ai/providers/claude_code.py`, `codex_cli.py`, `gemini_cli.py` |
| Rust format/lint config | `sdks/rust/rustfmt.toml`, `sdks/rust/.clippy.toml` |
| Python format/lint config | `sdks/python/ruff.toml` |
| Unified formatter config | `treefmt.toml` |
| CI workflows | `.github/workflows/ci-rust.yml`, `ci-python.yml` |
| Release workflows | `.github/workflows/publish-rust.yml`, `publish-python.yml`, `publish-motosan-ai-oauth.yml`, `publish-codex-oauth.yml`, `publish-anthropic-oauth.yml` |
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

**Stream read timeout** — applied in `dispatch_stream()` wrapping the provider's BoxStream. Not inside individual providers.

## Releasing

Tag `rust-vX.Y.Z` triggers `publish-rust.yml` → crates.io. Tag `python-vX.Y.Z` triggers `publish-python.yml` → PyPI. OAuth helper crates use per-crate tags (`motosan-ai-oauth-vX.Y.Z`, `codex-oauth-vX.Y.Z`, `anthropic-oauth-vX.Y.Z`). Publish `motosan-ai-oauth` before wrapper crates that depend on its new version.

Update before tagging: CHANGELOGs, version in `Cargo.toml`/`pyproject.toml`, `AGENTS.md`, `llms.txt`, `skills/motosan-ai/SKILL.md`.
