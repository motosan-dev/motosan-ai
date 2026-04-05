# Worker Report: Issue #158

## Summary
Implemented `ClaudeCodeClient::stream()` method with NDJSON parsing for `--output-format stream-json`.

## Changes

### `sdks/rust/Cargo.toml`
- Added `async-stream = { version = "0.3", optional = true }` dependency
- Added `"dep:async-stream"` to `claude-code` feature list

### `sdks/rust/src/claude_code/stream_json.rs` (new)
- `ClaudeStreamEvent` — tagged enum deserializing `text` and `result` NDJSON events
- `ClaudeStreamUsage` — usage fields from result events
- `NdjsonAction` — parsed action enum (Text or Result)
- `parse_ndjson_line()` — parses a single NDJSON line into `NdjsonAction`
- 6 unit tests: text event, result with/without usage, unknown events, malformed JSON, empty text

### `sdks/rust/src/claude_code/mod.rs`
- Added `stream()` method to `ClaudeCodeClient`
- Spawns `claude --print --output-format stream-json` with optional flags
- Reads stdout line-by-line via `BufReader`, parses each line with `parse_ndjson_line`
- Yields `StreamEvent::text`, `StreamEvent::usage`, and `StreamEvent::done` events
- Returns `Result<BoxStream, MotosanError>`

## Verification
- `cargo fmt` — clean
- `cargo check --features claude-code` — compiles (only pre-existing warning)
- `cargo test --features claude-code` — 31 passed, 0 failed, 1 ignored (integration)
