# Worker Report: Issue #157

## Summary
Implemented `ClaudeCodeClient::chat()` — the blocking subprocess path that invokes the `claude` CLI via `tokio::process::Command`.

## Changes

### `sdks/rust/Cargo.toml`
- Added `process` and `io-util` features to the tokio dependency (needed for subprocess spawning and stdin writing).

### `sdks/rust/src/claude_code/spawn.rs`
- `SpawnConfig` struct: holds binary path, agent mode flag, model, and system prompt.
- `invoke_cli()` async function: spawns `claude --print`, passes prompt via stdin, handles 300s timeout, parses agent-mode JSON output for result/usage.

### `sdks/rust/src/claude_code/prompt.rs` (NEW)
- `messages_to_prompt()`: flattens multi-turn `Message` slice into `(Option<system_prompt>, user_prompt)` for the CLI.
- 4 unit tests covering single message, multi-turn, system extraction, and empty input.

### `sdks/rust/src/claude_code/mod.rs`
- Added `pub mod prompt;` declaration.
- Implemented `chat()` method on `ClaudeCodeClient`: extracts system prompt, flattens messages, builds `SpawnConfig`, calls `invoke_cli`, returns `ChatResponse`.
- Added `#[ignore]` integration test for manual CLI verification.

## Verification
- `cargo fmt` — clean
- `cargo check --features claude-code` — compiles (no new warnings)
- `cargo clippy --features claude-code` — no new warnings (pre-existing `client.rs` warnings unchanged)
- `cargo test --features claude-code` — 25 passed, 0 failed, 1 ignored
