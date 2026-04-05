# Worker Report: Issue #156

## What was done
Added `claude-code` feature gate and `ClaudeCodeClient` skeleton struct to the Rust SDK.

## Files changed
- `sdks/rust/Cargo.toml` — added `claude-code = ["dep:tokio", "dep:tokio-stream"]` feature
- `sdks/rust/src/lib.rs` — added conditional compilation for `claude_code` module and `ClaudeCodeClient` re-export
- `sdks/rust/src/claude_code/mod.rs` — `ClaudeCodeClient` struct with `new()`, `with_path()`, `agent_mode()`, `model()` builder methods
- `sdks/rust/src/claude_code/spawn.rs` — empty placeholder for subprocess helpers
- `sdks/rust/src/claude_code/stream_json.rs` — empty placeholder for NDJSON event types

## Test results
- `cargo check --features claude-code` — passes (only pre-existing warning in client.rs)
- `cargo check` (no features) — passes, claude_code module not compiled
- `cargo clippy --features claude-code` — no warnings from claude_code module (pre-existing warnings in client.rs from dead_code and needless_return are unrelated)

## Concerns
- Pre-existing clippy warnings in `client.rs` cause `clippy -- -D warnings` to fail regardless of this change. Not addressed as it's out of scope.
