# CLAUDE.md

## Commands

```bash
fmt           # Format all (Rust + Python + TOML + Nix)
check-all     # Full CI gate: lint + test both SDKs
check-rust    # fmt → clippy → test --all-features
check-python  # ruff → format check → pytest
test-live     # Anthropic integration tests (auto-resolves API key)
```

`nix develop` provides a reproducible environment (direnv auto-activates on `cd`). Not required — `cargo` and `uv` work standalone too.

## Rules That Prevent Mistakes

HTTP provider logic goes in `providers/` only — never in `client.rs`/`client.py`. CLI backends that shell out to a local binary (`claude_code/`, `codex_cli/` in Rust) live as top-level modules outside `providers/` because they have subprocess lifetime, JSONL parsing, and exit-code semantics — not request/response serialization.

Tool call field is `input`, not `args` or `params`. Everywhere, both SDKs.

`ChatResponse.tool_calls` is always a list/Vec — never optional, never nullable.

Anthropic and OpenAI serialize tool calls differently. Read `@specs/types.md` and the provider files before touching serialization. Mixing them up is the #1 source of bugs.

Anthropic system prompt goes in top-level `"system"` field. OpenAI/MiniMax system prompt goes in messages array as `role: system`. Getting this wrong = silent failures.

Anthropic `tool_call_id` only appears in `content_block_start`, never in deltas. The stream adapter must track `current_tool_id` in state.

## What Not To Do

- No sync wrappers in Python — callers use `asyncio.run()`
- No FFI or shared code between SDKs — each language is idiomatic
- No provider logic outside `providers/`
- No breaking the `LlmClient` Protocol (motosan-chat depends on it)

## Release Checklist

See `@llms.txt` § Release for the full process. Files to update: `Cargo.toml`/`pyproject.toml`, `CHANGELOG.md`, `AGENTS.md`, `llms.txt`, `skills/motosan-ai/SKILL.md`.
