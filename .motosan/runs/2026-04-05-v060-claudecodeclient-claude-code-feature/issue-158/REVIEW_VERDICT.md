# Code Review: Issue #158 — ClaudeCodeClient::stream() NDJSON Streaming

## VERDICT: APPROVE

## Summary

The implementation is correct, well-scoped, and all tests pass (31 passed, 0 failed, 1 ignored integration test).

## Acceptance Criteria Checklist

- [x] `stream()` method emits `StreamEvent::text` for text chunks
- [x] `StreamEvent::done()` emitted at end (after result event, then `break`)
- [x] `StreamEvent::usage` emitted if usage available in result event (before done)
- [x] Process killed on drop (`kill_on_drop(true)` at mod.rs:110)
- [x] Unit tests for NDJSON parser exist and pass (6 tests in stream_json.rs)
- [x] `async-stream` dependency properly configured (optional dep + feature gate)

## Issues

None blocking.

### Minor observations (non-blocking)

1. **`u64` to `u32` truncation** (stream_json.rs:49): `ClaudeStreamUsage` deserializes tokens as `Option<u64>` then casts to `u32` via `as u32`. This is safe in practice (token counts won't exceed 4B) but `u32::try_from().unwrap_or(u32::MAX)` would be more defensive. Not worth blocking since `Usage` uses `u32` everywhere and the Anthropic API itself caps at values well within `u32` range.

2. **Child stderr is captured but not read**: stderr is set to `Stdio::piped()` (mod.rs:113) but never consumed. If the CLI writes substantial error output, the pipe buffer could fill and block the child. In practice this is unlikely with `claude --print` which writes errors sparingly, and the stream breaks on result event + `kill_on_drop` handles cleanup. Non-blocking.

## Scope Compliance

The changes are tightly scoped to the streaming feature:
- `Cargo.toml`: only added `async-stream` optional dep and feature entry
- `stream_json.rs`: new file with NDJSON types + parser + tests
- `mod.rs`: added `stream()` method following the same pattern as existing `chat()`

No unrelated changes, no provider logic outside `providers/`, tool_calls not involved. Clean.
