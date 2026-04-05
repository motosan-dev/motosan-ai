# Worktree Context
- **Issue**: #158
- **Title**: feat: ClaudeCodeClient::chat_stream() — NDJSON streaming via --output-format stream-json
- **Repo**: motosan-dev/motosan-ai
- **Branch**: feat/issue-158
- **Worktree**: /tmp/motosan-dev-motosan-ai-issue-158
- **Project Path**: /Users/wadeniubi/Projects/wade/motosan-ai
- **Test Command**: cd sdks/rust && cargo test --features claude-code
- **Format Command**: cd sdks/rust && cargo fmt
- **Lint Command**: cd sdks/rust && cargo clippy --features claude-code -- -D warnings
- **Skip CI**: false
- **Has CLAUDE.md**: true
- **Has AGENTS.md**: true

## IMPORTANT: Codebase Type Mapping

The issue description uses pseudo-code types. Map them to actual types:

| Issue says | Actual type | Location |
|---|---|---|
| `StreamChunk::TextDelta(text)` | `StreamEvent::text(text)` | `src/types.rs` |
| `StreamChunk::Done(...)` | `StreamEvent::done()` | `src/types.rs` |
| `StreamChunk::Usage(...)` | `StreamEvent::usage(usage)` | `src/types.rs` |
| `TokenUsage` | `Usage { input_tokens: u32, output_tokens: u32, cache_creation_input_tokens: Option<u32>, cache_read_input_tokens: Option<u32> }` | `src/types.rs` |
| `AgentError` | `MotosanError` | `src/error.rs` |
| `BoxStream` | `Pin<Box<dyn Stream<Item = StreamEvent> + Send>>` defined as `pub type BoxStream` | `src/stream.rs` |

All Rust SDK code is under `sdks/rust/`.

### Current State
- `ClaudeCodeClient` exists in `src/claude_code/mod.rs` with `chat()` method
- `spawn.rs` has `SpawnConfig` and `invoke_cli()` for blocking subprocess
- `stream_json.rs` is empty — needs NDJSON event types and parser
- The `claude-code` feature in Cargo.toml has `dep:tokio` and `dep:tokio-stream`
- You'll need to add `dep:async-stream` to Cargo.toml and the feature

### Key: async-stream crate
For producing the stream, use the `async-stream` crate's `stream!` macro. Add it as optional dep:
```toml
async-stream = { version = "0.3", optional = true }
```
And add `"dep:async-stream"` to the claude-code feature list.

### StreamEvent constructors (from src/types.rs)
```rust
StreamEvent::text(content: impl Into<String>) -> Self  // text delta
StreamEvent::done() -> Self                              // stream end marker (done=true)
StreamEvent::usage(usage: Usage) -> Self                 // usage event
```

### BoxStream (from src/stream.rs)
```rust
pub type BoxStream = Pin<Box<dyn Stream<Item = StreamEvent> + Send>>;
```
