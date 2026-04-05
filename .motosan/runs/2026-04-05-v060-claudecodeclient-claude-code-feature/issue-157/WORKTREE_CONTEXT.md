# Worktree Context
- **Issue**: #157
- **Title**: feat: ClaudeCodeClient::chat() — blocking subprocess path
- **Repo**: motosan-dev/motosan-ai
- **Branch**: feat/issue-157
- **Worktree**: /tmp/motosan-dev-motosan-ai-issue-157
- **Project Path**: /Users/wadeniubi/Projects/wade/motosan-ai
- **Test Command**: cd sdks/rust && cargo test --features claude-code
- **Format Command**: cd sdks/rust && cargo fmt
- **Lint Command**: cd sdks/rust && cargo clippy --features claude-code -- -D warnings
- **Skip CI**: false
- **Has CLAUDE.md**: true
- **Has AGENTS.md**: true

## IMPORTANT: Codebase Type Mapping

The issue description uses pseudo-code types from a different abstraction. The ACTUAL codebase types are:

| Issue says | Actual type | Location |
|---|---|---|
| `LlmClient` trait | Does NOT exist — `ClaudeCodeClient` is a standalone struct | N/A |
| `ChatOutput` | `ChatResponse` | `src/types.rs` |
| `StreamChunk` | `StreamEvent` | `src/types.rs` |
| `LlmResponse::Message(text)` | `ChatResponse { content: text, tool_calls: vec![], ... }` | `src/types.rs` |
| `TokenUsage` | `Usage` | `src/types.rs` |
| `AgentError` | `MotosanError` | `src/error.rs` |
| `ToolDef` | `Tool` | `src/types.rs` |
| `BoxStream` | `Pin<Box<dyn Stream<Item = StreamEvent> + Send>>` | `src/stream.rs` |

All Rust SDK code is under `sdks/rust/`. Cargo.toml at `sdks/rust/Cargo.toml`.

### Architecture Notes
- `ClaudeCodeClient` already exists in `sdks/rust/src/claude_code/mod.rs` with fields: `binary_path`, `agent_mode`, `model`
- Do NOT implement any trait. Add methods directly on `ClaudeCodeClient`:
  - `pub async fn chat(&self, request: ChatRequest) -> Result<ChatResponse, MotosanError>`
- Use `tokio::process::Command` for subprocess spawning
- `Usage` has fields: `input_tokens: u32, output_tokens: u32, cache_creation_input_tokens: Option<u32>, cache_read_input_tokens: Option<u32>`
- `ChatResponse` has fields: `content, thinking, tool_calls, model, usage, stop_reason`
- `StopReason::EndTurn` for normal completion
- The `claude-code` feature already has `dep:tokio` and `dep:tokio-stream`
- You may need to add `dep:serde_json` usage (it's already a non-optional dep)
- You'll need `tokio` with `process` and `io-util` features — update Cargo.toml to add those features to the tokio dependency for the claude-code feature
