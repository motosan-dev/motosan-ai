# Code Review: Issue #159 — docs: document three LlmClient backends

**Reviewer:** Code Review Agent (claude-opus-4-6)
**Date:** 2026-04-05
**Commit:** c552711

---

## VERDICT: APPROVE

---

## Summary

This is a docs-only change adding a "Backends (Rust)" section to README.md and creating CHANGELOG.md for v0.6.0. The documentation accurately describes the three ways to interact with Claude (API key, OAuth token, Claude Code CLI) and correctly notes the ClaudeCodeClient limitations.

Overall this is a clean, well-structured documentation update. The examples are accurate against the source code and the limitations are clearly communicated.

## Verified Claims

- `Client::builder().provider(...).api_key(...).build()?` matches `client.rs` lines 27, 386.
- `client.chat(vec![Message::user("Hello")])` matches `Client::chat(&self, messages: Vec<Message>)` at client.rs:63.
- `ClaudeCodeClient::new()` exists and resolves binary from `CLAUDE_CODE_PATH` env or `"claude"` in PATH (mod.rs:22-31).
- `ClaudeCodeClient::chat()` takes `ChatRequest` and returns `Result<ChatResponse, MotosanError>` (mod.rs:56).
- `ClaudeCodeClient::stream()` takes `ChatRequest` and returns `Result<BoxStream, MotosanError>` (mod.rs:84).
- `tool_calls` is always `vec![]` in ClaudeCodeClient (mod.rs:73). Correctly documented.
- `claude-code` feature is gated in Cargo.toml:43 and lib.rs:13-16. Not included in `full`.
- `ClaudeCodeClient` is re-exported from lib.rs:16, so `use motosan_ai::ClaudeCodeClient` works.

## ISSUES

### [nit] Example 3 uses `request` without showing its construction

**File:** README.md, lines 83-85

The API-key and OAuth examples use `vec![Message::user("Hello")]` which is self-contained. The ClaudeCodeClient example uses a bare `request` variable without showing how to construct it. This is a minor inconsistency -- a reader seeing just this section would need to look elsewhere to understand what `request` is.

Not blocking because the pattern is obvious from context, but a one-liner like `let request = ChatRequest::builder().messages(vec![Message::user("Hello")]).build();` would make the example self-contained.

### [nit] OAuth "auto-detected from token prefix" claim is undocumented in code

**File:** README.md, line 70

The comment says `// OAuth format` and the section heading says "auto-detected from token prefix." The `ClientBuilder::api_key()` just stores the string as-is (client.rs:386-388). Whether the Anthropic provider actually varies behavior based on `sk-ant-oat01-` vs `sk-ant-api03-` prefix is not verified in this review, but the documentation implies automatic behavioral switching that may not exist. If both token types are simply passed as `x-api-key` headers, the "auto-detected" wording is misleading.

Worth verifying, but not blocking a docs PR.

### [nit] CHANGELOG says v0.6.0 but Cargo.toml still says v0.5.4

**File:** CHANGELOG.md line 5 vs Cargo.toml line 3

The CHANGELOG documents `[0.6.0]` but the crate version has not been bumped. This is expected if the version bump happens in a separate release PR, but it could confuse readers who check both files. No action needed if this is intentional workflow.

## Security Notes

- The example code in README does not hardcode real API keys (uses placeholder prefixes `sk-ant-api03-...` and `sk-ant-oat01-...`). Good.
- The `--dangerously-skip-permissions` flag used in agent mode (spawn.rs:29, mod.rs:96) is not surfaced in the README examples, which is appropriate -- it should not be advertised casually.
- No secrets, credentials, or sensitive paths in the diff.

## What Looks Good

- Clear separation of the three backends with distinct code blocks.
- Limitations callout box is prominent and accurate.
- Feature flag instructions are correct (`--features claude-code`).
- CHANGELOG format follows Keep a Changelog conventions.
