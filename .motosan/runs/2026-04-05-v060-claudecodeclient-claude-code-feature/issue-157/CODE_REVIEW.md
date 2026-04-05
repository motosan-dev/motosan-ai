# Code Review: ClaudeCodeClient::chat() — blocking subprocess path (#157)

**VERDICT: REQUEST_CHANGES_MINOR**

## Overall Assessment

Clean, well-structured implementation. The separation into `spawn.rs` / `prompt.rs` is good for future streaming work. Error handling is thorough and the timeout mechanism is solid. Four issues below — none are blockers but two are worth fixing before merge.

## Issues

### 1. Zombie process on timeout (Medium)

**File:** `sdks/rust/src/claude_code/spawn.rs:63-70`

When the timeout fires, `tokio::time::timeout` returns `Err`, but the `child` process may still be running. `kill_on_drop(true)` (line 46) only fires when `child` is dropped, which happens here at end-of-scope — so the kill *does* happen. However, `kill_on_drop` sends SIGKILL without waiting for the process to actually exit. The child becomes a zombie until the parent process reaps it.

**Recommendation:** After timeout, explicitly `child.kill()` + `child.wait()` to reap. Or accept the current behavior with a comment explaining the trade-off. This is minor because `wait_with_output` consumes the child on the happy path, and on timeout the zombie is reaped when the tokio runtime collects it.

### 2. `--dangerously-skip-permissions` is gated on `agent_mode` only (Medium — Security)

**File:** `sdks/rust/src/claude_code/spawn.rs:28-31`

The `--dangerously-skip-permissions` flag disables Claude Code's permission system. Tying it to `agent_mode` makes sense for the intended use case (headless/automated), but there's no documentation or guard-rail communicating the security implications to callers. A caller might enable `agent_mode` just for JSON output without realizing they're also bypassing permissions.

**Recommendation:** Either:
- Split the concerns: separate `skip_permissions` from `agent_mode` so callers opt in explicitly, or
- Add a doc comment on `ClaudeCodeClient::agent_mode()` (mod.rs:43) that clearly states this enables `--dangerously-skip-permissions`.

### 3. Empty prompt silently sends empty stdin (Low)

**File:** `sdks/rust/src/claude_code/spawn.rs:56-61` / `prompt.rs:14-16`

`messages_to_prompt(&[])` returns `""`. This empty string is written to stdin and sent to the CLI, which may produce unexpected behavior depending on CLI version. The `chat()` method doesn't validate that the prompt is non-empty.

**Recommendation:** Add an early-return error in `chat()` if `user_prompt.is_empty()`:
```rust
if user_prompt.is_empty() {
    return Err(MotosanError::InvalidRequest("prompt is empty".into()));
}
```

### 4. `pub mod prompt` exposes internal flattening logic (Low — API surface)

**File:** `sdks/rust/src/claude_code/mod.rs:1`

`prompt` is `pub mod` but `spawn` is `mod` (private). `messages_to_prompt` is an internal detail — external callers shouldn't depend on how messages are flattened for the CLI.

**Recommendation:** Change to `mod prompt;` (private) unless there's a reason to expose it.

## Non-Issues (Confirmed OK)

- **stdin handle lifecycle:** `child.stdin.take()` moves the handle into the `if let` block; it's dropped at the end of the block, closing the pipe. Correct.
- **`wait_with_output` after stdin close:** `wait_with_output` reads stdout/stderr to completion then waits for exit. Since stdin is already closed, no deadlock risk. Correct.
- **`String::from_utf8_lossy` for stdout:** Reasonable for CLI text output. Non-UTF8 bytes become replacement characters rather than causing errors.
- **`kill_on_drop(true)`:** Good safety net for all exit paths (panics, early returns).
- **Timeout duration (300s):** Generous but appropriate for an LLM subprocess.
- **Architecture for streaming:** `SpawnConfig` and `invoke_cli` are cleanly separated. Adding a `stream_cli` function later that returns a stream over stdout lines would be straightforward without modifying existing code.

## FIX_INSTRUCTIONS

No critical fixes required. Issues #2 and #3 are the most valuable to address before merge — #2 for security clarity, #3 for robustness. Both are one-line changes.
