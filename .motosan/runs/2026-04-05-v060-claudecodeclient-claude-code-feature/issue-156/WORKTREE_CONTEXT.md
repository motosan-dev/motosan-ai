# Worktree Context
- **Issue**: #156
- **Title**: feat: add claude-code feature gate and ClaudeCodeClient skeleton
- **Repo**: motosan-dev/motosan-ai
- **Branch**: feat/issue-156
- **Worktree**: /tmp/motosan-dev-motosan-ai-issue-156
- **Project Path**: /Users/wadeniubi/Projects/wade/motosan-ai
- **Test Command**: cd sdks/rust && cargo test --all-features
- **Format Command**: cd sdks/rust && cargo fmt
- **Lint Command**: cd sdks/rust && cargo clippy --all-features -- -D warnings
- **Skip CI**: false
- **Has CLAUDE.md**: true
- **Has AGENTS.md**: true

## IMPORTANT: Codebase Type Mapping

The issue description uses pseudo-code types. The ACTUAL codebase types are:

| Issue says | Actual type | Location |
|---|---|---|
| `LlmClient` | `ProviderImpl` trait | `src/providers/mod.rs` |
| `ChatOutput` | `ChatResponse` | `src/types.rs` |
| `StreamChunk` | `StreamEvent` | `src/types.rs` |
| `TokenUsage` | `Usage` | `src/types.rs` |
| `AgentError` | `MotosanError` | `src/error.rs` |
| `LlmResponse::Message(text)` | `ChatResponse { content: text, ... }` | `src/types.rs` |
| `BoxStream` | `Pin<Box<dyn Stream<Item = StreamEvent> + Send>>` | `src/stream.rs` |

All Rust SDK code lives under `sdks/rust/`. The Cargo.toml is at `sdks/rust/Cargo.toml`, source at `sdks/rust/src/`.

`ClaudeCodeClient` should NOT implement `ProviderImpl` (that requires reqwest/HTTP). Instead, create it as a standalone struct in `sdks/rust/src/claude_code/` with its own methods that return the same types (`ChatResponse`, `BoxStream`).

The feature gate should be added to `sdks/rust/Cargo.toml`.
The module should be conditionally compiled in `sdks/rust/src/lib.rs`.
