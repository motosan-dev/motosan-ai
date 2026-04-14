# AGENTS.md

Multi-provider AI SDK. Rust (`sdks/rust/`) + Python (`sdks/python/`). Independent idiomatic implementations — no shared runtime.

Rust v0.7.0 (crates.io) · Python v0.5.0 (PyPI)

## Where To Find Things

| What | Where |
|------|-------|
| Type definitions (source of truth) | `specs/types.md` |
| Rust SDK entry point | `sdks/rust/src/lib.rs` → `client.rs` |
| Python SDK entry point | `sdks/python/motosan_ai/client.py` |
| Provider implementations | `sdks/rust/src/providers/`, `sdks/python/motosan_ai/providers/` |
| CLI backends (Rust only) | `sdks/rust/src/claude_code/` (feature `claude-code`), `sdks/rust/src/codex_cli/` (feature `codex-cli`) |
| Rust format/lint config | `sdks/rust/rustfmt.toml`, `sdks/rust/.clippy.toml` |
| Python format/lint config | `sdks/python/ruff.toml` |
| Unified formatter config | `treefmt.toml` |
| CI workflows | `.github/workflows/ci-rust.yml`, `ci-python.yml` |
| Release workflows | `.github/workflows/publish-rust.yml`, `publish-python.yml` |
| Dev shell & scripts | `devshell/default.nix`, `devshell/scripts.nix` |
| API reference for LLMs | `llms.txt` |

## Provider Serialization (READ BEFORE TOUCHING)

Anthropic and OpenAI use completely different wire formats. Mixing them up causes silent failures.

**Anthropic:**
- Tool calls: `content: [{"type":"tool_use","id":...,"name":...,"input":...}]`
- Tool result: `role:"user", content:[{"type":"tool_result","tool_use_id":...,"content":...}]`
- System: top-level `"system"` field — NOT in messages

**OpenAI / MiniMax:**
- Tool calls: `"tool_calls":[{"id":...,"type":"function","function":{"name":...,"arguments":"<JSON string>"}}]`
- `arguments` is a **JSON string**, not an object
- Tool result: `role:"tool", tool_call_id:..., content:...`

## Cross-SDK Consistency

These rules exist because motosan-chat and other downstream consumers depend on a stable interface:

1. `input` (not `args`, not `params`) for tool call payloads
2. `tool_call_id` (snake_case) in Rust/Python
3. `ChatResponse.tool_calls` is always `Vec`/`list` — never optional
4. `Message::tool_result(id, content)` constructor must exist in both SDKs
5. All providers implement: `chat()`, `stream()`, `chat_with()`, `stream_with()`

## Architecture Decisions

**No premature abstraction** — SDKs share design but not code. Rust uses feature flags per provider. Python uses optional deps.

**ThinkStripper** — stateful, applied at `Client.stream()` level. Handles cross-chunk `<think>` tags. Do not strip at the provider level.

**Stream read timeout** — applied in `dispatch_stream()` wrapping the provider's BoxStream. Not inside individual providers.

## Releasing

Tag `rust-vX.Y.Z` triggers `publish-rust.yml` → crates.io. Tag `python-vX.Y.Z` triggers `publish-python.yml` → PyPI.

Update before tagging: CHANGELOGs, version in `Cargo.toml`/`pyproject.toml`, `AGENTS.md`, `llms.txt`, `skills/motosan-ai/SKILL.md`.
